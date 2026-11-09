use crate::{error::UiError, vault::SensitiveJson};
use tauri::{AppHandle, WebviewWindow};
use zeroize::Zeroizing;
#[tauri::command]
#[specta::specta]
pub async fn sync_request(
    app: AppHandle,
    window: WebviewWindow,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    let payload = Zeroizing::new(payload);
    if window.label() != "dashboard" || payload.len() > 128 * 1024 {
        return Err(UiError::new("sync.invalid", "sync.invalid"));
    }
    let params = SensitiveJson(
        serde_json::from_str(&payload).map_err(|_| UiError::new("sync.invalid", "sync.invalid"))?,
    );
    let check = method != "cancel";
    let worker = app.clone();
    let value = tauri::async_runtime::spawn_blocking(move || {
        crate::sync::execute(&worker, &method, &params.0)
    })
    .await
    .map_err(|_| UiError::new("sync.failed", "sync.failed"))?
    .map_err(|error| {
        let code = error
            .chain()
            .find_map(|e| {
                let s = e.to_string();
                [
                    "sync.cancelled",
                    "sync.privacy",
                    "sync.invalid",
                    "sync.limit",
                    "sync.recovery",
                    "sync.mode_changed",
                    "sync.dataset",
                    "sync.conflict",
                    "sync.local_conflict",
                    "sync.deferred",
                    "sync.uninitialized",
                    "sync.unconfigured",
                    "sync.exists",
                    "sync.busy",
                    "sync.plaintext_confirm",
                    "sync.consent",
                    "sync.https",
                    "sync.network",
                    "sync.etag_required",
                    "sync.provider_conflict",
                    "sync.conflict_limit",
                    "sync.changed",
                    "sync.remote_incomplete",
                    "sync.vault_opt_in",
                    "sync.excluded",
                ]
                .into_iter()
                .find(|code| *code == s)
            })
            .unwrap_or("sync.failed");
        UiError::new(code, code)
    })?;
    if check && !crate::workspace::allowed_async(app).await {
        return Err(UiError::new("sync.privacy", "sync.privacy"));
    }
    Ok(value.to_string())
}
