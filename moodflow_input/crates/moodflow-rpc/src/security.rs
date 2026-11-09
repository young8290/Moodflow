//! Named Pipe 安全描述符工具（Windows）。
//!
//! Private settings/data control is for the current user at medium integrity or above.
//! TSF host pipes have their own broader ACL; never reuse that ACL for these data APIs.

#[cfg(windows)]
use tracing::error;

use std::io;
use windows::Win32::System::SystemServices::SECURITY_MANDATORY_MEDIUM_RID;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
    Security::*,
    System::Threading::*,
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct Identity {
    sid: String,
    app: bool,
    integrity: u32,
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "untrusted settings IPC peer",
    )
}
fn token_info(token: HANDLE, kind: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut size = 0;
    unsafe {
        let _ = GetTokenInformation(token, kind, None, 0, &mut size);
    }
    if size == 0 || size > 65536 {
        return Err(denied());
    }
    let mut words = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token,
            kind,
            Some(words.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(|_| denied())?;
    Ok(words)
}
fn identity(process: HANDLE) -> io::Result<Identity> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|_| denied())?;
        let token = Handle(token);
        let user = token_info(token.0, TokenUser)?;
        let user = &*user.as_ptr().cast::<TOKEN_USER>();
        let mut text = windows::core::PWSTR::null();
        Authorization::ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(|_| denied())?;
        let sid = text.to_string().map_err(|_| denied());
        let _ = LocalFree(HLOCAL(text.0.cast()));
        let sid = sid?;
        let app = token_info(token.0, TokenIsAppContainer)?[0] != 0;
        let label = token_info(token.0, TokenIntegrityLevel)?;
        let label = &*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let count = GetSidSubAuthorityCount(label.Label.Sid);
        if count.is_null() || *count == 0 {
            return Err(denied());
        }
        let rid = GetSidSubAuthority(label.Label.Sid, (*count - 1) as u32);
        if rid.is_null() {
            return Err(denied());
        }
        Ok(Identity {
            sid,
            app,
            integrity: *rid,
        })
    }
}
fn private_sddl() -> io::Result<String> {
    let owner = identity(unsafe { GetCurrentProcess() })?;
    if owner.app || owner.integrity < SECURITY_MANDATORY_MEDIUM_RID as u32 {
        return Err(denied());
    }
    Ok(format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{})S:(ML;;NWNR;;;ME)",
        owner.sid
    ))
}

/// Validate the server on the same opened handle before writing a request or accepting events.
/// Full-trust same-user impersonation is outside the OS boundary; other users and sandboxes fail.
pub fn verify_server(file: &std::fs::File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    unsafe {
        let mut pid = 0;
        windows::Win32::System::Pipes::GetNamedPipeServerProcessId(
            HANDLE(file.as_raw_handle()),
            &mut pid,
        )
        .map_err(|_| denied())?;
        let server = Handle(
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(|_| denied())?,
        );
        let peer = identity(server.0)?;
        let own = identity(GetCurrentProcess())?;
        if peer.sid != own.sid || peer.app || peer.integrity < SECURITY_MANDATORY_MEDIUM_RID as u32
        {
            return Err(denied());
        }
        Ok(())
    }
}

#[cfg(windows)]
const SDDL_REVISION_1: u32 = 1;

/// 解析后的安全描述符（RAII：bytes 持有副本，原始 LocalAlloc 内存已释放）。
#[cfg(windows)]
pub(crate) struct SecurityDescriptor {
    bytes: Vec<u8>,
}

