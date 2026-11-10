fn main() {
    println!("cargo:rerun-if-env-changed=MOODFLOW_APP_VERSION");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/VERSION");
    println!("cargo:rerun-if-changed={}", path.display());
    let version = std::env::var("MOODFLOW_APP_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    let version = version.trim();
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)),
        "invalid product version"
    );
    println!("cargo:rustc-env=MOODFLOW_HUB_VERSION={version}");
}
