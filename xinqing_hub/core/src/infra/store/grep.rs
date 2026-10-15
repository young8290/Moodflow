//! 库扫描（`tools/db-grep`，产品书 12 第 2 节、17 第 4 节；NFR-PRI-04 / TC-PRI-01）：在数据库所有表的所有列里
//! 做子串搜索，验证用户打过的原文没有落库。
//!
//! 每一列都按文本比较（`CAST(col AS TEXT)`），不只看声明为 TEXT 的列：SQLite 的列类型只是倾向，
//! 数值列里也可能被写进文字。`fragment` 大于 0 时，除整句外还搜句中每个这么长的连续片段，
//! 截断或拆开存的原文也能查出来。

use std::collections::BTreeSet;

use rusqlite::Connection;

use super::StoreError;

/// 一处命中：哪张表哪一列的哪一行，含第几句测试句（从 0 开始）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hit {
    pub table: String,
    pub column: String,
    pub rowid: i64,
    pub phrase: usize,
}

/// NFR-PRI-04 的例外：用户确认的日程标题和地点（以及同样由用户确认的待办标题），`表.列`。
pub const CONFIRMED_USER_TEXT: &[&str] = &["schedule.title", "schedule.location", "todo.title"];

/// 搜索用的词：整句，加上 `fragment` 个字一段的全部连续片段（句子不长于 `fragment` 时只有整句）。
/// 空白不算字，去掉首尾空白后为空的句子跳过。
pub fn needles(phrase: &str, fragment: usize) -> Vec<String> {
    let phrase = phrase.trim();
    if phrase.is_empty() {
        return Vec::new();
    }
    let mut out = BTreeSet::from([phrase.to_string()]);
    let chars: Vec<char> = phrase.chars().collect();
    if fragment > 0 && chars.len() > fragment {
        for w in chars.windows(fragment) {
            let s: String = w.iter().collect();
            if !s.trim().is_empty() {
                out.insert(s);
            }
        }
    }
    out.into_iter().collect()
}

/// 扫全库。`allow` 里的 `表.列` 不算命中。同一行同一列同一句只报一次。
pub fn grep(
    conn: &Connection,
    phrases: &[String],
    fragment: usize,
    allow: &[&str],
) -> Result<Vec<Hit>, StoreError> {
    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let needles: Vec<(usize, String)> = phrases
        .iter()
        .enumerate()
        .flat_map(|(i, p)| needles(p, fragment).into_iter().map(move |n| (i, n)))
        .collect();
    let mut hits = BTreeSet::new();
    for table in &tables {
        let columns: Vec<String> = conn
            .prepare(&format!(
                "SELECT name FROM pragma_table_info({})",
                quote_str(table)
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for column in columns {
            if allow.contains(&format!("{table}.{column}").as_str()) {
                continue;
            }
            let sql = format!(
                "SELECT rowid FROM {} WHERE instr(CAST({} AS TEXT), ?1) > 0",
                quote_ident(table),
                quote_ident(&column)
            );
            let mut stmt = match conn.prepare(&sql) {
                Ok(s) => s,
                // WITHOUT ROWID 表没有 rowid，行号记 -1
                Err(_) => conn.prepare(&format!(
                    "SELECT -1 FROM {} WHERE instr(CAST({} AS TEXT), ?1) > 0",
                    quote_ident(table),
                    quote_ident(&column)
                ))?,
            };
            for (phrase, needle) in &needles {
                let rows: Vec<i64> = stmt
                    .query_map([needle], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
                for rowid in rows {
                    hits.insert(Hit {
                        table: table.clone(),
                        column: column.clone(),
                        rowid,
                        phrase: *phrase,
                    });
                }
            }
        }
    }
    Ok(hits.into_iter().collect())
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::store::Db;

    #[test]
    fn needles_cover_whole_sentence_and_fragments() {
        assert_eq!(needles("  ", 4), Vec::<String>::new());
        assert_eq!(needles("周五开会", 4), ["周五开会"]);
        assert_eq!(
            needles("周五开组会", 4),
            ["五开组会", "周五开组", "周五开组会"]
        );
        assert_eq!(needles("abc", 0), ["abc"]);
    }

    #[test]
    fn finds_text_in_any_column_except_allowed() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        c.execute(
            "INSERT INTO settings (key, value) VALUES ('x', '我明天下午要交实验报告')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO todo (title, status, source, dedup_hash, created_ts) VALUES ('交实验报告', 'open', 'ai', 'h', 0)",
            [],
        )
        .unwrap();
        // 数值列里塞了文字也查得到
        c.execute(
            "INSERT INTO daily_summary (date, typing_min) VALUES ('2026-10-15', '实验报告真难写')",
            [],
        )
        .unwrap();
        let phrases = vec![
            "我明天下午要交实验报告".to_string(),
            "实验报告真难写".to_string(),
            "完全没出现过的一句话".to_string(),
        ];

        let whole = grep(c, &phrases, 0, &[]).unwrap();
        let at: Vec<_> = whole
            .iter()
            .map(|h| (h.table.as_str(), h.column.as_str(), h.phrase))
            .collect();
        assert_eq!(
            at,
            [("daily_summary", "typing_min", 1), ("settings", "value", 0)]
        );

        // 片段能查到只存了一部分的原文（待办标题），用户确认的待办标题可以放过
        let frag = grep(c, &phrases, 5, &[]).unwrap();
        assert!(
            frag.iter()
                .any(|h| h.table == "todo" && h.column == "title")
        );
        let allowed = grep(c, &phrases, 5, CONFIRMED_USER_TEXT).unwrap();
        assert!(!allowed.iter().any(|h| h.table == "todo"));
        assert!(allowed.iter().all(|h| h.phrase != 2));
    }
}
