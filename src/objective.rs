use rayon::prelude::*;

use crate::{Error, Result, validate_order};

/// A finite, nonnegative, symmetric, row-major matrix with zero diagonal.
#[derive(Debug, Clone)]
pub struct DistanceMatrix {
    count: usize,
    values: Vec<f64>,
}

impl DistanceMatrix {
    pub fn from_dense(count: usize, values: Vec<f64>) -> Result<Self> {
        if count == 0 || count.checked_mul(count) != Some(values.len()) {
            return Err(Error::InvalidInput(
                "distance matrix must be nonempty and square".into(),
            ));
        }

        for row in 0..count {
            for column in 0..count {
                let value = values[row * count + column];

                if !value.is_finite()
                    || value < 0.0
                    || (row == column && value != 0.0)
                    || value != values[column * count + row]
                {
                    return Err(Error::InvalidInput(
                        "distances must be finite, nonnegative, symmetric, with zero diagonal"
                            .into(),
                    ));
                }
            }
        }

        Ok(Self { count, values })
    }

    /// Euclidean distance, including the square root. All vectors must have equal length.
    pub fn from_vectors(vectors: &[Vec<f64>]) -> Result<Self> {
        let count = vectors.len();
        let dimensions = vectors.first().map_or(0, Vec::len);

        if count == 0
            || dimensions == 0
            || vectors.iter().any(|vector| {
                vector.len() != dimensions || vector.iter().any(|value| !value.is_finite())
            })
        {
            return Err(Error::InvalidInput(
                "expected nonempty, equal-length finite vectors".into(),
            ));
        }

        let length = count
            .checked_mul(count)
            .ok_or_else(|| Error::InvalidInput("distance matrix dimensions overflow".into()))?;

        let mut values = vec![0.0; length];
        values
            .par_chunks_mut(count)
            .enumerate()
            .for_each(|(row, distances)| {
                for column in row + 1..count {
                    distances[column] = vectors[row]
                        .iter()
                        .zip(&vectors[column])
                        .map(|(left, right)| (left - right).powi(2))
                        .sum::<f64>()
                        .sqrt();
                }
            });

        for row in 0..count {
            for column in row + 1..count {
                values[column * count + row] = values[row * count + column];
            }
        }

        Self::from_dense(count, values)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, row: usize, column: usize) -> f64 {
        assert!(row < self.count && column < self.count);
        self.values[row * self.count + column]
    }
}

/// Single-count form of the published objective:
/// `sum_{gap=1..w} (w+1-gap) * sum_i D[order[i], order[i+gap]]`.
#[derive(Debug)]
pub struct WindowObjective {
    distances: DistanceMatrix,
    window: usize,
    scale: f64,
}

impl WindowObjective {
    /// Windows larger than the order are clamped; a singleton has effective window zero.
    pub fn new(distances: DistanceMatrix, window: usize) -> Result<Self> {
        if window == 0 {
            return Err(Error::InvalidInput("window must be at least one".into()));
        }

        let window = window.min(distances.len() - 1);
        let max_distance = distances.values.iter().copied().fold(0.0, f64::max);
        let weight_sum: f64 = (1..=window)
            .map(|gap| (window + 1 - gap) as f64 * (distances.len() - gap) as f64)
            .sum();

        let scale = (max_distance * weight_sum).max(1.0);

        if !scale.is_finite() {
            return Err(Error::InvalidInput(
                "objective magnitude exceeds f64 capacity".into(),
            ));
        }

        Ok(Self {
            distances,
            window,
            scale,
        })
    }

    pub fn distances(&self) -> &DistanceMatrix {
        &self.distances
    }

    pub fn window(&self) -> usize {
        self.window
    }

    pub fn cost(&self, order: &[usize]) -> Result<f64> {
        validate_order(order, self.distances.len())?;
        Ok(self.score(order))
    }

    pub(crate) fn normalized_score(&self, order: &[usize]) -> f64 {
        self.score(order) / self.scale
    }

    pub(crate) fn score(&self, order: &[usize]) -> f64 {
        (1..=self.window)
            .map(|gap| {
                let distances: f64 = (0..order.len() - gap)
                    .map(|idx| self.distances.get(order[idx], order[idx + gap]))
                    .sum();

                (self.window + 1 - gap) as f64 * distances
            })
            .sum()
    }

    fn weight(&self, gap: usize) -> f64 {
        if gap == 0 || gap > self.window {
            0.0
        } else {
            (self.window + 1 - gap) as f64
        }
    }

