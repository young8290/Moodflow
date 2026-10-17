//! 专注计时的外壳一侧（FR-RST-10）：写库、推送 `focus:tick` / `focus:done`，把开始结束转给感知任务
//! （专注中小组件不切换天气）。计时与计数在 `moodflow_hub_core::focus`。

use std::sync::Arc;

use moodflow_hub_core::bus::HubEvent;
use moodflow_hub_core::domain::focus::FocusSummary;
use moodflow_hub_core::focus::{FocusHandle, FocusPort, FocusService};
use moodflow_hub_core::sense::SenseCmd;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::events::{FocusDone, FocusTick};
use crate::sensing::Sensing;
use crate::state::AppState;

/// 须在 `AppState`、`Sensing` 都托管之后调用。托管 [`FocusHandle`] 供命令使用。
pub fn start(app: &AppHandle) {
    let sensing = app.state::<Sensing>();
    let handle = FocusHandle::new(
        Arc::new(ShellPort { app: app.clone() }),
        crate::sim::clock(),
        sensing.bus.clone(),
    );
    tauri::async_runtime::spawn(FocusService::run(handle.clone(), sensing.bus.subscribe()));
    // 开始 / 结束转给感知任务：它不在总线上，只收命令
    let mut bus = sensing.bus.subscribe();
    let cmds = sensing.cmds.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match bus.recv().await {
                Ok(HubEvent::Focus(on)) => {
                    if cmds.send(SenseCmd::Focus(on)).await.is_err() {
                        return;
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    app.manage(handle);
}

struct ShellPort {
    app: AppHandle,
}

impl FocusPort for ShellPort {
    fn save(&self, s: &FocusSummary) {
        let s = *s;
        self.app
            .state::<AppState>()
            .writer()
            .enqueue("focus_session", move |db| db.record_focus(&s));
    }

    fn tick(&self, remaining_s: u32, planned_min: u32) {
        let ev = FocusTick {
            remaining_s,
            planned_min,
        };
        if let Err(e) = ev.emit(&self.app) {
            crate::hlog!("推送 focus:tick 失败：{e}");
        }
    }

    fn done(&self, s: &FocusSummary) {
        if let Err(e) = (FocusDone { summary: *s }).emit(&self.app) {
            crate::hlog!("推送 focus:done 失败：{e}");
        }
    }

    fn note(&self, msg: &str) {
        crate::hlog!("{msg}");
    }
}
