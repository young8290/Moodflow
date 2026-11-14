use super::{Store, export_one};
use moodflow_capsule::capture::catalog::Outcome;
use std::{cell::Cell, fs};

#[test]
fn batch_export_preserves_destination_and_never_publishes_changed_or_revoked_files() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&output).unwrap();
    fs::write(&source, b"verified media").unwrap();
    let mut store = Store::open(&dir.path().join("capture")).unwrap();
    let asset = store.publish(&source, "png", 2, 2, 0, 1).unwrap();
    let destination = output.join("capture.png");

    fs::write(&destination, b"keep existing").unwrap();
    assert_eq!(
        export_one(&store, &asset.id, &destination, || true),
        Outcome::DestinationExists
    );
    assert_eq!(fs::read(&destination).unwrap(), b"keep existing");
    fs::remove_file(&destination).unwrap();

    let called = Cell::new(false);
    let result = export_one(&store, &asset.id, &destination, || {
        called.set(true);
        false
    });
    assert!(called.get());
    assert_eq!(result, Outcome::Paused);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    assert_eq!(
        export_one(&store, &asset.id, &destination, || true),
        Outcome::Done
    );
    assert_eq!(fs::read(&destination).unwrap(), b"verified media");

    fs::write(store.path(&asset.id).unwrap(), b"modified media").unwrap();
    assert_eq!(
        export_one(&store, &asset.id, &output.join("changed.png"), || true),
        Outcome::ChangedFile
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
}
