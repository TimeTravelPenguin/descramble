use std::fs;

use descramble::storage;

#[test]
fn preflight_rejects_duplicate_destinations_before_files_exist() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    fs::write(&input, b"input").unwrap();
    let output = directory.path().join("output.png");
    let alias = directory.path().join(".").join("output.png");
    assert!(storage::validate_outputs(&input, &[&output, &alias]).is_err());
    assert!(!output.exists());
}

#[test]
#[cfg(unix)]
fn preflight_resolves_parent_symlinks_for_new_destinations() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    fs::write(&input, b"input").unwrap();
    let parent = directory.path().join("outputs");
    fs::create_dir(&parent).unwrap();
    let linked_parent = directory.path().join("linked-outputs");
    std::os::unix::fs::symlink(&parent, &linked_parent).unwrap();
    assert!(
        storage::validate_outputs(
            &input,
            &[&parent.join("new.png"), &linked_parent.join("new.png")]
        )
        .is_err()
    );
}
