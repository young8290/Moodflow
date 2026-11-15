//! Atomic, bounded local capture index. Unrecognized/corrupt data is never rebuilt or discarded.
use anyhow::{Result, ensure};
use moodflow_capsule::capture::{
    self, Asset, MAX_ASSET_BYTES, MAX_TOTAL_BYTES,
    catalog::{self, MAX_BATCH, Operation, Outcome, Scope, Selection},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u8,
    assets: Vec<Asset>,
}
const DELETE_MARKER: &str = ".capture-delete.json";
const DELETE_STAGE: &str = ".capture-delete-media";

pub struct Store {
    root: PathBuf,
    _lock: File,
    index: Index,
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "capture.invalid_file"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "capture.invalid_file"
        );
    }
    Ok(metadata)
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn digest(path: &Path) -> Result<(u64, String)> {
    ensure!(
        regular(path)?.len() <= MAX_ASSET_BYTES,
        "capture.file_limit"
    );
    let mut file = File::open(path)?;
    let mut sha = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0; 65536];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        bytes += length as u64;
        ensure!(bytes <= MAX_ASSET_BYTES, "capture.file_limit");
        sha.update(&buffer[..length]);
    }
    Ok((bytes, format!("{:x}", sha.finalize())))
}
pub fn copy_candidate(path: &Path, bytes: u64, output: &mut impl Write) -> Result<()> {
    ensure!(
        regular(path)?.len() == bytes && bytes <= MAX_ASSET_BYTES,
        "capture.changed"
    );
    ensure!(
        std::io::copy(&mut File::open(path)?.take(bytes + 1), output)? == bytes,
        "capture.changed"
    );
    Ok(())
}