#[cfg(windows)]
impl SecurityDescriptor {
    pub(crate) fn from_sddl(sddl: &str) -> Option<Self> {
        use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorA;

        let sddl_c = std::ffi::CString::new(sddl).ok()?;
        let mut psd = windows::Win32::Security::PSECURITY_DESCRIPTOR(std::ptr::null_mut());
        let mut sd_len: u32 = 0;

        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorA(
                windows::core::PCSTR(sddl_c.as_ptr() as *const u8),
                SDDL_REVISION_1,
                &mut psd,
                Some(&mut sd_len),
            )
        };
        if ok.is_err() || psd.0.is_null() {
            error!("Failed to parse SDDL security descriptor");
            return None;
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(psd.0 as *const u8, sd_len as usize) }.to_vec();
        unsafe {
            windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(psd.0));
        }
        Some(Self { bytes })
    }

    pub(crate) fn as_ptr(&self) -> *const std::ffi::c_void {
        self.bytes.as_ptr() as *const std::ffi::c_void
    }
}

/// 创建管道安全属性（保持存活直到 CreateNamedPipe 调用完成）。
#[cfg(windows)]
pub(crate) fn create_pipe_security_attributes() -> Option<SecurityDescriptor> {
    SecurityDescriptor::from_sddl(&private_sddl().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_private_pipe_allows_owner_and_rejects_low_integrity() {
        use std::fs::OpenOptions;
        use windows::Win32::{Storage::FileSystem::PIPE_ACCESS_DUPLEX, System::Pipes::*};
        let endpoint = format!(r"\\.\pipe\moodflow-rpc-acl-test-{}", std::process::id());
        let name = endpoint.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let descriptor = create_pipe_security_attributes().unwrap();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.as_ptr() as *mut _,
            bInheritHandle: false.into(),
        };
        let _pipe = Handle(unsafe {
            CreateNamedPipeW(
                windows::core::PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                Some(&attributes),
            )
        });
        assert!(!_pipe.0.is_invalid(), "create synthetic private pipe");

        // Impersonate only this test thread, and always revert before assertions/panic.
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                unsafe {
                    RevertToSelf().expect("restore test thread token");
                }
            }
        }
        let low_result = unsafe {
            let mut original = HANDLE::default();
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY,
                &mut original,
            )
            .unwrap();
            let original = Handle(original);
            let mut low = HANDLE::default();
            DuplicateTokenEx(
                original.0,
                TOKEN_QUERY | TOKEN_ADJUST_DEFAULT | TOKEN_IMPERSONATE,
                None,
                SecurityImpersonation,
                TokenImpersonation,
                &mut low,
            )
            .unwrap();
            let low = Handle(low);
            let mut sid = PSID::default();
            Authorization::ConvertStringSidToSidW(windows::core::w!("S-1-16-4096"), &mut sid)
                .unwrap();
            let label = TOKEN_MANDATORY_LABEL {
                Label: SID_AND_ATTRIBUTES {
                    Sid: sid,
                    Attributes: windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY as u32,
                },
            };
            let result = SetTokenInformation(
                low.0,
                TokenIntegrityLevel,
                (&label as *const TOKEN_MANDATORY_LABEL).cast(),
                std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
            );
            let _ = LocalFree(HLOCAL(sid.0));
            result.unwrap();
            SetThreadToken(None, low.0).unwrap();
            let _revert = Revert;
            OpenOptions::new().read(true).write(true).open(&endpoint)
        };
        assert_eq!(
            low_result.unwrap_err().raw_os_error(),
            Some(5),
            "MIC must reject low integrity"
        );
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&endpoint)
            .unwrap();
        verify_server(&owner).unwrap();
    }
    #[test]
    fn real_owner_descriptor_has_no_world_sandbox_or_low_integrity_grant() {
        let text = private_sddl().unwrap();
        assert!(SecurityDescriptor::from_sddl(&text).is_some());
        for forbidden in [";;;WD)", ";;;AC)", "S-1-15-"] {
            assert!(!text.contains(forbidden));
        }
        assert!(text.contains("S:(ML;;NWNR;;;ME)"));
        let id = identity(unsafe { GetCurrentProcess() }).unwrap();
        assert!(text.contains(&format!(";;;{})", id.sid)));
        assert!(!id.app);
        assert!(id.integrity >= SECURITY_MANDATORY_MEDIUM_RID as u32);
    }
}
