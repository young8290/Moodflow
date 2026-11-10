//! Explicitly trusted signed catalog. No network. One atomic policy file binds the source,
//! signed bytes and revision; registry reloads enforce cached revocations even when offline.
use crate::{Index, MAX_PACKAGE_BYTES, Version, hex, unhex, verify_index};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_INDEX_BYTES: usize = 1024 * 1024;
pub const POLICY_FILE: &str = ".moodflow-market.json";
pub const MAX_AGE_SECONDS: i64 = 7 * 86400;
const MAX_POLICY_BYTES: u64 = 2 * 1024 * 1024;
static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub label: String,
    pub url: String,
    pub public_key: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SignedIndex {
    pub json: String,
    pub signature: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Policy {
    format: u32,
    epoch: String,
    pub source: Option<Source>,
    pub index: Option<SignedIndex>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub &'static str);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|_| Error("market.io"))
}

pub fn https_url(text: &str) -> Result<url::Url> {
    let url = url::Url::parse(text).map_err(|_| Error("market.invalid"))?;
    if text.len() > 2048
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|p| p != 443)
    {
        return Err(Error("market.invalid"));
    }
    Ok(url)
}
impl Source {
    pub fn key(&self) -> Result<[u8; 32]> {
        if self.public_key.len() != 64 {
            return Err(Error("market.invalid"));
        }
        let bytes = unhex(&self.public_key)
            .and_then(|v| v.try_into().ok())
            .ok_or(Error("market.invalid"))?;
        ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| Error("market.invalid"))?;
        Ok(bytes)
    }
    pub fn validate(&self) -> Result<()> {
        if self.label.trim().is_empty()
            || self.label.chars().count() > 64
            || self.label.chars().any(char::is_control)
        {
            return Err(Error("market.invalid"));
        }
        https_url(&self.url)?;
        self.key()?;
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<String> {
        Ok(hex(&Sha256::digest(self.key()?)))
    }
}
impl Policy {
    pub fn revision(&self) -> String {
        hex(&Sha256::digest(
            serde_json::to_vec(self).expect("policy serializes"),
        ))
    }
    pub fn verified(&self, now: i64, fresh: bool) -> Result<Option<Index>> {
        let Some(source) = &self.source else {
            if self.index.is_some() {
                return Err(Error("market.invalid"));
            }
            return Ok(None);
        };
        source.validate()?;
        let Some(signed) = &self.index else {
            return Ok(None);
        };
        if signed.json.len() > MAX_INDEX_BYTES {
            return Err(Error("market.too_large"));
        }
        let index = verify_index(signed.json.as_bytes(), &signed.signature, &source.key()?)
            .map_err(|_| Error("market.signature"))?;
        validate_index(&index)?;
        if fresh {
            check_fresh(&index, now)?;
        }
        Ok(Some(index))
    }
}
pub fn check_fresh(index: &Index, now: i64) -> Result<()> {
    let at = generated(index)?;
    if at > now + 300 || now.saturating_sub(at) > MAX_AGE_SECONDS {
        return Err(Error("market.expired"));
    }
    Ok(())
}
fn generated(index: &Index) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(&index.generated_at)
        .map(|d| d.timestamp())
        .map_err(|_| Error("market.invalid"))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && !id.contains("..")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn validate_index(index: &Index) -> Result<()> {
    generated(index)?;
    if index.plugins.len() > 2000 || index.revoked.len() > 10000 {
        return Err(Error("market.too_large"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for item in &index.plugins {
        if !valid_id(&item.id)
            || !ids.insert(&item.id)
            || item.name.chars().count() > 128
            || item.author.chars().count() > 128
            || item.description.chars().count() > 4096
            || item.releases.len() > 100
        {
            return Err(Error("market.invalid"));
        }
        https_url(&item.repo)?;
        let mut versions = std::collections::BTreeSet::new();
        for release in &item.releases {
            if Version::parse(&release.version).is_none()
                || Version::parse(&release.min_host_version).is_none()
                || !versions.insert(&release.version)
                || release.size == 0
                || release.size > MAX_PACKAGE_BYTES
                || release.sha256.len() != 64
                || !release.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || release.platforms.is_empty()
            {
                return Err(Error("market.invalid"));
            }
            https_url(&release.url)?;
        }
    }
    for revoked in &index.revoked {
        if !valid_id(&revoked.id)
            || revoked
                .version
                .as_ref()
                .is_some_and(|v| Version::parse(v).is_none())
            || revoked.reason.chars().count() > 4096
        {
            return Err(Error("market.invalid"));
        }
    }
    Ok(())
}

/// Read without following a link; absence means the user has not configured a catalog.
pub fn load(root: &Path) -> Result<Option<Policy>> {
    let path = root.join(POLICY_FILE);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error("market.io")),
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > MAX_POLICY_BYTES
        || io(path.canonicalize())?.parent() != Some(io(root.canonicalize())?.as_path())
    {
        return Err(Error("market.invalid"));
    }
    let mut bytes = Vec::new();
    io(io(File::open(path))?
        .take(MAX_POLICY_BYTES + 1)
        .read_to_end(&mut bytes))?;
    if bytes.len() as u64 > MAX_POLICY_BYTES {
        return Err(Error("market.too_large"));
    }
    let policy: Policy = serde_json::from_slice(&bytes).map_err(|_| Error("market.invalid"))?;
    if policy.format != 1 {
        return Err(Error("market.invalid"));
    }
    policy.verified(0, false)?;
    Ok(Some(policy))
}
pub fn revision(policy: &Option<Policy>) -> String {
    policy.as_ref().map(Policy::revision).unwrap_or_default()
}
/// Hold through a policy-dependent install to serialize source changes and revocations.
pub fn acquire(root: &Path) -> Result<File> {
    io(std::fs::create_dir_all(root))?;
    let path = root.join(".moodflow-market.lock");
    if let Ok(meta) = std::fs::symlink_metadata(&path)
        && (!meta.is_file()
            || meta.file_type().is_symlink()
            || io(path.canonicalize())?.parent() != Some(io(root.canonicalize())?.as_path()))
    {
        return Err(Error("market.invalid"));
    }
    let lock = io(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path))?;
    lock.try_lock().map_err(|_| Error("market.busy"))?;
    Ok(lock)
}
fn save(root: &Path, policy: &mut Policy) -> Result<()> {
    policy.epoch = format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let bytes = serde_json::to_vec(policy).map_err(|_| Error("market.invalid"))?;
    if bytes.len() as u64 > MAX_POLICY_BYTES {
        return Err(Error("market.too_large"));
    }
    let tmp = root.join(format!(".market-{}.tmp", policy.epoch));
    let result = (|| {
        let mut file = io(OpenOptions::new().write(true).create_new(true).open(&tmp))?;
        io(file.write_all(&bytes))?;
        io(file.sync_all())?;
        drop(file);
        io(std::fs::rename(&tmp, root.join(POLICY_FILE)))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
pub fn configure(root: &Path, expected: &str, source: Option<Source>) -> Result<Policy> {
    if let Some(s) = &source {
        s.validate()?;
    }
    let _lock = acquire(root)?;
    if revision(&load(root)?) != expected {
        return Err(Error("market.source_changed"));
    }
    let mut policy = Policy {
        format: 1,
        epoch: String::new(),
        source,
        index: None,
    };
    save(root, &mut policy)?;
    Ok(policy)
}
pub fn accept(
    root: &Path,
    expected: &str,
    json: String,
    signature: String,
    now: i64,
) -> Result<Policy> {
    if json.len() > MAX_INDEX_BYTES || signature.len() > 256 {
        return Err(Error("market.too_large"));
    }
    let _lock = acquire(root)?;
    let previous = load(root)?;
    if revision(&previous) != expected {
        return Err(Error("market.source_changed"));
    }
    let mut policy = previous.ok_or(Error("market.not_configured"))?;
    if policy.source.is_none() {
        return Err(Error("market.not_configured"));
    }
    let old = policy.verified(now, false)?;
    let old_json = policy.index.as_ref().map(|v| v.json.clone());
    policy.index = Some(SignedIndex { json, signature });
    let new = policy.verified(now, true)?.ok_or(Error("market.invalid"))?;
    if let Some(old) = old
        && (generated(&new)? < generated(&old)?
            || (generated(&new)? == generated(&old)?
                && old_json != policy.index.as_ref().map(|v| v.json.clone()))
            || old.revoked.iter().any(|r| !new.revoked.contains(r)))
    {
        return Err(Error("market.rollback"));
    }
    save(root, &mut policy)?;
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    const NOW: i64 = 1794225600;
    fn fixture(at: i64, revoked: bool) -> String {
        let rev = if revoked {
            json!([{"id":"org.example.demo","version":"1.0.0","reason":"withdrawn"}])
        } else {
            json!([])
        };
        json!({"version":1,"generated_at":chrono::DateTime::from_timestamp(at,0).unwrap().to_rfc3339(),"plugins":[],"revoked":rev}).to_string()
    }
    fn source(key: &SigningKey) -> Source {
        Source {
            label: "Test catalog".into(),
            url: "https://example.com/index.json".into(),
            public_key: hex(key.verifying_key().as_bytes()),
        }
    }
    fn sign(key: &SigningKey, text: &str) -> String {
        hex(&key.sign(text.as_bytes()).to_bytes())
    }
    #[test]
    fn signed_disk_policy_rejects_stale_tampered_expired_and_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let key = SigningKey::from_bytes(&[42; 32]);
        let policy = configure(root, "", Some(source(&key))).unwrap();
        assert_eq!(
            configure(root, "", None).err(),
            Some(Error("market.source_changed"))
        );
        let text = fixture(NOW, true);
        assert!(accept(root, &policy.revision(), text.clone(), "00".repeat(64), NOW).is_err());
        let policy = accept(
            root,
            &policy.revision(),
            text.clone(),
            sign(&key, &text),
            NOW,
        )
        .unwrap();
        let reopened = load(root).unwrap().unwrap();
        assert_eq!(reopened.revision(), policy.revision());
        assert!(
            reopened
                .verified(NOW, true)
                .unwrap()
                .unwrap()
                .is_revoked("org.example.demo", "1.0.0")
        );
        assert!(reopened.verified(NOW + MAX_AGE_SECONDS + 1, true).is_err());
        assert!(
            reopened
                .verified(NOW + MAX_AGE_SECONDS + 1, false)
                .unwrap()
                .unwrap()
                .is_revoked("org.example.demo", "1.0.0")
        );
        for at in [NOW - 1, NOW, NOW + 1] {
            let stale = fixture(at, false);
            assert_eq!(
                accept(
                    root,
                    &policy.revision(),
                    stale.clone(),
                    sign(&key, &stale),
                    NOW
                )
                .err(),
                Some(Error("market.rollback"))
            );
        }
        assert_eq!(load(root).unwrap().unwrap().revision(), policy.revision());
        let future = fixture(NOW + 301, true);
        assert_eq!(
            accept(
                root,
                &policy.revision(),
                future.clone(),
                sign(&key, &future),
                NOW
            )
            .err(),
            Some(Error("market.expired"))
        );
        let text = fixture(NOW + 1, true);
        let next = accept(
            root,
            &policy.revision(),
            text.clone(),
            sign(&key, &text),
            NOW,
        )
        .unwrap();
        assert_ne!(next.revision(), policy.revision());
        let reset = configure(root, &next.revision(), None).unwrap();
        assert!(reset.source.is_none());
        assert!(reset.verified(NOW, true).unwrap().is_none());
    }
    #[test]
    fn invalid_source_and_corrupt_policy_never_become_untrusted_success() {
        let key = SigningKey::from_bytes(&[42; 32]);
        for url in [
            "http://example.com/index.json",
            "https://u:p@example.com/index.json",
            "https://example.com/index.json?token=x",
            "https://example.com:444/index.json",
            "file:///tmp/index.json",
        ] {
            let mut s = source(&key);
            s.url = url.into();
            assert!(s.validate().is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(POLICY_FILE), b"broken").unwrap();
        assert!(load(dir.path()).is_err());
        assert!(configure(dir.path(), "", Some(source(&key))).is_err());
    }
}
