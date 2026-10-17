//! 专注计时（06 FR-RST-10，P2）：一次专注的开始、计数、结束与小结。
//!
//! 期间主动关怀暂停、休息提醒只留深夜提醒、小组件不切换天气（由各服务收到 `HubEvent::Focus` 后自己处理）；
//! 这里只管时长和计数。计数只有按键次数与上屏字数（MFP `key` / `commit.chars`），不碰任何文字。
//! 专注**不计入**晴天值（C-ETH-05）。

use serde::{Deserialize, Serialize};

/// 可选时长（分钟），闭区间（FR-RST-10“可自定义 10–120 分钟”）。
pub const MIN_MINUTES: u32 = 10;
pub const MAX_MINUTES: u32 = 120;
/// 界面上的三个预设。
pub const PRESETS: [u32; 3] = [25, 45, 60];
/// 结束卡片建议休息的分钟数。
pub const REST_MINUTES: u32 = 5;

/// 一次进行中的专注。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    pub start_ms: i64,
    pub planned_min: u32,
    pub keys: u32,
    pub chars: u32,
}

/// 一次专注的结果（结束卡片与 `focus_session` 一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FocusSummary {
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_ms: i64,
    pub planned_min: u32,
    /// 实际专注的整分钟数（提前结束时少于计划）
    pub actual_min: u32,
    pub keys: u32,
    pub chars: u32,
    /// 是否到点结束（否则是用户提前结束，界面不做任何评价）
    pub completed: bool,
}

/// 时长不在 10–120 分钟内时返回 `None`。
pub fn valid_minutes(m: u32) -> Option<u32> {
    (MIN_MINUTES..=MAX_MINUTES).contains(&m).then_some(m)
}

impl Focus {
    pub fn start(now_ms: i64, minutes: u32) -> Option<Self> {
        Some(Self {
            start_ms: now_ms,
            planned_min: valid_minutes(minutes)?,
            keys: 0,
            chars: 0,
        })
    }

    pub fn end_ms(&self) -> i64 {
        self.start_ms + i64::from(self.planned_min) * 60_000
    }

    /// 还剩几秒（向上取整，到点为 0）。
    pub fn remaining_s(&self, now_ms: i64) -> u32 {
        let left = (self.end_ms() - now_ms).max(0);
        u32::try_from((left + 999) / 1000).unwrap_or(u32::MAX)
    }

    pub fn on_key(&mut self) {
        self.keys = self.keys.saturating_add(1);
    }

    pub fn on_commit(&mut self, chars: u32) {
        self.chars = self.chars.saturating_add(chars);
    }

    /// 结束（到点或提前）。实际时长按整分钟向下取，不超过计划。
    pub fn finish(&self, now_ms: i64) -> FocusSummary {
        let completed = now_ms >= self.end_ms();
        let elapsed_min = ((now_ms - self.start_ms).max(0) / 60_000) as u32;
        FocusSummary {
            start_ms: self.start_ms,
            planned_min: self.planned_min,
            actual_min: if completed {
                self.planned_min
            } else {
                elapsed_min.min(self.planned_min)
            },
            keys: self.keys,
            chars: self.chars,
            completed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_must_be_between_10_and_120() {
        assert_eq!(valid_minutes(9), None);
        assert_eq!(valid_minutes(10), Some(10));
        assert_eq!(valid_minutes(120), Some(120));
        assert_eq!(valid_minutes(121), None);
        assert!(PRESETS.iter().all(|&p| valid_minutes(p).is_some()));
    }

    #[test]
    fn counts_and_summary() {
        let mut f = Focus::start(0, 25).unwrap();
        for _ in 0..3 {
            f.on_key();
        }
        f.on_commit(2);
        f.on_commit(5);
        assert_eq!(f.remaining_s(0), 25 * 60);
        assert_eq!(f.remaining_s(1), 25 * 60, "向上取整");
        assert_eq!(f.remaining_s(30 * 60_000), 0);

        let early = f.finish(10 * 60_000 + 59_000);
        assert_eq!((early.actual_min, early.completed), (10, false));
        assert_eq!((early.keys, early.chars), (3, 7));

        let done = f.finish(25 * 60_000);
        assert_eq!((done.actual_min, done.completed), (25, true));
    }
}
