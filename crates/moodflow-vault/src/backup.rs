//! Portable encrypted recovery. No platform-wrapped key or live vault key is exported.
use crate::{Error, Key, KeyWrap, MAX_FILE_BYTES, Result, Snapshot, canonical_id, crypto};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    format: String,
    version: u8,
    id: String,
    key: KeyWrap,
    data: crypto::Sealed,
}
fn context(id: &str) -> String {
    format!("Moodflow vault portable backup v1:{id}")
}
pub fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn encrypt(snapshot: &Snapshot, recovery: &str) -> Result<Vec<u8>> {
    if recovery.chars().count() < 24 {
        return Err(Error::WeakRecovery);
    }
    snapshot.validate()?;
    // A fresh independent key means recovery credentials cannot unlock the live vault file.
    let key = Key::generate()?;
    let id = crate::new_id();
    let wrapping = KeyWrap::create(&key, recovery, &id)?;
    let plain = Zeroizing::new(serde_json::to_vec(snapshot).map_err(|_| Error::Invalid)?);
    let data = crypto::seal(&key, context(&id).as_bytes(), &plain)?;
    let bytes = serde_json::to_vec(&Backup {
        format: "moodflow-vault-backup".into(),
        version: 1,
        id,
        key: wrapping,
        data,
    })
    .map_err(|_| Error::Invalid)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
pub fn decrypt(bytes: &[u8], recovery: &str) -> Result<Snapshot> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::Limit);
    }
    let backup: Backup = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
    if backup.version != 1 || backup.format != "moodflow-vault-backup" {
        return Err(Error::Invalid);
    }
    canonical_id(&backup.id)?;
    let key = backup.key.unlock(recovery, &backup.id)?;
    let plain = crypto::open(&key, context(&backup.id).as_bytes(), &backup.data)?;
    let snapshot: Snapshot = serde_json::from_slice(&plain).map_err(|_| Error::Invalid)?;
    snapshot.validate()?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_backup_is_independent_authenticated_and_roundtrips_secrets() {
        let mut entry = crate::Entry::new();
        entry.title = "offline fixture title".into();
        entry.password = "offline fixture password".into();
        let mut source = Snapshot::empty();
        source.entries.push(entry);
        let phrase = "separate offline recovery phrase fixture";
        let bytes = encrypt(&source, phrase).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("offline fixture"));
        assert!(!text.contains(phrase));
        assert!(!text.contains("protected_key"));
        let restored = decrypt(&bytes, phrase).unwrap();
        assert_eq!(restored.entries[0].password, source.entries[0].password);
        assert_eq!(restored.entries[0].id, source.entries[0].id);
        assert!(decrypt(&bytes, "wrong recovery phrase fixture").is_err());
        assert!(encrypt(&source, "short").is_err());
        let mut corrupt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        corrupt["id"] = crate::new_id().into();
        assert!(decrypt(&serde_json::to_vec(&corrupt).unwrap(), phrase).is_err());
        assert_ne!(
            fingerprint(&bytes),
            fingerprint(&encrypt(&source, phrase).unwrap())
        );
    }
}
