//! Rime 方案包与文本词库导入：转换成 Moodflow 方案包（`.wpkg`）。
//!
//! 契约见 `docs/design/xsime-feature-port-plan.md` §2.5。Moodflow 没有 librime，**不能**完整执行 Rime 方案
//! （Lua、拼写运算、过滤器无法等价），所以目标收窄为：
//!
//! - 方案里能等价表达的部分（码表或全拼、词库、码长）生成 `.schema.toml`，词库文件原样搬运
//!   （Moodflow 直接读 Rime `.dict.yaml`，类型 `rime_codetable` / `rime_pinyin`）。
//! - `import_tables` 展开成多条 `[[dictionaries]]`（Moodflow 的词库加载器不跟随 `import_tables`）。
//! - 表达不了的部分逐条列为 [`Diagnostic`]，**不静默丢弃**。
//!
//! 安全规则沿用 XSIME 与本仓方案包：拒绝路径穿越与符号链接、限制条目数与解压总量；
//! 忽略 `__MACOSX`、`._*`、`build/`、`sync/`、`*.userdb*`、`installation.yaml`、`user.yaml`，
//! 不导入其他设备的私有状态。同一份 ZIP 以 SHA-256 标识，Windows 与 Android 转换结果一致。
//!
//! 导入的方案 id 一律加 `rime_` 前缀，避免与出厂方案同名而整体覆盖出厂方案（用户目录同名文件优先）。

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::scheme::{MAX_PACKAGE_ENTRIES, MAX_PACKAGE_UNCOMPRESSED_BYTES};

/// 包内一个文件（路径已去掉外层包装目录前的原始相对路径，`/` 分隔）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// 转换中发现的、无法等价表达或被跳过的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// 按安全规则忽略的文件（私有状态、系统垃圾等）。
    IgnoredFile { path: String },
    /// 导入不支持的组件（Lua、OpenCC、`.custom.yaml` 补丁等），列一次文件。
    UnsupportedComponent { path: String, kind: &'static str },
    /// 方案用了 Lua 翻译器或过滤器，这部分逻辑不会生效。
    LuaIgnored { schema: String },
    /// 拼写运算（双拼、模糊音、简拼规则等）未转换，按全拼处理。
    SpellerAlgebraIgnored { schema: String },
    /// 过滤器（简繁、Emoji 等）未转换。
    FiltersIgnored { schema: String },
    /// 方案没写码长，按 4 处理。
    AssumedMaxCodeLength { schema: String },
    /// 方案的词库或被导入的词表在包里找不到。
    MissingDictionary { schema: String, name: String },
    /// 方案的翻译器 Moodflow 不支持，整套方案跳过。
    SchemeSkipped {
        schema: String,
        reason: &'static str,
    },
    /// 方案 id 被改写（加前缀或清理非法字符）。
    IdRenamed { from: String, to: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Codetable,
    Pinyin,
}

/// 一套转换好的方案：`files` 即 `.wpkg` 的全部条目（schemas 根相对路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedScheme {
    pub id: String,
    pub name: String,
    pub engine: EngineKind,
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    /// 源 ZIP 的 SHA-256（十六进制小写）。
    pub source_sha256: String,
    pub schemes: Vec<ConvertedScheme>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RimeImportError {
    Zip(String),
    UnsafePath(String),
    Symlink(String),
    TooManyEntries,
    TooLarge,
    /// 包里没有任何 `.schema.yaml`。
    NoSchema,
    /// 存在互不包含的多个方案根目录，无法判断导入哪一个。
    MultipleRoots(Vec<String>),
    /// 文本词库编码无法识别（只支持 UTF-8 与带 BOM 的 UTF-16）。
    UnsupportedEncoding,
    /// 文本词库里没有可用条目。
    EmptyDictionary,
    /// 所有方案都被跳过。
    NothingConvertible,
}

impl std::fmt::Display for RimeImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for RimeImportError {}

// ── 读 ZIP ──────────────────────────────────────────────

/// 读取 Rime ZIP：做安全检查与忽略规则，返回文件、源哈希与忽略诊断。
pub fn read_zip(
    path: &Path,
) -> Result<(Vec<SourceFile>, String, Vec<Diagnostic>), RimeImportError> {
    let raw = std::fs::read(path).map_err(|e| RimeImportError::Zip(e.kind().to_string()))?;
    read_zip_bytes(&raw)
}

pub fn read_zip_bytes(
    raw: &[u8],
) -> Result<(Vec<SourceFile>, String, Vec<Diagnostic>), RimeImportError> {
    let sha = hex(&Sha256::digest(raw));
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw))
        .map_err(|e| RimeImportError::Zip(e.to_string()))?;
    if archive.len() > MAX_PACKAGE_ENTRIES {
        return Err(RimeImportError::TooManyEntries);
    }
    let mut files = Vec::new();
    let mut diags = Vec::new();
    let mut budget = MAX_PACKAGE_UNCOMPRESSED_BYTES;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| RimeImportError::Zip(e.to_string()))?;
        let name = entry.name().replace('\\', "/");
        if entry.is_dir() {
            continue;
        }
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(RimeImportError::Symlink(name));
        }
        if entry.enclosed_name().is_none() || name.starts_with('/') || name.contains(':') {
            return Err(RimeImportError::UnsafePath(name));
        }
        if is_ignored(&name) {
            diags.push(Diagnostic::IgnoredFile { path: name });
            continue;
        }
        if entry.size() > budget {
            return Err(RimeImportError::TooLarge);
        }
        let mut bytes = Vec::new();
        (&mut entry)
            .take(budget + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| RimeImportError::Zip(e.to_string()))?;
        if bytes.len() as u64 > budget {
            return Err(RimeImportError::TooLarge);
        }
        budget -= bytes.len() as u64;
        files.push(SourceFile { path: name, bytes });
    }
    Ok((files, sha, diags))
}

