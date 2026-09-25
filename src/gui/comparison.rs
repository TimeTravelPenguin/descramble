//! Pixel comparisons use centered canvases without changing either image's scale.

use image::{Rgba, RgbaImage, imageops};

#[derive(Clone, Copy, Debug)]
pub(super) enum Transform {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

pub(super) fn transform(image: &RgbaImage, operation: Transform) -> RgbaImage {
    match operation {
        Transform::RotateLeft => imageops::rotate270(image),
        Transform::RotateRight => imageops::rotate90(image),
        Transform::FlipHorizontal => imageops::flip_horizontal(image),
        Transform::FlipVertical => imageops::flip_vertical(image),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DifferenceStats {
    /// Raw RGBA mismatches, including pixels covered by only one image.
    pub different_pixels: u64,
    /// Pixels covered by either image; empty corners of the canvas are excluded.
    pub compared_pixels: u64,
    pub size_mismatch: bool,
}

pub(super) struct Comparison {
    pub original: RgbaImage,
    pub restored: RgbaImage,
    pub stats: DifferenceStats,
    base_difference: RgbaImage,
}

impl Comparison {
    /// Center both images on their maximum extent, rounding offsets downward.
    /// Padding is transparent, and source pixels retain their original values.
    pub fn new(original: &RgbaImage, restored: &RgbaImage) -> Self {
        let width = original.width().max(restored.width());
        let height = original.height().max(restored.height());
        let original_offset = centered_offset(original, width, height);
        let restored_offset = centered_offset(restored, width, height);
        let mut original_canvas = RgbaImage::new(width, height);
        let mut restored_canvas = RgbaImage::new(width, height);
        let mut stats = DifferenceStats {
            different_pixels: 0,
            compared_pixels: 0,
            size_mismatch: original.dimensions() != restored.dimensions(),
        };

        let base_difference = RgbaImage::from_fn(width, height, |x, y| {
            let original_pixel = centered_pixel(original, original_offset, x, y);
            let restored_pixel = centered_pixel(restored, restored_offset, x, y);

            if let Some(pixel) = original_pixel {
                original_canvas.put_pixel(x, y, *pixel);
            }

            if let Some(pixel) = restored_pixel {
                restored_canvas.put_pixel(x, y, *pixel);
            }

            match (original_pixel, restored_pixel) {
                (Some(original), Some(restored)) => {
                    stats.compared_pixels += 1;
                    let difference = original
                        .0
                        .iter()
                        .zip(restored.0.iter())
                        .map(|(&original, &restored)| original.abs_diff(restored))
                        .max()
                        .unwrap_or(0);

                    if difference != 0 {
                        stats.different_pixels += 1;
                    }

                    Rgba([difference, difference, difference, 255])
                }
                (Some(_), None) | (None, Some(_)) => {
                    stats.compared_pixels += 1;
                    stats.different_pixels += 1;
                    Rgba([255, 0, 255, 255])
                }
                (None, None) => Rgba([0, 0, 0, 255]),
            }
        });

        Self {
            original: original_canvas,
            restored: restored_canvas,
            stats,
            base_difference,
        }
    }

    /// Crossfade premultiplied colors so transparent pixels do not tint the result.
    pub fn blend(&self, opacity: f32) -> RgbaImage {
        let opacity = unit_fraction(opacity);

        if opacity == 0.0 {
            return self.original.clone();
        }

        if opacity == 1.0 {
            return self.restored.clone();
        }

        RgbaImage::from_fn(self.original.width(), self.original.height(), |x, y| {
            let original = self.original.get_pixel(x, y);
            let restored = self.restored.get_pixel(x, y);
            let original_alpha = f32::from(original[3]) * (1.0 - opacity);
            let restored_alpha = f32::from(restored[3]) * opacity;
            let alpha = original_alpha + restored_alpha;
            let mut blended = [0, 0, 0, alpha.round() as u8];

            if alpha > 0.0 {
                for channel in 0..3 {
                    let color = f32::from(original[channel]) * original_alpha
                        + f32::from(restored[channel]) * restored_alpha;
                    blended[channel] = (color / alpha).round() as u8;
                }
            }

            Rgba(blended)
        })
    }

    /// Keep the original to the left of the divider and the result to its right.
    pub fn wipe(&self, divider: f32) -> RgbaImage {
        let divider = split_column(self.original.width(), divider);

        RgbaImage::from_fn(self.original.width(), self.original.height(), |x, y| {
            if x < divider {
                *self.original.get_pixel(x, y)
            } else {
                *self.restored.get_pixel(x, y)
            }
        })
    }

    /// Maximum absolute RGBA error; magenta denotes missing image coverage.
    pub fn difference(&self, gain: f32) -> RgbaImage {
        let gain = gain.max(0.0);

        RgbaImage::from_fn(self.original.width(), self.original.height(), |x, y| {
            let pixel = self.base_difference.get_pixel(x, y);

            if pixel[0] == 0 || pixel[0] != pixel[1] {
                return *pixel;
            }

            let difference = (f32::from(pixel[0]) * gain).round().min(255.0) as u8;
            Rgba([difference, difference, difference, 255])
        })
    }
}

/// Share the same rounding between displayed divider and composited pixels.
pub(super) fn split_column(width: u32, fraction: f32) -> u32 {
    ((width as f32 * unit_fraction(fraction)).floor() as u32).min(width)
}

fn centered_offset(image: &RgbaImage, width: u32, height: u32) -> (u32, u32) {
    ((width - image.width()) / 2, (height - image.height()) / 2)
}

fn centered_pixel(image: &RgbaImage, offset: (u32, u32), x: u32, y: u32) -> Option<&Rgba<u8>> {
    let image_x = x.checked_sub(offset.0)?;
    let image_y = y.checked_sub(offset.1)?;
    image.get_pixel_checked(image_x, image_y)
}

fn unit_fraction(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Comparison, DifferenceStats, Transform, transform};
    use image::{Rgba, RgbaImage};

    fn labeled_image(width: u32, labels: &[u8]) -> RgbaImage {
        RgbaImage::from_fn(width, labels.len() as u32 / width, |x, y| {
            Rgba([labels[(y * width + x) as usize], 0, 0, 255])
        })
    }

    #[test]
    fn transforms_preserve_pixels_and_have_expected_orientation() {
        let image = labeled_image(2, &[1, 2, 3, 4, 5, 6]);
        let operations = [
            (Transform::RotateLeft, labeled_image(3, &[2, 4, 6, 1, 3, 5])),
            (
                Transform::RotateRight,
                labeled_image(3, &[5, 3, 1, 6, 4, 2]),
            ),
            (
                Transform::FlipHorizontal,
                labeled_image(2, &[2, 1, 4, 3, 6, 5]),
            ),
            (
                Transform::FlipVertical,
                labeled_image(2, &[5, 6, 3, 4, 1, 2]),
            ),
        ];

        for (operation, expected) in operations {
            assert_eq!(transform(&image, operation), expected);
        }
    }

    #[test]
    fn rotations_and_flips_are_reversible() {
        let image = labeled_image(2, &[1, 2, 3, 4, 5, 6]);
        let mut rotated = image.clone();

        for _ in 0..4 {
            rotated = transform(&rotated, Transform::RotateRight);
        }

        assert_eq!(rotated, image);
        assert_eq!(
            transform(
                &transform(&image, Transform::RotateLeft),
                Transform::RotateRight
            ),
            image
        );

        for operation in [Transform::FlipHorizontal, Transform::FlipVertical] {
            assert_eq!(transform(&transform(&image, operation), operation), image);
        }
    }

    #[test]
    fn blend_preserves_endpoints_and_interpolates_colors() {
        let original = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
        let restored = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 255]));
        let comparison = Comparison::new(&original, &restored);

