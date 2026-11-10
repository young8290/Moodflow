//! Loopback-only authenticated write gateway. Every response contains receipts or protocol metadata only.
use super::*;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use moodflow_mailbox::protocol::{self, Action};
use sha2::{Digest, Sha256};
use std::{
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::atomic::AtomicBool,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::{Semaphore, oneshot};
use zeroize::Zeroizing;

pub struct Server {
    pub address: String,
    enabled: Arc<AtomicBool>,
    stop: Option<oneshot::Sender<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.enabled.store(false, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
#[derive(Clone)]
struct Context {
    app: AppHandle,
    runtime: Arc<Runtime>,
    hash: [u8; 32],
    address: SocketAddr,
    enabled: Arc<AtomicBool>,
    permits: Arc<Semaphore>,
    rate: Arc<Mutex<(Instant, u32)>>,
}
impl Server {
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub fn start(app: AppHandle, runtime: Arc<Runtime>, port: u16, token: &str) -> Result<Self> {
        ensure!(port == 0 || port >= 1024, "workspace.invalid");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|_| anyhow::anyhow!("workspace.gateway_busy"))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let enabled = Arc::new(AtomicBool::new(true));
        let context = Context {
            app,
            runtime,
            hash: Sha256::digest(token.as_bytes()).into(),
            address,
            enabled: enabled.clone(),
            permits: Arc::new(Semaphore::new(4)),
            rate: Arc::new(Mutex::new((Instant::now(), 0))),
        };
        let router = Router::new()
            .route("/write", post(push))
            .route("/mcp", post(mcp))
            .layer(middleware::from_fn_with_state(context.clone(), guard))
            .with_state(context);
        let (sender, receiver) = oneshot::channel();
        let alive = enabled.clone();
        tauri::async_runtime::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = receiver.await;
                })
                .await;
            alive.store(false, Ordering::Release);
        });
        Ok(Self {
            address: format!("http://{address}"),
            enabled,
            stop: Some(sender),
        })
    }
}
pub fn rotate(root: &Path) -> Result<()> {
    let directory = root.join("gateway");
    crate::private_fs::ensure(&directory)?;
    let mut plain = Zeroizing::new(vec![0; 37]);
    plain[..5].copy_from_slice(b"MFWS1");
    getrandom::fill(&mut plain[5..]).map_err(|_| anyhow::anyhow!("workspace.failed"))?;
    let sealed =
        crate::secrets::protect(&plain).map_err(|_| anyhow::anyhow!("workspace.token_invalid"))?;
    let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
    staged.write_all(&sealed)?;
    staged.as_file().sync_all()?;
    staged.persist(directory.join("token.dpapi"))?;
    Ok(())
}
pub fn token(root: &Path) -> Result<Zeroizing<String>> {
    let directory = root.join("gateway");
    crate::private_fs::ensure(&directory)?;
    let path = directory.join("token.dpapi");
    let plain = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            ensure!(
                regular_file(&metadata) && metadata.len() <= 65536,
                "workspace.token_invalid"
            );
            let mut bytes = Vec::new();
            fs::File::open(path)?.take(65537).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 65536, "workspace.token_invalid");
            crate::secrets::unprotect(&bytes)
                .map_err(|_| anyhow::anyhow!("workspace.token_invalid"))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut plain = Zeroizing::new(vec![0; 37]);
            plain[..5].copy_from_slice(b"MFWS1");
            getrandom::fill(&mut plain[5..]).map_err(|_| anyhow::anyhow!("workspace.failed"))?;
            let protected = crate::secrets::protect(&plain)
                .map_err(|_| anyhow::anyhow!("workspace.token_invalid"))?;
            let mut staged = tempfile::NamedTempFile::new_in(directory)?;
            staged.write_all(&protected)?;
            staged.as_file().sync_all()?;
            staged.persist_noclobber(path)?;
            plain
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(
        plain.len() == 37 && &plain[..5] == b"MFWS1",
        "workspace.token_invalid"
    );
    let mut token = Zeroizing::new(String::with_capacity(64));
    let hex = b"0123456789abcdef";
    for byte in &plain[5..] {
        token.push(hex[(byte >> 4) as usize] as char);
        token.push(hex[(byte & 15) as usize] as char);
    }
    Ok(token)
}
fn rejection(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}
fn authenticated(headers: &axum::http::HeaderMap, address: SocketAddr, hash: &[u8; 32]) -> bool {
    if headers.contains_key(header::ORIGIN)
        || headers.get_all(header::HOST).iter().count() != 1
        || headers.get_all(header::AUTHORIZATION).iter().count() != 1
    {
        return false;
    }
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    if host != Some(address.to_string().as_str())
        && host != Some(format!("localhost:{}", address.port()).as_str())
    {
        return false;
    }
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    if token.len() != 64 {
        return false;
    }
    let candidate: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    bool::from(hash.ct_eq(&candidate))
}
async fn guard(State(context): State<Context>, request: Request, next: Next) -> Response {
    if request.headers().contains_key(header::ORIGIN) {
        return rejection(StatusCode::FORBIDDEN, "workspace.origin");
    }
    if !context.enabled.load(Ordering::Acquire)
        || !authenticated(request.headers(), context.address, &context.hash)
    {
        return rejection(StatusCode::UNAUTHORIZED, "workspace.unauthorized");
    }
    next.run(request).await
}
async fn incoming(
    context: &Context,
    request: Request,
) -> Result<(tokio::sync::OwnedSemaphorePermit, axum::body::Bytes), (StatusCode, &'static str)> {
    if request.headers().contains_key(header::ORIGIN) {
        return Err((StatusCode::FORBIDDEN, "workspace.origin"));
    }
    if !context.enabled.load(Ordering::Acquire)
        || !authenticated(request.headers(), context.address, &context.hash)
    {
        return Err((StatusCode::UNAUTHORIZED, "workspace.unauthorized"));
    }
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "workspace.invalid"));
    }
    let permit = context
        .permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| (StatusCode::TOO_MANY_REQUESTS, "workspace.busy"))?;
    {
        let mut rate = context.rate.lock().unwrap_or_else(|e| e.into_inner());
        if rate.0.elapsed() >= Duration::from_secs(60) {
            *rate = (Instant::now(), 0);
        }
        if rate.1 >= 1024 {
            return Err((StatusCode::TOO_MANY_REQUESTS, "workspace.busy"));
        }
        rate.1 += 1;
    }
    if !super::allowed_async(context.app.clone()).await {
        context.runtime.pause();
        return Err((StatusCode::FORBIDDEN, "workspace.privacy"));
    }
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 256 * 1024),
    )
    .await
    .map_err(|_| (StatusCode::REQUEST_TIMEOUT, "workspace.invalid"))?
    .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "workspace.limit"))?;
    Ok((permit, bytes))
}
async fn deliver(context: &Context, request: WriteRequest) -> Result<Value, ()> {
    if !context.enabled.load(Ordering::Acquire) {
        return Err(());
    }
    let runtime = context.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || runtime.write(request))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}
