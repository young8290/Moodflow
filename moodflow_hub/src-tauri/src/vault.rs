//! Vault runtime. Keys live only for a short native lease; plaintext is not cached.
use crate::{error::UiError, state::AppState, vault_platform};
use moodflow_vault::{Entry, FileRecord, Key, KeyWrap, Snapshot, Store, session::Session};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
use zeroize::{Zeroize, Zeroizing};

struct Inner {
    session: Session,
    clipboard: Option<vault_platform::ClipboardLease>,
    failures: u32,
    next_attempt: Instant,
}
pub struct Runtime {
    root: Option<PathBuf>,
    inner: Mutex<Inner>,
}

/// Clear command copies and parsed string fields on every success / error path.
pub struct SensitiveJson(pub Value);
impl Drop for SensitiveJson {
    fn drop(&mut self) {
        wipe_json(&mut self.0);
    }
}
fn wipe_json(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(wipe_json),
        Value::Object(o) => o.values_mut().for_each(wipe_json),
        _ => {}
    }
}
fn err(code: &str) -> UiError {
    UiError::new(code, code)
}
fn vault_error(error: moodflow_vault::Error) -> UiError {
    use moodflow_vault::Error;
    err(match error {
        Error::Conflict => "vault.conflict",
        Error::WeakPassword => "vault.weak_password",
        Error::Locked => "vault.locked",
        Error::Limit => "vault.limit",
        _ => "vault.failed",
    })
}
fn string<'a>(p: &'a Value, key: &str) -> Result<&'a str, UiError> {
    p[key].as_str().ok_or_else(|| err("vault.invalid"))
}
fn root() -> Option<PathBuf> {
    let hub = crate::paths::hub_data_dir().ok()?;
    Some(if std::env::var_os(crate::paths::DATA_DIR_ENV).is_some() {
        hub.join("vault")
    } else {
        hub.parent()?.join("vault")
    })
}
pub fn allowed(app: &AppHandle) -> bool {
    vault_platform::desktop_available()
        && app.try_state::<AppState>().is_some_and(|state| {
            !state
                .status
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .paused
        })
}

