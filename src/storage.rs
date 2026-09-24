//! Image loading and staged artifact output, shared by the CLI and GUI.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use color_eyre::{
    Result,
    eyre::{Context, bail},
};
use image::{ImageFormat, RgbaImage};
use tempfile::NamedTempFile;

pub fn read_image(path: &Path) -> Result<RgbaImage> {
    tracing::debug!(path = %path.display(), "Loading image");
    Ok(image::open(path)
        .with_context(|| format!("Could not read {}", path.display()))?
        .to_rgba8())
}

pub fn check_png(path: &Path) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        bail!("output must have a .png extension to preserve pixels losslessly");
    }

    Ok(())
}

/// Fail before searching or replacing any artifact if a destination is unusable.
pub fn validate_outputs(input: &Path, outputs: &[&Path]) -> Result<()> {
    let input =
        fs::canonicalize(input).with_context(|| format!("Could not read {}", input.display()))?;
    let mut existing = vec![input];
    let mut destinations = HashSet::new();

    for &output in outputs {
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));

        if !parent.is_dir() {
            bail!("Output directory does not exist: {}", parent.display());
        }

        let Some(filename) = output.file_name() else {
            bail!("Output must name a file: {}", output.display());
        };

        let destination = fs::canonicalize(parent)?.join(filename);

        if !destinations.insert(destination) {
            bail!("Duplicate output destination: {}", output.display());
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

pub struct PendingOutput {
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

pub fn stage_png(image: &RgbaImage, path: &Path) -> Result<PendingOutput> {
    let mut pending = PendingOutput::new(path)?;
    image
        .write_to(&mut pending.file, ImageFormat::Png)
        .with_context(|| format!("Could not encode {}", path.display()))?;

    Ok(pending)
}

pub fn stage_json(report: &impl serde::Serialize, path: &Path) -> Result<PendingOutput> {
    let mut pending = PendingOutput::new(path)?;
    serde_json::to_writer_pretty(&mut pending.file, report)
        .with_context(|| format!("Could not encode {}", path.display()))?;

    Ok(pending)
}

pub fn commit_outputs(outputs: Vec<PendingOutput>) -> Result<()> {
    // All encoding must succeed before replacing any output. Each rename is atomic,
    // but the group is not a transaction if the filesystem changes during commit.
    for output in outputs {
        output
            .file
            .persist(&output.path)
            .with_context(|| format!("Could not save {}", output.path.display()))?;
        tracing::debug!(path = %output.path.display(), "Output saved");
    }

    Ok(())
}

/// Resize an original image before scrambling. Never resize an already scrambled image.
pub fn read_thumbnail(path: &Path, max_dimension: u32) -> Result<RgbaImage> {
    if max_dimension == 0 {
        bail!("maximum image dimension must be positive");
    }

    tracing::debug!(path = %path.display(), max_dimension, "Loading image thumbnail");
    Ok(image::open(path)
        .with_context(|| format!("Could not read {}", path.display()))?
        .thumbnail(max_dimension, max_dimension)
        .to_rgba8())
}
