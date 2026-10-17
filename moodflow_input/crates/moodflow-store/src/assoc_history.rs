//! 联想历史（redb `assoc_history` 表）：用户在某个**上文**之后选了哪条联想候选。
//!
//! 词语联想的 History 来源（论坛 t185 方案 B）。与 [`crate::freq`] 的分工：FREQ 记的是
//! 「这个词被正常打字选中过几次」（按输入码），这里记的是「在这个上文后面、从联想里
//! 选了它几次」——FREQ 给不了「上文 → 词」这一维。
//!
//! key = `"{schema}\0{上文}\0{整词}"`，value = count u32 + last_used i64（与 FREQ 同编码）。
//! 查询是按 `"{schema}\0{上文}\0"` 的一次范围扫描，在按键路径上（每次上屏后进联想时一次）。
//!
//! 不影响 [`Store::words_generation`]：它不进用户词文本索引，每次都直接读表。

use crate::freq::{FreqRecord, dec_freq, enc_freq};
use crate::store::{ASSOC_HIST, Store};
use crate::user_words::{enc_key, now_secs, split_key};
use redb::{ReadableTable, ReadableTableMetadata};

/// 每个上文最多记几条联想词（与 XSIME 一致）。超出时淘汰 (count, last_used) 最低的。
pub const MAX_PER_CONTEXT: usize = 8;
/// 整张表的条数上限（约等于 XSIME 的 600 个上文 × 8）。超出时按最久未用淘汰到 90%。
/// 此前这张表只增不减，常年使用会无限增长（XSIME 吸收计划 §3.8）。
pub const MAX_TOTAL: u64 = 4_800;

impl Store {
    /// 记一次「在 `context` 之后选了联想 `word`」：count++、last_used = now。
    pub fn record_assoc_pick(&self, schema: &str, context: &str, word: &str) -> anyhow::Result<()> {
        if schema.is_empty() || context.is_empty() || word.is_empty() {
            return Ok(());
        }
        let key = enc_key(schema, context, word);
        let now = now_secs();
        self.with_db(|db| {
            let txn = db.begin_write()?;
            {
                let mut t = txn.open_table(ASSOC_HIST)?;
                let count = t
                    .get(key.as_str())?
                    .and_then(|g| dec_freq(g.value()))
                    .map_or(0, |r| r.count);
                t.insert(
                    key.as_str(),
                    enc_freq(count.saturating_add(1), now).as_slice(),
                )?;
                prune_context(&mut t, schema, context, &key)?;
                if t.len()? > MAX_TOTAL {
                    prune_total(&mut t, &key)?;
                }
            }
            txn.commit()?;
            Ok(())
        })
    }

    /// `context` 之后选过的联想词，按 (count, last_used) 降序取前 `limit` 条（0 = 不限）。
    pub fn assoc_history(
        &self,
        schema: &str,
        context: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, FreqRecord)>> {
        let scan = format!("{schema}\u{0}{context}\u{0}");
        let mut out: Vec<(String, FreqRecord)> = self.with_db(|db| {
            let txn = db.begin_read()?;
            let t = txn.open_table(ASSOC_HIST)?;
            let mut v = Vec::new();
            for item in t.range(scan.as_str()..)? {
                let (k, val) = item?;
                let key = k.value();
                if !key.starts_with(&scan) {
                    break;
                }
                if let Some(rec) = dec_freq(val.value()) {
                    v.push((key[scan.len()..].to_string(), rec));
                }
            }
            Ok(v)
        })?;
        out.sort_by(|a, b| {
            (b.1.count, b.1.last_used)
                .cmp(&(a.1.count, a.1.last_used))
                .then_with(|| a.0.cmp(&b.0))
        });
        if limit > 0 {
            out.truncate(limit);
        }
        Ok(out)
    }

    /// 清空某方案的联想历史，返回删除条数。
    pub fn clear_assoc_history(&self, schema: &str) -> anyhow::Result<usize> {
        let scan = format!("{schema}\u{0}");
        self.with_db(|db| {
            let txn = db.begin_write()?;
            let n;
            {
                let mut t = txn.open_table(ASSOC_HIST)?;
                let mut keys = Vec::new();
                for item in t.range(scan.as_str()..)? {
                    let (k, _) = item?;
                    if !k.value().starts_with(&scan) {
                        break;
                    }
                    keys.push(k.value().to_string());
                }
                n = keys.len();
                for k in &keys {
                    t.remove(k.as_str())?;
                }
            }
            txn.commit()?;
            Ok(n)
        })
    }

