//! Dashboard-only local catalog operations. Only index metadata enters batch reports.
use super::store::{Store, digest};
use anyhow::{Result, ensure};
use moodflow_capsule::capture::{
    Asset, RETENTION_SECONDS,
    catalog::{self, MAX_BATCH, Operation, Outcome, Query, Scope, Selection},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const MAX_EXPORT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    operation: Operation,
    items: Vec<Selection>,
    #[serde(default)]
    directory: Option<PathBuf>,
}

#[derive(Serialize)]
struct Entry {
    id: String,
    revision: String,
    status: Outcome,
    bytes: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
}

#[derive(Serialize)]
struct Report {
    entries: Vec<Entry>,
    ready: usize,
    ready_bytes: u64,
    done: usize,
    done_bytes: u64,
}
impl Report {
    fn collect(entries: Vec<Entry>) -> Self {
        let mut report = Self {
            entries,
            ready: 0,
            ready_bytes: 0,
            done: 0,
            done_bytes: 0,
        };
        for entry in &report.entries {
            match entry.status {
                Outcome::Ready => {
                    report.ready += 1;
                    report.ready_bytes += entry.bytes;
                }
                Outcome::Done => {
                    report.done += 1;
                    report.done_bytes += entry.bytes;
                }
                _ => {}
            }
        }
        report
    }
}

pub fn execute(
    store: &mut Store,
    method: &str,
    params: &Value,
    leases: &BTreeSet<String>,
    blocked: &BTreeSet<String>,
    now: u64,
    allowed: impl Fn() -> bool,
) -> Result<Value> {
    ensure!(allowed(), "capture.privacy");
    match method {
        "catalog_query" => {
            let query: Query = serde_json::from_value(params.clone())
                .map_err(|_| anyhow::anyhow!("capture.catalog_invalid_query"))?;
            Ok(serde_json::to_value(catalog::query(
                store.assets(),
                leases,
                &query,
                now,
            )?)?)
        }
        "catalog_rename" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Rename {
                item: Selection,
                name: String,
            }
            let request: Rename = selection_params(params)?;
            store.rename(&request.item, &request.name)?;
            Ok(json!({}))
        }
        "catalog_expired" => {
            let mut expired = store
                .assets()
                .iter()
                .filter(|asset| {
                    Scope::of(asset) == Scope::Temporary
                        && now.saturating_sub(asset.retention_start()) >= RETENTION_SECONDS
                        && !leases.contains(&asset.id)
                })
                .collect::<Vec<_>>();
            expired.sort_unstable_by_key(|asset| (asset.retention_start(), &asset.id));
            let remaining = expired.len().saturating_sub(MAX_BATCH);
            let entries = expired
                .into_iter()
                .take(MAX_BATCH)
                .map(|asset| Entry {
                    id: asset.id.clone(),
                    revision: catalog::revision(asset),
                    status: Outcome::Ready,
                    bytes: asset.bytes,
                    name: asset.name.clone(),
                    created_at: Some(asset.created_at),
                    filename: None,
                })
                .collect();
            Ok(json!({"report": Report::collect(entries), "remaining": remaining}))
        }
        "catalog_preview" | "catalog_apply" => {
            let request: Batch = selection_params(params)?;
            catalog::validate_batch(&request.items)?;
            let apply = method == "catalog_apply";
            let directory = match (apply, request.operation, request.directory.as_deref()) {
                (true, Operation::Export, Some(path)) => Some(export_directory(path)?),
                (true, Operation::Export, None) | (_, _, Some(_)) => {
                    anyhow::bail!("capture.catalog_invalid_destination")
                }
                _ => None,
            };
            Ok(serde_json::to_value(batch(
                store,
                request,
                directory.as_deref(),
                apply,
                blocked,
                now,
                allowed,
            ))?)
        }
        _ => anyhow::bail!("capture.invalid_request"),
    }
}

fn selection_params<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T> {
    serde_json::from_value(params.clone())
        .map_err(|_| anyhow::anyhow!("capture.catalog_invalid_selection"))
}

fn export_directory(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "capture.catalog_invalid_destination");
    let canonical = path
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("capture.catalog_invalid_destination"))?;
    let private = crate::paths::tool_data_dir()?.canonicalize()?;
    ensure!(
        canonical.is_dir() && !canonical.starts_with(private),
        "capture.catalog_invalid_destination"
    );
    Ok(canonical)
}

fn batch(
    store: &mut Store,
    request: Batch,
    directory: Option<&Path>,
    apply: bool,
    blocked: &BTreeSet<String>,
    now: u64,
    allowed: impl Fn() -> bool,
) -> Report {
    let mut planned_bytes = 0;
    let mut revoked = false;
    let entries = request
        .items
        .into_iter()
        .map(|item| {
            let op = request.operation;
            let asset = store.get(&item.id).ok();
            let bytes = asset.map_or(0, |a| a.bytes);
            let name = asset.map_or_else(String::new, |a| a.name.clone());
            let created_at = asset.map(|a| a.created_at);
            let mut status = store.preview(&item, op, blocked.contains(&item.id));
            let mut filename = None;
            if status == Outcome::Ready && op == Operation::Export {
                if planned_bytes + bytes > MAX_EXPORT_BYTES {
                    status = Outcome::ExportLimit;
                } else {
                    planned_bytes += bytes;
                    filename = asset.map(export_name);
                }
            }
            if apply && status == Outcome::Ready {
                revoked |= !allowed();
                status = if revoked {
                    Outcome::Paused
                } else if op == Operation::Export {
                    match (directory, filename.as_deref()) {
                        (Some(directory), Some(filename)) => {
                            export_one(store, &item.id, &directory.join(filename), &allowed)
                        }
                        _ => Outcome::Failed,
                    }
                } else {
                    store.apply(&item, op, blocked.contains(&item.id), now, &allowed)
                };
                revoked |= status == Outcome::Paused;
            }
            Entry {
                id: item.id,
                revision: item.revision,
                status,
                bytes,
                name,
                created_at,
                filename,
            }
        })
        .collect();
    Report::collect(entries)
}

fn export_name(asset: &Asset) -> String {
    // A stable, private-by-default filename: no title text or unsafe filename characters.
    format!("Moodflow-{}.{}", asset.id, asset.extension)
}

fn export_one(store: &Store, id: &str, destination: &Path, allowed: impl Fn() -> bool) -> Outcome {
    let export = (|| -> Result<Outcome> {
        let asset = store.active(id)?;
        let path = store.path(id)?;
        if destination.try_exists()? {
            return Ok(Outcome::DestinationExists);
        }
        let Some(parent) = destination.parent() else {
            return Ok(Outcome::Failed);
        };
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        if super::store::copy_candidate(&path, asset.bytes, &mut temporary).is_err()
            || digest(temporary.path()).ok().as_ref() != Some(&(asset.bytes, asset.sha256.clone()))
        {
            return Ok(Outcome::ChangedFile);
        }
        temporary.as_file().sync_all()?;
        if !allowed() {
            return Ok(Outcome::Paused);
        }
        match temporary.persist_noclobber(destination) {
            Ok(_) => Ok(Outcome::Done),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                Ok(Outcome::DestinationExists)
            }
            Err(e) => Err(e.into()),
        }
    })();
    export.unwrap_or(Outcome::Failed)
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod feature_tests;
