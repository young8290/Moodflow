//! 插件清单 `manifest.toml`。

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// 当前宿主支持的插件 SDK 主版本。宿主同时接受本版本与上一个主版本。
pub const SDK_VERSION: u32 = 1;

/// 内容包不得覆盖或删减的出厂安全资产（文件名，不分目录）。
pub const PROTECTED_FILES: &[&str] = &[
    "crisis_lexicon.toml",
    "banned_words.toml",
    "crisis_t2s.toml",
    "privacy_notice.md",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Android,
}

/// 内容包的类型，决定交给哪个现有加载器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    /// 输入方案包（`.wpkg` 结构）。
    Scheme,
    /// 主题包（`.wtheme` 结构）。
    Theme,
    Phrases,
    SoftKeyboard,
    Punctuation,
    ChordKeymap,
    /// Hub 文案模板（暖心话、周信等）。
    HubTemplates,
    /// 桌宠形象（Codex 精灵图格式，ADR 0044）。
    Pet,
    QuickInputFormats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// L0：纯数据，无代码。
    Content { content: ContentKind },
    /// L1：WASM 沙箱插件。
    Wasm { entry: String },
}

/// 插件声明要接入的扩展点。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contributes {
    /// 候选来源，`priority` 越小越靠前（与内置来源同一套配额，不跨源比分）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_provider: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_filter: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assoc_source: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_transform: Option<Priority>,
    /// 命令直通车函数名。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cmdbar_functions: Vec<String>,
    /// 缓冲区动作（用户显式触发，唯一可联网的扩展点）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_action: Option<BufferAction>,
    /// 设置声明文件（包内相对路径）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Priority {
    #[serde(default = "default_priority")]
    pub priority: u8,
}

fn default_priority() -> u8 {
    50
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferAction {
    pub label: String,
}

/// 能力声明。默认全部没有。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub storage: bool,
    #[serde(default)]
    pub secrets: bool,
    /// 允许访问的主机（只写主机名，如 `api.example.com`）。
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub clipboard_read: bool,
    #[serde(default)]
    pub context_recent: bool,
    #[serde(default)]
    pub notify: bool,
}

/// 单项能力，用于列给用户看与比较升级差异。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    Storage,
    Secrets,
    Network(String),
    ClipboardRead,
    ContextRecent,
    Notify,
}

impl Capabilities {
    pub fn list(&self) -> BTreeSet<Capability> {
        let mut s = BTreeSet::new();
        if self.storage {
            s.insert(Capability::Storage);
        }
        if self.secrets {
            s.insert(Capability::Secrets);
        }
        for h in &self.network {
            s.insert(Capability::Network(h.to_ascii_lowercase()));
        }
        if self.clipboard_read {
            s.insert(Capability::ClipboardRead);
        }
        if self.context_recent {
            s.insert(Capability::ContextRecent);
        }
        if self.notify {
            s.insert(Capability::Notify);
        }
        s
    }
}

/// 升级时新增的能力（须重新征得用户同意）。
pub fn added_capabilities(old: &Manifest, new: &Manifest) -> Vec<Capability> {
    let before = old.capabilities.list();
    new.capabilities
        .list()
        .into_iter()
        .filter(|c| !before.contains(c))
        .collect()
}

