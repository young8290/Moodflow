use super::*;
#[derive(Default)]
struct Remote {
    files: BTreeMap<String, Vec<u8>>,
    deferred: BTreeSet<String>,
    reject_manifest: bool,
}
impl Backend for Remote {
    fn get(&mut self, key: &str) -> Result<Read> {
        Ok(if self.deferred.contains(key) {
            Read::Deferred
        } else if let Some(bytes) = self.files.get(key) {
            Read::Present(Blob {
                bytes: bytes.clone(),
                revision: digest(bytes),
            })
        } else {
            Read::Missing
        })
    }
    fn put(&mut self, key: &str, bytes: &[u8], expected: Expect<'_>) -> Result<String> {
        if key == "index.mfsync" && self.reject_manifest {
            anyhow::bail!("sync.conflict");
        }
        let current = self.files.get(key).map(|v| digest(v));
        let matches = match expected {
            Expect::Missing => current.is_none(),
            Expect::Revision(r) => current.as_deref() == Some(r),
        };
        ensure!(matches, "sync.conflict");
        self.files.insert(key.into(), bytes.to_vec());
        Ok(digest(bytes))
    }
}
#[derive(Default)]
struct Device {
    entries: BTreeMap<String, Vec<u8>>,
    deleted: BTreeSet<String>,
    conflicts: BTreeMap<String, Vec<u8>>,
    edit_before_apply: bool,
}
impl Local for Device {
    fn snapshot(&mut self) -> Result<Snapshot> {
        Ok(Snapshot {
            entries: self.entries.clone(),
            deleted: self.deleted.clone(),
            deferred: BTreeSet::new(),
        })
    }
    fn apply(&mut self, id: &str, bytes: &[u8], expected: Option<&[u8]>) -> Result<()> {
        if self.edit_before_apply {
            self.entries
                .insert(id.into(), b"newer local editor draft".to_vec());
            self.edit_before_apply = false;
        }
        ensure!(
            self.entries.get(id).map(Vec::as_slice) == expected,
            "sync.local_conflict"
        );
        ensure!(!self.deleted.contains(id), "sync.local_conflict");
        self.entries.insert(id.into(), bytes.into());
        Ok(())
    }
    fn remove(&mut self, id: &str, expected: Option<&[u8]>) -> Result<()> {
        ensure!(
            self.entries.get(id).map(Vec::as_slice) == expected,
            "sync.local_conflict"
        );
        self.deleted.insert(id.into());
        self.entries.remove(id);
        Ok(())
    }
    fn conflict(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        self.conflicts
            .insert(format!("{id}-{}", digest(bytes)), bytes.into());
        Ok(())
    }
}
fn fixture() -> (Remote, Device, State, String) {
    let mut remote = Remote::default();
    initialize(&mut remote, None).unwrap();
    let key = uuid::Uuid::new_v4().to_string();
    let mut device = Device::default();
    device
        .entries
        .insert(key.clone(), b"original synthetic note".to_vec());
    (remote, device, State::default(), key)
}
fn sync(remote: &mut Remote, device: &mut Device, state: &mut State) -> Report {
    synchronize(remote, device, state, None, &|| true).unwrap()
}

