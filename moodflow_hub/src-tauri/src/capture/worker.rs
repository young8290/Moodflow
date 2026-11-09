//! Supervised worker: fixed executable, bounded JSON pipes, hard privacy cancellation and OS memory limits.
use anyhow::{Result, ensure};
use moodflow_capsule::capture::Request;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;

#[derive(Default)]
pub struct Cancel {
    pub stop: AtomicBool,
    pub abort: AtomicBool,
}
struct ProbeStop(Arc<AtomicBool>);
impl Drop for ProbeStop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub fn executable() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()?
        .parent()?
        .join("moodflow-capture.exe");
    if beside.is_file() {
        return Some(beside);
    }
    let local =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/moodflow-capture.exe");
    (cfg!(debug_assertions) && local.is_file()).then_some(local)
}
fn core_allowed(request: &Request) -> bool {
    let apps = match super::source::names(request) {
        Ok(apps) => apps,
        Err(_) => return false,
    };
    moodflow_hub_core::infra::imeconf::ImeRpc::from_env(cfg!(debug_assertions))
        .call("tools.captureAllowed", json!({"apps":apps}))
        .ok()
        .is_some_and(|v| v["allowed"].as_bool() == Some(true))
}
pub fn allowed(app: &AppHandle) -> bool {
    !crate::sim::demo() && crate::vault::allowed(app)
}
pub fn reviewed(app: &AppHandle) -> bool {
    allowed(app)
        && moodflow_hub_core::infra::imeconf::ImeRpc::from_env(cfg!(debug_assertions))
            .call("tools.allowed", json!({}))
            .ok()
            .is_some_and(|v| v["allowed"] == true)
}
pub fn run(app: &AppHandle, request: &Request, cancel: Arc<Cancel>) -> Result<Value> {
    let mut sleep = moodflow_capsule::capture::SleepGuard::default();
    ensure!(super::source::awake(&mut sleep), "capture.privacy");
    ensure!(allowed(app) && core_allowed(request), "capture.privacy");
    let path = executable().ok_or_else(|| anyhow::anyhow!("capture.missing_helper"))?;
    let mut command = Command::new(path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let _job = job::Job::assign(&child)?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("capture.failed"))?;
    let mut encoded = serde_json::to_vec(request)?;
    ensure!(encoded.len() <= 65535, "capture.invalid_request");
    encoded.push(b'\n');
    input.write_all(&encoded)?;
    input.flush()?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("capture.failed"))?;
    let oversized = Arc::new(AtomicBool::new(false));
    let reader_oversized = oversized.clone();
    let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        output.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        reader_oversized.store(bytes.len() > 8 * 1024 * 1024, Ordering::Release);
        Ok(bytes)
    });
    // The control loop never blocks on core IPC: a permit expires after one second.
    let permit = Arc::new(Mutex::new((true, Instant::now())));
    let done = Arc::new(AtomicBool::new(false));
    let _probe_stop = ProbeStop(done.clone());
    let probe_done = done.clone();
    let probe_permit = permit.clone();
    let probe_request = request.clone();
    std::thread::spawn(move || {
        while !probe_done.load(Ordering::Acquire) {
            let allowed = core_allowed(&probe_request);
            let mut current = probe_permit.lock().unwrap_or_else(|e| e.into_inner());
            current.0 &= allowed;
            current.1 = Instant::now();
            drop(current);
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    let started = Instant::now();
    let maximum = Duration::from_secs(if request.operation == "record" {
        u64::from(request.seconds) + 40
    } else if request.operation == "scroll" {
        300
    } else {
        40
    });
    let mut stopped = false;
    let mut failed = false;
    let status = loop {
        let (permitted, checked) = *permit.lock().unwrap_or_else(|e| e.into_inner());
        if !super::source::awake(&mut sleep)
            || cancel.abort.load(Ordering::Acquire)
            || !allowed(app)
            || !permitted
            || checked.elapsed() > Duration::from_secs(1)
            || oversized.load(Ordering::Acquire)
            || started.elapsed() > maximum
        {
            failed = true;
            let _ = child.kill();
            break child.wait()?;
        }
        if cancel.stop.load(Ordering::Acquire) && !stopped {
            let _ = input.write_all(b"stop\n");
            let _ = input.flush();
            stopped = true;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    done.store(true, Ordering::Release);
    drop(input);
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("capture.failed"))??;
    ensure!(
        !failed && allowed(app) && status.success(),
        "capture.cancelled"
    );
    ensure!(bytes.len() <= 8 * 1024 * 1024, "capture.reply_limit");
    let reply: Value = serde_json::from_slice(&bytes)?;
    if reply["ok"] != true {
        let code = match reply["error"].as_str() {
            Some("capture.privacy") => "capture.privacy",
            Some("capture.cancelled") => "capture.cancelled",
            Some("capture.ocr_missing") => "capture.ocr_missing",
            Some("capture.no_overlap") => "capture.no_overlap",
            Some("capture.pixel_limit") => "capture.pixel_limit",
            Some("capture.audio_unavailable") => "capture.audio_unavailable",
            _ => "capture.failed",
        };
        anyhow::bail!(code);
    }
    Ok(reply["result"].clone())
}

#[cfg(windows)]
mod job {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
    };
    pub struct Job(HANDLE);
    impl Job {
        pub fn assign(child: &std::process::Child) -> Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            ensure!(!handle.is_null(), "capture.isolation_failed");
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            limits.ProcessMemoryLimit = 1024 * 1024 * 1024;
            ensure!(
                unsafe {
                    SetInformationJobObject(
                        handle,
                        JobObjectExtendedLimitInformation,
                        (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                        std::mem::size_of_val(&limits) as u32,
                    )
                } != 0,
                "capture.isolation_failed"
            );
            ensure!(
                unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } != 0,
                "capture.isolation_failed"
            );
            Ok(job)
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
#[cfg(not(windows))]
mod job {
    use super::*;
    pub struct Job;
    impl Job {
        pub fn assign(_child: &std::process::Child) -> Result<Self> {
            anyhow::bail!("capture.unsupported")
        }
    }
}
