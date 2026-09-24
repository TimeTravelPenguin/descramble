use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use clap::Parser;
use color_eyre::{
    Result,
    eyre::{Context, bail},
};
use descramble::{
    cli::{Cli, Command, SolverArgs},
    image_ordering::{ImageOrdering, Restoration, apply_order, restore, scramble},
    memetic::SolverConfig,
};
use image::{ImageFormat, RgbaImage};
use serde_json::json;
use tempfile::NamedTempFile;

fn main() -> Result<()> {
    color_eyre::install()?;

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
    validate_outputs(
        &input,
        &[
            &original_path,
            &scrambled_path,
            &restored_path,
            &report_path,
        ],
    )?;

    let original = image::open(&input)
        .with_context(|| format!("Could not read {}", input.display()))?
        .thumbnail(max_dimension, max_dimension)
        .to_rgba8();

    let (scrambled, scramble_order) = scramble(&original, config.seed)?;
    let (restored, result, mut report) = run_restoration(&scrambled, &config)?;
    let recovery = adjacency_recovery(&scramble_order, &result.ordering());

    println!(
        "Original neighbor recovery: rows {:.1}%, columns {:.1}%",
        100.0 * recovery.0,
        100.0 * recovery.1
    );

    report["scramble_order"] = serde_json::to_value(scramble_order)?;
    report["row_adjacency_recovery"] = json!(recovery.0);
    report["column_adjacency_recovery"] = json!(recovery.1);
    commit_outputs(vec![
        stage_png(&original, &original_path)?,
        stage_png(&scrambled, &scrambled_path)?,
        stage_png(&restored, &restored_path)?,
        stage_json(&report, &report_path)?,
    ])?;

    println!("Saved demo to {}", output_dir.display());

    Ok(())
}

fn run_restore(input: PathBuf, output: PathBuf, solver: SolverArgs) -> Result<()> {
    check_png(&output)?;

    let config = SolverConfig::from(solver);
    config.validate()?;
    let report_path = output.with_extension("json");
    validate_outputs(&input, &[&output, &report_path])?;

    let image = read_image(&input)?;
    let (restored, _, report) = run_restoration(&image, &config)?;

    commit_outputs(vec![
        stage_png(&restored, &output)?,
        stage_json(&report, &report_path)?,
    ])?;

    println!("Saved {} and its search report", output.display());

    Ok(())
}

fn run_scramble(input: PathBuf, output: PathBuf, seed: u64) -> Result<()> {
    check_png(&output)?;
    let report_path = output.with_extension("json");
    validate_outputs(&input, &[&output, &report_path])?;

    let image = read_image(&input)?;
    let (scrambled, ordering) = scramble(&image, seed)?;

    let report = json!({ "seed": seed, "ordering": ordering });
    commit_outputs(vec![
        stage_png(&scrambled, &output)?,
        stage_json(&report, &report_path)?,
    ])?;

    println!("Saved {} and its permutation map", output.display());

    Ok(())
}

fn run_restoration(
    image: &RgbaImage,
    config: &SolverConfig,
) -> Result<(RgbaImage, Restoration, serde_json::Value)> {
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

    Ok((restored, result, report))
}

fn print_costs(result: &Restoration) {
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

/// Fail before searching or replacing any artifact if a destination is unusable.
fn validate_outputs(input: &Path, outputs: &[&Path]) -> Result<()> {
    let input =
        fs::canonicalize(input).with_context(|| format!("Could not read {}", input.display()))?;
    let mut existing = vec![input];

    for &output in outputs {
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));

        if !parent.is_dir() {
            bail!("Output directory does not exist: {}", parent.display());
        }

        match fs::metadata(output) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    bail!("Output is not a regular file: {}", output.display());
                }

                let resolved = fs::canonicalize(output)?;

                for other in &existing {
                    if same_file(&resolved, other)? {
                        bail!(
                            "Output aliases the input or another output: {}",
                            output.display()
                        );
                    }
                }

                existing.push(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Could not inspect {}", output.display()));
            }
        }
    }

    Ok(())
}

fn same_file(left: &Path, right: &Path) -> Result<bool> {
    if left == right {
        return Ok(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let left = fs::metadata(left)?;
        let right = fs::metadata(right)?;
        Ok(left.dev() == right.dev() && left.ino() == right.ino())
    }

    #[cfg(not(unix))]
    Ok(false)
}

struct PendingOutput {
    path: PathBuf,
    file: NamedTempFile,
}

impl PendingOutput {
    fn new(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file = NamedTempFile::new_in(parent)
            .with_context(|| format!("Could not prepare {}", path.display()))?;

        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }
}

fn stage_png(image: &RgbaImage, path: &Path) -> Result<PendingOutput> {
    let mut pending = PendingOutput::new(path)?;
    image
        .write_to(&mut pending.file, ImageFormat::Png)
        .with_context(|| format!("Could not encode {}", path.display()))?;

    Ok(pending)
}

fn stage_json(report: &serde_json::Value, path: &Path) -> Result<PendingOutput> {
    let mut pending = PendingOutput::new(path)?;
    serde_json::to_writer_pretty(&mut pending.file, report)
        .with_context(|| format!("Could not encode {}", path.display()))?;

    Ok(pending)
}

fn commit_outputs(outputs: Vec<PendingOutput>) -> Result<()> {
    // All encoding must succeed before replacing any output. Each rename is atomic,
    // but the group is not a transaction if the filesystem changes during commit.
    for output in outputs {
        output
            .file
            .persist(&output.path)
            .with_context(|| format!("Could not save {}", output.path.display()))?;
    }

    Ok(())
}
