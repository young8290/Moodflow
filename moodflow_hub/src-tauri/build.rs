fn main() {
    // .cargo/config.toml supplies Rust's full static CRT linkage. The Tauri
    // staticVCRuntime workaround is disabled in tauri.conf.json because it
    // explicitly switches libucrt back to the dynamic ucrt import library.
    tauri_build::build()
}
