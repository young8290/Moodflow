//! Isolated Windows screen/image worker. No logging of content or paths.
#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(windows)]
    let result = windows::run();
    #[cfg(not(windows))]
    let result: anyhow::Result<serde_json::Value> = Err(anyhow::anyhow!("capture.unsupported"));
    let reply = match result {
        Ok(value) => serde_json::json!({"ok":true,"result":value}),
        Err(error) => {
            let code = error
                .chain()
                .find_map(|cause| {
                    let text = cause.to_string();
                    [
                        "capture.cancelled",
                        "capture.privacy",
                        "capture.ocr_missing",
                        "capture.no_overlap",
                        "capture.pixel_limit",
                        "capture.audio_unavailable",
                    ]
                    .into_iter()
                    .find(|code| *code == text)
                })
                .unwrap_or("capture.failed");
            serde_json::json!({"ok":false,"error":code})
        }
    };
    println!("{reply}");
}
