use std::sync::Arc;

use radiate::prelude::*;
use serde::Serialize;

use crate::objective::{DistanceMatrix, WindowObjective};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize)]
pub struct SolverConfig {
    /// None selects max(1, floor(number_of_items / 100)).
    pub window: Option<usize>,
    pub population: usize,
    pub generations: usize,
    pub local_passes: usize,
    pub seed: u64,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            window: None,
            population: 32,
            generations: 200,
            local_passes: 2,
            seed: 1,
        }
    }
}

impl SolverConfig {
    pub fn validate(&self) -> Result<()> {
        if self.population < 4 || self.generations == 0 || self.window == Some(0) {
            return Err(Error::InvalidInput(
                "population must be at least 4; generations and any explicit window must be positive".into(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderingResult {
    /// Output position -> input item index.
    pub order: Vec<usize>,
    pub window: usize,
    pub input_cost: f64,
    /// Best starting candidate after initialization and local improvement.
    pub initial_best_cost: f64,
    pub cost: f64,
    pub generations: usize,
}

/// A practical permutation memetic algorithm, not an exact historical-tree reproduction.
/// Random operators run on the caller's thread; distance construction alone is parallel.
pub fn solve(distances: DistanceMatrix, config: &SolverConfig) -> Result<OrderingResult> {
    config.validate()?;
    let count = distances.len();
    let window = config.window.unwrap_or((count / 100).max(1));
    let objective = Arc::new(WindowObjective::new(distances, window)?);

    // Radiate's global seed() does not reset an already initialized thread-local RNG.
    // scoped_seed() resets that actual stream, including for repeated calls on one thread.
    random_provider::scoped_seed(config.seed, || solve_seeded(objective, config))
}

fn solve_seeded(objective: Arc<WindowObjective>, config: &SolverConfig) -> Result<OrderingResult> {
    let count = objective.distances().len();
    let identity: Vec<usize> = (0..count).collect();
    let input_cost = objective.score(&identity);

    if count == 1 {
        return Ok(OrderingResult {
            order: identity,
            window: 0,
            input_cost,
            initial_best_cost: 0.0,
            cost: 0.0,
            generations: 0,
        });
    }

    let local_search = LocalSearch::new(Arc::clone(&objective), config.local_passes);
    let alleles: Arc<[usize]> = identity.clone().into();
    let mut population = Vec::with_capacity(config.population);
    let mut best_order = identity.clone();
    let mut best_cost = input_cost;

    for idx in 0..config.population {
        let mut order = if idx == 0 {
            identity.clone()
        } else if idx < config.population / 2 {
            nearest_neighbor(objective.distances(), random_provider::range(0..count))
        } else {
            random_provider::shuffled_indices(0..count)
        };

        local_search.improve(&mut order);
        let cost = objective.score(&order);

        if cost < best_cost {
            best_cost = cost;
            best_order.clone_from(&order);
        }

        let genes = order
            .iter()
            .map(|&item| PermutationGene::new(item, Arc::clone(&alleles)))
            .collect();
        let chromosome = PermutationChromosome::new(genes, Arc::clone(&alleles));
        population.push(Phenotype::from(Genotype::from(chromosome)));
    }

    let initial_best_cost = best_cost;
    let fitness_objective = Arc::clone(&objective);
    let engine = GeneticEngine::builder()
        .codec(PermutationCodec::new(identity))
        .population_size(config.population)
        .population(population)
        .minimizing()
        .max_age(usize::MAX)
        .offspring_fraction(0.75)
        .offspring_selector(TournamentSelector::new(3))
        .survivor_selector(EliteSelector::new())
        .alter(alters![
            PMXCrossover::new(0.8),
            InversionMutator::new(0.2),
            local_search
        ])
        .fitness_fn(move |order: Vec<usize>| fitness_objective.normalized_score(&order))
        .try_build()
        .map_err(|error| Error::Evolution(error.to_string()))?;

    let result = engine
        .iter()
        .until_generation(config.generations)
        .run()
        .map_err(|error| Error::Evolution(error.to_string()))?;

    // Radiate compares f32 scores. Re-evaluate the final candidates in f64 and
    // retain the initial incumbent so rounding cannot make the returned result worse.
    let codec = PermutationCodec::new((0..count).collect());
    let mut candidates: Vec<Vec<usize>> = result
        .population()
        .iter()
        .map(|phenotype| codec.decode(phenotype.genotype()))
        .collect();

    candidates.push(result.value().clone());

    for order in candidates {
        let cost = objective.cost(&order)?;

        if cost < best_cost {
            best_cost = cost;
            best_order = order;
        }
    }

    // Whole-order reversal is equivalent. This convention only stabilizes output orientation.
    if best_order[0] > best_order[count - 1] {
        best_order.reverse();
    }

    Ok(OrderingResult {
        cost: objective.score(&best_order),
        order: best_order,
        window: objective.window(),
        input_cost,
        initial_best_cost,
        generations: result.index(),
    })
}

fn nearest_neighbor(distances: &DistanceMatrix, start: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(distances.len());
    let mut visited = vec![false; distances.len()];
    let mut current = start;
    order.push(current);
    visited[current] = true;

    while order.len() < distances.len() {
        current = (0..distances.len())
            .filter(|&item| !visited[item])
            .min_by(|&left, &right| {
                distances
                    .get(current, left)
                    .total_cmp(&distances.get(current, right))
                    .then(left.cmp(&right))
            })
            .expect("an unvisited item remains");

        order.push(current);
        visited[current] = true;
    }

    order
}

struct LocalSearch {
    objective: Arc<WindowObjective>,
    neighbors: Vec<Vec<usize>>,
    passes: usize,
}

impl LocalSearch {
    fn new(objective: Arc<WindowObjective>, passes: usize) -> Self {
        let distances = objective.distances();
        let neighbors = (0..distances.len())
            .map(|item| {
                let mut neighbors: Vec<usize> = (0..distances.len())
                    .filter(|&other| other != item)
                    .collect();
                neighbors.sort_unstable_by(|&left, &right| {
                    distances
                        .get(item, left)
                        .total_cmp(&distances.get(item, right))
                        .then(left.cmp(&right))
                });

                neighbors.truncate(8);
                neighbors
            })
            .collect();

        Self {
            objective,
            neighbors,
            passes,
        }
    }

    /// Bounded first-improvement search: candidate reversals then adjacent swaps.
    /// Each item proposes bringing its eight closest items next to itself.
    fn improve(&self, order: &mut [usize]) -> usize {
        let original = order.to_vec();
        let original_cost = self.objective.score(order);
        let tolerance = 1e-12 * original_cost.max(1.0);
        let mut positions = vec![0; order.len()];
        let mut accepted = 0;

        for (position, &item) in order.iter().enumerate() {
            positions[item] = position;
        }

        for _ in 0..self.passes {
            let before = accepted;

            for item in 0..order.len() {
                for &neighbor in &self.neighbors[item] {
                    let position = positions[item];
                    let neighbor_position = positions[neighbor];
                    let (start, end) = if position < neighbor_position {
                        (position + 1, neighbor_position)
                    } else {
                        (neighbor_position, position - 1)
                    };

                    if start < end && self.objective.reversal_delta(order, start, end) < -tolerance
                    {
                        order[start..=end].reverse();

                        for idx in start..=end {
                            positions[order[idx]] = idx;
                        }

                        accepted += 1;
                    }
                }
            }

            for left in 0..order.len() - 1 {
                let right = left + 1;

                if self.objective.swap_delta(order, left, right) < -tolerance {
                    order.swap(left, right);
                    positions[order[left]] = left;
                    positions[order[right]] = right;
                    accepted += 1;
                }
            }

            if accepted == before {
                break;
            }
        }

        // Delta arithmetic can round differently from the full objective.
        if self.objective.score(order) > original_cost {
            order.copy_from_slice(&original);
            return 0;
        }

        accepted
    }
}

impl Mutate<PermutationChromosome<usize>> for LocalSearch {
    fn mutate_chromosome(
        &mut self,
        chromosome: &mut PermutationChromosome<usize>,
        _: &mut AlterContext,
    ) -> usize {
        let mut order: Vec<usize> = chromosome.genes.iter().map(|gene| *gene.allele()).collect();
        let accepted = self.improve(&mut order);

        if accepted > 0 {
            for (gene, item) in chromosome.genes.iter_mut().zip(order) {
                *gene = gene.with_index(item);
            }
        }

        // The default Mutate::mutate invalidates each changed phenotype's cached score.
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_local_search_invalidates_cached_fitness() {
        let vectors: Vec<Vec<f64>> = (0..6).map(|item| vec![item as f64]).collect();
        let objective = Arc::new(
            WindowObjective::new(DistanceMatrix::from_vectors(&vectors).unwrap(), 1).unwrap(),
        );
        let alleles: Arc<[usize]> = (0..6).collect::<Vec<_>>().into();
        let genes = [0, 3, 1, 4, 2, 5]
            .iter()
            .map(|&item| PermutationGene::new(item, Arc::clone(&alleles)))
            .collect();
        let chromosome = PermutationChromosome::new(genes, alleles);
        let mut phenotype = Phenotype::from(Genotype::from(chromosome));
        phenotype.set_score(Some(Score::from(999.0)));
        let mut population = vec![phenotype];
        let mut updates = Default::default();
        let mut context = AlterContext::new(&mut updates, 7, 1.0, &[]);
        let mut search = LocalSearch::new(Arc::clone(&objective), 2);
        assert!(search.mutate(&mut population, &mut context) > 0);
        assert!(population[0].score().is_none());
        assert_eq!(population[0].generation(), 7);
        let codec = PermutationCodec::new((0..6).collect());
        let order = codec.decode(population[0].genotype());
        assert_eq!(objective.cost(&order).unwrap(), 5.0);
    }

    #[test]
    fn repeated_calls_with_same_seed_are_identical() {
        let vectors: Vec<Vec<f64>> = (0..16)
            .map(|item| vec![((item * 17) % 19) as f64, ((item * 7) % 11) as f64])
            .collect();

        let matrix = DistanceMatrix::from_vectors(&vectors).unwrap();
        let config = SolverConfig {
            population: 8,
            generations: 12,
            window: Some(3),
            ..SolverConfig::default()
        };
        let first = solve(matrix.clone(), &config).unwrap();
        let second = solve(matrix.clone(), &config).unwrap();
        assert_eq!(first.order, second.order);
        assert_eq!(first.cost, second.cost);
        assert_eq!(first.generations, 12);
        assert!(first.cost <= first.initial_best_cost + 1e-9);
        assert!(first.initial_best_cost <= first.input_cost + 1e-9);
        assert_eq!(
            first.cost,
            WindowObjective::new(matrix, 3)
                .unwrap()
                .cost(&first.order)
                .unwrap()
        );
    }
}
