use serde::Serialize;

use crate::{
    image_ordering::{ImageOrdering, Restoration},
    memetic::SolverConfig,
};

/// Output-to-input maps use the same JSON schema as the CLI's original reports.
#[derive(Debug, Serialize)]
pub struct ScrambleReport {
    pub seed: u64,
    pub ordering: ImageOrdering,
}

#[derive(Debug, Serialize)]
pub struct RestorationReport {
    pub config: SolverConfig,
    pub width: u32,
    pub height: u32,
    pub elapsed_seconds: f64,
    pub restoration: Restoration,
}

#[derive(Debug, Serialize)]
pub struct DemoReport {
    #[serde(flatten)]
    pub run: RestorationReport,
    pub scramble_order: ImageOrdering,
    pub row_adjacency_recovery: f64,
    pub column_adjacency_recovery: f64,
}
