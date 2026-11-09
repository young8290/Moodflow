//! Bounded three-way replica protocol. Host adapters own filesystem/network consent and privacy.
pub mod crypto;
#[cfg(test)]
mod tests;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_BLOB: usize = 16 * 1024 * 1024;
// Capsule documents remain 2 MiB in the host; independently encrypted vault backups may be 12 MiB.
pub const MAX_DOCUMENT: usize = 16 * 1024 * 1024;
pub const MAX_TOTAL: usize = 256 * 1024 * 1024;
pub const MAX_RECORDS: usize = 50_000;
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn id(value: &str) -> Result<()> {
    ensure!(
        uuid::Uuid::parse_str(value).is_ok_and(|v| v.to_string() == value),
        "sync.invalid"
    );
    Ok(())
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub struct Blob {
    pub bytes: Vec<u8>,
    pub revision: String,
}
pub enum Read {
    Missing,
    Deferred,
    Present(Blob),
}
pub enum Expect<'a> {
    Missing,
    Revision(&'a str),
}
pub trait Backend {
    fn get(&mut self, key: &str) -> Result<Read>;
    fn put(&mut self, key: &str, bytes: &[u8], expected: Expect<'_>) -> Result<String>;
}
#[derive(Default)]
pub struct Snapshot {
    pub entries: BTreeMap<String, Vec<u8>>,
    pub deleted: BTreeSet<String>,
    pub deferred: BTreeSet<String>,
}
pub trait Local {
    fn accepts(&self, _bytes: &[u8]) -> bool {
        true
    }
    fn snapshot(&mut self) -> Result<Snapshot>;
    /// Compare against the exact snapshot bytes before replacing or deleting anything.
    fn apply(&mut self, id: &str, bytes: &[u8], expected: Option<&[u8]>) -> Result<()>;
    fn remove(&mut self, id: &str, expected: Option<&[u8]>) -> Result<()>;
    fn conflict(&mut self, id: &str, bytes: &[u8]) -> Result<()>;
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub local: Option<String>,
    pub remote: Option<String>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub dataset: String,
    pub header_hash: String,
    pub baseline: BTreeMap<String, Baseline>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    revision: String,
    object: String,
    bytes: usize,
    deleted: bool,
}
impl Record {
    fn tombstone() -> Self {
        Self {
            revision: "deleted".into(),
            object: String::new(),
            bytes: 0,
            deleted: true,
        }
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            (self.deleted
                && self.revision == "deleted"
                && self.object.is_empty()
                && self.bytes == 0)
                || (!self.deleted
                    && hash(&self.revision)
                    && hash(&self.object)
                    && self.bytes <= MAX_DOCUMENT),
            "sync.invalid"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    dataset: String,
    records: BTreeMap<String, Record>,
}
impl Manifest {
    fn parse(bytes: &[u8], dataset: &str) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BLOB, "sync.limit");
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("sync.invalid"))?;
        ensure!(
            manifest.version == 1 && manifest.dataset == dataset,
            "sync.dataset"
        );
        ensure!(manifest.records.len() <= MAX_RECORDS, "sync.limit");
        let mut total = 0usize;
        for (key, value) in &manifest.records {
            id(key)?;
            value.validate()?;
            total = total.saturating_add(value.bytes);
        }
        ensure!(total <= MAX_TOTAL, "sync.limit");
        Ok(manifest)
    }
}
#[derive(Default, Serialize)]
pub struct Report {
    pub uploaded: usize,
    pub downloaded: usize,
    pub deleted: usize,
    pub conflicts: usize,
    pub deferred: usize,
}
enum Change {
    Pull(String, Vec<u8>, bool),
    Delete(String, bool),
}

