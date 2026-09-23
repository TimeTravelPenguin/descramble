use descramble::{
    image_ordering::{Axis, ImageOrdering, apply_order, distances, restore, scramble},
    memetic::{SolverConfig, solve},
    objective::DistanceMatrix,
};
use image::{Rgba, RgbaImage};

fn config() -> SolverConfig {
    SolverConfig {
        population: 8,
        generations: 10,
        window: Some(1),
        ..SolverConfig::default()
    }
}

#[test]
fn reconstructs_gradient_exactly_up_to_axis_reflections() {
    let original = RgbaImage::from_fn(13, 9, |column, row| {
        Rgba([
            (column * 17) as u8,
            (row * 23) as u8,
            0,
            (column + row) as u8,
        ])
    });

    let (scrambled, truth) = scramble(&original, 49).unwrap();
    let result = restore(&scrambled, &config()).unwrap();
    let restored = apply_order(&scrambled, &result.ordering()).unwrap();
    let recovered_rows: Vec<usize> = result
        .rows
        .order
        .iter()
        .map(|&item| truth.rows[item])
        .collect();
    let recovered_columns: Vec<usize> = result
        .columns
        .order
        .iter()
        .map(|&item| truth.columns[item])
        .collect();

    for order in [&recovered_rows, &recovered_columns] {
        assert!(order.windows(2).all(|pair| pair[0].abs_diff(pair[1]) == 1));
    }

    let expected = apply_order(
        &original,
        &ImageOrdering {
            rows: recovered_rows,
            columns: recovered_columns,
        },
    )
    .unwrap();
    assert_eq!(restored, expected);
}

#[test]
fn opposite_axis_permutation_preserves_euclidean_distances() {
    let image = RgbaImage::from_fn(7, 5, |column, row| {
        Rgba([
            (column * row) as u8,
            (column + 3 * row) as u8,
            (2 * column + row) as u8,
            255,
        ])
    });

    let reordered = apply_order(
        &image,
        &ImageOrdering {
            rows: vec![3, 0, 4, 1, 2],
            columns: vec![4, 6, 1, 3, 0, 5, 2],
        },
    )
    .unwrap();

    for (axis, order) in [
        (Axis::Rows, vec![3, 0, 4, 1, 2]),
        (Axis::Columns, vec![4, 6, 1, 3, 0, 5, 2]),
    ] {
        let before = distances(&image, axis).unwrap();
        let after = distances(&reordered, axis).unwrap();

        for row in 0..order.len() {
            for column in 0..order.len() {
                assert_eq!(
                    before.get(order[row], order[column]),
                    after.get(row, column)
                );
            }
        }
    }
}

#[test]
fn handles_singletons_thin_images_and_identical_strips() {
    for (width, height) in [(1, 1), (1, 7), (7, 1), (2, 2), (5, 4)] {
        let image = RgbaImage::from_pixel(width, height, Rgba([80, 40, 20, 255]));
        let result = restore(&image, &config()).unwrap();
        assert_eq!(result.rows.cost, 0.0);
        assert_eq!(result.columns.cost, 0.0);
        assert_eq!(apply_order(&image, &result.ordering()).unwrap(), image);
    }
}

#[test]
fn rejects_invalid_config_images_and_permutations() {
    let image = RgbaImage::new(2, 2);
    assert!(restore(&RgbaImage::new(0, 2), &config()).is_err());
    assert!(
        apply_order(
            &image,
            &ImageOrdering {
                rows: vec![0, 0],
                columns: vec![0, 1]
            }
        )
        .is_err()
    );

    for invalid in [
        SolverConfig {
            population: 3,
            ..config()
        },
        SolverConfig {
            generations: 0,
            ..config()
        },
        SolverConfig {
            window: Some(0),
            ..config()
        },
    ] {
        assert!(solve(DistanceMatrix::from_dense(1, vec![0.0]).unwrap(), &invalid).is_err());
    }
}
