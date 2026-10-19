//! 并击的移动端门面：键位方案 JSON → 结算器（`docs/design/xsime-feature-port-plan.md` §2.3）。
//!
//! 首批只接**物理键盘**并击：宿主把 keydown / keyup 连同时间戳交进来，结算出的拼音片段再经
//! [`crate::MobileCore::feed_text`] 送进拼音引擎（不另起候选路径）。触屏并击是 P3 末尾的事。
//!
//! 键名与 `moodflow-keys` 一致（`"a"`、`"semicolon"`…），宿主负责把平台键码换成键名。
//!
//! # 内置方案
//!
//! [`MobileChord::builtin`] 是 Android 触屏并击（XSIME 双拇指滑动并击，见 `moodflow_android`
//! 的 `ChordSurface`）用的默认方案：`data/chord-builtin.json`，由 XSIME（Apache-2.0）的
//! `platforms/android/resources/chord-profile.json`（427 条，SHA-256 `6cc5de95…0f90f0f`）转成
//! `moodflow-chord` 格式。XSIME 在结算时还会把「左手片段 + 右手片段」拼成合法音节、并认几条
//! 单指滑动路线，`moodflow-chord` 没有这两条规则，转换时按同样的规则展开成了 14 条显式映射，
//! 共 441 条。出处见仓库根 `NOTICE.md`。

use moodflow_chord::{ChordEvent, Keymap, OutputKind, Settler};

/// 一次结算的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordOutput {
    /// 送进拼音引擎的串。`fragment = true` 表示声母/韵母片段（与相邻输入拼合）。
    Emit { pinyin: String, fragment: bool },
    /// 这组键没有映射：不出字。
    NoMatch,
}

/// 已应用的并击方案 + 结算器。
#[derive(Debug, Clone)]
pub struct MobileChord {
    settler: Settler,
    name: String,
    left: Vec<String>,
    right: Vec<String>,
    mappings: Vec<ChordMapping>,
}

/// 内置方案原文（`moodflow-chord` 键位方案 JSON）。
pub const BUILTIN_JSON: &str = include_str!("../data/chord-builtin.json");

/// 方案里的一条映射（设置页列表、触屏手势的可达键计算用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordMapping {
    /// 键名，按左手键区、右手键区的方案顺序排列。
    pub keys: Vec<String>,
    pub output: String,
    pub fragment: bool,
}

impl MobileChord {
    /// 内置默认方案。出厂数据已在测试里校验过，解析失败只可能是构建产物被改坏，按 panic 处理。
    pub fn builtin() -> Self {
        Self::from_json(BUILTIN_JSON).expect("内置并击方案必须通过应用级校验")
    }

    /// 左手键区（方案顺序）。
    pub fn left_keys(&self) -> &[String] {
        &self.left
    }

    /// 右手键区（方案顺序）。
    pub fn right_keys(&self) -> &[String] {
        &self.right
    }

    /// 全部映射（方案顺序）。
    pub fn mappings(&self) -> &[ChordMapping] {
        &self.mappings
    }

    /// 一次性结算一组同时按下又全部松开的键（触屏手势：手指全部抬起时才知道整组键）。
    ///
    /// 只有一只手时 [`Settler`] 会等另一只手（先左后右），触屏手势已经是完整的一组，
    /// 这里把等待立即结算掉。单个键不查表，返回 `None`，由宿主按普通键处理。
    pub fn settle_batch(&mut self, keys: &[String], now_ms: u64) -> Option<ChordOutput> {
        if keys.len() < 2 {
            return None;
        }
        self.settler.reset();
        let mut out = None;
        for k in keys {
            if let Some(o) = self.settler.key_down(k, now_ms) {
                out = Some(convert(o));
            }
        }
        for k in keys {
            if let Some(o) = self.settler.key_up(k, now_ms) {
                out = Some(convert(o));
            }
        }
        if out.is_none() {
            out = self.settler.tick(u64::MAX).map(convert);
        }
        self.settler.reset();
        out
    }

    /// 解析并按**应用级**校验（空映射表可存草稿，不可应用）。错误串不含方案内容。
    pub fn from_json(json: &str) -> Result<Self, String> {
        let map = Keymap::from_json(json).map_err(|e| e.to_string())?;
        map.validate_for_apply().map_err(|e| e.to_string())?;
        let order = |k: &String| {
            map.left_keys
                .iter()
                .chain(&map.right_keys)
                .position(|x| x == k)
                .unwrap_or(usize::MAX)
        };
        let mappings = map
            .mappings
            .iter()
            .map(|m| {
                let mut keys = m.keys.clone();
                keys.sort_by_key(order);
                ChordMapping {
                    keys,
                    output: m.output.clone(),
                    fragment: m.kind == OutputKind::Fragment,
                }
            })
            .collect();
        Ok(Self {
            settler: Settler::new(&map),
            left: map.left_keys.clone(),
            right: map.right_keys.clone(),
            mappings,
            name: map.name,
        })
    }

