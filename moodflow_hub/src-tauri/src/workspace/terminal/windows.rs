use anyhow::{Result, ensure};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex, mpsc},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE},
    Storage::FileSystem::{ReadFile, WriteFile},
    System::{
        Console::*, JobObjects::*, Pipes::CreatePipe, SystemInformation::GetSystemDirectoryW,
        Threading::*,
    },
};
struct Handle(HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
struct Process(Handle);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            TerminateProcess(self.0.0, 1);
        }
    }
}
struct Console(HPCON);
impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            ClosePseudoConsole(self.0);
        }
    }
}
struct Attributes {
    words: Vec<usize>,
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.words.as_mut_ptr().cast());
        }
    }
}
#[derive(Default)]
struct Output {
    chunks: VecDeque<(u64, Vec<u8>)>,
    next: u64,
    bytes: usize,
    eof: bool,
}
pub struct Terminal {
    process: Process,
    job: Handle,
    console: Mutex<Console>,
    input: mpsc::SyncSender<Vec<u8>>,
    output: Arc<Mutex<Output>>,
}
fn dimensions(cols: u16, rows: u16) -> Result<COORD> {
    ensure!(
        (20..=240).contains(&cols) && (8..=100).contains(&rows),
        "workspace.invalid"
    );
    Ok(COORD {
        X: cols as i16,
        Y: rows as i16,
    })
}
fn wide(path: &std::ffi::OsStr) -> Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let mut value = path.encode_wide().collect::<Vec<_>>();
    ensure!(
        !value.contains(&0) && value.len() < 32767,
        "workspace.invalid"
    );
    value.push(0);
    Ok(value)
}
fn pipe() -> Result<(Handle, Handle)> {
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    ensure!(
        unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 65536) } != 0,
        "workspace.terminal_failed"
    );
    Ok((Handle(read), Handle(write)))
}
impl Terminal {
    pub fn spawn(directory: &Path, cols: u16, rows: u16) -> Result<Self> {
        Self::spawn_inner(directory, cols, rows, false)
    }
    fn spawn_inner(directory: &Path, cols: u16, rows: u16, fixture: bool) -> Result<Self> {
        let size = dimensions(cols, rows)?;
        let directory = directory.canonicalize()?;
        ensure!(directory.is_dir(), "workspace.invalid");
        let directory = wide(directory.as_os_str())?;
        let mut system = [0_u16; 32768];
        let length =
            unsafe { GetSystemDirectoryW(system.as_mut_ptr(), system.len() as u32) } as usize;
        ensure!(
            length > 0 && length < system.len(),
            "workspace.terminal_failed"
        );
        let executable =
            std::path::PathBuf::from(String::from_utf16(&system[..length])?).join(if fixture {
                "cmd.exe"
            } else {
                "WindowsPowerShell/v1.0/powershell.exe"
            });
        let application = wide(executable.as_os_str())?;
        let flags = if fixture {
            "/d /q"
        } else {
            "-NoLogo -NoProfile"
        };
        let mut command = format!("\"{}\" {flags}", executable.to_string_lossy())
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;
        let mut hpc = 0;
        ensure!(
            unsafe { CreatePseudoConsole(size, input_read.0, output_write.0, 0, &mut hpc) } >= 0,
            "workspace.terminal_failed"
        );
        let console = Console(hpc);
        let mut required = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut required);
        }
        ensure!(
            required > 0 && required <= 65536,
            "workspace.terminal_failed"
        );
        let mut words = vec![0_usize; required.div_ceil(std::mem::size_of::<usize>())];
        ensure!(
            unsafe {
                InitializeProcThreadAttributeList(words.as_mut_ptr().cast(), 1, 0, &mut required)
            } != 0,
            "workspace.terminal_failed"
        );
        let mut attributes = Attributes { words };
        ensure!(
            unsafe {
                UpdateProcThreadAttribute(
                    attributes.words.as_mut_ptr().cast(),
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    hpc as *const std::ffi::c_void,
                    std::mem::size_of::<HPCON>(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            } != 0,
            "workspace.terminal_failed"
        );
        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Redirected parent handles must not bypass the pseudoconsole.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = attributes.words.as_mut_ptr().cast();
        let job = Handle(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
        ensure!(!job.0.is_null(), "workspace.terminal_failed");
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.ProcessMemoryLimit = 1024 * 1024 * 1024;
        ensure!(
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } != 0,
            "workspace.terminal_failed"
        );
        let mut created: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe {
                CreateProcessW(
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                    std::ptr::null(),
                    directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut created,
                )
            } != 0,
            "workspace.terminal_failed"
        );
        let process = Process(Handle(created.hProcess));
        let primary = Handle(created.hThread);
        ensure!(
            unsafe { AssignProcessToJobObject(job.0, process.0.0) } != 0,
            "workspace.terminal_failed"
        );
        drop(input_read);
        drop(output_write);
        let output = Arc::new(Mutex::new(Output::default()));
        let reader_output = output.clone();
        std::thread::spawn(move || {
            let handle = output_read;
            let mut bytes = [0_u8; 4096];
            loop {
                let mut read = 0;
                if unsafe {
                    ReadFile(
                        handle.0,
                        bytes.as_mut_ptr(),
                        bytes.len() as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                } == 0
                    || read == 0
                {
                    break;
                }
                let mut output = reader_output.lock().unwrap_or_else(|e| e.into_inner());
                let at = output.next;
                output.next = output.next.saturating_add(read as u64);
                output.bytes += read as usize;
                output
                    .chunks
                    .push_back((at, bytes[..read as usize].to_vec()));
                while output.bytes > 256 * 1024 {
                    if let Some((_, removed)) = output.chunks.pop_front() {
                        output.bytes -= removed.len();
                    } else {
                        break;
                    }
                }
            }
            reader_output.lock().unwrap_or_else(|e| e.into_inner()).eof = true;
        });
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(32);
        std::thread::spawn(move || {
            let handle = input_write;
            while let Ok(bytes) = receiver.recv() {
                let mut position = 0;
                while position < bytes.len() {
                    let mut written = 0;
                    if unsafe {
                        WriteFile(
                            handle.0,
                            bytes[position..].as_ptr(),
                            (bytes.len() - position) as u32,
                            &mut written,
                            std::ptr::null_mut(),
                        )
                    } == 0
                        || written == 0
                    {
                        return;
                    }
                    position += written as usize;
                }
            }
        });
        ensure!(
            unsafe { ResumeThread(primary.0) } != u32::MAX,
            "workspace.terminal_failed"
        );
        Ok(Self {
            process,
            job,
            console: Mutex::new(console),
            input: sender,
            output,
        })
    }
    pub fn input(&self, bytes: Vec<u8>) -> Result<()> {
        ensure!(!bytes.is_empty() && bytes.len() <= 4096, "workspace.limit");
        self.input
            .try_send(bytes)
            .map_err(|_| anyhow::anyhow!("workspace.terminal_busy"))
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let size = dimensions(cols, rows)?;
        let console = self.console.lock().unwrap_or_else(|e| e.into_inner());
        ensure!(
            unsafe { ResizePseudoConsole(console.0, size) } >= 0,
            "workspace.terminal_failed"
        );
        Ok(())
    }
    pub fn poll(&self, after: u64) -> Result<Value> {
        let output = self.output.lock().unwrap_or_else(|e| e.into_inner());
        ensure!(after <= output.next, "workspace.invalid");
        let first = output.chunks.front().map_or(output.next, |(at, _)| *at);
        let mut next = after.max(first);
        let mut bytes = Vec::new();
        for (at, chunk) in &output.chunks {
            if *at + chunk.len() as u64 <= next {
                continue;
            }
            let start = next.saturating_sub(*at) as usize;
            let count = (chunk.len() - start).min(16384 - bytes.len());
            bytes.extend_from_slice(&chunk[start..start + count]);
            next += count as u64;
            if bytes.len() == 16384 {
                break;
            }
        }
        let mut status = 0;
        ensure!(
            unsafe { GetExitCodeProcess(self.process.0.0, &mut status) } != 0,
            "workspace.terminal_failed"
        );
        Ok(
            json!({"data":base64::engine::general_purpose::STANDARD.encode(bytes),"after":next,"gap":after < first,"running":status == STILL_ACTIVE as u32 && !output.eof,"exit_code":if status == STILL_ACTIVE as u32 { None } else { Some(status) }}),
        )
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            TerminateJobObject(self.job.0, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn processes(job: HANDLE) -> u32 {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        info.ActiveProcesses
    }
    #[test]
    fn ending_a_pseudoconsole_terminates_descendants_too() {
        use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
        let terminal = Terminal::spawn_inner(&std::env::temp_dir(), 80, 24, true).unwrap();
        terminal.input(b"start \"\" /b \"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -NonInteractive -Command \"Start-Sleep -Seconds 300\"\r\n".to_vec()).unwrap();
        let started = std::time::Instant::now();
        while processes(terminal.job.0) < 2
            && started.elapsed() < std::time::Duration::from_secs(10)
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            processes(terminal.job.0) >= 2,
            "synthetic child did not start"
        );
        let mut duplicate = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    terminal.job.0,
                    GetCurrentProcess(),
                    &mut duplicate,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            },
            0
        );
        let duplicate = Handle(duplicate);
        drop(terminal);
        let ended = std::time::Instant::now();
        while processes(duplicate.0) > 0 && ended.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(processes(duplicate.0), 0);
    }
    #[test]
    fn private_pseudoconsole_receives_explicit_input_and_stops_its_process_tree() {
        let terminal = Terminal::spawn_inner(&std::env::temp_dir(), 80, 24, true).unwrap();
        terminal
            .input(b"echo MoodflowConPtyFixture123 & exit\r\n".to_vec())
            .unwrap();
        let started = std::time::Instant::now();
        let mut after = 0;
        let mut bytes = Vec::new();
        while started.elapsed() < std::time::Duration::from_secs(15) {
            let result = terminal.poll(after).unwrap();
            after = result["after"].as_u64().unwrap();
            bytes.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(result["data"].as_str().unwrap())
                    .unwrap(),
            );
            if !result["running"].as_bool().unwrap() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            String::from_utf8_lossy(&bytes).contains("MoodflowConPtyFixture123"),
            "synthetic ConPTY status: {}, received bytes: {}",
            terminal.poll(after).unwrap(),
            bytes.len()
        );
        assert_eq!(terminal.poll(after).unwrap()["running"], false);
        assert!(terminal.input(vec![0; 4097]).is_err());
        assert!(terminal.resize(0, 24).is_err());
    }
}