#[test]
fn alternating_offline_edits_keep_the_local_loser_without_comparing_clocks() {
    let (mut remote, mut a, mut sa, key) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let (mut b, mut sb) = (Device::default(), State::default());
    sync(&mut remote, &mut b, &mut sb);
    a.entries.insert(key.clone(), b"a offline draft".to_vec());
    b.entries.insert(key.clone(), b"b remote winner".to_vec());
    sync(&mut remote, &mut b, &mut sb);
    let result = sync(&mut remote, &mut a, &mut sa);
    assert_eq!(result.conflicts, 1);
    assert_eq!(a.entries[&key], b"b remote winner");
    assert!(a.conflicts.values().any(|b| b == b"a offline draft"));
    assert_eq!(sync(&mut remote, &mut a, &mut sa).downloaded, 0);
}
#[test]
fn permanent_tombstones_win_and_preserve_an_offline_edit() {
    let (mut remote, mut a, mut sa, key) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let (mut b, mut sb) = (Device::default(), State::default());
    sync(&mut remote, &mut b, &mut sb);
    a.entries.remove(&key);
    a.deleted.insert(key.clone());
    b.entries
        .insert(key.clone(), b"offline unsent draft".to_vec());
    sync(&mut remote, &mut a, &mut sa);
    let result = sync(&mut remote, &mut b, &mut sb);
    assert_eq!(result.deleted, 1);
    assert!(b.deleted.contains(&key));
    assert!(b.entries.is_empty());
    assert!(b.conflicts.values().any(|v| v == b"offline unsent draft"));
    let (mut c, mut sc) = (Device::default(), State::default());
    sync(&mut remote, &mut c, &mut sc);
    assert!(c.deleted.contains(&key));
    assert!(c.entries.is_empty());
}
#[test]
fn cloud_placeholders_and_missing_files_are_never_interpreted_as_deletion() {
    let (mut remote, mut a, mut sa, key) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let object = remote
        .files
        .keys()
        .find(|k| k.starts_with("objects/"))
        .unwrap()
        .clone();
    remote.deferred.insert(object.clone());
    let (mut b, mut sb) = (Device::default(), State::default());
    assert_eq!(sync(&mut remote, &mut b, &mut sb).deferred, 1);
    assert!(b.entries.is_empty());
    assert!(b.deleted.is_empty());
    remote.deferred.clear();
    sync(&mut remote, &mut b, &mut sb);
    assert_eq!(b.entries[&key], a.entries[&key]);
    b.entries.remove(&key);
    a.entries
        .insert(key.clone(), b"remote later draft".to_vec());
    sync(&mut remote, &mut a, &mut sa);
    assert_eq!(sync(&mut remote, &mut b, &mut sb).deferred, 1);
    assert!(b.deleted.is_empty());
    assert!(b.entries.is_empty());
}
#[test]
fn a_failed_manifest_compare_and_swap_never_applies_remote_changes() {
    let (mut remote, mut a, mut sa, _) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let extra = uuid::Uuid::new_v4().to_string();
    let mut b = Device::default();
    b.entries.insert(extra.clone(), b"unsynced local".to_vec());
    remote.reject_manifest = true;
    let mut sb = State::default();
    assert!(synchronize(&mut remote, &mut b, &mut sb, None, &|| true).is_err());
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[&extra], b"unsynced local");
    assert!(sb.baseline.is_empty());
    remote.reject_manifest = false;
    sync(&mut remote, &mut b, &mut sb);
    assert_eq!(b.entries.len(), 2);
}
#[test]
fn unannounced_remote_disappearance_and_corrupt_objects_fail_closed() {
    let (mut remote, mut a, mut sa, _) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let object = remote
        .files
        .keys()
        .find(|k| k.starts_with("objects/"))
        .unwrap()
        .clone();
    remote.files.insert(object, b"changed ciphertext".to_vec());
    let (mut b, mut sb) = (Device::default(), State::default());
    assert!(synchronize(&mut remote, &mut b, &mut sb, None, &|| true).is_err());
    assert!(b.entries.is_empty());
    remote.files.remove("index.mfsync");
    assert!(synchronize(&mut remote, &mut a, &mut sa, None, &|| true).is_err());
    assert_eq!(a.entries.len(), 1);
}
#[test]
fn a_concurrent_local_editor_keeps_its_newer_draft_and_the_old_baseline() {
    let (mut remote, mut a, mut sa, key) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let (mut b, mut sb) = (Device::default(), State::default());
    sync(&mut remote, &mut b, &mut sb);
    a.entries.insert(key.clone(), b"remote draft".to_vec());
    sync(&mut remote, &mut a, &mut sa);
    b.edit_before_apply = true;
    let previous = sb.baseline[&key].local.clone();
    assert!(synchronize(&mut remote, &mut b, &mut sb, None, &|| true).is_err());
    assert_eq!(b.entries[&key], b"newer local editor draft");
    assert_eq!(sb.baseline[&key].local, previous);
    assert_eq!(sync(&mut remote, &mut b, &mut sb).conflicts, 1);
    assert!(
        b.conflicts
            .values()
            .any(|v| v == b"newer local editor draft")
    );
}
#[test]
fn pause_during_planning_leaves_local_content_and_baselines_intact() {
    let (mut remote, mut a, mut sa, _) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let (mut b, mut sb) = (Device::default(), State::default());
    let calls = std::cell::Cell::new(0);
    assert!(
        synchronize(&mut remote, &mut b, &mut sb, None, &|| {
            let n = calls.get() + 1;
            calls.set(n);
            n < 5
        })
        .is_err()
    );
    assert!(b.entries.is_empty());
    assert!(sb.baseline.is_empty());
}
#[test]
fn encrypted_replicas_hide_text_and_authenticate_phrase_dataset_and_object_identity() {
    let phrase = "synthetic recovery phrase with more than twenty four characters";
    let mut remote = Remote::default();
    let header = initialize(&mut remote, Some(phrase)).unwrap();
    let key = uuid::Uuid::new_v4().to_string();
    let mut a = Device::default();
    a.entries.insert(
        key.clone(),
        b"private synthetic title and note body".to_vec(),
    );
    let mut sa = State::default();
    synchronize(&mut remote, &mut a, &mut sa, Some(phrase), &|| true).unwrap();
    assert!(
        !remote
            .files
            .values()
            .any(|v| v.windows(9).any(|s| s == b"synthetic"))
    );
    let (mut b, mut sb) = (Device::default(), State::default());
    synchronize(&mut remote, &mut b, &mut sb, Some(phrase), &|| true).unwrap();
    assert_eq!(a.entries, b.entries);
    assert!(
        header
            .unlock(Some("a different synthetic recovery phrase"))
            .is_err()
    );
    assert!(header.unlock(None).is_err());
    let codec = header.unlock(Some(phrase)).unwrap();
    let data = codec.seal("document/a", b"secret").unwrap();
    assert!(codec.open("document/b", &data).is_err());
    let mut other = header.clone();
    other.dataset = uuid::Uuid::new_v4().to_string();
    assert!(other.unlock(Some(phrase)).is_err());
}

