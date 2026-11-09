//! Apply an explicitly confirmed tool-data wipe before opening any application stores.
use moodflow_hub_core::infra::wipe::{self, WipeReport};
use std::{fs, io, path::Path};

pub fn apply_pending(hub: &Path, tools: &Path) -> io::Result<Option<WipeReport>> {
    if wipe::pending(hub).is_some_and(|request| request.include_tools) {
        let root = tools.canonicalize()?;
        if !root.is_absolute() || !root.is_dir() {
            return Err(io::Error::other("invalid tools root"));
        }
        let mut remaining = 100_000_usize;
        // Fixed owned directories only. Never infer deletion paths from a manifest or imported data.
        for name in ["capsule", "vault", "capture"] {
            let path = root.join(name);
            if fs::symlink_metadata(&path).is_ok() {
                remove_owned(&root, &path, &mut remaining, 0)?;
            }
        }
    }
    wipe::apply_pending(hub, crate::secrets::SECRETS_FILE)
}
fn remove_owned(root: &Path, path: &Path, remaining: &mut usize, depth: usize) -> io::Result<()> {
    if depth > 128 || *remaining == 0 {
        return Err(io::Error::other("tool data wipe limit"));
    }
    *remaining -= 1;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other("tool data contains a link"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("tool data contains a reparse point"));
        }
    }
    let resolved = path.canonicalize()?;
    if resolved == root || !resolved.starts_with(root) {
        return Err(io::Error::other("tool data escaped root"));
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_owned(root, &entry?.path(), remaining, depth + 1)?;
        }
        fs::remove_dir(path)
    } else if meta.is_file() {
        fs::remove_file(path)
    } else {
        Err(io::Error::other("unsupported tool data object"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_new_explicit_requests_remove_owned_tools_and_preserve_other_data() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let hub = root.join("hub");
        for name in ["capsule", "vault", "data", "templates", "external-backups"] {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(root.join(name).join("fixture"), b"fixture").unwrap();
        }
        fs::create_dir_all(&hub).unwrap();
        fs::write(hub.join("moodflow.db"), b"fixture").unwrap();
        assert!(apply_pending(&hub, root).unwrap().is_none());
        // An old-version confirmation did not name these new stores.
        fs::write(
            hub.join(wipe::MARKER),
            br#"{"include_secrets":false,"requested_at":1}"#,
        )
        .unwrap();
        apply_pending(&hub, root).unwrap();
        assert!(root.join("vault/fixture").exists());
        wipe::request(
            &hub,
            &wipe::WipeRequest {
                include_secrets: false,
                include_tools: true,
                requested_at: 2,
            },
        )
        .unwrap();
        apply_pending(&hub, root).unwrap();
        for name in ["capsule", "vault"] {
            assert!(!root.join(name).exists());
        }
        for name in ["data", "templates", "external-backups"] {
            assert!(root.join(name).join("fixture").exists());
        }
        assert!(!hub.join(wipe::MARKER).exists());
    }
    #[cfg(unix)]
    #[test]
    fn links_fail_closed_and_retain_the_request_and_external_target() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("app");
        let hub = root.join("hub");
        let external = temporary.path().join("outside");
        fs::create_dir_all(&hub).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("keep"), b"fixture").unwrap();
        std::os::unix::fs::symlink(&external, root.join("capsule")).unwrap();
        wipe::request(
            &hub,
            &wipe::WipeRequest {
                include_secrets: false,
                include_tools: true,
                requested_at: 2,
            },
        )
        .unwrap();
        assert!(apply_pending(&hub, &root).is_err());
        assert!(hub.join(wipe::MARKER).exists());
        assert!(external.join("keep").exists());
    }
}
