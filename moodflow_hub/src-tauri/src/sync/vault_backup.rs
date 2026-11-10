//! Opaque portable backups only. This module never opens the daily vault or its platform key.
use super::*;
use moodflow_sync::{Local, Snapshot};
use std::{fs, io::Write};
const MAGIC: &[u8] = b"MFVB1\n";
fn wrap(bytes: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(bytes);
    out
}
fn raw<'a>(bytes: &'a [u8], id: &str) -> Result<&'a [u8]> {
    let raw = bytes
        .strip_prefix(MAGIC)
        .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?;
    ensure!(
        moodflow_vault::backup::inspect(raw).is_ok_and(|v| v == id),
        "sync.invalid"
    );
    Ok(raw)
}
fn uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id)
}
fn directory(root: &Path, conflict: bool) -> PathBuf {
    root.join(if conflict {
        "vault-conflicts"
    } else {
        "vault-backups"
    })
}
fn name_valid(name: &str, conflict: bool) -> bool {
    let Some(stem) = name.strip_suffix(".mfvbackup") else {
        return false;
    };
    stem.is_ascii()
        && if conflict {
            stem.len() == 101
                && uuid(&stem[..36])
                && stem.as_bytes()[36] == b'-'
                && stem[37..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        } else {
            uuid(stem)
        }
}
fn quota(root: &Path, incoming: usize) -> Result<()> {
    let mut total = incoming;
    for (n, entry) in fs::read_dir(root)?.enumerate() {
        ensure!(n < 200, "sync.limit");
        total = total.saturating_add(entry?.metadata()?.len() as usize);
    }
    ensure!(total <= 128 * 1024 * 1024, "sync.limit");
    Ok(())
}
pub struct Copies {
    capsule: local::Capsule,
    root: PathBuf,
    include: bool,
    allowed: backend::Policy,
}
impl Copies {
    pub fn new(
        capsule: local::Capsule,
        root: PathBuf,
        include: bool,
        allowed: backend::Policy,
    ) -> Result<Self> {
        for path in [
            directory(&root, false),
            directory(&root, true),
            root.join("vault-deleted"),
        ] {
            crate::private_fs::ensure(&path)?;
        }
        Ok(Self {
            capsule,
            root,
            include,
            allowed,
        })
    }
    pub fn skipped(&self) -> usize {
        self.capsule.skipped
    }
}
impl Local for Copies {
    fn accepts(&self, bytes: &[u8]) -> bool {
        self.include || !bytes.starts_with(MAGIC)
    }
    fn snapshot(&mut self) -> Result<Snapshot> {
        let mut snapshot = self.capsule.snapshot()?;
        let dir = directory(&self.root, false);
        for (n, entry) in fs::read_dir(&dir)?.enumerate() {
            ensure!((self.allowed)(), "sync.cancelled");
            ensure!(n < 200, "sync.limit");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name_valid(&name, false) {
                continue;
            }
            let id = &name[..36];
            if self.capsule.is_deleted(id)
                || self
                    .root
                    .join("vault-deleted")
                    .join(format!("{id}.json"))
                    .exists()
            {
                snapshot.deleted.insert(id.into());
                snapshot.entries.remove(id);
                continue;
            }
            if !self.include {
                snapshot.deferred.insert(id.into());
                continue;
            }
            let bytes = backend::read_bounded(&entry.path(), moodflow_vault::MAX_FILE_BYTES)?;
            ensure!(
                moodflow_vault::backup::inspect(&bytes).is_ok_and(|v| v == id),
                "sync.invalid"
            );
            snapshot.entries.insert(id.into(), wrap(&bytes));
        }
        for (n, entry) in fs::read_dir(self.root.join("vault-deleted"))?.enumerate() {
            ensure!(n < moodflow_sync::MAX_RECORDS, "sync.limit");
            let path = entry?.path();
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|id| uuid(id))
            else {
                continue;
            };
            if self.include {
                snapshot.deleted.insert(id.into());
                snapshot.entries.remove(id);
            } else {
                snapshot.deferred.insert(id.into());
            }
        }
        Ok(snapshot)
    }
    fn apply(&mut self, id: &str, bytes: &[u8], expected: Option<&[u8]>) -> Result<()> {
        if !bytes.starts_with(MAGIC) {
            return self.capsule.apply(id, bytes, expected);
        }
        ensure!(self.include && (self.allowed)(), "sync.vault_opt_in");
        let raw = raw(bytes, id)?;
        ensure!(
            !self.capsule.is_deleted(id)
                && !self
                    .root
                    .join("vault-deleted")
                    .join(format!("{id}.json"))
                    .exists(),
            "sync.local_conflict"
        );
        let dir = directory(&self.root, false);
        let _lock = backend::lock(&self.root.join(".vault-copies.lock"))?;
        let path = dir.join(format!("{id}.mfvbackup"));
        let current = if path.try_exists()? {
            Some(backend::read_bounded(
                &path,
                moodflow_vault::MAX_FILE_BYTES,
            )?)
        } else {
            None
        };
        ensure!(
            current.as_ref().map(|b| wrap(b)).as_deref() == expected,
            "sync.local_conflict"
        );
        quota(&dir, raw.len())?;
        ensure!((self.allowed)(), "sync.cancelled");
        backend::atomic(
            &path,
            raw,
            current
                .as_ref()
                .map(|b| moodflow_sync::digest(b))
                .as_deref(),
        )
    }
    fn remove(&mut self, id: &str, expected: Option<&[u8]>) -> Result<()> {
        let dir = directory(&self.root, false);
        let path = dir.join(format!("{id}.mfvbackup"));
        if !expected.is_some_and(|b| b.starts_with(MAGIC)) && !path.exists() {
            return self.capsule.remove(id, expected);
        }
        ensure!(self.include && (self.allowed)(), "sync.vault_opt_in");
        let _lock = backend::lock(&self.root.join(".vault-copies.lock"))?;
        let current = backend::read_bounded(&path, moodflow_vault::MAX_FILE_BYTES)?;
        ensure!(
            Some(wrap(&current)).as_deref() == expected,
            "sync.local_conflict"
        );
        let tomb = self.root.join("vault-deleted").join(format!("{id}.json"));
        if !tomb.exists() {
            backend::atomic(&tomb, &serde_json::to_vec(&json!({"id":id}))?, None)?;
        }
        ensure!((self.allowed)(), "sync.cancelled");
        fs::remove_file(path)?;
        Ok(())
    }
    fn conflict(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        if !bytes.starts_with(MAGIC) {
            return self.capsule.conflict(id, bytes);
        }
        ensure!(self.include && (self.allowed)(), "sync.vault_opt_in");
        let raw = raw(bytes, id)?;
        let dir = directory(&self.root, true);
        let path = dir.join(format!("{id}-{}.mfvbackup", moodflow_sync::digest(raw)));
        if path.exists() {
            ensure!(
                backend::read_bounded(&path, moodflow_vault::MAX_FILE_BYTES)? == raw,
                "sync.changed"
            );
            return Ok(());
        }
        quota(&dir, raw.len())?;
        backend::atomic(&path, raw, None)
    }
}
pub fn execute(
    root: &Path,
    method: &str,
    params: &Value,
    allowed: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure!(allowed(), "sync.privacy");
    let conflict = params["conflict"].as_bool() == Some(true);
    let dir = directory(root, conflict);
    crate::private_fs::ensure(&dir)?;
    let deleted = root.join("vault-deleted");
    crate::private_fs::ensure(&deleted)?;
    if method == "backup_list" {
        let mut items = vec![];
        for (n, entry) in fs::read_dir(&dir)?.enumerate() {
            ensure!(n < 200, "sync.limit");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name_valid(&name, conflict)
                || (!conflict && deleted.join(format!("{}.json", &name[..36])).exists())
            {
                continue;
            }
            let bytes = backend::read_bounded(&entry.path(), moodflow_vault::MAX_FILE_BYTES)?;
            let id = moodflow_vault::backup::inspect(&bytes)
                .map_err(|_| anyhow::anyhow!("sync.invalid"))?;
            items.push(json!({"id":id,"name":name,"bytes":bytes.len(),"revision":moodflow_sync::digest(&bytes),"conflict":conflict}));
        }
        return Ok(json!({"items":items}));
    }
    let _lock = backend::lock(&root.join(".vault-copies.lock"))?;
    if method == "backup_import" {
        ensure!(
            !conflict && params["confirm"].as_bool() == Some(true),
            "sync.invalid"
        );
        let path = params["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?;
        let bytes = backend::read_bounded(Path::new(path), moodflow_vault::MAX_FILE_BYTES)?;
        let phrase = params["backup_recovery"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sync.recovery"))?;
        drop(
            moodflow_vault::backup::decrypt(&bytes, phrase)
                .map_err(|_| anyhow::anyhow!("sync.recovery"))?,
        );
        let id =
            moodflow_vault::backup::inspect(&bytes).map_err(|_| anyhow::anyhow!("sync.invalid"))?;
        ensure!(
            !deleted.join(format!("{id}.json")).exists()
                && !crate::paths::tool_data_dir()?
                    .join("capsule/tombstones")
                    .join(format!("{id}.json"))
                    .exists(),
            "sync.local_conflict"
        );
        quota(&dir, bytes.len())?;
        ensure!(allowed(), "sync.privacy");
        backend::atomic(&dir.join(format!("{id}.mfvbackup")), &bytes, None)?;
        return Ok(json!({"imported":true,"id":id}));
    }
    let name = params["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?;
    ensure!(name_valid(name, conflict), "sync.invalid");
    let path = dir.join(name);
    let bytes = backend::read_bounded(&path, moodflow_vault::MAX_FILE_BYTES)?;
    let revision = moodflow_sync::digest(&bytes);
    ensure!(
        params["revision"].as_str() == Some(&revision),
        "sync.changed"
    );
    if method == "backup_export" {
        let destination = PathBuf::from(
            params["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?,
        );
        let mut staged = tempfile::NamedTempFile::new_in(
            destination
                .parent()
                .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?,
        )?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        ensure!(allowed(), "sync.privacy");
        staged.persist_noclobber(destination)?;
        return Ok(json!({"exported":true}));
    }
    ensure!(
        method == "backup_delete" && params["confirm"].as_bool() == Some(true),
        "sync.invalid"
    );
    ensure!(allowed(), "sync.privacy");
    if !conflict {
        let tomb = deleted.join(format!("{}.json", &name[..36]));
        if !tomb.exists() {
            backend::atomic(
                &tomb,
                &serde_json::to_vec(&json!({"id":&name[..36]}))?,
                None,
            )?;
        }
    }
    fs::remove_file(path)?;
    Ok(json!({"deleted":true}))
}
