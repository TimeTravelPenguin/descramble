# Descramble

Reorder independently shuffled image rows and columns using a **permutation memetic algorithm built with Radiate 1.3.1**. The library also orders arbitrary vectors or a supplied symmetric distance matrix.

This implements the weighted-window seriation objective from the Moscato/Cotta/Mendes research discussed below. It is a practical adaptation: nearest-neighbor initialization, PMX crossover, and candidate-based local search replace the historical ordered-tree representation, PDG crossover, and population hierarchy. It does **not** reproduce the 2007 Tabu Search implementation.

## Run

Use release builds for searches:

```sh
# Complete experiment using the included image; resize only before scrambling.
cargo run --release -- demo

# A smaller experiment.
cargo run --release -- demo images/penguin_small.jpg --max-dimension 128 --generations 60

# Independent commands; restore never reads the original or scramble map.
cargo run --release -- scramble images/penguin_small.jpg output/shuffled.png --seed 42
cargo run --release -- restore output/shuffled.png output/restored.png --window 2 --seed 42

cargo run --release -- restore --help
```

`demo` uses `images/penguin.jpg` by default, creates its output directory (default `output/demo`), and writes `original.png`, `scrambled.png`, `restored.png`, and `report.json`. For `scramble` and `restore`, the output parent directory must already exist. Outputs must use `.png`; the JSON map or search report is written beside it with the same stem. Existing output files are replaced after all destinations have been checked and all artifacts encoded into temporary files. Outputs that alias the input or another output are rejected. Each file replacement is atomic, but the group is not a transaction if a filesystem failure or concurrent change occurs during replacement.

Input supports PNG and JPEG, decoded to RGBA8. The solver measures RGB similarity and carries alpha through unchanged. It does not model alpha compositing. Save scrambled images losslessly: JPEG encoding after scrambling changes the data. Do not resize an already scrambled image because interpolation mixes unrelated strips.

Options shared by `demo` and `restore`:

| Option | Default | Meaning |
| --- | --- | --- |
| `--window` | `max(1, floor(n / 100))` per axis | Neighborhood radius, clamped to `n - 1` |
| `--population` | 32 | Number of candidate permutations, minimum 6 |
| `--generations` | 200 | Positive generation limit per axis |
| `--local-passes` | 2 | Improvement passes per candidate; 0 disables local search |
| `--seed` | 1 | Row search seed; column search uses seed + 1, wrapping |

`demo` additionally accepts `--output-dir` and `--max-dimension` (default 256). It reports the fraction of original adjacent pairs recovered, ignoring direction. Ground truth is used only for this final evaluation. The score measures neighbor recovery, not exact pixel reconstruction or orientation.

## Algorithm

For row ordering, each row is a vector of all its RGB components. Columns are treated analogously. Compute ordinary Euclidean distances, **including the square root**, between each pair of vectors. A common permutation of coordinates preserves Euclidean distance, so column scrambling does not change row distances and vice versa. Both axes can therefore be solved independently from the scrambled image.

For permutation `p`, minimize:

```text
F(p) = sum over gap = 1..w:
           (w + 1 - gap) * sum over i = 0..n-gap-1:
               D[p[i], p[i + gap]]
```

This counts each unordered pair once, giving half the published double-counted objective. The optimum is unchanged. There is no last-to-first edge. With `w = 1`, this is a minimum-weight Hamiltonian path objective. Larger windows reward wider coherent neighborhoods.

The implementation proceeds as follows:

1. Include the input ordering, generate nearest-neighbor paths for the rest of the first half of the population, and fill the second half with random permutations. Improve all starting candidates locally.
2. Radiate chooses offspring with tournament selection (size 3). Preserve the best 25% as survivors; use 75% offspring, subject to integer rounding.
3. Apply Radiate PMX crossover (rate 0.8), an inclusive reversal mutation (probability 0.2 per chromosome), then local improvement to the offspring. Reversal chooses a start in `0..n-1` and an inclusive end in `start+1..n`, so every selected mutation reverses at least two items and can include the final item. The minimum population ensures Radiate receives at least four offspring, as its PMX dispatch skips smaller offspring groups.
4. Each local pass considers reversals that bring an item's eight nearest neighbors next to it, then scans adjacent swaps. Accept the first strictly improving moves in deterministic item order. Stop early if a pass accepts nothing. This is a bounded candidate search, not exhaustive descent or Tabu Search.
5. Write the improved permutation back into the chromosome. Radiate invalidates its cached fitness before evaluation. Improvements are inherited by descendants.
6. Stop after the requested generations. Return the best `f64` result retained across initialization and every fitness evaluation, even if Radiate later discards it as an `f32` tie. Canonicalize whole-order reversal by putting the smaller input index at the first endpoint.

