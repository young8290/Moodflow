//! 安装数据的逐文件 SHA-256 清单。
//!
//! 打包时对 `data/` 生成 [`MANIFEST_NAME`]，安装器写入前、Android 首次解包后、或用户排障时用
//! [`verify`] 复核。规则取自 XSIME 的数据打包（`prepare-native-data.py`），见
//! `docs/design/xsime-feature-port-plan.md` §3.4：
//!
//! - 拒收用户库（`*.userdb*`）、`build/`、`sync/`、用户状态文件与符号链接——这些出现在出厂数据里
//!   说明打包环境被污染，应当让构建失败而不是带着发出去。
//! - 扩展名不在白名单内的文件只**警告**（[`Built::warnings`]）：数据生成链会随版本新增产物类型，
//!   不该因此让发版失败；警告由打包脚本打印出来，看到后补进白名单。
//! - 清单路径一律用 `/` 分隔、相对 `data/`，Windows 与 Android 结果一致。

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 清单文件名（位于数据根目录，自身不计入清单）。
pub const MANIFEST_NAME: &str = "manifest.json";
pub const FORMAT_VERSION: u32 = 1;

/// 允许出现在出厂数据里的扩展名（小写，不含点）。
const ALLOWED_EXT: &[&str] = &[
    "toml", "yaml", "yml", "txt", "md", "json", "tsv", "csv", "ttf", "otf", "png", "svg", "ico",
    "octrie", "ocd2", "wdat", "bin", "dat", "lm",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// 相对数据根的路径（`/` 分隔）→ 哈希与大小。
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    Io {
        path: String,
        kind: String,
    },
    /// 出现了不该进出厂数据的文件。
    Rejected {
        path: String,
        reason: &'static str,
    },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io { path, kind } => write!(f, "{path}: {kind}"),
            BuildError::Rejected { path, reason } => write!(f, "{path}: {reason}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// 校验发现的不一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    Missing(String),
    Changed(String),
    /// 清单里没有的多余文件（被篡改或混入）。
    Extra(String),
}

/// [`build`] 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub manifest: Manifest,
    /// 扩展名不在白名单内的文件（仍计入清单）。
    pub warnings: Vec<String>,
}

/// 扫描 `root` 生成清单。遇到被拒收的文件即失败。
pub fn build(root: &Path) -> Result<Built, BuildError> {
    let mut files = BTreeMap::new();
    let mut warnings = Vec::new();
    for (rel, path) in walk(root)? {
        if rel == MANIFEST_NAME {
            continue;
        }
        if !check_allowed(&rel)? {
            warnings.push(rel.clone());
        }
        files.insert(rel.clone(), hash_file(&path, &rel)?);
    }
    Ok(Built {
        manifest: Manifest {
            version: FORMAT_VERSION,
            files,
        },
        warnings,
    })
}

/// 按清单复核 `root`。返回全部不一致（空 = 通过）。
pub fn verify(root: &Path, manifest: &Manifest) -> Result<Vec<Mismatch>, BuildError> {
    let mut out = Vec::new();
    let on_disk: BTreeMap<String, PathBuf> = walk(root)?
        .into_iter()
        .filter(|(rel, _)| rel != MANIFEST_NAME)
        .collect();
    for (rel, want) in &manifest.files {
        match on_disk.get(rel) {
            None => out.push(Mismatch::Missing(rel.clone())),
            Some(path) => {
                let meta = fs::metadata(path).map_err(|e| io_err(rel, &e))?;
                // 先比大小，省掉明显不同时的哈希。
                if meta.len() != want.size || hash_file(path, rel)?.sha256 != want.sha256 {
                    out.push(Mismatch::Changed(rel.clone()));
                }
            }
        }
    }
    for rel in on_disk.keys() {
        if !manifest.files.contains_key(rel) {
            out.push(Mismatch::Extra(rel.clone()));
        }
    }
    Ok(out)
}

impl Manifest {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Manifest 可序列化")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// 读 `root/manifest.json`。
    pub fn load(root: &Path) -> Result<Self, BuildError> {
        let p = root.join(MANIFEST_NAME);
        let text = fs::read_to_string(&p).map_err(|e| io_err(MANIFEST_NAME, &e))?;
        Self::from_json(&text).map_err(|e| BuildError::Io {
            path: MANIFEST_NAME.into(),
            kind: format!("解析失败（第 {} 行）", e.line()),
        })
    }

    /// 写 `root/manifest.json`（先写临时文件再改名）。
    pub fn save(&self, root: &Path) -> Result<(), BuildError> {
        let tmp = root.join(format!("{MANIFEST_NAME}.tmp"));
        fs::write(&tmp, self.to_json()).map_err(|e| io_err(MANIFEST_NAME, &e))?;
        fs::rename(&tmp, root.join(MANIFEST_NAME)).map_err(|e| io_err(MANIFEST_NAME, &e))
    }
}

