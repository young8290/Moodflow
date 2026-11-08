//! Offline, source-preserving export. The original is never passed to redb.
//!
//! redb 2.6.3 requires a writable backend even when reading. Any recovery, table
//! initialization, or index backfill therefore runs only on an isolated copy.
use crate::backup::{BackupOptions, BackupSources, create_backup};
use anyhow::{Context, ensure};
use moodflow_store::Store;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub struct RecoveryExport {
    pub path: PathBuf,
    pub entries: Vec<String>,
    pub source_sha256: String,
}

/// Export every readable user-data domain into the existing backup format.
/// Stop the service first: a source locked by redb is rejected, never copied live.
/// Existing output files are never replaced, including aliases of the source.
/// A corrupt/unreadable copy fails the operation without publishing a partial zip.
pub fn export_backup(
    source: &Path,
    output: &Path,
    app_version: &str,
    platform: &str,
    created_at: &str,
) -> anyhow::Result<RecoveryExport> {
    ensure!(
        !output.try_exists()?,
        "恢复导出不会覆盖已有文件，请使用新的输出文件名"
    );
    let mut original = File::open(source).context("无法只读打开原库；原文件未修改")?;
    original
        .try_lock_shared()
        .context("原库正在使用或无法加只读锁；请退出输入法服务后重试")?;
    let before = original.metadata()?;
    ensure!(
        before.is_file() && before.len() > 0,
        "原库为空或不是普通文件，拒绝创建空白恢复包"
    );

    let work = tempfile::Builder::new()
        .prefix("moodflow-recovery-")
        .tempdir()?;
    let copy_path = work.path().join("snapshot.redb");
    let mut copy = File::create(&copy_path)?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = original.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copy.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    copy.sync_all()?;
    drop(copy);
    let after = original.metadata()?;
    ensure!(
        bytes == before.len()
            && after.len() == before.len()
            && after.modified()? == before.modified()?,
        "复制期间原库发生变化，请退出输入法服务后重试"
    );

    // This is intentionally the temporary copy, never `source`.
    let store = Store::open(&copy_path)
        .context("临时副本无法打开；原库未修改，请保留备份以便进一步恢复")?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staged = tempfile::Builder::new()
        .prefix(".moodflow-export-")
        .tempfile_in(parent)?
        .into_temp_path();
    let sources = BackupSources {
        user_config_file: None,
        compat_file: None,
        user_schemas_dir: None,
        user_schema_overrides_dir: None,
        user_themes_dir: None,
        state_file: None,
    };
    let backup = create_backup(
        &store,
        &sources,
        &staged,
        app_version,
        platform,
        created_at,
        &BackupOptions {
            include_stats: true,
            include_state: false,
        },
    )
    .context("副本导出失败；未发布不完整恢复包，原库未修改")?;
    drop(store);
    // Flush before the final no-clobber publication. Both files are on one volume.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&staged)?
        .sync_all()?;
    staged
        .persist_noclobber(output)
        .context("无法保存恢复包（可能目标已存在）；原库未修改")?;
    Ok(RecoveryExport {
        path: output.to_path_buf(),
        entries: backup.entries,
        source_sha256: format!("{:x}", digest.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::extract_entry;

    #[test]
    fn exports_all_schemas_and_phrases_without_changing_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("词库 source.redb");
        let output = dir.path().join("恢复 export.zip");
        {
            let store = Store::open(&source).unwrap();
            store
                .add_user_word("pinyin", "nihao", "你好", 100, 0)
                .unwrap();
            store
                .add_user_word("wubi", "abcd", "另一方案", 50, 0)
                .unwrap();
            store.record_freq("pinyin", "nihao", "你好").unwrap();
            store.add_phrase("hello", "全局短语", 1, 100).unwrap();
        }
        let before = std::fs::read(&source).unwrap();
        let report =
            export_backup(&source, &output, "test", "windows", "2026-11-09T00:00:00Z").unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), before);
        assert_eq!(
            report.source_sha256,
            format!("{:x}", Sha256::digest(&before))
        );
        for (path, expected) in [
            ("userdata/user_words/pinyin.wdict", "你好"),
            ("userdata/user_words/wubi.wdict", "另一方案"),
            ("userdata/phrases.wdict", "全局短语"),
        ] {
            let text = String::from_utf8(extract_entry(&output, path).unwrap()).unwrap();
            assert!(text.contains(expected));
            assert!(report.entries.iter().any(|entry| entry == path));
        }
        let restored = Store::open(dir.path().join("restored.redb")).unwrap();
        crate::backup::restore_backup(
            &output,
            &restored,
            &crate::backup::RestoreTargets {
                user_config_file: None,
                compat_file: None,
                user_schemas_dir: None,
                user_schema_overrides_dir: None,
                user_themes_dir: None,
                state_file: None,
            },
            crate::merge::Strategy::Merge,
            None,
        )
        .unwrap();
        assert_eq!(
            restored.get_user_words("pinyin", "nihao").unwrap()[0].text,
            "你好"
        );
        assert_eq!(
            restored.get_user_words("wubi", "abcd").unwrap()[0].text,
            "另一方案"
        );
        assert!(
            restored
                .list_phrases()
                .unwrap()
                .iter()
                .any(|p| p.text == "全局短语")
        );
        assert!(
            restored
                .get_freq("pinyin", "nihao", "你好")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn invalid_sources_leave_no_output_and_preserve_every_byte() {
        for content in [b"".as_slice(), b"not a redb database".as_slice()] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("broken.redb");
            let output = dir.path().join("export.zip");
            std::fs::write(&source, content).unwrap();
            assert!(export_backup(&source, &output, "test", "windows", "now").is_err());
            assert_eq!(std::fs::read(&source).unwrap(), content);
            assert!(!output.exists());
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn missing_sources_are_not_created_and_existing_outputs_are_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("absent.redb");
        let output = dir.path().join("export.zip");
        assert!(export_backup(&source, &output, "test", "windows", "now").is_err());
        assert!(!source.exists());
        assert!(!output.exists());
        std::fs::write(&output, b"keep existing output").unwrap();
        assert!(export_backup(&output, &output, "test", "windows", "now").is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"keep existing output");
    }

    #[test]
    fn live_database_is_rejected_instead_of_copying_a_torn_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("live.redb");
        let output = dir.path().join("export.zip");
        let _store = Store::open(&source).unwrap();
        assert!(export_backup(&source, &output, "test", "windows", "now").is_err());
        assert!(!output.exists());
    }
}