/// Header initialization is separate and requires explicit host confirmation for a new target.
pub fn initialize(backend: &mut dyn Backend, phrase: Option<&str>) -> Result<crypto::Header> {
    ensure!(
        matches!(backend.get("config.mfsync")?, Read::Missing),
        "sync.exists"
    );
    let (header, _) = crypto::Header::create(phrase)?;
    backend.put(
        "config.mfsync",
        &serde_json::to_vec(&header)?,
        Expect::Missing,
    )?;
    Ok(header)
}
fn remote_content(
    backend: &mut dyn Backend,
    codec: &crypto::Codec,
    id: &str,
    record: &Record,
) -> Result<Option<Vec<u8>>> {
    let blob = match backend.get(&format!("objects/{}.mfsync", record.object))? {
        Read::Present(blob) => blob,
        _ => return Ok(None),
    };
    ensure!(digest(&blob.bytes) == record.object, "sync.changed");
    let plain = codec.open(&format!("document/{id}"), &blob.bytes)?;
    ensure!(
        plain.len() == record.bytes && digest(&plain) == record.revision,
        "sync.changed"
    );
    Ok(Some(plain.to_vec()))
}
/// Persist `state` even when a later item fails: successfully applied items have already advanced it.
/// `allowed` must be sticky false after pause, sleep or cancellation; do not automatically resume.
pub fn synchronize(
    backend: &mut dyn Backend,
    local: &mut dyn Local,
    state: &mut State,
    phrase: Option<&str>,
    allowed: &dyn Fn() -> bool,
) -> Result<Report> {
    ensure!(allowed(), "sync.cancelled");
    let header_blob = match backend.get("config.mfsync")? {
        Read::Present(blob) => blob,
        Read::Deferred => anyhow::bail!("sync.deferred"),
        Read::Missing => anyhow::bail!("sync.uninitialized"),
    };
    let header = crypto::Header::parse(&header_blob.bytes)?;
    let header_hash = digest(&header_blob.bytes);
    ensure!(
        state.dataset.is_empty()
            || (state.dataset == header.dataset && state.header_hash == header_hash),
        "sync.dataset"
    );
    let codec = header.unlock(phrase)?;
    ensure!(allowed(), "sync.cancelled");
    let (mut manifest, remote_revision) = match backend.get("index.mfsync")? {
        Read::Present(blob) => (
            Manifest::parse(&codec.open("manifest", &blob.bytes)?, &header.dataset)?,
            Some(blob.revision),
        ),
        Read::Missing => {
            ensure!(state.baseline.is_empty(), "sync.remote_incomplete");
            (
                Manifest {
                    version: 1,
                    dataset: header.dataset.clone(),
                    records: BTreeMap::new(),
                },
                None,
            )
        }
        Read::Deferred => anyhow::bail!("sync.deferred"),
    };
    let snapshot = local.snapshot()?;
    ensure!(
        snapshot.entries.len() <= 10_000
            && snapshot.deleted.len() <= MAX_RECORDS
            && state.baseline.len() <= MAX_RECORDS,
        "sync.limit"
    );
    let mut total = 0usize;
    for (key, value) in &snapshot.entries {
        id(key)?;
        ensure!(value.len() <= MAX_DOCUMENT, "sync.limit");
        total = total.saturating_add(value.len());
    }
    ensure!(total <= MAX_TOTAL, "sync.limit");
    let ids: BTreeSet<String> = snapshot
        .entries
        .keys()
        .chain(snapshot.deleted.iter())
        .chain(manifest.records.keys())
        .chain(state.baseline.keys())
        .cloned()
        .collect();
    ensure!(ids.len() <= MAX_RECORDS, "sync.limit");
    let mut report = Report::default();
    let mut changes = vec![];
    let mut updates = BTreeMap::new();
    let mut dirty = remote_revision.is_none();
    let mut incoming = 0usize;
    for key in ids {
        ensure!(allowed(), "sync.cancelled");
        id(&key)?;
        if snapshot.deferred.contains(&key) {
            report.deferred += 1;
            continue;
        }
        let content = snapshot.entries.get(&key);
        let local_hash = content.map(|v| digest(v));
        let base = state.baseline.get(&key).cloned().unwrap_or_default();
        let record = manifest.records.get(&key).cloned();
        if snapshot.deleted.contains(&key) {
            ensure!(content.is_none(), "sync.invalid");
            if let Some(remote) = &record
                && !remote.deleted
                && base.remote.as_deref() != Some(&remote.revision)
            {
                let Some(bytes) = remote_content(backend, &codec, &key, remote)? else {
                    report.deferred += 1;
                    continue;
                };
                ensure!(allowed(), "sync.cancelled");
                local.conflict(&key, &bytes)?;
                report.conflicts += 1;
            }
            if !record.is_some_and(|r| r.deleted) {
                manifest.records.insert(key.clone(), Record::tombstone());
                dirty = true;
                report.deleted += 1;
            }
            updates.insert(
                key,
                Baseline {
                    local: None,
                    remote: Some("deleted".into()),
                },
            );
            continue;
        }
        if let Some(remote) = record {
            if remote.deleted {
                changes.push(Change::Delete(key.clone(), content.is_some()));
                updates.insert(
                    key,
                    Baseline {
                        local: None,
                        remote: Some("deleted".into()),
                    },
                );
                continue;
            }
            if local_hash.as_deref() == Some(&remote.revision) {
                updates.insert(
                    key,
                    Baseline {
                        local: local_hash,
                        remote: Some(remote.revision),
                    },
                );
                continue;
            }
            let local_changed = base.local != local_hash;
            let remote_changed = base.remote.as_deref() != Some(&remote.revision);
            if remote_changed || content.is_none() {
                // A missing local file is never interpreted as deletion or silently resurrected.
                if content.is_none() && base.local.is_some() {
                    report.deferred += 1;
                    continue;
                }
                let Some(bytes) = remote_content(backend, &codec, &key, &remote)? else {
                    report.deferred += 1;
                    continue;
                };
                if !local.accepts(&bytes) {
                    report.deferred += 1;
                    continue;
                }
                incoming = incoming.saturating_add(bytes.len());
                ensure!(incoming <= MAX_TOTAL, "sync.limit");
                changes.push(Change::Pull(
                    key.clone(),
                    bytes,
                    local_changed && content.is_some(),
                ));
                updates.insert(
                    key,
                    Baseline {
                        local: Some(remote.revision.clone()),
                        remote: Some(remote.revision),
                    },
                );
                continue;
            }
            if !local_changed {
                continue;
            }
        } else {
            ensure!(base.remote.is_none(), "sync.remote_incomplete");
        }
        if let Some(content) = content {
            let sealed = codec.seal(&format!("document/{key}"), content)?;
            let object = digest(&sealed);
            ensure!(allowed(), "sync.cancelled");
            let path = format!("objects/{object}.mfsync");
            match backend.get(&path)? {
                Read::Missing => {
                    backend.put(&path, &sealed, Expect::Missing)?;
                }
                Read::Present(blob) => ensure!(blob.bytes == sealed, "sync.changed"),
                Read::Deferred => anyhow::bail!("sync.deferred"),
            }
            let revision = digest(content);
            manifest.records.insert(
                key.clone(),
                Record {
                    revision: revision.clone(),
                    object,
                    bytes: content.len(),
                    deleted: false,
                },
            );
            dirty = true;
            report.uploaded += 1;
            updates.insert(
                key,
                Baseline {
                    local: Some(revision.clone()),
                    remote: Some(revision),
                },
            );
        }
    }
    ensure!(allowed(), "sync.cancelled");
    // No local replacement happens before the remote manifest CAS succeeds.
    if dirty {
        let plain = serde_json::to_vec(&manifest)?;
        Manifest::parse(&plain, &header.dataset)?;
        backend.put(
            "index.mfsync",
            &codec.seal("manifest", &plain)?,
            remote_revision
                .as_deref()
                .map_or(Expect::Missing, Expect::Revision),
        )?;
    } else {
        ensure!(
            matches!(backend.get("index.mfsync")?,Read::Present(blob) if Some(blob.revision.as_str())==remote_revision.as_deref()),
            "sync.conflict"
        );
    }
    state.dataset = header.dataset;
    state.header_hash = header_hash;
    let mut applied = BTreeSet::new();
    for change in changes {
        ensure!(allowed(), "sync.cancelled");
        match change {
            Change::Pull(key, bytes, conflict) => {
                let before = snapshot.entries.get(&key).map(Vec::as_slice);
                if conflict {
                    local.conflict(&key, before.ok_or_else(|| anyhow::anyhow!("sync.invalid"))?)?;
                    report.conflicts += 1;
                }
                ensure!(allowed(), "sync.cancelled");
                local.apply(&key, &bytes, before)?;
                report.downloaded += 1;
                applied.insert(key);
            }
            Change::Delete(key, has_local) => {
                let before = snapshot.entries.get(&key).map(Vec::as_slice);
                if has_local {
                    local.conflict(&key, before.ok_or_else(|| anyhow::anyhow!("sync.invalid"))?)?;
                    report.conflicts += 1;
                }
                ensure!(allowed(), "sync.cancelled");
                local.remove(&key, before)?;
                report.deleted += 1;
                applied.insert(key);
            }
        }
        // Commit only this successful local mutation; a later failure leaves other baselines old.
        for key in std::mem::take(&mut applied) {
            if let Some(base) = updates.remove(&key) {
                state.baseline.insert(key, base);
            }
        }
    }
    state.baseline.extend(updates);
    Ok(report)
}