fn is_ignored(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    lower
        .split('/')
        .any(|seg| seg == "__macosx" || seg == "build" || seg == "sync" || seg.contains(".userdb"))
        || file.starts_with("._")
        || file == ".ds_store"
        || file == "installation.yaml"
        || file == "user.yaml"
        || file.ends_with(".bin")
}

// ── 转换 ────────────────────────────────────────────────

/// 把已读出的文件转换成 Moodflow 方案。`pre` 是读包阶段的诊断，原样带上。
pub fn convert(
    files: &[SourceFile],
    source_sha256: String,
    pre: Vec<Diagnostic>,
) -> Result<Conversion, RimeImportError> {
    let mut diags = pre;
    let root = find_root(files)?;
    let by_path: BTreeMap<&str, &SourceFile> = files
        .iter()
        .filter_map(|f| f.path.strip_prefix(&root).map(|rel| (rel, f)))
        .collect();

    // 不支持的组件逐个列出。
    for rel in by_path.keys() {
        let kind = if rel.ends_with(".lua") {
            Some("lua")
        } else if rel.starts_with("opencc/") {
            Some("opencc")
        } else if rel.ends_with(".custom.yaml") {
            Some("custom_patch")
        } else {
            None
        };
        if let Some(kind) = kind {
            diags.push(Diagnostic::UnsupportedComponent {
                path: (*rel).to_owned(),
                kind,
            });
        }
    }

    let mut schemes = Vec::new();
    let mut used_ids = BTreeSet::new();
    for (rel, f) in &by_path {
        if !rel.ends_with(".schema.yaml") || rel.contains('/') {
            continue;
        }
        if let Some(s) = convert_schema(rel, f, &by_path, &mut used_ids, &mut diags) {
            schemes.push(s);
        }
    }
    if schemes.is_empty() {
        return Err(RimeImportError::NothingConvertible);
    }
    Ok(Conversion {
        source_sha256,
        schemes,
        diagnostics: diags,
    })
}

/// 找方案根目录：所有 `.schema.yaml` 必须在同一目录（允许外面套几层包装目录）。
fn find_root(files: &[SourceFile]) -> Result<String, RimeImportError> {
    let dirs: BTreeSet<String> = files
        .iter()
        .filter(|f| f.path.ends_with(".schema.yaml"))
        .map(|f| match f.path.rfind('/') {
            Some(i) => f.path[..=i].to_owned(),
            None => String::new(),
        })
        .collect();
    match dirs.len() {
        0 => Err(RimeImportError::NoSchema),
        1 => Ok(dirs.into_iter().next().expect("len 1")),
        _ => Err(RimeImportError::MultipleRoots(dirs.into_iter().collect())),
    }
}

