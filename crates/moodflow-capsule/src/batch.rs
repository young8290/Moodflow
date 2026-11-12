//! Bounded portable bundles and explicit, revision-checked batch migration.
//! Attachments are references only; no referenced file is opened or copied.
use crate::{Document, Library, MAX_DOCUMENT_BYTES, revision};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

pub const MAX_BATCH_ENTRIES: usize = 200;
pub const MAX_BATCH_BYTES: usize = 32 * 1024 * 1024;
const FORMAT: &str = "moodflow-library";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    pub documents: Vec<Document>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub index: usize,
    pub source: usize,
    pub id: String,
    pub title: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub status: String,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub sources: Vec<Source>,
    pub items: Vec<PreviewItem>,
    pub bytes: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    Skip,
    Copy,
    Create,
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub index: usize,
    pub mode: ImportMode,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub index: usize,
    pub id: Option<String>,
    /// Stable codes only; never expose parser errors or document contents.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
    pub revision: String,
}

impl Bundle {
    pub fn encode(documents: Vec<Document>) -> anyhow::Result<Vec<u8>> {
        ensure!(
            !documents.is_empty() && documents.len() <= MAX_BATCH_ENTRIES,
            "batch_count"
        );
        let mut ids = BTreeSet::new();
        let mut total = 0;
        for doc in &documents {
            total += doc.to_markdown()?.len();
            ensure!(total <= MAX_BATCH_BYTES, "batch_size");
            ensure!(ids.insert(&doc.metadata.id), "duplicate_id");
        }
        let bytes = serde_json::to_vec_pretty(&Self {
            format: FORMAT.into(),
            version: 1,
            documents,
        })?;
        ensure!(bytes.len() <= MAX_BATCH_BYTES, "batch_size");
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Vec<Document>> {
        ensure!(bytes.len() <= MAX_BATCH_BYTES, "batch_size");
        let bundle: Self = serde_json::from_slice(bytes).context("invalid_bundle")?;
        ensure!(
            bundle.format == FORMAT && bundle.version == 1,
            "unsupported_bundle"
        );
        // Validate before returning even one document. Revision is transport metadata,
        // never authority to overwrite a local record.
        Self::encode(bundle.documents.clone())?;
        Ok(bundle.documents)
    }
}

fn read_source(path: &Path) -> anyhow::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "invalid_source"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(meta.file_attributes() & 0x400 == 0, "invalid_source");
    }
    ensure!(meta.len() <= MAX_BATCH_BYTES as u64, "batch_size");
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BATCH_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_BATCH_BYTES, "batch_size");
    Ok(bytes)
}

type LoadedBatch = (Vec<Source>, Vec<(usize, Document)>, usize);

fn load(paths: &[String], now: &str) -> anyhow::Result<LoadedBatch> {
    ensure!(
        !paths.is_empty() && paths.len() <= MAX_BATCH_ENTRIES,
        "batch_count"
    );
    let mut sources = Vec::new();
    let mut documents = Vec::new();
    let mut total = 0;
    let mut seen = BTreeSet::new();
    for (source, path) in paths.iter().enumerate() {
        let path_ref = Path::new(path);
        ensure!(seen.insert(path_ref.canonicalize()?), "duplicate_source");
        let bytes = read_source(path_ref)?;
        total += bytes.len();
        ensure!(total <= MAX_BATCH_BYTES, "batch_size");
        let extension = path_ref.extension().and_then(|s| s.to_str()).unwrap_or("");
        let docs = if extension.eq_ignore_ascii_case("mflibrary") {
            Bundle::decode(&bytes)?
        } else {
            ensure!(
                extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown"),
                "unsupported_source"
            );
            ensure!(bytes.len() <= MAX_DOCUMENT_BYTES, "document_size");
            let text = std::str::from_utf8(&bytes)?;
            let doc = if text.trim_start_matches('\u{feff}').starts_with("---") {
                Document::from_markdown(text)?
            } else {
                // Stable preview ID for plain Markdown, even when preview is re-read.
                let mut doc = Document::new(
                    "note",
                    &path_ref.file_stem().unwrap_or_default().to_string_lossy(),
                    text,
                    now,
                );
                let digest = revision(&bytes);
                let key =
                    revision(format!("{}:{digest}", path_ref.canonicalize()?.display()).as_bytes());
                doc.metadata.id = format!(
                    "{}-{}-4{}-8{}-{}",
                    &key[..8],
                    &key[8..12],
                    &key[13..16],
                    &key[17..20],
                    &key[20..32]
                );
                doc
            };
            doc.to_markdown()?;
            vec![doc]
        };
        ensure!(
            documents.len() + docs.len() <= MAX_BATCH_ENTRIES,
            "batch_count"
        );
        documents.extend(docs.into_iter().map(|doc| (source, doc)));
        sources.push(Source {
            path: path.clone(),
            digest: revision(&bytes),
        });
    }
    Ok((sources, documents, total))
}

