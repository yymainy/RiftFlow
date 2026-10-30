//! Keyboard and text injection (`SendInput`), replacing the
//! `riftflow-native-win32/input` addon's `sendKey` / `sendString`.
//!
//! The original reported this capability as "available only when elevated" because
//! its addon shared one flag for *listening* (a `WH_KEYBOARD_LL` hook) and
//! *injecting*. This port keeps them apart: the hook works unelevated, and
//! injection is only blocked by UIPI when the window in front belongs to an
//! elevated process. [`available`] answers the latter question honestly instead of
//! hard-coding the former's answer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Whether the process owning the foreground window runs elevated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForegroundElevation {
    Unknown,
    Elevated,
    Standard,
}

/// `NATIVE_SUPPORT.nativeInput.available` for this port: injection can reach the
/// window that currently has focus.
///
/// `Unknown` is treated as reachable: UIPI only blocks the specific case where the
/// target is elevated and we are not, and every path that can tell (a real
/// foreground window, a queryable token) has already been tried.
pub fn injection_allowed(self_elevated: bool, foreground: ForegroundElevation) -> bool {
    cfg!(target_os = "windows")
        && match foreground {
            ForegroundElevation::Elevated => self_elevated,
            _ => true,
        }
}

/// The cancel shortcut's abort bit, shared with the shell. `in-game-send` reads it
/// between lines exactly where the original checked `!signal.aborted`.
#[derive(Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A `SendInput` writer. One instance owns one run, so the cancel flag cannot leak
/// from a previous send into the next.
#[derive(Clone, Default)]
pub struct KeyboardInjector {
    pub cancel: CancelFlag,
}

impl KeyboardInjector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cancel(cancel: CancelFlag) -> Self {
        Self { cancel }
    }

    /// `nativeInput.instance.sendKey(code, isDown)`.
    pub fn send_key(&mut self, key_code: u32, is_down: bool) {
        inject_key(key_code, is_down);
    }

    /// `nativeInput.instance.sendString(text)` — UTF-16 code units, so the CJK
    /// preset wording survives.
    pub fn send_text(&mut self, text: &str) {
        for unit in text.encode_utf16() {
            inject_unicode(unit);
        }
    }

    pub fn wait(&mut self, millis: u64) {
        std::thread::sleep(Duration::from_millis(millis));
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.cancelled()
    }

    /// The `CheckCancelled` points are already inside the queue; this is the check
    /// the original ran before opening the chat box at all.
    pub fn available(&self) -> bool {
        injection_allowed(current_process_elevated(), foreground_elevation())
    }
}

/// This process's real `TokenElevation`, which is what UIPI compares. The crate's
/// [`crate::is_elevated`] deliberately mimics the shipped addon (Administrators
/// membership in the effective token) and would report `true` for a medium-IL
/// process, so it cannot be used here.
#[cfg(windows)]
fn current_process_elevated() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let process = GetCurrentProcess();
        let mut token = HANDLE::default();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        queried.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
fn current_process_elevated() -> bool {
    false
}

#[cfg(windows)]
fn inject_key(key_code: u32, is_down: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key_code as u16),
                wScan: 0,
                dwFlags: if is_down {
                    windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(windows)]
fn inject_unicode(code_unit: u16) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VIRTUAL_KEY,
    };

    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Which process owns the window in front, and whether UIPI would let us type into
/// it. Any failure to ask (no foreground window, a protected process, a denied
/// token query) answers `Unknown` rather than guessing.
#[cfg(windows)]
pub fn foreground_elevation() -> ForegroundElevation {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return ForegroundElevation::Unknown;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return ForegroundElevation::Unknown;
        }

        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return ForegroundElevation::Unknown;
        };

        let mut token = HANDLE::default();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            let _ = CloseHandle(process);
            return ForegroundElevation::Unknown;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );

        let _ = CloseHandle(token);
        let _ = CloseHandle(process);

        if queried.is_err() {
            return ForegroundElevation::Unknown;
        }
        if elevation.TokenIsElevated != 0 {
            ForegroundElevation::Elevated
        } else {
            ForegroundElevation::Standard
        }
    }
}

#[cfg(not(windows))]
fn inject_key(_key_code: u32, _is_down: bool) {}

#[cfg(not(windows))]
fn inject_unicode(_code_unit: u16) {}

#[cfg(not(windows))]
fn foreground_elevation() -> ForegroundElevation {
    ForegroundElevation::Unknown
}

/// The same question [`KeyboardInjector::available`] asks, split into its inputs.
pub fn probe() -> (bool, ForegroundElevation) {
    let foreground = foreground_elevation();
    (injection_allowed(current_process_elevated(), foreground), foreground)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_elevated_target_requires_an_elevated_sender() {
        assert!(injection_allowed(false, ForegroundElevation::Standard));
        assert!(injection_allowed(false, ForegroundElevation::Unknown));
        assert!(!injection_allowed(false, ForegroundElevation::Elevated));
        assert!(injection_allowed(true, ForegroundElevation::Elevated));
    }

    #[test]
    fn cjk_and_emoji_survive_the_utf16_split() {
        // the preset wording is Chinese with digits; a surrogate pair is two units
        let units: Vec<u16> = "德莱文 123 🙂".encode_utf16().collect();
        assert_eq!(units.len(), "德莱文 123 🙂".chars().count() + 1);
    }

    #[test]
    fn the_cancel_flag_is_shared_by_clone() {
        let flag = CancelFlag::new();
        let injector = KeyboardInjector::with_cancel(flag.clone());
        assert!(!injector.cancelled());
        flag.cancel();
        assert!(injector.cancelled());
        flag.reset();
        assert!(!injector.cancelled());
    }
}
