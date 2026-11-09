//! Windows tools consume the existing conversion/import and chord contracts.
//! Source archives are rechecked after preview; editor writes use revisions.
use super::{WebDataRpc, str_param};
use anyhow::{Context, ensure};
use moodflow_chord::Keymap;
use moodflow_config::Config;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: usize = 1024 * 1024;
const BUILTIN: &str = include_str!("../../moodflow-mobile/data/chord-builtin.json");
const DRAFT: &str = "chord_keymap.draft.json";
const ACTIVE: &str = "chord_keymap.json";

fn diagnostic(d: &moodflow_transfer::rime_import::Diagnostic) -> String {
    use moodflow_transfer::rime_import::Diagnostic::*;
    match d {
        IgnoredFile { path } => format!("忽略私有状态或无关文件：{path}"),
        UnsupportedComponent { path, .. } => format!("此组件未转换，请在导入后检查：{path}"),
        LuaIgnored { schema } => format!("{schema}：Lua 逻辑未导入"),
        SpellerAlgebraIgnored { schema } => format!("{schema}：拼写运算未转换，按全拼处理"),
        FiltersIgnored { schema } => format!("{schema}：简繁、Emoji 等过滤器未转换"),
        AssumedMaxCodeLength { schema } => format!("{schema}：未声明码长，使用四码"),
        MissingDictionary { schema, name } => format!("{schema}：缺少词库 {name}"),
        SchemeSkipped { schema, .. } => format!("{schema}：不支持此翻译器，已跳过方案"),
        IdRenamed { from, to } => format!("方案标识已从 {from} 改为 {to}"),
    }
}

fn read(path: &Path) -> anyhow::Result<Option<String>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut text = String::new();
    file.take(LIMIT as u64 + 1).read_to_string(&mut text)?;
    ensure!(text.len() <= LIMIT, "方案文件超过 1 MiB");
    Ok(Some(text))
}

fn revision(text: Option<&str>) -> String {
    text.map(|s| format!("{:x}", Sha256::digest(s.as_bytes())))
        .unwrap_or_else(|| "missing".into())
}

fn atomic_write(path: &Path, text: &str) -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("无用户目录")?)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn user_dir() -> anyhow::Result<PathBuf> {
    Config::user_config_dir().context("无用户配置目录")
}

fn save_profile(dir: &Path, params: &Value) -> anyhow::Result<()> {
    let text = str_param(params, "text")?;
    ensure!(text.len() <= LIMIT, "方案文件超过 1 MiB");
    let map = Keymap::from_json(text).context("并击方案格式或键位不合法")?;
    let apply = params
        .get("apply")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if apply {
        map.validate_for_apply()
            .context("空映射或不完整方案不能应用")?;
        ensure!(
            map.left_keys
                .iter()
                .chain(&map.right_keys)
                .all(|k| moodflow_keys::keymap::key_name_to_vk_with_letters(k).is_some()),
            "存在不支持的物理键名"
        );
    }
    fs::create_dir_all(dir)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("chord-editor.lock"))?;
    lock.lock()?;
    let draft = dir.join(DRAFT);
    ensure!(
        revision(read(&draft)?.as_deref()) == str_param(params, "revision")?,
        "方案已被其他窗口修改，请重新载入后合并"
    );
    if apply {
        ensure!(
            revision(read(&dir.join(ACTIVE))?.as_deref()) == str_param(params, "activeRevision")?,
            "正在使用的方案已被修改，请重新载入后合并"
        );
    }
    atomic_write(&draft, text)?;
    if apply {
        atomic_write(&dir.join(ACTIVE), text)?;
    }
    Ok(())
}

