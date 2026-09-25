use image::RgbaImage;
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::Serialize;

use crate::memetic::{OrderingResult, SolverConfig, solve};
use crate::objective::DistanceMatrix;
use crate::{Error, Result, validate_order};

#[derive(Debug, Clone, Copy)]
pub enum Axis {
    Rows,
    Columns,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageOrdering {
    /// Output position -> input position, independently for each axis.
    pub rows: Vec<usize>,
    pub columns: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub struct Restoration {
    pub rows: OrderingResult,
    pub columns: OrderingResult,
}

impl Restoration {
    pub fn ordering(&self) -> ImageOrdering {
        ImageOrdering {
            rows: self.rows.order.clone(),
            columns: self.columns.order.clone(),
        }
    }
}

/// RGB components form each vector. Alpha is preserved when rearranging pixels,
/// but does not contribute to similarity. No spatial filters are used.
pub fn distances(image: &RgbaImage, axis: Axis) -> Result<DistanceMatrix> {
    check_dimensions(image)?;

    let (count, length) = match axis {
        Axis::Rows => (image.height(), image.width()),
        Axis::Columns => (image.width(), image.height()),
    };

    let vectors: Vec<Vec<f64>> = (0..count)
        .map(|item| {
            (0..length)
                .flat_map(|offset| {
                    let pixel = match axis {
                        Axis::Rows => image.get_pixel(offset, item),
                        Axis::Columns => image.get_pixel(item, offset),
                    };

                    [
                        f64::from(pixel[0]),
                        f64::from(pixel[1]),
                        f64::from(pixel[2]),
                    ]
                })
                .collect()
        })
        .collect();

    DistanceMatrix::from_vectors(&vectors)
}

pub fn restore(image: &RgbaImage, config: &SolverConfig) -> Result<Restoration> {
    config.validate()?;

    tracing::debug!(items = image.height(), "Ordering rows");

    let rows = solve(distances(image, Axis::Rows)?, config)?;
    let column_config = SolverConfig {
        seed: config.seed.wrapping_add(1),
        ..config.clone()
    };

    tracing::debug!(items = image.width(), "Ordering columns");

    let columns = solve(distances(image, Axis::Columns)?, &column_config)?;

    Ok(Restoration { rows, columns })
}

pub fn scramble(image: &RgbaImage, seed: u64) -> Result<(RgbaImage, ImageOrdering)> {
    check_dimensions(image)?;

    let mut rng = StdRng::seed_from_u64(seed);
    let mut rows: Vec<usize> = (0..image.height() as usize).collect();
    let mut columns: Vec<usize> = (0..image.width() as usize).collect();

    rows.shuffle(&mut rng);
    columns.shuffle(&mut rng);

    let ordering = ImageOrdering { rows, columns };

    Ok((apply_order(image, &ordering)?, ordering))
}

pub fn apply_order(image: &RgbaImage, ordering: &ImageOrdering) -> Result<RgbaImage> {
    check_dimensions(image)?;
    validate_order(&ordering.rows, image.height() as usize)?;
    validate_order(&ordering.columns, image.width() as usize)?;

    Ok(RgbaImage::from_fn(
        image.width(),
        image.height(),
        |column, row| {
            *image.get_pixel(
                ordering.columns[column as usize] as u32,
                ordering.rows[row as usize] as u32,
            )
        },
    ))
}

fn check_dimensions(image: &RgbaImage) -> Result<()> {
    if image.width() == 0 || image.height() == 0 {
        return Err(Error::InvalidInput(
            "image must have nonzero width and height".into(),
        ));
    }

    Ok(())
}