impl Library {
    /// Read-only preview. Invalid sources reject the whole preview, before any write.
    pub fn preview_import(&self, paths: &[String], now: &str) -> anyhow::Result<Preview> {
        let (sources, documents, bytes) = load(paths, now)?;
        let mut seen = BTreeSet::new();
        let mut items = Vec::new();
        for (index, (source, doc)) in documents.iter().enumerate() {
            let id = &doc.metadata.id;
            let deleted = self
                .root
                .join("tombstones")
                .join(format!("{id}.json"))
                .exists();
            let existing = self.read(id)?;
            let expected_revision = existing.as_ref().map(|text| revision(text.as_bytes()));
            let status = if !seen.insert(id) {
                "duplicate"
            } else if deleted {
                "deleted"
            } else if let Some(text) = existing {
                match Document::from_markdown(&text) {
                    Ok(current) if current.metadata.id == *id => {
                        if current.to_markdown()? == doc.to_markdown()? {
                            "identical"
                        } else {
                            "conflict"
                        }
                    }
                    _ => "unreadable",
                }
            } else {
                "new"
            };
            items.push(PreviewItem {
                index,
                source: *source,
                id: id.clone(),
                title: doc.metadata.title.clone(),
                kind: doc.metadata.capsule.clone(),
                tags: doc.metadata.tags.clone(),
                status: status.into(),
                expected_revision,
            });
        }
        Ok(Preview {
            sources,
            items,
            bytes,
        })
    }

    /// Each row commits independently. Failures retain prior successful rows and
    /// return an outcome; callers must never claim transaction-wide success.
    pub fn apply_import(
        &mut self,
        sources: &[Source],
        decisions: &[Decision],
        now: &str,
    ) -> anyhow::Result<Vec<Outcome>> {
        let paths = sources.iter().map(|s| s.path.clone()).collect::<Vec<_>>();
        let (actual, documents, _) = load(&paths, now)?;
        ensure!(
            actual
                .iter()
                .zip(sources)
                .all(|(a, b)| a.digest == b.digest),
            "source_changed"
        );
        let mut indexes = BTreeSet::new();
        ensure!(decisions.len() <= MAX_BATCH_ENTRIES, "batch_count");
        for choice in decisions {
            ensure!(
                choice.index < documents.len() && indexes.insert(choice.index),
                "invalid_decision"
            );
            ensure!(
                choice.mode != ImportMode::Replace || choice.expected_revision.is_some(),
                "missing_revision"
            );
            ensure!(
                choice.mode == ImportMode::Replace || choice.expected_revision.is_none(),
                "unexpected_revision"
            );
        }
        let mut results = Vec::new();
        for choice in decisions {
            if choice.mode == ImportMode::Skip {
                results.push(Outcome {
                    index: choice.index,
                    id: None,
                    status: "skipped".into(),
                });
                continue;
            }
            let mut doc = documents[choice.index].1.clone();
            doc.revision.clear();
            if choice.mode == ImportMode::Copy {
                doc.metadata.id = uuid::Uuid::new_v4().to_string();
                doc.metadata.updated_at = now.into();
            }
            let result = self.put(&doc, choice.expected_revision.as_deref());
            let (id, status) = match result {
                Ok(saved) => (Some(saved.metadata.id), "imported"),
                Err(e) if e.to_string() == "revision_conflict" => (None, "conflict"),
                Err(_) => (None, "failed"),
            };
            results.push(Outcome {
                index: choice.index,
                id,
                status: status.into(),
            });
        }
        Ok(results)
    }

