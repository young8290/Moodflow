//! Local conflict review. Never changes a remote replica or deletes a conflict source.
use crate::{Document, Library, MAX_DOCUMENT_BYTES, revision};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub mod diff;
const MAX_FILES: usize = 1000;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
pub fn valid_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    stem.is_ascii()
        && stem.len() == 101
        && uuid::Uuid::parse_str(&stem[..36]).is_ok_and(|id| id.to_string() == stem[..36])
        && stem.as_bytes()[36] == b'-'
        && stem[37..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    ensure!(m.is_file() && !m.file_type().is_symlink(), "sync.invalid");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(m.file_attributes() & 0x400 == 0, "sync.invalid");
    }
    Ok(m)
}
fn directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let m = fs::symlink_metadata(path)?;
    ensure!(m.is_dir() && !m.file_type().is_symlink(), "sync.invalid");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(m.file_attributes() & 0x400 == 0, "sync.invalid");
    }
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        regular(path)?.len() <= MAX_DOCUMENT_BYTES as u64,
        "sync.limit"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_DOCUMENT_BYTES, "sync.limit");
    Ok(bytes)
}
#[derive(Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub id: String,
    pub title: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub revision: String,
}
#[derive(Serialize)]
pub struct Listing {
    pub items: Vec<Entry>,
    pub unreadable: Vec<String>,
    pub total: usize,
}
#[derive(Serialize)]
pub struct FieldChange {
    pub key: String,
    pub current: String,
    pub saved: String,
}
#[derive(Serialize)]
pub struct Review {
    pub name: String,
    pub saved: Document,
    pub source_revision: String,
    pub current: Option<Document>,
    pub current_state: String,
    pub can_replace: bool,
    pub diff: diff::Comparison,
    pub fields: Vec<FieldChange>,
    pub fields_truncated: bool,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Copy,
    Replace,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub name: String,
    pub source_revision: String,
    pub current_revision: Option<String>,
    pub mode: Mode,
    pub operation_id: String,
    pub updated_at: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}
