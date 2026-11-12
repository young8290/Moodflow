//! Dashboard-only review of saved conflicts; no network or replica state mutations.
use super::*;
use moodflow_capsule::{
    Library,
    recovery::{Recovery, Resolution},
};
pub fn execute(
    root: &Path,
    method: &str,
    params: &Value,
    allowed: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure!(allowed(), "sync.privacy");
    let sources = if params["backups"].as_bool() == Some(true) {
        root.join("recovery").join("backups")
    } else {
        root.join("conflicts")
    };
    let controls = root.join("recovery");
    crate::private_fs::ensure(&sources)?;
    crate::private_fs::ensure(&controls)?;
    let recovery = Recovery::new(sources, controls);
    if method == "review_list" {
        return Ok(serde_json::to_value(
            recovery.list(params["query"].as_str().unwrap_or(""))?,
        )?);
    }
    if method == "review_delete" {
        ensure!(params["confirm"].as_bool() == Some(true), "sync.invalid");
        let name = params["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?;
        let revision = params["revision"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?;
        recovery.delete_source(name, revision, allowed)?;
        return Ok(json!({"deleted":true}));
    }
    let library_root = crate::paths::tool_data_dir()?.join("capsule");
    crate::private_fs::ensure(&library_root)?;
    let mut library = Library::new(library_root);
    if method == "review" {
        return Ok(serde_json::to_value(
            recovery.review(
                &library,
                params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?,
            )?,
        )?);
    }
    ensure!(
        method == "review_resolve" && params["confirm"].as_bool() == Some(true),
        "sync.invalid"
    );
    let resolution: Resolution = serde_json::from_value(params["resolution"].clone())?;
    Ok(serde_json::to_value(recovery.resolve(
        &mut library,
        &resolution,
        allowed,
    )?)?)
}
