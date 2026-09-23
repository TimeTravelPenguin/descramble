//! Weighted-window seriation using Radiate evolution and inherited local search.
//! See the README for the objective, research references, and algorithm adaptations.

pub mod cli;
pub mod image_ordering;
pub mod memetic;
pub mod objective;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("evolution failed: {0}")]
    Evolution(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn validate_order(order: &[usize], count: usize) -> Result<()> {
    let mut seen = vec![false; count];

    if order.len() != count {
        return Err(Error::InvalidInput(
            "permutation has the wrong length".into(),
        ));
    }

    for &item in order {
        if item >= count || seen[item] {
            return Err(Error::InvalidInput("order must be a permutation".into()));
        }

        seen[item] = true;
    }

    Ok(())
}
