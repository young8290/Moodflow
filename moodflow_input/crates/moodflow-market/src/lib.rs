//! moodflow-market：插件市场客户端（设计见 `docs/design/plugin-system.md` §7，信任模型见 ADR 0054）。
//!
//! 市场是一个 Git 仓库里的 `index.json` 加一份 Ed25519 签名。本 crate **不联网**：取索引、下载包由宿主做，
//! 这里只处理字节：
//!
//! 1. [`verify_index`]：验签后才解析索引；客户端内置公钥。
//! 2. [`Index::updates`] / [`Index::revoked_installed`]：可用更新与需要停用的已装版本。
//! 3. [`inspect_package`]：包的 SHA-256 与大小必须与签名索引一致；包内清单合法且与索引同 id、同版本；
//!    解压安全（路径穿越、符号链接、条目数与总量）；内容包不得带出厂安全资产。
//! 4. [`install`]：写到临时目录再改名，按版本分目录，旧版本保留可回滚。

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use moodflow_plugin_api::{
    Capability, Kind, Manifest, PROTECTED_FILES, Platform, Version, added_capabilities,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const INDEX_FORMAT: u32 = 1;
/// 单个插件包的大小上限。
pub const MAX_PACKAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub generated_at: String,
    pub plugins: Vec<Listing>,
    #[serde(default)]
    pub revoked: Vec<Revoked>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub author: String,
    /// 源码仓库地址（审核依据）。
    pub repo: String,
    #[serde(default)]
    pub description: String,
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub min_host_version: String,
    pub platforms: Vec<Platform>,
}

/// 撤回：`version` 为空表示该插件所有版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoked {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketError {
    BadSignature,
    BadIndex(String),
    UnsupportedIndex(u32),
    /// 包的哈希或大小与签名索引不一致。
    HashMismatch,
    TooLarge,
    Zip(String),
    UnsafePath(String),
    Symlink(String),
    TooManyEntries,
    MissingManifest,
    Manifest(String),
    /// 包内清单的 id 或版本与索引不一致。
    IdentityMismatch,
    MissingEntry(String),
    /// 内容包里出现出厂安全资产。
    ProtectedFile(String),
    Io(String),
}

impl std::fmt::Display for MarketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for MarketError {}

/// 验签后解析索引。签名是对 `index_json` 原始字节的 Ed25519 签名（十六进制）。
pub fn verify_index(
    index_json: &[u8],
    signature_hex: &str,
    public_key: &[u8; 32],
) -> Result<Index, MarketError> {
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| MarketError::BadSignature)?;
    let sig_bytes: [u8; 64] = unhex(signature_hex.trim())
        .and_then(|v| v.try_into().ok())
        .ok_or(MarketError::BadSignature)?;
    key.verify(index_json, &Signature::from_bytes(&sig_bytes))
        .map_err(|_| MarketError::BadSignature)?;
    let index: Index =
        serde_json::from_slice(index_json).map_err(|e| MarketError::BadIndex(e.to_string()))?;
    if index.version != INDEX_FORMAT {
        return Err(MarketError::UnsupportedIndex(index.version));
    }
    Ok(index)
}

impl Index {
    pub fn listing(&self, id: &str) -> Option<&Listing> {
        self.plugins.iter().find(|l| l.id == id)
    }

    pub fn is_revoked(&self, id: &str, version: &str) -> bool {
        self.revoked
            .iter()
            .any(|r| r.id == id && r.version.as_deref().is_none_or(|v| v == version))
    }

    /// 该插件在当前宿主与平台上可用的最新版本（跳过已撤回的）。
    pub fn latest_compatible(
        &self,
        id: &str,
        host_version: &str,
        platform: Platform,
    ) -> Option<&Release> {
        let host = Version::parse(host_version)?;
        self.listing(id)?
            .releases
            .iter()
            .filter(|r| r.platforms.contains(&platform))
            .filter(|r| !self.is_revoked(id, &r.version))
            .filter(|r| {
                Version::parse(&r.min_host_version).is_some_and(|need| {
                    (host.major, host.minor, host.patch) >= (need.major, need.minor, need.patch)
                })
            })
            .filter_map(|r| Version::parse(&r.version).map(|v| (v, r)))
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, r)| r)
    }

    /// 已装插件 `(id, version)` 的可用更新。
    pub fn updates(
        &self,
        installed: &[(String, String)],
        host_version: &str,
        platform: Platform,
    ) -> Vec<Update> {
        installed
            .iter()
            .filter_map(|(id, ver)| {
                let cur = Version::parse(ver)?;
                let next = self.latest_compatible(id, host_version, platform)?;
                (Version::parse(&next.version)? > cur).then(|| Update {
                    id: id.clone(),
                    from: ver.clone(),
                    to: next.version.clone(),
                })
            })
            .collect()
    }

    /// 已装版本中被撤回、需要停用的。
    pub fn revoked_installed(&self, installed: &[(String, String)]) -> Vec<(String, String)> {
        installed
            .iter()
            .filter(|(id, v)| self.is_revoked(id, v))
            .cloned()
            .collect()
    }
}

