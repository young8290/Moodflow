use super::*;
use moodflow_sync::{Backend, Blob, Expect, Read as RemoteRead};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub type Policy = Arc<dyn Fn() -> bool + Send + Sync>;
pub fn valid_key(key: &str) -> bool {
    ["config.mfsync", "index.mfsync"].contains(&key)
        || key
            .strip_prefix("objects/")
            .and_then(|s| s.strip_suffix(".mfsync"))
            .is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
}
pub fn regular(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "sync.invalid"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(meta.file_attributes() & 0x400 == 0, "sync.invalid");
    }
    Ok(meta)
}
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    ensure!(regular(path)?.len() <= limit as u64, "sync.limit");
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "sync.limit");
    Ok(bytes)
}
pub fn lock(path: &Path) -> Result<fs::File> {
    if path.exists() {
        regular(path)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                ensure!(start.elapsed() < Duration::from_secs(2), "sync.busy");
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error.into()),
        }
    }
}
pub fn atomic(path: &Path, bytes: &[u8], expected: Option<&str>) -> Result<()> {
    let old = match read_bounded(path, moodflow_sync::MAX_BLOB + 64) {
        Ok(bytes) => Some(moodflow_sync::digest(&bytes)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    ensure!(old.as_deref() == expected, "sync.conflict");
    let mut staged = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?,
    )?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    if expected.is_some() {
        staged.persist(path)?;
    } else {
        staged.persist_noclobber(path)?;
    }
    Ok(())
}
pub struct Folder {
    root: PathBuf,
    allowed: Policy,
    recovering: bool,
}
impl Folder {
    pub fn open(parent: &Path, create: bool, allowed: Policy) -> Result<Self> {
        ensure!(parent.is_absolute() && parent.is_dir(), "sync.invalid");
        let parent = parent.canonicalize()?;
        let root = parent.join("MoodflowSync-v1");
        if create {
            fs::create_dir_all(&root)?;
        }
        let metadata = fs::symlink_metadata(&root)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink() && root.canonicalize()? == root,
            "sync.invalid"
        );
        if create {
            fs::create_dir_all(root.join("objects"))?;
        }
        let objects = root.join("objects");
        ensure!(
            objects.is_dir() && objects.canonicalize()? == objects,
            "sync.invalid"
        );
        Ok(Self {
            root,
            allowed,
            recovering: false,
        })
    }
    fn path(&self, key: &str) -> Result<PathBuf> {
        ensure!(valid_key(key), "sync.invalid");
        Ok(self.root.join(key))
    }
    fn read(&self, key: &str) -> Result<RemoteRead> {
        self.read_path(&self.path(key)?)
    }
    fn read_path(&self, path: &Path) -> Result<RemoteRead> {
        ensure!((self.allowed)(), "sync.cancelled");
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(RemoteRead::Missing),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "sync.invalid"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
                FILE_ATTRIBUTE_RECALL_ON_OPEN,
            };
            if meta.file_attributes()
                & (FILE_ATTRIBUTE_OFFLINE
                    | FILE_ATTRIBUTE_RECALL_ON_OPEN
                    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
                != 0
            {
                return Ok(RemoteRead::Deferred);
            }
        }
        ensure!(
            path.canonicalize()?.parent() == path.parent(),
            "sync.invalid"
        );
        let mut bytes = vec![];
        fs::File::open(path)?
            .take(moodflow_sync::MAX_BLOB as u64 + 65)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= moodflow_sync::MAX_BLOB + 64, "sync.limit");
        ensure!((self.allowed)(), "sync.cancelled");
        Ok(RemoteRead::Present(Blob {
            revision: moodflow_sync::digest(&bytes),
            bytes,
        }))
    }
    fn check_provider_conflicts(&self) -> Result<()> {
        if self.recovering {
            return Ok(());
        }
        for (n, entry) in fs::read_dir(&self.root)?.enumerate() {
            ensure!(n < 10000, "sync.limit");
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".mfsync")
                && !["index.mfsync", "config.mfsync"].contains(&name.as_ref())
            {
                anyhow::bail!("sync.provider_conflict");
            }
        }
        Ok(())
    }
    pub fn repair_sources(&mut self) -> Result<Vec<(String, Vec<u8>)>> {
        self.recovering = true;
        let mut sources = vec![];
        let mut total = 0usize;
        for (n, entry) in fs::read_dir(&self.root)?.enumerate() {
            ensure!(n < 10000, "sync.limit");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".mfsync")
                || ["index.mfsync", "config.mfsync"].contains(&name.as_str())
            {
                continue;
            }
            ensure!(sources.len() < 64, "sync.limit");
            let RemoteRead::Present(blob) = self.read_path(&entry.path())? else {
                anyhow::bail!("sync.deferred")
            };
            total = total.saturating_add(blob.bytes.len());
            ensure!(total <= moodflow_sync::MAX_TOTAL, "sync.limit");
            sources.push((name, blob.bytes));
        }
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(sources)
    }
    pub fn archive_sources(&self, sources: &[(String, String)]) -> Result<()> {
        let archive = self.root.join("provider-conflicts");
        fs::create_dir_all(&archive)?;
        ensure!(archive.canonicalize()? == archive, "sync.invalid");
        for (name, hash) in sources {
            ensure!((self.allowed)(), "sync.cancelled");
            let path = self.root.join(name);
            ensure!(path.parent() == Some(self.root.as_path()), "sync.invalid");
            let RemoteRead::Present(blob) = self.read_path(&path)? else {
                anyhow::bail!("sync.deferred")
            };
            ensure!(moodflow_sync::digest(&blob.bytes) == *hash, "sync.changed");
            let target = archive.join(format!("{}-{}.mfsync", uuid::Uuid::new_v4(), hash));
            ensure!(
                target.parent() == Some(archive.as_path()) && !target.exists(),
                "sync.invalid"
            );
            ensure!((self.allowed)(), "sync.cancelled");
            fs::rename(path, target)?;
        }
        Ok(())
    }
}
impl Backend for Folder {
    fn get(&mut self, key: &str) -> Result<RemoteRead> {
        self.check_provider_conflicts()?;
        self.read(key)
    }
    fn put(&mut self, key: &str, bytes: &[u8], expected: Expect<'_>) -> Result<String> {
        ensure!(bytes.len() <= moodflow_sync::MAX_BLOB + 64, "sync.limit");
        ensure!((self.allowed)(), "sync.cancelled");
        let _lock = lock(&self.root.join(".sync.lock"))?;
        self.check_provider_conflicts()?;
        let existing = match self.read(key)? {
            RemoteRead::Present(blob) => Some(blob.revision),
            RemoteRead::Missing => None,
            RemoteRead::Deferred => anyhow::bail!("sync.deferred"),
        };
        let expected = match expected {
            Expect::Missing => None,
            Expect::Revision(s) => Some(s),
        };
        ensure!(existing.as_deref() == expected, "sync.conflict");
        let path = self.path(key)?;
        let mut staged = tempfile::NamedTempFile::new_in(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("sync.invalid"))?,
        )?;
        staged.write_all(bytes)?;
        staged.as_file().sync_all()?;
        ensure!((self.allowed)(), "sync.cancelled");
        // Recheck after flushing: a sync provider does not participate in our advisory lock.
        let current = match self.read(key)? {
            RemoteRead::Present(blob) => Some(blob.revision),
            RemoteRead::Missing => None,
            RemoteRead::Deferred => anyhow::bail!("sync.deferred"),
        };
        ensure!(current.as_deref() == expected, "sync.conflict");
        if expected.is_some() {
            staged.persist(path)?;
        } else {
            staged.persist_noclobber(path)?;
        }
        Ok(moodflow_sync::digest(bytes))
    }
}

