//! 专注计时服务（06 FR-RST-10，P2）：开始 / 结束、每秒倒计时、按 XQP 事件计数，到点自动结束。
//!
//! 开始、结束都在总线上发 `HubEvent::Focus`，关怀、休息提醒、感知各自据此暂停或冻结显示。
//! 命令由外壳经 [`FocusHandle`] 同步调用；倒计时与计数在 [`FocusService::run`] 里。
//! 与外壳之间只通过 [`FocusPort`]，本模块不依赖 Tauri（ADR 0007）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;
use xqp::{KeySrc, Up};

use crate::bus::HubEvent;
use crate::domain::focus::{Focus, FocusSummary};
use crate::infra::clock::Clock;

/// 倒计时推送间隔。
pub const TICK: Duration = Duration::from_secs(1);

/// 外壳提供给专注服务的能力。
pub trait FocusPort: Send + Sync {
    /// 写 `focus_session` 与 `daily_summary.focus_min`。
    fn save(&self, s: &FocusSummary);
    /// 推送 `focus:tick`。
    fn tick(&self, remaining_s: u32, planned_min: u32);
    /// 推送 `focus:done`（结束卡片）。
    fn done(&self, s: &FocusSummary);
    fn note(&self, _msg: &str) {}
}

/// `focus_status` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FocusStatus {
    pub active: bool,
    pub planned_min: u32,
    pub remaining_s: u32,
}

/// 开始失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FocusError {
    #[error("时长要在 10～120 分钟之间")]
    InvalidMinutes,
    #[error("已经在专注了")]
    AlreadyRunning,
}

struct Inner {
    current: Mutex<Option<Focus>>,
    port: Arc<dyn FocusPort>,
    clock: Arc<dyn Clock>,
    bus: broadcast::Sender<HubEvent>,
}

/// 外壳持有的句柄；可克隆。
#[derive(Clone)]
pub struct FocusHandle(Arc<Inner>);

