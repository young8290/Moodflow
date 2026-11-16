//! 在内存受限的宿主上预览 Rime ZIP，并按选中的方案安装。
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, ensure};
use tempfile::{NamedTempFile, tempdir_in};

use crate::bundle::validate_entry_rel;
use crate::rime_import::{
    Conversion, ConvertedScheme, Diagnostic, RimeImportError, convert_with_budget,
    read_zip_with_budget,
};

pub const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 48 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 256 * 1024;

pub fn read_archive(path: &Path) -> anyhow::Result<Conversion> {
    let mut raw = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut raw))
        .context("无法读取 ZIP")?;
    ensure!(
        raw.len() as u64 <= MAX_SOURCE_BYTES,
        "ZIP 超过 32 MiB，建议拆成较小的方案包"
    );
    let (files, sha, diagnostics) =
        read_zip_with_budget(&raw, MAX_EXPANDED_BYTES).map_err(user_error)?;
    drop(raw);
    let schemas = files
        .iter()
        .filter(|file| file.path.ends_with(".schema.yaml"));
    ensure!(schemas.clone().count() <= 64, "ZIP 内最多允许 64 套方案");
    ensure!(
        schemas
            .into_iter()
            .all(|file| file.bytes.len() <= MAX_HEADER_BYTES),
        "单个方案配置超过 256 KiB"
    );
    convert_with_budget(&files, sha, diagnostics, MAX_EXPANDED_BYTES as usize).map_err(user_error)
}

fn user_error(error: RimeImportError) -> anyhow::Error {
    use RimeImportError::*;
    anyhow::anyhow!(match error {
        Zip(_) => "无法读取 ZIP，文件可能损坏或使用了不支持的压缩格式",
        UnsafePath(_) | Symlink(_) => "ZIP 含重复或非法路径，或含符号链接",
        TooManyEntries => "ZIP 文件条目过多",
        TooLarge => "解压或转换后的词库超过 48 MiB，建议拆成较小的方案包",
        NoSchema => "ZIP 中没有 .schema.yaml 方案文件",
        MultipleRoots(_) => "请把 .schema.yaml 文件放在同一个目录下",
        NothingConvertible => "没有可转换的方案，请检查翻译器和对应词库",
        _ => "无法解析 Rime 方案",
    })
}

pub fn diagnostic_text(diagnostic: &Diagnostic) -> String {
    use Diagnostic::*;
    match diagnostic {
        IgnoredFile { path } => format!("跳过私有状态或无关文件：{path}"),
        UnsupportedComponent { path, .. } => format!("不支持的组件：{path}"),
        LuaIgnored { schema } => format!("{schema}：Lua 逻辑不会导入"),
        SpellerAlgebraIgnored { schema } => format!("{schema}：拼写运算不会导入"),
        FiltersIgnored { schema } => format!("{schema}：简繁、Emoji 等 Rime 过滤器不会导入"),
        AssumedMaxCodeLength { schema } => format!("{schema}：未声明码长，按四码处理"),
        MissingDictionary { schema, name } => format!("{schema}：ZIP 内缺少词库 {name}"),
        SchemeSkipped { schema, reason } => format!("{schema}：已跳过，{reason}"),
        IdRenamed { from, to } => format!("方案标识已从 {from} 改为 {to}"),
    }
}

