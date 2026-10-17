//! moodflow-plugin-api：插件清单、能力与设置声明（纯数据 + 校验）。
//!
//! 信任模型见 ADR 0054，完整设计见 `docs/design/plugin-system.md`。本 crate 只回答
//! 「这份清单合不合法、声明了什么、升级时多要了什么」，不加载、不执行任何插件。
//!
//! 三条在这里就要守住的规则：
//! - **联网只给缓冲区动作**：声明了逐键扩展点（候选来源、过滤、联想、上屏变换）的插件不得申请网络。
//! - **安全资产不可被内容包覆盖**：危机词表、禁用词表、繁简危机对照、隐私说明（[`PROTECTED_FILES`]）。
//! - **升级时能力增加须重新同意**：[`added_capabilities`]。

mod manifest;
mod settings;

pub use manifest::{
    Capabilities, Capability, ContentKind, Contributes, Kind, Manifest, ManifestError,
    PROTECTED_FILES, Platform, SDK_VERSION, Version, added_capabilities,
};
pub use settings::{Field, FieldKind, SettingsError, SettingsSchema};