/// 检查通过、待安装的包。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub manifest: Manifest,
    pub files: Vec<(String, Vec<u8>)>,
    /// 相对已装版本新增的能力；非空时安装前必须重新征得用户同意。
    pub added_capabilities: Vec<Capability>,
}

/// 检查下载到的包。`installed` 是同一插件当前已装版本的清单（首次安装为 `None`）。
pub fn inspect_package(
    bytes: &[u8],
    listing_id: &str,
    release: &Release,
    host_version: &str,
    installed: Option<&Manifest>,
) -> Result<InstallPlan, MarketError> {
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(MarketError::TooLarge);
    }
    if bytes.len() as u64 != release.size
        || hex(&Sha256::digest(bytes)) != release.sha256.to_ascii_lowercase()
    {
        return Err(MarketError::HashMismatch);
    }
    let files = read_zip(bytes)?;
    let manifest_text = files
        .iter()
        .find(|(p, _)| p == "manifest.toml")
        .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
        .ok_or(MarketError::MissingManifest)?;
    let manifest = Manifest::from_toml(&manifest_text, host_version)
        .map_err(|e| MarketError::Manifest(e.to_string()))?;
    if manifest.id != listing_id || manifest.version != release.version {
        return Err(MarketError::IdentityMismatch);
    }
    match &manifest.kind {
        Kind::Wasm { entry } => {
            if !files.iter().any(|(p, _)| p == entry) {
                return Err(MarketError::MissingEntry(entry.clone()));
            }
        }
        Kind::Content { .. } => {
            if let Some((p, _)) = files.iter().find(|(p, _)| {
                let name = p.rsplit('/').next().unwrap_or(p);
                PROTECTED_FILES.contains(&name)
            }) {
                return Err(MarketError::ProtectedFile(p.clone()));
            }
        }
    }
    if let Some(s) = &manifest.contributes.settings
        && !files.iter().any(|(p, _)| p == s)
    {
        return Err(MarketError::MissingEntry(s.clone()));
    }
    let added_capabilities = installed
        .map(|old| added_capabilities(old, &manifest))
        .unwrap_or_else(|| manifest.capabilities.list().into_iter().collect());
    Ok(InstallPlan {
        manifest,
        files,
        added_capabilities,
    })
}

/// 安装到 `plugins_dir/<id>/<version>/`：先写临时目录再改名，同版本已存在则先移开再替换。
pub fn install(plan: &InstallPlan, plugins_dir: &Path) -> Result<PathBuf, MarketError> {
    let base = plugins_dir.join(&plan.manifest.id);
    let target = base.join(&plan.manifest.version);
    let tmp = base.join(format!(".{}.tmp", plan.manifest.version));
    let io = |e: std::io::Error| MarketError::Io(e.kind().to_string());
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(io)?;
    for (rel, bytes) in &plan.files {
        let dest = tmp.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&dest, bytes).map_err(io)?;
    }
    if target.exists() {
        let old = base.join(format!(".{}.old", plan.manifest.version));
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(&target, &old).map_err(io)?;
        std::fs::rename(&tmp, &target).map_err(io)?;
        let _ = std::fs::remove_dir_all(&old);
    } else {
        std::fs::rename(&tmp, &target).map_err(io)?;
    }
    Ok(target)
}

/// 已安装的 `(id, 最新版本)`。忽略临时目录与解析不了的版本目录。
pub fn installed(plugins_dir: &Path) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(plugins_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        let Ok(versions) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        let best = versions
            .flatten()
            .filter_map(|v| {
                let name = v.file_name().to_string_lossy().into_owned();
                Version::parse(&name).map(|parsed| (parsed, name))
            })
            .max_by(|a, b| a.0.cmp(&b.0));
        if let Some((_, v)) = best {
            out.push((id, v));
        }
    }
    out.sort();
    out
}

/// 卸载插件的全部版本（插件数据由宿主另行询问是否删除）。
pub fn uninstall(plugins_dir: &Path, id: &str) -> Result<(), MarketError> {
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return Err(MarketError::UnsafePath(id.to_owned()));
    }
    match std::fs::remove_dir_all(plugins_dir.join(id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(MarketError::Io(e.kind().to_string())),
    }
}

fn read_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, MarketError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| MarketError::Zip(e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(MarketError::TooManyEntries);
    }
    let mut budget = MAX_PACKAGE_BYTES * 4;
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for i in 0..archive.len() {
        let mut e = archive
            .by_index(i)
            .map_err(|e| MarketError::Zip(e.to_string()))?;
        if e.is_dir() {
            continue;
        }
        let name = e.name().to_owned();
        if e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(MarketError::Symlink(name));
        }
        if e.enclosed_name().is_none()
            || name.starts_with('/')
            || name.contains('\\')
            || name.contains(':')
            || !seen.insert(name.to_ascii_lowercase())
        {
            return Err(MarketError::UnsafePath(name));
        }
        let mut buf = Vec::new();
        (&mut e)
            .take(budget + 1)
            .read_to_end(&mut buf)
            .map_err(|e| MarketError::Zip(e.to_string()))?;
        if buf.len() as u64 > budget {
            return Err(MarketError::TooLarge);
        }
        budget -= buf.len() as u64;
        files.push((name, buf));
    }
    Ok(files)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests;
