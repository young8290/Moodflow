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
