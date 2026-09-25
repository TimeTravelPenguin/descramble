use descramble::{application, image_ordering::apply_order, memetic::SolverConfig};
use image::{Rgba, RgbaImage};

#[test]
fn shared_experiment_returns_reconstructable_images_and_compatible_report() {
    let original = RgbaImage::from_fn(9, 7, |column, row| {
        Rgba([
            (column * 25) as u8,
            (row * 30) as u8,
            40,
            (column + row) as u8,
        ])
    });

    let config = SolverConfig {
        population: 6,
        generations: 3,
        ..SolverConfig::default()
    };
    let result = application::run_experiment(&original, &config).unwrap();
    assert_eq!(result.original, original);
    assert_eq!(
        apply_order(&original, &result.report.scramble_order).unwrap(),
        result.scrambled
    );
    assert_eq!(
        apply_order(&result.scrambled, &result.report.run.restoration.ordering()).unwrap(),
        result.restored
    );
    assert_eq!(result.report.row_adjacency_recovery, 1.0);
    assert_eq!(result.report.column_adjacency_recovery, 1.0);
    let report = serde_json::to_value(&result.report).unwrap();
    assert!(report.get("run").is_none());
    assert_eq!(report["width"], 9);
    assert_eq!(report["height"], 7);
    assert!(report["restoration"]["rows"]["order"].is_array());
    assert!(report["scramble_order"]["columns"].is_array());
    assert_eq!(report["config"]["generations"], 3);
}

#[test]
fn shared_workflows_propagate_invalid_input_without_startup_setup() {
    let empty = RgbaImage::new(0, 0);
    assert!(application::run_scramble(&empty, 1).is_err());
    assert!(application::run_restoration(&empty, &SolverConfig::default()).is_err());
    assert!(application::run_experiment(&empty, &SolverConfig::default()).is_err());
}
