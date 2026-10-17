//! 桌宠的文件部分（ADR 0044）：列出、导入、删除宠物包，把精灵图交给小组件。
//!
//! - 内置的“晴晴”在出厂模板目录 `hub_templates/pets/qingqing/`，除了标准精灵图还有 `body.png`（身体蒙版）和
//!   `lines.png`（线条），小组件据此按当前天气给身体上色；
//! - 导入的宠物复制到数据目录 `hub\pets\<编号>\`，只留 `pet.json` 和精灵图两个文件。来源可以是宠物文件夹、
//!   文件夹里的 `pet.json`、宠物包 zip，也可以只是一张精灵图；
//! - Codex（`$CODEX_HOME/pets`，默认 `~/.codex/pets`）和 petdex（`~/.petdex/pets`）装过的宠物会列出来，一键导入。
//!
//! 宠物包是用户自己选的图片，不含个人数据；“删除全部数据”时与模板覆盖一样保留。

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use moodflow_hub_core::domain::pet::{self, Manifest, PetError, SheetFormat};
use moodflow_hub_core::domain::settings::{self, SettingValue};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::error::UiError;
use crate::events::SettingsChanged;
use crate::state::AppState;

/// 宠物包 zip 里最多看这么多个条目
const ZIP_MAX_ENTRIES: usize = 256;
const MANIFEST: &str = "pet.json";

impl From<PetError> for UiError {
    fn from(e: PetError) -> Self {
        UiError::new("pet.invalid", e.copy_key())
    }
}

/// 设置页列表里的一只宠物
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PetInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 内置的晴晴（不能删除，身体随天气变色）
    pub builtin: bool,
    /// 精灵图版本：1 是 9 行，2 是 11 行
    pub version: u8,
}

/// 小组件画宠物要的东西。图片以 data URL 给出（CSP 允许 `img-src data:`）。
#[derive(Debug, Clone, Serialize, Type)]
pub struct PetSheet {
    pub id: String,
    pub builtin: bool,
    pub version: u8,
    /// 标准精灵图
    pub sheet: String,
    /// 内置宠物的身体蒙版与线条，两层叠起来就能按天气给身体上色；导入的宠物没有
    pub body: Option<String>,
    pub lines: Option<String>,
}

/// 别的应用里装过、可以一键导入的宠物
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PetCandidate {
    /// 宠物文件夹，原样交回 `pet_import`
    pub path: String,
    pub id: String,
    pub name: String,
    /// `codex` 或 `petdex`
    pub source: String,
    /// 已经导入过同一个编号
    pub imported: bool,
}

/// 一只读好的宠物：清单与精灵图字节
struct Loaded {
    id: String,
    manifest: Manifest,
    sheet: Vec<u8>,
    format: SheetFormat,
    version: u8,
}

fn pets_dir() -> Result<PathBuf, UiError> {
    Ok(crate::paths::hub_data_dir()
        .map_err(|e| UiError::internal("pet.dir", e))?
        .join("pets"))
}

fn builtin_dir() -> Option<PathBuf> {
    crate::paths::templates_dir().map(|d| d.join("pets").join(pet::BUILTIN_ID))
}

fn read_limited(path: &Path) -> Result<Vec<u8>, PetError> {
    let meta = std::fs::metadata(path).map_err(|_| PetError::NotPet)?;
    if meta.len() > pet::SHEET_MAX_BYTES {
        return Err(PetError::TooLarge);
    }
    std::fs::read(path).map_err(|_| PetError::NotPet)
}

/// 文件夹里的精灵图：先看清单写的文件名，再试 `spritesheet.webp` / `spritesheet.png`。
fn sheet_in(dir: &Path, m: &Manifest) -> Result<Vec<u8>, PetError> {
    let named = m.sheet_file().map(|f| dir.join(f));
    let candidates = named
        .into_iter()
        .chain(["spritesheet.webp", "spritesheet.png"].map(|f| dir.join(f)));
    for p in candidates {
        if p.is_file() {
            return read_limited(&p);
        }
    }
    Err(PetError::NotPet)
}

fn finish(id: String, manifest: Manifest, sheet: Vec<u8>) -> Result<Loaded, PetError> {
    let (format, version) = pet::check_sheet(&sheet)?;
    Ok(Loaded {
        id,
        manifest,
        sheet,
        format,
        version,
    })
}

