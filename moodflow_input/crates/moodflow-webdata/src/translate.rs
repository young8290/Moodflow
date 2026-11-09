//! Runtime dictionary lookup. Path resolution follows the user's data override rules.
use anyhow::Context;
use moodflow_config::Config;
use moodflow_translate::{Dictionary, MAX_INDEX_BYTES};
use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

struct Cached {
    path: PathBuf,
    modified: Option<SystemTime>,
    length: u64,
    dictionary: Dictionary,
}
static DICTIONARY: OnceLock<Mutex<Option<Cached>>> = OnceLock::new();

pub fn lookup(query: &str) -> anyhow::Result<serde_json::Value> {
    // Reject oversized queries before reading or allocating a dictionary.
    anyhow::ensure!(
        query.encode_utf16().count() <= 512 && !query.contains('\0'),
        "query_limit"
    );
    let data_dir = Config::data_dir();
    let path = Config::resolve_data_file(data_dir.as_deref(), "translate/cedict.mfcd")
        .context("dictionary_missing")?;
    let meta = path.metadata().context("dictionary_missing")?;
    anyhow::ensure!(meta.len() <= MAX_INDEX_BYTES as u64, "dictionary_limit");
    let mut cached = DICTIONARY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if cached.as_ref().is_none_or(|d| {
        d.path != path || d.modified != meta.modified().ok() || d.length != meta.len()
    }) {
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        File::open(&path)?
            .take(MAX_INDEX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let dictionary = Dictionary::from_bytes(&bytes)?;
        *cached = Some(Cached {
            path,
            modified: meta.modified().ok(),
            length: meta.len(),
            dictionary,
        });
    }
    let result = cached
        .as_ref()
        .context("dictionary_missing")?
        .dictionary
        .lookup(query)?;
    Ok(serde_json::to_value(result)?)
}
