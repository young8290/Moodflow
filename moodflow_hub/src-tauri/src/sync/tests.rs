use super::*;
use moodflow_capsule::{Document, Library};
use moodflow_sync::{Backend, Expect, Read};
#[test]
fn real_folder_replicas_preserve_conflicts_and_never_publish_local_resource_paths() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let policy: backend::Policy = Arc::new(|| true);
    let mut target = backend::Folder::open(root, true, policy.clone()).unwrap();
    moodflow_sync::initialize(&mut target, None).unwrap();
    let (a_root, b_root) = (root.join("a"), root.join("b"));
    crate::private_fs::ensure(&a_root).unwrap();
    crate::private_fs::ensure(&b_root).unwrap();
    let mut library = Library::new(a_root.clone());
    let mut document = Document::new(
        "pdf",
        "synthetic title",
        r"C:\LOCAL_ONLY_FIXTURE\a.pdf",
        "2026",
    );
    document.metadata.properties.insert(
        "reference_title".into(),
        "public bibliographic title".into(),
    );
    document
        .metadata
        .properties
        .insert("local_path".into(), r"C:\LOCAL_ONLY_METADATA\b.pdf".into());
    let saved = library.put(&document, None).unwrap();
    let id = saved.metadata.id.clone();
    let mut sa = moodflow_sync::State::default();
    let mut sb = moodflow_sync::State::default();
    let mut a =
        local::Capsule::open(a_root.clone(), root.join("conflicts-a"), policy.clone()).unwrap();
    moodflow_sync::synchronize(&mut target, &mut a, &mut sa, None, &|| true).unwrap();
    for entry in std::fs::read_dir(root.join("MoodflowSync-v1/objects")).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!bytes.windows(10).any(|v| v == b"LOCAL_ONLY"));
    }
    let mut b =
        local::Capsule::open(b_root.clone(), root.join("conflicts-b"), policy.clone()).unwrap();
    moodflow_sync::synchronize(&mut target, &mut b, &mut sb, None, &|| true).unwrap();
    let mut local_draft = library.get(&id).unwrap();
    local_draft.metadata.title = "local offline title".into();
    library
        .put(&local_draft, Some(&local_draft.revision))
        .unwrap();
    let mut b_library = Library::new(b_root);
    let mut remote = b_library.get(&id).unwrap();
    assert!(remote.body.is_empty());
    remote.metadata.title = "remote winner title".into();
    b_library.put(&remote, Some(&remote.revision)).unwrap();
    let mut b =
        local::Capsule::open(root.join("b"), root.join("conflicts-b"), policy.clone()).unwrap();
    moodflow_sync::synchronize(&mut target, &mut b, &mut sb, None, &|| true).unwrap();
    let mut a = local::Capsule::open(a_root, root.join("conflicts-a"), policy).unwrap();
    let report = moodflow_sync::synchronize(&mut target, &mut a, &mut sa, None, &|| true).unwrap();
    assert_eq!(report.conflicts, 1);
    let final_document = library.get(&id).unwrap();
    assert_eq!(final_document.metadata.title, "remote winner title");
    assert!(final_document.body.contains("LOCAL_ONLY_FIXTURE"));
    assert!(
        final_document
            .metadata
            .properties
            .contains_key("local_path")
    );
    assert_eq!(
        local::conflicts(&root.join("conflicts-a")).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn folder_conditions_and_provider_conflicts_refuse_overwrite() {
    let temporary = tempfile::tempdir().unwrap();
    let mut a = backend::Folder::open(temporary.path(), true, Arc::new(|| true)).unwrap();
    let mut b = backend::Folder::open(temporary.path(), false, Arc::new(|| true)).unwrap();
    let old = a.put("index.mfsync", b"first", Expect::Missing).unwrap();
    a.put("index.mfsync", b"second", Expect::Revision(&old))
        .unwrap();
    assert!(
        b.put("index.mfsync", b"losing", Expect::Revision(&old))
            .is_err()
    );
    assert!(b.get("../outside").is_err());
    let Read::Present(blob) = b.get("index.mfsync").unwrap() else {
        panic!("missing")
    };
    assert_eq!(blob.bytes, b"second");
    std::fs::write(
        temporary
            .path()
            .join("MoodflowSync-v1/index (device conflict).mfsync"),
        b"do not overwrite",
    )
    .unwrap();
    assert!(a.get("index.mfsync").is_err());
    assert!(!local::conflict_name(&"🔒".repeat(30)));
    assert!(backend::Dav::url("http://example.invalid/").is_err());
    assert!(backend::Dav::url("https://user:secret@example.invalid/").is_err());
}
#[test]
fn remote_deletion_of_an_unknown_id_blocks_later_import() {
    let temporary = tempfile::tempdir().unwrap();
    let mut library = Library::new(temporary.path().into());
    let document = Document::new("note", "fixture", "fixture", "2026");
    library
        .record_remote_deletion(&document.metadata.id, "2026")
        .unwrap();
    assert!(library.put(&document, None).is_err());
    library
        .record_remote_deletion(&document.metadata.id, "2027")
        .unwrap();
}

#[test]
fn provider_repair_preserves_sources_in_an_archive_and_accepts_identical_header_copies() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let policy: backend::Policy = Arc::new(|| true);
    let mut folder = backend::Folder::open(root, true, policy.clone()).unwrap();
    moodflow_sync::initialize(&mut folder, None).unwrap();
    let capsule_root = root.join("capsule");
    crate::private_fs::ensure(&capsule_root).unwrap();
    let mut library = Library::new(capsule_root.clone());
    let doc = Document::new("note", "repair fixture", "repair fixture body", "2026");
    library.put(&doc, None).unwrap();
    let mut local = local::Capsule::open(capsule_root, root.join("conflicts"), policy).unwrap();
    let mut state = moodflow_sync::State::default();
    moodflow_sync::synchronize(&mut folder, &mut local, &mut state, None, &|| true).unwrap();
    let base = root.join("MoodflowSync-v1");
    for name in ["config", "index"] {
        std::fs::copy(
            base.join(format!("{name}.mfsync")),
            base.join(format!("{name} (synthetic conflict).mfsync")),
        )
        .unwrap();
    }
    assert!(folder.get("index.mfsync").is_err());
    let sources = folder.repair_sources().unwrap();
    assert_eq!(sources.len(), 2);
    let archive = sources
        .iter()
        .map(|(name, bytes)| (name.clone(), moodflow_sync::digest(bytes)))
        .collect::<Vec<_>>();
    let copies = manifest_copies(&mut folder, sources).unwrap();
    assert_eq!(copies.len(), 1);
    moodflow_sync::recover_manifests(&mut folder, &mut local, &mut state, None, &copies, &|| true)
        .unwrap();
    folder.archive_sources(&archive).unwrap();
    assert!(!base.join("index (synthetic conflict).mfsync").exists());
    assert_eq!(
        std::fs::read_dir(base.join("provider-conflicts"))
            .unwrap()
            .count(),
        2
    );
    assert_eq!(library.get(&doc.metadata.id).unwrap().body, doc.body);
}