pub fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        regular(path)?.len() <= MAX_ASSET_BYTES,
        "capture.file_limit"
    );
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_ASSET_BYTES, "capture.file_limit");
    Ok(bytes)
}
pub fn read_verified(path: &Path, asset: &Asset) -> Result<Vec<u8>> {
    let bytes = read_bounded(path)?;
    ensure!(
        bytes.len() as u64 == asset.bytes
            && format!("{:x}", Sha256::digest(&bytes)) == asset.sha256,
        "capture.changed"
    );
    Ok(bytes)
}
impl Store {
    pub fn cleanup_work(root: &Path) -> Result<()> {
        crate::private_fs::ensure(root)?;
        let canonical = root.canonicalize()?;
        for entry in fs::read_dir(root)?.take(10001) {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(suffix) = name.strip_prefix("capture-work-") else {
                continue;
            };
            if suffix.len() != 6 || !suffix.bytes().all(|c| c.is_ascii_alphanumeric()) {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || path.canonicalize()?.parent() != Some(canonical.as_path())
            {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            let mut files = Vec::new();
            let mut valid = true;
            for file in fs::read_dir(&path)?.take(7) {
                let file = file?;
                if ![
                    "output.png",
                    "output.mp4",
                    "output.gif",
                    "edited.png",
                    "image.dib",
                ]
                .iter()
                .any(|n| file.file_name() == *n)
                    || regular(&file.path()).is_err()
                {
                    valid = false;
                    break;
                }
                files.push(file.path());
            }
            if !valid || files.len() > 5 {
                continue;
            }
            for file in files {
                fs::remove_file(file)?;
            }
            fs::remove_dir(path)?;
        }
        Ok(())
    }
    pub fn open(root: &Path) -> Result<Self> {
        crate::private_fs::ensure(root)?;
        let lock_path = root.join(".capture.lock");
        if lock_path.exists() {
            regular(&lock_path)?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.lock()?;
        let path = root.join("index.json");
        let index = match regular(&path) {
            Ok(metadata) => {
                ensure!(metadata.len() <= 4 * 1024 * 1024, "capture.index_limit");
                let mut bytes = Vec::new();
                File::open(path)?
                    .take(4 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 4 * 1024 * 1024, "capture.index_limit");
                serde_json::from_slice::<Index>(&bytes)?
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Index {
                    version: 1,
                    assets: Vec::new(),
                }
            }
            Err(error) => return Err(error),
        };
        ensure!(
            index.version == 1 && index.assets.len() <= 10000,
            "capture.index_limit"
        );
        capture::prune_candidates(&index.assets, &BTreeSet::new(), 0)?;
        let store = Self {
            root: root.to_owned(),
            _lock: lock,
            index,
        };
        store
            .recover_delete()
            .map_err(|_| anyhow::anyhow!("capture.catalog_delete_pending"))?;
        Ok(store)
    }
    pub fn assets(&self) -> &[Asset] {
        &self.index.assets
    }
    pub fn get(&self, id: &str) -> Result<&Asset> {
        self.index
            .assets
            .iter()
            .find(|asset| asset.id == id)
            .ok_or_else(|| anyhow::anyhow!("capture.missing"))
    }
    pub fn path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.asset_path(self.get(id)?))
    }
    fn asset_path(&self, asset: &Asset) -> PathBuf {
        self.root.join(format!("{}.{}", asset.id, asset.extension))
    }
    pub fn verified_path(&self, id: &str) -> Result<PathBuf> {
        let asset = self.get(id)?;
        let path = self.path(id)?;
        let (size, hash) = digest(&path)?;
        ensure!(
            size == asset.bytes && hash == asset.sha256,
            "capture.changed"
        );
        Ok(path)
    }
    pub fn prune_for_space(
        &mut self,
        leased: &BTreeSet<String>,
        now: u64,
        incoming: u64,
    ) -> Result<()> {
        let ids = capture::prune_for_space(&self.index.assets, leased, now, incoming)?;
        // Bound synchronous hashing and durable commits before publishing a new capture.
        // Further expired records remain visible in the cleanup preview.
        for id in ids.into_iter().take(MAX_BATCH) {
            // Busy/changed files are deferred. Do not start a second journal if cleanup is pending.
            self.recover_delete()?;
            if let Ok(path) = self.verified_path(&id) {
                let asset = self.get(&id)?.clone();
                let _ = self.delete_verified(&asset, &path, || true);
            }
        }
        Ok(())
    }
    fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.index)?;
        ensure!(bytes.len() <= 4 * 1024 * 1024, "capture.index_limit");
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(self.root.join("index.json"))?;
        Ok(())
    }
    pub fn publish(
        &mut self,
        source: &Path,
        extension: &str,
        width: u32,
        height: u32,
        duration_ms: u64,
        now: u64,
    ) -> Result<Asset> {
        ensure!(
            ["png", "gif", "mp4"].contains(&extension),
            "capture.invalid_asset"
        );
        let (bytes, sha256) = digest(source)?;
        let asset = Asset {
            id: uuid::Uuid::new_v4().to_string(),
            extension: extension.into(),
            width,
            height,
            duration_ms,
            created_at: now,
            bytes,
            sha256,
            permanent: false,
            name: String::new(),
            kept: false,
            trashed_at: None,
            temporary_since: None,
        };
        asset.validate()?;
        ensure!(
            self.index.assets.len() < 10000
                && self
                    .index
                    .assets
                    .iter()
                    .map(|a| a.bytes)
                    .sum::<u64>()
                    .saturating_add(bytes)
                    <= MAX_TOTAL_BYTES,
            "capture.storage_full"
        );
        let path = self.root.join(format!("{}.{}", asset.id, extension));
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let copy = (|| -> Result<()> {
            let copied = std::io::copy(
                &mut File::open(source)?.take(MAX_ASSET_BYTES + 1),
                &mut output,
            )?;
            ensure!(copied == bytes, "capture.changed");
            output.sync_all()?;
            ensure!(digest(&path)?.1 == asset.sha256, "capture.changed");
            self.index.assets.push(asset.clone());
            self.save()
        })();
        if let Err(error) = copy {
            self.index.assets.retain(|a| a.id != asset.id);
            drop(output);
            let _ = fs::remove_file(path);
            return Err(error);
        }
        Ok(asset)
    }
    fn replace(&mut self, id: &str, update: impl FnOnce(&mut Asset)) -> Result<()> {
        let index = self
            .index
            .assets
            .iter()
            .position(|asset| asset.id == id)
            .ok_or_else(|| anyhow::anyhow!("capture.missing"))?;
        let old = self.index.assets[index].clone();
        update(&mut self.index.assets[index]);
        if let Err(error) = self.index.assets[index]
            .validate()
            .and_then(|()| self.save())
        {
            self.index.assets[index] = old;
            return Err(error);
        }
        Ok(())
    }

    pub fn set_permanent(&mut self, id: &str) -> Result<()> {
        self.replace(id, |asset| {
            asset.permanent = true;
            asset.kept = false;
        })
    }

    pub fn rename(&mut self, item: &Selection, name: &str) -> Result<()> {
        item.validate()?;
        let asset = self.active(&item.id)?;
        ensure!(
            catalog::revision(asset) == item.revision,
            "capture.catalog_stale"
        );
        let name = catalog::normalize_name(name)?;
        self.replace(&item.id, |a| a.name = name)
    }

    pub fn active(&self, id: &str) -> Result<&Asset> {
        let asset = self.get(id)?;
        ensure!(Scope::of(asset) != Scope::Trash, "capture.catalog_trashed");
        Ok(asset)
    }

    pub fn preview(&self, item: &Selection, op: Operation, blocked: bool) -> Outcome {
        catalog::review(self.get(&item.id).ok(), item, op, blocked)
    }

    pub fn apply(
        &mut self,
        item: &Selection,
        op: Operation,
        blocked: bool,
        now: u64,
        allowed: impl Fn() -> bool,
    ) -> Outcome {
        let ready = self.preview(item, op, blocked);
        if ready != Outcome::Ready {
            return ready;
        }
        let id = &item.id;
        if op == Operation::Purge {
            return match self.remove_if(id, allowed) {
                Ok(()) => Outcome::Done,
                Err(error) if error.to_string() == "capture.privacy" => Outcome::Paused,
                Err(error) if error.to_string() == "capture.changed" => Outcome::ChangedFile,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    Outcome::ChangedFile
                }
                Err(_) => Outcome::Failed,
            };
        }
        let result = self.replace(id, |a| match op {
            Operation::Keep => a.kept = true,
            Operation::Release => {
                a.kept = false;
                // Give the user a full temporary retention window after switching protection off.
                a.temporary_since = Some(now);
            }
            Operation::Trash => a.trashed_at = Some(now),
            Operation::Restore => {
                a.trashed_at = None;
                a.temporary_since = Some(now);
            }
            Operation::Purge | Operation::Export => unreachable!(),
        });
        if result.is_ok() {
            Outcome::Done
        } else {
            Outcome::Failed
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.remove_if(id, || true)
    }

    pub fn remove_if(&mut self, id: &str, allowed: impl Fn() -> bool) -> Result<()> {
        let asset = self.get(id)?.clone();
        let path = self.verified_path(id)?;
        ensure!(allowed(), "capture.privacy");
        self.delete_verified(&asset, &path, allowed)
    }

    // A single journal under the index lock makes byte removal recoverable across an
    // interrupted index commit. Before commit, restore the bytes; after commit, finish deleting.
    fn delete_verified(
        &mut self,
        asset: &Asset,
        original: &Path,
        allowed: impl FnOnce() -> bool,
    ) -> Result<()> {
        self.recover_delete()?;
        let marker = self.root.join(DELETE_MARKER);
        let parked = self.root.join(DELETE_STAGE);
        ensure!(!exists(&parked)?, "capture.catalog_delete_pending");
        let mut record = tempfile::NamedTempFile::new_in(&self.root)?;
        record.write_all(&serde_json::to_vec(asset)?)?;
        record.as_file().sync_all()?;
        drop(record.persist_noclobber(&marker)?);
        if let Err(error) = fs::rename(original, &parked) {
            let _ = fs::remove_file(&marker);
            return Err(error.into());
        }
        // Verify the renamed file as well as the pre-rename path. Never delete an
        // externally replaced file that was not the asset described in the journal.
        if digest(&parked).ok().as_ref() != Some(&(asset.bytes, asset.sha256.clone())) {
            if !exists(original)? {
                fs::rename(&parked, original)?;
                fs::remove_file(&marker)?;
            }
            anyhow::bail!("capture.changed");
        }
        if !allowed() {
            self.recover_delete()?;
            anyhow::bail!("capture.privacy");
        }
        let position = self
            .index
            .assets
            .iter()
            .position(|a| a.id == asset.id)
            .expect("indexed asset");
        let removed = self.index.assets.remove(position);
        if let Err(error) = self.save() {
            self.index.assets.insert(position, removed);
            self.recover_delete()?;
            return Err(error);
        }
        if fs::remove_file(&parked).is_ok() {
            let _ = fs::remove_file(&marker);
        }
        Ok(())
    }

    fn recover_delete(&self) -> Result<()> {
        let marker = self.root.join(DELETE_MARKER);
        if !exists(&marker)? {
            return Ok(());
        }
        ensure!(
            regular(&marker)?.len() <= 4096,
            "capture.catalog_delete_pending"
        );
        let mut bytes = Vec::new();
        File::open(&marker)?.take(4097).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4096, "capture.catalog_delete_pending");
        let asset: Asset = serde_json::from_slice(&bytes)?;
        asset.validate()?;
        let original = self.asset_path(&asset);
        let parked = self.root.join(DELETE_STAGE);
        let indexed = self.get(&asset.id).ok();
        if let Some(indexed) = indexed {
            ensure!(
                catalog::revision(&asset) == catalog::revision(indexed),
                "capture.catalog_delete_pending"
            );
        }
        match (indexed.is_some(), exists(&original)?, exists(&parked)?) {
            (true, true, false) | (false, false, false) => {}
            (still_indexed, false, true) => {
                ensure!(
                    digest(&parked)? == (asset.bytes, asset.sha256),
                    "capture.catalog_delete_pending"
                );
                if still_indexed {
                    fs::rename(parked, original)?;
                } else {
                    fs::remove_file(parked)?;
                }
            }
            _ => anyhow::bail!("capture.catalog_delete_pending"),
        }
        fs::remove_file(marker)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abandoned_work_cleanup_leaves_unknown_files_and_library_assets_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("capture");
        fs::create_dir_all(root.join("capture-work-Ab1234")).unwrap();
        fs::write(root.join("capture-work-Ab1234/output.png"), b"synthetic").unwrap();
        fs::create_dir_all(root.join("capture-work-Ab1235")).unwrap();
        fs::write(root.join("capture-work-Ab1235/unknown.txt"), b"keep").unwrap();
        fs::write(root.join("permanent.png"), b"keep").unwrap();
        Store::cleanup_work(&root).unwrap();
        assert!(!root.join("capture-work-Ab1234").exists());
        assert!(root.join("capture-work-Ab1235/unknown.txt").exists());
        assert!(root.join("permanent.png").exists());
    }
    #[test]
    fn capture_assets_are_integrity_checked_and_pruning_honors_leases() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fixture");
        fs::write(&source, b"synthetic pixels").unwrap();
        let root = dir.path().join("capture");
        let mut store = Store::open(&root).unwrap();
        let asset = store.publish(&source, "png", 10, 10, 0, 1).unwrap();
        assert!(store.verified_path(&asset.id).is_ok());
        assert_eq!(
            read_verified(&store.path(&asset.id).unwrap(), &asset).unwrap(),
            b"synthetic pixels"
        );
        store
            .prune_for_space(
                &BTreeSet::from([asset.id.clone()]),
                capture::RETENTION_SECONDS + 1,
                0,
            )
            .unwrap();
        assert_eq!(store.assets().len(), 1);
        fs::write(store.path(&asset.id).unwrap(), b"changed fixture").unwrap();
        assert!(store.verified_path(&asset.id).is_err());
        assert!(read_verified(&store.path(&asset.id).unwrap(), &asset).is_err());
        drop(store);
        fs::write(root.join("index.json"), b"corrupt index fixture").unwrap();
        assert!(Store::open(&root).is_err());
        assert_eq!(
            fs::read(root.join("index.json")).unwrap(),
            b"corrupt index fixture"
        );
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod feature_tests;
