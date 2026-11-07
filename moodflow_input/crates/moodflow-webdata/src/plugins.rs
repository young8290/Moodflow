//! `plugins.*`：插件管理页的数据 RPC（docs/design/plugin-system.md §11 P5，产品书 07 FR-SET-12）。
//!
//! 插件装在输入法用户目录 `plugins/<id>/<版本>/`，只有核心知道这个目录在哪（便携、自定义数据目录），
//! 所以装、卸、列表都在核心里做；启用列表与安全模式是普通配置项（`plugins.enabled` /
//! `plugins.safe_mode`），由调用方经 `config.setItems` 改，这里不碰配置。
//!
//! | 方法 | 参数 | 结果 |
//! |---|---|---|
//! | `plugins.list` | — | `{available, safeMode, plugins:[…]}`，每项见 [`list_json`] |
//! | `plugins.inspect` | `{path}` | 包的说明与能力；`added` 是这次需要用户同意的能力 |
//! | `plugins.install` | `{path, accept:[能力]}` | 重新检查一遍，`added` 必须都在 `accept` 里才装 |
//! | `plugins.uninstall` | `{id, deleteData}` | 删掉全部版本，可选同时删私有数据 |
//! | `plugins.reenable` | `{id}` | 重新启用被自动停用的插件 |
//!
//! 现在还没有签名市场，装的都是本地文件（侧载），结果里 `source` 恒为 `"local"`，界面标“未审核”。
//! 能力用字符串表示：`storage`、`secrets`、`clipboard_read`、`context_recent`、`notify`、
//! `network:<主机>`。

use std::path::Path;

use moodflow_plugin_api::{Capability, Kind, Manifest};
use moodflow_plugin_registry::PluginStatus;
use serde_json::{Value, json};

/// 能力的字符串形式（界面按它查中文说明）。
pub(crate) fn cap_str(c: &Capability) -> String {
    match c {
        Capability::Storage => "storage".into(),
        Capability::Secrets => "secrets".into(),
        Capability::Network(h) => format!("network:{h}"),
        Capability::ClipboardRead => "clipboard_read".into(),
        Capability::ContextRecent => "context_recent".into(),
        Capability::Notify => "notify".into(),
    }
}

fn kind_json(m: &Manifest) -> Value {
    match &m.kind {
        Kind::Wasm { .. } => json!({ "type": "wasm" }),
        Kind::Content { content } => json!({ "type": "content", "content": content }),
    }
}

/// 插件声明接入的扩展点（给界面写“它会做什么”）。
fn contributes_json(m: &Manifest) -> Value {
    let c = &m.contributes;
    json!({
        "candidates": c.candidate_provider.is_some(),
        "candidateFilter": c.candidate_filter.is_some(),
        "association": c.assoc_source.is_some(),
        "commitTransform": c.commit_transform.is_some(),
        "cmdbarFunctions": c.cmdbar_functions,
        "bufferAction": c.buffer_action.as_ref().map(|b| b.label.clone()),
        "hasSettings": c.settings.is_some(),
    })
}

/// 一个包或已装插件的说明部分（列表与安装预览共用）。
fn manifest_json(m: &Manifest) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "version": m.version,
        "description": m.description,
        "license": m.license,
        "kind": kind_json(m),
        "platforms": m.platforms,
        "capabilities": m.capabilities.list().iter().map(cap_str).collect::<Vec<_>>(),
        "contributes": contributes_json(m),
    })
}

/// 已装插件列表：已装目录 × 启用列表 × 加载状态。
///
/// 状态取值：`disabled`（不在启用列表）、`safe_mode`（启用了但安全模式开着）、`loaded`、
/// `auto_disabled`（连续出错被停用，可重新启用）、`load_failed`（启用了却没加载上：
/// 清单不兼容、平台不支持、入口文件坏了等，核心加载时已弹过提示）。
pub(crate) fn list_json(
    plugins_dir: Option<&Path>,
    enabled: &[String],
    safe_mode: bool,
    status: &[PluginStatus],
) -> Value {
    let Some(dir) = plugins_dir else {
        return json!({ "available": false, "safeMode": safe_mode, "plugins": [] });
    };
    let plugins: Vec<Value> = moodflow_market::installed(dir)
        .into_iter()
        .map(|(id, version)| {
            let loaded = status.iter().find(|s| s.id == id);
            let on = enabled.contains(&id);
            let state = match (on, safe_mode, loaded) {
                (false, _, _) => "disabled",
                (true, true, _) => "safe_mode",
                (true, false, Some(s)) if s.disabled => "auto_disabled",
                (true, false, Some(_)) => "loaded",
                (true, false, None) => "load_failed",
            };
            let mut v = match moodflow_market::installed_manifest(dir, &id, &version) {
                Some(m) => manifest_json(&m),
                // 清单读不出来也要列出来，好让用户能卸载
                None => json!({
                    "id": id, "name": id, "version": version, "description": "",
                    "license": "", "kind": null, "platforms": [], "capabilities": [],
                    "contributes": null,
                }),
            };
            v["enabled"] = json!(on);
            v["state"] = json!(state);
            v["source"] = json!("local");
            v
        })
        .collect();
    json!({ "available": true, "safeMode": safe_mode, "plugins": plugins })
}

