//! 导入与恢复（09 FR-DAT-07，ADR 0037 第 5～8 条）：只收 [`super::export`] 导出的 zip。
//!
//! - **校验**：`manifest.json` 的格式名与版本；数据库结构版本高于当前就拒绝（先升级 Moodflow）；每个文件的 SHA-256；
//!   单个文件大小有上限，防止解压炸弹；
//! - **不导入**：同意记录（换了电脑要重新同意）、出网记录（是那台电脑的运行记录）、AI 密钥（本来就不在包里）；
//!   设置项可选是否导入，只收已登记的用户设置键，开发者选项（`dev.*`）不收；
//! - **合并**：内容完全相同的行（不算行号）视为同一条，跳过；其余作为新行插入、换新的行号，引用它们的列
//!   （`mood_state.window_id`、`chat_message.session_id`、`feedback.target_id`）跟着换；以日期、键名为主键的表
//!   （每日汇总、基线、收集）已有的保留；
//! - **覆盖**：清空可导入的表后按原行号写入（覆盖前的自动备份由外壳先做，[`Db::backup_to`]）；
//! - 全部在一个事务里，任何一步出错整体回滚，当前数据不受影响；
//! - 导出时给 AI 文字加的 `ai_generated` 标记和“（内容由 AI 生成）”后缀在这里去掉，库里存的是原文。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zip::ZipArchive;

use super::export::{AI_SUFFIX, FORMAT, FORMAT_VERSION, MANIFEST, Manifest, sha256_hex};
use super::{Db, StoreError};
use crate::domain::settings;

/// 包里单个文件的上限（1 年数据的最大表也远小于此）
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// 不导入的表
pub const NOT_IMPORTED: &[&str] = &["consent", "net_log"];

/// 先导入的表（被别的表引用），其余按名字顺序
const FIRST: &[&str] = &[
    "window_features",
    "mood_state",
    "chat_session",
    "chat_message",
];

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("不是 Moodflow 导出的文件")]
    NotExport,
    #[error("导出包格式版本 {0} 比这个 Moodflow 新")]
    FormatTooNew(u32),
    #[error("数据库结构版本 {found} 比这个 Moodflow（{current}）新")]
    SchemaTooNew { found: i64, current: i64 },
    #[error("文件 {0} 校验不通过，可能不完整或被改过")]
    Checksum(String),
    #[error("文件 {0} 太大")]
    TooLarge(String),
    #[error("读取导出包失败：{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("读取导出包失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("导出包内容无法解析：{0}")]
    Json(#[from] serde_json::Error),
    #[error("写入数据库失败：{0}")]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// 合并还是覆盖
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// 保留两边的数据，相同的只留一份
    Merge,
    /// 用导入的数据替换（先自动备份当前数据库）
    Overwrite,
}

/// 导入结果
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportReport {
    /// 新写入的行
    pub added: u32,
    /// 跳过的行（合并时已有相同的、设置不导入或不认识的键等）
    pub skipped: u32,
    /// 导入了几张表
    pub tables: u32,
}

/// 读出并校验过的导出包
pub struct Package {
    pub manifest: Manifest,
    /// 表名 → 行（字段名即列名），按包里的顺序
    pub tables: Vec<(String, Vec<Map<String, Value>>)>,
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, ImportError> {
    let entry = zip.by_name(name).map_err(|_| ImportError::NotExport)?;
    if entry.size() > MAX_FILE_BYTES {
        return Err(ImportError::TooLarge(name.into()));
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    // 声明的大小不可信：读的时候也只读到上限 + 1
    entry.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        return Err(ImportError::TooLarge(name.into()));
    }
    Ok(buf)
}

/// 打开并校验导出包。`current_schema` 是当前数据库的结构版本。
pub fn read_package(path: &Path, current_schema: i64) -> Result<Package, ImportError> {
    let file = File::open(path)?;
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|_| ImportError::NotExport)?;
    let manifest: Manifest = serde_json::from_slice(&read_entry(&mut zip, MANIFEST)?)
        .map_err(|_| ImportError::NotExport)?;
    if manifest.format != FORMAT {
        return Err(ImportError::NotExport);
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err(ImportError::FormatTooNew(manifest.format_version));
    }
    if manifest.schema_version > current_schema {
        return Err(ImportError::SchemaTooNew {
            found: manifest.schema_version,
            current: current_schema,
        });
    }
    let mut tables = Vec::new();
    for f in &manifest.files {
        let bytes = read_entry(&mut zip, &f.path).map_err(|e| match e {
            ImportError::NotExport => ImportError::Checksum(f.path.clone()),
            e => e,
        })?;
        if sha256_hex(&bytes) != f.sha256 {
            return Err(ImportError::Checksum(f.path.clone()));
        }
        if let Some(name) = f
            .path
            .strip_prefix("tables/")
            .and_then(|n| n.strip_suffix(".json"))
        {
            let rows: Vec<Map<String, Value>> = serde_json::from_slice(&bytes)?;
            tables.push((name.to_string(), rows));
        }
    }
    Ok(Package { manifest, tables })
}

