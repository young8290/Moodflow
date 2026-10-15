//! 晴天收集（05 FR-REV-04，P2）：只奖励健康行为的晴天值，每 20 点解锁一件小精灵装扮（FR-WGT-08）。
//!
//! 晴天值不单独记账，每次从已有的记录算出来（ADR 0040），所以不会重复加、也不会漏加：
//!
//! | 行为 | 晴天值 | 来源 |
//! |---|---|---|
//! | 完成一次休息提醒（点“已完成”，喝水除外） | +1 | `daily_summary.rests_done − water` |
//! | 喝水（每天最多计 4 次） | +1 | `min(daily_summary.water, 4)` |
//! | 23:30 前停止打字（次日结算） | +2 | 当天有打字、停止打字时间早于当晚 23:30，且那天已经过去 |
//! | 完成一个待办 | +1 | `todo.done_ts` |
//!
//! 不按打字量、使用时长发放，专注不计入（C-ETH-04/05）；没有“连续”的概念，也就没有断签。

use chrono::{Local, NaiveDate, TimeZone, Timelike};
use serde::Serialize;

/// 8 件装扮，按解锁顺序（FR-REV-04）。`id` 是界面文案键 `collection.item_<id>` 和小精灵图层名。
pub const ITEMS: [&str; 8] = [
    "straw_hat",
    "scarf",
    "headphones",
    "flower",
    "glasses",
    "beanie",
    "bow",
    "star_clip",
];
pub const POINTS_PER_ITEM: u32 = 20;
pub const WATER_CAP: u32 = 4;
pub const EARLY_NIGHT_POINTS: u32 = 2;
/// 23:30，当天的第几分钟
pub const EARLY_NIGHT_BEFORE_MIN: u32 = 23 * 60 + 30;

/// 一天的原始数（`daily_summary` 一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayFacts {
    pub date: NaiveDate,
    pub typing_min: u32,
    pub rests_done: u32,
    pub water: u32,
    /// 当晚停止打字时间（18:00–次日 06:00 的最后活跃分钟），Unix 毫秒；18:00 后没打字为 `None`
    pub last_active_ts: Option<i64>,
}

/// 这一天得的晴天值（不含待办）。`today` 当天还没过完，“早停打字”次日才结算。
pub fn day_points(d: &DayFacts, today: NaiveDate) -> u32 {
    let water = d.water.min(d.rests_done);
    let mut p = (d.rests_done - water) + water.min(WATER_CAP);
    if d.date < today && d.typing_min > 0 && stopped_early(d) {
        p += EARLY_NIGHT_POINTS;
    }
    p
}

/// 停止打字时间早于当晚 23:30（18:00 后没打字也算）。
fn stopped_early(d: &DayFacts) -> bool {
    let Some(ts) = d.last_active_ts else {
        return true;
    };
    let Some(t) = Local.timestamp_millis_opt(ts).single() else {
        return false;
    };
    t.date_naive() == d.date && t.hour() * 60 + t.minute() < EARLY_NIGHT_BEFORE_MIN
}

/// 已解锁几件。
pub fn unlocked_count(points: u32) -> usize {
    ((points / POINTS_PER_ITEM) as usize).min(ITEMS.len())
}

pub fn is_item(id: &str) -> bool {
    ITEMS.contains(&id)
}

/// 下一件还差几点；全部解锁后为 `None`。
pub fn next_item(points: u32) -> Option<(&'static str, u32)> {
    let n = unlocked_count(points);
    ITEMS
        .get(n)
        .map(|id| (*id, (n as u32 + 1) * POINTS_PER_ITEM - points))
}

/// 一件装扮在界面上的状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ItemView {
    pub id: String,
    pub unlocked: bool,
    pub equipped: bool,
}

/// `collection_get` 的结果，也是 `collection:changed` 的载荷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CollectionView {
    pub enabled: bool,
    pub points: u32,
    /// 下一件的 id 与还差的点数
    pub next_item: Option<String>,
    pub next_need: u32,
    pub items: Vec<ItemView>,
    /// 正在穿的那件
    pub equipped: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: NaiveDate, rests_done: u32, water: u32, stop: Option<(u32, u32, bool)>) -> DayFacts {
        DayFacts {
            date,
            typing_min: 60,
            rests_done,
            water,
            last_active_ts: stop.map(|(h, m, next_day)| {
                let d = if next_day { date.succ_opt().unwrap() } else { date };
                Local
                    .from_local_datetime(&d.and_hms_opt(h, m, 0).unwrap())
                    .unwrap()
                    .timestamp_millis()
            }),
        }
    }

    #[test]
    fn only_healthy_actions_count_and_water_is_capped() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        // 3 次护眼 / 活动 + 6 次喝水（只计 4）+ 23:00 停止打字（+2）
        assert_eq!(day_points(&day(d, 9, 6, Some((23, 0, false))), today), 3 + 4 + 2);
        // 23:30 及以后、过了午夜都不算早停
        assert_eq!(day_points(&day(d, 0, 0, Some((23, 30, false))), today), 0);
        assert_eq!(day_points(&day(d, 0, 0, Some((0, 40, true))), today), 0);
        // 18:00 后没打字算早停；当天还没过完不结算
        assert_eq!(day_points(&day(d, 0, 0, None), today), 2);
        assert_eq!(day_points(&day(today, 0, 0, None), today), 0);
        // 没打字的日子不发早停奖励
        let mut idle = day(d, 0, 0, None);
        idle.typing_min = 0;
        assert_eq!(day_points(&idle, today), 0);
    }

    #[test]
    fn every_twenty_points_unlock_the_next_item() {
        assert_eq!(unlocked_count(19), 0);
        assert_eq!(unlocked_count(20), 1);
        assert_eq!(unlocked_count(1000), 8);
        assert_eq!(next_item(0), Some(("straw_hat", 20)));
        assert_eq!(next_item(33), Some(("scarf", 7)));
        assert_eq!(next_item(160), None);
        assert!(is_item("star_clip") && !is_item("crown"));
    }
}
