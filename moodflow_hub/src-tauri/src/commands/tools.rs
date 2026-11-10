//! Dashboard-only explicit tools; errors never contain inputs, file paths or provider bodies.
use crate::{error::UiError, workspace::Runtime};
use std::sync::Arc;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
#[specta::specta]
pub async fn tools_request(
    app: AppHandle,
    window: WebviewWindow,
    runtime: State<'_, Arc<Runtime>>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    if window.label() != "dashboard" || payload.len() > 128 * 1024 {
        return Err(UiError::new("ai_tools.invalid", "ai_tools.invalid"));
    }
    let params: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("ai_tools.invalid", "ai_tools.invalid"))?;
    if method == "cancel" {
        let id = params["id"]
            .as_str()
            .ok_or_else(|| UiError::new("ai_tools.invalid", "ai_tools.invalid"))?;
        runtime.cancel_tool(id);
        return Ok("{}".into());
    }
    if !crate::workspace::allowed_async(app.clone()).await {
        return Err(UiError::new("ai_tools.privacy", "ai_tools.privacy"));
    }
    let result = if method == "unicode" {
        moodflow_latex::convert(params["text"].as_str().unwrap_or(""))
            .and_then(|v| Ok(serde_json::to_value(v)?))
    } else if method == "run" {
        runtime.inner().clone().tool(app.clone(), params).await
    } else {
        Err(anyhow::anyhow!("ai_tools.invalid"))
    };
    let value = result.map_err(|error| {
        let code = error
            .chain()
            .find_map(|cause| {
                let text = cause.to_string();
                text.starts_with("ai_tools.").then_some(text)
            })
            .unwrap_or("ai_tools.failed".into());
        UiError::new(&code, &code)
    })?;
    if !crate::workspace::allowed_async(app).await {
        return Err(UiError::new("ai_tools.privacy", "ai_tools.privacy"));
    }
    Ok(value.to_string())
}
