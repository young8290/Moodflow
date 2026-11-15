//! Metadata-only capture catalog and batch revision checks.
use super::{Asset, MAX_TOTAL_BYTES, RETENTION_SECONDS};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_BATCH: usize = 40;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    All,
    Temporary,
    Kept,
    Library,
    Trash,
}
impl Scope {
    pub fn of(a: &Asset) -> Self {
        if a.trashed_at.is_some() {
            Self::Trash
        } else if a.permanent {
            Self::Library
        } else if a.kept {
            Self::Kept
        } else {
            Self::Temporary
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Newest,
    Oldest,
    Largest,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub search: String,
    pub scope: Scope,
    pub format: Option<String>,
    pub days: Option<u8>,
    pub sort: Sort,
    pub offset: usize,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct Row<'a> {
    pub asset: &'a Asset,
    pub revision: String,
    pub scope: Scope,
    pub in_use: bool,
}

#[derive(Clone, Copy, Default, Serialize)]
pub struct Usage {
    pub count: usize,
    pub bytes: u64,
}
impl Usage {
    fn add(&mut self, asset: &Asset) {
        self.count += 1;
        self.bytes += asset.bytes;
    }
}

#[derive(Default, Serialize)]
pub struct Storage {
    pub total: Usage,
    pub temporary: Usage,
    pub kept: Usage,
    pub library: Usage,
    pub trash: Usage,
    pub expired: Usage,
    pub png: Usage,
    pub gif: Usage,
    pub mp4: Usage,
    pub quota_bytes: u64,
}

#[derive(Serialize)]
pub struct Page<'a> {
    pub rows: Vec<Row<'a>>,
    pub storage: Storage,
    pub matched: Usage,
    pub offset: usize,
    pub limit: usize,
}

pub fn normalize_name(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(
        name.chars().count() <= 80 && !name.chars().any(char::is_control),
        "capture.catalog_invalid_name"
    );
    Ok(name.to_owned())
}

pub fn revision(asset: &Asset) -> String {
    // Includes retention metadata so a selection cannot overwrite a newer edit.
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(asset).expect("serializable capture asset"))
    )
}

pub fn query<'a>(
    assets: &'a [Asset],
    leased: &BTreeSet<String>,
    query: &Query,
    now: u64,
) -> Result<Page<'a>> {
    let search = query.search.trim().to_lowercase();
    let limit = query.limit.unwrap_or(24);
    ensure!(
        search.chars().count() <= 120
            && query
                .format
                .as_deref()
                .is_none_or(|f| ["png", "gif", "mp4"].contains(&f))
            && query.days.is_none_or(|days| [7, 30].contains(&days))
            && (1..=80).contains(&limit),
        "capture.catalog_invalid_query"
    );
    let terms = search.split_whitespace().collect::<Vec<_>>();
    let mut storage = Storage {
        quota_bytes: MAX_TOTAL_BYTES,
        ..Storage::default()
    };
    let mut matched = Usage::default();
    let mut rows = Vec::new();
    for asset in assets {
        let scope = Scope::of(asset);
        storage.total.add(asset);
        match scope {
            Scope::Temporary => storage.temporary.add(asset),
            Scope::Kept => storage.kept.add(asset),
            Scope::Library => storage.library.add(asset),
            Scope::Trash => storage.trash.add(asset),
            Scope::All => unreachable!(),
        }
        match asset.extension.as_str() {
            "png" => storage.png.add(asset),
            "gif" => storage.gif.add(asset),
            "mp4" => storage.mp4.add(asset),
            _ => anyhow::bail!("capture.invalid_asset"),
        }
        if scope == Scope::Temporary
            && now.saturating_sub(asset.retention_start()) >= RETENTION_SECONDS
            && !leased.contains(&asset.id)
        {
            storage.expired.add(asset);
        }
        let scope_matches = match query.scope {
            Scope::All => scope != Scope::Trash,
            specific => specific == scope,
        };
        if !scope_matches
            || query.format.as_ref().is_some_and(|f| f != &asset.extension)
            || query.days.is_some_and(|days| {
                // In the trash, "recent" describes when the item was removed.
                now.saturating_sub(asset.trashed_at.unwrap_or(asset.created_at))
                    > u64::from(days) * 86_400
            })
        {
            continue;
        }
        if !terms.is_empty() {
            let name = asset.name.to_lowercase();
            if !terms.iter().all(|term| {
                name.contains(term) || asset.id.contains(term) || asset.extension.contains(term)
            }) {
                continue;
            }
        }
        matched.add(asset);
        rows.push(asset);
    }
    let activity = |a: &Asset| a.trashed_at.unwrap_or(a.created_at);
    rows.sort_unstable_by(|a, b| {
        match query.sort {
            Sort::Newest => activity(b).cmp(&activity(a)),
            Sort::Oldest => activity(a).cmp(&activity(b)),
            Sort::Largest => b
                .bytes
                .cmp(&a.bytes)
                .then_with(|| activity(b).cmp(&activity(a))),
        }
        .then_with(|| a.id.cmp(&b.id))
    });
    let last = matched.count.saturating_sub(1) / limit * limit;
    let offset = (query.offset / limit * limit).min(last);
    Ok(Page {
        rows: rows
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|asset| Row {
                asset,
                revision: revision(asset),
                scope: Scope::of(asset),
                in_use: leased.contains(&asset.id),
            })
            .collect(),
        storage,
        matched,
        offset,
        limit,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Keep,
    Release,
    Trash,
    Restore,
    Purge,
    Export,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
    pub revision: String,
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        crate::canonical_id(&self.id)
            .map_err(|_| anyhow::anyhow!("capture.catalog_invalid_selection"))?;
        ensure!(
            self.revision.len() == 64 && self.revision.bytes().all(|b| b.is_ascii_hexdigit()),
            "capture.catalog_invalid_selection"
        );
        Ok(())
    }
}
pub fn validate_batch(items: &[Selection]) -> Result<()> {
    ensure!(
        (1..=MAX_BATCH).contains(&items.len()),
        "capture.catalog_invalid_selection"
    );
    let mut ids = BTreeSet::new();
    for item in items {
        item.validate()?;
        ensure!(ids.insert(&item.id), "capture.catalog_invalid_selection");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ready,
    Done,
    Missing,
    Stale,
    InUse,
    WrongState,
    ChangedFile,
    Failed,
    DestinationExists,
    ExportLimit,
    Paused,
}

pub fn review(
    asset: Option<&Asset>,
    item: &Selection,
    operation: Operation,
    blocked: bool,
) -> Outcome {
    let Some(asset) = asset else {
        return Outcome::Missing;
    };
    if revision(asset) != item.revision {
        return Outcome::Stale;
    }
    use Operation::*;
    let scope = Scope::of(asset);
    let eligible = match operation {
        Keep | Trash => scope == Scope::Temporary,
        Release => scope == Scope::Kept,
        Restore | Purge => scope == Scope::Trash,
        Export => scope != Scope::Trash,
    };
    if !eligible {
        return Outcome::WrongState;
    }
    if blocked && matches!(operation, Trash | Purge) {
        return Outcome::InUse;
    }
    Outcome::Ready
}
