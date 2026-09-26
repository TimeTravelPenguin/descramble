//! Presentation-only alignment and inspection of an experiment's images.

use std::{path::PathBuf, sync::Arc};

use iced::{
    Element, Fill, Task,
    widget::{button, column, container, image::Handle, row, text},
};
use iced_runtime::image::{Allocation, Error as ImageError, allocate};
use image::RgbaImage;

use crate::application::ExperimentRun;

use super::{
    comparison::{self, Comparison, Transform},
    comparison_canvas, image_export, keyboard_control,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageKind {
    Original,
    Shuffled,
    Restored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Original,
    Restored,
    Shuffled,
    Blend,
    Wipe,
    Difference,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Restored => "Result",
            Self::Shuffled => "Shuffled",
            Self::Blend => "Onion skin",
            Self::Wipe => "Before / after",
            Self::Difference => "Difference",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Original => "original.png",
            Self::Restored => "result.png",
            Self::Shuffled => "shuffled.png",
            Self::Blend => "onion-skin.png",
            Self::Wipe => "before-after.png",
            Self::Difference => "difference.png",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Close,
    SelectMode(Mode),
    Swap,
    BlendChanged(f32),
    DividerChanged(f32),
    GainChanged(f32),
    Transform(Transform),
    ResetAlignment,
    Fit,
    ImageAllocated(Handle, Result<Allocation, ImageError>),
    Save,
    SaveFinished(Handle, Result<Option<PathBuf>, String>),
}

/// The frame and its navigation metadata change together, after upload finishes.
struct PresentedImage {
    allocation: Allocation,
    mode: Mode,
    divider: Option<f32>,
    reset_id: u64,
}

struct PendingImage {
    handle: Handle,
    mode: Mode,
    divider: Option<f32>,
    reset_id: u64,
}

pub(super) struct Viewer {
    source: Arc<ExperimentRun>,
    restored: RgbaImage,
    comparison: Comparison,
    original_handle: Handle,
    restored_handle: Handle,
    shuffled_handle: Handle,
    requested: Handle,
    presented: Option<PresentedImage>,
    retained: Vec<Allocation>,
    loading: Option<PendingImage>,
    load_error: Option<String>,
    saving: Option<Handle>,
    save_status: Option<String>,
    mode: Mode,
    opacity: f32,
    divider: f32,
    gain: f32,
    reset_id: u64,
}

impl Viewer {
    pub(super) fn new(source: Arc<ExperimentRun>, image: ImageKind) -> Self {
        let restored = source.restored.clone();
        let comparison = Comparison::new(&source.original, &restored);
        let original_handle = handle(&comparison.original);
        let restored_handle = handle(&comparison.restored);
        let shuffled_handle = handle(&source.scrambled);
        let mut viewer = Self {
            source,
            restored,
            comparison,
            requested: original_handle.clone(),
            presented: None,
            retained: Vec::new(),
            loading: None,
            load_error: None,
            saving: None,
            save_status: None,
            original_handle,
            restored_handle,
            shuffled_handle,
            mode: Mode::Original,
            opacity: 0.5,
            divider: 0.5,
            gain: 1.0,
            reset_id: 0,
        };

        viewer.open(image);

        viewer
    }

    /// Reopening preserves alignment and comparison controls for this experiment.
    pub(super) fn open(&mut self, image: ImageKind) {
        self.mode = match image {
            ImageKind::Original => Mode::Original,
            ImageKind::Shuffled => Mode::Shuffled,
            ImageKind::Restored => Mode::Restored,
        };

        self.refresh_display();
        self.reset_id = self.reset_id.wrapping_add(1);
    }

    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Save => {
                if self.saving.is_some() {
                    return Task::none();
                }

                let Some(presented) = &self.presented else {
                    return Task::none();
                };

                // Freeze the frame the user can actually see. The selected mode
                // or slider value may already be waiting on a different upload.
                let handle = presented.allocation.handle().clone();
                let filename = presented.mode.file_name();
                self.saving = Some(handle.clone());
                self.save_status = None;

                return Task::perform(
                    image_export::save(handle.clone(), filename),
                    move |result| Message::SaveFinished(handle.clone(), result),
                );
            }

            Message::SaveFinished(handle, result) => {
                if self.saving.as_ref() != Some(&handle) {
                    return Task::none();
                }

                self.saving = None;
                self.save_status = Some(match result {
                    Ok(Some(path)) => format!(
                        "Saved {}",
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                    ),
                    Ok(None) => "Save canceled.".into(),
                    Err(error) => format!("Save failed: {error}"),
                });

                return Task::none();
            }

            Message::ImageAllocated(handle, result) => {
                // Handles have unique IDs, so completions from a replaced
                // experiment cannot overwrite this viewer's pending request.
                if self.loading.as_ref().map(|pending| &pending.handle) != Some(&handle) {
                    return Task::none();
                }

                let pending = self.loading.take().expect("checked pending upload");

                match result {
                    Ok(allocation) => {
                        if self.is_base_image(&handle) {
                            self.retained.push(allocation.clone());
                        }

                        if handle == self.requested {
                            self.present(allocation);
                        } else if pending.mode == self.mode && pending.reset_id == self.reset_id {
                            // Keep a continuous drag moving even when upload is
                            // slower than pointer events. The divider must use
                            // this completed frame's value, not the newer input.
                            self.presented = Some(PresentedImage {
                                allocation,
                                mode: pending.mode,
                                divider: pending.divider,
                                reset_id: pending.reset_id,
                            });
                        }
                    }

                    Err(error) => {
                        tracing::warn!(%error, "Could not prepare viewer image");

                        if handle == self.requested {
                            self.load_error = Some(format!("Could not display image: {error}"));
                            return Task::none();
                        }
                    }
                }

                return self.prepare();
            }

            Message::SelectMode(mode) if mode == self.mode && self.load_error.is_none() => {
                return Task::none();
            }

            Message::SelectMode(mode) => self.mode = mode,
            Message::Swap => {
                self.mode = if self.mode == Mode::Original {
                    Mode::Restored
                } else {
                    Mode::Original
                };
            }

            Message::BlendChanged(value) if value.is_finite() => {
                self.opacity = value.clamp(0.0, 1.0);
            }

            Message::DividerChanged(value) if value.is_finite() => {
                self.divider = value.clamp(0.0, 1.0);
            }

            Message::GainChanged(value) if value.is_finite() => {
                self.gain = value.clamp(1.0, 16.0);
            }

            Message::Transform(operation) => {
                self.restored = comparison::transform(&self.restored, operation);
                self.rebuild_comparison();
            }

            Message::ResetAlignment => {
                self.restored.clone_from(&self.source.restored);
                self.rebuild_comparison();
            }

            Message::Fit => {
                self.reset_id = self.reset_id.wrapping_add(1);

                if let Some(presented) = &mut self.presented {
                    presented.reset_id = self.reset_id;
                }

                return Task::none();
            }

            Message::Close
            | Message::BlendChanged(_)
            | Message::DividerChanged(_)
            | Message::GainChanged(_) => return Task::none(),
        }

        self.refresh_display();

        self.prepare()
    }

    fn rebuild_comparison(&mut self) {
        let previous_dimensions = self.comparison.original.dimensions();
        self.comparison = Comparison::new(&self.source.original, &self.restored);

        // The centered original changes only when the comparison bounds change.
        if self.comparison.original.dimensions() != previous_dimensions {
            self.original_handle = handle(&self.comparison.original);
        }

        self.restored_handle = handle(&self.comparison.restored);
        self.reset_id = self.reset_id.wrapping_add(1);

        let original = &self.original_handle;
        let restored = &self.restored_handle;
        let shuffled = &self.shuffled_handle;
        self.retained
            .retain(|allocation| [original, restored, shuffled].contains(&allocation.handle()));
    }

    fn refresh_display(&mut self) {
        // Build derived pixels on input changes, never on redraw, zoom, or pan.
        // Single-image switching reuses the existing image handles.
        self.requested = match self.mode {
            Mode::Original => self.original_handle.clone(),
            Mode::Restored => self.restored_handle.clone(),
            Mode::Shuffled => self.shuffled_handle.clone(),
            Mode::Blend => handle(&self.comparison.blend(self.opacity)),
            Mode::Wipe => handle(&self.comparison.wipe(self.divider)),
            Mode::Difference => handle(&self.comparison.difference(self.gain)),
        };
    }

    fn is_base_image(&self, handle: &Handle) -> bool {
        [
            &self.original_handle,
            &self.restored_handle,
            &self.shuffled_handle,
        ]
        .contains(&handle)
    }

    fn present(&mut self, allocation: Allocation) {
        self.presented = Some(PresentedImage {
            allocation,
            mode: self.mode,
            divider: (self.mode == Mode::Wipe).then_some(self.divider),
            reset_id: self.reset_id,
        });

        self.load_error = None;
    }

    /// Pin uploaded images and publish only complete frames. Keep one upload in
    /// flight; rapid slider changes replace the desired frame instead of queuing
    /// every intermediate image. The current allocation stays alive throughout.
    pub(super) fn prepare(&mut self) -> Task<Message> {
        let ready = self
            .presented
            .as_ref()
            .map(|presented| &presented.allocation)
            .into_iter()
            .chain(self.retained.iter())
            .find(|allocation| allocation.handle() == &self.requested)
            .cloned();

        if let Some(allocation) = ready {
            self.present(allocation);
            return Task::none();
        }

        if self.loading.is_some() {
            return Task::none();
        }

        let handle = self.requested.clone();
        self.loading = Some(PendingImage {
            handle: handle.clone(),
            mode: self.mode,
            divider: (self.mode == Mode::Wipe).then_some(self.divider),
            reset_id: self.reset_id,
        });

        self.load_error = None;

        allocate(handle.clone()).map(move |result| Message::ImageAllocated(handle.clone(), result))
    }

    pub(super) fn view(&self) -> Element<'_, Message> {
        let mut modes = row![].spacing(6);

        for mode in [
            Mode::Original,
            Mode::Restored,
            Mode::Shuffled,
            Mode::Blend,
            Mode::Wipe,
            Mode::Difference,
        ] {
            modes = modes.push(keyboard_control::button(
                button(text(mode.label()).size(14)).style(if self.mode == mode {
                    button::primary
                } else {
                    button::secondary
                }),
                Some(Message::SelectMode(mode)),
            ));
        }

        let controls: Element<'_, Message> = match self.mode {
            Mode::Blend => row![
                text("Original"),
                keyboard_control::slider(0.0..=1.0, self.opacity, 0.01, Message::BlendChanged,),
                text(format!("Result opacity: {:.0}%", self.opacity * 100.0)),
            ]
            .spacing(12)
            .into(),
            Mode::Wipe => row![
                text("Original on left"),
                keyboard_control::slider(0.0..=1.0, self.divider, 0.001, Message::DividerChanged,),
                text("Result on right"),
            ]
            .spacing(12)
            .into(),
            Mode::Difference => row![
                text("Difference gain"),
                keyboard_control::slider(1.0..=16.0, self.gain, 1.0, Message::GainChanged,),
                text(format!(
                    "{:.0}× · black = equal · magenta = missing area",
                    self.gain
                )),
            ]
            .spacing(12)
            .into(),
            _ => text("Swap original and result with Space to inspect the same area.").into(),
        };

        let alignment = row![
            text("Align result:"),
            keyboard_control::button(
                button("Rotate left"),
                Some(Message::Transform(Transform::RotateLeft)),
            ),
            keyboard_control::button(
                button("Rotate right"),
                Some(Message::Transform(Transform::RotateRight)),
            ),
            keyboard_control::button(
                button("Flip horizontal"),
                Some(Message::Transform(Transform::FlipHorizontal)),
            ),
            keyboard_control::button(
                button("Flip vertical"),
                Some(Message::Transform(Transform::FlipVertical)),
            ),
            keyboard_control::button(button("Reset alignment"), Some(Message::ResetAlignment)),
        ]
        .spacing(8)
        .wrap();

        let stats = &self.comparison.stats;
        let equal_fraction = if stats.compared_pixels == 0 {
            1.0
        } else {
            1.0 - stats.different_pixels as f64 / stats.compared_pixels as f64
        };

        let mut info = column![
            text(format!(
                "Original {} × {} · Result {} × {} · {} / {} pixels differ · {:.2}% exact matches (RGBA)",
                self.source.original.width(), self.source.original.height(),
                self.restored.width(), self.restored.height(),
                stats.different_pixels, stats.compared_pixels, 100.0 * equal_fraction,
            )).size(13),
            text("Alignment affects this viewer only. \
                Scroll to zoom; drag to pan. \
                Drag the divider in before / after mode.").size(13),
            text(if self.saving.is_some() {
                "Saving image…"
            } else {
                self.save_status.as_deref().unwrap_or(
                    "Save exports the displayed image as PNG at its current pixel resolution.",
                )
            }).size(13),
        ].spacing(4);

        if stats.size_mismatch {
            info = info.push(
                text(
                    "Different dimensions: images are centered without resizing; \
                    unmatched areas count as differences.",
                )
                .size(13),
            );
        }

        if let Some(error) = &self.load_error {
            info = info.push(text(error).size(13));
        }

        let canvas = if let Some(presented) = &self.presented {
            let size = presented.allocation.size();

            comparison_canvas::view(
                presented.allocation.handle().clone(),
                (size.width, size.height),
                presented.divider,
                presented.reset_id,
                Message::DividerChanged,
            )
        } else {
            container(text("Preparing image…")).center(Fill).into()
        };

        container(
            column![
                row![
                    text(format!("Image viewer · {}", self.mode.label()))
                        .size(24)
                        .width(Fill),
                    keyboard_control::button(
                        button(if self.saving.is_some() {
                            "Saving…"
                        } else {
                            "Save…"
                        }),
                        (self.presented.is_some() && self.saving.is_none())
                            .then_some(Message::Save),
                    ),
                    keyboard_control::button(button("Back (Esc)"), Some(Message::Close)),
                ]
                .spacing(12),
                row![
                    modes.wrap(),
                    keyboard_control::button(button("Swap (Space)"), Some(Message::Swap)),
                    keyboard_control::button(button("Fit"), Some(Message::Fit))
                ]
                .spacing(12),
                container(controls).width(Fill).center_y(28),
                alignment,
                canvas,
                info,
            ]
            .spacing(12)
            .height(Fill),
        )
        .padding(20)
        .width(Fill)
        .height(Fill)
        .into()
    }
}

