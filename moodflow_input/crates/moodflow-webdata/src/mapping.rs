//! Local mapping editor. Workbench and backups share emoji_user; preview never writes.
use moodflow_coordinator::web_host::WebDataHost;
use moodflow_store::emoji_user::{
    EmojiEdit, EmojiImportPolicy, EmojiUserEntry, MAX_MAPPING_ROWS, MAX_MAPPING_TEXT,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn dispatch(
    host: &(impl WebDataHost + ?Sized),
    method: &str,
    params: &Value,
) -> anyhow::Result<Value> {
    anyhow::ensure!(host.tools_allowed(), "privacy_blocked");
    let store = host
        .user_store()
        .ok_or_else(|| anyhow::anyhow!("mapping.unavailable"))?;
    match method {
        "mapping.list" => {
            #[derive(Deserialize)]
            #[serde(default, deny_unknown_fields)]
            struct Query {
                search: String,
                offset: usize,
                kind: String,
            }
            impl Default for Query {
                fn default() -> Self {
                    Self {
                        search: String::new(),
                        offset: 0,
                        kind: "all".into(),
                    }
                }
            }
            let q: Query = request(params)?;
            anyhow::ensure!(
                q.search.len() <= 128 && ["all", "added", "hidden"].contains(&q.kind.as_str()),
                "mapping.invalid"
            );
            let search = q.search.trim().to_lowercase();
            let mut rows = store.list_emoji_user_entries()?;
            rows.retain(|row| {
                !row.is_empty()
                    && (q.kind != "added" || !row.added.is_empty())
                    && (q.kind != "hidden" || !row.hidden.is_empty())
                    && (search.is_empty()
                        || row.word.to_lowercase().contains(&search)
                        || row
                            .added
                            .iter()
                            .chain(&row.hidden)
                            .any(|s| s.to_lowercase().contains(&search)))
            });
            let total = rows.len();
            const PAGE: usize = 24;
            let offset = (q.offset / PAGE * PAGE).min(total.saturating_sub(1) / PAGE * PAGE);
            Ok(
                json!({"rows": rows.into_iter().skip(offset).take(PAGE).collect::<Vec<_>>(),
                "total": total, "offset": offset, "limit": PAGE}),
            )
        }
        "mapping.get" => {
            let row = store.emoji_user_entry(super::str_param(params, "word")?)?;
            let preview = host.emoji_editor_preview(&row);
            Ok(json!({"row":row,"preview":preview}))
        }
        "mapping.preview" => {
            let row: EmojiUserEntry = request(params)?;
            row.validate_edit()?;
            Ok(host.emoji_editor_preview(&row))
        }
        "mapping.apply" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Apply {
                edits: Vec<EmojiEdit>,
            }
            let apply: Apply = request(params)?;
            anyhow::ensure!(host.tools_allowed(), "privacy_blocked");
            store.apply_emoji_edits(&apply.edits)?;
            host.reload_emoji_hidden();
            Ok(json!({"applied":apply.edits.len()}))
        }
        "mapping.importPreview" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Import {
                text: String,
                policy: EmojiImportPolicy,
            }
            let import: Import = request(params)?;
            Ok(serde_json::to_value(
                store.plan_emoji_import(&import.text, import.policy)?,
            )?)
        }
        "mapping.export" => {
            anyhow::ensure!(
                params.as_object().is_some_and(serde_json::Map::is_empty),
                "mapping.invalid"
            );
            let rows = store.list_emoji_user_entries()?;
            anyhow::ensure!(rows.len() <= MAX_MAPPING_ROWS, "mapping.limit");
            let mut text = String::new();
            for row in rows.into_iter().filter(|row| !row.is_empty()) {
                row.validate_edit()?;
                text.push_str(&serde_json::to_string(&row)?);
                text.push('\n');
                anyhow::ensure!(text.len() <= MAX_MAPPING_TEXT, "mapping.limit");
            }
            anyhow::ensure!(!text.is_empty(), "mapping.empty");
            Ok(json!({"text":text}))
        }
        _ => anyhow::bail!("mapping.invalid"),
    }
}

fn request<T: serde::de::DeserializeOwned>(params: &Value) -> anyhow::Result<T> {
    serde_json::from_value(params.clone()).map_err(|_| anyhow::anyhow!("mapping.invalid"))
}