fn folder_name(p: &Path) -> String {
    p.file_name()
        .or_else(|| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 读一个宠物文件夹。
fn load_dir(dir: &Path) -> Result<Loaded, PetError> {
    let manifest = match std::fs::read_to_string(dir.join(MANIFEST)) {
        Ok(t) => Manifest::parse(&t)?,
        // 没有清单但有精灵图：用文件夹名当编号
        Err(_) => Manifest::default(),
    };
    let sheet = sheet_in(dir, &manifest)?;
    let id = manifest
        .resolved_id(&folder_name(dir))
        .ok_or(PetError::BadManifest)?;
    finish(id, manifest, sheet)
}

/// 读宠物包 zip：找最浅的那个 `pet.json`，精灵图在它旁边；没有清单就取第一张能认出的精灵图。
fn load_zip(path: &Path) -> Result<Loaded, PetError> {
    let file = std::fs::File::open(path).map_err(|_| PetError::NotPet)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| PetError::NotPet)?;
    if zip.len() > ZIP_MAX_ENTRIES {
        return Err(PetError::NotPet);
    }
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let mut read = |name: &str| -> Result<Vec<u8>, PetError> {
        let mut f = zip.by_name(name).map_err(|_| PetError::NotPet)?;
        if f.size() > pet::SHEET_MAX_BYTES {
            return Err(PetError::TooLarge);
        }
        let mut buf = Vec::new();
        f.by_ref()
            .take(pet::SHEET_MAX_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|_| PetError::NotPet)?;
        if buf.len() as u64 > pet::SHEET_MAX_BYTES {
            return Err(PetError::TooLarge);
        }
        Ok(buf)
    };
    let manifest_name = names
        .iter()
        .filter(|n| n.rsplit('/').next() == Some(MANIFEST))
        .min_by_key(|n| n.matches('/').count())
        .cloned();
    let stem = folder_name(path);
    if let Some(mn) = manifest_name {
        let text = String::from_utf8(read(&mn)?).map_err(|_| PetError::BadManifest)?;
        let manifest = Manifest::parse(&text)?;
        let prefix = &mn[..mn.len() - MANIFEST.len()];
        let wanted = manifest
            .sheet_file()
            .into_iter()
            .chain(["spritesheet.webp".into(), "spritesheet.png".into()]);
        for f in wanted {
            let full = format!("{prefix}{f}");
            if names.contains(&full) {
                let sheet = read(&full)?;
                let folder = prefix
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("");
                let id = manifest
                    .resolved_id(if folder.is_empty() { &stem } else { folder })
                    .ok_or(PetError::BadManifest)?;
                return finish(id, manifest, sheet);
            }
        }
        return Err(PetError::NotPet);
    }
    for n in &names {
        let lower = n.to_ascii_lowercase();
        if lower.ends_with(".png") || lower.ends_with(".webp") {
            let sheet = read(n)?;
            if pet::check_sheet(&sheet).is_ok() {
                let id = pet::slugify(&stem).ok_or(PetError::BadManifest)?;
                return finish(id, Manifest::default(), sheet);
            }
        }
    }
    Err(PetError::NotPet)
}

/// 按用户选的路径读宠物：文件夹、`pet.json`、zip 或单独一张精灵图。
fn load_any(path: &Path) -> Result<Loaded, PetError> {
    if path.is_dir() {
        return load_dir(path);
    }
    let name = folder_name(path).to_ascii_lowercase();
    if name == MANIFEST {
        return load_dir(path.parent().ok_or(PetError::NotPet)?);
    }
    if name.ends_with(".zip") {
        return load_zip(path);
    }
    // 单张精灵图：编号取文件名（`spritesheet.*` 就取上一级文件夹名）
    let sheet = read_limited(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = if stem.eq_ignore_ascii_case("spritesheet") {
        path.parent().map(folder_name).unwrap_or_default()
    } else {
        stem
    };
    let id = pet::slugify(&base).ok_or(PetError::BadManifest)?;
    finish(id, Manifest::default(), sheet)
}

fn info(l: &Loaded, builtin: bool) -> PetInfo {
    PetInfo {
        id: l.id.clone(),
        name: l.manifest.name(&l.id),
        description: l.manifest.desc(),
        builtin,
        version: l.version,
    }
}

/// 写进数据目录：先写到临时文件夹，再整个换掉旧的，中途失败不留半只宠物。
fn store(root: &Path, l: &Loaded) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let tmp = root.join(format!(".{}.tmp", l.id));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let file = format!("spritesheet.{}", l.format.ext());
    let manifest = Manifest {
        id: Some(l.id.clone()),
        display_name: Some(l.manifest.name(&l.id)),
        description: Some(l.manifest.desc()).filter(|d| !d.is_empty()),
        spritesheet_path: Some(file.clone()),
    };
    std::fs::write(
        tmp.join(MANIFEST),
        serde_json::to_string_pretty(&manifest).map_err(std::io::Error::other)?,
    )?;
    std::fs::write(tmp.join(&file), &l.sheet)?;
    let dest = root.join(&l.id);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&tmp, &dest)
}