/// 同一来源的词库目录先整体发布，入口 TOML 最后以不可覆盖的方式发布。
/// 如果进程在两步之间退出，相同 ZIP 重试可复用已经完整发布的词库目录。
pub fn install(
    conversion: &Conversion,
    selected_id: &str,
    user_schemas: &Path,
) -> anyhow::Result<()> {
    let selected = conversion
        .schemes
        .iter()
        .find(|scheme| scheme.id == selected_id)
        .context("预览中没有这个方案，请重新选取 ZIP")?;
    let id = &selected.id;
    let sha = &conversion.source_sha256;
    ensure!(
        id.starts_with("rime_")
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "方案标识过长或无效"
    );
    ensure!(
        sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "无效的来源标识"
    );
    let descriptor = format!("{id}.schema.toml");
    let schema_path = user_schemas.join(&descriptor);
    ensure!(
        !schema_path.exists(),
        "同名方案已安装，请使用不同的方案标识"
    );
    let prefix = format!("rime-imports-v1/{sha}");
    let dest_root = user_schemas.join(&prefix);
    let dest = dest_root.join(id);
    fs::create_dir_all(&dest_root).context("无法创建用户词库目录")?;
    let schema_text = rewrite_dictionary_paths(selected, &descriptor, &prefix)?;
    let mut schema = NamedTempFile::new_in(user_schemas).context("无法暂存方案")?;
    schema
        .write_all(schema_text.as_bytes())
        .context("无法写入方案")?;
    schema.as_file().sync_all().context("无法保存方案")?;
    if !dest.is_dir() {
        let temp = tempdir_in(&dest_root).context("无法暂存词库")?;
        let mut has_entries = false;
        for (path, bytes) in &selected.files {
            if path == &descriptor {
                continue;
            }
            let rel = validate_entry_rel(path, &format!("{id}/")).context("词库路径无效")?;
            let target = temp.path().join(rel);
            fs::create_dir_all(target.parent().context("词库路径无效")?)
                .context("无法创建词库子目录")?;
            has_entries |= write_dictionary(bytes, &target)
                .with_context(|| format!("词库 {path} 无法导入"))?;
        }
        ensure!(has_entries, "方案的词库没有可用条目");
        fs::rename(temp.path(), &dest).context("无法发布词库")?;
    }
    schema
        .persist_noclobber(schema_path)
        .context("无法发布方案，可能已存在同名方案")?;
    Ok(())
}

fn rewrite_dictionary_paths(
    selected: &ConvertedScheme,
    descriptor: &str,
    prefix: &str,
) -> anyhow::Result<String> {
    let (_, bytes) = selected
        .files
        .iter()
        .find(|(path, _)| path == descriptor)
        .context("方案缺少配置")?;
    let mut config: toml::Value = toml::from_str(std::str::from_utf8(bytes)?)?;
    let dictionaries = config
        .get_mut("dictionaries")
        .and_then(toml::Value::as_array_mut)
        .context("方案缺少词库配置")?;
    for entry in dictionaries {
        let path = entry.get_mut("path").context("词库缺少路径")?;
        let relative = path.as_str().context("词库路径无效")?;
        *path = toml::Value::String(format!("{prefix}/{relative}"));
    }
    Ok(toml::to_string_pretty(&config)?)
}

/// 转换器已经把 import_tables 展开成独立的词库条目。移除原声明，避免拼音引擎
/// 启动时二次展开，以及让源词库的相对路径引用已导入目录以外的文件。
fn write_dictionary(bytes: &[u8], target: &Path) -> anyhow::Result<bool> {
    let text = std::str::from_utf8(bytes).context("词库必须是 UTF-8")?;
    let text = moodflow_utils::text::normalize_input(text);
    let end = text
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, *offset, line.trim_end()))
        })
        .take_while(|(start, _, _)| *start <= MAX_HEADER_BYTES)
        .find(|(_, _, line)| *line == "...");
    let (end_header, start_body, _) = end.context("缺少 YAML 结束行或词库头部过长")?;
    let header = &text[..end_header];
    let mut metadata: serde_yaml::Mapping =
        serde_yaml::from_str(header).context("词库头部无法解析")?;
    metadata.remove("import_tables");
    let body = &text[start_body..];
    let mut file = File::create(target)?;
    file.write_all(b"---\n")?;
    serde_yaml::to_writer(&mut file, &metadata)?;
    file.write_all(b"...\n")?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    Ok(body.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && line
                .split_once('\t')
                .is_some_and(|(word, code)| !word.is_empty() && !code.trim().is_empty())
    }))
}
