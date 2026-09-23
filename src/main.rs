use std::{fs, path::Path, time::Instant};

use clap::Parser;
use color_eyre::{
    Result,
    eyre::{Context, bail},
};
use descramble::{
    cli::{Cli, Command},
    image_ordering::{ImageOrdering, apply_order, restore, scramble},
    memetic::SolverConfig,
};
use image::{ImageFormat, RgbaImage};
use serde_json::json;

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Scramble {
            input,
            output,
            seed,
        } => {
            run_scramble(input, output, seed)?;
        }
        Command::Restore {
            input,
            output,
            solver,
        } => {
            run_restore(input, output, solver)?;
        }
        Command::Demo {
            input,
            output_dir,
            max_dimension,
            solver,
        } => {
            run_demo(input, output_dir, max_dimension, solver)?;
        }
    }

    Ok(())
}

fn run_demo(
    input: std::path::PathBuf,
    output_dir: std::path::PathBuf,
    max_dimension: u32,
    solver: descramble::cli::SolverArgs,
) -> Result<(), color_eyre::eyre::Error> {
    let config = SolverConfig::from(solver);
    config.validate()?;

    let original = image::open(&input)
        .with_context(|| format!("Could not read {}", input.display()))?
        .thumbnail(max_dimension, max_dimension)
        .to_rgba8();

    let (scrambled, scramble_order) = scramble(&original, config.seed)?;
    fs::create_dir_all(&output_dir)?;
    save_png(&original, &output_dir.join("original.png"))?;
    save_png(&scrambled, &output_dir.join("scrambled.png"))?;

    let start = Instant::now();
    eprintln!(
        "Ordering {} rows and {} columns…",
        scrambled.height(),
        scrambled.width()
    );

    let result = restore(&scrambled, &config)?;
    let restored = apply_order(&scrambled, &result.ordering())?;
    let recovery = adjacency_recovery(&scramble_order, &result.ordering());

    print_costs(&result);
    println!(
        "Original neighbor recovery: rows {:.1}%, columns {:.1}%",
        100.0 * recovery.0,
        100.0 * recovery.1
    );

    save_png(&restored, &output_dir.join("restored.png"))?;
    save_json(
        &output_dir.join("report.json"),
        &json!({
            "config": config,
            "width": original.width(),
            "height": original.height(),
            "elapsed_seconds": start.elapsed().as_secs_f64(),
            "restoration": result,
            "scramble_order": scramble_order,
            "row_adjacency_recovery": recovery.0,
            "column_adjacency_recovery": recovery.1,
        }),
    )?;

    println!("Saved demo to {}", output_dir.display());

    Ok(())
}

fn run_restore(
    input: std::path::PathBuf,
    output: std::path::PathBuf,
    solver: descramble::cli::SolverArgs,
) -> Result<(), color_eyre::eyre::Error> {
    check_png(&output)?;

    let config = SolverConfig::from(solver);
    config.validate()?;

    let image = read_image(&input)?;
    let (restored, report) = run_restoration(&image, &config)?;

    save_png(&restored, &output)?;
    save_json(&output.with_extension("json"), &report)?;

    println!("Saved {} and its search report", output.display());

    Ok(())
}

fn run_scramble(
    input: std::path::PathBuf,
    output: std::path::PathBuf,
    seed: u64,
) -> Result<(), color_eyre::eyre::Error> {
    check_png(&output)?;

    let image = read_image(&input)?;
    let (scrambled, ordering) = scramble(&image, seed)?;

    save_png(&scrambled, &output)?;
    save_json(
        &output.with_extension("json"),
        &json!({ "seed": seed, "ordering": ordering }),
    )?;

    println!("Saved {} and its permutation map", output.display());

    Ok(())
}

fn run_restoration(
    image: &RgbaImage,
    config: &SolverConfig,
) -> Result<(RgbaImage, serde_json::Value)> {
    eprintln!(
        "Ordering {} rows and {} columns…",
        image.height(),
        image.width()
    );

    let start = Instant::now();
    let result = restore(image, config)?;
    let restored = apply_order(image, &result.ordering())?;

    print_costs(&result);

    let report = json!({
        "config": config,
        "width": image.width(),
        "height": image.height(),
        "elapsed_seconds": start.elapsed().as_secs_f64(),
        "restoration": result,
    });

    Ok((restored, report))
}

fn print_costs(result: &descramble::image_ordering::Restoration) {
    for (name, axis) in [("Rows", &result.rows), ("Columns", &result.columns)] {
        println!(
            "{name}: cost {:.3} → {:.3}, window {}, {} generations",
            axis.input_cost, axis.cost, axis.window, axis.generations
        );
    }
}

/// Ground truth is used only here, after the solver has returned.
fn adjacency_recovery(scrambled: &ImageOrdering, restored: &ImageOrdering) -> (f64, f64) {
    fn fraction(scrambled: &[usize], restored: &[usize]) -> f64 {
        if restored.len() < 2 {
            return 1.0;
        }

        let correct = restored
            .windows(2)
            .filter(|pair| scrambled[pair[0]].abs_diff(scrambled[pair[1]]) == 1)
            .count();

        correct as f64 / (restored.len() - 1) as f64
    }

    (
        fraction(&scrambled.rows, &restored.rows),
        fraction(&scrambled.columns, &restored.columns),
    )
}

fn read_image(path: &Path) -> Result<RgbaImage> {
    Ok(image::open(path)
        .with_context(|| format!("Could not read {}", path.display()))?
        .to_rgba8())
}

fn check_png(path: &Path) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        bail!("output must have a .png extension to preserve pixels losslessly");
    }

    Ok(())
}

fn save_png(image: &RgbaImage, path: &Path) -> Result<()> {
    image
        .save_with_format(path, ImageFormat::Png)
        .with_context(|| format!("Could not save {}", path.display()))
}

fn save_json(path: &Path, report: &serde_json::Value) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(report)?)
        .with_context(|| format!("Could not save {}", path.display()))
}
