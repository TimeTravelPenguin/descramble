//! Weighted-window seriation using Radiate evolution and inherited local search.
//! See the README for the objective, research references, and algorithm adaptations.

pub mod application;
pub mod cli;
mod error;
#[cfg(feature = "gui")]
pub mod gui;
pub mod image_ordering;
pub mod logging;
pub mod memetic;
pub mod objective;
mod permutation;
pub mod storage;

pub use error::{Error, Result};
pub(crate) use permutation::validate_order;
