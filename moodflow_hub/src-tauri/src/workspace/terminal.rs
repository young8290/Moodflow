//! ConPTY sessions belong to the native runtime, never to a webview or external gateway.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Terminal;
#[cfg(not(windows))]
pub struct Terminal;
#[cfg(not(windows))]
impl Terminal {
    pub fn spawn(_directory: &std::path::Path, _cols: u16, _rows: u16) -> anyhow::Result<Self> {
        anyhow::bail!("workspace.terminal_unsupported")
    }
    pub fn input(&self, _bytes: Vec<u8>) -> anyhow::Result<()> {
        anyhow::bail!("workspace.terminal_unsupported")
    }
    pub fn resize(&self, _cols: u16, _rows: u16) -> anyhow::Result<()> {
        anyhow::bail!("workspace.terminal_unsupported")
    }
    pub fn poll(&self, _after: u64) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("workspace.terminal_unsupported")
    }
}
