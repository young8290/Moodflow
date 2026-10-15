//! Hub 的运行日志：照旧写标准错误，同时在内存里留最近 [`KEEP`] 行，供“导出运行信息”（FR-OPS-04）。
//!
//! 只记连接状态、计数、错误原因这类不含用户数据的内容（NFR-LOG）；不落盘，退出即清空。

use std::collections::VecDeque;
use std::sync::Mutex;

/// 内存里留的行数。
pub const KEEP: usize = 500;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// 记一行（带本地时间）。用 [`hlog!`](crate::hlog) 调用。
pub fn note(msg: &str) {
    eprintln!("{msg}");
    let line = format!("{} {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
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

/// 像 `eprintln!` 一样用，另外记进内存里的运行日志。
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
}