async fn push(State(context): State<Context>, request: Request) -> Response {
    let (_permit, bytes) = match incoming(&context, request).await {
        Ok(input) => input,
        Err((status, code)) => return rejection(status, code),
    };
    let request = match serde_json::from_slice::<WriteRequest>(&bytes) {
        Ok(request) => request,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "workspace.invalid"),
    };
    match deliver(&context, request).await {
        Ok(receipt) => Json(receipt).into_response(),
        Err(_) => rejection(StatusCode::CONFLICT, "workspace.write_rejected"),
    }
}
fn decoded_name(value: Option<&str>) -> Option<String> {
    let value = value?;
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|s| s.strip_suffix("?="))
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?;
        if bytes.len() > 1024 {
            return None;
        }
        String::from_utf8(bytes).ok()
    } else {
        Some(value.into())
    }
}
async fn mcp(State(context): State<Context>, request: Request) -> Response {
    let headers = request.headers().clone();
    let (_permit, bytes) = match incoming(&context, request).await {
        Ok(input) => input,
        Err((status, code)) => return rejection(status, code),
    };
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "workspace.invalid"),
    };
    let name = decoded_name(headers.get("mcp-name").and_then(|v| v.to_str().ok()));
    let call = match protocol::parse(
        value,
        headers
            .get("mcp-protocol-version")
            .and_then(|v| v.to_str().ok()),
        headers.get("mcp-method").and_then(|v| v.to_str().ok()),
        name.as_deref(),
    ) {
        Ok(call) => call,
        Err(error) => {
            let status = if error.modern && error.code == -32601 {
                StatusCode::NOT_FOUND
            } else if error.code == -32020 || error.code == -32022 {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            };
            return (status, Json(error.reply())).into_response();
        }
    };
    let result = match call.action {
        Action::Discover => {
            json!({"resultType":"complete","supportedVersions":protocol::VERSIONS,"capabilities":{"tools":{}},"_meta":{"io.modelcontextprotocol/serverInfo":protocol::identity()}})
        }
        Action::Initialize(version) => {
            json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":protocol::identity()})
        }
        Action::List => protocol::tools(),
        Action::Ping => json!({"resultType":"complete"}),
        Action::Notification => return StatusCode::ACCEPTED.into_response(),
        Action::Write(request) => match deliver(&context, request).await {
            Ok(receipt) => {
                json!({"resultType":"complete","isError":false,"structuredContent":receipt,"content":[{"type":"text","text":receipt.to_string()}]})
            }
            Err(_) => {
                json!({"resultType":"complete","isError":true,"content":[{"type":"text","text":"workspace.write_rejected"}]})
            }
        },
    };
    Json(json!({"jsonrpc":"2.0","id":call.id,"result":result})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_headers_reject_web_origins_wrong_hosts_duplicates_and_tokens() {
        let address = "127.0.0.1:18771".parse().unwrap();
        let token = "a".repeat(64);
        let hash = Sha256::digest(token.as_bytes()).into();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:18771".parse().unwrap());
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(authenticated(&headers, address, &hash));
        headers.insert(header::ORIGIN, "null".parse().unwrap());
        assert!(!authenticated(&headers, address, &hash));
        headers.remove(header::ORIGIN);
        headers.insert(header::HOST, "evil.invalid:18771".parse().unwrap());
        assert!(!authenticated(&headers, address, &hash));
        headers.insert(header::HOST, "localhost:18771".parse().unwrap());
        assert!(authenticated(&headers, address, &hash));
        headers.append(header::AUTHORIZATION, "Bearer other".parse().unwrap());
        assert!(!authenticated(&headers, address, &hash));
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", "b".repeat(64)).parse().unwrap(),
        );
        assert!(!authenticated(&headers, address, &hash));
    }
    #[test]
    fn token_is_user_bound_and_mcp_surface_cannot_read_or_execute() {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        {
            let a = token(directory.path()).unwrap();
            let b = token(directory.path()).unwrap();
            assert_eq!(*a, *b);
            assert_eq!(a.len(), 64);
            let disk = fs::read(directory.path().join("gateway/token.dpapi")).unwrap();
            assert!(!disk.windows(a.len()).any(|s| s == a.as_bytes()));
        }
        #[cfg(not(windows))]
        assert!(token(directory.path()).is_err());
        let tools = protocol::tools().to_string();
        assert!(!tools.contains("resources/read") && !tools.contains("terminal_input"));
        assert_eq!(
            decoded_name(Some("=?base64?d29ya3NwYWNlX3B1c2g=?=")).as_deref(),
            Some("workspace_push")
        );
    }
}