fn data_url(fmt: SheetFormat, bytes: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        fmt.mime(),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn load_builtin() -> Result<Loaded, PetError> {
    load_dir(&builtin_dir().ok_or(PetError::Unknown)?)
}

/// 已导入的宠物（数据目录里读得出来的；读不出来的跳过并记日志）
fn imported(root: &Path) -> Vec<Loaded> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Loaded> = rd
        .flatten()
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| match load_dir(&e.path()) {
            Ok(l) if l.id != pet::BUILTIN_ID => Some(l),
            Ok(_) => None,
            Err(err) => {
                crate::hlog!("跳过读不出来的宠物：{err}");
                None
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// 内置的晴晴加上导入的宠物。
#[tauri::command]
#[specta::specta]
pub fn pet_list() -> Result<Vec<PetInfo>, UiError> {
    let mut out = Vec::new();
    match load_builtin() {
        Ok(b) => out.push(info(&b, true)),
        Err(e) => crate::hlog!("读取内置宠物失败：{e}"),
    }
    out.extend(imported(&pets_dir()?).iter().map(|l| info(l, false)));
    Ok(out)
}

/// 小组件要画的那只宠物；找不到或读不出来就用内置的晴晴。
#[tauri::command]
#[specta::specta]
pub fn pet_sheet(id: String) -> Result<PetSheet, UiError> {
    if id != pet::BUILTIN_ID && pet::valid_id(&id) {
        match load_dir(&pets_dir()?.join(&id)) {
            Ok(l) => {
                return Ok(PetSheet {
                    id: l.id,
                    builtin: false,
                    version: l.version,
                    sheet: data_url(l.format, &l.sheet),
                    body: None,
                    lines: None,
                });
            }
            Err(e) => crate::hlog!("宠物 {id} 读不出来，换回晴晴：{e}"),
        }
    }
    let dir = builtin_dir().ok_or(PetError::Unknown)?;
    let b = load_dir(&dir)?;
    let layer = |f: &str| -> Option<String> {
        let bytes = std::fs::read(dir.join(f)).ok()?;
        let (fmt, _) = pet::check_sheet(&bytes).ok()?;
        Some(data_url(fmt, &bytes))
    };
    Ok(PetSheet {
        id: b.id,
        builtin: true,
        version: b.version,
        sheet: data_url(b.format, &b.sheet),
        body: layer("body.png"),
        lines: layer("lines.png"),
    })
}

/// 导入宠物（设置页“导入宠物…”或一键导入）。同编号的旧宠物被替换；不能顶替内置的晴晴。
#[tauri::command]
#[specta::specta]
pub fn pet_import(path: String) -> Result<PetInfo, UiError> {
    let mut l = load_any(Path::new(&path))?;
    if l.id == pet::BUILTIN_ID {
        l.id = format!("{}-2", pet::BUILTIN_ID);
    }
    store(&pets_dir()?, &l).map_err(|e| UiError::internal("pet.store", e))?;
    crate::hlog!("已导入宠物 {}", l.id);
    Ok(info(&l, false))
}

fn codex_pets() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .map(|d| d.join("pets"))
}

/// Codex 与 petdex 里装过的宠物。
#[tauri::command]
#[specta::specta]
pub fn pet_discover() -> Result<Vec<PetCandidate>, UiError> {
    let have: Vec<String> = imported(&pets_dir()?).into_iter().map(|l| l.id).collect();
    let sources = [
        ("codex", codex_pets()),
        (
            "petdex",
            dirs::home_dir().map(|h| h.join(".petdex").join("pets")),
        ),
    ];
    let mut out = Vec::new();
    for (source, dir) in sources {
        let Some(Ok(rd)) = dir.map(std::fs::read_dir) else {
            continue;
        };
        for e in rd.flatten().filter(|e| e.path().is_dir()) {
            if let Ok(l) = load_dir(&e.path()) {
                out.push(PetCandidate {
                    path: e.path().to_string_lossy().into_owned(),
                    imported: have.contains(&l.id),
                    name: l.manifest.name(&l.id),
                    id: l.id,
                    source: source.into(),
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.source, &a.id).cmp(&(&b.source, &b.id)));
    Ok(out)
}

/// 删掉一只导入的宠物；正在用的话换回晴晴。
#[tauri::command]
#[specta::specta]
pub fn pet_remove(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), UiError> {
    if id == pet::BUILTIN_ID {
        return Err(PetError::Builtin.into());
    }
    if !pet::valid_id(&id) {
        return Err(PetError::Unknown.into());
    }
    let dir = pets_dir()?.join(&id);
    if !dir.is_dir() {
        return Err(PetError::Unknown.into());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| UiError::internal("pet.remove", e))?;
    let current = settings::get(&state.db(), "pet.id")?;
    if current.as_str() == Some(id.as_str()) {
        let builtin = SettingValue::from(pet::BUILTIN_ID);
        state
            .writer()
            .write_sync(|db| settings::set(db, "pet.id", &builtin))?;
        if let Err(e) = (SettingsChanged {
            key: "pet.id".into(),
        })
        .emit(&app)
        {
            crate::hlog!("推送 settings:changed 失败：{e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("mf-pets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn builtin_sheet() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../hub_templates/pets/qingqing/spritesheet.png"),
        )
        .unwrap()
    }

    #[test]
    fn imports_codex_folder_and_stores_two_files() {
        let src = tmp("src");
        let pet = src.0.join("Boba");
        std::fs::create_dir_all(&pet).unwrap();
        std::fs::write(
            pet.join("pet.json"),
            r#"{"id":"boba","displayName":"Boba","description":"tea","spritesheetPath":"spritesheet.png"}"#,
        )
        .unwrap();
        std::fs::write(pet.join("spritesheet.png"), builtin_sheet()).unwrap();
        let l = load_any(&pet).unwrap();
        assert_eq!((l.id.as_str(), l.version), ("boba", 1));
        let root = tmp("root");
        store(&root.0, &l).unwrap();
        let back = load_dir(&root.0.join("boba")).unwrap();
        assert_eq!(back.manifest.name("boba"), "Boba");
        let mut files: Vec<_> = std::fs::read_dir(root.0.join("boba"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files, ["pet.json", "spritesheet.png"]);
        // 选 pet.json 本身也行
        assert_eq!(load_any(&pet.join("pet.json")).unwrap().id, "boba");
    }

    #[test]
    fn imports_zip_with_nested_folder_and_bare_sheet() {
        let src = tmp("zip");
        let zpath = src.0.join("Mochi Pack.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zpath).unwrap());
            let opt = zip::write::SimpleFileOptions::default();
            z.start_file("mochi/pet.json", opt).unwrap();
            std::io::Write::write_all(&mut z, br#"{"displayName":"Mochi"}"#).unwrap();
            z.start_file("mochi/spritesheet.png", opt).unwrap();
            std::io::Write::write_all(&mut z, &builtin_sheet()).unwrap();
            z.finish().unwrap();
        }
        assert_eq!(load_any(&zpath).unwrap().id, "mochi");
        let bare = src.0.join("Cat Sheet.png");
        std::fs::write(&bare, builtin_sheet()).unwrap();
        assert_eq!(load_any(&bare).unwrap().id, "cat-sheet");
    }

    #[test]
    fn rejects_non_pets() {
        let src = tmp("bad");
        std::fs::write(src.0.join("pet.json"), "{}").unwrap();
        assert!(matches!(load_any(&src.0), Err(PetError::NotPet)));
        std::fs::write(src.0.join("spritesheet.png"), b"\x89PNG\r\n\x1a\nnope").unwrap();
        assert!(matches!(load_any(&src.0), Err(PetError::BadSheet)));
    }
}