fn convert_schema(
    rel: &str,
    file: &SourceFile,
    files: &BTreeMap<&str, &SourceFile>,
    used_ids: &mut BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ConvertedScheme> {
    let fallback_id = rel.trim_end_matches(".schema.yaml").to_owned();
    let yaml: serde_yaml::Value = match serde_yaml::from_slice(&file.bytes) {
        Ok(v) => v,
        Err(_) => {
            diags.push(Diagnostic::SchemeSkipped {
                schema: fallback_id,
                reason: "schema.yaml 解析失败",
            });
            return None;
        }
    };
    let get = |path: &[&str]| -> Option<&serde_yaml::Value> {
        path.iter().try_fold(&yaml, |v, k| v.get(*k))
    };
    let rime_id = get(&["schema", "schema_id"])
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_id)
        .to_owned();
    let name = get(&["schema", "name"])
        .and_then(|v| v.as_str())
        .unwrap_or(&rime_id)
        .to_owned();
    let translators: Vec<String> = get(&["engine", "translators"])
        .and_then(|v| v.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let filters: Vec<String> = get(&["engine", "filters"])
        .and_then(|v| v.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let engine = if translators
        .iter()
        .any(|t| t.starts_with("script_translator"))
    {
        EngineKind::Pinyin
    } else if translators
        .iter()
        .any(|t| t.starts_with("table_translator"))
    {
        EngineKind::Codetable
    } else {
        diags.push(Diagnostic::SchemeSkipped {
            schema: rime_id,
            reason: "没有 script_translator 或 table_translator",
        });
        return None;
    };
    if translators
        .iter()
        .chain(&filters)
        .any(|t| t.starts_with("lua_"))
    {
        diags.push(Diagnostic::LuaIgnored {
            schema: rime_id.clone(),
        });
    }
    if filters.iter().any(|t| !t.starts_with("uniquifier")) {
        diags.push(Diagnostic::FiltersIgnored {
            schema: rime_id.clone(),
        });
    }
    if get(&["speller", "algebra"]).is_some() {
        diags.push(Diagnostic::SpellerAlgebraIgnored {
            schema: rime_id.clone(),
        });
    }

    let Some(dict_name) = get(&["translator", "dictionary"]).and_then(|v| v.as_str()) else {
        diags.push(Diagnostic::SchemeSkipped {
            schema: rime_id,
            reason: "没有 translator/dictionary",
        });
        return None;
    };
    let dicts = collect_dicts(dict_name, files, &rime_id, diags);
    if dicts.is_empty() {
        diags.push(Diagnostic::SchemeSkipped {
            schema: rime_id,
            reason: "主词库缺失",
        });
        return None;
    }

    let id = moodflow_id(&rime_id, used_ids);
    if id != rime_id {
        diags.push(Diagnostic::IdRenamed {
            from: rime_id.clone(),
            to: id.clone(),
        });
    }
    let max_code_length = match engine {
        EngineKind::Codetable => Some(
            get(&["speller", "max_code_length"])
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| {
                    diags.push(Diagnostic::AssumedMaxCodeLength {
                        schema: rime_id.clone(),
                    });
                    4
                }),
        ),
        EngineKind::Pinyin => None,
    };

    let dict_type = match engine {
        EngineKind::Codetable => "rime_codetable",
        EngineKind::Pinyin => "rime_pinyin",
    };
    let mut out_files = Vec::new();
    let mut entries = Vec::new();
    for (i, rel) in dicts.iter().enumerate() {
        let out_rel = format!("{id}/{rel}");
        out_files.push((out_rel.clone(), files[rel.as_str()].bytes.clone()));
        entries.push(DictEntry {
            id: format!("{id}_{}", sanitize(rel.trim_end_matches(".dict.yaml"))),
            label: rel.trim_end_matches(".dict.yaml").to_owned(),
            path: out_rel,
            r#type: dict_type,
            default: (i == 0).then_some(true),
            default_enabled: (i > 0).then_some(true),
            base_order: i as u32,
        });
    }
    let schema = SchemaToml {
        schema: SchemaMeta {
            id: id.clone(),
            name: name.clone(),
            version: "1.0".into(),
            author: "Rime 导入".into(),
            description: format!("从 Rime 方案 {rime_id} 导入"),
        },
        engine: EngineToml {
            r#type: match engine {
                EngineKind::Codetable => "codetable",
                EngineKind::Pinyin => "pinyin",
            },
            codetable: max_code_length.map(|n| CodetableToml {
                max_code_length: n,
                base_sort: "weight",
            }),
            pinyin: (engine == EngineKind::Pinyin).then_some(PinyinToml { scheme: "full" }),
        },
        dictionaries: entries,
    };
    let toml_text = toml::to_string_pretty(&schema).expect("SchemaToml 可序列化");
    out_files.insert(0, (format!("{id}.schema.toml"), toml_text.into_bytes()));
    Some(ConvertedScheme {
        id,
        name,
        engine,
        files: out_files,
    })
}

