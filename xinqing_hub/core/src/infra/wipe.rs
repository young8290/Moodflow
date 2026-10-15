//! 删除全部数据（09 FR-DAT-04，ADR 0037）。
//!
//! 运行中的 Hub 有十几个服务各自拿着数据库连接，就地删文件既删不干净（Windows 上打开的文件删不掉），
//! 也可能被还没退出的写线程重新建出来。所以分两步：
//! 1. 设置页确认后，外壳调用 [`request`] 在数据目录写一个“待删除”标记，然后重启 Hub；
//! 2. 下次启动、任何东西打开数据库之前，外壳调用 [`apply_pending`]：按 [`targets`] 删掉文件，最后删掉标记。
//!
//! 删的是 Hub 数据库（真实库与演示库，连同 `-wal` / `-shm`）、损坏时留下的备份（`*.corrupt-*`）、
//! 覆盖导入前的自动备份（`*.bak-*`）；勾选时连 AI 密钥文件一起删。用户目录里的模板覆盖（`templates/`）
//! 是配置不是数据，保留。输入法自己的词库不在这个目录，不受影响。新库由正常启动流程重建，
//! 里面没有同意记录，Hub 因此重新进入首次引导。

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 待删除标记的文件名
pub const MARKER: &str = "wipe-pending.json";

/// 数据库文件名：真实库与演示库（与 `src-tauri` 的 `state::DB_FILE`、`domain::demo::DB_FILE` 一致，测试核对）
pub const DB_FILES: &[&str] = &["xinqing.db", "xinqing_demo.db"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WipeRequest {
    /// 连 AI 密钥一起删
    pub include_secrets: bool,
    /// 用户确认的时刻（Unix 毫秒），只记在标记里方便排查
    pub requested_at: i64,
}

/// 写下待删除标记（先写临时文件再改名，不会留下半个标记）。
pub fn request(data_dir: &Path, req: &WipeRequest) -> io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let tmp = data_dir.join(format!("{MARKER}.part"));
    std::fs::write(&tmp, serde_json::to_vec(req).map_err(io::Error::other)?)?;
    std::fs::rename(tmp, data_dir.join(MARKER))
}

/// 有没有待处理的删除请求；标记读不懂时按“删，但不删密钥”处理（宁可多删数据，也不悄悄留下）。
pub fn pending(data_dir: &Path) -> Option<WipeRequest> {
    let bytes = std::fs::read(data_dir.join(MARKER)).ok()?;
    Some(serde_json::from_slice(&bytes).unwrap_or(WipeRequest {
        include_secrets: false,
        requested_at: 0,
    }))
}

/// 要删的文件（只列出确实存在的）。`secrets_file` 是 AI 密钥文件名（外壳的 `secrets::SECRETS_FILE`）。
pub fn targets(data_dir: &Path, include_secrets: bool, secrets_file: &str) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return Ok(out);
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_db = DB_FILES.iter().any(|db| {
            name == *db
                || name == format!("{db}-wal")
                || name == format!("{db}-shm")
                || name.starts_with(&format!("{db}.corrupt-"))
                || name.starts_with(&format!("{db}.bak-"))
        });
        if is_db || (include_secrets && name == secrets_file) {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

/// 删了哪些文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WipeReport {
    pub removed: Vec<PathBuf>,
}

/// 启动时调用：有标记就删，删完再删标记；没有标记什么也不做。任何一个文件删不掉都返回错误、保留标记，
/// 下次启动接着删（外壳此时不该继续打开数据库）。
pub fn apply_pending(data_dir: &Path, secrets_file: &str) -> io::Result<Option<WipeReport>> {
    let Some(req) = pending(data_dir) else {
        return Ok(None);
    };
    let removed = targets(data_dir, req.include_secrets, secrets_file)?;
    for path in &removed {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    std::fs::remove_file(data_dir.join(MARKER))?;
    Ok(Some(WipeReport { removed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRETS: &str = "secrets.bin";

    /// 测试用临时目录，用完删掉（core 不依赖 tempfile）
    struct Dir(PathBuf);
    impl Dir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir_with(files: &[&str]) -> Dir {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = Dir(std::env::temp_dir().join(format!("xq-wipe-{}-{n}", std::process::id())));
        std::fs::create_dir_all(dir.path().join("templates")).unwrap();
        for f in files {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        std::fs::write(dir.path().join("templates/comfort.toml"), b"x").unwrap();
        dir
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    const ALL: &[&str] = &[
        "xinqing.db",
        "xinqing.db-wal",
        "xinqing.db-shm",
        "xinqing.db.corrupt-20261001-101010",
        "xinqing.db.bak-20261002-090000",
        "xinqing_demo.db",
        "secrets.bin",
        "other.txt",
    ];

    #[test]
    fn targets_cover_databases_and_backups_but_not_templates_or_other_files() {
        let dir = dir_with(ALL);
        let t = names(&targets(dir.path(), false, SECRETS).unwrap());
        assert_eq!(
            t,
            [
                "xinqing.db",
                "xinqing.db-shm",
                "xinqing.db-wal",
                "xinqing.db.bak-20261002-090000",
                "xinqing.db.corrupt-20261001-101010",
                "xinqing_demo.db",
            ]
        );
        let with_keys = names(&targets(dir.path(), true, SECRETS).unwrap());
        assert!(with_keys.contains(&"secrets.bin".to_string()));
    }

    #[test]
    fn nothing_happens_without_a_marker() {
        let dir = dir_with(ALL);
        assert_eq!(apply_pending(dir.path(), SECRETS).unwrap(), None);
        assert!(dir.path().join("xinqing.db").exists());
    }

    #[test]
    fn pending_wipe_removes_files_then_the_marker() {
        let dir = dir_with(ALL);
        request(dir.path(), &WipeRequest { include_secrets: false, requested_at: 1 }).unwrap();
        assert!(pending(dir.path()).is_some());
        let report = apply_pending(dir.path(), SECRETS).unwrap().unwrap();
        assert_eq!(report.removed.len(), 6);
        assert!(!dir.path().join("xinqing.db").exists());
        assert!(!dir.path().join("xinqing_demo.db").exists());
        assert!(dir.path().join(SECRETS).exists(), "没勾选时保留密钥");
        assert!(dir.path().join("other.txt").exists());
        assert!(dir.path().join("templates/comfort.toml").exists());
        assert!(!dir.path().join(MARKER).exists());
        // 第二次启动什么也不做
        assert_eq!(apply_pending(dir.path(), SECRETS).unwrap(), None);
    }

    #[test]
    fn secrets_are_removed_when_asked() {
        let dir = dir_with(ALL);
        request(dir.path(), &WipeRequest { include_secrets: true, requested_at: 1 }).unwrap();
        apply_pending(dir.path(), SECRETS).unwrap();
        assert!(!dir.path().join(SECRETS).exists());
    }

    #[test]
    fn unreadable_marker_still_wipes_data_but_keeps_secrets() {
        let dir = dir_with(ALL);
        std::fs::write(dir.path().join(MARKER), b"not json").unwrap();
        apply_pending(dir.path(), SECRETS).unwrap();
        assert!(!dir.path().join("xinqing.db").exists());
        assert!(dir.path().join(SECRETS).exists());
    }
}