#[derive(Serialize)]
pub struct Resolved {
    pub id: String,
    pub revision: String,
    pub backup: Option<String>,
    pub repeated: bool,
}
pub struct Recovery {
    sources: PathBuf,
    controls: PathBuf,
}
impl Recovery {
    /// The host creates these private, app-owned roots before opening this store.
    pub fn new(sources: PathBuf, controls: PathBuf) -> Self {
        Self { sources, controls }
    }
    fn source(&self, name: &str) -> Result<(Vec<u8>, Document)> {
        ensure!(valid_name(name), "sync.invalid");
        let bytes = read(&self.sources.join(name))?;
        ensure!(revision(&bytes) == name[37..101], "sync.changed");
        let doc = Document::from_markdown(std::str::from_utf8(&bytes)?)?;
        ensure!(
            doc.metadata.id == name[..36]
                && ["note", "skill", "pdf"].contains(&doc.metadata.capsule.as_str()),
            "sync.excluded"
        );
        Ok((bytes, doc))
    }
    pub fn list(&self, query: &str) -> Result<Listing> {
        ensure!(query.encode_utf16().count() <= 256, "sync.limit");
        let terms = query.to_lowercase();
        let mut items = Vec::new();
        let mut unreadable = Vec::new();
        let mut total = 0_u64;
        if !self.sources.exists() {
            return Ok(Listing {
                items,
                unreadable,
                total: 0,
            });
        }
        for (n, entry) in fs::read_dir(&self.sources)?.enumerate() {
            ensure!(n < MAX_FILES, "sync.conflict_limit");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !valid_name(&name) {
                continue;
            }
            total = total.saturating_add(fs::symlink_metadata(entry.path())?.len());
            ensure!(total <= MAX_BYTES, "sync.conflict_limit");
            match self.source(&name) {
                Ok((bytes, doc)) => {
                    let searchable = format!(
                        "{} {} {}",
                        doc.metadata.title,
                        doc.metadata.id,
                        doc.metadata.tags.join(" ")
                    )
                    .to_lowercase();
                    if !terms.split_whitespace().all(|t| searchable.contains(t)) {
                        continue;
                    }
                    items.push(Entry {
                        name,
                        id: doc.metadata.id,
                        title: doc.metadata.title,
                        kind: doc.metadata.capsule,
                        tags: doc.metadata.tags,
                        updated_at: doc.metadata.updated_at,
                        revision: revision(&bytes),
                    });
                }
                Err(_) => unreadable.push(name),
            }
        }
        items.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then(b.updated_at.cmp(&a.updated_at))
                .then(a.name.cmp(&b.name))
        });
        unreadable.sort();
        let total = items.len();
        Ok(Listing {
            items,
            unreadable,
            total,
        })
    }
    pub fn review(&self, library: &Library, name: &str) -> Result<Review> {
        let (bytes, saved) = self.source(name)?;
        let id = &saved.metadata.id;
        let (current, state) = match library.get(id) {
            Ok(doc) => (Some(doc), "available"),
            Err(_)
                if library
                    .root
                    .join("tombstones")
                    .join(format!("{id}.json"))
                    .exists() =>
            {
                (None, "deleted")
            }
            Err(_) if library.root.join(format!("{id}.md")).exists() => (None, "unreadable"),
            Err(_) => (None, "missing"),
        };
        let comparison = diff::compare(
            current.as_ref().map_or("", |d| d.body.as_str()),
            &saved.body,
        );
        let left = current
            .as_ref()
            .map(|d| serde_json::to_value(&d.metadata))
            .transpose()?
            .unwrap_or(serde_json::json!({}));
        let right = serde_json::to_value(&saved.metadata)?;
        let keys = left
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys())
            .chain(right.as_object().into_iter().flat_map(|o| o.keys()))
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut fields = Vec::new();
        let mut changed = 0;
        for key in keys {
            if left.get(&key) == right.get(&key) {
                continue;
            }
            changed += 1;
            if fields.len() < 64 {
                fields.push(FieldChange {
                    current: field_text(left.get(&key)),
                    saved: field_text(right.get(&key)),
                    key,
                });
            }
        }
        let can_replace = current.as_ref().is_some_and(|d| {
            ["note", "skill"].contains(&d.metadata.capsule.as_str())
                && d.metadata.capsule == saved.metadata.capsule
        });
        Ok(Review {
            name: name.into(),
            saved,
            source_revision: revision(&bytes),
            current,
            current_state: state.into(),
            can_replace,
            diff: comparison,
            fields,
            fields_truncated: changed > 64,
        })
    }
    /// Backup the exact current disk bytes before CAS replacement. The saved conflict
    /// stays untouched. A deterministic copy ID makes retrying the same intent safe.
    pub fn resolve(
        &self,
        library: &mut Library,
        request: &Resolution,
        allowed: &dyn Fn() -> bool,
    ) -> Result<Resolved> {
        ensure!(allowed(), "sync.privacy");
        ensure!(
            uuid::Uuid::parse_str(&request.operation_id)
                .is_ok_and(|id| id.to_string() == request.operation_id),
            "sync.invalid"
        );
        ensure!(
            !request.updated_at.is_empty() && request.updated_at.len() <= 64,
            "sync.invalid"
        );
        let _lock = self.lock(allowed)?;
        let (source, saved) = self.source(&request.name)?;
        ensure!(revision(&source) == request.source_revision, "sync.changed");
        let mut backup = None;
        let mut expected = None;
        let mut doc = if request.mode == Mode::Copy {
            ensure!(request.operation_id != saved.metadata.id, "sync.invalid");
            let mut doc = saved.clone();
            doc.metadata.id = request.operation_id.clone();
            doc
        } else {
            let current = library
                .get(&saved.metadata.id)
                .map_err(|_| anyhow::anyhow!("sync.local_conflict"))?;
            ensure!(
                ["note", "skill"].contains(&current.metadata.capsule.as_str())
                    && current.metadata.capsule == saved.metadata.capsule,
                "sync.excluded"
            );
            current
        };
        doc.metadata.title = request.title.clone();
        doc.metadata.tags = request.tags.clone();
        if doc.metadata.capsule != "pdf" {
            doc.body = request.body.clone();
        }
        doc.metadata.updated_at = request.updated_at.clone();
        doc.revision.clear();
        let desired = doc.to_markdown()?;
        let desired_revision = revision(desired.as_bytes());
        if let Ok(current) = library.get(&doc.metadata.id) {
            if current.revision == desired_revision {
                return Ok(Resolved {
                    id: current.metadata.id,
                    revision: current.revision,
                    backup: None,
                    repeated: true,
                });
            }
            ensure!(
                request.mode == Mode::Replace
                    && request.current_revision.as_deref() == Some(&current.revision),
                "sync.local_conflict"
            );
            let raw = library
                .read(&current.metadata.id)?
                .context("sync.local_conflict")?;
            ensure!(
                revision(raw.as_bytes()) == current.revision,
                "sync.local_conflict"
            );
            let name = format!("{}-{}.md", current.metadata.id, current.revision);
            ensure!(allowed(), "sync.privacy");
            self.backup(&name, raw.as_bytes())?;
            backup = Some(name);
            expected = Some(current.revision);
        } else {
            ensure!(request.mode == Mode::Copy, "sync.local_conflict");
        }
        ensure!(allowed(), "sync.privacy");
        ensure!(self.source(&request.name)?.0 == source, "sync.changed");
        let saved = library.put(&doc, expected.as_deref()).map_err(|e| {
            if e.to_string() == "revision_conflict" {
                anyhow::anyhow!("sync.local_conflict")
            } else {
                e
            }
        })?;
        Ok(Resolved {
            id: saved.metadata.id,
            revision: saved.revision,
            backup,
            repeated: false,
        })
    }
    pub fn delete_source(
        &self,
        name: &str,
        expected: &str,
        allowed: &dyn Fn() -> bool,
    ) -> Result<()> {
        ensure!(allowed(), "sync.privacy");
        let _lock = self.lock(allowed)?;
        let (bytes, _) = self.source(name)?;
        ensure!(revision(&bytes) == expected, "sync.changed");
        ensure!(allowed(), "sync.privacy");
        fs::remove_file(self.sources.join(name))?;
        Ok(())
    }
    fn lock(&self, allowed: &dyn Fn() -> bool) -> Result<fs::File> {
        directory(&self.controls)?;
        let lock_path = self.controls.join(".recovery.lock");
        if lock_path.exists() {
            regular(&lock_path)?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        let started = std::time::Instant::now();
        loop {
            ensure!(allowed(), "sync.privacy");
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock)
                    if started.elapsed() < std::time::Duration::from_secs(2) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!("sync.busy"),
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        Ok(lock)
    }
    fn backup(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let root = self.controls.join("backups");
        directory(&root)?;
        ensure!(
            !fs::symlink_metadata(&root)?.file_type().is_symlink(),
            "sync.invalid"
        );
        let path = root.join(name);
        if path.exists() {
            ensure!(read(&path)? == bytes, "sync.changed");
            return Ok(());
        }
        let mut total = bytes.len() as u64;
        for (n, entry) in fs::read_dir(&root)?.enumerate() {
            ensure!(n + 1 < MAX_FILES, "sync.conflict_limit");
            total = total.saturating_add(regular(&entry?.path())?.len());
        }
        ensure!(total <= MAX_BYTES, "sync.conflict_limit");
        let mut staged = tempfile::NamedTempFile::new_in(&root)?;
        staged.write_all(bytes)?;
        staged.as_file().sync_all()?;
        staged.persist_noclobber(path).map_err(|e| e.error)?;
        Ok(())
    }
}
fn field_text(value: Option<&serde_json::Value>) -> String {
    value
        .map(|v| {
            serde_json::to_string(v)
                .unwrap_or_default()
                .chars()
                .take(2048)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod recovery_tests;
