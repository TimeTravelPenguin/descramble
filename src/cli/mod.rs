//! Command-line presentation and dispatch; computation lives in `application`.

mod args;

use std::{fs, path::PathBuf};

use color_eyre::Result;

use crate::{application, image_ordering::Restoration, memetic::SolverConfig, storage};
pub use args::{Cli, Command, SolverArgs};

pub fn run(command: Command) -> Result<()> {
    match command {
        Command::Scramble {
            input,
            output,
            seed,
        } => run_scramble(input, output, seed),
        Command::Restore {
            input,
            output,
            solver,
        } => run_restore(input, output, solver),
        Command::Demo {
            input,
            output_dir,
            max_dimension,
            solver,
        } => run_demo(input, output_dir, max_dimension, solver),
    }
}

fn run_demo(
    input: PathBuf,
    output_dir: PathBuf,
    max_dimension: u32,
    solver: SolverArgs,
) -> Result<()> {
    let config = SolverConfig::from(solver);
    config.validate()?;
    fs::create_dir_all(&output_dir)?;

    let original_path = output_dir.join("original.png");
    let scrambled_path = output_dir.join("scrambled.png");
    let restored_path = output_dir.join("restored.png");
    let report_path = output_dir.join("report.json");

    storage::validate_outputs(
        &input,
        &[
            &original_path,
            &scrambled_path,
            &restored_path,
            &report_path,
        ],
    )?;

    let original = storage::read_thumbnail(&input, max_dimension)?;
    let demo = application::run_demo(&original, &config)?;

    print_costs(&demo.report.run.restoration);
    println!(
        "Original neighbor recovery: rows {:.1}%, columns {:.1}%",
        100.0 * demo.report.row_adjacency_recovery,
        100.0 * demo.report.column_adjacency_recovery
    );

    storage::commit_outputs(vec![
        storage::stage_png(&demo.original, &original_path)?,
        storage::stage_png(&demo.scrambled, &scrambled_path)?,
        storage::stage_png(&demo.restored, &restored_path)?,
        storage::stage_json(&demo.report, &report_path)?,
    ])?;

    println!("Saved demo to {}", output_dir.display());
    Ok(())
}

fn run_restore(input: PathBuf, output: PathBuf, solver: SolverArgs) -> Result<()> {
    storage::check_png(&output)?;

    let config = SolverConfig::from(solver);
    config.validate()?;

    let report_path = output.with_extension("json");
    storage::validate_outputs(&input, &[&output, &report_path])?;

    let image = storage::read_image(&input)?;
    let result = application::run_restoration(&image, &config)?;

    print_costs(&result.report.restoration);
    storage::commit_outputs(vec![
        storage::stage_png(&result.image, &output)?,
        storage::stage_json(&result.report, &report_path)?,
    ])?;

    println!("Saved {} and its search report", output.display());
    Ok(())
}

fn run_scramble(input: PathBuf, output: PathBuf, seed: u64) -> Result<()> {
    storage::check_png(&output)?;

    let report_path = output.with_extension("json");
    storage::validate_outputs(&input, &[&output, &report_path])?;

    let image = storage::read_image(&input)?;
    let result = application::run_scramble(&image, seed)?;

    storage::commit_outputs(vec![
        storage::stage_png(&result.image, &output)?,
        storage::stage_json(&result.report, &report_path)?,
    ])?;

    println!("Saved {} and its permutation map", output.display());
    Ok(())
}

fn print_costs(result: &Restoration) {
    for (name, axis) in [("Rows", &result.rows), ("Columns", &result.columns)] {
        println!(
            "{name}: cost {:.3} → {:.3}, window {}, {} generations",
            axis.input_cost, axis.cost, axis.window, axis.generations
        );
    }
}