    /// 导出某方案全部联想历史为 jsonl（每行 {"context","text","count","last_used"}）。
    pub fn export_assoc_history_jsonl(&self, schema: &str) -> anyhow::Result<String> {
        let scan = format!("{schema}\u{0}");
        let rows: Vec<(String, String, FreqRecord)> = self.with_db(|db| {
            let txn = db.begin_read()?;
            let t = txn.open_table(ASSOC_HIST)?;
            let mut v = Vec::new();
            for item in t.range(scan.as_str()..)? {
                let (k, val) = item?;
                let key = k.value();
                if !key.starts_with(&scan) {
                    break;
                }
                if let (Some((_, ctx, word)), Some(rec)) = (split_key(key), dec_freq(val.value())) {
                    v.push((ctx.to_string(), word.to_string(), rec));
                }
            }
            Ok(v)
        })?;
        let mut out = String::new();
        for (ctx, word, rec) in rows {
            out.push_str(&serde_json::to_string(&serde_json::json!({
                "context": ctx, "text": word, "count": rec.count, "last_used": rec.last_used,
            }))?);
            out.push('\n');
        }
        Ok(out)
    }

    /// 从 jsonl 导入联想历史（单写事务；已存在取 max(count) / max(last_used)）。
    /// 返回 (imported, skipped)。
    pub fn import_assoc_history_jsonl(
        &self,
        schema: &str,
        text: &str,
    ) -> anyhow::Result<(usize, usize)> {
        let normalized = moodflow_utils::text::normalize_input(text);
        let mut rows: Vec<(String, u32, i64)> = Vec::new();
        let mut skipped = 0usize;
        for line in normalized.as_ref().lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v = serde_json::from_str::<serde_json::Value>(line).ok();
            let get = |k: &str| v.as_ref().and_then(|v| v.get(k));
            let (Some(ctx), Some(word)) = (
                get("context")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty()),
                get("text")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty()),
            ) else {
                skipped += 1;
                continue;
            };
            let count = get("count").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let last_used = get("last_used").and_then(|x| x.as_i64()).unwrap_or(0);
            rows.push((enc_key(schema, ctx, word), count, last_used));
        }
        let imported = rows.len();
        self.with_db(|db| {
            let txn = db.begin_write()?;
            {
                let mut t = txn.open_table(ASSOC_HIST)?;
                for (key, count, last_used) in &rows {
                    let merged = match t.get(key.as_str())?.and_then(|g| dec_freq(g.value())) {
                        Some(old) => (old.count.max(*count), old.last_used.max(*last_used)),
                        None => (*count, *last_used),
                    };
                    t.insert(key.as_str(), enc_freq(merged.0, merged.1).as_slice())?;
                }
            }
            txn.commit()?;
            Ok(())
        })?;
        Ok((imported, skipped))
    }
}

/// 同一上文超过 [`MAX_PER_CONTEXT`] 条时，淘汰 (count, last_used) 最低的；刚选的那条不淘汰。
fn prune_context(
    t: &mut redb::Table<&str, &[u8]>,
    schema: &str,
    context: &str,
    keep: &str,
) -> anyhow::Result<()> {
    let scan = format!("{schema}\u{0}{context}\u{0}");
    let mut rows: Vec<(String, FreqRecord)> = Vec::new();
    for item in t.range(scan.as_str()..)? {
        let (k, v) = item?;
        let key = k.value();
        if !key.starts_with(&scan) {
            break;
        }
        if let Some(rec) = dec_freq(v.value()) {
            rows.push((key.to_string(), rec));
        }
    }
    if rows.len() <= MAX_PER_CONTEXT {
        return Ok(());
    }
    rows.sort_by(|a, b| {
        (b.0 == keep, b.1.count, b.1.last_used).cmp(&(a.0 == keep, a.1.count, a.1.last_used))
    });
    for (k, _) in rows.into_iter().skip(MAX_PER_CONTEXT) {
        t.remove(k.as_str())?;
    }
    Ok(())
}

