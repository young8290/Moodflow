//! moodflow-typing-stats：日常输入统计与打字测速（纯逻辑）。
//!
//! 口径取自 XSIME（见 `docs/design/xsime-feature-port-plan.md` §3.3），分两个互不相干的数据集：
//!
//! - [`daily`]：**日常统计**，只存计数——按键分布、上屏次数、成文字数（按汉字/拉丁/其他分类）、
//!   活跃时长。活跃时长只累计不超过 [`daily::ACTIVE_GAP_MS`] 的相邻事件间隔，走开、发呆不算。
//!   **不存任何正文**：上屏文本进来只数字数，立刻丢弃。
//! - [`practice`]：**打字测速**，用户显式开始的练习。计分口径：有效速度（按最终字准折算的每分钟字数）、
//!   过程正确率（每次新增的字是否与范文同位一致）、最终字准（与范文的有界编辑距离）。
//!   练习期间的输入**不计入**日常统计——由调用方在练习时不调用 [`daily::Recorder`]。
//!
//! # 边界
//!
//! - 隐私闸门在调用方：密码框、安全桌面、无痕、黑名单应用里不调用这里（与 `moodflow-tap` 同一个闸门）。
//!   [`daily::Recorder::on_commit`] 另带 `private` 参数兜底。
//! - 时间是毫秒时间戳，日期是调用方按本地时区算好的 `YYYY-MM-DD`；本 crate 不读时钟、不碰时区。
//! - 落盘由调用方：[`daily::History`] 给出 JSON 快照。

pub mod daily;
pub mod practice;

#[cfg(test)]
mod tests;
