//! Inspect process identities only, never screen pixels. The core applies its shared privacy rules.
pub fn awake(guard: &mut moodflow_capsule::capture::SleepGuard) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::RemoteDesktop::*;
        let active_session = unsafe {
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
        };
        if !active_session {
            return false;
        }
        let mut active = 0;
        if unsafe {
            windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime(&mut active)
        } == 0
        {
            return false;
        }
        let uptime = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        guard.observe(uptime, active)
    }
    #[cfg(not(windows))]
    {
        let _ = guard;
        false
    }
}
#[cfg(windows)]
pub fn names(request: &moodflow_capsule::capture::Request) -> anyhow::Result<Vec<String>> {
    use anyhow::ensure;
    use moodflow_capsule::capture::Target;
    use std::collections::{BTreeMap, BTreeSet};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HWND, RECT},
        Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowDisplayAffinity, GetWindowRect, GetWindowThreadProcessId,
            IsIconic, IsWindowVisible,
        },
    };
    fn process(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut path = [0_u16; 4096];
            let mut length = path.len() as u32;
            let success = QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            if success == 0 {
                return None;
            }
            String::from_utf16(&path[..length as usize])
                .ok()?
                .rsplit(['\\', '/'])
                .next()
                .filter(|s| !s.is_empty())
                .map(String::from)
        }
    }
    match request.target.as_ref() {
        Some(Target::Window { hwnd, pid }) => {
            let window = usize::try_from(*hwnd)? as HWND;
            let mut actual = 0;
            unsafe {
                GetWindowThreadProcessId(window, &mut actual);
            }
            ensure!(actual == *pid && actual != 0, "capture.privacy");
            Ok(vec![
                process(*pid).ok_or_else(|| anyhow::anyhow!("capture.privacy"))?,
            ])
        }
        Some(Target::Monitor { index }) => {
            unsafe extern "system" fn monitor(
                handle: HMONITOR,
                _dc: HDC,
                _rect: *mut RECT,
                data: isize,
            ) -> i32 {
                let monitors = unsafe { &mut *(data as *mut Vec<RECT>) };
                let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                if unsafe { GetMonitorInfoW(handle, &mut info) } != 0 {
                    monitors.push(info.rcMonitor);
                }
                1
            }
            let mut monitors = Vec::<RECT>::new();
            ensure!(
                unsafe {
                    EnumDisplayMonitors(
                        std::ptr::null_mut(),
                        std::ptr::null(),
                        Some(monitor),
                        (&mut monitors as *mut Vec<RECT>) as isize,
                    )
                } != 0,
                "capture.privacy"
            );
            let mut bounds = *monitors
                .get(
                    index
                        .checked_sub(1)
                        .ok_or_else(|| anyhow::anyhow!("capture.privacy"))?,
                )
                .ok_or_else(|| anyhow::anyhow!("capture.privacy"))?;
            if let Some(crop) = &request.crop {
                crop.validate(
                    u32::try_from(bounds.right - bounds.left)?,
                    u32::try_from(bounds.bottom - bounds.top)?,
                )?;
                bounds.left += i32::try_from(crop.x)?;
                bounds.top += i32::try_from(crop.y)?;
                bounds.right = bounds.left + i32::try_from(crop.width)?;
                bounds.bottom = bounds.top + i32::try_from(crop.height)?;
            }
            struct Scan {
                bounds: RECT,
                pids: BTreeSet<u32>,
                valid: bool,
            }
            unsafe extern "system" fn window(handle: HWND, data: isize) -> i32 {
                let scan = unsafe { &mut *(data as *mut Scan) };
                if unsafe { IsWindowVisible(handle) } == 0 || unsafe { IsIconic(handle) } != 0 {
                    return 1;
                }
                let mut affinity = 0;
                if unsafe { GetWindowDisplayAffinity(handle, &mut affinity) } != 0 && affinity != 0
                {
                    return 1;
                }
                let mut rect: RECT = unsafe { std::mem::zeroed() };
                if unsafe { GetWindowRect(handle, &mut rect) } == 0 {
                    return 1;
                }
                if rect.right <= scan.bounds.left
                    || rect.left >= scan.bounds.right
                    || rect.bottom <= scan.bounds.top
                    || rect.top >= scan.bounds.bottom
                {
                    return 1;
                }
                let mut pid = 0;
                unsafe {
                    GetWindowThreadProcessId(handle, &mut pid);
                }
                if pid != 0 {
                    scan.pids.insert(pid);
                }
                if scan.pids.len() > 1024 {
                    scan.valid = false;
                    return 0;
                }
                1
            }
            let mut scan = Scan {
                bounds,
                pids: BTreeSet::new(),
                valid: true,
            };
            ensure!(
                unsafe { EnumWindows(Some(window), (&mut scan as *mut Scan) as isize) } != 0
                    && scan.valid,
                "capture.privacy"
            );
            let mut names = BTreeMap::new();
            for pid in scan.pids {
                names.insert(
                    process(pid).ok_or_else(|| anyhow::anyhow!("capture.privacy"))?,
                    (),
                );
            }
            Ok(names.into_keys().collect())
        }
        None => Ok(Vec::new()),
    }
}
#[cfg(not(windows))]
pub fn names(_request: &moodflow_capsule::capture::Request) -> anyhow::Result<Vec<String>> {
    anyhow::bail!("capture.unsupported")
}
