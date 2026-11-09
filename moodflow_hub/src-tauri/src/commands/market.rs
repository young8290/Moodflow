//! Settings-only signed catalog transport. The core owns trust, policy and installation;
//! no API key, input text, automatic refresh or redirect is used by this client.
use crate::error::UiError;
use anyhow::{Result, anyhow, bail};
use moodflow_hub_core::infra::imeconf::ImeRpc;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tauri::{State, WebviewWindow};

const INDEX_LIMIT: usize = 1024 * 1024;
const PACKAGE_LIMIT: usize = 32 * 1024 * 1024;
pub(super) const CODES: &[&str] = &[
    "market.invalid",
    "market.io",
    "market.busy",
    "market.network",
    "market.too_large",
    "market.signature",
    "market.expired",
    "market.rollback",
    "market.not_configured",
    "market.source_changed",
    "market.withdrawn",
    "market.incompatible",
    "market.hash",
    "market.already_installed",
    "market.package",
    "market.consent",
    "market.cancelled",
    "market.failed",
];

#[derive(Default)]
pub struct Runtime {
    generation: AtomicU64,
    pending: Mutex<Option<Arc<Pending>>>,
}
struct Pending {
    token: String,
    params: Value,
    fresh: bool,
    _directory: tempfile::TempDir,
}
impl Runtime {
    fn invalidate(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        generation
    }
    fn check(&self, generation: u64) -> Result<()> {
        if generation != self.generation.load(Ordering::SeqCst) {
            bail!("market.cancelled");
        }
        Ok(())
    }
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().ok_or_else(|| anyhow!("market.invalid"))
}
async fn rpc(method: &str, params: Value) -> Result<Value> {
    let method = method.to_owned();
    tauri::async_runtime::spawn_blocking(move || -> Result<Value> {
        let client = ImeRpc::from_env(cfg!(debug_assertions));
        if method == "plugins.market.inspect" {
            Ok(serde_json::to_value(client.market_inspect(params)?)?)
        } else {
            Ok(client.call(&method, params)?)
        }
    })
    .await
    .map_err(|_| anyhow!("market.failed"))?
}
fn checked_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).map_err(|_| anyhow!("market.invalid"))?;
    let scheme_allowed = url.scheme() == "https"
        || cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if raw.len() > 2048
        || !scheme_allowed
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (!cfg!(test) && url.port().is_some_and(|p| p != 443))
    {
        bail!("market.invalid");
    }
    Ok(url)
}
async fn download(url: reqwest::Url, limit: usize) -> Result<Vec<u8>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = reqwest::Client::builder()
        .https_only(!cfg!(test))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(if limit == PACKAGE_LIMIT {
            60
        } else {
            20
        }))
        .user_agent("Moodflow (explicit signed catalog request)");
    #[cfg(test)]
    let builder = builder.no_proxy();
    let client = builder.build().map_err(|_| anyhow!("market.network"))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| anyhow!("market.network"))?;
    if !response.status().is_success() {
        bail!("market.network");
    }
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        bail!("market.too_large");
    }
    let mut out = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("market.network"))?
    {
        if out.len().saturating_add(chunk.len()) > limit {
            bail!("market.too_large");
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
async fn execute(runtime: &Runtime, method: &str, params: Value) -> Result<Value> {
    match method {
        "community" => {
            #[cfg(windows)]
            super::market_links::open(text(&params, "target")?)?;
            #[cfg(not(windows))]
            bail!("market.failed");
            #[cfg(windows)]
            Ok(json!({}))
        }
        "cancel" => {
            runtime.invalidate();
            Ok(json!({}))
        }
        "catalog" => rpc("plugins.market.catalog", json!({})).await,
        "configure" => {
            runtime.invalidate();
            rpc("plugins.market.configure", params).await
        }
        "refresh" => {
            if params["consent"].as_bool() != Some(true) {
                bail!("market.consent");
            }
            let generation = runtime.invalidate();
            let view = rpc("plugins.market.catalog", json!({})).await?;
            let url = checked_url(
                view["source"]["url"]
                    .as_str()
                    .ok_or_else(|| anyhow!("market.not_configured"))?,
            )?;
            let mut sig_url = url.clone();
            sig_url.set_path(&format!("{}.sig", url.path()));
            let (index, signature) =
                tokio::try_join!(download(url, INDEX_LIMIT), download(sig_url, 256))?;
            runtime.check(generation)?;
            rpc(
                "plugins.market.accept",
                json!({"revision":view["revision"],
                "index":String::from_utf8(index).map_err(|_|anyhow!("market.invalid"))?,
                "signature":String::from_utf8(signature).map_err(|_|anyhow!("market.signature"))?}),
            )
            .await
        }
        "preview" => {
            if params["consent"].as_bool() != Some(true) {
                bail!("market.consent");
            }
            let generation = runtime.invalidate();
            let id = text(&params, "id")?;
            let view = rpc("plugins.market.catalog", json!({})).await?;
            if view["expired"].as_bool() == Some(true) {
                bail!("market.expired");
            }
            let item = view["items"]
                .as_array()
                .and_then(|items| items.iter().find(|v| v["id"].as_str() == Some(id)))
                .ok_or_else(|| anyhow!("market.incompatible"))?;
            let release = &item["release"];
            let url = checked_url(text(release, "url")?)?;
            let bytes = download(url, PACKAGE_LIMIT).await?;
            runtime.check(generation)?;
            let directory = tempfile::tempdir().map_err(|_| anyhow!("market.io"))?;
            let path = directory.path().join("download.mfpack");
            std::fs::write(&path, bytes).map_err(|_| anyhow!("market.io"))?;
            let args = json!({"path":path,"revision":view["revision"],"id":id,"version":release["version"],"sha256":release["sha256"]});
            let preview = rpc("plugins.market.inspect", args.clone()).await?;
            runtime.check(generation)?;
            let pending = Arc::new(Pending {
                token: uuid::Uuid::new_v4().to_string(),
                params: args,
                fresh: preview["upgrade_from"].is_null(),
                _directory: directory,
            });
            let result = json!({"token":pending.token,"preview":preview,"source":view["source"],"fingerprint":view["fingerprint"],"sha256":release["sha256"]});
            let mut slot = runtime.pending.lock().unwrap_or_else(|e| e.into_inner());
            runtime.check(generation)?;
            *slot = Some(pending);
            Ok(result)
        }
        "install" => {
            let pending = runtime
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .filter(|p| Some(p.token.as_str()) == params["token"].as_str())
                .ok_or_else(|| anyhow!("market.cancelled"))?;
            if params["confirmed"].as_bool() != Some(true) {
                bail!("market.consent");
            }
            let mut args = pending.params.clone();
            args["accept"] = params["accept"].clone();
            let value = rpc("plugins.market.install", args).await?;
            let mut slot = runtime.pending.lock().unwrap_or_else(|e| e.into_inner());
            if slot.as_ref().is_some_and(|p| p.token == pending.token) {
                slot.take();
            }
            Ok(json!({"id":value["id"],"fresh":pending.fresh}))
        }
        _ => bail!("market.invalid"),
    }
}
#[tauri::command]
#[specta::specta]
pub async fn market_request(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    if window.label() != "settings" || payload.len() > 64 * 1024 {
        return Err(UiError::new("market.invalid", "market.invalid"));
    }
    let params = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("market.invalid", "market.invalid"))?;
    let result = execute(&runtime, &method, params).await.map_err(|error| {
        let messages = error.chain().map(ToString::to_string).collect::<Vec<_>>();
        let code = CODES
            .iter()
            .find(|code| messages.iter().any(|m| m.contains(**code)))
            .copied()
            .unwrap_or("market.failed");
        UiError::new(code, code)
    })?;
    Ok(result.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_http_redirect_and_size_boundaries() {
        use axum::{Router, response::Redirect, routing::get};
        let hits = Arc::new(AtomicU64::new(0));
        let target_hits = hits.clone();
        let router = Router::new()
            .route("/ok", get(|| async { "catalog" }))
            .route("/big", get(|| async { "x".repeat(1024) }))
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/target") }),
            )
            .route(
                "/target",
                get(move || {
                    let hits = target_hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        "redirected"
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        assert_eq!(
            download(checked_url(&format!("{base}/ok")).unwrap(), 20)
                .await
                .unwrap(),
            b"catalog"
        );
        assert!(
            download(checked_url(&format!("{base}/redirect")).unwrap(), 20)
                .await
                .is_err()
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(
            download(checked_url(&format!("{base}/big")).unwrap(), 20)
                .await
                .is_err()
        );
        assert!(checked_url("https://u:p@example.com/index.json").is_err());
        assert!(checked_url("https://example.com/index.json?token=secret").is_err());
        server.abort();
    }
    #[tokio::test]
    async fn requests_need_explicit_consent_and_matching_pending_token() {
        let runtime = Runtime::default();
        for method in ["refresh", "preview", "install"] {
            assert!(execute(&runtime, method, json!({})).await.is_err());
        }
        let generation = runtime.invalidate();
        runtime.invalidate();
        assert!(runtime.check(generation).is_err());
        assert!(
            execute(
                &runtime,
                "install",
                json!({"confirmed":true,"token":"missing","accept":[]})
            )
            .await
            .is_err()
        );
    }
}
