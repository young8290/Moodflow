//! Local capture assets and explicit actions. Capture/decoding work runs in a separate process.
mod clipboard;
mod management;
mod source;
mod store;
mod worker;
use anyhow::{Result, ensure};
use moodflow_capsule::capture::{Asset, Request};
use serde_json::{Value, json};
pub(crate) use source::awake;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
pub use worker::reviewed as worker_allowed;
/// Decode/OCR only explicitly selected, immutable image snapshots for AI safety screening.
pub(crate) fn tool_ocr(
    app: &AppHandle,
    path: &std::path::Path,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let cancel = Arc::new(worker::Cancel::default());
    let done = Arc::new(AtomicBool::new(false));
    struct Finish(Arc<AtomicBool>);
    impl Drop for Finish {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let _finish = Finish(done.clone());
    let worker_cancel = cancel.clone();
    std::thread::spawn(move || {
        while !done.load(Ordering::Acquire) {
            if stop.load(Ordering::Acquire) {
                worker_cancel.abort.store(true, Ordering::Release);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let normalized = path.with_extension("png");
    let request: Request =
        serde_json::from_value(json!({"operation":"normalize","input":path,"output":normalized}))?;
    worker::run(app, &request, cancel.clone())?;
    let request: Request = serde_json::from_value(json!({"operation":"ocr","input":normalized}))?;
    let result = worker::run(app, &request, cancel)?;
    result["text"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("ai_tools.image_failed"))
}
pub struct Runtime {
    epoch: std::sync::atomic::AtomicU64,
    running: Mutex<Option<(String, Arc<worker::Cancel>)>>,
    leases: Mutex<BTreeMap<(String, String), Option<Instant>>>,
    clipboard: Mutex<Option<crate::vault_platform::ClipboardLease>>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            epoch: std::sync::atomic::AtomicU64::new(0),
            running: Mutex::new(None),
            leases: Mutex::new(BTreeMap::new()),
            clipboard: Mutex::new(None),
        }
    }
}
struct Running<'a> {
    runtime: &'a Runtime,
    id: String,
}
struct AssetLease<'a> {
    runtime: &'a Runtime,
    owner: String,
}
impl Drop for AssetLease<'_> {
    fn drop(&mut self) {
        self.runtime.release(&self.owner);
    }
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut running = self
            .runtime
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if running.as_ref().is_some_and(|(id, _)| id == &self.id) {
            *running = None;
        }
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn string<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("capture.invalid_request"))
}
fn root() -> Result<PathBuf> {
    Ok(crate::paths::tool_data_dir()?.join("capture"))
}
impl Runtime {
    pub fn cancel(&self, abort: bool) {
        if abort {
            self.epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        if let Some((_, cancel)) = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            if abort {
                cancel
                    .abort
                    .store(true, std::sync::atomic::Ordering::Release);
            } else {
                cancel
                    .stop
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }
    }
    pub fn epoch(&self) -> u64 {
        self.epoch.load(std::sync::atomic::Ordering::Acquire)
    }
    fn leased(&self) -> BTreeSet<String> {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        leases.retain(|_, until| until.is_none_or(|until| until > Instant::now()));
        leases.keys().map(|(id, _)| id.clone()).collect()
    }
    pub fn release(&self, owner: &str) {
        self.leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(_, label), _| label != owner);
    }
    pub fn sweep_clipboard(&self, allowed: bool) {
        let mut clipboard = self.clipboard.lock().unwrap_or_else(|e| e.into_inner());
        let Some(lease) = clipboard.as_ref() else {
            return;
        };
        if allowed && Instant::now() < lease.expires {
            return;
        }
        match crate::vault_platform::clear_sensitive(lease) {
            crate::vault_platform::Clear::Done => *clipboard = None,
            #[cfg(windows)]
            crate::vault_platform::Clear::Changed => *clipboard = None,
            #[cfg(windows)]
            crate::vault_platform::Clear::Busy => {}
        }
    }
    fn lease(&self, id: &str, owner: &str, permanent: bool) {
        self.leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                (id.into(), owner.into()),
                if permanent {
                    None
                } else {
                    Some(Instant::now() + Duration::from_secs(30))
                },
            );
    }
    fn run(&self, app: &AppHandle, request: &Request) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = Arc::new(worker::Cancel::default());
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(running.is_none(), "capture.busy");
            *running = Some((id.clone(), cancel.clone()));
        }
        let _guard = Running { runtime: self, id };
        worker::run(app, request, cancel)
    }
    pub fn execute(&self, app: &AppHandle, owner: &str, method: &str, p: &Value) -> Result<Value> {
        if ["abort", "stop"].contains(&method) {
            self.cancel(method == "abort");
            return Ok(json!({"stopped":true}));
        }
        if method == "release" {
            self.release(owner);
            return Ok(json!({}));
        }
        if method == "status" {
            return Ok(
                json!({"supported":cfg!(windows),"helper":worker::executable().is_some(),"running":self.running.lock().unwrap_or_else(|e|e.into_inner()).is_some()}),
            );
        }
        ensure!(worker::reviewed(app), "capture.privacy");
        let epoch = self.epoch();
        let root = root()?;
        crate::private_fs::ensure(&root)?;
        if method == "targets" {
            let request: Request = serde_json::from_value(json!({"operation":"enumerate"}))?;
            return self.run(app, &request);
        }
        if method == "start" {
            let mut request: Request = serde_json::from_value(p["request"].clone())?;
            ensure!(
                ["screenshot", "record", "scroll"].contains(&request.operation.as_str()),
                "capture.invalid_request"
            );
            request.validate()?;
            let extension = if request.operation != "record" {
                "png"
            } else if request.gif {
                "gif"
            } else {
                "mp4"
            };
            let work = tempfile::Builder::new()
                .prefix("capture-work-")
                .tempdir_in(&root)?;
            request.input = None;
            request.output = Some(
                work.path()
                    .join(format!("output.{extension}"))
                    .to_string_lossy()
                    .into_owned(),
            );
            let result = self.run(app, &request)?;
            ensure!(
                self.epoch() == epoch && worker::reviewed(app),
                "capture.privacy"
            );
            let mut store = store::Store::open(&root)?;
            store.prune_for_space(
                &self.leased(),
                now(),
                store::digest(&PathBuf::from(request.output.as_ref().unwrap()))?.0,
            )?;
            let asset = store.publish(
                &PathBuf::from(request.output.as_ref().unwrap()),
                extension,
                result["width"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| anyhow::anyhow!("capture.failed"))?,
                result["height"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| anyhow::anyhow!("capture.failed"))?,
                result["duration_ms"].as_u64().unwrap_or(0),
                now(),
            )?;
            self.lease(&asset.id, owner, false);
            if self.epoch() != epoch {
                store.remove(&asset.id)?;
                anyhow::bail!("capture.cancelled");
            }
            return asset_reply(app, &store, &asset);
        }
        if method == "annotate" {
            use base64::Engine;
            let encoded = string(p, "png")?
                .strip_prefix("data:image/png;base64,")
                .ok_or_else(|| anyhow::anyhow!("capture.invalid_request"))?;
            ensure!(encoded.len() <= 24 * 1024 * 1024, "capture.pixel_limit");
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
            ensure!(
                bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "capture.invalid_request"
            );
            let work = tempfile::Builder::new()
                .prefix("capture-work-")
                .tempdir_in(&root)?;
            let path = work.path().join("edited.png");
            fs::write(&path, bytes)?;
            let request: Request =
                serde_json::from_value(json!({"operation":"inspect","input":path}))?;
            let result = self.run(app, &request)?;
            ensure!(
                self.epoch() == epoch && worker::reviewed(app),
                "capture.privacy"
            );
            let mut store = store::Store::open(&root)?;
            store.prune_for_space(&self.leased(), now(), store::digest(&path)?.0)?;
            let asset = store.publish(
                &path,
                "png",
                u32::try_from(
                    result["width"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("capture.failed"))?,
                )?,
                u32::try_from(
                    result["height"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("capture.failed"))?,
                )?,
                0,
                now(),
            )?;
            self.lease(&asset.id, owner, false);
            return asset_reply(app, &store, &asset);
        }
        if method == "copy_text" {
            let text = string(p, "text")?;
            ensure!(
                text.encode_utf16().count() <= 65536,
                "capture.invalid_request"
            );
            let handle = native_owner(app, owner)?;
            let lease = crate::vault_platform::copy_sensitive(handle, text)
                .map_err(|_| anyhow::anyhow!("capture.failed"))?;
            *self.clipboard.lock().unwrap_or_else(|e| e.into_inner()) = Some(lease);
            return Ok(json!({"copied":true}));
        }
        let mut store = store::Store::open(&root)?;
        if method.starts_with("catalog_") {
            ensure!(owner == "dashboard", "capture.invalid_request");
            let leased = self.leased();
            let blocked = self
                .leases
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .filter(|(_, label)| label != owner)
                .map(|(id, _)| id.clone())
                .collect();
            return management::execute(&mut store, method, p, &leased, &blocked, now(), || {
                self.epoch() == epoch && worker::reviewed(app)
            });
        }
        let id = string(p, "id")?;
        let asset = store.active(id)?.clone();
        if method == "lease" {
            self.lease(id, owner, owner.starts_with("capture-pin-"));
            return Ok(json!({}));
        }
        let path = store.verified_path(id)?;
        match method {
            "copy_image" => {
                ensure!(asset.extension == "png", "capture.image_required");
                self.lease(id, owner, false);
                let work = tempfile::Builder::new()
                    .prefix("capture-work-")
                    .tempdir_in(&root)?;
                let dib = work.path().join("image.dib");
                let bytes = store::read_verified(&path, &asset)?;
                let snapshot = work.path().join("edited.png");
                fs::write(&snapshot, &bytes)?;
                let request: Request = serde_json::from_value(
                    json!({"operation":"dib","input":snapshot,"output":dib}),
                )?;
                drop(store);
                self.run(app, &request)?;
                let dib = store::read_bounded(&dib)?;
                ensure!(worker::allowed(app), "capture.privacy");
                ensure!(
                    self.epoch() == epoch && worker::reviewed(app),
                    "capture.privacy"
                );
                clipboard::image(native_owner(app, owner)?, &bytes, &dib)
                    .map_err(|_| anyhow::anyhow!("capture.failed"))?;
                Ok(json!({"copied":true}))
            }
            "pin" => {
                ensure!(asset.extension != "mp4", "capture.image_required");
                asset_reply(app, &store, &asset)?;
                let label = format!("capture-pin-{id}");
                if let Some(window) = app.get_webview_window(&label) {
                    window.show()?;
                    return Ok(json!({"pinned":true}));
                }
                let width = f64::from(asset.width).clamp(160.0, 800.0);
                ensure!(
                    app.webview_windows()
                        .keys()
                        .filter(|label| label.starts_with("capture-pin-"))
                        .count()
                        < 16,
                    "capture.busy"
                );
                let height =
                    (width * f64::from(asset.height) / f64::from(asset.width)).clamp(100.0, 700.0);
                let window = tauri::WebviewWindowBuilder::new(
                    app,
                    &label,
                    tauri::WebviewUrl::App(format!("capture-pin/index.html?asset={id}").into()),
                )
                .title("Moodflow 贴图")
                .inner_size(width, height)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .build()?;
                window.set_content_protected(true)?;
                self.lease(id, &label, true);
                let runtime = app.state::<Arc<Runtime>>().inner().clone();
                window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        runtime.release(&label);
                    }
                });
                Ok(json!({"pinned":true}))
            }
            "unpin" | "pin_passthrough" => {
                if let Some(window) = app.get_webview_window(&format!("capture-pin-{id}")) {
                    if method == "unpin" {
                        window.close()?;
                        self.release(&format!("capture-pin-{id}"));
                    } else {
                        window.set_ignore_cursor_events(p["enabled"].as_bool().unwrap_or(false))?;
                    }
                }
                Ok(json!({}))
            }
            "asset" => {
                self.lease(id, owner, owner.starts_with("capture-pin-"));
                asset_reply(app, &store, &asset)
            }
            "remove" => {
                self.leased();
                let in_use = self
                    .leases
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .any(|((asset, label), _)| asset == id && label != owner);
                ensure!(!in_use, "capture.in_use");
                ensure!(
                    asset.permanent && p["confirm_saved"].as_bool() == Some(true),
                    "capture.saved"
                );
                ensure!(
                    self.epoch() == epoch && worker::reviewed(app),
                    "capture.privacy"
                );
                store.remove_if(id, || self.epoch() == epoch && worker::reviewed(app))?;
                Ok(json!({"removed":true}))
            }
            "export" => {
                let destination = PathBuf::from(string(p, "path")?);
                let mut temporary = tempfile::NamedTempFile::new_in(
                    destination
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("capture.invalid_request"))?,
                )?;
                std::io::copy(
                    &mut fs::File::open(&path)?.take(asset.bytes + 1),
                    &mut temporary,
                )?;
                temporary.as_file().sync_all()?;
                ensure!(
                    store::digest(temporary.path())?.1 == asset.sha256,
                    "capture.changed"
                );
                temporary.persist_noclobber(destination)?;
                Ok(json!({"exported":true}))
            }
            "ocr" | "qr" => {
                ensure!(asset.extension != "mp4", "capture.image_required");
                self.lease(id, owner, false);
                let label = format!("job:{}", uuid::Uuid::new_v4());
                self.lease(id, &label, true);
                let _lease = AssetLease {
                    runtime: self,
                    owner: label,
                };
                let request: Request =
                    serde_json::from_value(json!({"operation":method,"input":path}))?;
                drop(store);
                self.run(app, &request)
            }
            "save_library" => {
                ensure!(
                    asset.extension != "mp4" || asset.duration_ms > 0,
                    "capture.invalid_asset"
                );
                store.set_permanent(id)?;
                let document =
                    crate::commands::library::save_capture(&asset, &path, string(p, "title")?)?;
                Ok(json!({"document":document}))
            }
            _ => anyhow::bail!("capture.invalid_request"),
        }
    }
}
fn native_owner(app: &AppHandle, owner: &str) -> Result<usize> {
    #[cfg(windows)]
    {
        Ok(app
            .get_webview_window(owner)
            .ok_or_else(|| anyhow::anyhow!("capture.failed"))?
            .hwnd()?
            .0 as usize)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, owner);
        anyhow::bail!("capture.unsupported")
    }
}
fn asset_reply(app: &AppHandle, store: &store::Store, asset: &Asset) -> Result<Value> {
    let path = store.path(&asset.id)?;
    app.asset_protocol_scope().allow_file(&path)?;
    Ok(json!({"asset":asset,"path":path}))
}
pub fn start(app: &AppHandle) {
    if let Ok(path) = root() {
        // Setup precedes command handling, so all prior work directories are abandoned.
        let _ = store::Store::cleanup_work(&path);
    }
    let runtime = Arc::new(Runtime::default());
    app.manage(runtime.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let current = runtime.clone();
            let allowed = worker::allowed(&app);
            let _ = tauri::async_runtime::spawn_blocking(move || current.sweep_clipboard(allowed))
                .await;
        }
    });
}