        assert_eq!(comparison.blend(-1.0), original);
        assert_eq!(comparison.blend(0.0), original);
        assert_eq!(comparison.blend(1.0), restored);
        assert_eq!(comparison.blend(2.0), restored);
        assert_eq!(
            *comparison.blend(0.5).get_pixel(0, 0),
            Rgba([128, 0, 128, 255])
        );
    }

    #[test]
    fn blend_does_not_leak_color_from_transparent_pixels() {
        let original = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
        let restored = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 0]));
        let comparison = Comparison::new(&original, &restored);

        assert_eq!(
            *comparison.blend(0.5).get_pixel(0, 0),
            Rgba([255, 0, 0, 128])
        );

        let original = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 0]));
        let comparison = Comparison::new(&original, &restored);

        assert_eq!(comparison.blend(0.0), original);
        assert_eq!(comparison.blend(1.0), restored);
        assert_eq!(*comparison.blend(0.5).get_pixel(0, 0), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn wipe_selects_the_expected_pixels_and_clamps_endpoints() {
        let original = labeled_image(3, &[1, 2, 3]);
        let restored = labeled_image(3, &[4, 5, 6]);
        let comparison = Comparison::new(&original, &restored);

        assert_eq!(comparison.wipe(0.5), labeled_image(3, &[1, 5, 6]));
        assert_eq!(comparison.wipe(0.0), restored);
        assert_eq!(comparison.wipe(-1.0), restored);
        assert_eq!(comparison.wipe(1.0), original);
        assert_eq!(comparison.wipe(2.0), original);

        let original = labeled_image(10, &[1; 10]);
        let restored = labeled_image(10, &[2; 10]);
        let comparison = Comparison::new(&original, &restored);
        assert_eq!(
            comparison.wipe(0.7),
            labeled_image(10, &[1, 1, 1, 1, 1, 1, 1, 2, 2, 2])
        );
    }

    #[test]
    fn difference_includes_alpha_and_hidden_color_errors() {
        let original = RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 0]).unwrap();
        let restored = RgbaImage::from_raw(2, 1, vec![10, 20, 30, 245, 45, 50, 60, 0]).unwrap();
        let comparison = Comparison::new(&original, &restored);

        assert_eq!(comparison.stats.different_pixels, 2);
        assert_eq!(comparison.stats.compared_pixels, 2);
        assert_eq!(
            comparison.difference(1.0).into_raw(),
            vec![10, 10, 10, 255, 5, 5, 5, 255]
        );
        assert_eq!(
            comparison.difference(30.0).into_raw(),
            vec![255, 255, 255, 255, 150, 150, 150, 255]
        );
    }

    #[test]
    fn identical_images_have_no_difference_even_when_transparent() {
        let image = RgbaImage::from_raw(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 0]).unwrap();
        let comparison = Comparison::new(&image, &image);

        assert_eq!(
            comparison.stats,
            DifferenceStats {
                different_pixels: 0,
                compared_pixels: 2,
                size_mismatch: false,
            }
        );
        assert_eq!(
            comparison.difference(10.0),
            RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]))
        );
        assert_eq!(
            comparison.difference(f32::INFINITY),
            RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]))
        );
    }

    #[test]
    fn unequal_sizes_are_centered_without_resampling_and_track_coverage() {
        let original = RgbaImage::from_pixel(3, 1, Rgba([0, 0, 0, 0]));
        let restored = RgbaImage::from_pixel(1, 3, Rgba([0, 0, 0, 0]));
        let comparison = Comparison::new(&original, &restored);
        let difference = comparison.difference(0.0);

        assert_eq!(comparison.original.dimensions(), (3, 3));
        assert_eq!(comparison.restored.dimensions(), (3, 3));
        assert_eq!(
            comparison.stats,
            DifferenceStats {
                different_pixels: 4,
                compared_pixels: 5,
                size_mismatch: true,
            }
        );

        for (x, y) in [(0, 1), (1, 0), (2, 1), (1, 2)] {
            assert_eq!(*difference.get_pixel(x, y), Rgba([255, 0, 255, 255]));
        }

        for (x, y) in [(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)] {
            assert_eq!(*difference.get_pixel(x, y), Rgba([0, 0, 0, 255]));
        }

        let original = labeled_image(4, &[1, 2, 3, 4]);
        let restored = labeled_image(1, &[8]);
        let comparison = Comparison::new(&original, &restored);

        assert_eq!(comparison.original, original);
        assert_eq!(*comparison.restored.get_pixel(1, 0), Rgba([8, 0, 0, 255]));
        assert_eq!(*comparison.restored.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
        assert_eq!(*comparison.restored.get_pixel(2, 0), Rgba([0, 0, 0, 0]));
        assert_eq!(*comparison.restored.get_pixel(3, 0), Rgba([0, 0, 0, 0]));
    }
}
