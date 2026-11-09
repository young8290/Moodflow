//! Windows vault policy: interactive desktop only, private clipboard and ownership-safe clearing.
#[cfg(windows)]
use std::time::Duration;
use std::time::Instant;

pub struct ClipboardLease {
    pub sequence: u32,
    pub expires: Instant,
}
pub enum Clear {
    Done,
    Changed,
    Busy,
}
pub fn supported() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
pub fn desktop_available() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, DESKTOP_READOBJECTS, GetUserObjectInformationW, OpenInputDesktop, UOI_NAME,
    };
    // SAFETY: query-only access; close the returned desktop on every path.
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return false;
        }
        let mut name = [0_u16; 128];
        let mut needed = 0;
        let ok = GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            (name.len() * 2) as u32,
            &mut needed,
        );
        CloseDesktop(desktop);
        if ok == 0 || needed as usize > name.len() * 2 {
            return false;
        }
        let length = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        String::from_utf16_lossy(&name[..length]).eq_ignore_ascii_case("default")
    }
}
#[cfg(not(windows))]
pub fn desktop_available() -> bool {
    false
}

#[cfg(windows)]
mod clipboard {
    use super::*;
    use std::{ptr, sync::Mutex};
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HGLOBAL, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
                OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{
                GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
            },
            Ole::CF_UNICODETEXT,
        },
    };
    use zeroize::{Zeroize, Zeroizing};
    static CLIPBOARD: Mutex<()> = Mutex::new(());
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    struct Allocation(HGLOBAL);
    impl Drop for Allocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let size = GlobalSize(self.0);
                    let data = GlobalLock(self.0);
                    if !data.is_null() {
                        std::slice::from_raw_parts_mut(data.cast::<u8>(), size).zeroize();
                        GlobalUnlock(self.0);
                    }
                    GlobalFree(self.0);
                }
            }
        }
    }
    fn set(format: u32, bytes: &[u8]) -> Result<(), ()> {
        // SAFETY: moveable allocation is exactly the input size; on success the OS owns it.
        unsafe {
            let mut memory = Allocation(GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()));
            if memory.0.is_null() {
                return Err(());
            }
            let target = GlobalLock(memory.0);
            if target.is_null() {
                return Err(());
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast(), bytes.len());
            GlobalUnlock(memory.0);
            if SetClipboardData(format, memory.0).is_null() {
                return Err(());
            }
            memory.0 = ptr::null_mut();
        }
        Ok(())
    }
    fn scrub_and_empty() -> bool {
        unsafe {
            let memory = GetClipboardData(CF_UNICODETEXT as u32);
            if !memory.is_null() {
                let length = GlobalSize(memory);
                if length <= 1024 * 1024 {
                    let data = GlobalLock(memory);
                    if !data.is_null() {
                        // Only called while our sequence is still current and the clipboard is open.
                        std::slice::from_raw_parts_mut(data.cast::<u8>(), length).zeroize();
                        GlobalUnlock(memory);
                    }
                }
            }
            EmptyClipboard() != 0
        }
    }
    pub fn copy(owner: usize, text: &str) -> Result<ClipboardLease, ()> {
        if owner == 0 || text.len() > 65536 || text.contains('\0') {
            return Err(());
        }
        let _lock = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        let wide = Zeroizing::new(text.encode_utf16().chain([0]).collect::<Vec<_>>());
        unsafe {
            if OpenClipboard(owner as HWND) == 0 {
                return Err(());
            }
        }
        let _open = Open;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(());
        }
        let publish = (|| {
            for (name, value) in [
                ("ExcludeClipboardContentFromMonitorProcessing", 1_u32),
                ("CanIncludeInClipboardHistory", 0),
                ("CanUploadToCloudClipboard", 0),
            ] {
                let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
                let id = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
                if id == 0 {
                    return Err(());
                }
                set(id, &value.to_le_bytes())?;
            }
            // Privacy markers are published before the secret text, under one open.
            let bytes =
                unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
            set(CF_UNICODETEXT as u32, bytes)
        })();
        if publish.is_err() {
            scrub_and_empty();
            return Err(());
        }
        // Capture under the OS lock: a copy arriving immediately after CloseClipboard
        // must never become the sequence that our expiry timer is allowed to clear.
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence == 0 {
            scrub_and_empty();
            return Err(());
        }
        drop(_open);
        Ok(ClipboardLease {
            sequence,
            expires: Instant::now() + Duration::from_secs(60),
        })
    }
    pub fn clear(lease: &ClipboardLease) -> Clear {
        let _lock = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        if unsafe { GetClipboardSequenceNumber() } != lease.sequence {
            return Clear::Changed;
        }
        if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
            return Clear::Busy;
        }
        let _open = Open;
        // Compare again under the OS clipboard lock; never clear a newer copy.
        if unsafe { GetClipboardSequenceNumber() } != lease.sequence {
            return Clear::Changed;
        }
        if scrub_and_empty() {
            Clear::Done
        } else {
            Clear::Busy
        }
    }
}
#[cfg(windows)]
pub use clipboard::{clear as clear_sensitive, copy as copy_sensitive};
#[cfg(not(windows))]
pub fn copy_sensitive(_owner: usize, _text: &str) -> Result<ClipboardLease, ()> {
    Err(())
}
#[cfg(not(windows))]
pub fn clear_sensitive(_lease: &ClipboardLease) -> Clear {
    Clear::Busy
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::{
        Win32::{
            Foundation::HWND,
            System::{
                DataExchange::{
                    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
                    RegisterClipboardFormatW,
                },
                Memory::{GlobalLock, GlobalUnlock},
                Ole::CF_UNICODETEXT,
            },
            UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
        },
        core::w,
    };
    struct Window(HWND);
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    fn read() -> String {
        unsafe {
            assert_ne!(OpenClipboard(std::ptr::null_mut()), 0);
            let handle = GetClipboardData(CF_UNICODETEXT as u32);
            assert!(!handle.is_null());
            let text = GlobalLock(handle).cast::<u16>();
            assert!(!text.is_null());
            let mut length = 0;
            while *text.add(length) != 0 && length < 4096 {
                length += 1;
            }
            let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            GlobalUnlock(handle);
            CloseClipboard();
            result
        }
    }
    #[test]
    #[ignore = "writes clipboard; requires an isolated Windows CI runner"]
    fn private_clipboard_markers_and_newer_copy_preservation() {
        assert_eq!(
            std::env::var("GITHUB_ACTIONS").as_deref(),
            Ok("true"),
            "run only on an isolated Actions runner; do not overwrite the user's clipboard"
        );
        let owner = Window(unsafe {
            CreateWindowExW(
                0,
                w!("STATIC"),
                w!("Moodflow private clipboard test"),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        });
        assert!(!owner.0.is_null());
        let original = copy_sensitive(owner.0 as usize, "fixture credential 🔐").unwrap();
        assert_eq!(read(), "fixture credential 🔐");
        unsafe {
            assert_ne!(OpenClipboard(std::ptr::null_mut()), 0);
            for (name, expected) in [
                ("ExcludeClipboardContentFromMonitorProcessing", 1_u32),
                ("CanIncludeInClipboardHistory", 0),
                ("CanUploadToCloudClipboard", 0),
            ] {
                let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
                let format = RegisterClipboardFormatW(wide.as_ptr());
                assert_ne!(IsClipboardFormatAvailable(format), 0);
                let handle = GetClipboardData(format);
                let value = GlobalLock(handle).cast::<u32>();
                assert!(!value.is_null());
                assert_eq!(*value, expected);
                GlobalUnlock(handle);
            }
            CloseClipboard();
        }
        let newer = copy_sensitive(owner.0 as usize, "newer fixture copy").unwrap();
        assert!(matches!(clear_sensitive(&original), Clear::Changed));
        assert_eq!(read(), "newer fixture copy");
        assert!(matches!(clear_sensitive(&newer), Clear::Done));
        assert_eq!(
            unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT as u32) },
            0
        );
    }
}
