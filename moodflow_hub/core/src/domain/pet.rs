//! 桌宠（ADR 0044）：小组件的身体是一张 Codex 桌宠格式的精灵图，内置“晴晴”，也能直接导入 Codex / petdex 的宠物包。
//!
//! 宠物包就是一个文件夹：`pet.json`（`id`、`displayName`、`description`、`spritesheetPath`）加一张精灵图
//! （`spritesheet.webp` 或 `.png`）。精灵图 8 列，每格 192 × 208，9 行（v1）或 11 行（v2），可以整体等比缩放；
//! 行的含义见前端 `windows/widget/pet.ts`。这里只做不碰文件系统的部分：清单解析、编号整理、看图片头认尺寸。

use serde::{Deserialize, Serialize};

pub const COLUMNS: u32 = 8;
pub const CELL_W: u32 = 192;
pub const CELL_H: u32 = 208;
/// 内置宠物的编号（设置 `pet.id` 的出厂值）
pub const BUILTIN_ID: &str = "qingqing";
/// 编号最长几个字符
pub const ID_MAX_CHARS: usize = 48;
/// 名字、说明在界面上只显示这么长
pub const NAME_MAX_CHARS: usize = 40;
pub const DESC_MAX_CHARS: usize = 200;
/// 精灵图文件的上限
pub const SHEET_MAX_BYTES: u64 = 24 * 1024 * 1024;

/// 宠物编号：小写 ASCII 字母、数字、`-`、`_`，1–48 个字符（也是数据目录下的文件夹名）。
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= ID_MAX_CHARS
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

/// 把任意字符串整理成合法编号：转小写，其余字符换成 `-`，去掉首尾的 `-`；整理完是空串时返回 `None`。
pub fn slugify(s: &str) -> Option<String> {
    let mut out = String::new();
    for c in s.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(ID_MAX_CHARS).collect();
    let out = out.trim_end_matches('-').to_string();
    valid_id(&out).then_some(out)
}

/// `pet.json`。Codex 的字段名是驼峰；别的字段忽略。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub spritesheet_path: Option<String>,
}

fn clip(s: Option<&str>, max: usize) -> String {
    let s = s.unwrap_or("").trim();
    s.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, PetError> {
        serde_json::from_str(text).map_err(|_| PetError::BadManifest)
    }

    /// 整理后的编号：`id` 不合法时用名字或文件夹名整理出来。
    pub fn resolved_id(&self, folder: &str) -> Option<String> {
        self.id
            .as_deref()
            .and_then(slugify)
            .or_else(|| self.display_name.as_deref().and_then(slugify))
            .or_else(|| slugify(folder))
    }

    pub fn name(&self, id: &str) -> String {
        let n = clip(self.display_name.as_deref(), NAME_MAX_CHARS);
        if n.is_empty() { id.to_string() } else { n }
    }

    pub fn desc(&self) -> String {
        clip(self.description.as_deref(), DESC_MAX_CHARS)
    }

    /// 清单里写的精灵图文件名：只认同一文件夹里的文件（去掉 `./`，不许有路径分隔符和 `..`）。
    pub fn sheet_file(&self) -> Option<String> {
        let p = self.spritesheet_path.as_deref()?.trim();
        let p = p.strip_prefix("./").unwrap_or(p);
        (!p.is_empty() && !p.contains(['/', '\\']) && p != ".." && p != ".").then(|| p.to_string())
    }
}

/// 精灵图的图片格式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum SheetFormat {
    Png,
    Webp,
}

impl SheetFormat {
    pub fn mime(self) -> &'static str {
        match self {
            SheetFormat::Png => "image/png",
            SheetFormat::Webp => "image/webp",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            SheetFormat::Png => "png",
            SheetFormat::Webp => "webp",
        }
    }
}

/// 只读文件头认出格式与宽高，不解码像素。
pub fn image_size(bytes: &[u8]) -> Option<(SheetFormat, u32, u32)> {
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let le24 = |b: &[u8]| u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16;
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR" {
        return Some((SheetFormat::Png, be(&bytes[16..20]), be(&bytes[20..24])));
    }
    if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let chunk = &bytes[12..16];
        let d = &bytes[20..];
        return match chunk {
            // 有损：帧头 3 字节 + 起始码 9d 01 2a，之后是 14 位宽高
            b"VP8 " if d.len() >= 10 && d[3..6] == [0x9d, 0x01, 0x2a] => Some((
                SheetFormat::Webp,
                u32::from(u16::from_le_bytes([d[6], d[7]]) & 0x3fff),
                u32::from(u16::from_le_bytes([d[8], d[9]]) & 0x3fff),
            )),
            // 无损：签名 0x2f，之后 14 位宽 − 1、14 位高 − 1
            b"VP8L" if d.len() >= 5 && d[0] == 0x2f => {
                let b = u32::from_le_bytes([d[1], d[2], d[3], d[4]]);
                Some((
                    SheetFormat::Webp,
                    (b & 0x3fff) + 1,
                    ((b >> 14) & 0x3fff) + 1,
                ))
            }
            // 扩展格式：画布宽高各 24 位、减 1
            b"VP8X" if d.len() >= 10 => {
                Some((SheetFormat::Webp, le24(&d[4..7]) + 1, le24(&d[7..10]) + 1))
            }
            _ => None,
        };
    }
    None
}

