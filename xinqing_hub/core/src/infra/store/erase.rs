//! 撤回同意 ① 时一并删除情绪数据（09 FR-DAT-05 第 3 条，ADR 0037）。
//!
//! “情绪数据”指感知打字节奏得出的东西：特征窗口、状态判断、个人基线、对判断的“准 / 不准”反馈、自己报告的心情，
//! 以及每日汇总里的状态分布、主导状态和情绪倾向（同一行的输入时长、休息与饮水次数不是情绪数据，留着）。
//! 对话、日记、日程待办、周信等用户自己写的内容也不算（要全删用“删除全部数据”）。

use super::{Db, StoreError};

/// 情绪数据所在的表，按删除顺序（`mood_state.window_id` 引用 `window_features`）
pub const MOOD_TABLES: &[&str] = &[
    "feedback",
    "self_report",
    "mood_state",
    "window_features",
    "baseline",
];

impl Db {
    /// 在一个事务里清空 [`MOOD_TABLES`]、抹掉每日汇总里的情绪列，返回改动的行数。
    pub fn delete_mood_data(&self) -> Result<u64, StoreError> {
        let tx = self.conn().unchecked_transaction()?;
        let mut n = 0u64;
        for t in MOOD_TABLES {
            n += tx.execute(&format!("DELETE FROM {t}"), [])? as u64;
        }
        n += tx.execute(
            "UPDATE daily_summary SET state_dist_json = NULL, dominant_state = NULL, avg_valence = NULL
             WHERE state_dist_json IS NOT NULL OR dominant_state IS NOT NULL OR avg_valence IS NOT NULL",
            [],
        )? as u64;
        tx.commit()?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(db: &Db, t: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn mood_tables_are_cleared_and_user_content_is_kept() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        c.execute_batch(
            "INSERT INTO window_features (id, start_ts, end_ts, app_cat, features_json) VALUES (1, 0, 1, 'other', '{}');
             INSERT INTO mood_state (ts, window_id, state, shown_state, source) VALUES (1, 1, 'tired', 'tired', 'rule');
             INSERT INTO baseline (feature, bucket, med, mad, n, updated_ts) VALUES ('iki', 'day', 1, 1, 1, 0);
             INSERT INTO daily_summary (date, typing_min, dominant_state, state_dist_json) VALUES ('2026-10-01', 90, 'tired', '{}');
             INSERT INTO memory (content, created_ts) VALUES ('喜欢猫', 0);",
        )
        .unwrap();
        assert!(db.delete_mood_data().unwrap() >= 3);
        for t in MOOD_TABLES {
            assert_eq!(count(&db, t), 0, "{t}");
        }
        assert_eq!(count(&db, "memory"), 1);
        let (typing, dominant): (i64, Option<String>) = db
            .conn()
            .query_row(
                "SELECT typing_min, dominant_state FROM daily_summary",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((typing, dominant), (90, None), "输入时长留着，主导状态抹掉");
    }

    #[test]
    fn every_mood_table_exists_in_the_schema() {
        let db = Db::open_in_memory().unwrap();
        for t in MOOD_TABLES {
            count(&db, t);
        }
    }
}
