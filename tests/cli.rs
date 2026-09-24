use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use descramble::image_ordering::{ImageOrdering, apply_order};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_descramble"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LOG", "off")
        .output()
        .unwrap()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn fixture(directory: &TempDir) -> std::path::PathBuf {
    let image = RgbaImage::from_fn(9, 7, |column, row| {
        Rgba([
            (column * 25) as u8,
            (row * 30) as u8,
            40,
            (column + row) as u8,
        ])
    });

    let input = directory.path().join("input.png");
    image.save(&input).unwrap();
    input
}

fn read_order(rows: &Value, columns: &Value) -> ImageOrdering {
    ImageOrdering {
        rows: serde_json::from_value(rows.clone()).unwrap(),
        columns: serde_json::from_value(columns.clone()).unwrap(),
    }
}

#[test]
fn scramble_and_restore_reports_match_emitted_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let scrambled_path = directory.path().join("scrambled.png");
    let restored_path = directory.path().join("restored.png");
    // Regular existing output files may still be replaced.
    fs::write(&scrambled_path, b"old output").unwrap();
    let result = run(&[
        "scramble",
        path(&input),
        path(&scrambled_path),
        "--seed",
        "42",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = image::open(&input).unwrap().to_rgba8();
    let scrambled = image::open(&scrambled_path).unwrap().to_rgba8();
    let report: Value =
        serde_json::from_slice(&fs::read(scrambled_path.with_extension("json")).unwrap()).unwrap();
    let ordering = read_order(&report["ordering"]["rows"], &report["ordering"]["columns"]);
    assert_eq!(apply_order(&original, &ordering).unwrap(), scrambled);
    let result = run(&[
        "restore",
        path(&scrambled_path),
        path(&restored_path),
        "--population",
        "6",
        "--generations",
        "3",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let restored = image::open(&restored_path).unwrap().to_rgba8();
    let report: Value =
        serde_json::from_slice(&fs::read(restored_path.with_extension("json")).unwrap()).unwrap();
    let ordering = read_order(
        &report["restoration"]["rows"]["order"],
        &report["restoration"]["columns"]["order"],
    );
    assert_eq!(apply_order(&scrambled, &ordering).unwrap(), restored);
    assert_eq!(image::open(&input).unwrap().to_rgba8(), original);
}

#[test]
fn sidecar_failure_preserves_existing_png_for_both_commands() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let output = directory.path().join("blocked.png");
    fs::write(&output, b"must remain unchanged").unwrap();
    fs::create_dir(output.with_extension("json")).unwrap();

    for command in ["scramble", "restore"] {
        let result = run(&[command, path(&input), path(&output)]);
        assert!(!result.status.success());
        assert_eq!(fs::read(&output).unwrap(), b"must remain unchanged");
    }
}

#[test]
fn invalid_options_create_no_output_files() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let output = directory.path().join("output.png");
    let invalid_config = run(&["restore", path(&input), path(&output), "--population", "4"]);
    assert!(!invalid_config.status.success());
    assert!(!output.exists());
    assert!(!output.with_extension("json").exists());
    let invalid_extension = directory.path().join("output.jpg");
    assert!(
        !run(&["scramble", path(&input), path(&invalid_extension)])
            .status
            .success()
    );
    assert!(!invalid_extension.exists());
}

#[test]
fn commands_reject_input_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let original = fs::read(&input).unwrap();

    for command in ["scramble", "restore"] {
        assert!(!run(&[command, path(&input), path(&input)]).status.success());
        assert_eq!(fs::read(&input).unwrap(), original);
    }
}

#[test]
#[cfg(unix)]
fn commands_reject_symbolic_and_hard_link_aliases() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let original = fs::read(&input).unwrap();
    let symbolic = directory.path().join("symbolic.png");
    let hard = directory.path().join("hard.png");
    std::os::unix::fs::symlink(&input, &symbolic).unwrap();
    fs::hard_link(&input, &hard).unwrap();

    for alias in [&symbolic, &hard] {
        assert!(
            !run(&["scramble", path(&input), path(alias)])
                .status
                .success()
        );
        assert_eq!(fs::read(&input).unwrap(), original);
    }
}

#[test]
fn demo_checks_every_destination_before_replacing_outputs() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let output = directory.path().join("demo");
    fs::create_dir(&output).unwrap();
    fs::create_dir(output.join("report.json")).unwrap();
    fs::write(output.join("original.png"), b"original output").unwrap();
    let result = run(&["demo", path(&input), "--output-dir", path(&output)]);
    assert!(!result.status.success());
    assert_eq!(
        fs::read(output.join("original.png")).unwrap(),
        b"original output"
    );
    assert!(!output.join("scrambled.png").exists());
    assert!(!output.join("restored.png").exists());
}

#[test]
fn logging_uses_stderr_and_respects_environment_filters() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let output = directory.path().join("logged.png");
    let result = Command::new(env!("CARGO_BIN_EXE_descramble"))
        .args(["scramble", path(&input), path(&output)])
        .env("RUST_LOG", "descramble=debug")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stderr.contains("Loading image"));
    assert!(stderr.contains("Image scrambled"));
    assert!(!stderr.contains('\u{1b}'));
    assert!(stdout.contains("Saved"));
    assert!(!stdout.contains("Image scrambled"));
    let quiet = run(&["scramble", path(&input), path(&output)]);
    assert!(quiet.status.success());
    assert!(quiet.stderr.is_empty());
}

#[test]
fn malformed_logging_filter_reports_error_before_writing_outputs() {
    let directory = tempfile::tempdir().unwrap();
    let input = fixture(&directory);
    let output = directory.path().join("invalid-log.png");
    let result = Command::new(env!("CARGO_BIN_EXE_descramble"))
        .args(["scramble", path(&input), path(&output)])
        .env("RUST_LOG", "descramble=not-a-level")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Invalid RUST_LOG filter"));
    assert!(!output.exists());
}
