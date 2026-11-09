//! Explicit sync jobs. No schedules, cloud account, plaintext recovery secret or automatic vault access.
mod backend;
mod local;
#[cfg(test)]
mod tests;
mod vault_backup;
use anyhow::{Result, ensure};
use moodflow_sync::Backend;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;
static ACTIVE: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
pub fn cancel() {
    if let Some(stop) = ACTIVE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        stop.store(true, Ordering::Release);
    }
}
struct Job {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}
impl Job {
    fn begin() -> Result<Self> {
        let mut active = ACTIVE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        ensure!(active.is_none(), "sync.busy");
        let stop = Arc::new(AtomicBool::new(false));
        *active = Some(stop.clone());
        Ok(Self {
            stop,
            done: Arc::new(AtomicBool::new(false)),
        })
    }
    fn policy(&self, app: &AppHandle) -> Result<backend::Policy> {
        ensure!(crate::workspace::allowed(app), "sync.privacy");
        let permit = Arc::new(Mutex::new(Instant::now()));
        let stop = self.stop.clone();
        let done = self.done.clone();
        let probe = app.clone();
        let stamp = permit.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
                if !crate::workspace::allowed(&probe) {
                    stop.store(true, Ordering::Release);
                    break;
                }
                *stamp.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        let app = app.clone();
        let stop = self.stop.clone();
        let sleep = Mutex::new(moodflow_capsule::capture::SleepGuard::default());
        let start = Instant::now();
        Ok(Arc::new(move || {
            if stop.load(Ordering::Acquire) {
                return false;
            }
            let fresh =
                permit.lock().unwrap_or_else(|e| e.into_inner()).elapsed() < Duration::from_secs(1);
            let awake = crate::capture::awake(&mut sleep.lock().unwrap_or_else(|e| e.into_inner()));
            let valid = fresh
                && awake
                && crate::vault::allowed(&app)
                && start.elapsed() < Duration::from_secs(600);
            if !valid {
                stop.store(true, Ordering::Release);
            }
            valid
        }))
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        ACTIVE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Folder { path: String },
    Webdav { url: String, username: String },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    target: Target,
    encrypted: bool,
}
impl Profile {
    fn validate(&self) -> Result<()> {
        match &self.target {
            Target::Folder { path } => {
                ensure!(
                    path.len() <= 32768 && Path::new(path).is_absolute(),
                    "sync.invalid"
                );
            }
            Target::Webdav { url, username } => {
                backend::Dav::url(url)?;
                ensure!(username.len() <= 1024, "sync.limit");
            }
        }
        Ok(())
    }
    fn backend(
        &self,
        params: &Value,
        create: bool,
        policy: backend::Policy,
    ) -> Result<Box<dyn Backend>> {
        match &self.target {
            Target::Folder { path } => Ok(Box::new(backend::Folder::open(
                Path::new(path),
                create,
                policy,
            )?)),
            Target::Webdav { url, username } => Ok(Box::new(backend::Dav::open(
                url,
                username,
                params["password"].as_str().unwrap_or(""),
                create,
                policy,
            )?)),
        }
    }
    fn identity(&self) -> Result<String> {
        // Mode belongs to the authenticated header. It cannot erase a previous baseline.
        let target = match &self.target {
            Target::Folder { path } => {
                json!({"folder":Path::new(path).canonicalize()?.to_string_lossy().to_lowercase()})
            }
            Target::Webdav { url, username } => {
                json!({"webdav":backend::Dav::url(url)?.as_str(),"username":username})
            }
        };
        Ok(moodflow_sync::digest(&serde_json::to_vec(&target)?))
    }
}
fn root() -> Result<PathBuf> {
    let root = crate::paths::tool_data_dir()?.join("sync");
    crate::private_fs::ensure(&root)?;
    Ok(root)
}
fn manifest_copies(
    backend: &mut dyn Backend,
    sources: Vec<(String, Vec<u8>)>,
) -> Result<Vec<Vec<u8>>> {
    let moodflow_sync::Read::Present(header) = backend.get("config.mfsync")? else {
        anyhow::bail!("sync.deferred")
    };
    let mut copies = vec![];
    for (_, bytes) in sources {
        if moodflow_sync::crypto::Header::parse(&bytes).is_ok() {
            ensure!(bytes == header.bytes, "sync.dataset");
        } else {
            copies.push(bytes);
        }
    }
    Ok(copies)
}
fn profile(root: &Path) -> Result<Option<(Profile, String)>> {
    let path = root.join("profile.json");
    if !path.try_exists()? {
        return Ok(None);
    }
    let bytes = backend::read_bounded(&path, 65536)?;
    let profile: Profile = serde_json::from_slice(&bytes)?;
    profile.validate()?;
    Ok(Some((profile, moodflow_sync::digest(&bytes))))
}
pub fn execute(app: &AppHandle, method: &str, params: &Value) -> Result<Value> {
    if method == "cancel" {
        cancel();
        return Ok(json!({"cancelled":true}));
    }
    ensure!(crate::workspace::allowed(app), "sync.privacy");
    let root = root()?;
    if method == "status" {
        let profile = profile(&root)?;
        return Ok(
            json!({"profile":profile.as_ref().map(|p|&p.0),"revision":profile.map(|p|p.1),"busy":ACTIVE.get_or_init(Default::default).lock().unwrap_or_else(|e|e.into_inner()).is_some()}),
        );
    }
    if method == "save" {
        let profile: Profile = serde_json::from_value(params["profile"].clone())?;
        profile.validate()?;
        ensure!(
            profile.encrypted || params["confirm_plaintext"].as_bool() == Some(true),
            "sync.plaintext_confirm"
        );
        let _lock = backend::lock(&root.join(".profile.lock"))?;
        backend::atomic(
            &root.join("profile.json"),
            &serde_json::to_vec(&profile)?,
            params["revision"].as_str(),
        )?;
        return Ok(json!({"saved":true}));
    }
    if [
        "backup_import",
        "backup_list",
        "backup_export",
        "backup_delete",
    ]
    .contains(&method)
    {
        if method == "backup_import" {
            ensure!(
                profile(&root)?.is_some_and(|p| p.0.encrypted),
                "sync.vault_opt_in"
            );
        }
        return vault_backup::execute(&root, method, params, &|| crate::workspace::allowed(app));
    }
    let conflicts = root.join("conflicts");
    crate::private_fs::ensure(&conflicts)?;
    if method == "conflicts" {
        return local::conflicts(&conflicts);
    }
    if ["conflict", "restore_conflict", "delete_conflict"].contains(&method) {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?;
        ensure!(local::conflict_name(name), "sync.invalid");
        let path = conflicts.join(name);
        let bytes = backend::read_bounded(&path, moodflow_sync::MAX_DOCUMENT)?;
        let revision = moodflow_sync::digest(&bytes);
        let mut document = moodflow_capsule::Document::from_markdown(std::str::from_utf8(&bytes)?)?;
        if method == "conflict" {
            return Ok(json!({"document":document,"revision":revision}));
        }
        ensure!(
            params["revision"].as_str() == Some(&revision)
                && params["confirm"].as_bool() == Some(true),
            "sync.conflict"
        );
        if method == "delete_conflict" {
            std::fs::remove_file(path)?;
            return Ok(json!({"deleted":true}));
        }
        document.metadata.id = uuid::Uuid::new_v4().to_string();
        document.metadata.updated_at = chrono::Utc::now().to_rfc3339();
        document.revision.clear();
        let capsule_root = crate::paths::tool_data_dir()?.join("capsule");
        crate::private_fs::ensure(&capsule_root)?;
        let saved = moodflow_capsule::Library::new(capsule_root).put(&document, None)?;
        return Ok(json!({"restored_id":saved.metadata.id}));
    }
    ensure!(
        ["initialize", "run", "repair"].contains(&method),
        "sync.invalid"
    );
    ensure!(params["consent"].as_bool() == Some(true), "sync.consent");
    let profile = profile(&root)?
        .ok_or_else(|| anyhow::anyhow!("sync.unconfigured"))?
        .0;
    let include_vault = params["include_vault"].as_bool() == Some(true);
    ensure!(!include_vault || profile.encrypted, "sync.vault_opt_in");
    ensure!(
        profile.encrypted || params["confirm_plaintext"].as_bool() == Some(true),
        "sync.plaintext_confirm"
    );
    let phrase = if profile.encrypted {
        Some(
            params["recovery"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("sync.recovery"))?,
        )
    } else {
        None
    };
    let job = Job::begin()?;
    let policy = job.policy(app)?;
    ensure!(
        method == "run" || params["confirm"].as_bool() == Some(true),
        "sync.invalid"
    );
    let replica = root.join("replicas").join(profile.identity()?);
    crate::private_fs::ensure(&replica)?;
    let _lock = backend::lock(&replica.join(".replica.lock"))?;
    let mut backend = if method == "repair" {
        None
    } else {
        Some(profile.backend(params, method == "initialize", policy.clone())?)
    };
    if method == "initialize" {
        moodflow_sync::initialize(
            backend
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?
                .as_mut(),
            phrase,
        )?;
        return Ok(json!({"initialized":true}));
    }
    let state_path = replica.join("baseline.json");
    let previous = if state_path.try_exists()? {
        Some(backend::read_bounded(&state_path, moodflow_sync::MAX_BLOB)?)
    } else {
        None
    };
    let mut state: moodflow_sync::State = previous
        .as_ref()
        .map(|b| serde_json::from_slice(b))
        .transpose()?
        .unwrap_or_default();
    let capsule = local::Capsule::open(
        crate::paths::tool_data_dir()?.join("capsule"),
        conflicts,
        policy.clone(),
    )?;
    let mut local =
        vault_backup::Copies::new(capsule, root.clone(), include_vault, policy.clone())?;
    let result = if method == "repair" {
        let Target::Folder { path } = &profile.target else {
            anyhow::bail!("sync.invalid")
        };
        let mut folder = backend::Folder::open(Path::new(path), false, policy.clone())?;
        let sources = folder.repair_sources()?;
        let archive = sources
            .iter()
            .map(|(name, bytes)| (name.clone(), moodflow_sync::digest(bytes)))
            .collect::<Vec<_>>();
        let copies = manifest_copies(&mut folder, sources)?;
        moodflow_sync::recover_manifests(
            &mut folder,
            &mut local,
            &mut state,
            phrase,
            &copies,
            &*policy,
        )
        .and_then(|report| {
            folder.archive_sources(&archive)?;
            Ok(report)
        })
    } else {
        moodflow_sync::synchronize(
            backend
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?
                .as_mut(),
            &mut local,
            &mut state,
            phrase,
            &*policy,
        )
    };
    // This checkpoint contains no body or credential and must survive a late privacy cancellation.
    backend::atomic(
        &state_path,
        &serde_json::to_vec(&state)?,
        previous
            .as_ref()
            .map(|b| moodflow_sync::digest(b))
            .as_deref(),
    )?;
    Ok(json!({"report":result?,"skipped":local.skipped()}))
}
