use crate::{
    Error, Key, Result, Snapshot, canonical_id,
    crypto::{self, Sealed},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;

pub const MAX_FILE_BYTES: usize = 12 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedEntry {
    id: String,
    data: Sealed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedSnapshot {
    version: u8,
    entries: Vec<SealedEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    version: u8,
    pub id: String,
    /// OS-protected KeyWrap. Never an unprotected key or password verifier.
    pub protected_key: Vec<u8>,
    data: Sealed,
}
impl FileRecord {
    pub fn create(id: String, protected_key: Vec<u8>, key: &Key) -> Result<Self> {
        canonical_id(&id)?;
        if protected_key.is_empty() || protected_key.len() > 65536 {
            return Err(Error::Invalid);
        }
        let data = encrypt_snapshot(&id, key, &Snapshot::empty())?;
        Ok(Self {
            version: 1,
            id,
            protected_key,
            data,
        })
    }
    pub fn decrypt(&self, key: &Key) -> Result<Snapshot> {
        self.validate()?;
        let plain = crypto::open(key, context(&self.id).as_bytes(), &self.data)?;
        let stored: SealedSnapshot = serde_json::from_slice(&plain).map_err(|_| Error::Invalid)?;
        if stored.version != 1 || stored.entries.len() > 1000 {
            return Err(Error::Limit);
        }
        let mut entries = Vec::with_capacity(stored.entries.len());
        for sealed in stored.entries {
            canonical_id(&sealed.id)?;
            let plain = crypto::open(
                key,
                entry_context(&self.id, &sealed.id).as_bytes(),
                &sealed.data,
            )?;
            let entry: crate::Entry = serde_json::from_slice(&plain).map_err(|_| Error::Invalid)?;
            if entry.id != sealed.id {
                return Err(Error::Crypt);
            }
            entries.push(entry);
        }
        let snapshot = Snapshot {
            version: 1,
            entries,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    fn validate(&self) -> Result<()> {
        canonical_id(&self.id)?;
        if self.version != 1 || self.protected_key.is_empty() || self.protected_key.len() > 65536 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
fn context(id: &str) -> String {
    format!("Moodflow vault snapshot v1:{id}")
}
fn entry_context(vault_id: &str, id: &str) -> String {
    format!("Moodflow vault entry v1:{vault_id}:{id}")
}
fn encrypt_snapshot(id: &str, key: &Key, snapshot: &Snapshot) -> Result<Sealed> {
    snapshot.validate()?;
    let mut entries = Vec::with_capacity(snapshot.entries.len());
    for entry in &snapshot.entries {
        let plain = Zeroizing::new(serde_json::to_vec(entry).map_err(|_| Error::Invalid)?);
        entries.push(SealedEntry {
            id: entry.id.clone(),
            data: crypto::seal(key, entry_context(id, &entry.id).as_bytes(), &plain)?,
        });
    }
    let plain = Zeroizing::new(
        serde_json::to_vec(&SealedSnapshot {
            version: 1,
            entries,
        })
        .map_err(|_| Error::Invalid)?,
    );
    crypto::seal(key, context(id).as_bytes(), &plain)
}
fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct Store {
    root: PathBuf,
}
impl Store {
    /// The host must create and protect this directory before calling the store.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn exists(&self) -> Result<bool> {
        match fs::symlink_metadata(self.path()) {
            Ok(meta)
                if meta.file_type().is_file()
                    && !meta.file_type().is_symlink()
                    && meta.len() <= MAX_FILE_BYTES as u64 =>
            {
                Ok(true)
            }
            Ok(_) => Err(Error::Invalid),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.join("vault.mfv")
    }
    fn bytes(&self) -> Result<Option<Vec<u8>>> {
        let path = self.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !meta.file_type().is_file() || meta.file_type().is_symlink() {
            return Err(Error::Invalid);
        }
        if meta.len() > MAX_FILE_BYTES as u64 {
            return Err(Error::Limit);
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        File::open(path)?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(Error::Limit);
        }
        Ok(Some(bytes))
    }
    pub fn read(&self) -> Result<(FileRecord, String)> {
        let bytes = self.bytes()?.ok_or(Error::NotFound)?;
        let record: FileRecord = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        record.validate()?;
        Ok((record, revision(&bytes)))
    }
    pub fn write(
        &self,
        mut record: FileRecord,
        key: &Key,
        snapshot: &Snapshot,
        expected: Option<&str>,
    ) -> Result<String> {
        record.validate()?;
        let lock_path = self.root.join(".vault.lock");
        if fs::symlink_metadata(&lock_path)
            .is_ok_and(|m| !m.file_type().is_file() || m.file_type().is_symlink())
        {
            return Err(Error::Invalid);
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.lock()?;
        let check = || -> Result<()> {
            if self.bytes()?.as_deref().map(revision).as_deref() != expected {
                return Err(Error::Conflict);
            }
            Ok(())
        };
        check()?;
        record.data = encrypt_snapshot(&record.id, key, snapshot)?;
        let bytes = serde_json::to_vec(&record).map_err(|_| Error::Invalid)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(Error::Limit);
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.root)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        check()?;
        if expected.is_none() {
            staged.persist_noclobber(self.path()).map_err(|e| e.error)?;
        } else {
            staged.persist(self.path()).map_err(|e| e.error)?;
        }
        Ok(revision(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;
    #[test]
    fn encrypted_snapshot_is_atomic_conflict_checked_and_contains_no_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_owned());
        let key = Key::generate().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let record = FileRecord::create(id.clone(), vec![1, 2, 3], &key).unwrap();
        let mut snapshot = Snapshot::empty();
        let mut entry = Entry::new();
        entry.title = "private-test-title".into();
        entry.password = "private-test-password".into();
        snapshot.entries.push(entry);
        let first = store.write(record, &key, &snapshot, None).unwrap();
        let disk = fs::read_to_string(store.path()).unwrap();
        assert!(!disk.contains("private-test-title"));
        assert!(!disk.contains("private-test-password"));
        let (record, revision) = store.read().unwrap();
        assert_eq!(first, revision);
        assert_eq!(
            record.decrypt(&key).unwrap().entries[0].password,
            "private-test-password"
        );
        let other = Store::new(dir.path().to_owned());
        let second = other
            .write(record, &key, &Snapshot::empty(), Some(&first))
            .unwrap();
        assert_ne!(first, second);
        let (record, _) = store.read().unwrap();
        assert!(matches!(
            store.write(record, &key, &snapshot, Some(&first)),
            Err(Error::Conflict)
        ));
        assert_eq!(store.read().unwrap().1, second);
        let (mut record, _) = store.read().unwrap();
        record.id = uuid::Uuid::new_v4().to_string();
        assert!(record.decrypt(&key).is_err());
        fs::write(store.path(), b"truncated").unwrap();
        assert!(store.read().is_err());
        assert_eq!(fs::read(store.path()).unwrap(), b"truncated");
    }
}
