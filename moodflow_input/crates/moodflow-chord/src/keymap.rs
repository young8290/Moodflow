//! 键位方案：格式、校验、查表。

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// 当前支持的方案格式版本。
pub const FORMAT_VERSION: u32 = 1;

/// 映射产出的是完整音节还是拼音片段（声母/韵母等，需与另一半拼合）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Syllable,
    Fragment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hand {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    /// 组合里的键名（与 `moodflow-keys` 的键名一致，如 `"a"`、`"semicolon"`）。顺序无关。
    pub keys: Vec<String>,
    /// 输出的拼音（小写 a–z，片段可含 `'` 分隔符）。
    pub output: String,
    pub kind: OutputKind,
}

/// 一套键位方案，即导入导出的 JSON 文件内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    pub version: u32,
    pub name: String,
    pub left_keys: Vec<String>,
    pub right_keys: Vec<String>,
    /// 先敲一只手、再敲另一只手时，两批之间允许的最长间隔（毫秒）。
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(default)]
    pub mappings: Vec<Mapping>,
}

fn default_interval_ms() -> u64 {
    300
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapError {
    /// JSON 解析失败（只给位置，不回显内容）。
    Parse {
        line: usize,
        column: usize,
    },
    UnsupportedVersion(u32),
    EmptyName,
    /// 同一个键同时出现在左右键区。
    KeyInBothHands(String),
    /// 第 `index` 条映射为空组合。
    EmptyCombo {
        index: usize,
    },
    /// 第 `index` 条映射用了不在左右键区里的键。
    UnknownKey {
        index: usize,
        key: String,
    },
    /// 第 `index` 条映射的输出不是合法拼音。
    BadOutput {
        index: usize,
    },
    /// 第 `index` 条映射与第 `first` 条组合相同。
    DuplicateCombo {
        index: usize,
        first: usize,
    },
    IntervalOutOfRange,
    /// 应用前要求至少一条映射。
    NoMappings,
}

impl std::fmt::Display for KeymapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for KeymapError {}

pub(crate) type Combo = BTreeSet<String>;

impl Keymap {
    /// 解析并校验（草稿级别：允许空映射表）。
    pub fn from_json(json: &str) -> Result<Self, KeymapError> {
        let map: Keymap = serde_json::from_str(json).map_err(|e| KeymapError::Parse {
            line: e.line(),
            column: e.column(),
        })?;
        map.validate()?;
        Ok(map)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Keymap 只含可序列化的字段")
    }

    /// 草稿级校验：格式与一致性，不要求有映射。
    pub fn validate(&self) -> Result<(), KeymapError> {
        if self.version != FORMAT_VERSION {
            return Err(KeymapError::UnsupportedVersion(self.version));
        }
        if self.name.trim().is_empty() {
            return Err(KeymapError::EmptyName);
        }
        if !(50..=2000).contains(&self.interval_ms) {
            return Err(KeymapError::IntervalOutOfRange);
        }
        let left: HashSet<&str> = self.left_keys.iter().map(String::as_str).collect();
        if let Some(k) = self.right_keys.iter().find(|k| left.contains(k.as_str())) {
            return Err(KeymapError::KeyInBothHands(k.clone()));
        }
        let mut seen: HashMap<Combo, usize> = HashMap::new();
        for (index, m) in self.mappings.iter().enumerate() {
            if m.keys.is_empty() {
                return Err(KeymapError::EmptyCombo { index });
            }
            if let Some(key) = m.keys.iter().find(|k| self.hand_of(k).is_none()) {
                return Err(KeymapError::UnknownKey {
                    index,
                    key: key.clone(),
                });
            }
            if !valid_output(&m.output) {
                return Err(KeymapError::BadOutput { index });
            }
            let combo: Combo = m.keys.iter().cloned().collect();
            if let Some(&first) = seen.get(&combo) {
                return Err(KeymapError::DuplicateCombo { index, first });
            }
            seen.insert(combo, index);
        }
        Ok(())
    }

    /// 应用级校验：草稿校验之外，映射表不能为空。
    pub fn validate_for_apply(&self) -> Result<(), KeymapError> {
        self.validate()?;
        if self.mappings.is_empty() {
            return Err(KeymapError::NoMappings);
        }
        Ok(())
    }

    pub fn hand_of(&self, key: &str) -> Option<Hand> {
        if self.left_keys.iter().any(|k| k == key) {
            Some(Hand::Left)
        } else if self.right_keys.iter().any(|k| k == key) {
            Some(Hand::Right)
        } else {
            None
        }
    }

    /// 建查表索引。
    pub(crate) fn index(&self) -> HashMap<Combo, Mapping> {
        self.mappings
            .iter()
            .map(|m| (m.keys.iter().cloned().collect(), m.clone()))
            .collect()
    }
}

fn valid_output(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Keymap {
        Keymap {
            version: 1,
            name: "示例".into(),
            left_keys: vec!["a".into(), "s".into(), "d".into(), "f".into()],
            right_keys: vec!["j".into(), "k".into(), "l".into(), "semicolon".into()],
            interval_ms: 300,
            mappings: vec![
                Mapping {
                    keys: vec!["s".into(), "j".into()],
                    output: "zhang".into(),
                    kind: OutputKind::Syllable,
                },
                Mapping {
                    keys: vec!["a".into()],
                    output: "zh".into(),
                    kind: OutputKind::Fragment,
                },
                Mapping {
                    keys: vec!["k".into()],
                    output: "ang".into(),
                    kind: OutputKind::Fragment,
                },
            ],
        }
    }

    #[test]
    fn round_trips_through_json() {
        let m = sample();
        assert_eq!(Keymap::from_json(&m.to_json()).unwrap(), m);
    }

    #[test]
    fn interval_defaults_when_missing() {
        let json = r#"{"version":1,"name":"x","left_keys":["a"],"right_keys":["j"]}"#;
        let m = Keymap::from_json(json).unwrap();
        assert_eq!(m.interval_ms, 300);
        assert!(m.mappings.is_empty());
    }

    #[test]
    fn empty_mappings_are_a_valid_draft_but_cannot_be_applied() {
        let mut m = sample();
        m.mappings.clear();
        assert!(m.validate().is_ok());
        assert_eq!(m.validate_for_apply(), Err(KeymapError::NoMappings));
    }

    #[test]
    fn rejects_inconsistent_maps() {
        let mut m = sample();
        m.right_keys.push("a".into());
        assert_eq!(m.validate(), Err(KeymapError::KeyInBothHands("a".into())));

        let mut m = sample();
        m.mappings[0].keys.push("q".into());
        assert_eq!(
            m.validate(),
            Err(KeymapError::UnknownKey {
                index: 0,
                key: "q".into()
            })
        );

        let mut m = sample();
        m.mappings[1].output = "ZH".into();
        assert_eq!(m.validate(), Err(KeymapError::BadOutput { index: 1 }));

        let mut m = sample();
        m.mappings.push(Mapping {
            keys: vec!["j".into(), "s".into()],
            output: "zhan".into(),
            kind: OutputKind::Syllable,
        });
        assert_eq!(
            m.validate(),
            Err(KeymapError::DuplicateCombo { index: 3, first: 0 }),
            "组合与顺序无关"
        );

        let mut m = sample();
        m.version = 2;
        assert_eq!(m.validate(), Err(KeymapError::UnsupportedVersion(2)));
    }

    #[test]
    fn parse_errors_report_position_only() {
        let err = Keymap::from_json("{\n  \"version\": 1,\n  oops").unwrap_err();
        assert!(matches!(err, KeymapError::Parse { line: 3, .. }));
    }
}
