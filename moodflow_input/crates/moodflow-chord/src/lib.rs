//! moodflow-chord：并击（多键同按）输入的纯逻辑。
//!
//! 契约取自 XSIME `CHORD-KEYMAPS.md`（见 `docs/design/xsime-feature-port-plan.md` §2.3）：
//!
//! - 一套规则同时支持**同批按下**和**先左后右分开敲**；一组键**全部松开时**才结算。
//! - 键位方案 = 左右键区 + 映射表（组合 → 完整音节或拼音片段），以 JSON 导入导出。
//!   空映射表可以存为草稿，但不能应用（[`Keymap::validate_for_apply`]）。
//! - 并击只产出拼音片段，喂给现有拼音引擎；本 crate 不出候选。
//!
//! 时间由调用方给（毫秒，单调），便于测试，也让 Windows 与 Android 共用一份结算逻辑。

mod keymap;
mod settle;

pub use keymap::{Hand, Keymap, KeymapError, Mapping, OutputKind};
pub use settle::{ChordEvent, Settler};