fn clear(inner: &mut Inner, force: bool) {
    let Some(lease) = inner.clipboard.as_ref() else {
        return;
    };
    if !force && Instant::now() < lease.expires {
        return;
    }
    match vault_platform::clear_sensitive(lease) {
        vault_platform::Clear::Done => inner.clipboard = None,
        #[cfg(windows)]
        vault_platform::Clear::Changed => inner.clipboard = None,
        #[cfg(windows)]
        vault_platform::Clear::Busy => {}
    }
}
impl Runtime {
    fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            inner: Mutex::new(Inner {
                session: Session::default(),
                clipboard: None,
                failures: 0,
                next_attempt: Instant::now(),
            }),
        }
    }
    pub fn lock(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.session.lock();
        clear(&mut inner, true);
    }
    pub fn remaining_ms(&self, allowed: bool) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session
            .remaining_ms(allowed)
    }
    pub fn sweep(&self, allowed: bool) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let unlocked = inner.session.check(allowed);
        clear(&mut inner, !unlocked);
    }
    pub fn execute(
        &self,
        method: &str,
        p: &Value,
        owner: usize,
        allowed: bool,
    ) -> Result<Value, UiError> {
        if method == "lock" {
            self.lock();
            return Ok(json!({"locked":true}));
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !allowed {
            inner.session.lock();
            clear(&mut inner, true);
        }
        if !vault_platform::supported() || self.root.is_none() {
            return if method == "status" {
                Ok(
                    json!({"supported":false,"exists":false,"unlocked":false,"blocked":true,"remaining_ms":0}),
                )
            } else {
                Err(err("vault.unsupported"))
            };
        }
        let root = self.root.as_ref().ok_or_else(|| err("vault.failed"))?;
        crate::private_fs::ensure(root).map_err(|_| err("vault.failed"))?;
        let store = Store::new(root.clone());
        if method == "status" {
            return Ok(
                json!({"supported":true,"exists":store.exists().map_err(vault_error)?,"unlocked":inner.session.check(allowed),"blocked":!allowed,"remaining_ms":inner.session.remaining_ms(allowed)}),
            );
        }
        if !allowed {
            return Err(err("library.privacy"));
        }
        if crate::sim::demo() {
            return Err(err("vault.demo"));
        }
        if method == "create" {
            if store.exists().map_err(vault_error)? {
                return Err(err("vault.conflict"));
            }
            let master = Zeroizing::new(string(p, "master")?.to_owned());
            let id = moodflow_vault::new_id();
            let key = Key::generate().map_err(vault_error)?;
            let wrap = KeyWrap::create(&key, &master, &id).map_err(vault_error)?;
            let bytes = Zeroizing::new(serde_json::to_vec(&wrap).map_err(|_| err("vault.failed"))?);
            let protected = crate::secrets::protect(&bytes).map_err(|_| err("vault.failed"))?;
            let file = FileRecord::create(id.clone(), protected, &key).map_err(vault_error)?;
            let revision = store
                .write(file, &key, &Snapshot::empty(), None)
                .map_err(vault_error)?;
            inner.session.unlock(id, key);
            return Ok(json!({"revision":revision}));
        }
        let (mut file, revision) = store.read().map_err(vault_error)?;
        if method == "unlock" {
            if Instant::now() < inner.next_attempt {
                return Err(err("vault.cooldown"));
            }
            let master = Zeroizing::new(string(p, "master")?.to_owned());
            let unlock = (|| {
                let bytes = crate::secrets::unprotect(&file.protected_key).map_err(|_| ())?;
                let wrap: KeyWrap = serde_json::from_slice(&bytes).map_err(|_| ())?;
                let key = wrap.unlock(&master, &file.id).map_err(|_| ())?;
                file.decrypt(&key).map_err(|_| ())?;
                Ok::<_, ()>(key)
            })();
            match unlock {
                Ok(key) => {
                    inner.failures = 0;
                    inner.next_attempt = Instant::now();
                    inner.session.unlock(file.id, key);
                    return Ok(json!({"revision":revision}));
                }
                Err(()) => {
                    inner.session.lock();
                    inner.failures = inner.failures.saturating_add(1);
                    inner.next_attempt =
                        Instant::now() + Duration::from_secs(1_u64 << inner.failures.min(5));
                    return Err(err("vault.unlock_failed"));
                }
            }
        }
        let key = inner.session.key(allowed, &file.id).map_err(vault_error)?;
        if !["list", "new", "generate", "copy_draft"].contains(&method)
            && p["revision"].as_str() != Some(&revision)
        {
            return Err(err("vault.conflict"));
        }
        if method == "generate" {
            let password = moodflow_vault::crypto::generate_password(
                p["length"].as_u64().unwrap_or(24) as usize,
                p["symbols"].as_bool().unwrap_or(true),
            )
            .map_err(vault_error)?;
            return Ok(json!({"value":password.as_str(),"reveal_ms":15000}));
        }
        if method == "copy_draft" {
            let text = Zeroizing::new(string(p, "text")?.to_owned());
            let lease = vault_platform::copy_sensitive(owner, &text)
                .map_err(|_| err("vault.clipboard_failed"))?;
            inner.clipboard = Some(lease);
            return Ok(json!({"copied":true}));
        }
        let mut snapshot = file.decrypt(key).map_err(vault_error)?;
        match method {
            "list" => Ok(
                json!({"revision":revision,"items":snapshot.entries.iter().map(|e|json!({"id":e.id,"title":e.title})).collect::<Vec<_>>()}),
            ),
            "new" => Ok(
                json!({"revision":revision,"entry":Entry::new(),"has_password":false,"has_totp":false}),
            ),
            "get" => {
                let id = string(p, "id")?;
                let entry = snapshot
                    .entries
                    .iter()
                    .find(|e| e.id == id)
                    .ok_or_else(|| err("vault.invalid"))?;
                Ok(
                    json!({"revision":revision,"entry":{"id":entry.id,"title":entry.title,"account":entry.account,"password":"","url":entry.url,"notes":entry.notes,"totp":null},"has_password":!entry.password.is_empty(),"has_totp":entry.totp.is_some(),"totp_settings":entry.totp.as_ref().map(|t|json!({"algorithm":t.algorithm,"digits":t.digits,"period":t.period}))}),
                )
            }
            "save" => {
                let mut entry: Entry =
                    serde_json::from_value(p["entry"].clone()).map_err(|_| err("vault.invalid"))?;
                if let Some(old) = snapshot.entries.iter_mut().find(|e| e.id == entry.id) {
                    if p["keep_password"].as_bool() == Some(true) {
                        entry.password = std::mem::take(&mut old.password);
                    }
                    if p["keep_totp"].as_bool() == Some(true) {
                        entry.totp = old.totp.take();
                        if let Some(totp) = entry.totp.as_mut()
                            && let Some(settings) = p.get("totp_settings")
                        {
                            totp.algorithm = serde_json::from_value(settings["algorithm"].clone())
                                .map_err(|_| err("vault.invalid"))?;
                            totp.digits = settings["digits"]
                                .as_u64()
                                .and_then(|n| u8::try_from(n).ok())
                                .ok_or_else(|| err("vault.invalid"))?;
                            totp.period = settings["period"]
                                .as_u64()
                                .and_then(|n| u32::try_from(n).ok())
                                .ok_or_else(|| err("vault.invalid"))?;
                        }
                    }
                }
                if let Some(uri) = p["totp_uri"].as_str().filter(|s| !s.is_empty()) {
                    entry.totp = Some(moodflow_vault::Totp::from_uri(uri).map_err(vault_error)?);
                }
                entry.validate().map_err(vault_error)?;
                let id = entry.id.clone();
                if let Some(index) = snapshot.entries.iter().position(|e| e.id == id) {
                    snapshot.entries[index] = entry;
                } else {
                    snapshot.entries.push(entry);
                }
                let revision = store
                    .write(file, key, &snapshot, Some(&revision))
                    .map_err(vault_error)?;
                Ok(json!({"revision":revision,"id":id}))
            }
            "delete" => {
                let id = string(p, "id")?;
                let len = snapshot.entries.len();
                snapshot.entries.retain(|e| e.id != id);
                if len == snapshot.entries.len() {
                    return Err(err("vault.invalid"));
                }
                let revision = store
                    .write(file, key, &snapshot, Some(&revision))
                    .map_err(vault_error)?;
                Ok(json!({"revision":revision}))
            }
            "reveal" | "copy" => {
                let id = string(p, "id")?;
                let entry = snapshot
                    .entries
                    .iter()
                    .find(|e| e.id == id)
                    .ok_or_else(|| err("vault.invalid"))?;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| err("vault.failed"))?
                    .as_secs();
                let field = string(p, "field")?;
                let value = match field {
                    "password" => Zeroizing::new(entry.password.clone()),
                    "account" => Zeroizing::new(entry.account.clone()),
                    "notes" => Zeroizing::new(entry.notes.clone()),
                    "url" => Zeroizing::new(entry.url.clone()),
                    "totp" => entry
                        .totp
                        .as_ref()
                        .ok_or_else(|| err("vault.invalid"))?
                        .code(now)
                        .map_err(vault_error)?,
                    _ => return Err(err("vault.invalid")),
                };
                if method == "reveal" {
                    return Ok(
                        json!({"value":value.as_str(),"reveal_ms":15000,"totp_remaining":entry.totp.as_ref().map(|t|t.period-(now%t.period as u64) as u32)}),
                    );
                }
                let lease = vault_platform::copy_sensitive(owner, &value)
                    .map_err(|_| err("vault.clipboard_failed"))?;
                inner.clipboard = Some(lease);
                Ok(json!({"copied":true}))
            }
            "change_master" => {
                let master = Zeroizing::new(string(p, "master")?.to_owned());
                let wrap = KeyWrap::create(key, &master, &file.id).map_err(vault_error)?;
                let bytes =
                    Zeroizing::new(serde_json::to_vec(&wrap).map_err(|_| err("vault.failed"))?);
                file.protected_key =
                    crate::secrets::protect(&bytes).map_err(|_| err("vault.failed"))?;
                let revision = store
                    .write(file, key, &snapshot, Some(&revision))
                    .map_err(vault_error)?;
                Ok(json!({"revision":revision}))
            }
            _ => Err(err("vault.invalid")),
        }
    }
}