/// 一张表的列：名字与是否为主键的一部分
struct Columns {
    names: Vec<String>,
    /// 单列整数主键 `id`：合并时换新行号
    id_pk: bool,
}

fn columns(conn: &rusqlite::Connection, table: &str) -> Result<Columns, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT name, pk FROM pragma_table_info('{}')",
        table.replace('\'', "''")
    ))?;
    let cols: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let pks: Vec<&str> = cols
        .iter()
        .filter(|(_, pk)| *pk > 0)
        .map(|(n, _)| n.as_str())
        .collect();
    Ok(Columns {
        id_pk: pks == ["id"],
        names: cols.into_iter().map(|(n, _)| n).collect(),
    })
}

fn sql_value(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| n.as_f64().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

/// 去掉导出时附加的 AI 标记与后缀（[`super::export`] 的 `mark_ai`）。
fn strip_ai_marks(table: &str, row: &mut Map<String, Value>) {
    let text_col = match table {
        "comfort_log" => "text",
        "chat_message" | "letter" | "diary" => "content",
        _ => return,
    };
    let ai = match row.get("ai_generated") {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        _ => false,
    };
    if table == "chat_message" {
        // chat_message 本来就有这一列，导出时被换成了布尔值
        row.insert("ai_generated".into(), Value::from(i64::from(ai)));
    } else {
        row.remove("ai_generated");
    }
    if ai
        && let Some(Value::String(text)) = row.get_mut(text_col)
        && let Some(stripped) = text.strip_suffix(AI_SUFFIX)
    {
        *text = stripped.to_string();
    }
}

/// 设置键能不能导入：已登记的用户设置，开发者选项除外
fn importable_setting(key: &str) -> bool {
    settings::spec(key).is_some() && !key.starts_with("dev.")
}

impl Db {
    /// 把当前数据库完整复制到 `dest`（`VACUUM INTO`，WAL 下也是一致的快照）。覆盖导入前的自动备份用。
    pub fn backup_to(&self, dest: &Path) -> Result<(), StoreError> {
        self.conn()
            .execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
        Ok(())
    }

    /// 在一个事务里导入 `pkg`。`include_settings` 为假时设置项一律跳过。
    pub fn import_package(
        &self,
        pkg: &Package,
        mode: ImportMode,
        include_settings: bool,
    ) -> Result<ImportReport, ImportError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        // 引用关系在提交时再检查：覆盖时先删后插，中间状态不完整
        tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;

        let mut order: Vec<&(String, Vec<Map<String, Value>>)> = pkg
            .tables
            .iter()
            .filter(|(t, _)| !NOT_IMPORTED.contains(&t.as_str()))
            .filter(|(t, _)| include_settings || t != "settings")
            .collect();
        order.sort_by_key(|(t, _)| {
            (
                FIRST.iter().position(|f| f == t).unwrap_or(FIRST.len()),
                t.clone(),
            )
        });

        if mode == ImportMode::Overwrite {
            // 覆盖：清空所有可导入的数据表（设置只换导入的键，内部键与未导入的键保留）
            let mut stmt = tx.prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table'
                   AND name NOT LIKE 'sqlite_%' AND name NOT IN ('schema_version', 'settings')",
            )?;
            let names: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            for t in names.iter().filter(|t| !NOT_IMPORTED.contains(&t.as_str())) {
                tx.execute(&format!("DELETE FROM \"{}\"", t.replace('"', "\"\"")), [])?;
            }
        }

        let mut report = ImportReport::default();
        // 表名 → 包里的行号 → 库里的行号
        let mut ids: HashMap<&str, HashMap<i64, i64>> = HashMap::new();

        for (table, rows) in order {
            let cols = columns(&tx, table)?;
            if cols.names.is_empty() {
                // 当前版本没有这张表（理论上不会：结构版本不高于当前）
                report.skipped += rows.len() as u32;
                continue;
            }
            report.tables += 1;
            let quoted = format!("\"{}\"", table.replace('"', "\"\""));
            // 合并：先把已有各行的内容签名读进内存，逐行比对是 O(1)（逐行查库在一年的数据上是平方级）
            let mut seen: HashMap<String, i64> = if mode == ImportMode::Merge && cols.id_pk {
                existing_signatures(&tx, &quoted, &cols.names)?
            } else {
                HashMap::new()
            };
            for raw in rows {
                let mut row = raw.clone();
                strip_ai_marks(table, &mut row);
                if table == "settings" {
                    let key = row.get("key").and_then(Value::as_str).unwrap_or_default();
                    if !importable_setting(key) {
                        report.skipped += 1;
                        continue;
                    }
                }
                remap(table, &mut row, &ids);

                // 只写当前表里有的列；合并且是行号主键时不写行号
                let skip_id = mode == ImportMode::Merge && cols.id_pk;
                let present: Vec<&String> = cols
                    .names
                    .iter()
                    .filter(|c| row.contains_key(c.as_str()))
                    .filter(|c| !(skip_id && c.as_str() == "id"))
                    .collect();
                let values: Vec<SqlValue> = present
                    .iter()
                    .map(|c| sql_value(&row[c.as_str()]))
                    .collect();
                let old_id = row.get("id").and_then(Value::as_i64);

                // 内容完全相同（不算行号）的已有行：视为同一条
                let sig = if mode == ImportMode::Merge && cols.id_pk {
                    let sig = signature(&cols.names, |c| row.get(c).map(sql_value));
                    if let Some(&existing) = seen.get(&sig) {
                        if let (Some(old), Some(key)) = (old_id, referenced(table)) {
                            ids.entry(key).or_default().insert(old, existing);
                        }
                        report.skipped += 1;
                        continue;
                    }
                    Some(sig)
                } else {
                    None
                };

                let names: Vec<String> = present
                    .iter()
                    .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
                    .collect();
                let marks: Vec<String> = (1..=present.len()).map(|i| format!("?{i}")).collect();
                let verb = match (table.as_str(), mode, cols.id_pk) {
                    // 选了导入设置：导入的值替换当前值
                    ("settings", _, _) => "INSERT OR REPLACE",
                    // 合并时以日期 / 键名为主键的表：已有的保留
                    (_, ImportMode::Merge, false) => "INSERT OR IGNORE",
                    _ => "INSERT",
                };
                let n = tx.execute(
                    &format!(
                        "{verb} INTO {quoted} ({}) VALUES ({})",
                        names.join(", "),
                        marks.join(", ")
                    ),
                    rusqlite::params_from_iter(values.iter()),
                )?;
                if n == 0 {
                    report.skipped += 1;
                    continue;
                }
                report.added += 1;
                let new_id = if skip_id {
                    tx.last_insert_rowid()
                } else {
                    old_id.unwrap_or_default()
                };
                if let Some(sig) = sig {
                    // 包里自己重复的行也只留一份
                    seen.insert(sig, new_id);
                }
                if let (Some(old), Some(key)) = (old_id, referenced(table)) {
                    ids.entry(key).or_default().insert(old, new_id);
                }
            }
        }
        tx.commit()?;
        Ok(report)
    }
}

