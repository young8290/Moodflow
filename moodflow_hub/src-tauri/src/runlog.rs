//! Hub 的运行日志：照旧写标准错误，同时写进日志文件（NFR-LOG-02/03），并在内存里留最近 [`KEEP`] 行，
//! 供“导出运行信息”（FR-OPS-04）。
//!
//! 日志文件在 `%LOCALAPPDATA%\Moodflow\logs\moodflow_hub.N.log`（[`crate::paths::log_dir`]）：每次启动（setup 里）滚动一次，
//! `.0` 是本次，单个文件写满 [`MAX_FILE_BYTES`] 再滚一次，最多留 [`MAX_FILES`] 个。只记连接状态、计数、
//! 错误原因这类不含用户数据的内容（NFR-LOG-01）。

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 内存里留的行数。
pub const KEEP: usize = 500;
/// 单个日志文件的上限（NFR-LOG-02）。
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// 最多留几个日志文件（NFR-LOG-02）。
pub const MAX_FILES: usize = 10;
const PREFIX: &str = "moodflow_hub";

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: Mutex<Option<LogFile>> = Mutex::new(None);

struct LogFile {
    dir: PathBuf,
    file: File,
    written: u64,
}

fn path(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("{PREFIX}.{n}.log"))
}

/// 把 `.0`…`.8` 往后挪一位（最老的 `.9` 删掉），再新建空的 `.0`。
fn roll(dir: &Path) -> std::io::Result<File> {
    fs::create_dir_all(dir)?;
    let _ = fs::remove_file(path(dir, MAX_FILES - 1));
    for n in (0..MAX_FILES - 1).rev() {
        let from = path(dir, n);
        if from.exists() {
            fs::rename(&from, path(dir, n + 1))?;
        }
    }
    File::create(path(dir, 0))
}

/// 删除全部数据时删掉 Hub 的日志文件（FR-DAT-04）。须在 [`open_file`] 之前调用。
pub fn remove_files(dir: &Path) {
    for n in 0..MAX_FILES {
        let _ = fs::remove_file(path(dir, n));
    }
}

/// 启动时调一次：滚动旧文件，开新文件，把之前已记在内存里的行补写进去。打不开就只写标准错误和内存。
pub fn open_file(dir: &Path) {
    let file = match roll(dir) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("打开日志文件失败：{e}");
            return;
        }
    };
    let mut log = LogFile {
        dir: dir.to_path_buf(),
        file,
        written: 0,
    };
    for line in lines() {
        log.write(&line);
    }
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(log);
}

impl LogFile {
    fn write(&mut self, line: &str) {
        if self.written >= MAX_FILE_BYTES {
            match roll(&self.dir) {
                Ok(f) => {
                    self.file = f;
                    self.written = 0;
                }
                Err(_) => return,
            }
        }
        if writeln!(self.file, "{line}").is_ok() {
            self.written += line.len() as u64 + 1;
        }
    }
}

/// 记一行（带本地时间）。用 [`hlog!`](crate::hlog) 调用。
pub fn note(msg: &str) {
    eprintln!("{msg}");
    let line = format!("{} {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        f.write(&line);
    }
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if lines.len() == KEEP {
        lines.pop_front();
    }
    lines.push_back(line);
}

/// 目前留着的全部行，最早的在前。
pub fn lines() -> Vec<String> {
    LINES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// 像 `eprintln!` 一样用，另外记进日志文件和内存里的运行日志。
#[macro_export]
macro_rules! hlog {
    ($($t:tt)*) => {
        $crate::runlog::note(&format!($($t)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_latest_lines() {
        for i in 0..KEEP + 3 {
            crate::hlog!("第 {i} 行");
        }
        let l = lines();
        assert_eq!(l.len(), KEEP);
        assert!(l[0].ends_with("第 3 行"), "{}", l[0]);
        assert!(l[KEEP - 1].ends_with(&format!("第 {} 行", KEEP + 2)));
    }

    #[test]
    fn rolls_on_open_and_keeps_at_most_ten_files() {
        let dir = std::env::temp_dir().join(format!("mf-runlog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for i in 0..MAX_FILES + 2 {
            let mut f = roll(&dir).unwrap();
            writeln!(f, "启动 {i}").unwrap();
        }
        let count = fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, MAX_FILES);
        let newest = fs::read_to_string(path(&dir, 0)).unwrap();
        assert_eq!(newest.trim(), format!("启动 {}", MAX_FILES + 1));
        let oldest = fs::read_to_string(path(&dir, MAX_FILES - 1)).unwrap();
        assert_eq!(oldest.trim(), "启动 2");
        remove_files(&dir);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