pub(super) fn dispatch<H: WebDataRpc + ?Sized>(
    host: &H,
    method: &str,
    params: &Value,
) -> anyhow::Result<Value> {
    match method {
        "chord.readFile" => {
            let text = read(Path::new(str_param(params, "path")?))?.context("所选文件不存在")?;
            Keymap::from_json(&text).context("并击方案格式不合法")?;
            Ok(json!({ "text": text }))
        }
        "chord.exportFile" => {
            let text = str_param(params, "text")?;
            ensure!(text.len() <= LIMIT, "方案文件超过 1 MiB");
            Keymap::from_json(text).context("并击方案格式不合法")?;
            let path = Path::new(str_param(params, "path")?);
            let mut staged =
                tempfile::NamedTempFile::new_in(path.parent().context("请选择导出目录")?)?;
            staged.write_all(text.as_bytes())?;
            staged.as_file().sync_all()?;
            staged.persist_noclobber(path).map_err(|e| e.error)?;
            Ok(json!({ "exported": true }))
        }
        "chord.builtin" => Ok(json!({ "text": BUILTIN })),
        "chord.get" => {
            let dir = user_dir()?;
            let draft = read(&dir.join(DRAFT))?;
            let active = read(&dir.join(ACTIVE))?;
            let (enabled, configured) = host.chord_settings();
            let configured_path = if Path::new(&configured).is_absolute() {
                PathBuf::from(&configured)
            } else {
                dir.join(&configured)
            };
            let text = draft
                .clone()
                .or(read(&configured_path)?)
                .unwrap_or_else(|| BUILTIN.to_owned());
            Ok(
                json!({ "text": text, "revision": revision(draft.as_deref()),
                "activeRevision": revision(active.as_deref()), "enabled": enabled }),
            )
        }
        "chord.save" => {
            save_profile(&user_dir()?, params)?;
            if params
                .get("apply")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                Config::set_user_value(&["chord", "keymap"], toml::Value::String(ACTIVE.into()))?;
                Config::set_user_value(&["chord", "enabled"], toml::Value::Boolean(true))?;
                host.reload_user_config();
                host.reload_chord_profile();
            }
            dispatch(host, "chord.get", &json!({}))
        }
        "scheme.rimePreview" | "scheme.rimeImport" => {
            use moodflow_transfer::rime_import::{convert, read_zip, write_wpkg};
            let (files, sha, pre) = read_zip(Path::new(str_param(params, "path")?))?;
            let converted = convert(&files, sha, pre)?;
            let temp = tempfile::tempdir()?;
            if method == "scheme.rimePreview" {
                let mut schemes = Vec::new();
                for scheme in &converted.schemes {
                    let path = temp.path().join(format!("{}.wpkg", scheme.id));
                    write_wpkg(scheme, &path)?;
                    let preview = host.web_scheme_preview_import(&json!({ "path": path }))?;
                    schemes.push(json!({ "id": scheme.id, "name": scheme.name,
                        "engine": match scheme.engine { moodflow_transfer::rime_import::EngineKind::Pinyin => "拼音", _ => "码表" }, "preview": preview }));
                }
                return Ok(
                    json!({ "sha256": converted.source_sha256, "schemes": schemes,
                    "diagnostics": converted.diagnostics.iter().map(diagnostic).collect::<Vec<_>>() }),
                );
            }
            ensure!(
                str_param(params, "sha256")? == converted.source_sha256,
                "源包在预览后已变化，请重新预览"
            );
            let ids = params
                .get("ids")
                .and_then(Value::as_array)
                .context("请选择方案")?;
            ensure!(
                !ids.is_empty()
                    && ids.iter().all(|id| id
                        .as_str()
                        .is_some_and(|id| converted.schemes.iter().any(|s| s.id == id))),
                "所选方案不在预览中"
            );
            let mut results = Vec::new();
            for scheme in &converted.schemes {
                if !ids.iter().any(|id| id.as_str() == Some(&scheme.id)) {
                    continue;
                }
                let path = temp.path().join(format!("{}.wpkg", scheme.id));
                write_wpkg(scheme, &path)?;
                let result = host.web_scheme_import_package(&json!({ "path": path,
                    "strategy": params.get("strategy").and_then(Value::as_str).unwrap_or("merge") }));
                results.push(match result {
                    Ok(value) => json!({ "id": scheme.id, "result": value }),
                    Err(_) => json!({ "id": scheme.id, "error": "方案导入失败，现有文件已保留" }),
                });
            }
            Ok(json!({ "results": results }))
        }
        _ => anyhow::bail!("unknown tools method"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_can_be_applied_and_drafts_cannot_overwrite_concurrent_edits() {
        Keymap::from_json(BUILTIN)
            .unwrap()
            .validate_for_apply()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let request = json!({"text": BUILTIN, "revision":"missing", "activeRevision":"missing", "apply":true});
        save_profile(dir.path(), &request).unwrap();
        assert!(save_profile(dir.path(), &request).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join(ACTIVE)).unwrap(),
            BUILTIN
        );
        let mut empty = serde_json::from_str::<Value>(BUILTIN).unwrap();
        empty["mappings"] = json!([]);
        let request = json!({"text":empty.to_string(), "revision":revision(Some(BUILTIN)),
            "activeRevision":revision(Some(BUILTIN)), "apply":true});
        assert!(save_profile(dir.path(), &request).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join(ACTIVE)).unwrap(),
            BUILTIN
        );
    }
}
