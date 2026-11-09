//! User-invoked local library actions. No content is sent online or to the input host.
use crate::{error::UiError, state::AppState};
use moodflow_capsule::{Document, Library, MAX_DOCUMENT_BYTES};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tauri::State;

static LIBRARIES: OnceLock<Mutex<BTreeMap<PathBuf, Library>>> = OnceLock::new();

pub(crate) fn save_capture(
    asset: &moodflow_capsule::capture::Asset,
    path: &std::path::Path,
    title: &str,
) -> anyhow::Result<Document> {
    let root = root()?;
    crate::private_fs::ensure(&root)?;
    let mut document = Document::new("capture", title, "", &chrono::Utc::now().to_rfc3339());
    document
        .metadata
        .properties
        .insert("path".into(), serde_yaml_value(&path.to_string_lossy()));
    document
        .metadata
        .properties
        .insert("asset_id".into(), serde_yaml_value(&asset.id));
    // Use the same cache/lock as the library editor; there is one authoritative store.
    let mut libraries = LIBRARIES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let library = libraries
        .entry(root.clone())
        .or_insert_with(|| Library::new(root));
    library.put(&document, None)
}
fn serde_yaml_value(value: &str) -> moodflow_capsule::PropertyValue {
    moodflow_capsule::PropertyValue::String(value.to_owned())
}

fn root() -> anyhow::Result<PathBuf> {
    Ok(crate::paths::tool_data_dir()?.join("capsule"))
}

fn string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing argument"))
}

fn execute(method: &str, p: &Value) -> anyhow::Result<Value> {
    let root = root()?;
    crate::private_fs::ensure(&root)?;
    let mut libraries = LIBRARIES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let library = libraries
        .entry(root.clone())
        .or_insert_with(|| Library::new(root));
    let now = chrono::Utc::now().to_rfc3339();
    match method {
        "list" => {
            if p.get("refresh").and_then(Value::as_bool).unwrap_or(false) {
                library.refresh()?;
            }
            Ok(serde_json::to_value(
                library.search(
                    p.get("query").and_then(Value::as_str).unwrap_or(""),
                    p.get("kind")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty()),
                    500,
                )?,
            )?)
        }
        "new" => Ok(serde_json::to_value(Document::new(
            string(p, "kind")?,
            string(p, "title")?,
            "",
            &now,
        ))?),
        "get" => Ok(serde_json::to_value(library.get(string(p, "id")?)?)?),
        "save" => {
            let mut document: Document = serde_json::from_value(
                p.get("document")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing document"))?,
            )?;
            document.metadata.updated_at = now;
            Ok(serde_json::to_value(library.put(
                &document,
                p.get("expectedRevision").and_then(Value::as_str),
            )?)?)
        }
        "delete" => {
            library.delete(string(p, "id")?, string(p, "revision")?, &now)?;
            Ok(json!({"deleted":true}))
        }
        "import" => {
            let path = PathBuf::from(string(p, "path")?);
            let mut text = String::new();
            std::fs::File::open(&path)?
                .take(MAX_DOCUMENT_BYTES as u64 + 1)
                .read_to_string(&mut text)?;
            anyhow::ensure!(text.len() <= MAX_DOCUMENT_BYTES, "document too large");
            let document = if text.trim_start_matches('\u{feff}').starts_with("---") {
                Document::from_markdown(&text)?
            } else {
                Document::new(
                    "note",
                    &path.file_stem().unwrap_or_default().to_string_lossy(),
                    &text,
                    &now,
                )
            };
            // Import is a preview. Only an explicit Save writes to the library.
            let mut document = document;
            document.revision.clear();
            Ok(serde_json::to_value(document)?)
        }
        "export" => {
            let document = library.get(string(p, "id")?)?;
            let path = PathBuf::from(string(p, "path")?);
            let mut staged = tempfile::NamedTempFile::new_in(
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("no output directory"))?,
            )?;
            staged.write_all(document.to_markdown()?.as_bytes())?;
            staged.as_file().sync_all()?;
            staged.persist_noclobber(path).map_err(|e| e.error)?;
            Ok(json!({"exported":true}))
        }
        _ => anyhow::bail!("unsupported library action"),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn library_request(
    state: State<'_, AppState>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    if state
        .status
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .paused
    {
        return Err(UiError::new("library.privacy", "library.privacy"));
    }
    if payload.len() > MAX_DOCUMENT_BYTES * 3 + 16384 {
        return Err(UiError::new("library.invalid", "library.invalid"));
    }
    let params: Value = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("library.invalid", "library.invalid"))?;
    tauri::async_runtime::spawn_blocking(move || execute(&method, &params).map(|v| v.to_string()))
        .await
        .map_err(|_| UiError::new("library.join", "library.failed"))?
        .map_err(|e| {
            if e.to_string() == "revision_conflict" {
                UiError::new("library.conflict", "library.conflict")
            } else {
                UiError::new("library.operation", "library.failed")
            }
        })
}
