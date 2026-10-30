//! Port of the capability surface that `native/win32-x64`'s `riftflow-tools` addon
//! provided, plus the availability questions from `src/main/native/index.ts`.
//!
//! `is_elevated` reproduces `IsElevated` (`tools.cc:192-213`): it tests membership
//! of the Administrators group in the **effective** token with
//! `CheckTokenMembership`, which is deliberately *not*
//! `GetTokenInformation(TokenElevation)`. The consequence is kept on purpose — a
//! standard user who belongs to Administrators reports `true`, exactly like the
//! shipped addon did, because that is what the UI's `requiresElevation` flags were
//! built against.

pub mod clipboard;
pub mod input;
pub mod placement;

use anyhow::anyhow;

pub const LEAGUE_CLIENT_UX_PROCESS_NAME: &str = "LeagueClientUx.exe";
pub const LEAGUE_CLIENT_PROCESS_NAME: &str = "LeagueClient.exe";
pub const GAME_CLIENT_PROCESS_NAME: &str = "League of Legends.exe";

/// `tools.isElevated()` — always answers, never raises; any failure means `false`.
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::Security::{
            AllocateAndInitializeSid, CheckTokenMembership, FreeSid, PSID, SECURITY_NT_AUTHORITY,
        };
        use windows::Win32::System::SystemServices::{
            DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID,
        };

        let authority = SECURITY_NT_AUTHORITY;
        let mut administrators_group = PSID::default();

        unsafe {
            if AllocateAndInitializeSid(
                &authority,
                2,
                SECURITY_BUILTIN_DOMAIN_RID as u32,
                DOMAIN_ALIAS_RID_ADMINS as u32,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut administrators_group,
            )
            .is_err()
            {
                return false;
            }

            // a NULL token handle means "current thread's token", as in the addon
            let mut is_member = BOOL::default();
            let membership = CheckTokenMembership(None, administrators_group, &mut is_member);
            FreeSid(administrators_group);

            membership.is_ok() && is_member.0 != 0
        }
    }

    #[cfg(not(windows))]
    {
        false
    }
}

/// `tools.getPidsByName` — Toolhelp snapshot with an exact name match.
pub fn pids_by_name(process_name: &str) -> Vec<u32> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return Vec::new();
        };

        // the wide form, which removes the ANSI/Unicode ambiguity in the addon's
        // `strcmp((const char*)pe32.szExeFile, …)`
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut pids = Vec::new();

        unsafe {
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    if wide_to_string(&entry.szExeFile) == process_name {
                        pids.push(entry.th32ProcessID);
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }

        pids
    }

    #[cfg(not(windows))]
    {
        let _ = process_name;
        Vec::new()
    }
}

#[cfg(windows)]
fn wide_to_string(buffer: &[u16]) -> String {
    let terminator = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..terminator])
}

/// `tools.isProcessRunning` — a successful limited-information open means alive.
pub fn is_process_running(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

        let opened =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
        match opened {
            Ok(handle) => {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                true
            }
            Err(_) => false,
        }
    }

    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// `tools.isProcessForeground` — the foreground window's process id compared to
/// `pid`. Flagged by `NATIVE_SUPPORT.isProcessForeground` as win32 + elevated.
pub fn is_process_foreground(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        let foreground = unsafe { GetForegroundWindow() };
        if foreground.is_invalid() {
            return false;
        }
        let mut target: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(foreground, Some(&mut target));
        }
        target == pid
    }

    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// `tools.terminateProcess` — `PROCESS_TERMINATE` + exit code 0.
pub fn terminate_process(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };

        let Ok(handle) = (unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }) else {
            return false;
        };

        let terminated = unsafe { TerminateProcess(handle, 0) }.is_ok();
        unsafe {
            let _ = CloseHandle(handle);
        }
        terminated
    }

    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// `tools.getCommandLine1` — `NtQueryInformationProcess` with
