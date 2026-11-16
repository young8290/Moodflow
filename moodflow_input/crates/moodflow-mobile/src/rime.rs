//! Rime 导入的移动端门面。文件路径来自宿主私有缓存。
use std::path::Path;

use moodflow_config::Config;
use moodflow_transfer::rime_import::EngineKind;
use moodflow_transfer::rime_install;

pub use moodflow_transfer::rime_install::MAX_SOURCE_BYTES;

pub struct Scheme {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub dictionaries: usize,
    pub blocked: bool,
}

pub struct Preview {
    pub revision: String,
    pub schemes: Vec<Scheme>,
    pub warnings: Vec<String>,
}

fn unavailable(data_dir: &Path, id: &str) -> bool {
    let name = format!("{id}.schema.toml");
    Config::custom_hides_schema(id)
        || Config::resource_layers_with(Some(data_dir))
            .iter()
            .any(|base| base.join("schemas").join(&name).exists())
}

pub fn preview(data_dir: &Path, archive: &Path) -> Result<Preview, String> {
    let conversion = rime_install::read_archive(archive).map_err(|e| e.to_string())?;
    let schemes = conversion
        .schemes
        .into_iter()
        .map(|scheme| {
            let blocked = unavailable(data_dir, &scheme.id) || scheme.id.len() > 128;
            Scheme {
                id: scheme.id,
                name: scheme.name,
                kind: match scheme.engine {
                    EngineKind::Pinyin => "全拼",
                    EngineKind::Codetable => "码表",
                },
                dictionaries: scheme.files.len().saturating_sub(1),
                blocked,
            }
        })
        .collect();
    Ok(Preview {
        revision: conversion.source_sha256,
        schemes,
        warnings: conversion
            .diagnostics
            .iter()
            .map(|d| rime_install::diagnostic_text(d).chars().take(500).collect())
            .collect(),
    })
}

pub fn install(data_dir: &Path, archive: &Path, id: &str, revision: &str) -> Result<(), String> {
    if !id.starts_with("rime_")
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("无效的方案标识".into());
    }
    let conversion = rime_install::read_archive(archive).map_err(|e| e.to_string())?;
    if conversion.source_sha256 != revision {
        return Err("文件已变化，请重新选取 ZIP 并确认预览".into());
    }
    if unavailable(data_dir, id) {
        return Err("该方案标识不可用，或已有同名方案".into());
    }
    let user = Config::user_config_dir().ok_or("无法找到用户配置目录")?;
    rime_install::install(&conversion, id, &user.join("schemas")).map_err(|e| format!("{e:#}"))
}