/// 检查一个本地包。`installed` 的那一版清单用来算升级时新增的能力。
pub(crate) fn inspect(
    bytes: &[u8],
    plugins_dir: &Path,
    host_version: &str,
) -> anyhow::Result<(moodflow_market::InstallPlan, Option<String>)> {
    let mut upgrade_from = None;
    let plan = moodflow_market::inspect_local_package(bytes, host_version, |id| {
        let version = moodflow_market::installed(plugins_dir)
            .into_iter()
            .find(|(i, _)| i == id)
            .map(|(_, v)| v)?;
        let m = moodflow_market::installed_manifest(plugins_dir, id, &version);
        upgrade_from = Some(version);
        m
    })
    .map_err(market_error)?;
    Ok((plan, upgrade_from))
}

/// 安装预览：包的说明 + 这次要同意的能力 + 升级自哪个版本。
pub(crate) fn preview_json(
    plan: &moodflow_market::InstallPlan,
    upgrade_from: Option<&str>,
) -> Value {
    let mut v = manifest_json(&plan.manifest);
    v["added"] = json!(
        plan.added_capabilities
            .iter()
            .map(cap_str)
            .collect::<Vec<_>>()
    );
    v["upgradeFrom"] = json!(upgrade_from);
    v["source"] = json!("local");
    v
}

/// 用户在预览里同意过的能力必须覆盖这次实际新增的能力；文件在预览后被换掉、
/// 或升级时多要了能力，都拒绝安装，让界面重新预览。
pub(crate) fn check_consent(
    plan: &moodflow_market::InstallPlan,
    accept: &[String],
) -> anyhow::Result<()> {
    let missing: Vec<String> = plan
        .added_capabilities
        .iter()
        .map(cap_str)
        .filter(|c| !accept.contains(c))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "plugins.consent_mismatch: 这些能力还没有经过同意：{}",
            missing.join("、")
        )
    }
}

/// 插件 id 能不能当目录名用（卸载、删私有数据前检查）。
pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty() && !id.contains(['/', '\\']) && !id.contains("..")
}