/// `ProcessCommandLineInformation` (60), sized in two passes like the C++ source.
///
/// NOTE: the addon signalled failure with the sentinel string
/// `"Failed to retrieve command line."`; the Rust port returns `Err` instead, and
/// the caller (`riftflow_lcu::discovery::CommandLineReader`) treats `Err` as
/// "unreadable", which is the same observable outcome.
#[cfg(windows)]
pub fn read_command_line(pid: u32) -> anyhow::Result<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(
            process: HANDLE,
            information_class: u32,
            information: *mut core::ffi::c_void,
            information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    // the same three NTSTATUS values the addon's tools.h:13-23 accepted, written
    // as u32 casts so the signed conversion cannot drift
    const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005u32 as i32;
    const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|error| anyhow!("OpenProcess({pid}): {error}"))?;

    let mut needed: u32 = 0;
    let sizing = unsafe {
        NtQueryInformationProcess(handle, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &mut needed)
    };

    let retryable = [STATUS_BUFFER_OVERFLOW, STATUS_BUFFER_TOO_SMALL, STATUS_INFO_LENGTH_MISMATCH];
    if sizing != 0 && !retryable.contains(&sizing) {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(anyhow!(
            "command line sizing failed with {:#010x}",
            sizing as u32
        ));
    }

    // A failed sizing call can leave `returnLength` unset rather than zeroed, so
    // the reported size is range-checked before it becomes an allocation.
    if needed == 0 || needed > 1_048_576 {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(anyhow!("implausible command line buffer size: {needed}"));
    }

    let mut buffer = vec![0u8; needed as usize];
    let mut written: u32 = 0;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut written,
        )
    };

    unsafe {
        let _ = CloseHandle(handle);
    }

    // The payload is a UNICODE_STRING. On x64 that lays out as two u16 fields,
    // four bytes of alignment padding, then the pointer at offset 8 — reading the
    // pointer anywhere else dereferences garbage.
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }

    if (status as u32) != 0 {
        return Err(anyhow!(
            "command line query failed with {:#010x}",
            status as u32
        ));
    }

    if (needed as usize) < std::mem::size_of::<UnicodeString>() {
        return Err(anyhow!("command line buffer too small: {needed}"));
    }

    let parsed = unsafe { &*(buffer.as_ptr() as *const UnicodeString) };
    let length = parsed.length as usize;

    if parsed.buffer.is_null() || length == 0 || length % 2 != 0 {
        return Err(anyhow!("no command line available"));
    }

    let units = unsafe { std::slice::from_raw_parts(parsed.buffer, length / 2) };
    Ok(String::from_utf16_lossy(units))
}

#[cfg(not(windows))]
pub fn read_command_line(pid: u32) -> anyhow::Result<String> {
    Err(anyhow!("command line reading is win32-only (pid {pid})"))
}

/// [`ProcessProbe`](riftflow_lcu::discovery::ProcessProbe) on the same Win32 calls the
/// addon used, so auto-connect works without shelling out to PowerShell.
pub struct Win32Probe;

impl riftflow_lcu::discovery::ProcessProbe for Win32Probe {
    fn pids_by_name(&self, process_name: &str) -> Vec<u32> {
        pids_by_name(process_name)
    }

    fn command_line(&self, pid: u32) -> anyhow::Result<String> {
        read_command_line(pid)
    }
}

/// `dialog.showSaveDialog({ defaultPath, filters: [{ name: 'JSON', extensions: ['json'] }] })`
/// from Electron. Returns `None` when the user cancels, which is what the callers
/// treat as "do nothing" — the original returns `undefined` in that case too.
pub fn pick_save_path(default_name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        shell_dialog(true, default_name)
    }
    #[cfg(not(windows))]
    {
        let _ = default_name;
        None
    }
}

/// `dialog.showOpenDialog(...)` — a single existing file.
pub fn pick_open_path(default_name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        shell_dialog(false, default_name)
    }
    #[cfg(not(windows))]
    {
        let _ = default_name;
        None
    }
}

#[cfg(windows)]
fn shell_dialog(save: bool, default_name: &str) -> Option<String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        CoTaskMemFree,
    };
    use windows::Win32::UI::Shell::{
        FOS_FILEMUSTEXIST, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FileOpenDialog, FileSaveDialog,
        IFileDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::{PCWSTR, w};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;

    // the common item dialogs need COM; re-initializing on this thread is a no-op
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok()? };
    struct Apartment;
    impl Drop for Apartment { fn drop(&mut self) { unsafe { CoUninitialize() } } }
    let _apartment = Apartment;

    let clsid = if save { &FileSaveDialog } else { &FileOpenDialog };
    let file_dialog: IFileDialog = unsafe { CoCreateInstance(clsid, None, CLSCTX_INPROC_SERVER) }.ok()?;

    let options = if save {
        FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST
    } else {
        FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST
    };
    unsafe { file_dialog.SetOptions(options).ok()? };

    let csv = default_name.ends_with(".csv");
    let filter = if csv {
        COMDLG_FILTERSPEC { pszName: w!("CSV"), pszSpec: w!("*.csv") }
    } else {
        COMDLG_FILTERSPEC { pszName: w!("JSON"), pszSpec: w!("*.json") }
    };
    unsafe { file_dialog.SetFileTypes(&[filter]).ok()? };
    if save {
        unsafe { file_dialog.SetDefaultExtension(if csv { w!("csv") } else { w!("json") }).ok()? };
    }

    if save && !default_name.is_empty() {
        let name: Vec<u16> = default_name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { file_dialog.SetFileName(PCWSTR(name.as_ptr())).ok()? };
    }

    unsafe {
        // no owner window: Electron passes the main window, a modal without one
        // still comes to the front
        file_dialog.Show(HWND::default()).ok()?;
        let item = file_dialog.GetResult().ok()?;
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
        text
    }
}