fn handle(image: &RgbaImage) -> Handle {
    Handle::from_rgba(image.width(), image.height(), image.as_raw().clone())
}

#[cfg(test)]
mod tests {
    use image::{Rgba, imageops};

    use super::*;

    fn fixture() -> Arc<ExperimentRun> {
        let original =
            RgbaImage::from_fn(3, 2, |x, y| Rgba([(x * 50 + y * 10) as u8, 40, 80, 255]));
        let config = crate::memetic::SolverConfig {
            population: 6,
            generations: 1,
            ..crate::memetic::SolverConfig::default()
        };

        let mut run = crate::application::run_experiment(&original, &config).unwrap();
        // Supply a known reflection for testing the viewer independently of search quality.
        run.restored = imageops::flip_horizontal(&original);

        Arc::new(run)
    }

    #[test]
    fn alignment_corrects_reflections_without_changing_source_or_report() {
        let run = fixture();
        let saved_result = run.restored.clone();
        let saved_report = serde_json::to_value(&run.report).unwrap();
        let mut viewer = Viewer::new(Arc::clone(&run), ImageKind::Restored);
        assert_eq!(viewer.comparison.stats.different_pixels, 4);

        let _task = viewer.update(Message::Transform(Transform::FlipHorizontal));
        assert_eq!(viewer.comparison.stats.different_pixels, 0);
        viewer.open(ImageKind::Original);
        let _task = viewer.update(Message::Swap);
        assert_eq!(viewer.mode, Mode::Restored);
        assert_eq!(viewer.restored, run.original);

        let _task = viewer.update(Message::Transform(Transform::RotateRight));
        assert_eq!(viewer.restored.dimensions(), (2, 3));
        assert!(viewer.comparison.stats.size_mismatch);

        let _task = viewer.update(Message::ResetAlignment);
        assert_eq!(viewer.restored, saved_result);
        assert_eq!(run.restored, saved_result);
        assert_eq!(serde_json::to_value(&run.report).unwrap(), saved_report);
    }