/// 一行除行号以外各列的值连成的签名（缺的列按空值算）
fn signature(names: &[String], mut get: impl FnMut(&str) -> Option<SqlValue>) -> String {
    let mut out = String::new();
    for c in names.iter().filter(|c| c.as_str() != "id") {
        let v = get(c).unwrap_or(SqlValue::Null);
        out.push_str(&format!("{v:?}\u{1f}"));
    }
    out
}

/// 已有各行的签名 → 行号
fn existing_signatures(
    conn: &rusqlite::Connection,
    quoted: &str,
    names: &[String],
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let cols: Vec<&String> = names.iter().filter(|c| c.as_str() != "id").collect();
    let list: Vec<String> = cols
        .iter()
        .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
        .collect();
    let sql = if list.is_empty() {
        format!("SELECT id FROM {quoted}")
    } else {
        format!("SELECT id, {} FROM {quoted}", list.join(", "))
    };
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = HashMap::new();
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let mut i = 0usize;
        let sig = signature(names, |_| {
            i += 1;
            r.get::<_, SqlValue>(i).ok()
        });
        out.entry(sig).or_insert(id);
    }
    Ok(out)
}

/// 会被别的表引用行号的表：要记下导入前后的行号对应
fn referenced(table: &str) -> Option<&'static str> {
    ["window_features", "mood_state", "chat_session"]
        .into_iter()
        .find(|t| *t == table)
}

