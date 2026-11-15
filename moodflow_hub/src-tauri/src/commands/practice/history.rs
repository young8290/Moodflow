//! Local numeric practice history. Source and input strings are never accepted here.
use super::{MAX_CHARS, MAX_MS, PracticeScore};
use crate::error::UiError;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_RECORDS: usize = 200;
const FILE_LIMIT: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PracticeCategory {
    Chinese,
    English,
    Mixed,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PracticeMode {
    Passage,
    Timed30,
    Timed60,
    Timed120,
}
impl PracticeMode {
    fn limit(self) -> f64 {
        match self {
            Self::Passage => f64::from(MAX_MS),
            Self::Timed30 => 30_000.0,
            Self::Timed60 => 60_000.0,
            Self::Timed120 => 120_000.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct PracticeRecordInput {
    pub id: String,
    pub category: PracticeCategory,
    pub mode: PracticeMode,
    pub score: PracticeScore,
}

impl PracticeRecordInput {
    fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        let score = &self.score;
        let target = usize::try_from(score.target_chars)?;
        let typed = usize::try_from(score.typed_chars)?;
        let distance = usize::try_from(score.distance)?;
        let longest = target.max(typed);
        ensure!(
            (1..=MAX_CHARS).contains(&target)
                && (1..=MAX_CHARS).contains(&typed)
                && distance >= target.abs_diff(typed)
                && distance <= longest
                && (1000.0..=self.mode.limit()).contains(&score.elapsed_ms)
                && (0.0..=1.0).contains(&score.process_accuracy),
            "practice.invalid_record"
        );
        let accuracy = 1.0 - distance as f64 / longest as f64;
        let speed = target.saturating_sub(distance) as f64 * 60_000.0 / score.elapsed_ms;
        ensure!(
            (score.final_accuracy - accuracy).abs() < 1e-9
                && (score.effective_cpm - speed).abs() < 1e-9,
            "practice.invalid_record"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PracticeRecord {
    pub date: String,
    pub data: PracticeRecordInput,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct History {
    version: u8,
    records: Vec<PracticeRecord>,
}

struct Store {
    root: PathBuf,
    _lock: File,
    history: History,
}
impl Store {
    fn open(root: &Path) -> Result<Self> {
        crate::private_fs::ensure(root)?;
        let lock = root.join(".history.lock");
        regular_if_present(&lock)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock)?;
        lock.lock()?;
        let file = root.join("history.json");
        let history = if regular_if_present(&file)? {
            let mut buffer = Vec::new();
            File::open(file)?
                .take(FILE_LIMIT + 1)
                .read_to_end(&mut buffer)?;
            ensure!(buffer.len() as u64 <= FILE_LIMIT, "practice.history_size");
            serde_json::from_slice::<History>(&buffer)?
        } else {
            History {
                version: 1,
                records: Vec::new(),
            }
        };
        ensure!(
            history.version == 1 && history.records.len() <= MAX_RECORDS,
            "practice.history_format"
        );
        let mut ids = BTreeSet::new();
        for record in &history.records {
            record.data.validate()?;
            chrono::DateTime::parse_from_rfc3339(&record.date)?;
            ensure!(ids.insert(&record.data.id), "practice.history_duplicate");
        }
        Ok(Self {
            root: root.to_owned(),
            _lock: lock,
            history,
        })
    }

    fn save(&mut self, input: PracticeRecordInput) -> Result<PracticeRecord> {
        input.validate()?;
        if let Some(existing) = self.history.records.iter().find(|r| r.data.id == input.id) {
            let old = &existing.data;
            let a = &old.score;
            let b = &input.score;
            ensure!(
                old.category == input.category
                    && old.mode == input.mode
                    && a.elapsed_ms == b.elapsed_ms
                    && a.target_chars == b.target_chars
                    && a.typed_chars == b.typed_chars
                    && a.distance == b.distance
                    && (a.process_accuracy - b.process_accuracy).abs() < 1e-9,
                "practice.invalid_record"
            );
            return Ok(existing.clone());
        }
        let record = PracticeRecord {
            date: chrono::Utc::now().to_rfc3339(),
            data: input,
        };
        self.history.records.insert(0, record.clone());
        self.history.records.truncate(MAX_RECORDS);
        self.persist()?;
        Ok(record)
    }

    fn delete(&mut self, id: Option<&str>) -> Result<()> {
        if let Some(id) = id {
            valid_id(id)?;
            let before = self.history.records.len();
            self.history.records.retain(|r| r.data.id != id);
            if before == self.history.records.len() {
                return Ok(());
            }
        } else {
            self.history.records.clear();
        }
        self.persist()
    }

    fn persist(&self) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        let bytes = serde_json::to_vec(&self.history)?;
        ensure!(bytes.len() as u64 <= FILE_LIMIT, "practice.history_size");
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(self.root.join("history.json"))?;
        Ok(())
    }
}

fn valid_id(id: &str) -> Result<()> {
    ensure!(
        id.len() == 36 && uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id),
        "practice.invalid_record"
    );
    Ok(())
}

fn regular_if_present(path: &Path) -> Result<bool> {
    use std::io::ErrorKind;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "practice.invalid_file"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "practice.invalid_file"
        );
    }
    Ok(true)
}

async fn with_store<T: Send + 'static>(
    window: tauri::WebviewWindow,
    action: impl FnOnce(Store) -> Result<T> + Send + 'static,
) -> Result<T, UiError> {
    if window.label() != "dashboard" || crate::sim::demo() {
        return Err(UiError::new(
            "practice.history_unavailable",
            "dashboard.practice.history_unavailable",
        ));
    }
    let error = || UiError::new("practice.history_error", "dashboard.practice.history_error");
    tauri::async_runtime::spawn_blocking(move || {
        let root = crate::paths::tool_data_dir()?.join("practice");
        action(Store::open(&root)?)
    })
    .await
    .map_err(|_| error())?
    .map_err(|_| error())
}

#[tauri::command]
#[specta::specta]
pub async fn practice_history_list(
    window: tauri::WebviewWindow,
) -> Result<Vec<PracticeRecord>, UiError> {
    with_store(window, |store| Ok(store.history.records)).await
}

#[tauri::command]
#[specta::specta]
pub async fn practice_history_save(
    window: tauri::WebviewWindow,
    input: PracticeRecordInput,
) -> Result<PracticeRecord, UiError> {
    with_store(window, move |mut store| store.save(input)).await
}

/// No ID means an explicitly confirmed clear-all action.
#[tauri::command]
#[specta::specta]
pub async fn practice_history_delete(
    window: tauri::WebviewWindow,
    id: Option<String>,
) -> Result<(), UiError> {
    with_store(window, move |mut store| store.delete(id.as_deref())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PracticeRecordInput {
        PracticeRecordInput {
            id: uuid::Uuid::new_v4().to_string(),
            category: PracticeCategory::Custom,
            mode: PracticeMode::Timed30,
            score: super::super::score(
                "练习",
                &[super::super::PracticeEvent {
                    t_ms: 0.0,
                    text: "练习".into(),
                }],
                30_000.0,
            ),
        }
    }

    #[test]
    fn retry_and_clear_are_safe_and_only_numeric_results_persist() {
        let dir = tempfile::tempdir().unwrap();
        let input = fixture();
        let mut store = Store::open(dir.path()).unwrap();
        let one = store.save(input.clone()).unwrap();
        drop(store);
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(store.save(input.clone()).unwrap().date, one.date);
        let mut changed = input;
        changed.category = PracticeCategory::Chinese;
        assert!(store.save(changed).is_err());
        let payload = fs::read_to_string(dir.path().join("history.json")).unwrap();
        assert!(!payload.contains("练习"));
        store.delete(None).unwrap();
        drop(store);
        assert!(Store::open(dir.path()).unwrap().history.records.is_empty());
    }

    #[test]
    fn invalid_scores_or_corrupt_files_do_not_replace_existing_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = fixture();
        input.score.effective_cpm = 999.0;
        assert!(Store::open(dir.path()).unwrap().save(input).is_err());
        let path = dir.path().join("history.json");
        assert!(!path.exists());
        fs::write(&path, b"broken").unwrap();
        assert!(Store::open(dir.path()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"broken");
    }
}
