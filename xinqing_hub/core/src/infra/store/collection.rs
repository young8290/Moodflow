//! 晴天收集的读写（09 D-30，FR-REV-04）。晴天值从 `daily_summary` 与 `todo` 算出来（ADR 0040）；
//! `collection` 表只记每件装扮第一次解锁的时间和正在穿哪件。关闭“晴天收集”时清空这张表。

use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::params;

use super::{Db, StoreError};
use crate::domain::collection::{self, CollectionView, DayFacts, ItemView};

impl Db {
    /// `[from, to)` 这些天得的晴天值。`today` 当天的“早停打字”还没结算。
    pub fn sunny_points(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        today: NaiveDate,
    ) -> Result<u32, StoreError> {
        // 库里的日期是 `YYYY-MM-DD` 文本；NaiveDate::MIN / MAX 的写法带符号，按文本比较会出错，先夹到四位年份
        let lo = NaiveDate::from_ymd_opt(1, 1, 1).unwrap_or(NaiveDate::MIN);
        let hi = NaiveDate::from_ymd_opt(9999, 12, 31).unwrap_or(NaiveDate::MAX);
        let (from, to) = (from.clamp(lo, hi), to.clamp(lo, hi));
        let mut stmt = self.conn.prepare(
            "SELECT date, typing_min, rests_done, water, last_active_ts FROM daily_summary
             WHERE date >= ?1 AND date < ?2",
        )?;
        let fmt = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
        let days = stmt.query_map(params![fmt(from), fmt(to)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?;
        let mut total = 0u32;
        for d in days {
            let (date, typing_min, rests_done, water, last_active_ts) = d?;
            let Ok(date) = date.parse::<NaiveDate>() else {
                continue;
            };
            total += collection::day_points(
                &DayFacts {
                    date,
                    typing_min,
                    rests_done,
                    water,
                    last_active_ts,
                },
                today,
            );
        }
        let ms = |d: NaiveDate| {
            Local
                .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
                .earliest()
                .map_or(0, |t| t.timestamp_millis())
        };
        let todos: u32 = self.conn.query_row(
            "SELECT count(*) FROM todo WHERE status = 'done' AND done_ts >= ?1 AND done_ts < ?2",
            params![ms(from), ms(to)],
            |r| r.get(0),
        )?;
        Ok(total + todos)
    }

    /// 当前的晴天值与装扮；新达到的解锁记下时间。关闭时只返回 `enabled = false`。
    pub fn collection_view(
        &self,
        enabled: bool,
        today: NaiveDate,
        now_ms: i64,
    ) -> Result<CollectionView, StoreError> {
        if !enabled {
            return Ok(CollectionView {
                enabled,
                points: 0,
                next_item: None,
                next_need: 0,
                items: Vec::new(),
                equipped: None,
            });
        }
        let points = self.sunny_points(NaiveDate::MIN, NaiveDate::MAX, today)?;
        // 解锁过的不会再收回（旧的每日汇总被清理后点数可能变少）
        for id in &collection::ITEMS[..collection::unlocked_count(points)] {
            self.conn.execute(
                "INSERT OR IGNORE INTO collection (item, unlocked_ts) VALUES (?1, ?2)",
                params![id, now_ms],
            )?;
        }
        let mut stmt = self
            .conn
            .prepare("SELECT item, equipped FROM collection WHERE unlocked_ts IS NOT NULL")?;
        let rows: Vec<(String, bool)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let equipped = rows.iter().find(|(_, e)| *e).map(|(id, _)| id.clone());
        let next = collection::next_item(points);
        Ok(CollectionView {
            enabled,
            points,
            next_item: next.map(|(id, _)| id.to_string()),
            next_need: next.map_or(0, |(_, n)| n),
            items: collection::ITEMS
                .iter()
                .map(|id| ItemView {
                    id: (*id).into(),
                    unlocked: rows.iter().any(|(r, _)| r == id),
                    equipped: equipped.as_deref() == Some(id),
                })
                .collect(),
            equipped,
        })
    }

    /// 穿上一件（必须已解锁）或脱下（`None`）。同一时间只穿一件。返回是否成功。
    pub fn collection_equip(&self, item: Option<&str>) -> Result<bool, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(id) = item {
            let unlocked: bool = tx.query_row(
                "SELECT count(*) > 0 FROM collection WHERE item = ?1 AND unlocked_ts IS NOT NULL",
                [id],
                |r| r.get(0),
            )?;
            if !unlocked {
                return Ok(false);
            }
        }
        tx.execute("UPDATE collection SET equipped = 0 WHERE equipped != 0", [])?;
        if let Some(id) = item {
            tx.execute("UPDATE collection SET equipped = 1 WHERE item = ?1", [id])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// 关闭“晴天收集”：解锁记录与穿戴一并删除（D-30“直到用户关闭该功能”）。
    pub fn collection_clear(&self) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM collection", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn seed(db: &Db) {
        // 两天：每天护眼 / 活动 8 次、喝水 6 次，没在 18:00 后打字（早停 +2）→ 8 + 4 + 2 = 14 / 天
        for date in ["2026-10-13", "2026-10-14"] {
            db.conn()
                .execute(
                    "INSERT INTO daily_summary (date, typing_min, rests_done, water, focus_min) VALUES (?1, 90, 14, 6, 120)",
                    [date],
                )
                .unwrap();
        }
        let done = Local.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).unwrap().timestamp_millis();
        for i in 0..3 {
            db.conn()
                .execute(
                    "INSERT INTO todo (title, status, source, dedup_hash, created_ts, done_ts) VALUES ('t', 'done', 'manual', ?1, 0, ?2)",
                    params![format!("h{i}"), done],
                )
                .unwrap();
        }
    }

    #[test]
    fn points_come_from_healthy_actions_only() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let today = d("2026-10-15");
        // 专注 120 分钟、打字 90 分钟都不加分
        assert_eq!(db.sunny_points(NaiveDate::MIN, NaiveDate::MAX, today).unwrap(), 14 * 2 + 3);
        assert_eq!(db.sunny_points(d("2026-10-14"), d("2026-10-15"), today).unwrap(), 14 + 3);
        // 当天的早停还没结算
        assert_eq!(db.sunny_points(d("2026-10-14"), d("2026-10-15"), d("2026-10-14")).unwrap(), 12 + 3);
    }

