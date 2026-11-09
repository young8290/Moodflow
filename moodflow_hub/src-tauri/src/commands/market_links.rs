//! Fixed community destinations; never interpret a frontend string as a shell command.
use anyhow::{Result, bail};
pub fn open(key: &str) -> Result<()> {
    let url = match key {
        "issues" => "https://github.com/young8290/Moodflow/issues",
        "submit" => "https://github.com/young8290/Moodflow/issues/new?title=Plugin%20submission",
        "format" => "https://github.com/young8290/Moodflow/blob/main/docs/design/plugin-system.md",
        _ => bail!("market.invalid"),
    };
    let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if result as usize <= 32 {
        bail!("market.failed");
    }
    Ok(())
}