fn market_error(e: moodflow_market::MarketError) -> anyhow::Error {
    use moodflow_market::MarketError as E;
    let code = match &e {
        E::TooLarge => "plugins.too_large",
        E::Zip(_) | E::TooManyEntries => "plugins.bad_package",
        E::UnsafePath(_) | E::Symlink(_) => "plugins.unsafe_package",
        E::MissingManifest | E::Manifest(_) => "plugins.bad_manifest",
        E::MissingEntry(_) => "plugins.missing_entry",
        E::ProtectedFile(_) => "plugins.protected_file",
        E::Io(_) => "plugins.io",
        _ => "plugins.bad_package",
    };
    anyhow::anyhow!("{code}: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const HOST: &str = "0.130.0";

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (n, b) in entries {
                z.start_file(*n, zip::write::SimpleFileOptions::default())
                    .unwrap();
                z.write_all(b).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    fn pkg(id: &str, version: &str, caps: &str) -> Vec<u8> {
        let manifest = format!(
            r#"
id = "{id}"
name = "表情 {version}"
version = "{version}"
sdk = 1
min_host_version = "0.120.0"
type = "wasm"
entry = "plugin.wasm"
platforms = ["windows", "android"]
license = "MIT"
description = "打字时给出表情候选"
[contributes]
candidate_provider = {{}}
cmdbar_functions = ["emoji"]
[capabilities]
{caps}
"#
        );
        zip_of(&[
            ("manifest.toml", manifest.as_bytes()),
            ("plugin.wasm", b"\0asm"),
        ])
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("mf-webdata-plugins-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn status(id: &str, disabled: bool) -> PluginStatus {
        PluginStatus {
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            disabled,
            candidate_provider: true,
            commit_transform: false,
            cmdbar_functions: vec![],
        }
    }

    #[test]
    fn first_install_asks_for_every_capability_and_upgrade_only_for_new_ones() {
        let dir = tmp("consent");
        let (plan, from) = inspect(
            &pkg("dev.example.emoji", "1.0.0", "storage = true"),
            &dir,
            HOST,
        )
        .unwrap();
        assert_eq!(from, None);
        let preview = preview_json(&plan, from.as_deref());
        assert_eq!(preview["added"], json!(["storage"]));
        assert_eq!(preview["contributes"]["cmdbarFunctions"], json!(["emoji"]));
        assert!(check_consent(&plan, &[]).is_err());
        check_consent(&plan, &["storage".into()]).unwrap();
        moodflow_market::install(&plan, &dir).unwrap();

        // 升级：只多要了读剪贴板（逐键插件拿不到联网能力，清单校验就会拒绝）
        let (plan, from) = inspect(
            &pkg(
                "dev.example.emoji",
                "1.1.0",
                "storage = true\nclipboard_read = true",
            ),
            &dir,
            HOST,
        )
        .unwrap();
        assert_eq!(from.as_deref(), Some("1.0.0"));
        assert_eq!(
            preview_json(&plan, from.as_deref())["added"],
            json!(["clipboard_read"])
        );
        let err = check_consent(&plan, &["storage".into()])
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("plugins.consent_mismatch"), "{err}");

        // 能力没增加的升级：不用再同意
        let (plan, _) = inspect(&pkg("dev.example.emoji", "1.0.1", ""), &dir, HOST).unwrap();
        check_consent(&plan, &[]).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_joins_installed_enabled_safe_mode_and_runtime_state() {
        let dir = tmp("list");
        for id in ["dev.a.one", "dev.b.two", "dev.c.three", "dev.d.four"] {
            let (plan, _) = inspect(&pkg(id, "1.0.0", ""), &dir, HOST).unwrap();
            moodflow_market::install(&plan, &dir).unwrap();
        }
        let enabled: Vec<String> = ["dev.a.one", "dev.b.two", "dev.c.three"]
            .map(String::from)
            .into();
        let st = [status("dev.a.one", false), status("dev.b.two", true)];
        let v = list_json(Some(&dir), &enabled, false, &st);
        let state = |id: &str| {
            v["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == id)
                .map(|p| p["state"].as_str().unwrap().to_owned())
                .unwrap()
        };
        assert_eq!(v["available"], json!(true));
        assert_eq!(state("dev.a.one"), "loaded");
        assert_eq!(state("dev.b.two"), "auto_disabled");
        assert_eq!(state("dev.c.three"), "load_failed");
        assert_eq!(state("dev.d.four"), "disabled");
        let safe = list_json(Some(&dir), &enabled, true, &[]);
        assert_eq!(safe["plugins"][0]["state"], json!("safe_mode"));
        assert_eq!(safe["safeMode"], json!(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_manifest_still_lists_so_it_can_be_removed() {
        let dir = tmp("broken");
        std::fs::create_dir_all(dir.join("dev.x.broken").join("1.0.0")).unwrap();
        std::fs::write(dir.join("dev.x.broken/1.0.0/manifest.toml"), "not = [toml").unwrap();
        let v = list_json(Some(&dir), &[], false, &[]);
        assert_eq!(v["plugins"][0]["id"], json!("dev.x.broken"));
        assert_eq!(v["plugins"][0]["state"], json!("disabled"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_user_dir_reports_unavailable() {
        assert_eq!(list_json(None, &[], false, &[])["available"], json!(false));
    }

    #[test]
    fn bad_packages_map_to_stable_error_codes() {
        let dir = tmp("errors");
        let err = inspect(b"nope", &dir, HOST).unwrap_err().to_string();
        assert!(err.starts_with("plugins.bad_package"), "{err}");
        let bad_manifest = zip_of(&[("manifest.toml", b"x")]);
        let err = inspect(&bad_manifest, &dir, HOST).unwrap_err().to_string();
        assert!(err.starts_with("plugins.bad_manifest"), "{err}");
    }

    #[test]
    fn ids_with_path_parts_are_refused() {
        assert!(safe_id("dev.example.emoji"));
        for bad in ["", "..", "a/b", "a\\b", "../x"] {
            assert!(!safe_id(bad), "{bad}");
        }
    }
}
