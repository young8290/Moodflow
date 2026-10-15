//! 导出运行信息（11 FR-OPS-04）：把排查问题要看的东西打成一个 zip，由用户自己决定发给谁。
//!
//! 包里只有版本、系统、显示器、连接状态、AI 接口的统计与最近的出网记录（只有接口、模型、字段名、耗时、状态）、
//! 用户模板覆盖了哪些文件、Hub 最近的运行日志；**不含数据库、密钥、AI 地址、设置值和任何用户写的文字**。
//! 导出前界面先列出这些文件（[`FILES`]）。界面上叫“运行信息”：“诊断”在禁用词表里（DS-COPY-03，ADR 0039）。

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::json;
use specta::Type;
use xinqing_hub_core::domain::status::StatusSnapshot;
use xinqing_hub_core::infra::gateway::{GatewayHealth, NetLogEntry};

use crate::commands::ai::{GatewayMetricsView, UsageRow};

/// 包里的文件，按导出顺序。`what` 对应界面文案键 `about.file_<what>`。
pub const FILES: [(&str, &str); 7] = [
    ("README.txt", "readme"),
    ("about.json", "about"),
    ("displays.json", "displays"),
    ("status.json", "status"),
    ("ai.json", "ai"),
    ("templates.txt", "templates"),
    ("hub-log.txt", "log"),
];

/// 出网记录最多带这么多条（最近的在前）。
pub const NET_LOG_ROWS: usize = 100;

/// 导出前给界面看的一项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DiagFile {
    pub name: String,
    pub what: String,
}

pub fn preview() -> Vec<DiagFile> {
    FILES
        .iter()
        .map(|(name, what)| DiagFile {
            name: (*name).into(),
            what: (*what).into(),
        })
        .collect()
}

/// 一块显示器（逻辑坐标以物理像素计，`scale` 即系统缩放 1.0 / 1.25 / 1.5 …）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Display {
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
}

/// 打包用到的全部输入，由外壳收集；拆出来是为了不起 Tauri 也能测。
pub struct Inputs {
    pub generated: String,
    pub hub_version: String,
    pub debug_build: bool,
    pub demo: bool,
    pub os: String,
    pub arch: String,
    pub displays: Vec<Display>,
    pub status: StatusSnapshot,
    pub health: GatewayHealth,
    pub usage: Vec<UsageRow>,
    pub metrics: GatewayMetricsView,
    pub net_log: Vec<NetLogEntry>,
    pub template_overrides: Vec<String>,
    pub log: Vec<String>,
}

/// 生成包里每个文件的内容，顺序与 [`FILES`] 一致。
pub fn build(i: &Inputs) -> Vec<(&'static str, String)> {
    let pretty = |v: serde_json::Value| serde_json::to_string_pretty(&v).unwrap_or_default();
    let readme = format!(
        "心晴运行信息，生成于 {}。\n\
         用来排查问题，不含数据库、密钥、AI 服务地址、设置内容和任何你写的文字。\n\
         包含：{}\n",
        i.generated,
        FILES.map(|(n, _)| n).join("、")
    );
    let about = json!({
        "hub_version": i.hub_version,
        "build": if i.debug_build { "debug" } else { "release" },
        "demo": i.demo,
        "os": i.os,
        "arch": i.arch,
    });
    // 只带连接与降级状态，不带当前的情绪状态
    let s = &i.status;
    let status = json!({
        "connected": s.connected,
        "offline": s.offline,
        "paused": s.paused,
        "baseline_progress": s.baseline_progress,
    });
    let ai = json!({
        "health": i.health,
        "usage_today": i.usage,
        "metrics": i.metrics,
        "net_log": i.net_log.iter().take(NET_LOG_ROWS).collect::<Vec<_>>(),
    });
    let templates = if i.template_overrides.is_empty() {
        "（没有覆盖出厂模板）\n".to_string()
    } else {
        i.template_overrides.join("\n") + "\n"
    };
    vec![
        ("README.txt", readme),
        ("about.json", pretty(about)),
        ("displays.json", pretty(json!(i.displays))),
        ("status.json", pretty(status)),
        ("ai.json", pretty(ai)),
        ("templates.txt", templates),
        ("hub-log.txt", i.log.join("\n") + "\n"),
    ]
}

