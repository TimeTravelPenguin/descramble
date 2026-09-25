use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::memetic::SolverConfig;

#[derive(Parser)]
#[command(
    version,
    about = "Restore shuffled image rows and columns with memetic seriation"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Shuffle whole rows and columns; write a lossless PNG and a JSON permutation map.
    Scramble {
        /// Input image to shuffle.
        input: PathBuf,

        /// Output image file for the shuffled image.
        output: PathBuf,

        /// Random seed for reproducibility.
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Infer row and column orders from a scrambled image, without an original or map.
    Restore {
        /// Input image to restore.
        input: PathBuf,

        /// Output image file for the restored image.
        output: PathBuf,

        #[command(flatten)]
        solver: SolverArgs,
    },
    /// Resize BEFORE shuffling, then save the original, scrambled, restored, and report.
    Experiment {
        /// Input image to resize and scramble.
        #[arg(default_value = "images/penguin.jpg")]
        input: PathBuf,

        /// Output directory for the original, scrambled, restored images and report.
        #[arg(long, default_value = "output/experiment")]
        output_dir: PathBuf,

        /// Maximum width or height of the resized image.
        #[arg(long, default_value_t = 256, value_parser = clap::value_parser!(u32).range(1..))]
        max_dimension: u32,

        #[command(flatten)]
        solver: SolverArgs,
    },
}

/// Solver configuration arguments for the CLI.
#[derive(Args)]
pub struct SolverArgs {
    /// Neighborhood radius; default is max(1, floor(axis length / 100)).
    #[arg(long)]
    window: Option<usize>,

    /// Population size (at least 6, so crossover has enough offspring).
    #[arg(long, default_value_t = 32)]
    population: usize,

    /// Number of generations.
    #[arg(long, default_value_t = 200)]
    generations: usize,

    /// Local improvement passes per candidate; zero disables local search.
    #[arg(long, default_value_t = 2)]
    local_passes: usize,

    /// Random seed for reproducibility.
    #[arg(long, default_value_t = 1)]
    seed: u64,
}

impl From<SolverArgs> for SolverConfig {
    fn from(args: SolverArgs) -> Self {
        Self {
            window: args.window,
            population: args.population,
            generations: args.generations,
            local_passes: args.local_passes,
            seed: args.seed,
        }
    }
}
