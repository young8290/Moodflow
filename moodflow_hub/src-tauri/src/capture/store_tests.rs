use super::{DELETE_MARKER, DELETE_STAGE, Store};
use moodflow_capsule::capture::{
    Asset, RETENTION_SECONDS,
    catalog::{Operation, Outcome, Scope, Selection, revision},
};
use std::{collections::BTreeSet, fs, path::Path};

fn fixture(store: &mut Store, dir: &Path, data: &[u8]) -> Asset {
    let path = dir.join("source");
    fs::write(&path, data).unwrap();
    store.publish(&path, "png", 12, 10, 0, 1).unwrap()
}

fn selection(asset: &Asset) -> Selection {
    Selection {
        id: asset.id.clone(),
        revision: revision(asset),
    }
}

#[test]
fn restore_preserves_bytes_and_resets_expiration_without_unprotecting_library_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("capture");
    let mut store = Store::open(&root).unwrap();
    let a = fixture(&mut store, dir.path(), b"first fixture");
    let b = fixture(&mut store, dir.path(), b"second fixture");
    let c = fixture(&mut store, dir.path(), b"library fixture");
    let now = RETENTION_SECONDS + 10;
    let old = selection(&a);
    store.rename(&old, "  演示记录  ").unwrap();
    assert_eq!(
        store.apply(&old, Operation::Trash, false, now, || true),
        Outcome::Stale
    );
    let item = selection(store.get(&a.id).unwrap());
    assert_eq!(
        store.apply(&item, Operation::Trash, true, now, || true),
        Outcome::InUse
    );
    assert_eq!(
        store.apply(&item, Operation::Trash, false, now, || true),
        Outcome::Done
    );
    assert!(store.active(&a.id).is_err());
    assert_eq!(
        store.apply(&selection(&b), Operation::Keep, false, now, || true),
        Outcome::Done
    );
    store.set_permanent(&c.id).unwrap();
    let library_item = selection(store.get(&c.id).unwrap());
    assert_eq!(
        store.apply(&library_item, Operation::Trash, false, now, || true),
        Outcome::WrongState
    );
    store.prune_for_space(&BTreeSet::new(), now, 0).unwrap();
    assert_eq!(store.assets().len(), 3);
    drop(store);

    let mut store = Store::open(&root).unwrap();
    let trash = selection(store.get(&a.id).unwrap());
    assert_eq!(
        store.apply(&trash, Operation::Restore, false, now, || true),
        Outcome::Done
    );
    assert_eq!(store.active(&a.id).unwrap().name, "演示记录");
    assert_eq!(
        fs::read(store.verified_path(&a.id).unwrap()).unwrap(),
        b"first fixture"
    );
    store.prune_for_space(&BTreeSet::new(), now, 0).unwrap();
    assert_eq!(store.assets().len(), 3);

    let item = selection(store.get(&b.id).unwrap());
    assert_eq!(
        store.apply(&item, Operation::Release, false, now, || true),
        Outcome::Done
    );
    store
        .prune_for_space(&BTreeSet::new(), now + RETENTION_SECONDS, 0)
        .unwrap();
    assert_eq!(store.assets().len(), 1);
    assert!(Scope::of(store.get(&c.id).unwrap()) == Scope::Library);
}

#[test]
fn interrupted_permanent_delete_rolls_back_or_finishes_based_on_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("capture");
    let mut store = Store::open(&root).unwrap();
    let asset = fixture(&mut store, dir.path(), b"journal fixture");
    let path = store.path(&asset.id).unwrap();
    let marker = root.join(DELETE_MARKER);
    let parked = root.join(DELETE_STAGE);
    let record = serde_json::to_vec(&asset).unwrap();
    drop(store);
    fs::write(&marker, &record).unwrap();
    fs::rename(&path, &parked).unwrap();
    let mut store = Store::open(&root).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"journal fixture");
    assert!(!marker.exists() && !parked.exists());

    store.remove(&asset.id).unwrap();
    drop(store);
    fs::write(&marker, record).unwrap();
    fs::write(&parked, b"journal fixture").unwrap();
    let store = Store::open(&root).unwrap();
    assert!(store.assets().is_empty());
    assert!(!path.exists() && !marker.exists() && !parked.exists());
}

#[test]
fn changed_media_is_not_purged_and_corrupt_journal_leaves_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("capture");
    let mut store = Store::open(&root).unwrap();
    let asset = fixture(&mut store, dir.path(), b"expected");
    assert_eq!(
        store.apply(&selection(&asset), Operation::Trash, false, 2, || true),
        Outcome::Done
    );
    let item = selection(store.get(&asset.id).unwrap());
    let media = store.path(&asset.id).unwrap();
    fs::write(&media, b"external").unwrap();
    assert_eq!(
        store.apply(&item, Operation::Purge, false, 3, || true),
        Outcome::ChangedFile
    );
    assert_eq!(fs::read(&media).unwrap(), b"external");
    drop(store);
    fs::write(root.join(DELETE_MARKER), b"corrupt journal").unwrap();
    assert!(Store::open(&root).is_err());
    assert_eq!(fs::read(media).unwrap(), b"external");
    assert_eq!(
        fs::read(root.join(DELETE_MARKER)).unwrap(),
        b"corrupt journal"
    );
}

#[test]
fn privacy_revocation_during_purge_restores_the_staged_file() {
    use std::cell::Cell;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("capture");
    let mut store = Store::open(&root).unwrap();
    let asset = fixture(&mut store, dir.path(), b"private fixture");
    assert_eq!(
        store.apply(&selection(&asset), Operation::Trash, false, 2, || true),
        Outcome::Done
    );
    let item = selection(store.get(&asset.id).unwrap());
    let checks = Cell::new(0);
    let result = store.apply(&item, Operation::Purge, false, 3, || {
        checks.set(checks.get() + 1);
        checks.get() == 1
    });
    assert_eq!(result, Outcome::Paused);
    assert_eq!(checks.get(), 2);
    assert_eq!(Scope::of(store.get(&asset.id).unwrap()), Scope::Trash);
    assert_eq!(
        fs::read(store.verified_path(&asset.id).unwrap()).unwrap(),
        b"private fixture"
    );
    assert!(!root.join(DELETE_MARKER).exists() && !root.join(DELETE_STAGE).exists());
}