pub struct Dav {
    base: reqwest::Url,
    client: reqwest::blocking::Client,
    username: String,
    password: zeroize::Zeroizing<String>,
    allowed: Policy,
}
impl Dav {
    #[cfg(test)]
    pub(super) fn fixture(base: &str) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let base = reqwest::Url::parse(base).unwrap();
        assert_eq!(base.host_str(), Some("127.0.0.1"));
        Self {
            base,
            client: reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
            username: "fixture".into(),
            password: zeroize::Zeroizing::new("fixture-password".into()),
            allowed: Arc::new(|| true),
        }
    }
    pub fn url(value: &str) -> Result<reqwest::Url> {
        ensure!(value.len() <= 2048, "sync.invalid");
        let mut url = reqwest::Url::parse(value)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "sync.https"
        );
        let path = format!("{}/MoodflowSync-v1/", url.path().trim_end_matches('/'));
        url.set_path(&path);
        Ok(url)
    }
    pub fn open(
        url: &str,
        user: &str,
        password: &str,
        create: bool,
        allowed: Policy,
    ) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        ensure!(user.len() <= 1024 && password.len() <= 4096, "sync.limit");
        let backend = Self {
            base: Self::url(url)?,
            client: reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(20))
                .build()?,
            username: user.into(),
            password: zeroize::Zeroizing::new(password.into()),
            allowed,
        };
        if create {
            for suffix in ["", "objects/"] {
                let response = backend
                    .request(reqwest::Method::from_bytes(b"MKCOL")?, suffix)?
                    .send()
                    .map_err(|_| anyhow::anyhow!("sync.network"))?;
                ensure!(
                    response.status().is_success()
                        || response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED,
                    "sync.network"
                );
            }
        }
        Ok(backend)
    }
    fn request(
        &self,
        method: reqwest::Method,
        key: &str,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        ensure!((self.allowed)(), "sync.cancelled");
        ensure!(
            key.is_empty() || key == "objects/" || valid_key(key),
            "sync.invalid"
        );
        Ok(self
            .client
            .request(method, self.base.join(key)?)
            .basic_auth(&self.username, Some(self.password.as_str())))
    }
}
struct CheckedBody {
    reader: std::io::Cursor<Vec<u8>>,
    allowed: Policy,
}
impl std::io::Read for CheckedBody {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if !(self.allowed)() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "sync.cancelled",
            ));
        }
        self.reader.read(bytes)
    }
}
impl Backend for Dav {
    fn get(&mut self, key: &str) -> Result<RemoteRead> {
        let response = self
            .request(reqwest::Method::GET, key)?
            .send()
            .map_err(|_| anyhow::anyhow!("sync.network"))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(RemoteRead::Missing);
        }
        if [202, 423, 503].contains(&status.as_u16()) {
            return Ok(RemoteRead::Deferred);
        }
        ensure!(status.is_success(), "sync.network");
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.starts_with('"') && v.ends_with('"') && v.len() <= 1024)
            .map(str::to_owned);
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= moodflow_sync::MAX_BLOB as u64 + 64),
            "sync.limit"
        );
        let mut bytes = vec![];
        let mut reader = response.take(moodflow_sync::MAX_BLOB as u64 + 65);
        let mut chunk = [0; 65536];
        loop {
            ensure!((self.allowed)(), "sync.cancelled");
            let n = reader
                .read(&mut chunk)
                .map_err(|_| anyhow::anyhow!("sync.network"))?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        ensure!(bytes.len() <= moodflow_sync::MAX_BLOB + 64, "sync.limit");
        let revision = if key.starts_with("objects/") {
            etag.unwrap_or_else(|| moodflow_sync::digest(&bytes))
        } else {
            etag.ok_or_else(|| anyhow::anyhow!("sync.etag_required"))?
        };
        Ok(RemoteRead::Present(Blob { revision, bytes }))
    }
    fn put(&mut self, key: &str, bytes: &[u8], expected: Expect<'_>) -> Result<String> {
        ensure!(bytes.len() <= moodflow_sync::MAX_BLOB + 64, "sync.limit");
        let request = self
            .request(reqwest::Method::PUT, key)?
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream");
        let request = match expected {
            Expect::Missing => request.header(reqwest::header::IF_NONE_MATCH, "*"),
            Expect::Revision(tag) => request.header(reqwest::header::IF_MATCH, tag),
        };
        let body = reqwest::blocking::Body::sized(
            CheckedBody {
                reader: std::io::Cursor::new(bytes.to_vec()),
                allowed: self.allowed.clone(),
            },
            bytes.len() as u64,
        );
        let response = request
            .body(body)
            .send()
            .map_err(|_| anyhow::anyhow!("sync.network"))?;
        ensure!(
            ![409, 412].contains(&response.status().as_u16()),
            "sync.conflict"
        );
        ensure!(response.status().is_success(), "sync.network");
        Ok(response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .into())
    }
}