    /// 草稿级校验（设置页保存草稿时用）：`Ok` 不代表可以应用。
    pub fn validate_draft(json: &str) -> Result<(), String> {
        Keymap::from_json(json)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// 该键是否属于并击键区；不属于的键宿主照常走普通路径。
    pub fn is_chord_key(&self, key: &str) -> bool {
        self.settler.is_chord_key(key)
    }

    pub fn is_busy(&self) -> bool {
        self.settler.is_busy()
    }

    pub fn key_down(&mut self, key: &str, now_ms: u64) -> Option<ChordOutput> {
        self.settler.key_down(key, now_ms).map(convert)
    }

    pub fn key_up(&mut self, key: &str, now_ms: u64) -> Option<ChordOutput> {
        self.settler.key_up(key, now_ms).map(convert)
    }

    /// 时间推进：等另一只手超时则单独结算。宿主在定时器或下一次按键前调用。
    pub fn tick(&mut self, now_ms: u64) -> Option<ChordOutput> {
        self.settler.tick(now_ms).map(convert)
    }

    /// 换焦点、切方案、取消：丢弃未结算的输入。
    pub fn reset(&mut self) {
        self.settler.reset();
    }
}

fn convert(e: ChordEvent) -> ChordOutput {
    match e {
        ChordEvent::Emit { output, kind } => ChordOutput::Emit {
            pinyin: output,
            fragment: kind == OutputKind::Fragment,
        },
        ChordEvent::NoMatch { .. } => ChordOutput::NoMatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = r#"{
        "version": 1,
        "name": "测试",
        "left_keys": ["a", "s"],
        "right_keys": ["j", "k"],
        "interval_ms": 300,
        "mappings": [
            {"keys": ["s", "j"], "output": "zhang", "kind": "syllable"},
            {"keys": ["a"], "output": "zh", "kind": "fragment"}
        ]
    }"#;

    #[test]
    fn simultaneous_chord_settles_on_last_release() {
        let mut c = MobileChord::from_json(MAP).unwrap();
        assert!(c.is_chord_key("s"));
        assert!(!c.is_chord_key("q"));
        assert_eq!(c.key_down("s", 0), None);
        assert_eq!(c.key_down("j", 10), None);
        assert_eq!(c.key_up("s", 20), None, "只松开一只手不出字");
        assert_eq!(
            c.key_up("j", 30),
            Some(ChordOutput::Emit {
                pinyin: "zhang".into(),
                fragment: false
            })
        );
    }

    #[test]
    fn fragment_is_flagged() {
        let mut c = MobileChord::from_json(MAP).unwrap();
        c.key_down("a", 0);
        assert_eq!(
            c.key_up("a", 5),
            Some(ChordOutput::Emit {
                pinyin: "zh".into(),
                fragment: true
            })
        );
    }

    fn keys(s: &[&str]) -> Vec<String> {
        s.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn builtin_profile_carries_the_xsime_mappings() {
        let mut c = MobileChord::builtin();
        assert_eq!(c.mappings().len(), 441, "XSIME 427 条 + 展开的 14 条");
        assert_eq!(c.left_keys().len(), 15);
        assert_eq!(c.right_keys().len(), 13);
        assert!(c.right_keys().iter().any(|k| k == "comma"));
        // XSIME ChordInputTest 里的例子：dvi → ni、xck → hao（全拼输出）
        assert_eq!(
            c.settle_batch(&keys(&["d", "v", "i"]), 0),
            Some(ChordOutput::Emit {
                pinyin: "ni".into(),
                fragment: false
            })
        );
        assert_eq!(
            c.settle_batch(&keys(&["x", "c", "k"]), 10),
            Some(ChordOutput::Emit {
                pinyin: "hao".into(),
                fragment: false
            })
        );
        // 单手片段不必等另一只手
        assert_eq!(
            c.settle_batch(&keys(&["e", "f"]), 20),
            Some(ChordOutput::Emit {
                pinyin: "sh".into(),
                fragment: true
            })
        );
        // 展开的片段拼合：左 a + 右 i = ai
        assert_eq!(
            c.settle_batch(&keys(&["a", "i"]), 30),
            Some(ChordOutput::Emit {
                pinyin: "ai".into(),
                fragment: false
            })
        );
        assert_eq!(c.settle_batch(&keys(&["q"]), 40), None, "单键交给宿主");
        assert_eq!(
            c.settle_batch(&keys(&["q", "z"]), 50),
            Some(ChordOutput::NoMatch)
        );
    }

    #[test]
    fn builtin_mappings_list_keys_in_profile_order() {
        let c = MobileChord::builtin();
        let m = c
            .mappings()
            .iter()
            .find(|m| m.output == "que")
            .expect("que");
        assert_eq!(m.keys, keys(&["q", "m", "period"]));
        assert!(c.mappings().iter().all(|m| m.keys.len() >= 2));
    }

    #[test]
    fn empty_mapping_is_draft_only() {
        let draft =
            r#"{"version":1,"name":"草稿","left_keys":["a"],"right_keys":["j"],"mappings":[]}"#;
        assert!(MobileChord::validate_draft(draft).is_ok());
        assert!(MobileChord::from_json(draft).is_err());
    }
}