    #[test]
    fn modes_show_expected_pixels_and_fit_preserves_comparison_settings() {
        let run = fixture();
        let mut viewer = Viewer::new(Arc::clone(&run), ImageKind::Shuffled);
        assert_eq!(viewer.requested, viewer.shuffled_handle);

        for (mode, message, expected) in [
            (
                Mode::Blend,
                Message::BlendChanged(0.0),
                run.original.clone(),
            ),
            (
                Mode::Blend,
                Message::BlendChanged(1.0),
                run.restored.clone(),
            ),
            (
                Mode::Wipe,
                Message::DividerChanged(0.0),
                run.restored.clone(),
            ),
            (
                Mode::Wipe,
                Message::DividerChanged(1.0),
                run.original.clone(),
            ),
        ] {
            let _task = viewer.update(Message::SelectMode(mode));
            let _task = viewer.update(message);

            let Handle::Rgba {
                width,
                height,
                pixels,
                ..
            } = &viewer.requested
            else {
                panic!("expected decoded pixels");
            };

            assert_eq!((*width, *height), expected.dimensions());
            assert_eq!(pixels.as_ref(), expected.as_raw());
        }

        let displayed = viewer.requested.clone();
        let previous_reset = viewer.reset_id;
        let _task = viewer.update(Message::Fit);
        assert_eq!(viewer.reset_id, previous_reset + 1);
        assert_eq!(viewer.requested, displayed);
        assert_eq!(viewer.mode, Mode::Wipe);
        assert_eq!(viewer.divider, 1.0);
    }

