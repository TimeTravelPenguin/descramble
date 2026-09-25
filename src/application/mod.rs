//! Synchronous, in-memory workflows shared by all interfaces.
//!
//! No file writes, terminal output, global logging setup, or GUI dependencies live
//! here. GUI callers must run these CPU-bound operations on a background worker.

mod report;

use std::time::Instant;

use image::RgbaImage;

use crate::{
    Result,
    image_ordering::{Restoration, apply_order, restore, scramble},
    memetic::SolverConfig,
};
pub use report::{ExperimentReport, RestorationReport, ScrambleReport};

#[derive(Debug)]
pub struct ScrambleRun {
    pub image: RgbaImage,
    pub report: ScrambleReport,
}

#[derive(Debug)]
pub struct RestorationRun {
    pub image: RgbaImage,
    pub report: RestorationReport,
}

#[derive(Debug)]
pub struct ExperimentRun {
    pub original: RgbaImage,
    pub scrambled: RgbaImage,
    pub restored: RgbaImage,
    pub report: ExperimentReport,
}

pub fn run_scramble(image: &RgbaImage, seed: u64) -> Result<ScrambleRun> {
    let (image, ordering) = scramble(image, seed)?;
    tracing::info!(
        width = image.width(),
        height = image.height(),
        seed,
        "Image scrambled"
    );

    Ok(ScrambleRun {
        image,
        report: ScrambleReport { seed, ordering },
    })
}

pub fn run_restoration(image: &RgbaImage, config: &SolverConfig) -> Result<RestorationRun> {
    run_restoration_with(image, config, restore)
}

fn run_restoration_with(
    image: &RgbaImage,
    config: &SolverConfig,
    restore_image: impl FnOnce(&RgbaImage, &SolverConfig) -> Result<Restoration>,
) -> Result<RestorationRun> {
    let span = tracing::info_span!(
        "restoration",
        width = image.width(),
        height = image.height(),
        seed = config.seed
    );

    let _entered = span.enter();
    tracing::info!("Starting row and column ordering");

    let start = Instant::now();
    let restoration = restore_image(image, config)?;
    let restored = apply_order(image, &restoration.ordering())?;
    let elapsed_seconds = start.elapsed().as_secs_f64();

    tracing::info!(
        elapsed_seconds,
        row_cost = restoration.rows.cost,
        column_cost = restoration.columns.cost,
        "Restoration finished"
    );

    Ok(RestorationRun {
        image: restored,
        report: RestorationReport {
            config: config.clone(),
            width: image.width(),
            height: image.height(),
            elapsed_seconds,
            restoration,
        },
    })
}

/// The caller resizes the original before calling this function, if desired.
/// Ground truth is only used for evaluation after the solver has completed.
pub fn run_experiment(original: &RgbaImage, config: &SolverConfig) -> Result<ExperimentRun> {
    run_experiment_with(original, config, restore)
}

/// Reuse scrambling, reconstruction, timing, and evaluation with an observed restoration.
/// The supplied operation sees only the scrambled image, never the ground truth.
pub(crate) fn run_experiment_with(
    original: &RgbaImage,
    config: &SolverConfig,
    restore_image: impl FnOnce(&RgbaImage, &SolverConfig) -> Result<Restoration>,
) -> Result<ExperimentRun> {
    config.validate()?;

    let scrambled = run_scramble(original, config.seed)?;
    let restored = run_restoration_with(&scrambled.image, config, restore_image)?;

    let rows = &restored.report.restoration.rows.order;
    let columns = &restored.report.restoration.columns.order;

    let row_adjacency_recovery = adjacency_recovery(&scrambled.report.ordering.rows, rows);
    let column_adjacency_recovery = adjacency_recovery(&scrambled.report.ordering.columns, columns);

    tracing::info!(
        row_adjacency_recovery,
        column_adjacency_recovery,
        "Experiment evaluated"
    );

    Ok(ExperimentRun {
        original: original.clone(),
        scrambled: scrambled.image,
        restored: restored.image,
        report: ExperimentReport {
            run: restored.report,
            scramble_order: scrambled.report.ordering,
            row_adjacency_recovery,
            column_adjacency_recovery,
        },
    })
}

fn adjacency_recovery(scrambled: &[usize], restored: &[usize]) -> f64 {
    if restored.len() < 2 {
        return 1.0;
    }

    let correct = restored
        .windows(2)
        .filter(|pair| scrambled[pair[0]].abs_diff(scrambled[pair[1]]) == 1)
        .count();

    correct as f64 / (restored.len() - 1) as f64
}
