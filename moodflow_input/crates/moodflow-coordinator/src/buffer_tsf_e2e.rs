//! Explicit isolated acceptance: real registered DLL + Rich Edit + bridge + table engine.
//! Never bind the production pipe or create desktop windows on a developer's machine.
use crate::Coordinator;
use moodflow_bridge::server::{BridgeConfig, BridgeServer};
use moodflow_ui_types::MenuCmd;
struct FixtureRpcCore;
impl moodflow_rpc::CoreRpc for FixtureRpcCore {
    fn is_chinese_mode(&self) -> bool {
        true
    }
    fn active_schema_id(&self) -> String {
        "wubi86".into()
    }
}
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(child: &mut OwnedChild, report: &Path, marker: &str) {
    let start = Instant::now();
    loop {
        if let Ok(error) = std::fs::read_to_string(report.join("failure")) {
            panic!("TSF host failed: {error}");
        }
        if report.join(marker).is_file() {
            return;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "TSF host exited before {marker}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(50),
            "timed out waiting for {marker}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn until(predicate: impl Fn() -> bool, stage: &str) {
    let start = Instant::now();
    while !predicate() {
        assert!(start.elapsed() < Duration::from_secs(10), "{stage}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "registers a real TSF only on a disposable Windows CI runner"]
fn actual_tsf_buffer_engine_desktop_appcontainer_and_lpac() {
    assert_eq!(
        std::env::var("GITHUB_ACTIONS").as_deref(),
        Ok("true"),
        "isolated Windows CI only"
    );
    let host = PathBuf::from(
        std::env::var_os("MOODFLOW_TSF_E2E_HOST").expect("explicit native fixture host"),
    );
    let data = PathBuf::from(
        std::env::var_os("MOODFLOW_TSF_E2E_DATA").expect("explicit built data directory"),
    );
    let root = PathBuf::from(
        std::env::var_os("MOODFLOW_TSF_E2E_REPORTS").expect("explicit fixture reports"),
    );
    let user = PathBuf::from(
        std::env::var_os("MOODFLOW_TSF_E2E_USER").expect("separate broker-owned fixture data"),
    );
    assert!(host.is_file());
    assert!(data.join("schemas/wubi86.schema.toml").is_file());
    let mut config = moodflow_config::Config::default();
    config.schema.active = "wubi86".into();
    config.schema.available = vec!["wubi86".into()];
    config.input.default.chinese_mode = true;
    config.moodflow.enabled = false;
    let (coordinator, _ui) = Coordinator::new_headless_with_ui_at(config, Some(&data), Some(&user));
    let suffix = format!("_e2e_{}", std::process::id());
    moodflow_rpc::RpcServer::new(std::sync::Arc::new(FixtureRpcCore), "release", &suffix)
        .unwrap()
        .start()
        .unwrap();
    coordinator.push_server.start().unwrap();
    BridgeServer::new(BridgeConfig::default(), coordinator.clone())
        .start()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    for identity in ["ordinary", "appcontainer", "lpac"] {
        let report = root.join(identity);
        assert!(report.is_dir());
        let mut command = Command::new(&host);
        command.env_clear();
        for name in [
            "SystemRoot",
            "WINDIR",
            "PATH",
            "USERPROFILE",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("GITHUB_ACTIONS", "true")
            .env(
                "MOODFLOW_TSF_E2E_RPC_CTRL",
                moodflow_rpc::ctrl_endpoint(&suffix),
            )
            .env(
                "MOODFLOW_TSF_E2E_RPC_EVENTS",
                moodflow_rpc::events_endpoint(&suffix),
            );
        let mut child = OwnedChild(command.arg(identity).arg(&report).spawn().unwrap());
        wait(&mut child, &report, "ready");
        coordinator.stage_buffer_text(" buffered").unwrap();
        coordinator.toggle_buffer();
        assert!(
            coordinator.buffer.is_capturing(),
            "explicitly bind actual TSF target"
        );
        coordinator.run_buffer_menu_cmd(MenuCmd::BufferSend);
        wait(&mut child, &report, "delivered");
        until(
            || coordinator.read_buffer_text().is_ok_and(|v| v.is_empty()),
            "real edit receipt did not accept delivery",
        );
        wait(&mut child, &report, "switched");
        until(
            || !coordinator.buffer.is_capturing(),
            "same-process field change did not revoke binding",
        );
        coordinator
            .stage_buffer_text("must stay in Buffer")
            .unwrap();
        coordinator.run_buffer_menu_cmd(MenuCmd::BufferSend);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            coordinator.read_buffer_text().unwrap(),
            "must stay in Buffer"
        );
        coordinator.run_buffer_menu_cmd(MenuCmd::BufferClear);
        std::fs::write(report.join("finish"), "ok").unwrap();
        wait(&mut child, &report, "passed");
        assert!(
            child.0.wait().unwrap().success(),
            "native fixture process failed"
        );
        println!("PASS actual TSF engine, receipt and field binding: {identity}");
    }
}