    #[test]
    fn rapid_changes_coalesce_and_stale_uploads_cannot_replace_the_latest_request() {
        let mut viewer = Viewer::new(fixture(), ImageKind::Original);
        let _initial_upload = viewer.prepare();
        let initial = viewer.loading.as_ref().unwrap().handle.clone();
        let _mode = viewer.update(Message::SelectMode(Mode::Blend));
        let _opacity = viewer.update(Message::BlendChanged(0.75));
        let latest = viewer.requested.clone();
        assert_ne!(initial, latest);
        assert_eq!(
            viewer.loading.as_ref().map(|pending| &pending.handle),
            Some(&initial)
        );

        // Even if an obsolete upload fails, the newest request must proceed.
        let _completion = viewer.update(Message::ImageAllocated(
            initial.clone(),
            Err(ImageError::OutOfMemory),
        ));
        assert_eq!(
            viewer.loading.as_ref().map(|pending| &pending.handle),
            Some(&latest)
        );
        assert!(viewer.load_error.is_none());

        let _stale = viewer.update(Message::ImageAllocated(
            initial,
            Err(ImageError::OutOfMemory),
        ));
        assert_eq!(
            viewer.loading.as_ref().map(|pending| &pending.handle),
            Some(&latest)
        );
        assert!(viewer.load_error.is_none());

        // A failed current upload must stop, report the error, and allow retry.
        let _failure = viewer.update(Message::ImageAllocated(
            latest,
            Err(ImageError::OutOfMemory),
        ));
        assert!(viewer.loading.is_none());
        assert!(viewer.load_error.is_some());

        let _retry = viewer.update(Message::SelectMode(Mode::Blend));
        assert_eq!(
            viewer.loading.as_ref().map(|pending| &pending.handle),
            Some(&viewer.requested)
        );
        assert!(viewer.load_error.is_none());
    }