    #[test]
    fn unlocks_stick_and_one_item_is_worn_at_a_time() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let v = db.collection_view(true, d("2026-10-15"), 1).unwrap();
        assert_eq!(v.points, 31);
        assert_eq!(
            v.items.iter().filter(|i| i.unlocked).map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["straw_hat"]
        );
        assert_eq!((v.next_item.as_deref(), v.next_need), (Some("scarf"), 9));

        assert!(!db.collection_equip(Some("scarf")).unwrap(), "没解锁不能穿");
        assert!(db.collection_equip(Some("straw_hat")).unwrap());
        let v = db.collection_view(true, d("2026-10-15"), 2).unwrap();
        assert_eq!(v.equipped.as_deref(), Some("straw_hat"));

        // 每日汇总被清理后点数变少，已解锁的仍在
        db.conn().execute("DELETE FROM daily_summary", []).unwrap();
        let v = db.collection_view(true, d("2026-10-15"), 3).unwrap();
        assert_eq!(v.points, 3);
        assert!(v.items[0].unlocked && v.items[0].equipped);

        assert!(db.collection_equip(None).unwrap());
        assert_eq!(db.collection_view(true, d("2026-10-15"), 4).unwrap().equipped, None);

        // 关闭：不显示，记录清空
        assert!(!db.collection_view(false, d("2026-10-15"), 5).unwrap().enabled);
        db.collection_clear().unwrap();
        let n: i64 = db.conn().query_row("SELECT count(*) FROM collection", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
