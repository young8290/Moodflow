//! Explicit workspace UI, inert inbox and native terminal ownership.
pub mod gateway;
mod terminal;
pub mod tools;
use anyhow::{Result, ensure};
use base64::Engine;
use moodflow_hub_core::{
    domain::{
        chat::{self, ReplyCheck, SafeMode},
        safety::CrisisLexicon,
        validate::BannedWords,
    },
    infra::gateway::{
        AiError, AiGateway, Answer, CompleteRequest, JudgeRequest, Message as AiMessage, Question,
        Scenario,
    },
};
use moodflow_mailbox::{
    Attachment, FrozenModel, Inbox, Kind, Message, Role, Session, Store, WriteRequest,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

pub struct Runtime {
    storage: Mutex<()>,
    streams: Mutex<Inbox>,
    stream_epoch: AtomicU64,
    terminals: Mutex<BTreeMap<String, Arc<terminal::Terminal>>>,
    running: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
    network: Mutex<Option<gateway::Server>>,
    epoch: AtomicU64,
    clipboard: Mutex<Option<crate::vault_platform::ClipboardLease>>,
    safety: Option<(CrisisLexicon, BannedWords)>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("workspace.invalid"))
}
fn root() -> Result<PathBuf> {
    let path = crate::paths::tool_data_dir()?.join("workspace");
    crate::private_fs::ensure(&path)?;
    Ok(path)
}
fn session_reply(session: &Session) -> Value {
    json!({"session":session,"revision":session.revision})
}
fn regular_file(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    metadata.is_file() && !metadata.file_type().is_symlink()
}
fn read_asset(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        regular_file(&fs::symlink_metadata(path)?),
        "workspace.invalid"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(moodflow_mailbox::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= moodflow_mailbox::MAX_FILE_BYTES,
        "workspace.limit"
    );
    Ok(bytes)
}
fn endpoint_id(endpoint: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(endpoint.as_bytes()))
}
pub fn allowed(app: &AppHandle) -> bool {
    !crate::sim::demo() && crate::capture::worker_allowed(app)
}
struct Running {
    runtime: Arc<Runtime>,
    id: String,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.runtime
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
impl Runtime {
    pub fn pause(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        if let Ok(running) = self.running.try_lock() {
            for cancel in running.values() {
                cancel.store(true, Ordering::Release);
            }
        }
        if let Ok(mut inbox) = self.streams.try_lock() {
            inbox.clear();
        }
        self.sweep_clipboard(false);
    }
    fn sweep_clipboard(&self, allowed: bool) {
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
    pub fn stop_all(&self) {
        self.pause();
        self.network
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.terminals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    pub fn write(&self, request: WriteRequest) -> Result<Value> {
        let epoch = self.epoch.load(Ordering::Acquire);
        let _lock = self.storage.lock().unwrap_or_else(|e| e.into_inner());
        ensure!(
            self.epoch.load(Ordering::Acquire) == epoch,
            "workspace.privacy"
        );
        let mut inbox = self.streams.lock().unwrap_or_else(|e| e.into_inner());
        if self.stream_epoch.swap(epoch, Ordering::AcqRel) != epoch {
            inbox.clear();
        }
        inbox.accept(&Store::new(root()?), request, now())
    }
    pub fn execute(
        &self,
        app: &AppHandle,
        owner: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if method == "gateway_status" {
            let network = self.network.lock().unwrap_or_else(|e| e.into_inner());
            return Ok(
                json!({"enabled":network.as_ref().is_some_and(gateway::Server::is_enabled),"address":network.as_ref().filter(|s|s.is_enabled()).map(|s|s.address.clone()),"terminal_supported":cfg!(windows)}),
            );
        }
        if method == "gateway_stop" {
            self.network
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            self.pause();
            return Ok(json!({}));
        }
        ensure!(allowed(app), "workspace.privacy");
        if method == "chat_cancel" {
            if let Some(cancel) = self
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(string(params, "id")?)
            {
                cancel.store(true, Ordering::Release);
            }
            return Ok(json!({"cancelled":true}));
        }
        if method == "gateway_rotate" {
            ensure!(
                params["confirm"].as_bool() == Some(true),
                "workspace.invalid"
            );
            self.network
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            self.pause();
            gateway::rotate(&root()?)?;
            return Ok(json!({"rotated":true}));
        }
        if method.starts_with("terminal_") && method != "terminal_start" {
            let id = string(params, "id")?;
            moodflow_mailbox::valid_id(id)?;
            if method == "terminal_end" {
                self.terminals
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(id);
                return Ok(json!({}));
            }
            let terminal = self
                .terminals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("workspace.no_terminal"))?;
            return match method {
                "terminal_read" => terminal.poll(params["after"].as_u64().unwrap_or(0)),
                "terminal_input" => {
                    let encoded = string(params, "data")?;
                    ensure!(encoded.len() <= 5500, "workspace.limit");
                    terminal.input(base64::engine::general_purpose::STANDARD.decode(encoded)?)?;
                    Ok(json!({}))
                }
                "terminal_resize" => {
                    terminal.resize(
                        u16::try_from(params["cols"].as_u64().unwrap_or(0))?,
                        u16::try_from(params["rows"].as_u64().unwrap_or(0))?,
                    )?;
                    Ok(json!({}))
                }
                _ => anyhow::bail!("workspace.invalid"),
            };
        }
        if method == "gateway_token" || method == "copy_text" {
            let token = if method == "gateway_token" {
                gateway::token(&root()?)?
            } else {
                let text = string(params, "text")?;
                ensure!(text.encode_utf16().count() <= 16384, "workspace.limit");
                zeroize::Zeroizing::new(text.to_owned())
            };
            #[cfg(windows)]
            let owner = app
                .get_webview_window(owner)
                .ok_or_else(|| anyhow::anyhow!("workspace.invalid"))?
                .hwnd()?
                .0 as usize;
            #[cfg(not(windows))]
            let owner = {
                let _ = owner;
                0_usize
            };
            let lease = crate::vault_platform::copy_sensitive(owner, &token)
                .map_err(|_| anyhow::anyhow!("workspace.failed"))?;
            *self.clipboard.lock().unwrap_or_else(|e| e.into_inner()) = Some(lease);
            return Ok(json!({"copied":true}));
        }
        let _lock = self.storage.lock().unwrap_or_else(|e| e.into_inner());
        let root = root()?;
        let store = Store::new(root.clone());
        if method == "list" {
            let (available, unreadable) = store.available_sessions()?;
            let sessions=available.into_iter().map(|s|json!({"id":s.id,"kind":s.kind,"title":s.title,"created_at":s.created_at,"updated_at":s.updated_at,"archived":s.archived,"unread":s.unread,"count":s.messages.len(),"model":s.model,"revision":s.revision})).collect::<Vec<_>>();
            return Ok(json!({"sessions":sessions,"unreadable":unreadable}));
        }
        if method == "search" {
            let request: moodflow_mailbox::catalog::SearchRequest =
                serde_json::from_value(params.clone())?;
            return Ok(serde_json::to_value(store.search(&request)?)?);
        }
        if method == "manage" {
            use moodflow_mailbox::catalog::{Change, ChangeResult, Selection};
            let selection: Vec<Selection> = serde_json::from_value(params["selection"].clone())?;
            let change: Change = serde_json::from_value(params["change"].clone())?;
            ensure!(
                !selection.is_empty() && selection.len() <= 50,
                "workspace.limit"
            );
            let mut unique = std::collections::BTreeSet::new();
            for item in &selection {
                moodflow_mailbox::valid_id(&item.id)?;
                ensure!(
                    unique.insert(&item.id) && item.revision.len() == 64,
                    "workspace.invalid"
                );
            }
            // Snapshot activity while holding storage. Release these short-lived
            // guards so cancellation and terminal polling remain responsive.
            let mut busy_ids = self
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            busy_ids.extend(
                self.terminals
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .keys()
                    .cloned(),
            );
            let batch_epoch = self.epoch.load(Ordering::Acquire);
            let mut outcomes = Vec::new();
            for item in selection {
                ensure!(
                    allowed(app) && self.epoch.load(Ordering::Acquire) == batch_epoch,
                    "workspace.privacy"
                );
                if busy_ids.contains(&item.id) {
                    outcomes.push(ChangeResult {
                        id: item.id,
                        status: "busy".into(),
                        revision: None,
                    });
                } else {
                    outcomes.extend(store.change_sessions(&[item], &change, now())?);
                }
            }
            return Ok(serde_json::to_value(outcomes)?);
        }
        if method == "rename" {
            let selection: moodflow_mailbox::catalog::Selection =
                serde_json::from_value(params["selection"].clone())?;
            let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(!running.contains_key(&selection.id), "workspace.busy");
            let session = store.rename_session(&selection, string(params, "title")?, now())?;
            return Ok(session_reply(&session));
        }
        if method == "transcript" {
            use moodflow_mailbox::catalog::TranscriptFormat;
            let selection = serde_json::from_value(params["selection"].clone())?;
            let format: TranscriptFormat = serde_json::from_value(params["format"].clone())?;
            let bytes = store.transcript(
                &selection,
                format,
                params["include_instructions"].as_bool().unwrap_or(false),
            )?;
            let destination = PathBuf::from(string(params, "path")?);
            let mut temporary = tempfile::NamedTempFile::new_in(
                destination
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("workspace.invalid"))?,
            )?;
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            temporary.persist_noclobber(destination)?;
            return Ok(json!({"exported":true}));
        }
        if method == "create" {
            let kind: Kind = serde_json::from_value(params["kind"].clone())?;
            let mut session = Session::new(kind, string(params, "title")?.trim().into(), now());
            session.instructions = params["instructions"].as_str().unwrap_or("").into();
            let session = store.save(&session, None)?;
            return Ok(session_reply(&session));
        }
        let id = string(params, "id")?;
        let mut session = store.get(id)?;
        if method == "get" {
            return Ok(session_reply(&session));
        }
        if method == "terminal_start" {
            ensure!(
                session.kind == Kind::Terminal && !session.archived,
                "workspace.invalid"
            );
            let directory = PathBuf::from(string(params, "directory")?);
            let mut terminals = self.terminals.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                !terminals.contains_key(id) && terminals.len() < 8,
                "workspace.terminal_busy"
            );
            terminals.insert(
                id.into(),
                Arc::new(terminal::Terminal::spawn(
                    &directory,
                    params["cols"]
                        .as_u64()
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(96),
                    params["rows"]
                        .as_u64()
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(24),
                )?),
            );
            return Ok(json!({"started":true}));
        }
        let expected = string(params, "revision")?;
        ensure!(session.revision == expected, "workspace.conflict");
        match method {
            "update" => {
                session.title = string(params, "title")?.trim().into();
                session.archived = params["archived"].as_bool().unwrap_or(false);
                session.unread = params["unread"].as_bool().unwrap_or(false);
                session.updated_at = now();
            }
            "note" => {
                session.append(
                    vec![Message::new(
                        Role::Note,
                        string(params, "text")?.into(),
                        now(),
                    )],
                    None,
                    now(),
                )?;
            }
            "receive_file" => {
                ensure!(session.kind != Kind::Chat, "workspace.invalid");
                let source = PathBuf::from(string(params, "path")?);
                let metadata = fs::symlink_metadata(&source)?;
                ensure!(
                    regular_file(&metadata) && metadata.len() <= moodflow_mailbox::MAX_FILE_BYTES,
                    "workspace.limit"
                );
                let asset_dir = root.join("files");
                crate::private_fs::ensure(&asset_dir)?;
                let mut total = metadata.len();
                for (n, file) in fs::read_dir(&asset_dir)?.enumerate() {
                    ensure!(n < 10000, "workspace.limit");
                    total = total.saturating_add(file?.metadata()?.len());
                }
                ensure!(
                    total <= moodflow_mailbox::MAX_TOTAL_BYTES,
                    "workspace.limit"
                );
                let asset_id = uuid::Uuid::new_v4().to_string();
                let mut temporary = tempfile::NamedTempFile::new_in(&asset_dir)?;
                let length = std::io::copy(
                    &mut fs::File::open(&source)?.take(moodflow_mailbox::MAX_FILE_BYTES + 1),
                    &mut temporary,
                )?;
                ensure!(length == metadata.len(), "workspace.changed");
                temporary.as_file().sync_all()?;
                let bytes = read_asset(temporary.path())?;
                use sha2::{Digest, Sha256};
                let file = Attachment {
                    id: asset_id.clone(),
                    name: source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| anyhow::anyhow!("workspace.invalid"))?
                        .into(),
                    bytes: length,
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                };
                let mut message = Message::new(
                    Role::Note,
                    params["text"].as_str().unwrap_or("").into(),
                    now(),
                );
                message.files.push(file);
                session.append(vec![message], None, now())?;
                let path = asset_dir.join(format!("{asset_id}.bin"));
                temporary.persist_noclobber(&path)?;
                match store.save(&session, Some(expected)) {
                    Ok(saved) => return Ok(session_reply(&saved)),
                    Err(error) => {
                        let _ = fs::remove_file(path);
                        return Err(error);
                    }
                }
            }
            "export_file" => {
                let file_id = string(params, "file_id")?;
                let file = session
                    .messages
                    .iter()
                    .flat_map(|m| &m.files)
                    .find(|f| f.id == file_id)
                    .ok_or_else(|| anyhow::anyhow!("workspace.missing"))?;
                let path = root.join("files").join(format!("{}.bin", file.id));
                let destination = PathBuf::from(string(params, "path")?);
                let mut temporary = tempfile::NamedTempFile::new_in(
                    destination
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("workspace.invalid"))?,
                )?;
                let length = std::io::copy(
                    &mut fs::File::open(path)?.take(moodflow_mailbox::MAX_FILE_BYTES + 1),
                    &mut temporary,
                )?;
                ensure!(length == file.bytes, "workspace.changed");
                use sha2::{Digest, Sha256};
                ensure!(
                    format!("{:x}", Sha256::digest(read_asset(temporary.path())?)) == file.sha256,
                    "workspace.changed"
                );
                temporary.as_file().sync_all()?;
                temporary.persist_noclobber(destination)?;
                return Ok(json!({"exported":true}));
            }
            "remove" => {
                ensure!(
                    !self
                        .terminals
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains_key(id)
                        && !self
                            .running
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .contains_key(id),
                    "workspace.terminal_busy"
                );
                store.remove(id, expected)?;
                for file in session.messages.iter().flat_map(|m| &m.files) {
                    let _ = fs::remove_file(root.join("files").join(format!("{}.bin", file.id)));
                }
                return Ok(json!({"removed":true}));
            }
            _ => anyhow::bail!("workspace.invalid"),
        }
        let saved = store.save(&session, Some(expected))?;
        Ok(session_reply(&saved))
    }
    pub async fn start_gateway(self: &Arc<Self>, app: AppHandle, port: u16) -> Result<Value> {
        ensure!(allowed_async(app.clone()).await, "workspace.privacy");
        let token = gateway::token(&root()?)?;
        let mut network = self.network.lock().unwrap_or_else(|e| e.into_inner());
        if network.as_ref().is_some_and(|server| !server.is_enabled()) {
            network.take();
        }
        ensure!(network.is_none(), "workspace.gateway_busy");
        let server = gateway::Server::start(app, self.clone(), port, &token)?;
        let address = server.address.clone();
        *network = Some(server);
        Ok(json!({"enabled":true,"address":address}))
    }
    pub async fn chat(self: &Arc<Self>, app: AppHandle, params: Value) -> Result<Value> {
        ensure!(
            params["consent"].as_bool() == Some(true),
            "workspace.consent"
        );
        ensure!(allowed_async(app.clone()).await, "workspace.privacy");
        let id = string(&params, "id")?.to_owned();
        let body = string(&params, "text")?.trim().to_owned();
        let message_id = string(&params, "message_id")?.to_owned();
        moodflow_mailbox::valid_id(&message_id)?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                !running.contains_key(&id) && running.len() < 4,
                "workspace.busy"
            );
            running.insert(id.clone(), cancel.clone());
        }
        let _running = Running {
            runtime: self.clone(),
            id: id.clone(),
        };
        let epoch = self.epoch.load(Ordering::Acquire);
        let (lex, banned) = self
            .safety
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace.safety_unavailable"))?;
        let ai = app.state::<Arc<crate::gateway::Ai>>().inner().clone();
        let (source, masked) = ai.view();
        ensure!(
            source != crate::gateway::Source::None,
            "workspace.ai_unavailable"
        );
        let side = masked
            .llm
            .ok_or_else(|| anyhow::anyhow!("workspace.ai_unavailable"))?;
        let gateway = ai.gateway();
        let session = {
            let _lock = self.storage.lock().unwrap_or_else(|e| e.into_inner());
            let store = Store::new(root()?);
            let mut session = store.get(&id)?;
            ensure!(
                session.kind == Kind::Chat && !session.archived,
                "workspace.invalid"
            );
            ensure!(
                session.messages.len() < moodflow_mailbox::MAX_MESSAGES,
                "workspace.limit"
            );
            let frozen = if let Some(frozen) = &session.model {
                ensure!(
                    frozen.endpoint == endpoint_id(&side.base_url)
                        && side.models.contains(&frozen.model),
                    "workspace.model_changed"
                );
                frozen.clone()
            } else {
                let model = side
                    .models
                    .iter()
                    .find(|name| {
                        gateway
                            .models()
                            .iter()
                            .any(|m| m.model == **name && m.available && !m.breaker_open)
                    })
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("workspace.ai_unavailable"))?;
                FrozenModel {
                    endpoint: endpoint_id(&side.base_url),
                    model,
                }
            };
            let previous = session.revision.clone();
            if let Some(existing) = session.messages.iter().find(|m| m.id == message_id) {
                ensure!(
                    existing.role == Role::User && existing.body == body,
                    "workspace.conflict"
                );
                if session
                    .messages
                    .iter()
                    .any(|m| m.reply_to.as_deref() == Some(message_id.as_str()))
                {
                    return Ok(session_reply(&session));
                }
            } else {
                let user = Message {
                    id: message_id.clone(),
                    role: Role::User,
                    body: body.clone(),
                    created_at: now(),
                    reply_to: None,
                    mock: false,
                    files: vec![],
                };
                ensure!(
                    session.messages.len() + 2 <= moodflow_mailbox::MAX_MESSAGES,
                    "workspace.limit"
                );
                session.append(vec![user], Some(frozen), now())?;
                session = store.save(&session, Some(&previous))?;
            }
            session
        };
        let messages =
            context_messages(&session, params["include_history"].as_bool() == Some(true));
        let checked = messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if chat::lexicon_hit(&checked, lex, SafeMode::Off) {
            show_safety(&app);
            anyhow::bail!("workspace.safety");
        }
        let request = JudgeRequest {
            questions: vec![Question::Crisis],
            state: [("message".into(), json!(checked))].into_iter().collect(),
        };
        let verdict = self.guarded(&app, epoch, &cancel, ai.judge(request)).await;
        if verdict.as_ref().is_ok_and(|v|matches!(v.answers.get(&Question::Crisis),Some(Answer::Noul{p}) if *p>=chat::JEV_CRISIS_THRESHOLD)) { show_safety(&app); anyhow::bail!("workspace.safety"); }
        if self.epoch.load(Ordering::Acquire) != epoch || cancel.load(Ordering::Acquire) {
            anyhow::bail!("workspace.cancelled");
        }
        ensure!(allowed_async(app.clone()).await, "workspace.privacy");
        let frozen = session
            .model
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace.model_changed"))?;
        let request = CompleteRequest {
            scenario: Scenario::Chat,
            prompt_ver: "workspace/user-instructions-v1".into(),
            messages,
        };
        let answer = self
            .guarded(
                &app,
                epoch,
                &cancel,
                gateway.complete_pinned(request, &frozen.model),
            )
            .await;
        ensure!(!cancel.load(Ordering::Acquire), "workspace.cancelled");
        let answer = answer.map_err(|_| anyhow::anyhow!("workspace.ai_failed"))?;
        let body = match chat::check_reply(&answer.text, banned, lex) {
            ReplyCheck::Ok(_) => answer.text.trim().to_owned(),
            ReplyCheck::Banned => anyhow::bail!("workspace.invalid_output"),
            ReplyCheck::Unsafe => {
                show_safety(&app);
                anyhow::bail!("workspace.safety");
            }
        };
        ensure!(
            !cancel.load(Ordering::Acquire)
                && self.epoch.load(Ordering::Acquire) == epoch
                && allowed_async(app.clone()).await,
            "workspace.cancelled"
        );
        let _lock = self.storage.lock().unwrap_or_else(|e| e.into_inner());
        let store = Store::new(root()?);
        let mut latest = store.get(&id)?;
        ensure!(
            latest.model == session.model && !latest.archived,
            "workspace.conflict"
        );
        ensure!(
            latest.messages.iter().any(|m| m.id == message_id),
            "workspace.conflict"
        );
        let previous = latest.revision.clone();
        let mut reply = Message::new(Role::Assistant, body, now());
        reply.reply_to = Some(message_id);
        reply.mock = source == crate::gateway::Source::DevMock;
        latest.append(vec![reply], None, now())?;
        let saved = store.save(&latest, Some(&previous))?;
        Ok(
            json!({"session":saved,"revision":saved.revision,"mock":source==crate::gateway::Source::DevMock}),
        )
    }
    async fn guarded<F, T>(
        &self,
        app: &AppHandle,
        epoch: u64,
        cancel: &AtomicBool,
        future: F,
    ) -> Result<T, AiError>
    where
        F: std::future::Future<Output = Result<T, AiError>>,
    {
        tokio::pin!(future);
        let started = Instant::now();
        #[cfg(windows)]
        let mut awake = moodflow_capsule::capture::SleepGuard::default();
        loop {
            tokio::select! {
                result=&mut future=>return result,
                _=tokio::time::sleep(std::time::Duration::from_millis(250))=>{
                    #[cfg(windows)]
                    if !crate::capture::awake(&mut awake) { return Err(AiError::BadRequest); }
                    if cancel.load(Ordering::Acquire) || started.elapsed()>std::time::Duration::from_secs(60) || self.epoch.load(Ordering::Acquire)!=epoch || !allowed_async(app.clone()).await { return Err(AiError::BadRequest); }
                }
            }
        }
    }
}
fn show_safety(app: &AppHandle) {
    if let Some(chat) = app.try_state::<crate::chat::Chat>()
        && let Some(service) = &chat.0
    {
        let _ = service.open_safety();
    }
}
fn context_messages(session: &Session, history: bool) -> Vec<AiMessage> {
    let mut messages = Vec::new();
    if !session.instructions.trim().is_empty() {
        messages.push(AiMessage {
            role: "system".into(),
            content: session.instructions.clone(),
        });
    }
    let mut selected = Vec::new();
    let mut units = session.instructions.encode_utf16().count();
    for item in session
        .messages
        .iter()
        .rev()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant))
    {
        let length = item.body.encode_utf16().count();
        if units + length > 32000 {
            break;
        }
        units += length;
        selected.push(AiMessage {
            role: if item.role == Role::User {
                "user"
            } else {
                "assistant"
            }
            .into(),
            content: item.body.clone(),
        });
        if !history {
            break;
        }
    }
    selected.reverse();
    messages.extend(selected);
    messages
}
impl Runtime {
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
}
pub async fn allowed_async(app: AppHandle) -> bool {
    tauri::async_runtime::spawn_blocking(move || allowed(&app))
        .await
        .unwrap_or(false)
}
pub fn start(app: &AppHandle) {
    let safety = crate::paths::hub_template_dirs()
        .and_then(|dirs| Ok((CrisisLexicon::load(&dirs)?, BannedWords::load(&dirs)?)))
        .ok();
    let runtime = Arc::new(Runtime {
        storage: Mutex::new(()),
        streams: Mutex::new(Inbox::default()),
        stream_epoch: AtomicU64::new(0),
        terminals: Mutex::new(BTreeMap::new()),
        running: Mutex::new(BTreeMap::new()),
        network: Mutex::new(None),
        epoch: AtomicU64::new(0),
        clipboard: Mutex::new(None),
        safety,
    });
    app.manage(runtime.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let current = runtime.clone();
            let allowed = crate::vault::allowed(&app);
            let _ = tauri::async_runtime::spawn_blocking(move || current.sweep_clipboard(allowed))
                .await;
        }
    });
}