/// 写 zip（deflate）。返回写入的文件数。
pub fn write_zip(path: &Path, files: &[(&str, String)]) -> std::io::Result<u32> {
    let f = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(f);
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in files {
        zip.start_file(*name, opt).map_err(std::io::Error::other)?;
        zip.write_all(body.as_bytes())?;
    }
    zip.finish().map_err(std::io::Error::other)?;
    Ok(files.len() as u32)
}

/// 用户模板目录里覆盖了哪些文件（只列文件名）。
pub fn template_overrides(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            generated: "2026-10-15 10:00".into(),
            hub_version: "0.1.0".into(),
            debug_build: false,
            demo: false,
            os: "Windows 11 (26100) [64-bit]".into(),
            arch: "x86_64".into(),
            displays: vec![Display {
                name: Some("\\\\.\\DISPLAY1".into()),
                width: 2560,
                height: 1440,
                x: 0,
                y: 0,
                scale: 1.5,
            }],
            status: StatusSnapshot {
                connected: true,
                ..StatusSnapshot::default()
            },
            health: GatewayHealth {
                jev: true,
                llm: false,
            },
            usage: Vec::new(),
            metrics: GatewayMetricsView {
                apis: Vec::new(),
                models: Vec::new(),
            },
            net_log: (0..150)
                .map(|i| NetLogEntry {
                    ts: i,
                    api: "jev".into(),
                    model: None,
                    fields: vec!["kpm".into()],
                    latency_ms: Some(40),
                    status: "ok".into(),
                    tokens_in: None,
                    tokens_out: None,
                })
                .collect(),
            template_overrides: vec!["comfort_templates.toml".into()],
            log: vec!["2026-10-15 09:59:00 XQP 已连接（核心 0.1.0）".into()],
        }
    }

    #[test]
    fn package_matches_the_announced_file_list() {
        let files = build(&inputs());
        assert_eq!(
            files.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            FILES.map(|(n, _)| n)
        );
        let get = |n: &str| &files.iter().find(|(f, _)| *f == n).unwrap().1;
        // 情绪状态不进包；出网记录截到最近 100 条
        let status: serde_json::Value = serde_json::from_str(get("status.json")).unwrap();
        assert!(status.get("state").is_none() && status.get("weather").is_none());
        assert_eq!(status["connected"], true);
        let ai: serde_json::Value = serde_json::from_str(get("ai.json")).unwrap();
        assert_eq!(ai["net_log"].as_array().unwrap().len(), NET_LOG_ROWS);
        assert_eq!(ai["health"]["llm"], false);
        let displays: serde_json::Value = serde_json::from_str(get("displays.json")).unwrap();
        assert_eq!(displays[0]["scale"], 1.5);
        assert!(get("hub-log.txt").contains("XQP 已连接"));
        assert!(get("templates.txt").contains("comfort_templates.toml"));
    }

    #[test]
    fn zip_holds_every_file() {
        let dir = std::env::temp_dir().join(format!("xq-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.zip");
        let files = build(&inputs());
        assert_eq!(write_zip(&path, &files).unwrap(), FILES.len() as u32);
        let mut z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut readme = String::new();
        z.by_name("README.txt")
            .unwrap()
            .read_to_string(&mut readme)
            .unwrap();
        assert!(readme.contains("不含数据库、密钥"));
        assert_eq!(z.len(), FILES.len());
        std::fs::write(dir.join("ui_copy.toml"), "x").unwrap();
        assert_eq!(template_overrides(&dir), ["run.zip", "ui_copy.toml"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
