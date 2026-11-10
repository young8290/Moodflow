//! Dashboard/pinned-image actions only; payloads and OCR never enter diagnostics.
use crate::{capture::Runtime, error::UiError};
use std::sync::Arc;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
#[specta::specta]
pub async fn capture_request(
    app: AppHandle,
    window: WebviewWindow,
    runtime: State<'_, Arc<Runtime>>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    if payload.len() > 24 * 1024 * 1024 {
        return Err(UiError::new("capture.invalid_request", "capture.failed"));
    }
    let params: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("capture.invalid_request", "capture.failed"))?;
    let owner = window.label().to_owned();
    if owner != "dashboard" {
        let id = owner
            .strip_prefix("capture-pin-")
            .ok_or_else(|| UiError::new("capture.invalid_window", "capture.failed"))?;
        if params["id"].as_str() != Some(id)
            || ![
                "asset",
                "lease",
                "release",
                "pin_passthrough",
                "unpin",
                "copy_image",
                "export",
            ]
            .contains(&method.as_str())
        {
            return Err(UiError::new("capture.invalid_window", "capture.failed"));
        }
    }
    if method == "status" {
        window.set_content_protected(true)?;
    }
    if method == "release" && owner == "dashboard" {
        window.set_content_protected(false)?;
    }
    let current = runtime.inner().clone();
    let epoch = current.epoch();
    let guard = current.clone();
    let public_result = !["status", "release", "abort", "stop"].contains(&method.as_str());
    let handle = app.clone();
    let reply = tauri::async_runtime::spawn_blocking(move || {
        current.execute(&handle, &owner, &method, &params)
    })
    .await
    .map_err(|_| UiError::new("capture.failed", "capture.failed"))?
    .map_err(|error| {
        let code = error
            .chain()
            .find_map(|cause| {
                let code = cause.to_string();
                [
                    "capture.privacy",
                    "capture.cancelled",
                    "capture.missing_helper",
                    "capture.busy",
                    "capture.ocr_missing",
                    "capture.no_overlap",
                    "capture.pixel_limit",
                    "capture.audio_unavailable",
                    "capture.changed",
                    "capture.in_use",
                    "capture.saved",
                    "capture.storage_full",
                ]
                .into_iter()
                .find(|allowed| *allowed == code)
            })
            .unwrap_or("capture.failed");
        UiError::new(code, code)
    })?;
    if public_result
        && !tauri::async_runtime::spawn_blocking(move || {
            guard.epoch() == epoch && crate::capture::worker_allowed(&app)
        })
        .await
        .unwrap_or(false)
    {
        return Err(UiError::new("capture.privacy", "capture.privacy"));
    }
    Ok(reply.to_string())
}