/// 从主词库出发，按 `import_tables` 递归收集词库文件（根相对路径），去重、防环。
fn collect_dicts(
    main: &str,
    files: &BTreeMap<&str, &SourceFile>,
    schema: &str,
    diags: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![main.to_owned()];
    while let Some(name) = stack.pop() {
        let rel = format!("{name}.dict.yaml");
        if !seen.insert(rel.clone()) {
            continue;
        }
        let Some(f) = files.get(rel.as_str()) else {
            diags.push(Diagnostic::MissingDictionary {
                schema: schema.to_owned(),
                name,
            });
            continue;
        };
        out.push(rel);
        for import in dict_imports(&f.bytes).into_iter().rev() {
            stack.push(import);
        }
    }
    out
}

/// 读 `.dict.yaml` 头部（到 `...` 为止）的 `import_tables`。
fn dict_imports(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let header: String = text
        .lines()
        .take_while(|l| l.trim_end() != "...")
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(v) = serde_yaml::from_str::<serde_yaml::Value>(&header) else {
        return Vec::new();
    };
    v.get("import_tables")
        .and_then(|t| t.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn moodflow_id(rime_id: &str, used: &mut BTreeSet<String>) -> String {
    let base = sanitize(rime_id);
    let base = if base.starts_with("rime_") {
        base
    } else {
        format!("rime_{base}")
    };
    let mut id = base.clone();
    let mut n = 2;
    while !used.insert(id.clone()) {
        id = format!("{base}_{n}");
        n += 1;
    }
    id
}

fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() { "scheme".into() } else { out }
}

#[derive(Serialize)]
struct SchemaToml {
    schema: SchemaMeta,
    engine: EngineToml,
    dictionaries: Vec<DictEntry>,
}

#[derive(Serialize)]
struct SchemaMeta {
    id: String,
    name: String,
    version: String,
    author: String,
    description: String,
}

#[derive(Serialize)]
struct EngineToml {
    r#type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    codetable: Option<CodetableToml>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pinyin: Option<PinyinToml>,
}

#[derive(Serialize)]
struct CodetableToml {
    max_code_length: u64,
    base_sort: &'static str,
}

#[derive(Serialize)]
struct PinyinToml {
    scheme: &'static str,
}

#[derive(Serialize)]
struct DictEntry {
    id: String,
    label: String,
    path: String,
    r#type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_enabled: Option<bool>,
    base_order: u32,
}

// ── 写方案包 ────────────────────────────────────────────

/// 把一套转换好的方案写成 `.wpkg`，交给现有的 [`crate::scheme::import_package`] 安装。
pub fn write_wpkg(scheme: &ConvertedScheme, out: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in &scheme.files {
        zip.start_file(name.as_str(), opts)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}

// ── 文本词库 ────────────────────────────────────────────

/// 把 UTF-8（可带 BOM）或带 BOM 的 UTF-16 文本词库转成 Rime `.dict.yaml` 文本。
///
/// 每行 `词<TAB>编码[<TAB>权重]`；也接受 `编码<TAB>词`（第一列全是 ASCII 字母、第二列含非 ASCII 时自动对调）。
/// 空行与 `#` 注释跳过；格式不对的行计数后跳过。返回 `(dict.yaml 文本, 跳过的行数)`。
pub fn text_to_rime_dict(bytes: &[u8], name: &str) -> Result<(String, usize), RimeImportError> {
    let text = decode_text(bytes)?;
    let mut lines = Vec::new();
    let mut skipped = 0usize;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
        if cols.len() < 2 || cols[0].is_empty() || cols[1].is_empty() {
            skipped += 1;
            continue;
        }
        let (word, code) = if cols[0]
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b' ')
            && !cols[1].is_ascii()
        {
            (cols[1], cols[0])
        } else {
            (cols[0], cols[1])
        };
        let weight = cols.get(2).and_then(|w| w.parse::<i64>().ok());
        lines.push(match weight {
            Some(w) => format!("{word}\t{code}\t{w}"),
            None => format!("{word}\t{code}"),
        });
    }
    if lines.is_empty() {
        return Err(RimeImportError::EmptyDictionary);
    }
    let safe_name = sanitize(name);
    let mut out = format!(
        "# 由 Moodflow 从文本词库转换\n---\nname: {safe_name}\nversion: \"1.0\"\nsort: by_weight\n...\n"
    );
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    Ok((out, skipped))
}

fn decode_text(bytes: &[u8]) -> Result<String, RimeImportError> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).map_err(|_| RimeImportError::UnsupportedEncoding);
    }
    let utf16 = |rest: &[u8], le: bool| -> Result<String, RimeImportError> {
        if !rest.len().is_multiple_of(2) {
            return Err(RimeImportError::UnsupportedEncoding);
        }
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| RimeImportError::UnsupportedEncoding)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| RimeImportError::UnsupportedEncoding)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
#[path = "rime_import_tests.rs"]
mod tests;
