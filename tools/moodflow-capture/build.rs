fn main() {
    println!("cargo:rerun-if-changed=../../docs/VERSION");
    println!("cargo:rerun-if-env-changed=MOODFLOW_APP_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("MOODFLOW_APP_VERSION")
        .ok()
        .or_else(|| std::fs::read_to_string("../../docs/VERSION").ok())
        .unwrap_or_else(|| "0.0.0".into());
    let version = version.trim();
    let numbers = version
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect::<Vec<_>>();
    let numeric = (numbers.first().copied().unwrap_or(0) << 48)
        | (numbers.get(1).copied().unwrap_or(0) << 32)
        | (numbers.get(2).copied().unwrap_or(0) << 16);
    tauri_winres::WindowsResource::new()
        .set_icon("../../moodflow_hub/src-tauri/icons/icon.ico")
        .set("ProductName", "Moodflow")
        .set("FileDescription", "Moodflow 捕获工具")
        .set("CompanyName", "Moodflow")
        .set("OriginalFilename", "moodflow-capture.exe")
        .set("FileVersion", version)
        .set("ProductVersion", version)
        .set_version_info(tauri_winres::VersionInfo::FILEVERSION, numeric)
        .set_version_info(tauri_winres::VersionInfo::PRODUCTVERSION, numeric)
        .compile()
        .expect("compile Moodflow capture resources");
}