impl FocusHandle {
    pub fn new(
        port: Arc<dyn FocusPort>,
        clock: Arc<dyn Clock>,
        bus: broadcast::Sender<HubEvent>,
    ) -> Self {
        Self(Arc::new(Inner {
            current: Mutex::new(None),
            port,
            clock,
            bus,
        }))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Focus>> {
        self.0.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> FocusStatus {
        let now = self.0.clock.now_ms();
        match *self.lock() {
            Some(f) => FocusStatus {
                active: true,
                planned_min: f.planned_min,
                remaining_s: f.remaining_s(now),
            },
            None => FocusStatus {
                active: false,
                planned_min: 0,
                remaining_s: 0,
            },
        }
    }

    pub fn start(&self, minutes: u32) -> Result<FocusStatus, FocusError> {
        {
            let mut cur = self.lock();
            if cur.is_some() {
                return Err(FocusError::AlreadyRunning);
            }
            *cur = Some(
                Focus::start(self.0.clock.now_ms(), minutes).ok_or(FocusError::InvalidMinutes)?,
            );
        }
        let _ = self.0.bus.send(HubEvent::Focus(true));
        let s = self.status();
        self.0.port.tick(s.remaining_s, s.planned_min);
        Ok(s)
    }

    /// 提前结束（或到点结束）。没在专注时返回 `None`。
    pub fn stop(&self) -> Option<FocusSummary> {
        let f = self.lock().take()?;
        let summary = f.finish(self.0.clock.now_ms());
        self.0.port.save(&summary);
        let _ = self.0.bus.send(HubEvent::Focus(false));
        self.0.port.done(&summary);
        Some(summary)
    }

    fn on_up(&self, up: &Up) {
        let mut cur = self.lock();
        let Some(f) = cur.as_mut() else { return };
        match up {
            // 只数核心来源的按键，免得 TSF 追踪时同一个键数两次
            Up::Key {
                src: KeySrc::Core, ..
            } => f.on_key(),
            Up::Commit { chars, .. } => f.on_commit(*chars),
            _ => {}
        }
    }

    /// 每秒：推送倒计时，到点自动结束。
    pub fn on_tick(&self) {
        let now = self.0.clock.now_ms();
        let Some(f) = *self.lock() else { return };
        let left = f.remaining_s(now);
        if left == 0 {
            self.stop();
        } else {
            self.0.port.tick(left, f.planned_min);
        }
    }
}

pub struct FocusService;

impl FocusService {
    /// 运行到总线关闭为止。
    pub async fn run(handle: FocusHandle, mut bus: broadcast::Receiver<HubEvent>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                ev = bus.recv() => match ev {
                    Ok(HubEvent::Xqp(up)) => handle.on_up(&up),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        handle.0.port.note(&format!("专注计时跳过了 {n} 条总线事件"));
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                _ = tick.tick() => handle.on_tick(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};
    use xqp::KeyKind;

    use super::*;
    use crate::infra::clock::ManualClock;

    #[derive(Default)]
    struct Port {
        saved: Mutex<Vec<FocusSummary>>,
        ticks: Mutex<Vec<u32>>,
        done: Mutex<Vec<FocusSummary>>,
    }

    impl FocusPort for Port {
        fn save(&self, s: &FocusSummary) {
            self.saved.lock().unwrap().push(*s);
        }
        fn tick(&self, remaining_s: u32, _planned_min: u32) {
            self.ticks.lock().unwrap().push(remaining_s);
        }
        fn done(&self, s: &FocusSummary) {
            self.done.lock().unwrap().push(*s);
        }
    }

    fn rig() -> (
        FocusHandle,
        Arc<Port>,
        Arc<ManualClock>,
        broadcast::Receiver<HubEvent>,
    ) {
        let port = Arc::new(Port::default());
        let clock = Arc::new(ManualClock::new(
            Local.with_ymd_and_hms(2026, 10, 15, 9, 0, 0).unwrap(),
        ));
        let (bus, rx) = crate::bus::channel();
        (FocusHandle::new(port.clone(), clock.clone(), bus), port, clock, rx)
    }

    fn key(src: KeySrc) -> Up {
        Up::Key {
            ts: 0,
            seq: None,
            kind: KeyKind::Letter,
            vk: Some(b'A'),
            in_comp: true,
            src,
            eaten: None,
            del_committed: None,
        }
    }

    fn commit(chars: u32) -> Up {
        Up::Commit {
            ts: 0,
            seq: None,
            chars,
            keystrokes: chars * 3,
            cand_pos: 1,
            src: "core".into(),
            text: None,
            truncated: None,
        }
    }

    #[test]
    fn runs_to_the_end_and_counts_only_core_keys() {
        let (h, port, clock, mut rx) = rig();
        assert_eq!(h.start(5), Err(FocusError::InvalidMinutes));
        let s = h.start(25).unwrap();
        assert_eq!((s.active, s.remaining_s), (true, 1500));
        assert_eq!(h.start(25), Err(FocusError::AlreadyRunning));
        assert!(matches!(rx.try_recv(), Ok(HubEvent::Focus(true))));

        h.on_up(&key(KeySrc::Core));
        h.on_up(&key(KeySrc::Core));
        h.on_up(&key(KeySrc::Tsf));
        h.on_up(&commit(4));
        clock.advance_ms(60_000);
        h.on_tick();
        assert_eq!(port.ticks.lock().unwrap().last(), Some(&1440));

        clock.advance_ms(24 * 60_000);
        h.on_tick();
        let done = port.done.lock().unwrap()[0];
        assert_eq!(
            (done.actual_min, done.keys, done.chars, done.completed),
            (25, 2, 4, true)
        );
        assert_eq!(port.saved.lock().unwrap().len(), 1);
        assert!(matches!(rx.try_recv(), Ok(HubEvent::Focus(false))));
        assert!(!h.status().active);
        assert_eq!(h.stop(), None);
    }

    #[test]
    fn stopping_early_is_recorded_without_judgement() {
        let (h, port, clock, _rx) = rig();
        h.start(45).unwrap();
        clock.advance_ms(12 * 60_000);
        let s = h.stop().unwrap();
        assert_eq!((s.actual_min, s.completed), (12, false));
        assert_eq!(port.saved.lock().unwrap()[0], s);
        // 不在专注时计数不算
        h.on_up(&commit(9));
        assert_eq!(h.status(), FocusStatus { active: false, planned_min: 0, remaining_s: 0 });
    }
}