    #[test]
    fn unchanged_mode_and_alignment_bounds_reuse_image_handles() {
        let mut viewer = Viewer::new(fixture(), ImageKind::Original);
        let original = viewer.original_handle.clone();
        let _flip = viewer.update(Message::Transform(Transform::FlipHorizontal));
        assert_eq!(viewer.original_handle, original);

        let _blend = viewer.update(Message::SelectMode(Mode::Blend));
        let blend = viewer.requested.clone();
        let _unchanged = viewer.update(Message::SelectMode(Mode::Blend));
        assert_eq!(viewer.requested, blend);

        let _rotate = viewer.update(Message::Transform(Transform::RotateRight));
        assert_ne!(viewer.original_handle, original);
    }

    #[test]
    fn every_mode_exports_its_snapshot_even_after_controls_change() {
        let run = fixture();
        let directory = tempfile::tempdir().unwrap();
        let blend = RgbaImage::from_fn(3, 2, |_, y| Rgba([(50 + y * 10) as u8, 40, 80, 255]));
        let wipe = RgbaImage::from_fn(3, 2, |x, y| {
            let red = if x == 1 { 50 } else { 0 };

            Rgba([(red + y * 10) as u8, 40, 80, 255])
        });
        let difference = RgbaImage::from_fn(3, 2, |x, _| {
            let error = if x == 1 { 0 } else { 200 };

            Rgba([error, error, error, 255])
        });

        for (mode, expected) in [
            (Mode::Original, run.original.clone()),
            (Mode::Restored, run.restored.clone()),
            (Mode::Shuffled, run.scrambled.clone()),
            (Mode::Blend, blend),
            (Mode::Wipe, wipe),
            (Mode::Difference, difference),
        ] {
            let mut viewer = Viewer::new(Arc::clone(&run), ImageKind::Original);
            let _mode = viewer.update(Message::SelectMode(mode));

            if mode == Mode::Difference {
                let _gain = viewer.update(Message::GainChanged(2.0));
            }

            let snapshot = viewer.requested.clone();
            let _change_mode = viewer.update(Message::SelectMode(Mode::Restored));
            let _change_alignment = viewer.update(Message::Transform(Transform::RotateRight));
            let path = directory.path().join(mode.file_name());
            image_export::write_png(&snapshot, &path).unwrap();
            assert_eq!(image::open(&path).unwrap().to_rgba8(), expected, "{mode:?}");
        }

        let mut viewer = Viewer::new(run, ImageKind::Restored);
        let _flip = viewer.update(Message::Transform(Transform::FlipHorizontal));
        let _rotate = viewer.update(Message::Transform(Transform::RotateRight));
        let path = directory.path().join("aligned.png");
        image_export::write_png(&viewer.requested, &path).unwrap();
        let saved = image::open(&path).unwrap().to_rgba8();
        assert_eq!(saved.dimensions(), (3, 3));
        assert_eq!(saved, viewer.comparison.restored);
    }