/// `Err` = 拒收；`Ok(false)` = 扩展名不在白名单（警告）。
fn check_allowed(rel: &str) -> Result<bool, BuildError> {
    let lower = rel.to_ascii_lowercase();
    let reject = |reason| {
        Err(BuildError::Rejected {
            path: rel.to_owned(),
            reason,
        })
    };
    if lower
        .split('/')
        .any(|seg| seg == "build" || seg == "sync" || seg.contains(".userdb"))
    {
        return reject("用户状态或编译缓存不应进出厂数据");
    }
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    if matches!(file, "installation.yaml" | "user.yaml" | "userdata.redb") {
        return reject("用户状态不应进出厂数据");
    }
    let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    Ok(ALLOWED_EXT.contains(&ext))
}

fn walk(root: &Path) -> Result<Vec<(String, PathBuf)>, BuildError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| io_err(&dir.display().to_string(), &e))?;
        for entry in rd {
            let entry = entry.map_err(|e| io_err(&dir.display().to_string(), &e))?;
            let path = entry.path();
            let rel = rel_of(root, &path);
            let ft = entry.file_type().map_err(|e| io_err(&rel, &e))?;
            if ft.is_symlink() {
                return Err(BuildError::Rejected {
                    path: rel,
                    reason: "出厂数据里不允许符号链接",
                });
            }
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                out.push((rel, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn hash_file(path: &Path, rel: &str) -> Result<FileEntry, BuildError> {
    let mut f = fs::File::open(path).map_err(|e| io_err(rel, &e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| io_err(rel, &e))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        h.update(&buf[..n]);
    }
    Ok(FileEntry {
        sha256: h.finalize().iter().map(|b| format!("{b:02x}")).collect(),
        size,
    })
}

fn io_err(path: &str, e: &std::io::Error) -> BuildError {
    BuildError::Io {
        path: path.to_owned(),
        kind: e.kind().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mf-manifest-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("schemas/wubi86")).unwrap();
        fs::write(d.join("config.toml"), "a = 1\n").unwrap();
        fs::write(d.join("schemas/wubi86/main.dict.yaml"), "你\twq\n").unwrap();
        d
    }

    #[test]
    fn build_save_load_verify_round_trip() {
        let d = tree("ok");
        let built = build(&d).unwrap();
        assert!(built.warnings.is_empty());
        let m = built.manifest;
        assert_eq!(m.files.len(), 2);
        assert!(m.files.contains_key("schemas/wubi86/main.dict.yaml"));
        m.save(&d).unwrap();
        let back = Manifest::load(&d).unwrap();
        assert_eq!(back, m);
        assert_eq!(verify(&d, &back).unwrap(), vec![], "清单自身不算多余文件");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn verify_reports_missing_changed_and_extra() {
        let d = tree("diff");
        let m = build(&d).unwrap().manifest;
        fs::write(d.join("config.toml"), "a = 2\n").unwrap();
        fs::remove_file(d.join("schemas/wubi86/main.dict.yaml")).unwrap();
        fs::write(d.join("new.txt"), "x").unwrap();
        let mut got = verify(&d, &m).unwrap();
        got.sort_by_key(|x| format!("{x:?}"));
        assert_eq!(
            got,
            vec![
                Mismatch::Changed("config.toml".into()),
                Mismatch::Extra("new.txt".into()),
                Mismatch::Missing("schemas/wubi86/main.dict.yaml".into()),
            ]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn same_size_change_is_caught_by_hash() {
        let d = tree("same-size");
        let m = build(&d).unwrap().manifest;
        fs::write(d.join("config.toml"), "a = 9\n").unwrap();
        assert_eq!(
            verify(&d, &m).unwrap(),
            vec![Mismatch::Changed("config.toml".into())]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn polluted_data_is_rejected() {
        for bad in ["x.userdb/LOG", "build/x.prism.bin", "user.yaml"] {
            let d = tree("bad");
            let p = d.join(bad);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x").unwrap();
            assert!(
                matches!(build(&d), Err(BuildError::Rejected { .. })),
                "{bad} 应被拒收"
            );
            let _ = fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn unknown_extension_is_a_warning_not_a_failure() {
        let d = tree("ext");
        fs::write(d.join("model.xyz"), "x").unwrap();
        let built = build(&d).unwrap();
        assert_eq!(built.warnings, vec!["model.xyz".to_string()]);
        assert!(built.manifest.files.contains_key("model.xyz"));
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_rejected() {
        let d = tree("link");
        std::os::unix::fs::symlink(d.join("config.toml"), d.join("link.toml")).unwrap();
        assert!(matches!(build(&d), Err(BuildError::Rejected { .. })));
        let _ = fs::remove_dir_all(&d);
    }
}
