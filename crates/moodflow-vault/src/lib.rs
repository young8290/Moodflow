//! Encrypted vault snapshots. OS key protection, session gates and clipboard are host concerns.
pub mod crypto;
mod store;
pub mod totp;
pub use crypto::{Key, KeyWrap};
pub use store::{FileRecord, MAX_FILE_BYTES, Store};
pub use totp::{Algorithm, Totp};
pub mod session;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("vault_invalid")]
    Invalid,
    #[error("vault_limit")]
    Limit,
    #[error("vault_crypt")]
    Crypt,
    #[error("vault_random")]
    Random,
    #[error("vault_conflict")]
    Conflict,
    #[error("vault_not_found")]
    NotFound,
    #[error("vault_weak_password")]
    WeakPassword,
    #[error("vault_locked")]
    Locked,
    #[error("vault_io")]
    Io(#[from] std::io::Error),
}

/// No Debug or Clone: credential fields must not accidentally enter diagnostics.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub account: String,
    pub password: String,
    pub url: String,
    pub notes: String,
    #[serde(default)]
    pub totp: Option<Totp>,
}
impl Entry {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            account: String::new(),
            password: String::new(),
            url: String::new(),
            notes: String::new(),
            totp: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        canonical_id(&self.id)?;
        if self.title.trim().is_empty() || self.title.chars().count() > 256 {
            return Err(Error::Invalid);
        }
        if [
            &self.title,
            &self.account,
            &self.password,
            &self.url,
            &self.notes,
        ]
        .iter()
        .any(|s| s.len() > 65536 || s.contains('\0'))
        {
            return Err(Error::Limit);
        }
        if let Some(totp) = &self.totp {
            totp.validate()?;
        }
        Ok(())
    }
}
impl Default for Entry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    version: u8,
    pub entries: Vec<Entry>,
}
impl Snapshot {
    pub fn empty() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.entries.len() > 1000 {
            return Err(Error::Limit);
        }
        let mut ids = std::collections::BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(&entry.id) {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
}
fn canonical_id(id: &str) -> Result<()> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| Error::Invalid)?;
    if parsed.to_string() != id {
        return Err(Error::Invalid);
    }
    Ok(())
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