Adjacent-swap deltas take `O(w)` distance lookups; reversal deltas take `O(w²)` because internal pairs cancel. Applying a reversal and updating positions takes time proportional to its length. Full objective evaluation is `O(nw)`. Each local pass proposes at most eight reversals per item and `n - 1` swaps. Candidate construction selects the nearest eight in linear time per item, then sorts only those eight. Local search reuses its rollback/position buffers and skips all preparation when disabled. A relative tolerance of `1e-12 * current candidate cost at entry` avoids accepting floating-point noise without suppressing all improvements on small-scale data; a final full-score check rolls back a local-search call if its result became worse.

Distance matrices and local search use `f64`. Euclidean distances use a scaled-norm fallback when squaring would overflow or underflow. Radiate stores fitness as `f32`, so selection can treat very close scores as ties; fitness is scaled by a fixed positive bound on the objective, including for very small distances. The separate `f64` incumbent preserves evaluated improvements, but does not change Radiate's selection precision. The returned cost is recomputed in `f64`. Seeds reproduce searches with this implementation, dependency lockfile, and platform; changes to operators, cross-platform floating-point behavior, and future dependencies can change seeded results. Only distance construction uses parallel computation; random evolutionary operations run on the caller's thread using Radiate's scoped RNG.

## Library

```rust
use descramble::{
    memetic::{SolverConfig, solve},
    objective::DistanceMatrix,
};

fn main() -> descramble::Result<()> {
    let vectors = vec![vec![2.0], vec![0.0], vec![3.0], vec![1.0]];
    let distances = DistanceMatrix::from_vectors(&vectors)?;
    let config = SolverConfig {
        window: Some(1),
        ..SolverConfig::default()
    };

    let result = solve(distances, &config)?;
    // result.order[output_position] is the corresponding input vector index.
    println!("{:?}", result.order);
    Ok(())
}
```

For custom metrics, use `DistanceMatrix::from_dense(count, row_major_values)`. It checks exact symmetry, finite nonnegative values, and a zero diagonal. `WindowObjective::cost` validates the permutation before scoring. `image_ordering::{scramble, restore, apply_order}` expose the image operations.

## Interpretation and limits

A low score is evidence of similar strips being grouped, not proof of the original ordering. Whole-image horizontal and vertical reflections have exactly the same objective. Repeated textures, identical strips, and similar regions can produce additional ambiguities. Larger windows can favor an arrangement other than the original. The solver has no face or image semantics.

For an `H × W` image, distance construction is `O(H²W + W²H)` for fixed RGB channels. Each axis holds a dense `f64` matrix, requiring `8n²` bytes, plus feature vectors, candidates, and local-search data. Axes are processed sequentially. Start with the small demo before running a large image; the search is heuristic and has no guarantee of finding the global optimum.

Source layout:

- `src/objective.rs`: validated distances, weighted objective, exact move deltas.
- `src/memetic.rs`: initialization, Radiate integration, inherited local search.
- `src/image_ordering.rs`: RGB vector extraction, scrambling and reconstruction.
- `src/cli.rs`: command definitions and solver options.
- `src/main.rs`: command execution, JSON reports, and staged output handling.

## Verification

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Tests compare every swap and reversal against full rescoring for every permutation and window at sizes 2 through 6; check the objective fixture; verify cached-fitness invalidation and deterministic seeds; and reconstruct a synthetic image exactly up to axis reflections. They also cover distance invariance, extreme numerical scales, retained full-precision improvements, mutation endpoints, crossover population requirements, alpha preservation, degenerate axes, duplicate strips, invalid inputs, and CLI output failures/aliases. Synthetic recovery is not a guarantee of exact recovery on natural images.

## Research and dependencies

Read these in order for the original algorithm family:

1. [Cotta et al. (2003), Applying Memetic Algorithms to the Analysis of Microarray Data](https://carloscotta.com/papers/evobio03microarray.pdf), especially §2.
2. [Mendes et al. (2005), Gene Ordering in Microarray Data Using Parallel Memetic Algorithms](https://carloscotta.com/papers/icpp05memetic.pdf), especially §§2–3.
3. [Cotta, Langston and Moscato, Combinatorial and Algorithmic Issues for Microarray Analysis](https://carloscotta.com/papers/comb-in-bioinfo.pdf), especially §§4–5.
4. [Moscato, Mendes and Berretta (2007), Benchmarking a memetic algorithm for ordering microarray data](https://doi.org/10.1016/j.biosystems.2006.04.005). Only its abstract was available during this implementation; no exact 2007 reproduction is claimed.

The face demonstration is on PDF pages 25–26 of [Moscato's 2015 lecture](https://carmamaths.org/meetings/mathsandcomputation/pdfs/mathscomp2015-moscato.pdf). See also [Radiate's documentation](https://docs.rs/radiate/1.3.1/radiate/).