/// 整表超过 [`MAX_TOTAL`] 时按最久未用淘汰到 90%；刚选的那条不淘汰。
fn prune_total(t: &mut redb::Table<&str, &[u8]>, keep: &str) -> anyhow::Result<()> {
    let mut rows: Vec<(i64, String)> = Vec::new();
    for item in t.iter()? {
        let (k, v) = item?;
        if k.value() == keep {
            continue;
        }
        let last = dec_freq(v.value()).map_or(0, |r| r.last_used);
        rows.push((last, k.value().to_string()));
    }
    let target = (MAX_TOTAL * 9 / 10) as usize;
    let total = rows.len() + 1;
    if total <= target {
        return Ok(());
    }
    rows.sort();
    for (_, k) in rows.into_iter().take(total - target) {
        t.remove(k.as_str())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tag: &str) -> Store {
        let p = std::env::temp_dir().join(format!("moodflow_assoc_hist_{tag}.redb"));
        let _ = std::fs::remove_file(&p);
        Store::open(p).unwrap()
    }

    #[test]
    fn record_query_is_keyed_by_context_and_sorted_by_count() {
        let s = store("q");
        s.record_assoc_pick("wb", "荷", "荷花").unwrap();
        for _ in 0..3 {
            s.record_assoc_pick("wb", "荷", "荷枪实弹").unwrap();
        }
        s.record_assoc_pick("wb", "荷枪", "荷枪实弹").unwrap();
        s.record_assoc_pick("py", "荷", "荷叶").unwrap();
        let got: Vec<(String, u32)> = s
            .assoc_history("wb", "荷", 0)
            .unwrap()
            .into_iter()
            .map(|(w, r)| (w, r.count))
            .collect();
        assert_eq!(got, vec![("荷枪实弹".into(), 3), ("荷花".into(), 1)]);
        assert_eq!(s.assoc_history("wb", "荷", 1).unwrap().len(), 1);
        assert!(s.list_data_schemas().unwrap().contains(&"py".to_string()));
    }

    #[test]
    fn each_context_keeps_at_most_eight_and_never_drops_the_new_pick() {
        let s = store("cap_ctx");
        for i in 0..MAX_PER_CONTEXT {
            for _ in 0..3 {
                s.record_assoc_pick("wb", "我", &format!("我{i}")).unwrap();
            }
        }
        s.record_assoc_pick("wb", "我", "我新").unwrap();
        let got = s.assoc_history("wb", "我", 0).unwrap();
        assert_eq!(got.len(), MAX_PER_CONTEXT);
        assert!(
            got.iter().any(|(w, _)| w == "我新"),
            "刚选的即使次数最少也保留"
        );
        // 其他上文不受影响
        s.record_assoc_pick("wb", "你", "你好").unwrap();
        assert_eq!(s.assoc_history("wb", "你", 0).unwrap().len(), 1);
    }

    #[test]
    fn whole_table_is_pruned_by_least_recent_use() {
        let s = store("cap_total");
        // 批量导入超过上限的旧记录（导入本身不剪枝，下一次选择时剪）。
        let mut lines = String::new();
        for i in 0..=MAX_TOTAL {
            lines.push_str(&format!(
                "{{\"context\":\"c{i}\",\"text\":\"w{i}\",\"count\":1,\"last_used\":{i}}}\n"
            ));
        }
        s.import_assoc_history_jsonl("wb", &lines).unwrap();
        s.record_assoc_pick("wb", "新", "新词").unwrap();
        let left = s.export_assoc_history_jsonl("wb").unwrap().lines().count() as u64;
        assert!(left <= MAX_TOTAL * 9 / 10, "剪到 90% 以内，实际 {left}");
        assert_eq!(
            s.assoc_history("wb", "新", 0).unwrap().len(),
            1,
            "刚选的保留"
        );
        assert!(
            s.assoc_history("wb", "c0", 0).unwrap().is_empty(),
            "最久未用的先淘汰"
        );
        assert_eq!(
            s.assoc_history("wb", &format!("c{MAX_TOTAL}"), 0)
                .unwrap()
                .len(),
            1,
            "最近用过的保留"
        );
    }

    #[test]
    fn export_import_roundtrip_and_clear() {
        let a = store("a");
        a.record_assoc_pick("wb", "荷", "荷花").unwrap();
        a.record_assoc_pick("wb", "荷", "荷花").unwrap();
        let text = a.export_assoc_history_jsonl("wb").unwrap();
        let b = store("b");
        assert_eq!(b.import_assoc_history_jsonl("wb", &text).unwrap(), (1, 0));
        assert_eq!(b.assoc_history("wb", "荷", 0).unwrap()[0].1.count, 2);
        assert_eq!(b.clear_assoc_history("wb").unwrap(), 1);
        assert!(b.assoc_history("wb", "荷", 0).unwrap().is_empty());
    }
}
