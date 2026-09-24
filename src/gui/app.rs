use std::{path::Path, sync::Arc};

use iced::{Task, widget::image::Handle};
use image::RgbaImage;

use crate::{application::DemoRun, memetic::SolverConfig};

const PREVIEW_SIZE: u32 = 256;

pub(super) struct App {
    pub input_path: String,
    pub status: Status,
    pub preview: Option<Preview>,
    config: SolverConfig,
}

impl Default for App {
    fn default() -> Self {
        Self {
            input_path: "images/penguin.jpg".into(),
            status: Status::Ready,
            preview: None,
            config: SolverConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    InputChanged(String),
    RunDemo,
    DemoFinished(Result<Arc<DemoRun>, String>),
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
                self.input_path = path;
                self.status = Status::Ready;
                self.preview = None;
            }
            Message::RunDemo if !self.is_running() => {
                if self.input_path.trim().is_empty() {
                    self.status = Status::Failed("Enter an image path to begin.".into());

                    return Task::none();
                }

                self.status = Status::Running;
                self.preview = None;
                let input_path = self.input_path.clone();
                let config = self.config.clone();

                return Task::perform(
                    async move {
                        // Keep the synchronous solver on one blocking worker: its
                        // scoped random stream is thread-local. This also leaves
                        // Iced's event loop and async executor free to respond.
                        tokio::task::spawn_blocking(move || {
                            tracing::info_span!("gui_demo", input = %input_path).in_scope(|| {
                                let original = crate::storage::read_thumbnail(
                                    Path::new(&input_path),
                                    PREVIEW_SIZE,
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
            Message::InputChanged(_) | Message::RunDemo => {}
        }

        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Message, Status};

    #[test]
    fn running_job_rejects_duplicate_requests_and_input_changes() {
        let mut app = App::default();
        let _task = app.update(Message::RunDemo);
        assert!(app.is_running());
        let _duplicate = app.update(Message::RunDemo);
        let _edit = app.update(Message::InputChanged("different.png".into()));
        assert!(app.is_running());
        assert_eq!(app.input_path, "images/penguin.jpg");
    }

    #[test]
    fn empty_input_is_rejected_and_failure_allows_retry() {
        let mut app = App {
            input_path: " ".into(),
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
