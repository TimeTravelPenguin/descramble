use std::sync::{Arc, Mutex};

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
            seed: 42,
        }
    }
}

impl SolverConfig {
    pub fn validate(&self) -> Result<()> {
        // Radiate 1.3.1 only performs crossover with more than three offspring.
        // With a 75% offspring fraction, six is the smallest working population.
        if self.population < 6 || self.generations == 0 || self.window == Some(0) {
            return Err(Error::InvalidInput(
                "population must be at least 6; generations and any explicit window must be positive".into(),
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
    tracing::debug!(
        items = count,
        window = objective.window(),
        population = config.population,
        generations = config.generations,
        local_passes = config.local_passes,
        seed = config.seed,
        "Starting ordering search"
    );

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

    let mut local_search = LocalSearch::new(Arc::clone(&objective), config.local_passes);
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
    tracing::debug!(initial_best_cost, input_cost, "Initial population prepared");

    let incumbent = Arc::new(Mutex::new(Incumbent {
        order: best_order,
        cost: best_cost,
    }));
    let fitness_incumbent = Arc::clone(&incumbent);
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
            InclusiveInversion,
            local_search
        ])
        .fitness_fn(move |order: Vec<usize>| {
            let cost = fitness_objective.score(&order);
            fitness_incumbent
                .lock()
                .expect("incumbent lock poisoned")
                .consider(&order, cost);
            fitness_objective.normalize(cost)
        })
        .try_build()
        .map_err(|error| Error::Evolution(error.to_string()))?;

    let result = engine
        .iter()
        .until_generation(config.generations)
        .run()
        .map_err(|error| Error::Evolution(error.to_string()))?;

    // Keep every evaluated improvement in f64, even if Radiate's f32 selection
    // treats it as a tie and discards it before the final generation.
    let mut best_order = incumbent
        .lock()
        .expect("incumbent lock poisoned")
        .order
        .clone();

    // Whole-order reversal is equivalent. This convention only stabilizes output orientation.
    if best_order[0] > best_order[count - 1] {
        best_order.reverse();
    }

    tracing::debug!(
        cost = objective.score(&best_order),
        initial_best_cost,
        generations = result.index(),
        "Ordering search finished"
    );

    Ok(OrderingResult {
        cost: objective.score(&best_order),
        order: best_order,
        window: objective.window(),
        input_cost,
        initial_best_cost,
        generations: result.index(),
    })
}

struct Incumbent {
    order: Vec<usize>,
    cost: f64,
}

impl Incumbent {
    fn consider(&mut self, order: &[usize], cost: f64) {
        if cost < self.cost {
            self.order.clear();
            self.order.extend_from_slice(order);
            self.cost = cost;
        }
    }
}

/// Radiate 1.3.1's built-in inversion excludes the final allele from its slice.
/// Use inclusive endpoints and require at least two items so every mutation acts.
struct InclusiveInversion;

impl Mutate<PermutationChromosome<usize>> for InclusiveInversion {
    fn rates(&self) -> RateSet {
        RateSet::new(0.2)
    }

    fn mutate_chromosome(
        &mut self,
        chromosome: &mut PermutationChromosome<usize>,
        ctx: &mut AlterContext,
    ) -> usize {
        let count = chromosome.genes.len();

        if count < 2 || !random_provider::bool(ctx.rate()) {
            return 0;
        }

        let start = random_provider::range(0..count - 1);
        let end = random_provider::range(start + 1..count);
        chromosome.genes[start..=end].reverse();
        1
    }
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
    original: Vec<usize>,
    positions: Vec<usize>,
}

impl LocalSearch {
    fn new(objective: Arc<WindowObjective>, passes: usize) -> Self {
        let distances = objective.distances();
        let count = if passes == 0 { 0 } else { distances.len() };
        let neighbors = (0..count)
            .map(|item| {
                let mut neighbors: Vec<usize> = (0..distances.len())
                    .filter(|&other| other != item)
                    .collect();
                let compare = |left: &usize, right: &usize| {
                    distances
                        .get(item, *left)
                        .total_cmp(&distances.get(item, *right))
                        .then(left.cmp(right))
                };

                if neighbors.len() > 8 {
                    neighbors.select_nth_unstable_by(8, compare);
                    neighbors.truncate(8);
                }

                neighbors.sort_unstable_by(compare);
                neighbors
            })
            .collect();

        Self {
            objective,
            neighbors,
            passes,
            original: Vec::new(),
            positions: Vec::new(),
        }
    }

    /// Bounded first-improvement search: candidate reversals then adjacent swaps.
    /// Each item proposes bringing its eight closest items next to itself.
    fn improve(&mut self, order: &mut [usize]) -> usize {
        if self.passes == 0 {
            return 0;
        }

        let original = &mut self.original;
        original.clear();
        original.extend_from_slice(order);

        let original_cost = self.objective.score(order);
        let tolerance = 1e-12 * original_cost;
        let positions = &mut self.positions;
        positions.resize(order.len(), 0);

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
            order.copy_from_slice(original);
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
        if self.passes == 0 {
            return 0;
        }

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
    fn inversion_includes_final_item_and_changes_two_item_orders() {
        random_provider::scoped_seed(7, || {
            let alleles: Arc<[usize]> = vec![0, 1].into();
            let genes = (0..2)
                .map(|item| PermutationGene::new(item, Arc::clone(&alleles)))
                .collect();
            let mut chromosome = PermutationChromosome::new(genes, alleles);
            let mut updates = Default::default();
            let mut context = AlterContext::new(&mut updates, 1, 1.0, &[]);
            assert_eq!(
                InclusiveInversion.mutate_chromosome(&mut chromosome, &mut context),
                1
            );
            assert_eq!(*chromosome.genes[0].allele(), 1);
            assert_eq!(*chromosome.genes[1].allele(), 0);
        });
    }

    #[test]
    fn every_accepted_population_has_enough_offspring_for_pmx() {
        for population in 0..=5 {
            assert!(
                SolverConfig {
                    population,
                    ..SolverConfig::default()
                }
                .validate()
                .is_err()
            );
        }

        let alleles: Arc<[usize]> = (0..6).collect::<Vec<_>>().into();
        let mut offspring: Vec<_> = (0..4)
            .map(|offset| {
                let genes = (0..6)
                    .map(|item| PermutationGene::new((item + offset) % 6, Arc::clone(&alleles)))
                    .collect();
                Phenotype::from(Genotype::from(PermutationChromosome::new(
                    genes,
                    Arc::clone(&alleles),
                )))
            })
            .collect();

        let mut updates = Default::default();
        let mut context = AlterContext::new(&mut updates, 1, 1.0, &[]);
        let crossed = random_provider::scoped_seed(3, || {
            PMXCrossover::new(1.0).crossover(&mut offspring, &mut context)
        });
        assert!(crossed > 0);
        assert!(offspring.iter().all(Valid::is_valid));
        assert!(
            SolverConfig {
                population: 6,
                ..SolverConfig::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn incumbent_preserves_improvements_that_f32_selection_cannot_distinguish() {
        let mut incumbent = Incumbent {
            order: vec![0, 1, 2],
            cost: 1.0,
        };
        let improved = 1.0 - 1e-10;
        assert_eq!(incumbent.cost as f32, improved as f32);
        incumbent.consider(&[0, 2, 1], improved);
        incumbent.consider(&[1, 0, 2], 1.0);
        assert_eq!(incumbent.order, vec![0, 2, 1]);
        assert_eq!(incumbent.cost, improved);
    }

    #[test]
    fn local_search_improves_small_scale_distances_and_can_be_disabled() {
        let vectors: Vec<Vec<f64>> = (0..6).map(|item| vec![item as f64 * 1e-100]).collect();
        let objective = Arc::new(
            WindowObjective::new(DistanceMatrix::from_vectors(&vectors).unwrap(), 1).unwrap(),
        );
        let mut order = vec![0, 3, 1, 4, 2, 5];
        let mut disabled = LocalSearch::new(Arc::clone(&objective), 0);
        assert_eq!(disabled.improve(&mut order), 0);
        assert!(disabled.neighbors.is_empty());
        assert!(disabled.original.is_empty());
        assert_eq!(order, vec![0, 3, 1, 4, 2, 5]);
        let mut search = LocalSearch::new(Arc::clone(&objective), 2);
        assert!(search.improve(&mut order) > 0);
        assert!((objective.score(&order) / 1e-100 - 5.0).abs() < 1e-12);
    }

    #[test]
    fn candidate_selection_matches_full_sort_including_ties() {
        let vectors: Vec<Vec<f64>> = (0..32).map(|item| vec![(item % 7) as f64]).collect();
        let objective = Arc::new(
            WindowObjective::new(DistanceMatrix::from_vectors(&vectors).unwrap(), 2).unwrap(),
        );
        let search = LocalSearch::new(Arc::clone(&objective), 1);

        for item in 0..vectors.len() {
            let mut expected: Vec<usize> =
                (0..vectors.len()).filter(|&other| other != item).collect();
            expected.sort_unstable_by(|&left, &right| {
                objective
                    .distances()
                    .get(item, left)
                    .total_cmp(&objective.distances().get(item, right))
                    .then(left.cmp(&right))
            });

            expected.truncate(8);
            assert_eq!(search.neighbors[item], expected);
        }
    }

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