/// 语义化版本（只认 `主.次.补丁`，可带 `-预发布` 后缀，比较时预发布低于正式版）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_owned())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut it = core.split('.');
        let num = |x: Option<&str>| -> Option<u64> {
            let x = x?;
            if x.is_empty() || (x.len() > 1 && x.starts_with('0')) {
                return None;
            }
            x.parse().ok()
        };
        let v = Version {
            major: num(it.next())?,
            minor: num(it.next())?,
            patch: num(it.next())?,
            pre,
        };
        it.next().is_none().then_some(v)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{p}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// 反向域名，全小写，如 `dev.example.pinyin-emoji`。发布后不可复用。
    pub id: String,
    pub name: String,
    pub version: String,
    pub sdk: u32,
    pub min_host_version: String,
    #[serde(flatten)]
    pub kind: Kind,
    pub platforms: Vec<Platform>,
    pub license: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub contributes: Contributes,
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    Parse(String),
    BadId,
    BadVersion,
    UnsupportedSdk(u32),
    HostTooOld {
        required: String,
    },
    NoPlatforms,
    EmptyName,
    BadEntry,
    /// WASM 插件没有声明任何扩展点。
    NothingContributed,
    /// 内容包声明了扩展点或能力（内容包没有代码，什么也不能声明）。
    ContentWithCode,
    /// 逐键扩展点不得申请网络（ADR 0054 第 6 条）。
    NetworkOnPerKeystroke,
    /// 申请了网络却没有缓冲区动作。
    NetworkWithoutBufferAction,
    BadHost(String),
    BadFunctionName(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    /// 解析并校验。`host_version` 是当前宿主版本（`docs/VERSION`，可带预发布后缀）。
    pub fn from_toml(text: &str, host_version: &str) -> Result<Self, ManifestError> {
        let m: Manifest = toml::from_str(text).map_err(|e| {
            // 只报位置与类别，不回显原文（清单里可能被误填了密钥）。
            let at = e
                .span()
                .map(|s| format!("字节 {}", s.start))
                .unwrap_or_default();
            ManifestError::Parse(format!("{at} {}", e.message()))
        })?;
        m.validate(host_version)?;
        Ok(m)
    }

    pub fn validate(&self, host_version: &str) -> Result<(), ManifestError> {
        if !valid_id(&self.id) {
            return Err(ManifestError::BadId);
        }
        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }
        Version::parse(&self.version).ok_or(ManifestError::BadVersion)?;
        if self.sdk == 0 || self.sdk > SDK_VERSION || self.sdk + 1 < SDK_VERSION {
            return Err(ManifestError::UnsupportedSdk(self.sdk));
        }
        let need = Version::parse(&self.min_host_version).ok_or(ManifestError::BadVersion)?;
        let host = Version::parse(host_version).ok_or(ManifestError::BadVersion)?;
        // 只比主次补丁：宿主 0.130.0-beta 满足 min 0.130.0。
        if (host.major, host.minor, host.patch) < (need.major, need.minor, need.patch) {
            return Err(ManifestError::HostTooOld {
                required: self.min_host_version.clone(),
            });
        }
        if self.platforms.is_empty() {
            return Err(ManifestError::NoPlatforms);
        }
        let c = &self.contributes;
        let caps = &self.capabilities;
        match &self.kind {
            Kind::Content { .. } => {
                if *c != Contributes::default() || *caps != Capabilities::default() {
                    return Err(ManifestError::ContentWithCode);
                }
            }
            Kind::Wasm { entry } => {
                if entry.is_empty()
                    || !entry.ends_with(".wasm")
                    || entry.contains("..")
                    || entry.starts_with('/')
                    || entry.contains('\\')
                {
                    return Err(ManifestError::BadEntry);
                }
                let per_keystroke = c.candidate_provider.is_some()
                    || c.candidate_filter.is_some()
                    || c.assoc_source.is_some()
                    || c.commit_transform.is_some();
                if !per_keystroke && c.cmdbar_functions.is_empty() && c.buffer_action.is_none() {
                    return Err(ManifestError::NothingContributed);
                }
                if !caps.network.is_empty() {
                    if per_keystroke {
                        return Err(ManifestError::NetworkOnPerKeystroke);
                    }
                    if c.buffer_action.is_none() {
                        return Err(ManifestError::NetworkWithoutBufferAction);
                    }
                }
                if let Some(h) = caps.network.iter().find(|h| !valid_host(h)) {
                    return Err(ManifestError::BadHost(h.clone()));
                }
                if let Some(f) = c.cmdbar_functions.iter().find(|f| !valid_fn(f)) {
                    return Err(ManifestError::BadFunctionName(f.clone()));
                }
            }
        }
        Ok(())
    }

    pub fn parsed_version(&self) -> Version {
        Version::parse(&self.version).expect("validate 过的版本可解析")
    }

    pub fn supports(&self, p: Platform) -> bool {
        self.platforms.contains(&p)
    }
}

fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    id.len() <= 128
        && parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !p.starts_with('-')
                && !p.ends_with('-')
        })
        // 内置插件的命名空间保留给官方。
        && parts[0] != "builtin"
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.contains('.')
        && h.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn valid_fn(f: &str) -> bool {
    !f.is_empty()
        && f.len() <= 32
        && f.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && f.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "0.130.0-beta";

    const WASM: &str = r#"
id = "dev.example.emoji"
name = "拼音出表情"
version = "1.2.0"
sdk = 1
min_host_version = "0.130.0"
type = "wasm"
entry = "plugin.wasm"
platforms = ["windows", "android"]
license = "MIT"

[contributes]
candidate_provider = { priority = 40 }

[capabilities]
storage = true
"#;

    #[test]
    fn parses_a_wasm_plugin() {
        let m = Manifest::from_toml(WASM, HOST).unwrap();
        assert_eq!(
            m.kind,
            Kind::Wasm {
                entry: "plugin.wasm".into()
            }
        );
        assert_eq!(m.contributes.candidate_provider.unwrap().priority, 40);
        assert!(m.supports(Platform::Android));
        assert_eq!(
            m.capabilities.list(),
            [Capability::Storage].into_iter().collect()
        );
    }

    #[test]
    fn parses_a_content_pack() {
        let m = Manifest::from_toml(
            r#"
id = "dev.example.dark"
name = "夜色"
version = "1.0.0"
sdk = 1
min_host_version = "0.100.0"
type = "content"
content = "theme"
platforms = ["windows"]
license = "CC-BY-4.0"
"#,
            HOST,
        )
        .unwrap();
        assert_eq!(
            m.kind,
            Kind::Content {
                content: ContentKind::Theme
            }
        );
    }

    #[test]
    fn per_keystroke_plugins_cannot_ask_for_network() {
        let bad = WASM.replace("storage = true", "network = [\"api.example.com\"]");
        assert_eq!(
            Manifest::from_toml(&bad, HOST),
            Err(ManifestError::NetworkOnPerKeystroke)
        );
    }

    #[test]
    fn network_requires_a_buffer_action() {
        let base = WASM
            .replace(
                "candidate_provider = { priority = 40 }",
                "cmdbar_functions = [\"rate\"]",
            )
            .replace("storage = true", "network = [\"api.example.com\"]");
        assert_eq!(
            Manifest::from_toml(&base, HOST),
            Err(ManifestError::NetworkWithoutBufferAction)
        );
        let ok = base.replace(
            "cmdbar_functions = [\"rate\"]",
            "buffer_action = { label = \"翻译\" }",
        );
        assert!(Manifest::from_toml(&ok, HOST).is_ok());
    }

    #[test]
    fn content_packs_cannot_declare_code_things() {
        let bad = r#"
id = "dev.example.pack"
name = "x"
version = "1.0.0"
sdk = 1
min_host_version = "0.1.0"
type = "content"
content = "phrases"
platforms = ["windows"]
license = "MIT"
[capabilities]
notify = true
"#;
        assert_eq!(
            Manifest::from_toml(bad, HOST),
            Err(ManifestError::ContentWithCode)
        );
    }

    #[test]
    fn rejects_bad_ids_versions_and_entries() {
        for (from, to, err) in [
            ("dev.example.emoji", "Emoji", ManifestError::BadId),
            ("dev.example.emoji", "builtin.emoji", ManifestError::BadId),
            (
                "version = \"1.2.0\"",
                "version = \"1.2\"",
                ManifestError::BadVersion,
            ),
            (
                "entry = \"plugin.wasm\"",
                "entry = \"../x.wasm\"",
                ManifestError::BadEntry,
            ),
            ("sdk = 1", "sdk = 9", ManifestError::UnsupportedSdk(9)),
        ] {
            assert_eq!(Manifest::from_toml(&WASM.replace(from, to), HOST), Err(err));
        }
        assert!(matches!(
            Manifest::from_toml(&WASM.replace("0.130.0\"", "0.131.0\""), HOST),
            Err(ManifestError::HostTooOld { .. })
        ));
    }

    #[test]
    fn nothing_contributed_is_rejected() {
        let bad = WASM.replace("candidate_provider = { priority = 40 }", "");
        assert_eq!(
            Manifest::from_toml(&bad, HOST),
            Err(ManifestError::NothingContributed)
        );
    }

    #[test]
    fn added_capabilities_lists_only_new_ones() {
        let old = Manifest::from_toml(WASM, HOST).unwrap();
        let new = Manifest::from_toml(
            &WASM.replace(
                "storage = true",
                "storage = true\nnotify = true\nclipboard_read = true",
            ),
            HOST,
        )
        .unwrap();
        assert_eq!(
            added_capabilities(&old, &new),
            vec![Capability::ClipboardRead, Capability::Notify]
        );
        assert!(added_capabilities(&new, &old).is_empty());
    }

    #[test]
    fn version_ordering() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("1.2.0") > v("1.1.9"));
        assert!(v("1.0.0") > v("1.0.0-beta"));
        assert!(Version::parse("01.0.0").is_none());
        assert_eq!(v("2.0.1-rc1").to_string(), "2.0.1-rc1");
    }

    #[test]
    fn parse_errors_do_not_echo_content() {
        let err = Manifest::from_toml("id = \"sk-secret-123\nname", HOST).unwrap_err();
        let ManifestError::Parse(msg) = err else {
            panic!()
        };
        assert!(!msg.contains("sk-secret-123"), "{msg}");
    }
}
