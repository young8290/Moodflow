//! Local Markdown library. Platform ACLs and network consent belong to the host.
pub mod capture;
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
pub use serde_yaml::Value as PropertyValue;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
};

pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 10_000;
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub capsule: String,
    pub version: u32,
    pub id: String,
    pub title: String,
    pub updated_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub properties: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub metadata: Metadata,
    pub body: String,
    #[serde(default)]
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub items: Vec<Summary>,
    /// IDs only, never document contents.
    pub unreadable: Vec<String>,
}

pub fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_id(id: &str) -> anyhow::Result<String> {
    let parsed = uuid::Uuid::parse_str(id).context("无效条目标识")?;
    ensure!(parsed.to_string() == id, "条目标识必须是规范的小写 UUID");
    Ok(id.to_owned())
}

impl Document {
    pub fn new(kind: &str, title: &str, body: &str, now: &str) -> Self {
        Self {
            metadata: Metadata {
                capsule: kind.into(),
                version: 1,
                id: uuid::Uuid::new_v4().to_string(),
                title: title.into(),
                updated_at: now.into(),
                tags: Vec::new(),
                properties: BTreeMap::new(),
            },
            body: body.into(),
            revision: String::new(),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        canonical_id(&self.metadata.id)?;
        ensure!(self.metadata.version == 1, "不支持的资料格式版本");
        ensure!(
            [
                "note", "skill", "image", "pdf", "video", "resource", "capture"
            ]
            .contains(&self.metadata.capsule.as_str()),
            "不支持此资料类型；密码必须存入加密密码库"
        );
        ensure!(
            !self.metadata.title.trim().is_empty() && self.metadata.title.chars().count() <= 256,
            "标题须为 1–256 个字符"
        );
        ensure!(
            !self.metadata.updated_at.is_empty() && self.metadata.updated_at.len() <= 64,
            "缺少更新时间"
        );
        ensure!(
            self.metadata.tags.len() <= 64
                && self.metadata.tags.iter().all(|s| s.chars().count() <= 64),
            "标签过多或过长"
        );
        ensure!(
            self.body.len() <= MAX_DOCUMENT_BYTES,
            "资料超过单条大小上限"
        );
        ensure!(
            !self.metadata.properties.contains_key("password")
                && !self.metadata.properties.contains_key("totp_secret"),
            "密码和密钥不得保存在明文资料中"
        );
        Ok(())
    }

    pub fn from_markdown(text: &str) -> anyhow::Result<Self> {
        ensure!(text.len() <= MAX_DOCUMENT_BYTES, "资料超过单条大小上限");
        let normalized = text
            .strip_prefix('\u{feff}')
            .unwrap_or(text)
            .replace("\r\n", "\n");
        let rest = normalized
            .strip_prefix("---\n")
            .context("缺少 front matter")?;
        let end = rest.find("\n---\n").context("front matter 未闭合")?;
        let metadata: Metadata =
            serde_yaml::from_str(&rest[..end]).context("资料元信息格式不合法")?;
        let mut body = &rest[end + 5..];
        if let Some(stripped) = body.strip_prefix('\n') {
            body = stripped;
        }
        let document = Self {
            metadata,
            body: body.into(),
            revision: revision(text.as_bytes()),
        };
        document.validate()?;
        Ok(document)
    }

    pub fn to_markdown(&self) -> anyhow::Result<String> {
        self.validate()?;
        // JSON scalars are a YAML subset, and are accepted by XSIME's line parser.
        let mut fields = self.metadata.properties.clone();
        for (key, value) in [
            ("capsule", self.metadata.capsule.as_str()),
            ("id", &self.metadata.id),
            ("title", &self.metadata.title),
            ("updated_at", &self.metadata.updated_at),
        ] {
            fields.insert(key.into(), serde_yaml::Value::String(value.into()));
        }
        fields.insert("version".into(), serde_yaml::to_value(1_u32)?);
        fields.insert("tags".into(), serde_yaml::to_value(&self.metadata.tags)?);
        let mut text = String::from("---\n");
        for (key, value) in fields {
            ensure!(
                !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                "元信息字段名不合法"
            );
            text.push_str(&format!("{key}: {}\n", serde_json::to_string(&value)?));
        }
        text.push_str("---\n\n");
        text.push_str(&self.body);
        ensure!(text.len() <= MAX_DOCUMENT_BYTES, "资料超过单条大小上限");
        Ok(text)
    }

    fn summary(&self) -> Summary {
        Summary {
            id: self.metadata.id.clone(),
            title: self.metadata.title.clone(),
            kind: self.metadata.capsule.clone(),
            tags: self.metadata.tags.clone(),
            updated_at: self.metadata.updated_at.clone(),
            revision: self.revision.clone(),
        }
    }
}

/// Root must already be private to the current user. Each instance owns a search cache.
pub struct Library {
    root: PathBuf,
    cache: BTreeMap<String, (Summary, String)>,
    unreadable: Vec<String>,
    loaded: bool,
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: BTreeMap::new(),
            unreadable: Vec::new(),
            loaded: false,
        }
    }

    fn path(&self, id: &str) -> anyhow::Result<PathBuf> {
        Ok(self.root.join(format!("{}.md", canonical_id(id)?)))
    }

    fn read(&self, id: &str) -> anyhow::Result<Option<String>> {
        let path = self.path(id)?;
        match fs::symlink_metadata(&path) {
            Ok(m) => ensure!(
                m.file_type().is_file() && !m.file_type().is_symlink(),
                "资料不能是链接或目录"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let mut text = String::new();
        fs::File::open(path)?
            .take(MAX_DOCUMENT_BYTES as u64 + 1)
            .read_to_string(&mut text)?;
        ensure!(text.len() <= MAX_DOCUMENT_BYTES, "资料超过大小上限");
        Ok(Some(text))
    }

    fn lock(&self) -> anyhow::Result<fs::File> {
        fs::create_dir_all(&self.root)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(".library.lock"))?;
        file.lock()?;
        Ok(file)
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Document> {
        canonical_id(id)?;
        ensure!(
            !self
                .root
                .join("tombstones")
                .join(format!("{id}.json"))
                .exists(),
            "资料已删除"
        );
        let text = self.read(id)?.context("资料不存在")?;
        let doc = Document::from_markdown(&text)?;
        ensure!(doc.metadata.id == id, "资料标识与文件名不一致");
        Ok(doc)
    }

    /// None means create-only. Existing records require the exact disk revision.
    pub fn put(&mut self, doc: &Document, expected: Option<&str>) -> anyhow::Result<Document> {
        let text = doc.to_markdown()?;
        let id = &doc.metadata.id;
        let _lock = self.lock()?;
        let old = self.read(id)?;
        ensure!(
            old.as_ref().map(|s| revision(s.as_bytes())).as_deref() == expected,
            "revision_conflict"
        );
        let tombstone = self.root.join("tombstones").join(format!("{id}.json"));
        ensure!(
            !tombstone.exists(),
            "已删除的条目不能隐式复活，请保存为新条目"
        );
        let mut count = 0_usize;
        let mut bytes = 0_u64;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let meta = entry.file_type()?;
            if meta.is_file() && entry.path().extension().is_some_and(|s| s == "md") {
                count += 1;
                bytes = bytes.saturating_add(entry.metadata()?.len());
            }
        }
        ensure!(old.is_some() || count < MAX_ENTRIES, "资料条数已达上限");
        ensure!(
            bytes
                .saturating_sub(old.as_ref().map_or(0, |s| s.len() as u64))
                .saturating_add(text.len() as u64)
                <= MAX_TOTAL_BYTES,
            "资料库总大小已达上限"
        );
        let mut staged = tempfile::NamedTempFile::new_in(&self.root)?;
        staged.write_all(text.as_bytes())?;
        staged.as_file().sync_all()?;
        ensure!(
            self.read(id)?
                .as_ref()
                .map(|s| revision(s.as_bytes()))
                .as_deref()
                == expected,
            "revision_conflict"
        );
        let path = self.path(id)?;
        if expected.is_none() {
            staged.persist_noclobber(path).map_err(|e| e.error)?;
        } else {
            staged.persist(path).map_err(|e| e.error)?;
        }
        let mut saved = doc.clone();
        saved.revision = revision(text.as_bytes());
        self.index(&saved);
        Ok(saved)
    }

    /// Deletion leaves a permanent, content-free tombstone for future synchronization.
    pub fn delete(&mut self, id: &str, expected: &str, now: &str) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        let text = self.read(id)?.context("资料不存在")?;
        ensure!(revision(text.as_bytes()) == expected, "revision_conflict");
        let tombstones = self.root.join("tombstones");
        fs::create_dir_all(&tombstones)?;
        let mut staged = tempfile::NamedTempFile::new_in(&tombstones)?;
        serde_json::to_writer(
            staged.as_file_mut(),
            &serde_json::json!({"id":id,"revision":expected,"deleted_at":now}),
        )?;
        staged.as_file().sync_all()?;
        staged
            .persist_noclobber(tombstones.join(format!("{id}.json")))
            .map_err(|e| e.error)?;
        // A failed removal remains hidden by the tombstone; never resurrect during refresh.
        self.cache.remove(id);
        self.loaded = false;
        fs::remove_file(self.path(id)?)?;
        Ok(())
    }

    fn index(&mut self, doc: &Document) {
        let haystack = format!(
            "{} {} {} {}",
            doc.metadata.title,
            doc.metadata.tags.join(" "),
            doc.body,
            serde_json::to_string(&doc.metadata.properties).unwrap_or_default()
        )
        .to_lowercase();
        self.cache
            .insert(doc.metadata.id.clone(), (doc.summary(), haystack));
    }

    /// Sync may receive a permanent deletion for an ID this device has never stored.
    /// Absence alone does not authorize this operation; the host must verify a remote tombstone.
    pub fn record_remote_deletion(&mut self, id: &str, now: &str) -> anyhow::Result<()> {
        canonical_id(id)?;
        ensure!(now.len() <= 64, "无效删除时间");
        let _lock = self.lock()?;
        ensure!(self.read(id)?.is_none(), "revision_conflict");
        let directory = self.root.join("tombstones");
        fs::create_dir_all(&directory)?;
        ensure!(
            !fs::symlink_metadata(&directory)?.file_type().is_symlink(),
            "删除标记目录不能是链接"
        );
        let target = directory.join(format!("{id}.json"));
        if target.exists() {
            ensure!(fs::symlink_metadata(target)?.is_file(), "无效删除标记");
            return Ok(());
        }
        let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
        serde_json::to_writer(
            staged.as_file_mut(),
            &serde_json::json!({"id":id,"deleted_at":now,"source":"sync"}),
        )?;
        staged.as_file().sync_all()?;
        staged.persist_noclobber(target).map_err(|e| e.error)?;
        self.cache.remove(id);
        self.loaded = false;
        Ok(())
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.loaded = false;
        self.cache.clear();
        self.unreadable.clear();
        if !self.root.exists() {
            self.loaded = true;
            return Ok(());
        }
        let mut bytes = 0_u64;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.path().extension().is_none_or(|s| s != "md") {
                continue;
            }
            let Some(id) = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
            else {
                continue;
            };
            if canonical_id(&id).is_err() {
                continue;
            }
            if self
                .root
                .join("tombstones")
                .join(format!("{id}.json"))
                .exists()
            {
                continue;
            }
            bytes = bytes.saturating_add(entry.metadata()?.len());
            ensure!(
                self.cache.len() + self.unreadable.len() < MAX_ENTRIES && bytes <= MAX_TOTAL_BYTES,
                "资料库超过读取上限"
            );
            match self.get(&id) {
                Ok(doc) => self.index(&doc),
                Err(_) => self.unreadable.push(id),
            }
        }
        self.loaded = true;
        Ok(())
    }

    pub fn search(
        &mut self,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Listing> {
        if !self.loaded {
            self.refresh()?;
        }
        ensure!(query.len() <= 1024, "搜索内容过长");
        let query = query.to_lowercase();
        let terms = query.split_whitespace().collect::<Vec<_>>();
        let mut items = self
            .cache
            .values()
            .filter(|(s, text)| {
                kind.is_none_or(|k| k == s.kind) && terms.iter().all(|term| text.contains(term))
            })
            .map(|(s, _)| s.clone())
            .collect::<Vec<_>>();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        items.truncate(limit.min(500));
        Ok(Listing {
            items,
            unreadable: self.unreadable.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "manual 10k-entry search measurement; creates only temporary fixture files"]
    fn ten_thousand_document_search_measurement() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..MAX_ENTRIES {
            let document = Document::new(
                "note",
                &format!("研究笔记 {index}"),
                &format!(
                    "{} 独立索引{index}",
                    "普通资料库正文 performance example ".repeat(20)
                ),
                "2026-11-09T00:00:00Z",
            );
            fs::write(
                dir.path().join(format!("{}.md", document.metadata.id)),
                document.to_markdown().unwrap(),
            )
            .unwrap();
        }
        let mut library = Library::new(dir.path().to_owned());
        let start = std::time::Instant::now();
        library.refresh().unwrap();
        let refresh = start.elapsed();
        let mut maximum = std::time::Duration::ZERO;
        for query in [
            "研究笔记",
            "performance example",
            "独立索引9999",
            "缺失词项",
        ] {
            for _ in 0..10 {
                let start = std::time::Instant::now();
                let result = library.search(query, None, 50).unwrap();
                maximum = maximum.max(start.elapsed());
                if query == "独立索引9999" {
                    assert_eq!(result.items.len(), 1);
                }
            }
        }
        eprintln!("10,000 entries; cold refresh {refresh:?}; maximum cached query {maximum:?}");
        assert!(
            maximum < std::time::Duration::from_millis(100),
            "10k cached search exceeds acceptance threshold"
        );
    }
    #[test]
    fn xsime_format_preserves_reference_and_unknown_metadata() {
        let raw = "---\ncapsule: pdf\nversion: 1\nid: \"12345678-1234-4234-8234-123456789abc\"\ntitle: \"无附件文献\"\nupdated_at: \"2026-11-09T00:00:00Z\"\nreference_authors: \"甲;乙\"\nreference_year: \"2026\"\ncustom_key: [one, two]\n---\n\n";
        let doc = Document::from_markdown(raw).unwrap();
        let round = Document::from_markdown(&doc.to_markdown().unwrap()).unwrap();
        assert_eq!(doc.metadata.properties, round.metadata.properties);
        assert!(round.body.is_empty());
    }
    #[test]
    fn stale_updates_and_deletes_never_overwrite_other_windows() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Library::new(dir.path().to_owned());
        let mut b = Library::new(dir.path().to_owned());
        let doc = a
            .put(&Document::new("note", "标题", "原文", "2026-11-09"), None)
            .unwrap();
        let mut changed = doc.clone();
        changed.body = "另一个窗口".into();
        let changed = b.put(&changed, Some(&doc.revision)).unwrap();
        assert!(a.put(&doc, Some(&doc.revision)).is_err());
        assert!(a.delete(&doc.metadata.id, &doc.revision, "now").is_err());
        assert_eq!(a.get(&doc.metadata.id).unwrap().body, "另一个窗口");
        b.delete(&doc.metadata.id, &changed.revision, "now")
            .unwrap();
        assert!(a.put(&doc, None).is_err());
        a.refresh().unwrap();
        assert!(a.search("", None, 500).unwrap().items.is_empty());
    }
    #[test]
    fn search_refresh_and_malformed_entries_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::new(dir.path().into());
        let mut doc = Document::new("note", "Alpha", "检索正文", "2026");
        doc.metadata.tags.push("研究".into());
        let saved = lib.put(&doc, None).unwrap();
        assert_eq!(lib.search("ALPHA 研究", None, 10).unwrap().items.len(), 1);
        fs::write(
            lib.path(&uuid::Uuid::new_v4().to_string()).unwrap(),
            "corrupt",
        )
        .unwrap();
        lib.refresh().unwrap();
        let list = lib.search("检索", None, 10).unwrap();
        assert_eq!(list.items[0].id, saved.metadata.id);
        assert_eq!(list.unreadable.len(), 1);
        assert!(lib.get("../../escape").is_err());
        let mut secret = Document::new("password", "no", "password", "now");
        assert!(lib.put(&secret, None).is_err());
        secret.metadata.capsule = "note".into();
        secret.body = "x".repeat(MAX_DOCUMENT_BYTES + 1);
        assert!(lib.put(&secret, None).is_err());
    }
}
