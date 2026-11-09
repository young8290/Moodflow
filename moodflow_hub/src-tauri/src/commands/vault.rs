//! Sensitive vault commands never go through the input core, logging or AI gateway.
use crate::{
    error::UiError,
    state::AppState,
    vault::{Runtime, SensitiveJson},
};
use std::sync::Arc;
use tauri::{AppHandle, State, WebviewWindow};
use zeroize::Zeroizing;

#[tauri::command]
#[specta::specta]
pub async fn vault_request(
    app: AppHandle,
    window: WebviewWindow,
    runtime: State<'_, Arc<Runtime>>,
    state: State<'_, AppState>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    let payload = Zeroizing::new(payload);
    if window.label() != "dashboard" || payload.len() > 512 * 1024 {
        return Err(UiError::new("vault.invalid", "vault.invalid"));
    }
    let paused = state
        .status
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .paused;
    if paused && !["lock", "status"].contains(&method.as_str()) {
        let current = runtime.inner().clone();
        tauri::async_runtime::spawn_blocking(move || current.lock())
            .await
            .map_err(|_| UiError::new("vault.failed", "vault.failed"))?;
        return Err(UiError::new("library.privacy", "library.privacy"));
    }
    let params = SensitiveJson(
        serde_json::from_str(&payload)
            .map_err(|_| UiError::new("vault.invalid", "vault.invalid"))?,
    );
    #[cfg(windows)]
    let owner = window
        .hwnd()
        .map_err(|_| UiError::new("vault.failed", "vault.failed"))?
        .0 as usize;
    #[cfg(not(windows))]
    let owner = 0;
    let runtime = runtime.inner().clone();
    let policy = crate::vault::allowed(&app);
    let sensitive_result = ["reveal", "get", "list", "new", "generate"].contains(&method.as_str());
    let current = runtime.clone();
    let mut result = tauri::async_runtime::spawn_blocking(move || {
        current
            .execute(&method, &params.0, owner, policy)
            .map(SensitiveJson)
    })
    .await
    .map_err(|_| UiError::new("vault.failed", "vault.failed"))??;
    if sensitive_result && !crate::vault::allowed(&app) {
        drop(result);
        let current = runtime.clone();
        tauri::async_runtime::spawn_blocking(move || current.lock())
            .await
            .map_err(|_| UiError::new("vault.failed", "vault.failed"))?;
        return Err(UiError::new("library.privacy", "library.privacy"));
    }
    if sensitive_result {
        let remaining = tauri::async_runtime::spawn_blocking(move || runtime.remaining_ms(true))
            .await
            .map_err(|_| UiError::new("vault.failed", "vault.failed"))?;
        if remaining == 0 {
            return Err(UiError::new("vault.locked", "vault.locked"));
        }
        if let Some(reveal) = result.0.get_mut("reveal_ms") {
            *reveal = serde_json::json!(remaining.min(15000));
        }
    }
    Ok(result.0.to_string())
}
