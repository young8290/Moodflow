//! 正在运行的程序名（设置页“不感知的程序”“只感知这些程序”“勿扰应用”的候选，07 FR-SET-04/05）。
//! 只取进程的可执行文件名，不取路径、窗口标题或命令行；只在本机给设置页用，不落库、不出网。
//! Windows 用 ToolHelp 快照；其他平台读 `/proc`（只为开发时能看到效果）。

/// 系统进程，列出来没有意义（用户也不会想把它们加进名单）。比较时忽略大小写。
const SYSTEM: &[&str] = &[
    "[system process]",
    "system",
    "registry",
    "secure system",
    "memory compression",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "sihost.exe",
    "taskhostw.exe",
    "ctfmon.exe",
    "dllhost.exe",
    "wmiprvse.exe",
    "searchindexer.exe",
    "spoolsv.exe",
    "audiodg.exe",
    "xinqing_hub.exe",
];

/// 去掉系统进程、去重（忽略大小写，保留第一次出现的写法）、按名字排序。
pub fn tidy(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = names
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && !SYSTEM.contains(&n.to_ascii_lowercase().as_str()))
        .filter(|n| seen.insert(n.to_ascii_lowercase()))
        .collect();
    out.sort_by_key(|n| n.to_ascii_lowercase());
    out
}

pub fn running_apps() -> Vec<String> {
    tidy(raw())
}

#[cfg(windows)]
fn raw() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    let mut e = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: 快照句柄只在本函数里用，结束时关闭；PROCESSENTRY32W 已按文档填好 dwSize
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push(String::from_utf16_lossy(&e.szExeFile[..len]));
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn raw() -> Vec<String> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_system_processes_and_duplicates() {
        let names = ["svchost.exe", "WeChat.exe", "Code.exe", "wechat.exe", "", "  Zoom.exe ", "System"]
            .map(String::from);
        assert_eq!(tidy(names), ["Code.exe", "WeChat.exe", "Zoom.exe"]);
    }

    #[test]
    fn lists_something_on_this_machine() {
        // 至少能看到测试进程自己
        assert!(!running_apps().is_empty());
    }
}
