//! 输入法设置命令（10 第 5.1 节“输入法设置”组，07 FR-SET-02，ADR 0016）：透传 moodflow-rpc 的
//! `config.schema` / `config.get` / `config.getDefaults` / `config.setItems`。管道读写是阻塞的，放进 `spawn_blocking`，不占主线程。
//!
//! 配置值是任意 JSON。specta 把 `serde_json::Value` 当递归类型内联导出时会栈溢出，所以值字段在
//! TypeScript 里标成 `unknown`（`#[specta(type = Unknown)]`），由前端按 `ime_schema` 的类型自己判断。

use moodflow_hub_core::infra::imeconf::{
    ImeField, ImeItem, ImeRpc, ImeRpcError, ImeSetResult, PluginList, PluginPreview,
    plugin_error_code,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use specta_typescript::Unknown;

use crate::error::UiError;
use crate::state::AppState;
use tauri::State;

/// `ime_config_set` 的一项：点分键名与新值。
#[derive(Debug, Clone, Deserialize, Type)]
pub struct ImeItemInput {
    pub key: String,
    #[specta(type = Unknown)]
    pub value: Value,
}

/// `ime_config_get` 的返回：整份合并后的配置，界面按键名的点分路径取值。
#[derive(Debug, Clone, Serialize, Type)]
pub struct ImeConfig {
    #[specta(type = Unknown)]
    pub values: Value,
}

fn rpc() -> ImeRpc {
    // 开发版 Hub 连开发版核心（管道名带 `_dev`），与 MFP 管道的规则一致
    ImeRpc::from_env(cfg!(debug_assertions))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce(ImeRpc) -> Result<T, ImeRpcError> + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(move || f(rpc()))
        .await
        .map_err(|e| UiError::internal("ime.join", e))?
        .map_err(UiError::from)
}

/// 全部已登记的输入法配置键与类型，设置页据此生成表单。
#[tauri::command]
#[specta::specta]
pub async fn ime_schema() -> Result<Vec<ImeField>, UiError> {
    blocking(|r| r.schema()).await
}

/// 整份合并后的输入法配置。
#[tauri::command]
#[specta::specta]
pub async fn ime_config_get() -> Result<ImeConfig, UiError> {
    blocking(|r| r.config().map(|values| ImeConfig { values })).await
}

/// 输入法配置的出厂默认（整份，moodflow-rpc `config.getDefaults`）：设置页“恢复默认”按键名取值，和当前值不同的
/// 用 `ime_config_set` 写回（07 FR-SET-01）。
#[tauri::command]
#[specta::specta]
pub async fn ime_config_defaults() -> Result<ImeConfig, UiError> {
    blocking(|r| r.defaults().map(|values| ImeConfig { values })).await
}

/// 写入若干项：失败项不让整批失败，`skipped` 里带原因，界面显示在对应控件旁（17 第 3.4 节）。
#[tauri::command]
#[specta::specta]
pub async fn ime_config_set(items: Vec<ImeItemInput>) -> Result<ImeSetResult, UiError> {
    let items: Vec<ImeItem> = items
        .into_iter()
        .map(|i| ImeItem {
            key: i.key,
            value: i.value,
        })
        .collect();
    blocking(move |r| r.set_items(&items)).await
}

/// Windows 工具只允许指定的数据操作；前端不能调用任意核心 RPC。
#[tauri::command]
#[specta::specta]
pub async fn ime_tools(
    state: State<'_, AppState>,
    method: String,
    payload: String,
) -> Result<String, UiError> {
    const ALLOWED: &[&str] = &[
        "storage.health",
        "chord.get",
        "chord.save",
        "chord.builtin",
        "chord.readFile",
        "chord.exportFile",
        "scheme.rimePreview",
        "scheme.rimeImport",
        "buffer.stage",
        "buffer.read",
        "tools.allowed",
        "translate.lookup",
        "clipboard.list",
        "clipboard.action",
    ];
    if !ALLOWED.contains(&method.as_str()) || payload.len() > 2 * 1024 * 1024 {
        return Err(UiError::new("ime.tools_input", "tools.invalid"));
    }
    if [
        "buffer.read",
        "buffer.stage",
        "translate.lookup",
        "clipboard.list",
        "clipboard.action",
    ]
    .contains(&method.as_str())
        && state
            .status
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .paused
    {
        return Err(UiError::new("tools.privacy", "library.privacy"));
    }
    let params: Value = serde_json::from_str(&payload)
        .map_err(|_| UiError::new("ime.tools_input", "tools.invalid"))?;
    // Do not log input or remote text: profile mappings and staged content are private.
    tauri::async_runtime::spawn_blocking(move || rpc().call(&method, params).map(|v| v.to_string()))
        .await
        .map_err(|_| UiError::new("ime.tools_join", "error.generic"))?
        .map_err(|error| match error {
            ImeRpcError::Unavailable(_) => UiError::new("ime.unavailable", "error.ime_unavailable"),
            ImeRpcError::Remote(ref message)
                if message.contains("dictionary_") || message.contains("invalid_dictionary") =>
            {
                UiError::new("translate.dictionary", "translate.unavailable")
            }
            ImeRpcError::Remote(ref message) if message.contains("privacy_blocked") => {
                UiError::new("tools.privacy", "library.privacy")
            }
            _ => UiError::new("ime.tools_failed", "tools.failed"),
        })
}

// ── 插件管理（07 FR-SET-12，plugin-system.md §11 P5）──────────────────────────
// 装、卸、列表都在输入法核心里做（只有它知道用户目录在哪）；启用列表与安全模式是普通配置项
// （`plugins.enabled` / `plugins.safe_mode`），界面用 `ime_config_set` 改。

/// 插件命令的错误：核心给的 `plugins.*` 错误码映射成文案键 `error.plugin_*`，其余照常。
fn plugin_error(e: ImeRpcError) -> UiError {
    if let ImeRpcError::Remote(msg) = &e
        && let Some(code) = plugin_error_code(msg)
    {
        crate::hlog!("[{code}] {msg}");
        let key = format!("error.plugin_{}", &code["plugins.".len()..]);
        return UiError::new(&code, &key);
    }
    UiError::from(e)
}

async fn plugin_call<T: Send + 'static>(
    f: impl FnOnce(ImeRpc) -> Result<T, ImeRpcError> + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(move || f(rpc()))
        .await
        .map_err(|e| UiError::internal("ime.join", e))?
        .map_err(plugin_error)
}