/// 引用别的表行号的列，换成导入后的行号；找不到对应行时可空的列置空。
fn remap(table: &str, row: &mut Map<String, Value>, ids: &HashMap<&str, HashMap<i64, i64>>) {
    let refs: &[(&str, &str)] = match table {
        "mood_state" => &[("window_id", "window_features")],
        "chat_message" => &[("session_id", "chat_session")],
        "feedback" if row.get("target").and_then(Value::as_str) == Some("mood_state") => {
            &[("target_id", "mood_state")]
        }
        _ => &[],
    };
    for (col, target) in refs {
        let Some(old) = row.get(*col).and_then(Value::as_i64) else {
            continue;
        };
        match ids.get(target).and_then(|m| m.get(&old)) {
            Some(new) => {
                row.insert((*col).into(), Value::from(*new));
            }
            // 引用的行不在包里：可空的列置空（必填的 session_id 保持原样，交给外键检查）
            None if *col != "session_id" => {
                row.insert((*col).into(), Value::Null);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::store::export;
    use chrono::TimeZone;

    struct Tmp(std::path::PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp() -> Tmp {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("mf-import-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    fn count(db: &Db, t: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    }

    /// 一个有几类数据的库：状态窗口、对话（含 AI 回复）、日记、反馈、同意、设置、出网记录
    fn source_db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO window_features (id, start_ts, end_ts, app_cat, features_json) VALUES (7, 0, 1, 'chat', '{}');
                 INSERT INTO mood_state (id, ts, window_id, state, shown_state, source) VALUES (3, 1, 7, 'tired', 'tired', 'rule');
                 INSERT INTO feedback (ts, target, target_id, verdict) VALUES (2, 'mood_state', 3, 'fit');
                 INSERT INTO chat_session (id, title, created_ts) VALUES (5, '作业好多', 10);
                 INSERT INTO chat_message (session_id, role, content, ts, ai_generated) VALUES (5, 'user', '好累', 11, 0);
                 INSERT INTO chat_message (session_id, role, content, ts, ai_generated) VALUES (5, 'assistant', '辛苦了', 12, 1);
                 INSERT INTO diary (date, content, source, created_ts, updated_ts) VALUES ('2026-10-01', '今天还行', 'ai_draft', 1, 1);
                 INSERT INTO daily_summary (date, typing_min) VALUES ('2026-10-01', 90);
                 INSERT INTO consent (item, policy_ver, granted, ts) VALUES ('sense', 1, 1, 0);
                 INSERT INTO net_log (ts, api, fields, status) VALUES (1, 'jev', '[]', 'ok');
                 INSERT INTO settings (key, value) VALUES ('care.level', '\"less\"'), ('dev.demo', 'true'), ('no.such', '1');",
            )
            .unwrap();
        db
    }

    fn package(db: &Db, dir: &Path) -> Package {
        let path = dir.join("x.zip");
        let now = chrono::Local
            .timestamp_millis_opt(1_700_000_000_000)
            .unwrap();
        export::export(db, &path, "test", now).unwrap();
        read_package(&path, db.schema_version().unwrap()).unwrap()
    }

    #[test]
    fn overwrite_restores_data_without_consent_or_net_log_and_strips_ai_marks() {
        let dir = tmp();
        let pkg = package(&source_db(), &dir.0);
        let target = Db::open_in_memory().unwrap();
        target
            .conn()
            .execute_batch("INSERT INTO memory (content, created_ts) VALUES ('旧的', 0);")
            .unwrap();
        let r = target
            .import_package(&pkg, ImportMode::Overwrite, false)
            .unwrap();
        assert!(r.added >= 8, "{r:?}");
        assert_eq!(count(&target, "memory"), 0, "覆盖会清掉原来的");
        assert_eq!(count(&target, "consent"), 0, "同意不导入");
        assert_eq!(count(&target, "net_log"), 0, "出网记录不导入");
        assert_eq!(count(&target, "chat_message"), 2);
        let (content, ai): (String, i64) = target
            .conn()
            .query_row(
                "SELECT content, ai_generated FROM chat_message WHERE role = 'assistant'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((content.as_str(), ai), ("辛苦了", 1), "去掉导出时加的后缀");
        let diary: String = target
            .conn()
            .query_row("SELECT content FROM diary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(diary, "今天还行");
        assert_eq!(count(&target, "settings"), 0, "没选导入设置");
    }

    #[test]
    fn settings_are_imported_only_when_asked_and_only_registered_user_keys() {
        let dir = tmp();
        let pkg = package(&source_db(), &dir.0);
        let target = Db::open_in_memory().unwrap();
        target
            .import_package(&pkg, ImportMode::Merge, true)
            .unwrap();
        let keys: Vec<String> = target
            .conn()
            .prepare("SELECT key FROM settings ORDER BY key")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(keys, ["care.level"], "开发者选项与未登记的键不导入");
    }

    #[test]
    fn merge_keeps_both_sides_skips_identical_rows_and_remaps_references() {
        let dir = tmp();
        let src = source_db();
        let pkg = package(&src, &dir.0);
        let target = Db::open_in_memory().unwrap();
        // 目标库已有一段对话占了行号 5，以及同一天的汇总
        target
            .conn()
            .execute_batch(
                "INSERT INTO chat_session (id, title, created_ts) VALUES (5, '别的对话', 99);
                 INSERT INTO chat_message (session_id, role, content, ts) VALUES (5, 'user', '别的', 99);
                 INSERT INTO window_features (id, start_ts, end_ts, app_cat, features_json) VALUES (7, 50, 51, 'doc', '{}');
                 INSERT INTO daily_summary (date, typing_min) VALUES ('2026-10-01', 30);",
            )
            .unwrap();
        target
            .import_package(&pkg, ImportMode::Merge, false)
            .unwrap();
        assert_eq!(count(&target, "chat_session"), 2);
        assert_eq!(count(&target, "chat_message"), 3);
        // 导入的消息挂在导入的会话下，不挂到原来的 5 号
        let session: i64 = target
            .conn()
            .query_row(
                "SELECT id FROM chat_session WHERE title = '作业好多'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(session, 5);
        let n: i64 = target
            .conn()
            .query_row(
                "SELECT count(*) FROM chat_message WHERE session_id = ?1",
                [session],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
        // 状态记录指向导入的窗口，反馈指向导入的状态记录
        let (ms_id, win): (i64, i64) = target
            .conn()
            .query_row("SELECT id, window_id FROM mood_state", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        let win_start: i64 = target
            .conn()
            .query_row(
                "SELECT start_ts FROM window_features WHERE id = ?1",
                [win],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(win_start, 0);
        let fb: i64 = target
            .conn()
            .query_row("SELECT target_id FROM feedback", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fb, ms_id);
        // 以日期为主键的汇总：已有的保留
        let typing: i64 = target
            .conn()
            .query_row("SELECT typing_min FROM daily_summary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(typing, 30);

        // 再导一次：内容都相同，全部跳过
        let again = target
            .import_package(&pkg, ImportMode::Merge, false)
            .unwrap();
        assert_eq!(again.added, 0, "{again:?}");
        assert_eq!(count(&target, "chat_message"), 3);
    }

    #[test]
    fn tampered_or_foreign_packages_are_rejected() {
        let dir = tmp();
        let src = source_db();
        let path = dir.0.join("x.zip");
        let now = chrono::Local
            .timestamp_millis_opt(1_700_000_000_000)
            .unwrap();
        export::export(&src, &path, "test", now).unwrap();
        let current = src.schema_version().unwrap();

        // 结构版本更新的包
        assert!(matches!(
            read_package(&path, current - 1),
            Err(ImportError::SchemaTooNew { .. })
        ));

        // 改过一个表文件：校验不通过
        let tampered = dir.0.join("t.zip");
        {
            let mut zin = ZipArchive::new(File::open(&path).unwrap()).unwrap();
            let mut zout = zip::ZipWriter::new(File::create(&tampered).unwrap());
            for i in 0..zin.len() {
                let mut e = zin.by_index(i).unwrap();
                let name = e.name().to_string();
                let mut buf = Vec::new();
                e.read_to_end(&mut buf).unwrap();
                if name == "tables/diary.json" {
                    buf = String::from_utf8(buf)
                        .unwrap()
                        .replace("今天还行", "今天很糟")
                        .into_bytes();
                }
                zout.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zout, &buf).unwrap();
            }
            zout.finish().unwrap();
        }
        assert!(
            matches!(read_package(&tampered, current), Err(ImportError::Checksum(p)) if p == "tables/diary.json")
        );

        // 不是 zip
        let junk = dir.0.join("j.zip");
        std::fs::write(&junk, b"hello").unwrap();
        assert!(matches!(
            read_package(&junk, current),
            Err(ImportError::NotExport)
        ));
    }

    #[test]
    fn failed_import_rolls_back_everything() {
        let dir = tmp();
        let mut pkg = package(&source_db(), &dir.0);
        // 让最后一张表的一行违反约束（diary.source 只能是三个值之一）
        for (t, rows) in &mut pkg.tables {
            if t == "diary" {
                rows[0].insert("source".into(), Value::from("bogus"));
            }
        }
        let target = Db::open_in_memory().unwrap();
        target
            .conn()
            .execute_batch("INSERT INTO memory (content, created_ts) VALUES ('旧的', 0);")
            .unwrap();
        assert!(
            target
                .import_package(&pkg, ImportMode::Overwrite, false)
                .is_err()
        );
        assert_eq!(count(&target, "memory"), 1, "覆盖失败时原来的数据还在");
        assert_eq!(count(&target, "chat_message"), 0);
    }

    /// NFR-PERF-10：1 年数据的导入在 60 秒内。按一年约 12 万个特征窗口与状态记录造数据，覆盖导入到空库、
    /// 再合并导入同一份（全部重复）。耗时较长，默认不跑：`cargo test -p moodflow-hub-core --release -- --ignored year_of_data`
    #[test]
    #[ignore]
    fn year_of_data_imports_within_a_minute() {
        let dir = tmp();
        let src = Db::open(&dir.0.join("src.db")).unwrap();
        {
            let tx = src.conn().unchecked_transaction().unwrap();
            for i in 0..120_000i64 {
                tx.execute(
                    "INSERT INTO window_features (id, start_ts, end_ts, app_cat, features_json) VALUES (?1, ?2, ?3, 'doc', '{\"iki_med\":180.5}')",
                    rusqlite::params![i + 1, i * 60_000, i * 60_000 + 30_000],
                )
                .unwrap();
                tx.execute(
                    "INSERT INTO mood_state (ts, window_id, state, shown_state, source) VALUES (?1, ?2, 'fluent', 'fluent', 'rule')",
                    rusqlite::params![i * 60_000 + 30_000, i + 1],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        let pkg = package(&src, &dir.0);
        let target = Db::open(&dir.0.join("dst.db")).unwrap();
        let t0 = std::time::Instant::now();
        target
            .import_package(&pkg, ImportMode::Overwrite, false)
            .unwrap();
        let overwrite = t0.elapsed();
        let t1 = std::time::Instant::now();
        let r = target
            .import_package(&pkg, ImportMode::Merge, false)
            .unwrap();
        let merge = t1.elapsed();
        eprintln!("覆盖 {overwrite:?}，合并（全部重复）{merge:?}，{r:?}");
        assert_eq!(r.added, 0);
        assert!(overwrite.as_secs() < 60 && merge.as_secs() < 60);
    }

    #[test]
    fn backup_copies_the_database() {
        let dir = tmp();
        let path = dir.0.join("a.db");
        let db = Db::open(&path).unwrap();
        db.conn()
            .execute_batch("INSERT INTO memory (content, created_ts) VALUES ('留着', 0);")
            .unwrap();
        let bak = dir.0.join("a.db.bak-1");
        db.backup_to(&bak).unwrap();
        let copy = Db::open(&bak).unwrap();
        assert_eq!(count(&copy, "memory"), 1);
    }
}
