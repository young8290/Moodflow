#[cfg(test)]
mod tests {
    use crate::catalog::*;
    use crate::{FrozenModel, Kind, Message, Role, Session, Store};
    use std::fs;
    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("workspace"));
        (dir, store)
    }
    fn fixture(store: &Store, kind: Kind, title: &str, body: &str, time: u64) -> Session {
        let mut session = Session::new(kind, title.into(), time);
        session
            .append(
                vec![Message::new(
                    if kind == Kind::Inbox {
                        Role::External
                    } else {
                        Role::Note
                    },
                    body.into(),
                    time,
                )],
                None,
                time,
            )
            .unwrap();
        session.unread = false;
        store.save(&session, None).unwrap()
    }
    fn select(session: &Session) -> Selection {
        Selection {
            id: session.id.clone(),
            revision: session.revision.clone(),
        }
    }
    #[test]
    fn searches_title_body_and_filenames_with_and_terms_and_case_folding() {
        let (_dir, store) = store();
        let mut s = fixture(&store, Kind::Inbox, "Project 中文", "FIRST 正文", 1);
        s.messages[0].files.push(crate::Attachment {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Report.pdf".into(),
            bytes: 4,
            sha256: "a".repeat(64),
        });
        s = store.save(&s, Some(&s.revision)).unwrap();
        for query in ["PROJECT first", "中文 正文", "REPORT.PDF"] {
            let result = store
                .search(&SearchRequest {
                    query: query.into(),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(result.total, 1);
            assert_eq!(result.items[0].id, s.id);
            assert_eq!(result.items[0].matched_messages, 1);
        }
        assert_eq!(
            store
                .search(&SearchRequest {
                    query: "first absent".into(),
                    ..Default::default()
                })
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            store
                .search(&SearchRequest {
                    query: "first".into(),
                    scope: SearchScope::Title,
                    ..Default::default()
                })
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            store
                .search(&SearchRequest {
                    query: "project".into(),
                    scope: SearchScope::Messages,
                    ..Default::default()
                })
                .unwrap()
                .total,
            0
        );
    }
    #[test]
    fn filtering_and_stable_pagination_use_id_tie_breaks() {
        let (_dir, store) = store();
        let a = fixture(&store, Kind::Inbox, "alpha", "one", 1);
        let mut b = fixture(&store, Kind::Chat, "beta", "two", 1);
        b.archived = true;
        b.unread = true;
        store.save(&b, Some(&b.revision)).unwrap();
        fixture(&store, Kind::Terminal, "gamma", "three", 1);
        let all = SearchRequest {
            archive: ArchiveFilter::All,
            sort: Sort::Title,
            limit: 1,
            ..Default::default()
        };
        let first = store.search(&all).unwrap();
        let second = store
            .search(&SearchRequest {
                offset: 1,
                ..all.clone()
            })
            .unwrap();
        assert_eq!(first.total, 3);
        assert_eq!(first.items[0].id, a.id);
        assert_eq!(second.items[0].title, "beta");
        let unread = store
            .search(&SearchRequest {
                unread_only: true,
                archive: ArchiveFilter::All,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(unread.total, 1);
        assert_eq!(unread.items[0].id, b.id);
        assert_eq!(store.search(&SearchRequest::default()).unwrap().total, 2);
        assert_eq!(
            store
                .search(&SearchRequest {
                    archive: ArchiveFilter::Archived,
                    kind: Some(Kind::Inbox),
                    ..Default::default()
                })
                .unwrap()
                .total,
            0
        );
    }
    #[test]
    fn damaged_record_is_isolated_and_healthy_writes_still_work() {
        let (_dir, store) = store();
        let good = fixture(&store, Kind::Inbox, "healthy", "one", 1);
        let broken = uuid::Uuid::new_v4().to_string();
        fs::write(store.root.join(format!("{broken}.json")), b"broken-json").unwrap();
        let result = store.search(&SearchRequest::default()).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.unreadable, vec![broken.clone()]);
        assert_eq!(store.available_sessions().unwrap().0.len(), 1);
        store.rename_session(&select(&good), "changed", 2).unwrap();
        fixture(&store, Kind::Chat, "new", "message", 3);
        assert_eq!(
            fs::read(store.root.join(format!("{broken}.json"))).unwrap(),
            b"broken-json"
        );
    }
    #[test]
    fn deleted_and_unrelated_files_never_appear_in_catalog() {
        let (_dir, store) = store();
        let s = fixture(&store, Kind::Inbox, "delete", "hidden", 1);
        store.remove(&s.id, &s.revision).unwrap();
        fs::write(store.root.join("settings.json"), b"{}").unwrap();
        assert_eq!(store.search(&SearchRequest::default()).unwrap().total, 0);
    }
    #[test]
    fn all_limits_are_checked_before_search() {
        let (_dir, store) = store();
        for request in [
            SearchRequest {
                limit: 0,
                ..Default::default()
            },
            SearchRequest {
                limit: 51,
                ..Default::default()
            },
            SearchRequest {
                offset: 201,
                ..Default::default()
            },
            SearchRequest {
                query: "a ".repeat(9),
                ..Default::default()
            },
            SearchRequest {
                query: "𠮷".repeat(129),
                ..Default::default()
            },
            SearchRequest {
                query: "a\0b".into(),
                ..Default::default()
            },
        ] {
            assert!(store.search(&request).is_err());
        }
        assert!(!store.root.exists());
    }
    #[test]
    fn snippets_are_bounded_and_do_not_return_whole_sessions() {
        let (_dir, store) = store();
        let mut s = fixture(
            &store,
            Kind::Inbox,
            "bounded",
            &format!("{}needle{}", "前".repeat(1000), "后".repeat(1000)),
            1,
        );
        s.append(
            (0..5)
                .map(|_| Message::new(Role::External, "needle other".into(), 1))
                .collect(),
            None,
            1,
        )
        .unwrap();
        store.save(&s, Some(&s.revision)).unwrap();
        let page = store
            .search(&SearchRequest {
                query: "needle".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.items[0].matched_messages, 6);
        assert_eq!(page.items[0].matches.len(), 3);
        let snippet = &page.items[0].matches[0].snippet;
        assert!(snippet.chars().count() <= 182);
        assert!(snippet.contains("needle"));
        let wire = serde_json::to_value(page).unwrap();
        assert!(wire["items"][0].get("messages").is_none());
    }
    #[test]
    fn partial_changes_preserve_newer_content_and_frozen_model() {
        let (_dir, store) = store();
        let mut a = fixture(&store, Kind::Chat, "frozen", "content", 1);
        a.model = Some(FrozenModel {
            endpoint: "hashed-endpoint".into(),
            model: "pinned".into(),
        });
        a.instructions = "instructions".into();
        a = store.save(&a, Some(&a.revision)).unwrap();
        let b = fixture(&store, Kind::Inbox, "other", "old", 1);
        let newer = store.rename_session(&select(&b), "newer", 2).unwrap();
        let outcomes = store
            .change_sessions(
                &[select(&a), select(&b)],
                &Change::Archive { value: true },
                3,
            )
            .unwrap();
        assert_eq!(outcomes[0].status, "updated");
        assert_eq!(outcomes[1].status, "conflict");
        let archived = store.get(&a.id).unwrap();
        assert!(archived.archived);
        assert_eq!(archived.instructions, a.instructions);
        assert_eq!(archived.model, a.model);
        assert_eq!(archived.messages, a.messages);
        assert_eq!(store.get(&b.id).unwrap().revision, newer.revision);
    }
    #[test]
    fn rename_and_export_reject_stale_or_deleted_sessions() {
        let (_dir, store) = store();
        let original = fixture(&store, Kind::Chat, "title", "body", 1);
        let renamed = store
            .rename_session(&select(&original), "new title", 2)
            .unwrap();
        assert!(
            store
                .rename_session(&select(&original), "stale", 3)
                .is_err()
        );
        assert!(
            store
                .transcript(&select(&original), TranscriptFormat::Json, false)
                .is_err()
        );
        store.remove(&renamed.id, &renamed.revision).unwrap();
        assert!(
            store
                .transcript(&select(&renamed), TranscriptFormat::Markdown, false)
                .is_err()
        );
    }
    #[test]
    fn transcript_preserves_roles_but_omits_instructions_by_default() {
        let (_dir, store) = store();
        let mut s = fixture(
            &store,
            Kind::Inbox,
            "export",
            "<script>malicious</script>\n```\ntext",
            1,
        );
        s.instructions = "private custom instruction".into();
        s = store.save(&s, Some(&s.revision)).unwrap();
        let md = String::from_utf8(
            store
                .transcript(&select(&s), TranscriptFormat::Markdown, false)
                .unwrap(),
        )
        .unwrap();
        assert!(md.contains("````text"));
        assert!(!md.contains("private custom instruction"));
        assert!(md.contains("External"));
        let json: serde_json::Value = serde_json::from_slice(
            &store
                .transcript(&select(&s), TranscriptFormat::Json, true)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(json["format"], "moodflow-transcript");
        assert_eq!(
            json["session"]["instructions"],
            "private custom instruction"
        );
        assert!(json["session"].get("revision").is_none());
    }
    #[test]
    fn invalid_bulk_selection_never_changes_prior_rows() {
        let (_dir, store) = store();
        let s = fixture(&store, Kind::Inbox, "keep", "body", 1);
        assert!(
            store
                .change_sessions(
                    &[select(&s), select(&s)],
                    &Change::Unread { value: true },
                    2
                )
                .is_err()
        );
        assert!(
            store
                .change_sessions(
                    &[
                        select(&s),
                        Selection {
                            id: "../invalid".into(),
                            revision: "a".repeat(64)
                        }
                    ],
                    &Change::Unread { value: true },
                    2
                )
                .is_err()
        );
        assert!(!store.get(&s.id).unwrap().unread);
    }
    #[test]
    fn damaged_records_still_consume_count_and_byte_quotas() {
        let (_dir, store) = store();
        fs::create_dir_all(&store.root).unwrap();
        for _ in 0..crate::MAX_SESSIONS {
            fs::write(
                store.root.join(format!("{}.json", uuid::Uuid::new_v4())),
                "broken",
            )
            .unwrap();
        }
        let new = Session::new(Kind::Inbox, "over count".into(), 1);
        assert!(store.save(&new, None).is_err());
        assert!(!store.root.join(format!("{}.json", new.id)).exists());
        let (_bytes_dir, bytes_store) = self::store();
        fs::create_dir_all(&bytes_store.root).unwrap();
        let file = fs::File::create(
            bytes_store
                .root
                .join(format!("{}.json", uuid::Uuid::new_v4())),
        )
        .unwrap();
        file.set_len(crate::MAX_TOTAL_BYTES).unwrap();
        assert!(
            bytes_store
                .save(&Session::new(Kind::Inbox, "over bytes".into(), 1), None)
                .is_err()
        );
    }
    #[test]
    fn markdown_title_cannot_introduce_html_or_new_headings() {
        let (_dir, store) = store();
        let s = fixture(
            &store,
            Kind::Inbox,
            "<script>alert(1)</script>\n# injected",
            "body",
            1,
        );
        let md = String::from_utf8(
            store
                .transcript(&select(&s), TranscriptFormat::Markdown, false)
                .unwrap(),
        )
        .unwrap();
        assert!(!md.contains("<script>"));
        assert!(!md.contains("\n# injected"));
        assert!(md.contains("&lt;script&gt;"));
    }
    #[test]
    fn management_actions_are_not_external_mcp_methods_or_tools() {
        use serde_json::json;
        for name in ["search", "manage", "rename", "transcript"] {
            assert!(
                crate::protocol::parse(
                    json!({"jsonrpc":"2.0","id":1,"method":name,"params":{}}),
                    None,
                    None,
                    None
                )
                .is_err()
            );
            assert!(crate::protocol::parse(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":{}}}), None, None, None).is_err());
        }
        assert_eq!(
            crate::protocol::tools()["tools"].as_array().unwrap().len(),
            5
        );
    }
}