/// 认出精灵图是 v1（9 行）还是 v2（11 行）；允许整体等比缩放，但缩放后每格仍须落在整像素上。
pub fn atlas_version(width: u32, height: u32) -> Option<u8> {
    for (version, rows) in [(1u8, 9u64), (2, 11)] {
        let (w, h) = (u64::from(width), u64::from(height));
        if w == 0 || h == 0 || w % u64::from(COLUMNS) != 0 || h % rows != 0 {
            continue;
        }
        // 宽高比与标准尺寸一致：w / (8·192) == h / (rows·208)
        if w * rows * u64::from(CELL_H) == h * u64::from(COLUMNS) * u64::from(CELL_W) {
            return Some(version);
        }
    }
    None
}

/// 导入、读取宠物时的错误（界面文案键 `pet.error.*`）
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PetError {
    #[error("找不到 pet.json 或精灵图")]
    NotPet,
    #[error("pet.json 读不懂")]
    BadManifest,
    #[error("精灵图不是 PNG / WebP，或尺寸不是 8 列 × 9 行（或 11 行）的 192 × 208 格")]
    BadSheet,
    #[error("精灵图太大")]
    TooLarge,
    #[error("内置宠物不能删除")]
    Builtin,
    #[error("没有这只宠物")]
    Unknown,
}

impl PetError {
    pub fn copy_key(&self) -> &'static str {
        match self {
            PetError::NotPet => "pet.error.not_pet",
            PetError::BadManifest => "pet.error.bad_manifest",
            PetError::BadSheet => "pet.error.bad_sheet",
            PetError::TooLarge => "pet.error.too_large",
            PetError::Builtin => "pet.error.builtin",
            PetError::Unknown => "pet.error.unknown",
        }
    }
}

/// 检查精灵图：格式、大小、行列。返回格式与版本。
pub fn check_sheet(bytes: &[u8]) -> Result<(SheetFormat, u8), PetError> {
    if bytes.len() as u64 > SHEET_MAX_BYTES {
        return Err(PetError::TooLarge);
    }
    let (fmt, w, h) = image_size(bytes).ok_or(PetError::BadSheet)?;
    let v = atlas_version(w, h).ok_or(PetError::BadSheet)?;
    Ok((fmt, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    fn webp_vp8x(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\x10\0\0\0".to_vec();
        b.extend(&(w - 1).to_le_bytes()[..3]);
        b.extend(&(h - 1).to_le_bytes()[..3]);
        b
    }

    fn webp_vp8l(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        let bits = (w - 1) | (h - 1) << 14;
        b.extend(bits.to_le_bytes());
        b.extend([0; 8]);
        b
    }

    #[test]
    fn ids_are_slugs() {
        assert!(valid_id("boba") && valid_id("my_pet-2"));
        assert!(
            !valid_id("") && !valid_id("Boba") && !valid_id("a/b") && !valid_id(&"a".repeat(49))
        );
        assert_eq!(slugify("  Boba Tea! ").as_deref(), Some("boba-tea"));
        assert_eq!(slugify("晴晴"), None);
        assert_eq!(slugify("../../etc").as_deref(), Some("etc"));
    }

    #[test]
    fn manifest_fields_and_fallbacks() {
        let m = Manifest::parse(
            r#"{"id":"Boba","displayName":"  Boba ","description":"tea","spritesheetPath":"./spritesheet.webp","extra":1}"#,
        )
        .unwrap();
        assert_eq!(m.resolved_id("x").as_deref(), Some("boba"));
        assert_eq!(m.name("boba"), "Boba");
        assert_eq!(m.sheet_file().as_deref(), Some("spritesheet.webp"));
        let m = Manifest::parse(r#"{"displayName":"猫","spritesheetPath":"../x.png"}"#).unwrap();
        assert_eq!(m.resolved_id("cat-pet").as_deref(), Some("cat-pet"));
        assert_eq!(m.sheet_file(), None, "不许指到文件夹外面");
        assert_eq!(Manifest::parse("not json"), Err(PetError::BadManifest));
    }

    #[test]
    fn reads_sizes_from_headers() {
        assert_eq!(
            image_size(&png(1536, 1872)),
            Some((SheetFormat::Png, 1536, 1872))
        );
        assert_eq!(
            image_size(&webp_vp8x(1536, 2288)),
            Some((SheetFormat::Webp, 1536, 2288))
        );
        assert_eq!(
            image_size(&webp_vp8l(768, 936)),
            Some((SheetFormat::Webp, 768, 936))
        );
        assert_eq!(
            image_size(b"GIF89a....................................."),
            None
        );
    }

    #[test]
    fn atlas_versions_and_scaling() {
        assert_eq!(atlas_version(1536, 1872), Some(1));
        assert_eq!(atlas_version(1536, 2288), Some(2));
        assert_eq!(atlas_version(768, 936), Some(1), "缩成一半也认");
        assert_eq!(atlas_version(1536, 1536), None);
        assert_eq!(atlas_version(1000, 1872), None);
        assert_eq!(check_sheet(&png(1536, 1872)), Ok((SheetFormat::Png, 1)));
        assert_eq!(check_sheet(&png(10, 10)), Err(PetError::BadSheet));
    }

    #[test]
    fn builtin_sheet_in_repo_is_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../hub_templates/pets/qingqing");
        let m = Manifest::parse(&std::fs::read_to_string(dir.join("pet.json")).unwrap()).unwrap();
        assert_eq!(m.resolved_id("qingqing").as_deref(), Some(BUILTIN_ID));
        for f in [m.sheet_file().unwrap().as_str(), "body.png", "lines.png"] {
            let bytes = std::fs::read(dir.join(f)).unwrap();
            assert_eq!(check_sheet(&bytes), Ok((SheetFormat::Png, 1)), "{f}");
        }
    }
}
