//! 设置声明：插件不带自己的界面，设置页由宿主按声明渲染（ADR 0054 第 7 条）。
//!
//! 字段类型与 XSIME `PLUGIN-CONFIGURATION.md` 一致：text / secure_text / toggle / choice / number。
//! 每个字段必须有默认值、边界与面向用户的说明；错误信息只说怎么改，不回显被拒绝的值。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldKind {
    Text {
        #[serde(default)]
        default: String,
        #[serde(default = "default_max_len")]
        max_len: usize,
    },
    /// 密钥类：存平台加密存储，不进日志、不回显，设置页只显示是否已填。
    SecureText,
    Toggle {
        #[serde(default)]
        default: bool,
    },
    Choice {
        options: Vec<String>,
        default: String,
    },
    Number {
        default: f64,
        min: f64,
        max: f64,
    },
}

fn default_max_len() -> usize {
    256
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// 稳定的 ASCII 小写标识；改标题不改 id。
    pub id: String,
    pub label: String,
    pub help: String,
    #[serde(flatten)]
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsSchema {
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    Parse(String),
    BadFieldId(String),
    DuplicateField(String),
    MissingHelp(String),
    BadDefault(String),
    /// 用户填的值不合法：只给字段 id 与修复提示。
    Invalid {
        field: String,
        hint: &'static str,
    },
    UnknownField(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for SettingsError {}

impl SettingsSchema {
    pub fn from_toml(text: &str) -> Result<Self, SettingsError> {
        let s: SettingsSchema =
            toml::from_str(text).map_err(|e| SettingsError::Parse(e.message().to_owned()))?;
        s.validate()?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        let mut seen = BTreeSet::new();
        for f in &self.fields {
            let ok_id = !f.id.is_empty()
                && f.id.len() <= 32
                && f.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !ok_id {
                return Err(SettingsError::BadFieldId(f.id.clone()));
            }
            if !seen.insert(f.id.as_str()) {
                return Err(SettingsError::DuplicateField(f.id.clone()));
            }
            if f.help.trim().is_empty() || f.label.trim().is_empty() {
                return Err(SettingsError::MissingHelp(f.id.clone()));
            }
            let bad_default = match &f.kind {
                FieldKind::Text { default, max_len } => default.chars().count() > *max_len,
                FieldKind::Choice { options, default } => {
                    options.is_empty() || !options.contains(default)
                }
                FieldKind::Number { default, min, max } => {
                    !(min <= max && (*min..=*max).contains(default))
                }
                FieldKind::SecureText | FieldKind::Toggle { .. } => false,
            };
            if bad_default {
                return Err(SettingsError::BadDefault(f.id.clone()));
            }
        }
        Ok(())
    }

    /// 默认值（密钥字段不在其中：密钥没有默认值，也不进普通设置存储）。
    pub fn defaults(&self) -> BTreeMap<String, Value> {
        self.fields
            .iter()
            .filter_map(|f| {
                let v = match &f.kind {
                    FieldKind::Text { default, .. } => Value::from(default.clone()),
                    FieldKind::Toggle { default } => Value::from(*default),
                    FieldKind::Choice { default, .. } => Value::from(default.clone()),
                    FieldKind::Number { default, .. } => Value::from(*default),
                    FieldKind::SecureText => return None,
                };
                Some((f.id.clone(), v))
            })
            .collect()
    }

    /// 校验一份用户设置（不含密钥字段）。
    pub fn check_values(&self, values: &BTreeMap<String, Value>) -> Result<(), SettingsError> {
        for (k, v) in values {
            let Some(f) = self.fields.iter().find(|f| &f.id == k) else {
                return Err(SettingsError::UnknownField(k.clone()));
            };
            let invalid = |hint| {
                Err(SettingsError::Invalid {
                    field: k.clone(),
                    hint,
                })
            };
            match &f.kind {
                FieldKind::Text { max_len, .. } => match v.as_str() {
                    Some(s) if s.chars().count() <= *max_len => {}
                    _ => return invalid("请填写文字，且不要超过长度上限"),
                },
                FieldKind::Toggle { .. } => {
                    if !v.is_boolean() {
                        return invalid("请选择开或关");
                    }
                }
                FieldKind::Choice { options, .. } => match v.as_str() {
                    Some(s) if options.iter().any(|o| o == s) => {}
                    _ => return invalid("请从列表中选择"),
                },
                FieldKind::Number { min, max, .. } => match v.as_f64() {
                    Some(n) if (*min..=*max).contains(&n) => {}
                    _ => return invalid("请填写范围内的数字"),
                },
                FieldKind::SecureText => return invalid("密钥不经普通设置保存"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"
[[fields]]
id = "target_lang"
label = "目标语言"
help = "翻译成哪种语言"
type = "choice"
options = ["en", "ja"]
default = "en"

[[fields]]
id = "api_key"
label = "API 密钥"
help = "在服务商控制台创建"
type = "secure_text"

[[fields]]
id = "max_chars"
label = "最长字数"
help = "超过即截断"
type = "number"
default = 500
min = 1
max = 5000
"#;

    #[test]
    fn parses_and_gives_defaults_without_secrets() {
        let s = SettingsSchema::from_toml(SCHEMA).unwrap();
        let d = s.defaults();
        assert_eq!(d.get("target_lang"), Some(&Value::from("en")));
        assert!(!d.contains_key("api_key"));
        assert!(s.check_values(&d).is_ok());
    }

    #[test]
    fn rejects_bad_declarations() {
        assert!(matches!(
            SettingsSchema::from_toml(&SCHEMA.replace("default = \"en\"", "default = \"fr\"")),
            Err(SettingsError::BadDefault(_))
        ));
        assert!(matches!(
            SettingsSchema::from_toml(&SCHEMA.replace("max_chars", "target_lang")),
            Err(SettingsError::DuplicateField(_))
        ));
        assert!(matches!(
            SettingsSchema::from_toml(&SCHEMA.replace("help = \"超过即截断\"", "help = \" \"")),
            Err(SettingsError::MissingHelp(_))
        ));
    }

    #[test]
    fn value_errors_name_the_field_but_not_the_value() {
        let s = SettingsSchema::from_toml(SCHEMA).unwrap();
        let mut v = BTreeMap::new();
        v.insert("max_chars".to_string(), Value::from(99999));
        let err = s.check_values(&v).unwrap_err();
        assert_eq!(
            err,
            SettingsError::Invalid {
                field: "max_chars".into(),
                hint: "请填写范围内的数字"
            }
        );
        let mut v = BTreeMap::new();
        v.insert("api_key".to_string(), Value::from("sk-123"));
        assert!(s.check_values(&v).is_err(), "密钥不能走普通设置");
    }
}
