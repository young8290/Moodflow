use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::io::Write;

const HOST: &str = "0.130.0";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn sign(bytes: &[u8]) -> String {
    hex(&key().sign(bytes).to_bytes())
}

fn pubkey() -> [u8; 32] {
    key().verifying_key().to_bytes()
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (n, b) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

fn wasm_manifest(version: &str, extra_caps: &str) -> String {
    format!(
        r#"
id = "dev.example.emoji"
name = "表情"
version = "{version}"
sdk = 1
min_host_version = "0.120.0"
type = "wasm"
entry = "plugin.wasm"
platforms = ["windows", "android"]
license = "MIT"
[contributes]
candidate_provider = {{}}
[capabilities]
storage = true
{extra_caps}
"#
    )
}

fn release_for(bytes: &[u8], version: &str) -> Release {
    Release {
        version: version.into(),
        url: format!("https://example.com/emoji-{version}.mfpack"),
        sha256: hex(&Sha256::digest(bytes)),
        size: bytes.len() as u64,
        min_host_version: "0.120.0".into(),
        platforms: vec![Platform::Windows, Platform::Android],
    }
}

fn index() -> Index {
    let r = |v: &str, min: &str, plats: Vec<Platform>| Release {
        version: v.into(),
        url: String::new(),
        sha256: String::new(),
        size: 0,
        min_host_version: min.into(),
        platforms: plats,
    };
    Index {
        version: 1,
        generated_at: "2026-10-17".into(),
        plugins: vec![Listing {
            id: "dev.example.emoji".into(),
            name: "表情".into(),
            author: "a".into(),
            repo: "https://github.com/example/emoji".into(),
            description: String::new(),
            releases: vec![
                r(
                    "1.0.0",
                    "0.100.0",
                    vec![Platform::Windows, Platform::Android],
                ),
                r(
                    "1.1.0",
                    "0.120.0",
                    vec![Platform::Windows, Platform::Android],
                ),
                r("1.2.0", "0.120.0", vec![Platform::Windows]),
                r(
                    "2.0.0",
                    "0.200.0",
                    vec![Platform::Windows, Platform::Android],
                ),
            ],
        }],
        revoked: vec![],
    }
}

#[test]
fn index_is_parsed_only_after_signature_checks_out() {
    let json = serde_json::to_vec(&index()).unwrap();
    let sig = sign(&json);
    assert_eq!(verify_index(&json, &sig, &pubkey()).unwrap(), index());

    let mut tampered = json.clone();
    let pos = tampered.iter().position(|&b| b == b'1').unwrap();
    tampered[pos] = b'9';
    assert_eq!(
        verify_index(&tampered, &sig, &pubkey()),
        Err(MarketError::BadSignature)
    );
    let other = SigningKey::from_bytes(&[8u8; 32])
        .verifying_key()
        .to_bytes();
    assert_eq!(
        verify_index(&json, &sig, &other),
        Err(MarketError::BadSignature)
    );
    assert_eq!(
        verify_index(&json, "zz", &pubkey()),
        Err(MarketError::BadSignature)
    );
}

#[test]
fn latest_compatible_respects_host_platform_and_revocation() {
    let mut idx = index();
    let win = idx.latest_compatible("dev.example.emoji", HOST, Platform::Windows);
    assert_eq!(win.unwrap().version, "1.2.0", "2.0.0 要求更新的宿主");
    let android = idx.latest_compatible("dev.example.emoji", HOST, Platform::Android);
    assert_eq!(android.unwrap().version, "1.1.0", "1.2.0 不支持 Android");
    idx.revoked.push(Revoked {
        id: "dev.example.emoji".into(),
        version: Some("1.2.0".into()),
        reason: "崩溃".into(),
    });
    let win = idx.latest_compatible("dev.example.emoji", HOST, Platform::Windows);
    assert_eq!(win.unwrap().version, "1.1.0", "跳过撤回版本");
}

#[test]
fn updates_and_revoked_installed() {
    let mut idx = index();
    let installed = vec![("dev.example.emoji".to_string(), "1.0.0".to_string())];
    assert_eq!(
        idx.updates(&installed, HOST, Platform::Windows),
        vec![Update {
            id: "dev.example.emoji".into(),
            from: "1.0.0".into(),
            to: "1.2.0".into()
        }]
    );
    assert!(idx.revoked_installed(&installed).is_empty());
    idx.revoked.push(Revoked {
        id: "dev.example.emoji".into(),
        version: None,
        reason: "恶意".into(),
    });
    assert_eq!(idx.revoked_installed(&installed), installed, "整个插件撤回");
    assert!(idx.updates(&installed, HOST, Platform::Windows).is_empty());
}

#[test]
fn package_must_match_signed_hash_and_identity() {
    let pkg = zip_of(&[
        ("manifest.toml", wasm_manifest("1.1.0", "").as_bytes()),
        ("plugin.wasm", b"\0asm"),
    ]);
    let rel = release_for(&pkg, "1.1.0");
    let plan = inspect_package(&pkg, "dev.example.emoji", &rel, HOST, None).unwrap();
    assert_eq!(plan.manifest.version, "1.1.0");
    assert_eq!(
        plan.added_capabilities,
        vec![Capability::Storage],
        "首次安装列出全部能力"
    );

    let mut bad = pkg.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert_eq!(
        inspect_package(&bad, "dev.example.emoji", &rel, HOST, None),
        Err(MarketError::HashMismatch)
    );
    assert_eq!(
        inspect_package(&pkg, "dev.other.plugin", &rel, HOST, None),
        Err(MarketError::IdentityMismatch)
    );
    let wrong_version = release_for(&pkg, "1.2.0");
    assert_eq!(
        inspect_package(&pkg, "dev.example.emoji", &wrong_version, HOST, None),
        Err(MarketError::IdentityMismatch)
    );
}

#[test]
fn package_needs_manifest_and_entry() {
    let no_manifest = zip_of(&[("plugin.wasm", b"\0asm")]);
    assert_eq!(
        inspect_package(
            &no_manifest,
            "dev.example.emoji",
            &release_for(&no_manifest, "1.1.0"),
            HOST,
            None
        ),
        Err(MarketError::MissingManifest)
    );
    let no_entry = zip_of(&[("manifest.toml", wasm_manifest("1.1.0", "").as_bytes())]);
    assert_eq!(
        inspect_package(
            &no_entry,
            "dev.example.emoji",
            &release_for(&no_entry, "1.1.0"),
            HOST,
            None
        ),
        Err(MarketError::MissingEntry("plugin.wasm".into()))
    );
}

#[test]
fn content_pack_cannot_ship_protected_files() {
    let manifest = r#"
id = "dev.example.words"
name = "词表"
version = "1.0.0"
sdk = 1
min_host_version = "0.1.0"
type = "content"
content = "hub_templates"
platforms = ["windows"]
license = "MIT"
"#;
    let pkg = zip_of(&[
        ("manifest.toml", manifest.as_bytes()),
        ("templates/banned_words.toml", b"version = 1"),
    ]);
    assert_eq!(
        inspect_package(
            &pkg,
            "dev.example.words",
            &release_for(&pkg, "1.0.0"),
            HOST,
            None
        ),
        Err(MarketError::ProtectedFile(
            "templates/banned_words.toml".into()
        ))
    );
}

#[test]
fn unsafe_zip_paths_are_rejected() {
    let pkg = zip_of(&[
        ("manifest.toml", wasm_manifest("1.1.0", "").as_bytes()),
        ("../evil.wasm", b"x"),
    ]);
    assert!(matches!(
        inspect_package(
            &pkg,
            "dev.example.emoji",
            &release_for(&pkg, "1.1.0"),
            HOST,
            None
        ),
        Err(MarketError::UnsafePath(_))
    ));
}

#[test]
fn upgrade_reports_only_newly_requested_capabilities() {
    let old_pkg = zip_of(&[
        ("manifest.toml", wasm_manifest("1.0.0", "").as_bytes()),
        ("plugin.wasm", b"\0asm"),
    ]);
    let old = inspect_package(
        &old_pkg,
        "dev.example.emoji",
        &release_for(&old_pkg, "1.0.0"),
        HOST,
        None,
    )
    .unwrap()
    .manifest;
    let new_pkg = zip_of(&[
        (
            "manifest.toml",
            wasm_manifest("1.1.0", "notify = true").as_bytes(),
        ),
        ("plugin.wasm", b"\0asm"),
    ]);
    let plan = inspect_package(
        &new_pkg,
        "dev.example.emoji",
        &release_for(&new_pkg, "1.1.0"),
        HOST,
        Some(&old),
    )
    .unwrap();
    assert_eq!(plan.added_capabilities, vec![Capability::Notify]);
}

#[test]
fn install_lists_and_uninstalls_atomically() {
    let dir = std::env::temp_dir().join(format!("mf-market-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for v in ["1.0.0", "1.1.0"] {
        let pkg = zip_of(&[
            ("manifest.toml", wasm_manifest(v, "").as_bytes()),
            ("plugin.wasm", b"\0asm"),
            ("assets/data.json", b"{}"),
        ]);
        let plan =
            inspect_package(&pkg, "dev.example.emoji", &release_for(&pkg, v), HOST, None).unwrap();
        let path = install(&plan, &dir).unwrap();
        assert!(path.join("assets/data.json").exists());
    }
    // 重装同版本：替换而不是报错
    let pkg = zip_of(&[
        ("manifest.toml", wasm_manifest("1.1.0", "").as_bytes()),
        ("plugin.wasm", b"\0asm2"),
    ]);
    let plan = inspect_package(
        &pkg,
        "dev.example.emoji",
        &release_for(&pkg, "1.1.0"),
        HOST,
        None,
    )
    .unwrap();
    let path = install(&plan, &dir).unwrap();
    assert_eq!(std::fs::read(path.join("plugin.wasm")).unwrap(), b"\0asm2");
    assert!(!path.join("assets/data.json").exists(), "旧版本内容不残留");
    assert!(
        dir.join("dev.example.emoji/1.0.0").exists(),
        "旧版本目录保留可回滚"
    );

    assert_eq!(
        installed(&dir),
        vec![("dev.example.emoji".to_string(), "1.1.0".to_string())]
    );
    assert!(uninstall(&dir, "../x").is_err());
    uninstall(&dir, "dev.example.emoji").unwrap();
    assert!(installed(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