    /// O(w), visiting the union of the two position neighborhoods once.
    pub(crate) fn swap_delta(&self, order: &[usize], left: usize, right: usize) -> f64 {
        let mut delta = 0.0;
        let left_start = left.saturating_sub(self.window);
        let left_end = (left + self.window + 1).min(order.len());
        let right_start = right.saturating_sub(self.window);
        let right_end = (right + self.window + 1).min(order.len());

        for idx in (left_start..left_end).chain(right_start.max(left_end)..right_end) {
            if idx == left || idx == right {
                continue;
            }

            delta += (self.weight(left.abs_diff(idx)) - self.weight(right.abs_diff(idx)))
                * (self.distances.get(order[right], order[idx])
                    - self.distances.get(order[left], order[idx]));
        }

        delta
    }

    /// O(w²): internal and external pairs cancel; only pairs crossing the boundary change.
    pub(crate) fn reversal_delta(&self, order: &[usize], start: usize, end: usize) -> f64 {
        let mut delta = 0.0;

        for outside in start.saturating_sub(self.window)..start {
            for inside in start..=end.min(outside + self.window) {
                delta += self.weight(inside - outside)
                    * (self
                        .distances
                        .get(order[start + end - inside], order[outside])
                        - self.distances.get(order[inside], order[outside]));
            }
        }

        for outside in end + 1..order.len().min(end + self.window + 1) {
            for inside in start.max(outside.saturating_sub(self.window))..=end {
                delta += self.weight(outside - inside)
                    * (self
                        .distances
                        .get(order[start + end - inside], order[outside])
                        - self.distances.get(order[inside], order[outside]));
            }
        }

        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permutations(order: &mut [usize], start: usize, visit: &mut impl FnMut(&[usize])) {
        if start == order.len() {
            visit(order);
            return;
        }

        for idx in start..order.len() {
            order.swap(start, idx);
            permutations(order, start + 1, visit);
            order.swap(start, idx);
        }
    }

    #[test]
    fn all_small_move_deltas_match_full_rescoring() {
        for count in 2..=6 {
            let mut values = vec![0.0; count * count];

            for row in 0..count {
                for column in row + 1..count {
                    let distance = ((row * 31 + column * 17 + row * column * 7) % 41) as f64;
                    values[row * count + column] = distance;
                    values[column * count + row] = distance;
                }
            }

            for window in 1..count {
                let objective = WindowObjective::new(
                    DistanceMatrix::from_dense(count, values.clone()).unwrap(),
                    window,
                )
                .unwrap();

                permutations(&mut (0..count).collect::<Vec<_>>(), 0, &mut |order| {
                    let original = objective.score(order);

                    for left in 0..count {
                        for right in left + 1..count {
                            let mut swapped = order.to_vec();
                            swapped.swap(left, right);
                            assert_eq!(
                                objective.swap_delta(order, left, right),
                                objective.score(&swapped) - original
                            );
                            let mut reversed = order.to_vec();
                            reversed[left..=right].reverse();
                            assert_eq!(
                                objective.reversal_delta(order, left, right),
                                objective.score(&reversed) - original
                            );
                        }
                    }
                });
            }
        }
    }

    #[test]
    fn published_objective_fixture_and_reversal_symmetry() {
        let vectors: Vec<Vec<f64>> = (0..4).map(|item| vec![item as f64]).collect();
        let objective =
            WindowObjective::new(DistanceMatrix::from_vectors(&vectors).unwrap(), 2).unwrap();
        assert_eq!(objective.cost(&[0, 1, 2, 3]).unwrap(), 10.0);
        assert_eq!(objective.cost(&[3, 2, 1, 0]).unwrap(), 10.0);
        assert_eq!(objective.cost(&[0, 2, 1, 3]).unwrap(), 12.0);
        assert!(objective.cost(&[0, 0, 2, 3]).is_err());
        assert!(objective.cost(&[0, 1, 2, 4]).is_err());
        assert!(objective.cost(&[0, 1]).is_err());
    }

    #[test]
    fn invalid_distances_and_windows_are_rejected() {
        assert!(DistanceMatrix::from_dense(0, vec![]).is_err());
        assert!(DistanceMatrix::from_dense(2, vec![0.0, 1.0, 2.0, 0.0]).is_err());
        assert!(DistanceMatrix::from_dense(1, vec![f64::NAN]).is_err());
        assert!(DistanceMatrix::from_vectors(&[vec![1.0], vec![1.0, 2.0]]).is_err());
        assert!(DistanceMatrix::from_vectors(&[vec![f64::INFINITY]]).is_err());
        assert!(
            WindowObjective::new(DistanceMatrix::from_dense(1, vec![0.0]).unwrap(), 0).is_err()
        );
    }
}
