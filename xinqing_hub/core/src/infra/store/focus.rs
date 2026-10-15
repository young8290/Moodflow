//! 专注记录（09 D-29，FR-RST-10）：每次专注一行 `focus_session`，并把实际分钟数累计到开始那天的
//! `daily_summary.focus_min`。只有时长与计数，没有文字。

use chrono::{Local, TimeZone};
use rusqlite::params;

use super::{Db, StoreError};
use crate::domain::focus::FocusSummary;

impl Db {
    /// 记一次专注。不足 1 分钟的（开始就结束）不记。
    pub fn record_focus(&self, s: &FocusSummary) -> Result<(), StoreError> {
        if s.actual_min == 0 {
            return Ok(());
        }
        let date = Local
            .timestamp_millis_opt(s.start_ms)
            .single()
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO focus_session (start_ts, planned_min, actual_min, keys, chars) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![s.start_ms, s.planned_min, s.actual_min, s.keys, s.chars],
        )?;
        tx.execute(
            "INSERT INTO daily_summary (date, focus_min) VALUES (?1, ?2)
             ON CONFLICT(date) DO UPDATE SET focus_min = focus_min + excluded.focus_min",
            params![date, s.actual_min],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 一段时间内（Unix 毫秒，左闭右开）开始的专注总分钟数，周报用（07 第 4 节）。
    pub fn focus_minutes(&self, from_ms: i64, to_ms: i64) -> Result<u32, StoreError> {
        let n: i64 = self.conn.query_row(
            "SELECT coalesce(sum(actual_min), 0) FROM focus_session WHERE start_ts >= ?1 AND start_ts < ?2",
            params![from_ms, to_ms],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(start_ms: i64, actual_min: u32) -> FocusSummary {
        FocusSummary {
            start_ms,
            planned_min: 25,
            actual_min,
            keys: 1832,
            chars: 960,
            completed: actual_min == 25,
        }
    }

    #[test]
    fn sessions_add_up_per_day_and_empty_ones_are_skipped() {
        let db = Db::open_in_memory().unwrap();
        let day = Local
            .with_ymd_and_hms(2026, 10, 15, 9, 0, 0)
            .unwrap()
            .timestamp_millis();
        db.record_focus(&summary(day, 25)).unwrap();
        db.record_focus(&summary(day + 3_600_000, 12)).unwrap();
        db.record_focus(&summary(day + 7_200_000, 0)).unwrap();
        let (rows, min): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT (SELECT count(*) FROM focus_session), focus_min FROM daily_summary WHERE date = '2026-10-15'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((rows, min), (2, 37));
        assert_eq!(db.focus_minutes(day, day + 86_400_000).unwrap(), 37);
        assert_eq!(db.focus_minutes(0, day).unwrap(), 0);
    }
}
