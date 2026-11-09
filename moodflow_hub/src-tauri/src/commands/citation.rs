//! Explicit bibliography actions. Imports and DOI results are previews, never library writes.
use crate::{error::UiError, state::AppState};
use moodflow_cite::{MAX_IMPORT_BYTES, Reference};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
};
use tauri::State;

fn execute(method: &str, p: &Value) -> anyhow::Result<Value> {
    match method {
        "format" => {
            let reference: Reference = serde_json::from_value(p["reference"].clone())?;
            let missing = reference.missing();
            if !missing.is_empty() {
                return Ok(json!({"missing":missing, "text":""}));
            }
            let text =
                moodflow_cite::format(&reference, p["style"].as_str().unwrap_or("gbt-compatible"))?;
            Ok(json!({"missing":[],"text":text}))
        }
        "import" => {
            let path = PathBuf::from(
                p["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid path"))?,
            );
            let mut text = String::new();
            std::fs::File::open(&path)?
                .take(MAX_IMPORT_BYTES as u64 + 1)
                .read_to_string(&mut text)?;
            anyhow::ensure!(text.len() <= MAX_IMPORT_BYTES, "reference_limit");
            let references = if path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("bib"))
            {
                moodflow_cite::import_bibtex(&text)?
            } else {
                moodflow_cite::import_csl(&text)?
            };
            Ok(json!({"references":references}))
        }
        "export" => {
            let reference: Reference = serde_json::from_value(p["reference"].clone())?;
            let path = PathBuf::from(
                p["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid path"))?,
            );
            let text = if p["format"].as_str() == Some("bibtex") {
                moodflow_cite::export_bibtex(&reference)?
            } else {
                serde_json::to_string_pretty(&json!([reference.to_csl()?]))?
            };
            let mut staged = tempfile::NamedTempFile::new_in(
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("no output directory"))?,
            )?;
            staged.write_all(text.as_bytes())?;
            staged.as_file().sync_all()?;
            staged.persist_noclobber(path).map_err(|e| e.error)?;
            Ok(json!({"exported":true}))
        }
        _ => anyhow::bail!("unsupported citation action"),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn citation_request(
    state: State<'_, AppState>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    let paused = || {
        state
            .status
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .paused
    };
    if paused() {
        return Err(UiError::new("citation.privacy", "library.privacy"));
    }
    if payload.len() > MAX_IMPORT_BYTES * 2 {
        return Err(UiError::new("citation.invalid", "citation.invalid"));
    }
    let p: Value = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("citation.invalid", "citation.invalid"))?;
    if method == "doi" {
        if p["consent"].as_bool() != Some(true) {
            return Err(UiError::new("citation.consent", "citation.consent"));
        }
        let doi = p["doi"]
            .as_str()
            .ok_or_else(|| UiError::new("citation.invalid", "citation.invalid"))?;
        let reference = moodflow_hub_gateway::doi::lookup(doi)
            .await
            .map_err(|_| UiError::new("citation.doi_failed", "citation.doi_failed"))?;
        if paused() {
            return Err(UiError::new("citation.privacy", "library.privacy"));
        }
        return Ok(json!({"reference":reference}).to_string());
    }
    let result =
        tauri::async_runtime::spawn_blocking(move || execute(&method, &p).map(|v| v.to_string()))
            .await
            .map_err(|_| UiError::new("citation.join", "citation.failed"))?
            .map_err(|_| UiError::new("citation.operation", "citation.failed"))?;
    if paused() {
        return Err(UiError::new("citation.privacy", "library.privacy"));
    }
    Ok(result)
}
