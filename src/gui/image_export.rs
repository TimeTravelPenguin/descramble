//! Save a snapshot of the displayed pixels without blocking the GUI.

use std::path::{Path, PathBuf};

use color_eyre::{
    Result,
    eyre::{bail, eyre},
};
use iced::widget::image::Handle;
use image::RgbaImage;

use crate::storage;

pub(super) async fn save(
    handle: Handle,
    filename: &'static str,
) -> std::result::Result<Option<PathBuf>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save displayed image…")
        .set_file_name(filename)
        .add_filter("PNG image", &["png"])
        .save_file()
        .await
    else {
        tracing::debug!("Image export canceled");

        return Ok(None);
    };

    let path = file.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        write_png(&handle, &path).map_err(|error| {
            tracing::error!(path = %path.display(), error = ?error, "Image export failed");
            format!("{error:#}")
        })?;

        tracing::info!(path = %path.display(), "Displayed image saved");

        Ok(Some(path))
    })
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "Image export worker stopped");
        format!("The image export task stopped: {error}")
    })?
}

/// Encode decoded pixels before atomically replacing the exact selected path.
pub(super) fn write_png(handle: &Handle, path: &Path) -> Result<()> {
    storage::check_png(path)?;

    let Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = handle
    else {
        bail!("Only decoded image pixels can be exported");
    };

    if *width == 0 || *height == 0 {
        bail!("Cannot export an image with zero width or height");
    }

    let expected_length = (*width as usize)
        .checked_mul(*height as usize)
        .and_then(|length| length.checked_mul(4))
        .ok_or_else(|| eyre!("Image dimensions are too large to export"))?;

    if pixels.len() != expected_length {
        bail!("Image pixel data does not match its dimensions");
    }

    let image = RgbaImage::from_raw(*width, *height, pixels.to_vec())
        .ok_or_else(|| eyre!("Could not reconstruct the displayed image pixels"))?;
    let pending = storage::stage_png(&image, path)?;

    storage::commit_outputs(vec![pending])
}

#[cfg(test)]
mod tests {
    use std::fs;

    use iced::widget::image::Handle;

    use super::write_png;

    #[test]
    fn png_export_preserves_dimensions_rgba_and_transparent_colors() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("displayed.png");
        let pixels = vec![
            255, 0, 30, 255, 60, 70, 80, 128, 5, 6, 7, 1, 99, 100, 101, 0,
        ];
        let handle = Handle::from_rgba(2, 2, pixels.clone());

        write_png(&handle, &output).unwrap();

        let saved = image::open(&output).unwrap().to_rgba8();

        assert_eq!(saved.dimensions(), (2, 2));
        assert_eq!(saved.into_raw(), pixels);
    }

    #[test]
    fn wrong_extension_preserves_existing_file_and_does_not_append_png() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("displayed.jpg");
        let previous_contents = b"Keep this file";
        let handle = Handle::from_rgba(1, 1, vec![1, 2, 3, 255]);
        fs::write(&output, previous_contents).unwrap();

        assert!(write_png(&handle, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), previous_contents);
        assert!(!directory.path().join("displayed.jpg.png").exists());
        assert!(!directory.path().join("displayed.png").exists());
    }

    #[test]
    fn invalid_handles_preserve_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("displayed.png");
        let previous_contents = b"Keep this file";
        let handles = [
            Handle::from_rgba(0, 1, Vec::<u8>::new()),
            Handle::from_rgba(1, 0, Vec::<u8>::new()),
            Handle::from_rgba(2, 1, vec![1, 2, 3, 255]),
            Handle::from_rgba(1, 1, vec![1, 2, 3, 255, 4]),
            Handle::from_rgba(u32::MAX, u32::MAX, Vec::<u8>::new()),
            Handle::from_path(&output),
            Handle::from_bytes(vec![1_u8, 2, 3]),
        ];

        fs::write(&output, previous_contents).unwrap();

        for handle in handles {
            assert!(write_png(&handle, &output).is_err());
            assert_eq!(fs::read(&output).unwrap(), previous_contents);
        }
    }
}
