use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct Store {
    root: PathBuf,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "workspace.invalid_file"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "workspace.invalid_file"
        );
    }
    Ok(metadata)
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        valid_id(id)?;
        Ok(self.root.join(format!("{id}.json")))
    }
    fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.root)?;
        let path = self.root.join(".workspace.lock");
        if path.exists() {
            regular(&path)?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock)
                    if started.elapsed() < std::time::Duration::from_secs(2) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!("workspace.busy"),
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        Ok(file)
    }
    pub fn get(&self, id: &str) -> Result<Session> {
        let path = self.path(id)?;
        ensure!(
            !self.root.join(format!("{id}.deleted")).exists(),
            "workspace.missing"
        );
        ensure!(
            regular(&path)?.len() <= MAX_SESSION_BYTES,
            "workspace.limit"
        );
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_SESSION_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= MAX_SESSION_BYTES, "workspace.limit");
        let mut session: Session = serde_json::from_slice(&bytes)?;
        session.validate()?;
        ensure!(session.id == id, "workspace.invalid");
        session.revision = digest(&bytes);
        Ok(session)
    }
    pub fn list(&self) -> Result<Vec<Session>> {
        if !self.root.exists() {
            return Ok(vec![]);
        }
        let mut sessions = Vec::new();
        for (index, entry) in fs::read_dir(&self.root)?.enumerate() {
            ensure!(index < 20000, "workspace.limit");
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let id = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            valid_id(&id)?;
            if self.root.join(format!("{id}.deleted")).exists() {
                continue;
            }
            sessions.push(self.get(&id)?);
            ensure!(sessions.len() <= MAX_SESSIONS, "workspace.limit");
        }
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(sessions)
    }
    pub fn save(&self, session: &Session, expected: Option<&str>) -> Result<Session> {
        session.validate()?;
        let _lock = self.lock()?;
        let path = self.path(&session.id)?;
        ensure!(
            !self.root.join(format!("{}.deleted", session.id)).exists(),
            "workspace.missing"
        );
        let previous = if path.exists() {
            Some(self.get(&session.id)?)
        } else {
            None
        };
        ensure!(
            previous.as_ref().map(|s| s.revision.as_str()) == expected,
            "workspace.conflict"
        );
        if let Some(previous) = &previous {
            ensure!(
                previous.kind == session.kind && previous.created_at == session.created_at,
                "workspace.invalid"
            );
            if previous.model.is_some() {
                ensure!(previous.model == session.model, "workspace.model_changed");
                ensure!(
                    previous.instructions == session.instructions,
                    "workspace.model_changed"
                );
            }
        }
        let bytes = serde_json::to_vec(session)?;
        ensure!(bytes.len() as u64 <= MAX_SESSION_BYTES, "workspace.limit");
        let current = self.list()?;
        ensure!(
            previous.is_some() || current.len() < MAX_SESSIONS,
            "workspace.limit"
        );
        let mut total = bytes.len() as u64;
        for item in current.iter().filter(|s| s.id != session.id) {
            total += regular(&self.path(&item.id)?)?.len();
        }
        ensure!(total <= MAX_TOTAL_BYTES, "workspace.limit");
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path)?;
        let mut saved = session.clone();
        saved.revision = digest(&bytes);
        Ok(saved)
    }
    pub fn remove(&self, id: &str, expected: &str) -> Result<()> {
        let _lock = self.lock()?;
        let session = self.get(id)?;
        ensure!(session.revision == expected, "workspace.conflict");
        let mut marker = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.root.join(format!("{id}.deleted")))?;
        marker.write_all(b"v1\n")?;
        marker.sync_all()?;
        fs::remove_file(self.path(id)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_windows_cannot_overwrite_or_resurrect_a_deleted_session() {
        let dir = tempfile::tempdir().unwrap();
        let a = Store::new(dir.path().join("workspace"));
        let b = Store::new(dir.path().join("workspace"));
        let original = a
            .save(&Session::new(Kind::Inbox, "fixture".into(), 1), None)
            .unwrap();
        let stale = b.get(&original.id).unwrap();
        let mut changed = original.clone();
        changed
            .append(
                vec![Message::new(
                    Role::External,
                    "explicit external message".into(),
                    2,
                )],
                None,
                2,
            )
            .unwrap();
        let saved = a.save(&changed, Some(&original.revision)).unwrap();
        assert!(b.save(&stale, Some(&stale.revision)).is_err());
        assert!(b.remove(&saved.id, &stale.revision).is_err());
        a.remove(&saved.id, &saved.revision).unwrap();
        assert!(a.list().unwrap().is_empty());
        assert!(b.save(&original, None).is_err());
        assert!(b.get("../escape").is_err());
    }
}
