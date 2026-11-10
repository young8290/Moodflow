//! Private application directories. Fail closed if privacy cannot be established.
use std::path::Path;

pub fn ensure(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "private directory is not a real directory"
    );
    protect(path)
}

#[cfg(windows)]
fn protect(path: &Path) -> anyhow::Result<()> {
    use std::{
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        Security::{
            DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
            SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    anyhow::ensure!(
        std::fs::symlink_metadata(path)?.file_attributes() & 0x400 == 0,
        "private directory is a reparse point"
    );
    let mut token = ptr::null_mut();
    // SAFETY: valid process pseudo-handle and writable token output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| {
        let mut size = 0;
        // SAFETY: first call only queries the required buffer size.
        unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size) };
        anyhow::ensure!(size > 0 && size < 65536, "invalid token size");
        let mut storage = vec![0_usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: storage is aligned and at least size bytes, and token remains open.
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                storage.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut sid_text = ptr::null_mut();
        // SAFETY: successful token query produced a valid TOKEN_USER in aligned storage.
        let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut len = 0;
        // SAFETY: OS owns a NUL-terminated SID string until LocalFree.
        while unsafe { *sid_text.add(len) } != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_text, len) });
        unsafe { LocalFree(sid_text.cast()) };
        let sddl = format!("D:P(A;OICI;FA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut descriptor = ptr::null_mut();
        // SAFETY: sddl is valid and NUL-terminated; descriptor is an output pointer.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let name = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let ok = unsafe {
            SetFileSecurityW(
                name.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
        };
        let error = std::io::Error::last_os_error();
        unsafe { LocalFree(descriptor) };
        if ok == 0 {
            return Err(error.into());
        }
        Ok(())
    })();
    // SAFETY: token is owned by this function and no longer used.
    unsafe { CloseHandle(token) };
    result
}

#[cfg(unix)]
fn protect(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn protect(_: &Path) -> anyhow::Result<()> {
    anyhow::bail!("private storage unsupported")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn directory_has_a_protected_single_user_inheritable_acl() {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            Security::{
                ACL_SIZE_INFORMATION, AclSizeInformation, DACL_SECURITY_INFORMATION,
                GetAclInformation, GetSecurityDescriptorControl, SE_DACL_PROTECTED,
            },
        };
        let temp = tempfile::tempdir().unwrap();
        let private = temp.path().join("private");
        ensure(&private).unwrap();
        std::fs::write(private.join("probe"), "private fixture").unwrap();
        let name = private
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut descriptor = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        // SAFETY: valid NUL-terminated path and output pointers; descriptor is released below.
        let result = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(result, 0);
        assert!(!dacl.is_null());
        let mut info: ACL_SIZE_INFORMATION = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                GetAclInformation(
                    dacl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
            },
            0
        );
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        assert_ne!(control & SE_DACL_PROTECTED, 0);
        assert_eq!(info.AceCount, 1, "no inherited group or Everyone grants");
        unsafe { LocalFree(descriptor) };
    }
}
