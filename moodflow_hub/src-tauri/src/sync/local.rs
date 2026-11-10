use super::*;
use moodflow_capsule::{Document, Library};
use moodflow_sync::{Local, Snapshot};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub struct Capsule {
    root: PathBuf,
    conflicts: PathBuf,
    library: Library,
    revisions: BTreeMap<String, String>,
    pub skipped: usize,
    allowed: backend::Policy,
}
pub fn portable(document: &Document) -> Result<Document> {
    ensure!(
        ["note", "skill", "pdf"].contains(&document.metadata.capsule.as_str()),
        "sync.excluded"
    );
    document.validate()?;
    let mut copy = document.clone();
    copy.revision.clear();
    // Resource bodies are local paths. Only bibliographic metadata crosses devices.
    if copy.metadata.capsule == "pdf" {
        copy.body.clear();
    }
    copy.metadata
        .properties
        .retain(|key, _| key.starts_with("reference_"));
    copy.metadata.properties = copy
        .metadata
        .properties
        .into_iter()
        .filter_map(|(key, value)| clean_property(value).map(|v| (key, v)))
        .collect();
    Ok(copy)
}
fn clean_property(
    value: moodflow_capsule::PropertyValue,
) -> Option<moodflow_capsule::PropertyValue> {
    use moodflow_capsule::PropertyValue as V;
    match value {
        V::String(ref s)
            if s.starts_with("file:")
                || s.starts_with("\\\\")
                || (s.as_bytes().get(1) == Some(&b':')
                    && s.as_bytes()
                        .get(2)
                        .is_some_and(|b| *b == b'\\' || *b == b'/')) =>
        {
            None
        }
        V::Sequence(values) => Some(V::Sequence(
            values.into_iter().filter_map(clean_property).collect(),
        )),
        V::Mapping(values) => Some(V::Mapping(
            values
                .into_iter()
                .filter(|(k, _)| {
                    k.as_str().is_none_or(|s| {
                        !s.ends_with("_path")
                            && !["path", "file", "filename", "attachment", "attachments"]
                                .contains(&s)
                    })
                })
                .filter_map(|(k, v)| clean_property(v).map(|v| (k, v)))
                .collect(),
        )),
        V::Tagged(value) => clean_property(value.value),
        other => Some(other),
    }
}
fn parsed(bytes: &[u8], id: &str) -> Result<Document> {
    let document = Document::from_markdown(std::str::from_utf8(bytes)?)?;
    ensure!(document.metadata.id == id, "sync.invalid");
    ensure!(
        portable(&document)?.to_markdown()?.as_bytes() == bytes,
        "sync.excluded"
    );
    Ok(document)
}
pub fn conflict_name(name: &str) -> bool {
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
impl Capsule {
    pub fn is_deleted(&self, id: &str) -> bool {
        self.root
            .join("tombstones")
            .join(format!("{id}.json"))
            .exists()
    }
    pub fn open(root: PathBuf, conflicts: PathBuf, allowed: backend::Policy) -> Result<Self> {
        crate::private_fs::ensure(&root)?;
        crate::private_fs::ensure(&root.join("tombstones"))?;
        crate::private_fs::ensure(&conflicts)?;
        Ok(Self {
            library: Library::new(root.clone()),
            root,
            conflicts,
            revisions: BTreeMap::new(),
            skipped: 0,
            allowed,
        })
    }
    fn current(&self, id: &str, expected: Option<&[u8]>) -> Result<Option<Document>> {
        let document = match fs::symlink_metadata(self.root.join(format!("{id}.md"))) {
            Ok(_) => Some(self.library.get(id)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        ensure!(
            document.as_ref().map(|d| d.revision.as_str())
                == self.revisions.get(id).map(String::as_str),
            "sync.local_conflict"
        );
        let current = document
            .as_ref()
            .map(|d| portable(d).and_then(|d| d.to_markdown()))
            .transpose()?;
        ensure!(
            current.as_ref().map(|s| s.as_bytes()) == expected,
            "sync.local_conflict"
        );
        Ok(document)
    }
}
impl Local for Capsule {
    fn snapshot(&mut self) -> Result<Snapshot> {
        let mut out = Snapshot::default();
        let mut total = 0usize;
        for (n, entry) in fs::read_dir(&self.root)?.enumerate() {
            ensure!((self.allowed)(), "sync.cancelled");
            ensure!(n < 20_000, "sync.limit");
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|s| s != "md") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == *s))
            else {
                continue;
            };
            let document = match self.library.get(id) {
                Ok(d) => d,
                Err(_) => {
                    out.deferred.insert(id.into());
                    self.skipped += 1;
                    continue;
                }
            };
            if !["note", "skill", "pdf"].contains(&document.metadata.capsule.as_str()) {
                out.deferred.insert(id.into());
                self.skipped += 1;
                continue;
            }
            let bytes = portable(&document)?.to_markdown()?.into_bytes();
            total = total.saturating_add(bytes.len());
            ensure!(total <= moodflow_sync::MAX_TOTAL, "sync.limit");
            self.revisions.insert(id.into(), document.revision);
            out.entries.insert(id.into(), bytes);
        }
        for (n, entry) in fs::read_dir(self.root.join("tombstones"))?.enumerate() {
            ensure!(n < moodflow_sync::MAX_RECORDS, "sync.limit");
            let path = entry?.path();
            if path.extension().is_none_or(|s| s != "json") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == *s))
            else {
                continue;
            };
            let bytes = backend::read_bounded(&path, 65536)?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            ensure!(value["id"] == id, "sync.invalid");
            out.deleted.insert(id.into());
            out.entries.remove(id);
            out.deferred.remove(id);
        }
        Ok(out)
    }
    fn apply(&mut self, id: &str, bytes: &[u8], expected: Option<&[u8]>) -> Result<()> {
        ensure!((self.allowed)(), "sync.cancelled");
        let mut incoming = parsed(bytes, id)?;
        let current = self.current(id, expected)?;
        if let Some(current) = &current {
            if incoming.metadata.capsule == "pdf" {
                incoming.body = current.body.clone();
            }
            for (key, value) in &current.metadata.properties {
                if !key.starts_with("reference_") {
                    incoming
                        .metadata
                        .properties
                        .insert(key.clone(), value.clone());
                }
            }
        }
        ensure!((self.allowed)(), "sync.cancelled");
        self.library
            .put(&incoming, current.as_ref().map(|d| d.revision.as_str()))?;
        Ok(())
    }
    fn remove(&mut self, id: &str, expected: Option<&[u8]>) -> Result<()> {
        ensure!((self.allowed)(), "sync.cancelled");
        let current = self.current(id, expected)?;
        let now = chrono::Utc::now().to_rfc3339();
        ensure!((self.allowed)(), "sync.cancelled");
        if let Some(current) = current {
            self.library.delete(id, &current.revision, &now)?;
        } else {
            self.library.record_remote_deletion(id, &now)?;
        }
        Ok(())
    }
    fn conflict(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        ensure!((self.allowed)(), "sync.cancelled");
        parsed(bytes, id)?;
        let name = format!("{id}-{}.md", moodflow_sync::digest(bytes));
        ensure!(conflict_name(&name), "sync.invalid");
        let path = self.conflicts.join(name);
        if path.exists() {
            ensure!(
                backend::read_bounded(&path, moodflow_sync::MAX_DOCUMENT)? == bytes,
                "sync.changed"
            );
            return Ok(());
        }
        let mut total = bytes.len();
        for (n, entry) in fs::read_dir(&self.conflicts)?.enumerate() {
            ensure!(n < 1000, "sync.conflict_limit");
            total = total.saturating_add(entry?.metadata()?.len() as usize);
        }
        ensure!(total <= moodflow_sync::MAX_TOTAL, "sync.conflict_limit");
        backend::atomic(&path, bytes, None)
    }
}
pub fn conflicts(root: &Path) -> Result<serde_json::Value> {
    crate::private_fs::ensure(root)?;
    let mut items = vec![];
    for (n, entry) in fs::read_dir(root)?.enumerate() {
        ensure!(n < 1000, "sync.conflict_limit");
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !conflict_name(&name) {
            continue;
        }
        let bytes = backend::read_bounded(&entry.path(), moodflow_sync::MAX_DOCUMENT)?;
        let document = parsed(&bytes, &name[..36])?;
        items.push(serde_json::json!({"name":name,"title":document.metadata.title,"kind":document.metadata.capsule,"updated_at":document.metadata.updated_at}));
    }
    Ok(serde_json::json!({"items":items}))
}
