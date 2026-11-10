//! Fail closed for an unavailable / secure desktop; reject known password input.
pub fn allowed() -> bool {
    use std::cell::{Cell, RefCell};
    thread_local! { static DENIED:Cell<bool>=const{Cell::new(false)}; static SLEEP:RefCell<moodflow_capsule::capture::SleepGuard>=RefCell::new(Default::default()); }
    if DENIED.with(Cell::get) {
        return false;
    }
    let mut active = 0;
    let sampled = unsafe {
        windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime(&mut active)
    } != 0;
    let uptime = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let awake = sampled && SLEEP.with(|guard| guard.borrow_mut().observe(uptime, active));
    let allowed = awake && available_now();
    if !allowed {
        DENIED.with(|denied| denied.set(true));
    }
    allowed
}
fn available_now() -> bool {
    if !active_session() {
        return false;
    }
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, DESKTOP_READOBJECTS, GetUserObjectInformationW, OpenInputDesktop, UOI_NAME,
    };
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return false;
        }
        let mut name = [0_u16; 128];
        let mut needed = 0;
        let success = GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            (name.len() * 2) as u32,
            &mut needed,
        );
        CloseDesktop(desktop);
        if success == 0 || needed as usize > name.len() * 2 {
            return false;
        }
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        if !String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("default") {
            return false;
        }
    }
    !password_input()
}
fn active_session() -> bool {
    use windows_sys::Win32::System::RemoteDesktop::*;
    unsafe {
        let mut buffer = std::ptr::null_mut();
        let mut bytes = 0;
        let success = WTSQuerySessionInformationW(
            std::ptr::null_mut(),
            WTS_CURRENT_SESSION,
            WTSConnectState,
            &mut buffer,
            &mut bytes,
        );
        let active = success != 0
            && !buffer.is_null()
            && bytes >= 4
            && std::ptr::read_unaligned(buffer.cast::<i32>()) == WTSActive;
        if !buffer.is_null() {
            WTSFreeMemory(buffer.cast());
        }
        active
    }
}
fn password_input() -> bool {
    use windows::Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::Accessibility::{CUIAutomation, IUIAutomation},
    };
    thread_local! {
        static AUTOMATION: Option<IUIAutomation> = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok() };
    }
    AUTOMATION.with(|automation| {
        automation
            .as_ref()
            .and_then(|a| unsafe { a.GetFocusedElement().ok() })
            .and_then(|element| unsafe { element.CurrentIsPassword().ok() })
            .is_some_and(|value| value.as_bool())
    })
}
