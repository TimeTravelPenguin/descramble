//! Blocking image work and the bridge from Radiate events to Iced messages.

use std::{path::PathBuf, sync::Arc};

use iced::futures::{SinkExt, Stream, channel::mpsc};
use image::RgbaImage;
use radiate::prelude::{Engine, EpochComplete, EventStream, Subscription};
use tokio::{sync::watch, task::JoinHandle};

use crate::{
    Result,
    application::{self, ExperimentRun},
    image_ordering::{self, Axis},
    memetic::{OrderingResult, SolverConfig, with_seeded_search},
    objective::DistanceMatrix,
};

type ExperimentResult = std::result::Result<Arc<ExperimentRun>, String>;

#[derive(Debug)]
pub(super) enum ExperimentEvent {
    Progress(f32),
    Finished(ExperimentResult),
}

pub(super) fn events(
    input: PathBuf,
    preview_size: u32,
    config: SolverConfig,
) -> impl Stream<Item = ExperimentEvent> {
    iced::stream::channel(1, async move |output| {
        let (progress_tx, progress_rx) = watch::channel(0.0);
        let worker = tokio::task::spawn_blocking(move || {
            tracing::info_span!("gui_experiment", input = %input.display()).in_scope(|| {
                let original = crate::storage::read_thumbnail(&input, preview_size)
                    .map_err(|error| format!("{error:#}"))?;

                run_experiment(&original, &config, &progress_tx)
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
        });

        relay_progress(worker, progress_rx, output).await;
    })
}

async fn relay_progress(
    mut worker: JoinHandle<ExperimentResult>,
    mut progress: watch::Receiver<f32>,
    mut output: mpsc::Sender<ExperimentEvent>,
) {
    let mut progress_open = true;

    loop {
        tokio::select! {
            // Prefer completion if both are ready; the GUI sets 100% on success.
            biased;
            result = &mut worker => {
                let result = result.unwrap_or_else(|error| {
                    Err(format!("The image task stopped: {error}"))
                });

                let _ = output.send(ExperimentEvent::Finished(result)).await;
                break;
            }

            changed = progress.changed(), if progress_open => {
                if changed.is_ok() {
                    // Release watch's read lock before awaiting Iced's output.
                    let value = *progress.borrow_and_update();

                    if output.send(ExperimentEvent::Progress(value)).await.is_err() {
                        break;
                    }
                } else {
                    // Progress can close before the worker returns (including on error).
                    // Still await its result; do not spin on a closed receiver.
                    progress_open = false;
                }
            }
        }
    }
}

fn run_experiment(
    original: &RgbaImage,
    config: &SolverConfig,
    progress: &watch::Sender<f32>,
) -> Result<ExperimentRun> {
    application::run_experiment_with(original, config, |image, config| {
        image_ordering::restore_with(image, config, |matrix, config, axis| {
            solve_with_progress(matrix, config, axis, progress)
        })
    })
}

fn solve_with_progress(
    matrix: DistanceMatrix,
    config: &SolverConfig,
    axis: Axis,
    progress: &watch::Sender<f32>,
) -> Result<OrderingResult> {
    let offset = match axis {
        Axis::Rows => 0.0,
        Axis::Columns => 0.5,
    };

    let generations = config.generations;

    with_seeded_search(matrix, config, |search| {
        let _subscription = search.engine().map(|engine| {
            let progress = progress.clone();
            let stream = engine.context().event_stream().clone();
            let subscription = engine.subscribe(move |event: &EpochComplete<Vec<usize>>| {
                // Radiate publishes after incrementing its generation index.
                let fraction = event.index.min(generations) as f32 / generations as f32;
                let _ = progress.send(offset + 0.5 * fraction);

                tracing::trace!(?axis, generation = event.index, "Generation completed");
            });

            ProgressSubscription {
                stream,
                subscription,
            }
        });

        let result = search.run()?;
        // A singleton has no engine and therefore no generation events.
        let _ = progress.send(offset + 0.5);

        Ok(result)
    })
}

struct ProgressSubscription {
    stream: EventStream,
    subscription: Subscription,
}

impl Drop for ProgressSubscription {
    fn drop(&mut self) {
        // Radiate 1.3.1's Subscription has no Drop cleanup. Removing the handler
        // also releases its captured sender and breaks the event-stream Arc cycle.
        self.stream.unsubscribe(self.subscription.id());
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::StreamExt;
    use image::Rgba;

    use super::*;

    fn config() -> SolverConfig {
        SolverConfig {
            population: 6,
            generations: 4,
            seed: u64::MAX,
            ..SolverConfig::default()
        }
    }

    #[test]
    fn observation_preserves_results_and_releases_senders_including_singleton_axes() {
        for (width, height) in [(9, 7), (1, 7), (7, 1), (1, 1)] {
            let image = RgbaImage::from_fn(width, height, |column, row| {
                Rgba([(column * 23) as u8, (row * 31) as u8, 40, 255])
            });

            let expected = application::run_experiment(&image, &config()).unwrap();

            for _ in 0..2 {
                let (progress_tx, progress_rx) = watch::channel(0.0);
                let observed = run_experiment(&image, &config(), &progress_tx).unwrap();

                assert_eq!(observed.scrambled, expected.scrambled);
                assert_eq!(observed.restored, expected.restored);
                assert_eq!(
                    serde_json::to_value(&observed.report.run.restoration).unwrap(),
                    serde_json::to_value(&expected.report.run.restoration).unwrap(),
                );
                assert_eq!(*progress_rx.borrow(), 1.0);
                drop(progress_tx);
                assert!(
                    progress_rx.has_changed().is_err(),
                    "subscription leaked its sender"
                );
            }
        }
    }

    #[test]
    fn epochs_are_one_based_and_each_axis_ends_at_its_half() {
        let config = config();
        let matrix = DistanceMatrix::from_vectors(&[vec![2.0], vec![0.0], vec![1.0]]).unwrap();
        let (progress_tx, progress_rx) = watch::channel(0.0);

        for (axis, expected) in [(Axis::Rows, 0.5), (Axis::Columns, 1.0)] {
            solve_with_progress(matrix.clone(), &config, axis, &progress_tx).unwrap();
            assert_eq!(*progress_rx.borrow(), expected);
        }

        let indices = Arc::new(std::sync::Mutex::new(Vec::new()));

        with_seeded_search(matrix, &config, |search| {
            let engine = search.engine().unwrap();
            let captured = Arc::clone(&indices);
            let _guard = ProgressSubscription {
                stream: engine.context().event_stream().clone(),
                subscription: engine.subscribe(move |event: &EpochComplete<Vec<usize>>| {
                    captured.lock().unwrap().push(event.index);
                }),
            };

            search.run()
        })
        .unwrap();

        assert_eq!(*indices.lock().unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn relay_coalesces_progress_and_waits_for_result_after_progress_closes() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (progress_tx, progress_rx) = watch::channel(0.0);
            let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
            let worker = tokio::spawn(async move {
                progress_tx.send(0.25).unwrap();
                progress_tx.send(0.5).unwrap();
                drop(progress_tx);
                ready_tx.send(()).unwrap();
                finish_rx.await.unwrap();

                Err("expected failure after progress closed".into())
            });

            ready_rx.await.unwrap();

            let (output, mut events) = mpsc::channel(1);
            let relay = tokio::spawn(relay_progress(worker, progress_rx, output));

            assert!(matches!(events.next().await, Some(ExperimentEvent::Progress(0.5))));
            finish_tx.send(()).unwrap();
            assert!(matches!(
                events.next().await,
                Some(ExperimentEvent::Finished(Err(error))) if error == "expected failure after progress closed"
            ));
            assert!(events.next().await.is_none());
            relay.await.unwrap();
        });
    }

    #[test]
    fn panicked_worker_becomes_a_completion_error() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (progress_tx, progress_rx) = watch::channel(0.0);
            let worker = tokio::task::spawn_blocking(move || {
                drop(progress_tx);
                panic!("test worker panic");
            });

            let (output, mut events) = mpsc::channel(1);
            let relay = tokio::spawn(relay_progress(worker, progress_rx, output));

            assert!(matches!(
                events.next().await,
                Some(ExperimentEvent::Finished(Err(error))) if error.contains("The image task stopped:")
            ));
            assert!(events.next().await.is_none());
            relay.await.unwrap();
        });
    }

    #[test]
    fn image_worker_stream_finishes_after_success_and_read_failure() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("image.png");
        RgbaImage::from_pixel(5, 3, Rgba([80, 40, 20, 255]))
            .save(&input)
            .unwrap();

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for (path, succeeds) in [(input, true), (directory.path().join("missing.png"), false)] {
                let events = events(path, 4, config());
                iced::futures::pin_mut!(events);

                let mut previous = 0.0;
                let mut finished = false;

                while let Some(event) = events.next().await {
                    assert!(!finished, "no events should follow completion");

                    match event {
                        ExperimentEvent::Progress(value) => {
                            assert!((previous..=1.0).contains(&value));
                            previous = value;
                        }
                        ExperimentEvent::Finished(result) => {
                            assert_eq!(result.is_ok(), succeeds);

                            if let Ok(result) = result {
                                assert!(result.original.width() <= 4);
                                assert!(result.original.height() <= 4);
                            }

                            finished = true;
                        }
                    }
                }

                assert!(finished);
            }
        });
    }
}