pub fn start(app: &AppHandle) {
    let runtime = Arc::new(Runtime::new(root()));
    app.manage(runtime.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let runtime = runtime.clone();
            let policy = allowed(&app);
            // OS clipboard calls and mutex contention never block Tauri's async executor.
            let _ = tauri::async_runtime::spawn_blocking(move || runtime.sweep(policy)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sensitive_json_clears_nested_string_storage() {
        let mut value = json!({"entry":{"password":"sample-secret"},"array":["sample-totp"]});
        wipe_json(&mut value);
        assert!(value["entry"]["password"].as_str().unwrap().is_empty());
        assert!(value["array"][0].as_str().unwrap().is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn windows_key_wrap_is_dpapi_protected_and_session_lock_revokes_access() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(Some(dir.path().to_owned()));
        runtime
            .execute(
                "create",
                &json!({"master":"test master phrase sample"}),
                0,
                true,
            )
            .unwrap();
        let data = std::fs::read(dir.path().join("vault.mfv")).unwrap();
        assert!(!String::from_utf8_lossy(&data).contains("test master phrase sample"));
        runtime.execute("list", &json!({}), 0, true).unwrap();
        let fresh = runtime.execute("new", &json!({}), 0, true).unwrap();
        let mut entry = fresh["entry"].clone();
        entry["title"] = json!("fixture title");
        entry["account"] = json!("fixture account");
        entry["password"] = json!("fixture password");
        // Public RFC test input, never a real account credential.
        let fixture_seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        entry["totp"] = json!({"secret":fixture_seed,"algorithm":"SHA256","digits":8,"period":60});
        let saved = runtime
            .execute(
                "save",
                &json!({"entry":entry,"revision":fresh["revision"]}),
                0,
                true,
            )
            .unwrap();
        let id = entry["id"].clone();
        let view = runtime
            .execute(
                "get",
                &json!({"id":id,"revision":saved["revision"]}),
                0,
                true,
            )
            .unwrap();
        assert_eq!(view["entry"]["password"], "");
        assert_eq!(view["totp_settings"]["algorithm"], "SHA256");
        assert_eq!(view["totp_settings"]["period"], 60);
        let updated=runtime.execute("save",&json!({"entry":view["entry"],"revision":saved["revision"],"keep_password":true,"keep_totp":true,"totp_settings":{"algorithm":"SHA256","digits":8,"period":30}}),0,true).unwrap();
        let revealed = runtime
            .execute(
                "reveal",
                &json!({"id":id,"revision":updated["revision"],"field":"password"}),
                0,
                true,
            )
            .unwrap();
        assert_eq!(revealed["value"], "fixture password");
        let current = runtime
            .execute(
                "get",
                &json!({"id":id,"revision":updated["revision"]}),
                0,
                true,
            )
            .unwrap();
        assert_eq!(current["totp_settings"]["period"], 30);
        assert!(
            runtime
                .execute(
                    "get",
                    &json!({"id":id,"revision":saved["revision"]}),
                    0,
                    true
                )
                .is_err()
        );
        let encrypted = std::fs::read_to_string(dir.path().join("vault.mfv")).unwrap();
        for private in [
            "fixture title",
            "fixture account",
            "fixture password",
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
        ] {
            assert!(!encrypted.contains(private));
        }
        runtime.lock();
        assert!(runtime.execute("list", &json!({}), 0, true).is_err());
        runtime
            .execute(
                "unlock",
                &json!({"master":"test master phrase sample"}),
                0,
                true,
            )
            .unwrap();
        assert!(runtime.execute("list", &json!({}), 0, false).is_err());
        let (record, _) = Store::new(dir.path().to_owned()).read().unwrap();
        assert!(serde_json::from_slice::<KeyWrap>(&record.protected_key).is_err());
        let restored = crate::secrets::unprotect(&record.protected_key).unwrap();
        assert!(serde_json::from_slice::<KeyWrap>(&restored).is_ok());
    }
}
