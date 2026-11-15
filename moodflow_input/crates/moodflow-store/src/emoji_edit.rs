//! Bounded editor operations for the existing emoji_user table. Writes compare the
//! full previous row in the same redb transaction as the update.
use super::{EmojiUserEntry, normalized_symbol, union_symbols};
use crate::Store;
use crate::store::EMOJI_USER;
use anyhow::{Context, Result, ensure};
use redb::{ReadableTable, ReadableTableMetadata};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_MAPPING_ROWS: usize = 512;
pub const MAX_MAPPING_TEXT: usize = 768 * 1024;
pub const MAX_ADDED: usize = 8;
const MAX_HIDDEN: usize = 16;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmojiEdit {
    pub before: EmojiUserEntry,
    pub after: EmojiUserEntry,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmojiImportPolicy {
    Skip,
    Merge,
    Replace,
}

#[derive(Serialize)]
pub struct EmojiImportPlan {
    pub edits: Vec<EmojiEdit>,
    pub new_count: usize,
    pub conflict_count: usize,
    pub skipped_count: usize,
    pub unchanged_count: usize,
}

impl EmojiUserEntry {
    pub fn empty(word: &str) -> Self {
        Self {
            word: word.to_owned(),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.hidden.is_empty()
    }

    pub fn validate_edit(&self) -> Result<()> {
        validate_token(&self.word, 32)?;
        for (list, limit) in [(&self.added, MAX_ADDED), (&self.hidden, MAX_HIDDEN)] {
            ensure!(list.len() <= limit, "mapping.limit");
            let mut seen = BTreeSet::new();
            for symbol in list {
                ensure!(valid_mapping_symbol(symbol), "mapping.invalid");
                let key = normalized_symbol(symbol);
                ensure!(seen.insert(key), "mapping.duplicate");
            }
        }
        ensure!(
            self.added
                .iter()
                .all(|item| normalized_symbol(item) != normalized_symbol(&self.word)),
            "mapping.same_as_host"
        );
        Ok(())
    }
}

/// Ignore unsupported legacy values at runtime while leaving the backup data intact.
pub fn valid_mapping_symbol(text: &str) -> bool {
    validate_token(text, 16).is_ok() && text.chars().any(|ch| ch != '\u{fe0f}')
}

fn validate_token(text: &str, max: usize) -> Result<()> {
    ensure!(
        !text.is_empty()
            && text.len() <= max * 4
            && text.chars().count() <= max
            && !text.chars().any(|c| c.is_control() || c.is_whitespace()),
        "mapping.invalid"
    );
    Ok(())
}

fn decode(word: &str, bytes: &[u8]) -> Result<EmojiUserEntry> {
    let mut entry = serde_json::from_slice::<EmojiUserEntry>(bytes).context("mapping.damaged")?;
    entry.word = word.to_owned();
    Ok(entry)
}

impl Store {
    pub fn emoji_user_entry(&self, word: &str) -> Result<EmojiUserEntry> {
        validate_token(word, 32)?;
        self.with_db(|db| {
            let txn = db.begin_read()?;
            let table = txn.open_table(EMOJI_USER)?;
            table.get(word)?.map_or_else(
                || Ok(EmojiUserEntry::empty(word)),
                |value| decode(word, value.value()),
            )
        })
    }

    /// Compute edits from one read snapshot. The returned changes can be reviewed
    /// before the user applies them, at which point each prior row is checked again.
    pub fn plan_emoji_import(
        &self,
        text: &str,
        policy: EmojiImportPolicy,
    ) -> Result<EmojiImportPlan> {
        ensure!(text.len() <= MAX_MAPPING_TEXT, "mapping.limit");
        let mut entries = Vec::new();
        let mut words = BTreeSet::new();
        let normalized = moodflow_utils::text::normalize_input(text);
        for (i, raw) in normalized.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            ensure!(entries.len() < MAX_MAPPING_ROWS, "mapping.limit");
            let entry: EmojiUserEntry = serde_json::from_str(line)
                .map_err(|_| anyhow::anyhow!("mapping.import_line:{}", i + 1))?;
            if entry.validate_edit().is_err()
                || entry.is_empty()
                || !words.insert(entry.word.clone())
            {
                anyhow::bail!("mapping.import_line:{}", i + 1);
            }
            entries.push(entry);
        }
        ensure!(!entries.is_empty(), "mapping.empty");
        self.with_db(|db| {
            let txn = db.begin_read()?;
            let table = txn.open_table(EMOJI_USER)?;
            let mut plan = EmojiImportPlan {
                edits: vec![],
                new_count: 0,
                conflict_count: 0,
                skipped_count: 0,
                unchanged_count: 0,
            };
            for mut after in entries {
                let before = match table.get(after.word.as_str())? {
                    Some(value) => decode(&after.word, value.value())?,
                    None => EmojiUserEntry::empty(&after.word),
                };
                if before == after {
                    plan.unchanged_count += 1;
                    continue;
                }
                if before.is_empty() {
                    plan.new_count += 1;
                } else {
                    plan.conflict_count += 1;
                    match policy {
                        EmojiImportPolicy::Skip => {
                            plan.skipped_count += 1;
                            continue;
                        }
                        EmojiImportPolicy::Merge => {
                            let mut merged = before.clone();
                            union_symbols(&mut merged.added, &after.added);
                            union_symbols(&mut merged.hidden, &after.hidden);
                            after = merged;
                        }
                        EmojiImportPolicy::Replace => {}
                    }
                }
                after.validate_edit()?;
                if before != after {
                    plan.edits.push(EmojiEdit { before, after });
                } else {
                    plan.unchanged_count += 1;
                }
            }
            Ok(plan)
        })
    }

    pub fn apply_emoji_edits(&self, edits: &[EmojiEdit]) -> Result<()> {
        ensure!(
            (1..=MAX_MAPPING_ROWS).contains(&edits.len()),
            "mapping.limit"
        );
        let mut words = BTreeSet::new();
        for edit in edits {
            edit.after.validate_edit()?;
            ensure!(
                edit.before.word == edit.after.word && words.insert(edit.after.word.as_str()),
                "mapping.invalid"
            );
        }
        self.with_db(|db| {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(EMOJI_USER)?;
                let existing = table.len()?;
                let mut inserted = 0;
                let mut removed = 0;
                for edit in edits {
                    let word = edit.after.word.as_str();
                    let (present, current) = match table.get(word)? {
                        Some(value) => (true, decode(word, value.value())?),
                        None => (false, EmojiUserEntry::empty(word)),
                    };
                    ensure!(edit.before == current, "mapping.stale");
                    if edit.after.is_empty() {
                        if present {
                            table.remove(word)?;
                            removed += 1;
                        }
                    } else {
                        let bytes = serde_json::to_vec(&edit.after)?;
                        table.insert(word, bytes.as_slice())?;
                        inserted += u64::from(!present);
                    }
                }
                // Legacy rows remain editable even when an earlier backup exceeded this bound.
                let final_count = existing + inserted - removed;
                ensure!(
                    final_count <= MAX_MAPPING_ROWS as u64 || final_count <= existing,
                    "mapping.limit"
                );
            }
            txn.commit()?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::store;
    use super::*;

    #[test]
    fn stale_second_row_rolls_back_every_change_in_the_import() {
        let store = store("mapping-edit");
        let text = "{\"word\":\"开心\",\"added\":[\"🙂\"]}\r{\"word\":\"完成\",\"added\":[\"✔️\"]}";
        let plan = store
            .plan_emoji_import(text, EmojiImportPolicy::Merge)
            .unwrap();
        assert_eq!(plan.new_count, 2);
        store.hide_emoji_for_word("完成", "✔").unwrap();
        assert!(
            store
                .apply_emoji_edits(&plan.edits)
                .unwrap_err()
                .to_string()
                .contains("mapping.stale")
        );
        assert!(store.emoji_user_entry("开心").unwrap().is_empty());
        assert_eq!(store.emoji_user_entry("完成").unwrap().hidden, ["✔"]);

        let plan = store
            .plan_emoji_import(text, EmojiImportPolicy::Merge)
            .unwrap();
        store.apply_emoji_edits(&plan.edits).unwrap();
        let complete = store.emoji_user_entry("完成").unwrap();
        assert_eq!(complete.added, ["✔️"]);
        assert_eq!(complete.hidden, ["✔"]);
        assert_eq!(store.emoji_user_entry("开心").unwrap().added, ["🙂"]);
    }

    #[test]
    fn invalid_lines_and_variation_duplicates_are_reported_without_writes() {
        let store = store("mapping-invalid");
        let input = "{\"word\":\"开心\",\"added\":[\"🙂\"]}\r\n{\"word\":\"完成\",\"added\":[\"✔\",\"✔️\"]}";
        assert_eq!(
            store
                .plan_emoji_import(input, EmojiImportPolicy::Replace)
                .err()
                .unwrap()
                .to_string(),
            "mapping.import_line:2"
        );
        assert!(store.list_emoji_user_entries().unwrap().is_empty());
    }
}