    /// Snapshot selected revisions under the same lock used by all library writers.
    pub fn export_bundle(&self, selection: &[Selection]) -> anyhow::Result<Vec<u8>> {
        ensure!(
            !selection.is_empty() && selection.len() <= MAX_BATCH_ENTRIES,
            "batch_count"
        );
        let _lock = self.lock()?;
        let mut docs = Vec::new();
        let mut bytes = 0;
        for item in selection {
            let doc = self.get(&item.id)?;
            ensure!(doc.revision == item.revision, "revision_conflict");
            bytes += doc.to_markdown()?.len();
            ensure!(bytes <= MAX_BATCH_BYTES, "batch_size");
            docs.push(doc);
        }
        Bundle::encode(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    const NOW: &str = "2026-11-12T00:00:00Z";
    fn fixture() -> (TempDir, Library, Document) {
        let dir = tempdir().unwrap();
        let library = Library::new(dir.path().join("library"));
        let mut doc = Document::new("note", "migration", "正文", NOW);
        doc.metadata.properties.insert(
            "custom_field".into(),
            serde_yaml::Value::String("preserve".into()),
        );
        (dir, library, doc)
    }
    fn source(dir: &TempDir, name: &str, docs: Vec<Document>) -> String {
        let path = dir.path().join(name);
        fs::write(&path, Bundle::encode(docs).unwrap()).unwrap();
        path.to_str().unwrap().into()
    }
    fn decision(index: usize, mode: ImportMode, revision: Option<String>) -> Decision {
        Decision {
            index,
            mode,
            expected_revision: revision,
        }
    }
    #[test]
    fn bundle_roundtrip_preserves_unknown_fields_and_never_reads_attachments() {
        let (dir, mut library, mut doc) = fixture();
        doc.metadata.properties.insert(
            "path".into(),
            serde_yaml::Value::String("nonexistent-secret-file".into()),
        );
        let saved = library.put(&doc, None).unwrap();
        let bytes = library
            .export_bundle(&[Selection {
                id: saved.metadata.id.clone(),
                revision: saved.revision,
            }])
            .unwrap();
        let docs = Bundle::decode(&bytes).unwrap();
        assert_eq!(docs[0].body, doc.body);
        assert_eq!(docs[0].metadata.properties, doc.metadata.properties);
        let path = dir.path().join("backup.mflibrary");
        fs::write(&path, bytes).unwrap();
        let mut target = Library::new(dir.path().join("restored"));
        let preview = target
            .preview_import(&[path.to_str().unwrap().into()], NOW)
            .unwrap();
        assert_eq!(preview.items[0].status, "new");
        assert!(!dir.path().join("restored").exists());
        let result = target
            .apply_import(
                &preview.sources,
                &[decision(0, ImportMode::Create, None)],
                NOW,
            )
            .unwrap();
        assert_eq!(result[0].status, "imported");
        assert_eq!(
            target.get(&doc.metadata.id).unwrap().metadata.properties,
            doc.metadata.properties
        );
    }
    #[test]
    fn preview_distinguishes_identical_conflicting_deleted_and_duplicate() {
        let (dir, mut library, doc) = fixture();
        let saved = library.put(&doc, None).unwrap();
        let identical = source(&dir, "same.mflibrary", vec![doc.clone()]);
        assert_eq!(
            library
                .preview_import(std::slice::from_ref(&identical), NOW)
                .unwrap()
                .items[0]
                .status,
            "identical"
        );
        let mut changed = doc.clone();
        changed.body = "changed".into();
        let conflict = source(&dir, "conflict.mflibrary", vec![changed]);
        let preview = library.preview_import(&[identical, conflict], NOW).unwrap();
        assert_eq!(preview.items[1].status, "duplicate");
        library
            .delete(&doc.metadata.id, &saved.revision, NOW)
            .unwrap();
        let path = source(&dir, "deleted.mflibrary", vec![doc]);
        assert_eq!(
            library.preview_import(&[path], NOW).unwrap().items[0].status,
            "deleted"
        );
    }
    #[test]
    fn changed_source_rejects_entire_batch_before_writes() {
        let (dir, mut library, doc) = fixture();
        let path = source(&dir, "source.mflibrary", vec![doc.clone()]);
        let preview = library
            .preview_import(std::slice::from_ref(&path), NOW)
            .unwrap();
        let mut changed = doc.clone();
        changed.body = "changed".into();
        fs::write(path, Bundle::encode(vec![changed]).unwrap()).unwrap();
        assert!(
            library
                .apply_import(
                    &preview.sources,
                    &[decision(0, ImportMode::Create, None)],
                    NOW
                )
                .unwrap_err()
                .to_string()
                .contains("source_changed")
        );
        assert!(library.get(&doc.metadata.id).is_err());
    }
    #[test]
    fn stale_replacement_preserves_concurrent_edit_and_reports_partial_success() {
        let (dir, mut library, doc) = fixture();
        let saved = library.put(&doc, None).unwrap();
        let fresh = Document::new("note", "fresh", "fresh", NOW);
        let mut imported = doc.clone();
        imported.body = "imported".into();
        let path = source(&dir, "source.mflibrary", vec![imported, fresh.clone()]);
        let preview = library.preview_import(&[path], NOW).unwrap();
        let mut other = doc.clone();
        other.body = "other window".into();
        library.put(&other, Some(&saved.revision)).unwrap();
        let result = library
            .apply_import(
                &preview.sources,
                &[
                    decision(
                        0,
                        ImportMode::Replace,
                        preview.items[0].expected_revision.clone(),
                    ),
                    decision(1, ImportMode::Create, None),
                ],
                NOW,
            )
            .unwrap();
        assert_eq!(result[0].status, "conflict");
        assert_eq!(result[1].status, "imported");
        assert_eq!(library.get(&doc.metadata.id).unwrap().body, "other window");
        assert_eq!(library.get(&fresh.metadata.id).unwrap().body, "fresh");
    }
    #[test]
    fn replace_requires_explicit_revision_and_copy_never_resurrects_deleted_id() {
        let (dir, mut library, doc) = fixture();
        let saved = library.put(&doc, None).unwrap();
        library
            .delete(&doc.metadata.id, &saved.revision, NOW)
            .unwrap();
        let path = source(&dir, "source.mflibrary", vec![doc.clone()]);
        let preview = library.preview_import(&[path], NOW).unwrap();
        assert!(
            library
                .apply_import(
                    &preview.sources,
                    &[decision(0, ImportMode::Replace, None)],
                    NOW
                )
                .is_err()
        );
        let result = library
            .apply_import(
                &preview.sources,
                &[decision(0, ImportMode::Copy, None)],
                NOW,
            )
            .unwrap();
        assert_eq!(result[0].status, "imported");
        assert_ne!(result[0].id.as_ref().unwrap(), &doc.metadata.id);
        assert!(library.get(&doc.metadata.id).is_err());
    }
    #[test]
    fn plain_markdown_ids_are_stable_and_source_deduplication_is_enforced() {
        let (dir, library, _) = fixture();
        let path = dir.path().join("plain.md");
        fs::write(&path, "plain").unwrap();
        let paths = vec![path.to_str().unwrap().into()];
        let a = library.preview_import(&paths, NOW).unwrap();
        let b = library.preview_import(&paths, "later").unwrap();
        assert_eq!(a.items[0].id, b.items[0].id);
        assert!(
            library
                .preview_import(&[paths[0].clone(), paths[0].clone()], NOW)
                .is_err()
        );
    }
    #[test]
    fn invalid_decisions_are_rejected_before_any_write() {
        let (dir, mut library, doc) = fixture();
        let path = source(&dir, "source.mflibrary", vec![doc.clone()]);
        let preview = library.preview_import(&[path], NOW).unwrap();
        for decisions in [
            vec![
                decision(0, ImportMode::Create, None),
                decision(0, ImportMode::Copy, None),
            ],
            vec![
                decision(0, ImportMode::Create, None),
                decision(1, ImportMode::Create, None),
            ],
        ] {
            assert!(
                library
                    .apply_import(&preview.sources, &decisions, NOW)
                    .is_err()
            );
            assert!(library.get(&doc.metadata.id).is_err());
        }
    }
    #[test]
    fn malformed_secret_and_oversized_bundles_fail_closed() {
        let (_, _, doc) = fixture();
        let mut value = serde_json::to_value(Bundle {
            format: FORMAT.into(),
            version: 1,
            documents: vec![doc.clone()],
        })
        .unwrap();
        value["version"] = 99.into();
        assert!(Bundle::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(Bundle::decode(&vec![b' '; MAX_BATCH_BYTES + 1]).is_err());
        assert!(Bundle::encode(vec![]).is_err());
        assert!(Bundle::encode(vec![doc.clone(), doc.clone()]).is_err());
        let mut secret = doc;
        secret.metadata.capsule = "password".into();
        assert!(Bundle::encode(vec![secret]).is_err());
    }
    #[test]
    fn export_refuses_stale_duplicate_or_deleted_selection() {
        let (_dir, mut library, doc) = fixture();
        let saved = library.put(&doc, None).unwrap();
        let item = Selection {
            id: doc.metadata.id.clone(),
            revision: saved.revision.clone(),
        };
        assert!(
            library
                .export_bundle(&[item.clone(), item.clone()])
                .is_err()
        );
        assert!(
            library
                .export_bundle(&[Selection {
                    revision: "stale".into(),
                    ..item.clone()
                }])
                .is_err()
        );
        library
            .delete(&doc.metadata.id, &saved.revision, NOW)
            .unwrap();
        assert!(library.export_bundle(&[item]).is_err());
    }
    #[test]
    fn replacing_known_revision_and_skipping_have_explicit_results() {
        let (dir, mut library, doc) = fixture();
        let saved = library.put(&doc, None).unwrap();
        let mut changed = doc.clone();
        changed.body = "new body".into();
        let path = source(&dir, "replace.mflibrary", vec![changed]);
        let preview = library.preview_import(&[path], NOW).unwrap();
        assert_eq!(preview.items[0].status, "conflict");
        let skipped = library
            .apply_import(
                &preview.sources,
                &[decision(0, ImportMode::Skip, None)],
                NOW,
            )
            .unwrap();
        assert_eq!(skipped[0].status, "skipped");
        assert_eq!(library.get(&doc.metadata.id).unwrap().body, doc.body);
        let result = library
            .apply_import(
                &preview.sources,
                &[decision(0, ImportMode::Replace, Some(saved.revision))],
                NOW,
            )
            .unwrap();
        assert_eq!(result[0].status, "imported");
        assert_eq!(library.get(&doc.metadata.id).unwrap().body, "new body");
    }
    #[test]
    fn damaged_local_records_are_copy_only_in_preview() {
        let (dir, library, doc) = fixture();
        fs::create_dir_all(&library.root).unwrap();
        fs::write(
            library.root.join(format!("{}.md", doc.metadata.id)),
            "broken",
        )
        .unwrap();
        let path = source(&dir, "damaged.mflibrary", vec![doc]);
        let preview = library.preview_import(&[path], NOW).unwrap();
        assert_eq!(preview.items[0].status, "unreadable");
    }

    #[test]
    fn invalid_last_source_prevents_partial_preview_or_import() {
        let (dir, mut library, doc) = fixture();
        let valid = source(&dir, "valid.mflibrary", vec![doc.clone()]);
        let invalid = dir.path().join("invalid.md");
        fs::write(&invalid, "---\npassword: secret\n---\n").unwrap();
        let paths = vec![valid, invalid.to_str().unwrap().into()];
        assert!(library.preview_import(&paths, NOW).is_err());
        let sources = paths
            .into_iter()
            .map(|path| Source {
                digest: revision(&fs::read(&path).unwrap()),
                path,
            })
            .collect::<Vec<_>>();
        assert!(
            library
                .apply_import(&sources, &[decision(0, ImportMode::Create, None)], NOW)
                .is_err()
        );
        assert!(library.get(&doc.metadata.id).is_err());
    }

    #[test]
    fn batch_count_boundaries_and_directories_are_rejected() {
        let (dir, library, _) = fixture();
        let docs = (0..=MAX_BATCH_ENTRIES)
            .map(|_| Document::new("note", "fixture", "body", NOW))
            .collect::<Vec<_>>();
        assert!(Bundle::encode(docs[..MAX_BATCH_ENTRIES].to_vec()).is_ok());
        assert!(Bundle::encode(docs).is_err());
        assert!(
            library
                .preview_import(&[dir.path().to_str().unwrap().into()], NOW)
                .is_err()
        );
    }
}