#[test]
fn vault_sync_requires_opt_in_and_transports_only_independent_encrypted_backups() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let policy: backend::Policy = Arc::new(|| true);
    let phrase = "synthetic outer sync phrase which is not the vault phrase";
    let backup_phrase = "synthetic independent portable backup phrase";
    let mut snapshot = moodflow_vault::Snapshot::empty();
    let mut entry = moodflow_vault::Entry::new();
    entry.title = "synthetic secret title".into();
    entry.password = "synthetic private password".into();
    snapshot.entries.push(entry);
    let bytes = moodflow_vault::backup::encrypt(&snapshot, backup_phrase).unwrap();
    let id = moodflow_vault::backup::inspect(&bytes).unwrap();
    let copies_a = root.join("copy-a");
    let copies_b = root.join("copy-b");
    crate::private_fs::ensure(&copies_a.join("vault-backups")).unwrap();
    std::fs::write(
        copies_a
            .join("vault-backups")
            .join(format!("{id}.mfvbackup")),
        &bytes,
    )
    .unwrap();
    let mut backend = backend::Folder::open(root, true, policy.clone()).unwrap();
    moodflow_sync::initialize(&mut backend, Some(phrase)).unwrap();
    let mut a = vault_backup::Copies::new(
        local::Capsule::open(
            root.join("capsule-a"),
            root.join("conflict-a"),
            policy.clone(),
        )
        .unwrap(),
        copies_a,
        true,
        policy.clone(),
    )
    .unwrap();
    let (mut sa, mut sb) = (
        moodflow_sync::State::default(),
        moodflow_sync::State::default(),
    );
    moodflow_sync::synchronize(&mut backend, &mut a, &mut sa, Some(phrase), &|| true).unwrap();
    let mut off = vault_backup::Copies::new(
        local::Capsule::open(
            root.join("capsule-b"),
            root.join("conflict-b"),
            policy.clone(),
        )
        .unwrap(),
        copies_b.clone(),
        false,
        policy.clone(),
    )
    .unwrap();
    let report =
        moodflow_sync::synchronize(&mut backend, &mut off, &mut sb, Some(phrase), &|| true)
            .unwrap();
    assert_eq!(report.deferred, 1);
    assert_eq!(
        std::fs::read_dir(copies_b.join("vault-backups"))
            .unwrap()
            .count(),
        0
    );
    let mut on = vault_backup::Copies::new(
        local::Capsule::open(
            root.join("capsule-b"),
            root.join("conflict-b"),
            policy.clone(),
        )
        .unwrap(),
        copies_b.clone(),
        true,
        policy,
    )
    .unwrap();
    moodflow_sync::synchronize(&mut backend, &mut on, &mut sb, Some(phrase), &|| true).unwrap();
    let received = std::fs::read(
        copies_b
            .join("vault-backups")
            .join(format!("{id}.mfvbackup")),
    )
    .unwrap();
    assert_eq!(received, bytes);
    assert_eq!(
        moodflow_vault::backup::decrypt(&received, backup_phrase)
            .unwrap()
            .entries[0]
            .password,
        "synthetic private password"
    );
    assert!(!root.join("vault").exists());
    assert_eq!(
        std::fs::read_dir(root.join("capsule-b"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|s| s == "md"))
            .count(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webdav_uses_strong_etags_authentication_and_conditional_puts_without_redirects() {
    use axum::{
        Router,
        body::Bytes,
        extract::{Path as AxumPath, State},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    type Files = Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>>;
    async fn read(
        State(files): State<Files>,
        AxumPath(key): AxumPath<String>,
        headers: HeaderMap,
    ) -> axum::response::Response {
        use base64::Engine;
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("fixture:fixture-password")
        );
        if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(&expected) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if key == "config.mfsync" {
            return (
                StatusCode::FOUND,
                [("location", "/MoodflowSync-v1/redirect-target")],
            )
                .into_response();
        }
        let mut files = files.lock().unwrap();
        if key == "redirect-target" {
            files.insert("followed_redirect".into(), vec![]);
            return StatusCode::OK.into_response();
        }
        let Some(bytes) = files.get(&key) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        if bytes == b"without-etag" {
            return bytes.clone().into_response();
        }
        let etag = format!("\"{}\"", moodflow_sync::digest(bytes));
        ([(reqwest::header::ETAG, etag)], bytes.clone()).into_response()
    }
    async fn write(
        State(files): State<Files>,
        AxumPath(key): AxumPath<String>,
        headers: HeaderMap,
        bytes: Bytes,
    ) -> StatusCode {
        use base64::Engine;
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("fixture:fixture-password")
        );
        if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(&expected) {
            return StatusCode::UNAUTHORIZED;
        }
        let mut files = files.lock().unwrap();
        let current = files
            .get(&key)
            .map(|b| format!("\"{}\"", moodflow_sync::digest(b)));
        let valid = if let Some(expected) = headers.get("if-match") {
            current.as_deref() == expected.to_str().ok()
        } else {
            headers.get("if-none-match").is_some_and(|h| h == "*") && current.is_none()
        };
        if !valid {
            return StatusCode::PRECONDITION_FAILED;
        }
        files.insert(key, bytes.to_vec());
        StatusCode::CREATED
    }
    let files: Files = Default::default();
    let router = Router::new()
        .route("/MoodflowSync-v1/*key", get(read).put(write))
        .with_state(files.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/MoodflowSync-v1/", listener.local_addr().unwrap());
    let (stop, done) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = done.await;
            })
            .await
            .unwrap();
    });
    tokio::task::spawn_blocking(move || {
        let mut dav = backend::Dav::fixture(&base);
        dav.put("index.mfsync", b"first", Expect::Missing).unwrap();
        let Read::Present(first) = dav.get("index.mfsync").unwrap() else {
            panic!("missing")
        };
        dav.put("index.mfsync", b"second", Expect::Revision(&first.revision))
            .unwrap();
        assert!(
            dav.put("index.mfsync", b"loser", Expect::Revision(&first.revision))
                .is_err()
        );
        assert!(dav.get("config.mfsync").is_err());
        let Read::Present(second) = dav.get("index.mfsync").unwrap() else {
            panic!("missing")
        };
        dav.put(
            "index.mfsync",
            b"without-etag",
            Expect::Revision(&second.revision),
        )
        .unwrap();
        assert!(dav.get("index.mfsync").is_err());
    })
    .await
    .unwrap();
    let _ = stop.send(());
    server.await.unwrap();
    assert!(!files.lock().unwrap().contains_key("followed_redirect"));
    assert_eq!(files.lock().unwrap()["index.mfsync"], b"without-etag");
}