/// Recover sync-provider conflict copies without guessing a winner from device clocks.
/// Canonical records win; deletions are permanent, and all losing content is preserved locally.
pub fn recover_manifests(
    backend: &mut dyn Backend,
    local: &mut dyn Local,
    state: &mut State,
    phrase: Option<&str>,
    copies: &[Vec<u8>],
    allowed: &dyn Fn() -> bool,
) -> Result<Report> {
    ensure!(allowed(), "sync.cancelled");
    ensure!(copies.len() <= 64, "sync.limit");
    let Read::Present(header_blob) = backend.get("config.mfsync")? else {
        anyhow::bail!("sync.deferred")
    };
    let header = crypto::Header::parse(&header_blob.bytes)?;
    let header_hash = digest(&header_blob.bytes);
    ensure!(
        state.dataset.is_empty()
            || (state.dataset == header.dataset && state.header_hash == header_hash),
        "sync.dataset"
    );
    let codec = header.unlock(phrase)?;
    let (mut canonical, expected) = match backend.get("index.mfsync")? {
        Read::Present(blob) => (
            Manifest::parse(&codec.open("manifest", &blob.bytes)?, &header.dataset)?,
            Some(blob.revision),
        ),
        Read::Missing => (
            Manifest {
                version: 1,
                dataset: header.dataset.clone(),
                records: BTreeMap::new(),
            },
            None,
        ),
        Read::Deferred => anyhow::bail!("sync.deferred"),
    };
    let mut report = Report::default();
    let mut total = 0usize;
    for copy in copies {
        ensure!(allowed(), "sync.cancelled");
        total = total.saturating_add(copy.len());
        ensure!(total <= MAX_TOTAL, "sync.limit");
        let alternative = Manifest::parse(&codec.open("manifest", copy)?, &header.dataset)?;
        for (id, record) in alternative.records {
            ensure!(allowed(), "sync.cancelled");
            if let Some(current) = canonical.records.get(&id) {
                if current.revision == record.revision {
                    continue;
                }
                let loser = if record.deleted {
                    current.clone()
                } else {
                    record.clone()
                };
                if !loser.deleted {
                    let bytes = remote_content(backend, &codec, &id, &loser)?
                        .ok_or_else(|| anyhow::anyhow!("sync.deferred"))?;
                    ensure!(local.accepts(&bytes), "sync.excluded");
                    ensure!(allowed(), "sync.cancelled");
                    local.conflict(&id, &bytes)?;
                    report.conflicts += 1;
                }
                if record.deleted {
                    canonical.records.insert(id, record);
                    report.deleted += 1;
                }
            } else {
                canonical.records.insert(id, record);
                report.downloaded += 1;
            }
        }
    }
    let plain = serde_json::to_vec(&canonical)?;
    Manifest::parse(&plain, &header.dataset)?;
    ensure!(allowed(), "sync.cancelled");
    backend.put(
        "index.mfsync",
        &codec.seal("manifest", &plain)?,
        expected
            .as_deref()
            .map_or(Expect::Missing, Expect::Revision),
    )?;
    state.dataset = header.dataset;
    state.header_hash = header_hash;
    Ok(report)
}
