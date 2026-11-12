#[cfg(test)]
mod tests {
    use crate::recovery::*;
    fn fixture() -> (tempfile::TempDir, Recovery, Library, String, Document) {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("conflicts");
        fs::create_dir_all(&sources).unwrap();
        let recovery = Recovery::new(sources.clone(), dir.path().join("recovery"));
        let mut library = Library::new(dir.path().join("library"));
        let saved = Document::new("note", "saved title", "shared\nsaved line", "2026-11-01");
        let raw = saved.to_markdown().unwrap();
        let name = format!("{}-{}.md", saved.metadata.id, revision(raw.as_bytes()));
        fs::write(sources.join(&name), raw).unwrap();
        let mut current = saved.clone();
        current.body = "shared\ncurrent line".into();
        current.metadata.properties.insert(
            "custom_unknown".into(),
            crate::PropertyValue::String("keep me".into()),
        );
        let current = library.put(&current, None).unwrap();
        (dir, recovery, library, name, current)
    }
    fn request(recovery: &Recovery, library: &Library, name: &str, mode: Mode) -> Resolution {
        let review = recovery.review(library, name).unwrap();
        Resolution {
            name: name.into(),
            source_revision: review.source_revision,
            current_revision: review.current.map(|d| d.revision),
            mode,
            operation_id: uuid::Uuid::new_v4().to_string(),
            updated_at: "2026-11-12".into(),
            title: "resolved title".into(),
            body: "manually merged".into(),
            tags: vec!["reviewed".into()],
        }
    }
    #[test]
    fn review_shows_current_saved_diff_and_unknown_metadata_without_writing() {
        let (_dir, recovery, library, name, current) = fixture();
        let review = recovery.review(&library, &name).unwrap();
        assert!(review.can_replace);
        assert_eq!(review.current_state, "available");
        assert_eq!(review.current.unwrap().revision, current.revision);
        assert_eq!(review.diff.added, 1);
        assert_eq!(review.diff.removed, 1);
        assert!(review.fields.iter().any(|f| f.key == "custom_unknown"));
        assert!(!recovery.controls.exists());
        let listing = recovery.list("SAVED title").unwrap();
        assert_eq!(listing.total, 1);
        assert_eq!(recovery.list("not present").unwrap().total, 0);
    }
    #[test]
    fn replacement_preserves_exact_original_bytes_and_current_metadata() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let original = fs::read(library.path(&current.metadata.id).unwrap()).unwrap();
        let source = fs::read(recovery.sources.join(&name)).unwrap();
        let request = request(&recovery, &library, &name, Mode::Replace);
        let result = recovery.resolve(&mut library, &request, &|| true).unwrap();
        assert_eq!(result.id, current.metadata.id);
        assert_eq!(
            fs::read(
                recovery
                    .controls
                    .join("backups")
                    .join(result.backup.unwrap())
            )
            .unwrap(),
            original
        );
        assert_eq!(fs::read(recovery.sources.join(&name)).unwrap(), source);
        let merged = library.get(&result.id).unwrap();
        assert_eq!(merged.body, "manually merged");
        assert_eq!(merged.metadata.properties, current.metadata.properties);
        assert_eq!(merged.metadata.tags, vec!["reviewed"]);
        assert!(
            recovery
                .resolve(&mut library, &request, &|| true)
                .unwrap()
                .repeated
        );
    }
    #[test]
    fn copy_retries_use_one_id_and_do_not_modify_the_original() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let request = request(&recovery, &library, &name, Mode::Copy);
        let first = recovery.resolve(&mut library, &request, &|| true).unwrap();
        let second = recovery.resolve(&mut library, &request, &|| true).unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.repeated);
        assert_eq!(
            library.get(&current.metadata.id).unwrap().revision,
            current.revision
        );
        assert_eq!(library.search("", None, 100).unwrap().items.len(), 2);
    }
    #[test]
    fn stale_target_and_changed_source_never_overwrite_newer_data() {
        let (_dir, recovery, mut library, name, mut current) = fixture();
        let request = request(&recovery, &library, &name, Mode::Replace);
        current.body = "newer edit".into();
        let newer = library.put(&current, Some(&current.revision)).unwrap();
        assert!(recovery.resolve(&mut library, &request, &|| true).is_err());
        assert_eq!(
            library.get(&current.metadata.id).unwrap().revision,
            newer.revision
        );
        let copy = self::request(&recovery, &library, &name, Mode::Copy);
        fs::write(recovery.sources.join(&name), b"tampered").unwrap();
        assert!(recovery.resolve(&mut library, &copy, &|| true).is_err());
        assert!(library.get(&copy.operation_id).is_err());
    }
    #[test]
    fn tombstones_disable_replace_and_copy_never_resurrects_the_old_id() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let replace = request(&recovery, &library, &name, Mode::Replace);
        library
            .delete(&current.metadata.id, &current.revision, "now")
            .unwrap();
        let review = recovery.review(&library, &name).unwrap();
        assert_eq!(review.current_state, "deleted");
        assert!(!review.can_replace);
        assert!(recovery.resolve(&mut library, &replace, &|| true).is_err());
        let copy = request(&recovery, &library, &name, Mode::Copy);
        let result = recovery.resolve(&mut library, &copy, &|| true).unwrap();
        assert_ne!(result.id, current.metadata.id);
        assert!(library.get(&current.metadata.id).is_err());
    }
    #[test]
    fn invalid_and_damaged_sources_are_isolated_and_paths_are_rejected() {
        let (_dir, recovery, library, name, _) = fixture();
        let bad = format!("{}-{}.md", uuid::Uuid::new_v4(), "a".repeat(64));
        fs::write(recovery.sources.join(&bad), "damaged").unwrap();
        let listing = recovery.list("").unwrap();
        assert_eq!(listing.total, 1);
        assert_eq!(listing.unreadable, vec![bad]);
        for name in ["../outside.md", "é.md", &name.replace(".md", ".json")] {
            assert!(recovery.review(&library, name).is_err());
        }
    }
    #[test]
    fn privacy_cancellation_and_operation_id_collision_fail_closed() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let mut r = request(&recovery, &library, &name, Mode::Copy);
        assert!(recovery.resolve(&mut library, &r, &|| false).is_err());
        r.operation_id = current.metadata.id.clone();
        assert!(recovery.resolve(&mut library, &r, &|| true).is_err());
        assert_eq!(
            library.get(&current.metadata.id).unwrap().revision,
            current.revision
        );
    }
    #[test]
    fn missing_or_damaged_current_versions_remain_copy_only() {
        let (_dir, recovery, mut library, name, current) = fixture();
        fs::write(library.path(&current.metadata.id).unwrap(), "broken").unwrap();
        assert_eq!(
            recovery.review(&library, &name).unwrap().current_state,
            "unreadable"
        );
        let mut request = request(&recovery, &library, &name, Mode::Replace);
        assert!(recovery.resolve(&mut library, &request, &|| true).is_err());
        request.mode = Mode::Copy;
        recovery.resolve(&mut library, &request, &|| true).unwrap();
        fs::remove_file(library.path(&current.metadata.id).unwrap()).unwrap();
        assert_eq!(
            recovery.review(&library, &name).unwrap().current_state,
            "missing"
        );
    }
    #[test]
    fn replacement_backups_can_be_reviewed_and_recovered_as_new_documents() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let request = request(&recovery, &library, &name, Mode::Replace);
        let result = recovery.resolve(&mut library, &request, &|| true).unwrap();
        let backups = Recovery::new(recovery.controls.join("backups"), recovery.controls.clone());
        let list = backups.list("").unwrap();
        assert_eq!(list.total, 1);
        let review = backups
            .review(&library, result.backup.as_ref().unwrap())
            .unwrap();
        assert_eq!(review.saved.body, current.body);
        let restore = self::request(&backups, &library, &review.name, Mode::Copy);
        assert_ne!(
            backups
                .resolve(&mut library, &restore, &|| true)
                .unwrap()
                .id,
            current.metadata.id
        );
    }
    #[test]
    fn comparison_is_bounded_and_late_changes_stay_visible() {
        let left = (0..1000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let right = left.replace("line 950", "changed near end");
        let result = diff::compare(&left, &right);
        assert!(result.rows.iter().any(|r| r.text == "changed near end"));
        assert!(result.rows.len() <= 400);
        assert_eq!(result.added, 1);
        assert_eq!(result.removed, 1);
        assert!(!result.detailed);
        let huge = diff::compare(&"x\n".repeat(5000), "x");
        assert!(huge.truncated);
        assert!(huge.rows.is_empty());
        let long = diff::compare(&"x".repeat(10000), "other");
        assert!(long.truncated);
        assert!(long.rows.iter().all(|r| r.text.chars().count() <= 512));
    }
    #[test]
    fn detailed_diff_handles_unicode_additions_deletions_and_empty_text() {
        let comparison = diff::compare("one\n中文\nlast", "one\n🙂\nnew\nlast");
        assert!(comparison.detailed);
        assert_eq!(comparison.added, 2);
        assert_eq!(comparison.removed, 1);
        assert!(
            comparison
                .rows
                .iter()
                .any(|r| r.kind == "same" && r.text == "last")
        );
        let same = diff::compare("", "");
        assert_eq!(same.added + same.removed, 0);
    }
    #[test]
    fn pause_before_persistence_keeps_the_original_and_source_unchanged() {
        let (_dir, recovery, mut library, name, current) = fixture();
        for mode in [Mode::Copy, Mode::Replace] {
            let request = request(&recovery, &library, &name, mode);
            let calls = std::cell::Cell::new(0);
            assert!(
                recovery
                    .resolve(&mut library, &request, &|| {
                        calls.set(calls.get() + 1);
                        calls.get() == 1
                    })
                    .is_err()
            );
            assert_eq!(
                library.get(&current.metadata.id).unwrap().revision,
                current.revision
            );
            assert!(recovery.source(&name).is_ok());
        }
    }
    #[test]
    fn a_new_edit_after_backup_is_not_overwritten_at_publish() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let request = request(&recovery, &library, &name, Mode::Replace);
        let root = library.root.clone();
        let calls = std::cell::Cell::new(0);
        let allowed = || {
            calls.set(calls.get() + 1);
            if calls.get() == 4 {
                let mut newer = current.clone();
                newer.body = "changed during recovery".into();
                Library::new(root.clone())
                    .put(&newer, Some(&current.revision))
                    .unwrap();
            }
            true
        };
        assert!(recovery.resolve(&mut library, &request, &allowed).is_err());
        assert_eq!(
            library.get(&current.metadata.id).unwrap().body,
            "changed during recovery"
        );
        assert_eq!(
            fs::read_dir(recovery.controls.join("backups"))
                .unwrap()
                .count(),
            1
        );
    }
    #[test]
    fn backup_capacity_failure_preserves_the_current_record() {
        let (_dir, recovery, mut library, name, current) = fixture();
        let request = request(&recovery, &library, &name, Mode::Replace);
        let backups = recovery.controls.join("backups");
        fs::create_dir_all(&backups).unwrap();
        let large = fs::File::create(backups.join("existing-fixture")).unwrap();
        large.set_len(MAX_BYTES).unwrap();
        assert!(recovery.resolve(&mut library, &request, &|| true).is_err());
        assert_eq!(
            library.get(&current.metadata.id).unwrap().revision,
            current.revision
        );
    }
    #[test]
    fn explicit_deletion_checks_the_snapshot_and_never_changes_current_data() {
        let (_dir, recovery, library, name, current) = fixture();
        let revision = recovery.review(&library, &name).unwrap().source_revision;
        assert!(recovery.delete_source(&name, "wrong", &|| true).is_err());
        assert!(recovery.delete_source(&name, &revision, &|| false).is_err());
        assert!(recovery.source(&name).is_ok());
        recovery.delete_source(&name, &revision, &|| true).unwrap();
        assert!(recovery.source(&name).is_err());
        assert_eq!(
            library.get(&current.metadata.id).unwrap().revision,
            current.revision
        );
    }
}