/// 已装插件、启用与运行状态。
#[tauri::command]
#[specta::specta]
pub async fn plugins_list() -> Result<PluginList, UiError> {
    plugin_call(|r| r.plugins()).await
}

/// 安装前检查一个本地插件包（`path` 来自文件对话框），返回说明与要同意的能力；不写任何东西。
#[tauri::command]
#[specta::specta]
pub async fn plugins_inspect(path: String) -> Result<PluginPreview, UiError> {
    plugin_call(move |r| r.plugin_inspect(&path)).await
}

/// 安装：`accept` 是用户在确认框里同意的能力。核心重新检查一遍，新增能力没被同意就拒绝
/// （`plugins.consent_mismatch`，界面重新预览）。
#[tauri::command]
#[specta::specta]
pub async fn plugins_install(path: String, accept: Vec<String>) -> Result<(), UiError> {
    plugin_call(move |r| r.plugin_install(&path, &accept)).await
}

/// 卸载全部版本；`delete_data` 时连插件的私有数据一起删。启用列表由界面另行去掉这一项。
#[tauri::command]
#[specta::specta]
pub async fn plugins_uninstall(id: String, delete_data: bool) -> Result<(), UiError> {
    plugin_call(move |r| r.plugin_uninstall(&id, delete_data)).await
}

/// 重新启用被连续出错自动停用的插件；核心里没加载这个插件时返回 false。
#[tauri::command]
#[specta::specta]
pub async fn plugins_reenable(id: String) -> Result<bool, UiError> {
    plugin_call(move |r| r.plugin_reenable(&id)).await
}

/// 插件设置声明里的一项（插件不带界面，设置页按它渲染，ADR 0054 第 7 条）。
/// `kind` 是 `text` / `secure_text` / `toggle` / `choice` / `number`，其余字段按类型有无。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PluginSettingField {
    pub id: String,
    pub label: String,
    pub help: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// 默认值（密钥字段没有）
    #[serde(default)]
    #[specta(type = Unknown)]
    pub default: Value,
    /// text：最长字数
    #[serde(default)]
    pub max_len: Option<u32>,
    /// choice：可选项
    #[serde(default)]
    pub options: Option<Vec<String>>,
    /// number：范围
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

/// `plugins_settings` 的结果：设置声明与当前值（字段 id → 值，不含密钥字段）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PluginSettings {
    pub fields: Vec<PluginSettingField>,
    #[specta(type = Unknown)]
    pub values: Value,
}

/// 插件的设置声明与当前值；没有设置的插件 `fields` 为空。
#[tauri::command]
#[specta::specta]
pub async fn plugins_settings(id: String) -> Result<PluginSettings, UiError> {
    let v = plugin_call(move |r| r.plugin_settings(&id)).await?;
    serde_json::from_value(v).map_err(|e| UiError::internal("plugins.settings_shape", e))
}

/// 保存插件设置（`values`：每项是字段 id 与值，形状同 `ime_config_set`）。核心按声明校验，
/// 不合法时 `plugins.bad_settings`。
#[tauri::command]
#[specta::specta]
pub async fn plugins_save_settings(id: String, values: Vec<ImeItemInput>) -> Result<(), UiError> {
    let values = Value::Object(values.into_iter().map(|i| (i.key, i.value)).collect());
    plugin_call(move |r| r.plugin_save_settings(&id, &values)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 形状照 moodflow-webdata `plugins.rs` 的 `settings_json`（`Field` 平铺 `FieldKind`，标签字段是 `type`）。
    #[test]
    fn plugin_settings_parse_from_core_shape() {
        let s: PluginSettings = serde_json::from_value(json!({
            "fields": [
                { "id": "lang", "label": "目标语言", "help": "翻译成哪种语言",
                  "type": "choice", "options": ["en", "ja"], "default": "en" },
                { "id": "limit", "label": "最长字数", "help": "超过就截断",
                  "type": "number", "default": 500.0, "min": 1.0, "max": 5000.0 },
                { "id": "note", "label": "前缀", "help": "加在前面",
                  "type": "text", "default": "", "max_len": 256 },
                { "id": "key", "label": "密钥", "help": "服务商给的", "type": "secure_text" }
            ],
            "values": { "lang": "ja", "limit": 500.0, "note": "" }
        }))
        .unwrap();
        assert_eq!(
            s.fields[0].options.as_deref(),
            Some(&["en".to_string(), "ja".to_string()][..])
        );
        assert_eq!(s.fields[1].max, Some(5000.0));
        assert_eq!(s.fields[2].max_len, Some(256));
        assert!(s.fields[3].default.is_null());
        assert_eq!(s.values["lang"], json!("ja"));
    }
}
