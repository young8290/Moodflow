//! Clipboard publication of an explicitly selected image; never synthesizes paste.
#[cfg(windows)]
pub fn image(owner: usize, png: &[u8], dib: &[u8]) -> Result<(), ()> {
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HGLOBAL},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
                SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::CF_DIB,
        },
    };
    struct Memory(HGLOBAL);
    impl Drop for Memory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    GlobalFree(self.0);
                }
            }
        }
    }
    fn allocate(bytes: &[u8]) -> Result<Memory, ()> {
        unsafe {
            let memory = Memory(GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()));
            if memory.0.is_null() {
                return Err(());
            }
            let target = GlobalLock(memory.0);
            if target.is_null() {
                return Err(());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast(), bytes.len());
            GlobalUnlock(memory.0);
            Ok(memory)
        }
    }
    fn format(name: &str) -> Result<u32, ()> {
        let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
        if id == 0 { Err(()) } else { Ok(id) }
    }
    if owner == 0 || png.len() > 512 * 1024 * 1024 || dib.len() > 256 * 1024 * 1024 + 40 {
        return Err(());
    }
    let mut formats = Vec::new();
    for (name, value) in [
        ("ExcludeClipboardContentFromMonitorProcessing", 1_u32),
        ("CanIncludeInClipboardHistory", 0),
        ("CanUploadToCloudClipboard", 0),
    ] {
        formats.push((format(name)?, allocate(&value.to_le_bytes())?));
    }
    formats.push((format("PNG")?, allocate(png)?));
    formats.push((CF_DIB as u32, allocate(dib)?));
    unsafe {
        if OpenClipboard(owner as *mut std::ffi::c_void) == 0 {
            return Err(());
        }
        struct Open;
        impl Drop for Open {
            fn drop(&mut self) {
                unsafe {
                    CloseClipboard();
                }
            }
        }
        let _open = Open;
        if EmptyClipboard() == 0 {
            return Err(());
        }
        for (format, mut memory) in formats {
            if SetClipboardData(format, memory.0).is_null() {
                EmptyClipboard();
                return Err(());
            }
            memory.0 = std::ptr::null_mut();
        }
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn image(_owner: usize, _png: &[u8], _dib: &[u8]) -> Result<(), ()> {
    Err(())
}
