//! Only the dashboard can read local sessions or operate the terminal. MCP has a separate write-only port.
use crate::{error::UiError, workspace::Runtime};
use std::sync::Arc;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
#[specta::specta]
pub async fn workspace_request(
    app: AppHandle,
    window: WebviewWindow,
    runtime: State<'_, Arc<Runtime>>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    if window.label() != "dashboard" || payload.len() > 256 * 1024 {
        return Err(UiError::new("workspace.invalid", "workspace.failed"));
    }
    let params: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("workspace.invalid", "workspace.failed"))?;
    let runtime = runtime.inner().clone();
    let check = !["gateway_status", "gateway_stop"].contains(&method.as_str());
    let rotates = method == "gateway_rotate";
    let guard = runtime.clone();
    let epoch = guard.epoch();
    let result = if method == "chat" {
        runtime.chat(app.clone(), params).await
    } else if method == "gateway_start" {
        let port = params["port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .ok_or_else(|| UiError::new("workspace.invalid", "workspace.failed"))?;
        runtime.start_gateway(app.clone(), port).await
    } else {
        let owner = window.label().to_owned();
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            runtime.execute(&app, &owner, &method, &params)
        })
        .await
        .map_err(|_| UiError::new("workspace.failed", "workspace.failed"))?
    };
    let result = result.map_err(|error| {
        let code = error
            .chain()
            .find_map(|cause| {
                let message = cause.to_string();
                [
                    "workspace.privacy",
                    "workspace.invalid",
                    "workspace.limit",
                    "workspace.empty",
                    "workspace.conflict",
                    "workspace.changed",
                    "workspace.inbox_required",
                    "workspace.missing",
                    "workspace.model_changed",
                    "workspace.token_invalid",
                    "workspace.gateway_busy",
                    "workspace.terminal_busy",
                    "workspace.terminal_failed",
                    "workspace.terminal_unsupported",
                    "workspace.no_terminal",
                    "workspace.consent",
                    "workspace.safety_unavailable",
                    "workspace.safety",
                    "workspace.ai_unavailable",
                    "workspace.ai_failed",
                    "workspace.invalid_output",
                    "workspace.cancelled",
                    "workspace.busy",
                ]
                .into_iter()
                .find(|code| *code == message)
            })
            .unwrap_or("workspace.failed");
        UiError::new(code, code)
    })?;
    if check
        && ((!rotates && guard.epoch() != epoch) || !crate::workspace::allowed_async(app).await)
    {
        return Err(UiError::new("workspace.privacy", "workspace.privacy"));
    }
    Ok(result.to_string())
}