    #[test]
    fn save_completion_handles_cancellation_errors_and_stale_requests() {
        let mut viewer = Viewer::new(fixture(), ImageKind::Original);
        let _not_ready = viewer.update(Message::Save);
        assert!(viewer.saving.is_none());

        let snapshot = viewer.requested.clone();
        viewer.saving = Some(snapshot.clone());
        let _duplicate = viewer.update(Message::Save);
        assert_eq!(viewer.saving.as_ref(), Some(&snapshot));

        let unrelated = handle(&RgbaImage::new(1, 1));
        let _stale = viewer.update(Message::SaveFinished(unrelated, Err("old failure".into())));
        assert_eq!(viewer.saving.as_ref(), Some(&snapshot));
        assert!(viewer.save_status.is_none());

        let _cancel = viewer.update(Message::SaveFinished(snapshot.clone(), Ok(None)));
        assert!(viewer.saving.is_none());
        assert_eq!(viewer.save_status.as_deref(), Some("Save canceled."));

        viewer.saving = Some(snapshot.clone());
        let _failure = viewer.update(Message::SaveFinished(
            snapshot.clone(),
            Err("disk full".into()),
        ));
        assert!(viewer.saving.is_none());
        assert_eq!(
            viewer.save_status.as_deref(),
            Some("Save failed: disk full")
        );

        viewer.saving = Some(snapshot.clone());
        let _success = viewer.update(Message::SaveFinished(
            snapshot,
            Ok(Some("image.png".into())),
        ));
        assert!(viewer.saving.is_none());
        assert_eq!(viewer.save_status.as_deref(), Some("Saved image.png"));
    }
}
