//! Atomic, bounded local capture index. Unrecognized/corrupt data is never rebuilt or discarded.
use anyhow::{Result, ensure};
use moodflow_capsule::capture::{self, Asset, MAX_ASSET_BYTES, MAX_TOTAL_BYTES};
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
        Ok(Self {
            root: root.to_owned(),
            _lock: lock,
            index,
        })
    }
    pub fn list(&self) -> Vec<Asset> {
        let mut assets = self.index.assets.clone();
        assets.sort_by_key(|asset| std::cmp::Reverse(asset.created_at));
        assets
    }
    pub fn get(&self, id: &str) -> Result<&Asset> {
        self.index
            .assets
            .iter()
            .find(|asset| asset.id == id)
            .ok_or_else(|| anyhow::anyhow!("capture.missing"))
    }
    pub fn path(&self, id: &str) -> Result<PathBuf> {
        let asset = self.get(id)?;
        Ok(self.root.join(format!("{}.{}", asset.id, asset.extension)))
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
    pub fn prune(&mut self, leased: &BTreeSet<String>, now: u64) -> Result<()> {
        self.prune_for_space(leased, now, 0)
    }
    pub fn prune_for_space(
        &mut self,
        leased: &BTreeSet<String>,
        now: u64,
        incoming: u64,
    ) -> Result<()> {
        let ids = capture::prune_for_space(&self.index.assets, leased, now, incoming)?;
        let mut changed = false;
        for id in ids {
            let path = self.path(&id)?;
            // Busy/unreadable files are deferred, not interpreted as deletions.
            if self.verified_path(&id).is_ok() && fs::remove_file(path).is_ok() {
                self.index.assets.retain(|asset| asset.id != id);
                changed = true;
            }
        }
        if changed {
            self.save()?;
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
    pub fn set_permanent(&mut self, id: &str) -> Result<()> {
        self.get(id)?;
        self.index
            .assets
            .iter_mut()
            .find(|asset| asset.id == id)
            .unwrap()
            .permanent = true;
        self.save()
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        fs::remove_file(self.verified_path(id)?)?;
        self.index.assets.retain(|asset| asset.id != id);
        self.save()
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
            .prune(
                &BTreeSet::from([asset.id.clone()]),
                capture::RETENTION_SECONDS + 1,
            )
            .unwrap();
        assert_eq!(store.list().len(), 1);
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
