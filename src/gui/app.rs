use std::{path::PathBuf, sync::Arc};

use iced::{Subscription, Task, keyboard, widget::image::Handle};
use image::RgbaImage;
use tracing::info;

use crate::{
    application::ExperimentRun, gui::controls_state::ValidatedBinding, memetic::SolverConfig,
};

use super::viewer::{self, ImageKind, Viewer};
use super::worker::{self, ExperimentEvent};

const INITIAL_PREVIEW_SIZE: u32 = 256;

pub(super) struct App {
    pub controls: AppControlsState,
    pub status: Status,
    pub preview: Option<Preview>,
    pub progress: f32,
    viewer: Option<Viewer>,
    viewer_open: bool,
    config: SolverConfig,
}

impl Default for App {
    fn default() -> Self {
        Self {
            controls: AppControlsState::default(),
            status: Status::Ready,
            preview: None,
            progress: 0.0,
            viewer: None,
            viewer_open: false,
            config: SolverConfig::default(),
        }
    }
}

pub(super) struct AppControlsState {
    pub input_path: String,
    pub preview_size: ValidatedBinding<u32, String>,
    pub rng_seed: ValidatedBinding<u64, String>,
}

impl Default for AppControlsState {
    fn default() -> Self {
        Self {
            input_path: "images/penguin.jpg".into(),
            preview_size: ValidatedBinding::new(INITIAL_PREVIEW_SIZE, validate_preview_size),
            rng_seed: ValidatedBinding::new(SolverConfig::default().seed, |value| {
                value
                    .parse()
                    .map_err(|_| "Seed must be an unsigned integer".into())
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    InputChanged(String),
    OpenFileDialog,
    FileDialogResult(Option<PathBuf>),
    RunExperiment,
    ExperimentProgress(f32),
    ExperimentFinished(Result<Arc<ExperimentRun>, String>),
    PreviewSizeChanged(String),
    RngSeedChanged(String),
    OpenImage(ImageKind),
    Viewer(viewer::Message),
}

pub(super) enum Status {
    Ready,
    Running,
    Complete,
    Failed(String),
}

pub(super) struct Preview {
    pub original: Handle,
    pub scrambled: Handle,
    pub restored: Handle,
    pub result: Arc<ExperimentRun>,
}

impl Preview {
    fn new(result: Arc<ExperimentRun>) -> Self {
        Self {
            original: image_handle(&result.original),
            scrambled: image_handle(&result.scrambled),
            restored: image_handle(&result.restored),
            result,
        }
    }
}

fn image_handle(image: &RgbaImage) -> Handle {
    Handle::from_rgba(image.width(), image.height(), image.as_raw().clone())
}

impl App {
    pub fn viewer(&self) -> Option<&Viewer> {
        self.viewer.as_ref().filter(|_| self.viewer_open)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if !self.viewer_open {
            return Subscription::none();
        }

        keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            } => Some(Message::Viewer(viewer::Message::Close)),
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Space),
                repeat: false,
                ..
            } => Some(Message::Viewer(viewer::Message::Swap)),
            _ => None,
        })
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, Status::Running)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenImage(image) => {
                if let Some(preview) = &self.preview {
                    if let Some(viewer) = &mut self.viewer {
                        viewer.open(image);
                    } else {
                        self.viewer = Some(Viewer::new(Arc::clone(&preview.result), image));
                    }

                    self.viewer_open = true;

                    if let Some(viewer) = &mut self.viewer {
                        return viewer.prepare().map(Message::Viewer);
                    }
                }
            }

            Message::Viewer(viewer::Message::Close) => self.viewer_open = false,
            Message::Viewer(message) => {
                // Finish background work even when the viewer was closed in the meantime.
                if (self.viewer_open
                    || matches!(
                        message,
                        viewer::Message::ImageAllocated(..) | viewer::Message::SaveFinished(..)
                    ))
                    && let Some(viewer) = &mut self.viewer
                {
                    return viewer.update(message).map(Message::Viewer);
                }
            }

            Message::InputChanged(path) if !self.is_running() => {
                self.controls.input_path = path;
                self.status = Status::Ready;
                self.preview = None;
                self.progress = 0.0;
                self.viewer = None;
                self.viewer_open = false;
            }

            Message::OpenFileDialog if !self.is_running() => {
                info!("Opening file dialog");
                return iced::Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select an image…")
                            .set_directory(
                                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                            )
                            .add_filter("Image files", &["png", "jpg", "jpeg"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::FileDialogResult,
                );
            }

            Message::FileDialogResult(Some(path)) if !self.is_running() => {
                info!(path = %path.display(), "File selected");
                self.controls.input_path = path.to_string_lossy().to_string();
                self.status = Status::Ready;
                self.preview = None;
                self.progress = 0.0;
                self.viewer = None;
                self.viewer_open = false;
            }

            Message::FileDialogResult(None) if !self.is_running() => {
                info!("File selection canceled");
            }

            Message::RunExperiment if !self.is_running() => {
                if self.controls.input_path.trim().is_empty() {
                    self.status = Status::Failed("Enter an image path to begin.".into());

                    return Task::none();
                }

                if !self.controls.preview_size.is_valid() || !self.controls.rng_seed.is_valid() {
                    self.status =
                        Status::Failed("Correct the image size and seed before starting.".into());

                    return Task::none();
                }

                self.status = Status::Running;
                self.preview = None;
                self.progress = 0.0;
                self.viewer = None;
                self.viewer_open = false;

                return Task::run(
                    worker::events(
                        PathBuf::from(&self.controls.input_path),
                        *self.controls.preview_size.get_validated(),
                        self.config.clone(),
                    ),
                    |event| match event {
                        ExperimentEvent::Progress(progress) => {
                            Message::ExperimentProgress(progress)
                        }
                        ExperimentEvent::Finished(result) => Message::ExperimentFinished(result),
                    },
                );
            }

            Message::ExperimentProgress(progress) if self.is_running() => {
                if progress.is_finite() {
                    self.progress = self.progress.max(progress.clamp(0.0, 1.0));
                }
            }

            Message::ExperimentFinished(result) if self.is_running() => match result {
                Ok(result) => {
                    self.preview = Some(Preview::new(result));
                    self.status = Status::Complete;
                    self.progress = 1.0;
                }
                Err(error) => {
                    tracing::error!(%error, "Image experiment failed");
                    self.status = Status::Failed(error);
                }
            },

            Message::PreviewSizeChanged(size) if !self.is_running() => {
                self.controls.preview_size.update(&size).ok();
            }

            Message::RngSeedChanged(seed) if !self.is_running() => {
                if let Ok(seed) = self.controls.rng_seed.update(&seed) {
                    self.config.seed = seed;
                }
            }

            Message::InputChanged(_)
            | Message::RunExperiment
            | Message::ExperimentProgress(_)
            | Message::ExperimentFinished(_)
            | Message::PreviewSizeChanged(_)
            | Message::RngSeedChanged(_)
            | Message::OpenFileDialog
            | Message::FileDialogResult(_) => {}
        }

        Task::none()
    }
}

