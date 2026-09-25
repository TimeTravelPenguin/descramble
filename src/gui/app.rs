use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use iced::{Task, widget::image::Handle};
use image::RgbaImage;
use num::Integer;
use tracing::info;

use crate::{application::DemoRun, gui::controls_state::ValidatedBinding, memetic::SolverConfig};

const INITIAL_PREVIEW_SIZE: u32 = 256;

pub(super) struct App {
    pub controls: AppControlsState,
    pub status: Status,
    pub preview: Option<Preview>,
    config: SolverConfig,
}

impl Default for App {
    fn default() -> Self {
        Self {
            controls: AppControlsState::default(),
            status: Status::Ready,
            preview: None,
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
            preview_size: ValidatedBinding::new(INITIAL_PREVIEW_SIZE, validate),
            rng_seed: ValidatedBinding::new(42, validate),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    InputChanged(String),
    OpenFileDialog,
    FileDialogResult(Option<PathBuf>),
    RunDemo,
    DemoFinished(Result<Arc<DemoRun>, String>),
    PreviewSizeChanged(String),
    RngSeedChanged(String),
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
    pub result: Arc<DemoRun>,
}

impl Preview {
    fn new(result: Arc<DemoRun>) -> Self {
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
    pub fn is_running(&self) -> bool {
        matches!(self.status, Status::Running)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::InputChanged(path) if !self.is_running() => {
                self.controls.input_path = path;
                self.status = Status::Ready;
                self.preview = None;
            }

            Message::OpenFileDialog if !self.is_running() => {
                info!("Opening file dialog");
                return iced::Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select and image...")
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
            }

            Message::FileDialogResult(None) if !self.is_running() => {
                info!("File selection canceled");
            }

            Message::RunDemo if !self.is_running() => {
                if self.controls.input_path.trim().is_empty() {
                    self.status = Status::Failed("Enter an image path to begin.".into());

                    return Task::none();
                }

                self.status = Status::Running;
                self.preview = None;
                let input_path = self.controls.input_path.clone();
                let config = self.config.clone();
                let preview_size = *self.controls.preview_size.get_validated();

                return Task::perform(
                    async move {
                        // Keep the synchronous solver on one blocking worker: its
                        // scoped random stream is thread-local. This also leaves
                        // Iced's event loop and async executor free to respond.
                        tokio::task::spawn_blocking(move || {
                            tracing::info_span!("gui_demo", input = %input_path).in_scope(|| {
                                let original = crate::storage::read_thumbnail(
                                    Path::new(&input_path),
                                    preview_size,
                                )
                                .map_err(|error| format!("{error:#}"))?;

                                crate::application::run_demo(&original, &config)
                                    .map(Arc::new)
                                    .map_err(|error| error.to_string())
                            })
                        })
                        .await
                        .unwrap_or_else(|error| Err(format!("The image task stopped: {error}")))
                    },
                    Message::DemoFinished,
                );
            }

            Message::DemoFinished(result) => match result {
                Ok(result) => {
                    self.preview = Some(Preview::new(result));
                    self.status = Status::Complete;
                }
                Err(error) => {
                    tracing::error!(%error, "Image demo failed");
                    self.status = Status::Failed(error);
                }
            },

            Message::PreviewSizeChanged(size) => {
                self.controls.preview_size.update(&size).ok();
            }

            Message::RngSeedChanged(seed) => {
                if let Ok(seed) = self.controls.rng_seed.update(&seed) {
                    self.config.seed = seed;
                }
            }

            Message::InputChanged(_)
            | Message::RunDemo
            | Message::OpenFileDialog
            | Message::FileDialogResult(_) => {}
        }

        Task::none()
    }
}

fn validate<T: Integer + FromStr>(value: &str) -> Result<T, String> {
    value
        .parse::<T>()
        .map_err(|_| "Preview size must be a positive integer".into())
        .and_then(|size| {
            if size == T::zero() {
                Err("Preview size must be greater than zero".into())
            } else {
                Ok(size)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{App, AppControlsState, Message, Status};

    #[test]
    fn running_job_rejects_duplicate_requests_and_input_changes() {
        let mut app = App::default();
        let _task = app.update(Message::RunDemo);

        assert!(app.is_running());

        let _duplicate = app.update(Message::RunDemo);
        let _edit = app.update(Message::InputChanged("different.png".into()));

        assert!(app.is_running());
        assert_eq!(app.controls.input_path, "images/penguin.jpg");
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

        let _task = app.update(Message::RunDemo);
        assert!(matches!(app.status, Status::Failed(_)));

        let _edit = app.update(Message::InputChanged("input.png".into()));
        let _task = app.update(Message::RunDemo);
        assert!(app.is_running());

        let _completion = app.update(Message::DemoFinished(Err("Could not read image".into())));
        assert!(matches!(app.status, Status::Failed(_)));

        let _retry = app.update(Message::RunDemo);
        assert!(app.is_running());
    }
}