#[test]
fn provider_conflict_recovery_unions_new_records_and_preserves_the_loser() {
    let (mut remote, mut a, mut sa, key) = fixture();
    sync(&mut remote, &mut a, &mut sa);
    let mut branch = Remote {
        files: remote.files.clone(),
        ..Default::default()
    };
    let mut b = Device {
        entries: a.entries.clone(),
        ..Default::default()
    };
    let mut sb: State = serde_json::from_slice(&serde_json::to_vec(&sa).unwrap()).unwrap();
    b.entries
        .insert(key.clone(), b"provider conflict draft".to_vec());
    let extra = uuid::Uuid::new_v4().to_string();
    b.entries
        .insert(extra.clone(), b"branch-only note".to_vec());
    sync(&mut branch, &mut b, &mut sb);
    a.entries.insert(key.clone(), b"canonical winner".to_vec());
    sync(&mut remote, &mut a, &mut sa);
    for (name, bytes) in &branch.files {
        if name.starts_with("objects/") {
            remote.files.insert(name.clone(), bytes.clone());
        }
    }
    let copy = branch.files["index.mfsync"].clone();
    let report = recover_manifests(&mut remote, &mut a, &mut sa, None, &[copy], &|| true).unwrap();
    assert_eq!(report.conflicts, 1);
    assert_eq!(a.entries[&key], b"canonical winner");
    assert!(!a.entries.contains_key(&extra));
    assert!(
        a.conflicts
            .values()
            .any(|b| b == b"provider conflict draft")
    );
    sync(&mut remote, &mut a, &mut sa);
    assert_eq!(a.entries[&extra], b"branch-only note");
}