fn validate_preview_size(value: &str) -> Result<u32, String> {
    let size = value
        .parse::<u32>()
        .map_err(|_| "Preview size must be a positive integer")?;

    if size == 0 {
        return Err("Preview size must be greater than zero".into());
    }

    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::{App, AppControlsState, Message, Status};

    #[test]
    fn running_job_rejects_duplicate_requests_and_input_changes() {
        let mut app = App::default();
        let _task = app.update(Message::RunExperiment);

        assert!(app.is_running());

        let _duplicate = app.update(Message::RunExperiment);
        let _edit = app.update(Message::InputChanged("different.png".into()));
        let _size = app.update(Message::PreviewSizeChanged("100".into()));
        let _seed = app.update(Message::RngSeedChanged("123".into()));

        assert!(app.is_running());
        assert_eq!(app.controls.input_path, "images/penguin.jpg");
        assert_eq!(*app.controls.preview_size.get_validated(), 256);
        assert_eq!(*app.controls.rng_seed.get_validated(), 42);
    }

    #[test]
    fn empty_input_is_rejected_and_failure_allows_retry() {
        let mut app = App {
            controls: AppControlsState {
                input_path: " ".into(),
                ..AppControlsState::default()
            },
            ..App::default()
        };

        let _task = app.update(Message::RunExperiment);
        assert!(matches!(app.status, Status::Failed(_)));

        let _edit = app.update(Message::InputChanged("input.png".into()));
        let _task = app.update(Message::RunExperiment);
        assert!(app.is_running());

        let _completion = app.update(Message::ExperimentFinished(Err(
            "Could not read image".into()
        )));
        assert!(matches!(app.status, Status::Failed(_)));

        let _retry = app.update(Message::RunExperiment);
        assert!(app.is_running());
    }

    #[test]
    fn progress_updates_complete_and_reset_for_another_experiment() {
        let mut app = App::default();
        let _start = app.update(Message::RunExperiment);
        let _progress = app.update(Message::ExperimentProgress(0.25));
        let _older_progress = app.update(Message::ExperimentProgress(0.1));
        assert_eq!(app.progress, 0.25);

        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([20, 40, 60, 255]));
        let result = crate::application::run_experiment(&image, &app.config).unwrap();
        let _completion = app.update(Message::ExperimentFinished(Ok(std::sync::Arc::new(result))));
        let _late_progress = app.update(Message::ExperimentProgress(0.9));
        assert!(matches!(app.status, Status::Complete));
        assert!(app.preview.is_some());
        assert_eq!(app.progress, 1.0);

        let _restart = app.update(Message::RunExperiment);
        assert!(app.is_running());
        assert!(app.preview.is_none());
        assert_eq!(app.progress, 0.0);
    }

    #[test]
    fn invalid_settings_block_start_and_zero_is_a_valid_seed() {
        let mut app = App::default();
        let _invalid_size = app.update(Message::PreviewSizeChanged("0".into()));
        let _start = app.update(Message::RunExperiment);
        assert!(!app.is_running());

        let _size = app.update(Message::PreviewSizeChanged("128".into()));
        let _invalid_seed = app.update(Message::RngSeedChanged("invalid".into()));
        let _start = app.update(Message::RunExperiment);
        assert!(!app.is_running());

        let _zero_seed = app.update(Message::RngSeedChanged("0".into()));
        let _start = app.update(Message::RunExperiment);
        assert!(app.is_running());
        assert_eq!(app.config.seed, 0);
    }

    #[test]
    fn viewer_requires_a_result_and_closes_when_the_experiment_is_replaced() {
        use super::super::viewer::{self, ImageKind};

        let mut app = App::default();
        let _open = app.update(Message::OpenImage(ImageKind::Original));
        assert!(app.viewer().is_none());

        let _start = app.update(Message::RunExperiment);
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([20, 40, 60, 255]));
        let result = crate::application::run_experiment(&image, &app.config).unwrap();
        let _complete = app.update(Message::ExperimentFinished(Ok(std::sync::Arc::new(result))));
        let _open = app.update(Message::OpenImage(ImageKind::Restored));
        assert!(app.viewer().is_some());

        let _close = app.update(Message::Viewer(viewer::Message::Close));
        assert!(app.viewer().is_none());
        assert!(app.viewer.is_some(), "closing preserves alignment state");

        let _open = app.update(Message::OpenImage(ImageKind::Original));
        assert!(app.viewer().is_some());

        let _new_input = app.update(Message::InputChanged("next.png".into()));
        assert!(app.viewer().is_none());
        assert!(app.viewer.is_none());
        assert!(app.preview.is_none());
    }
}
