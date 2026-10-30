//! The clipboard half of the Win32 surface, replacing Electron's
//! `clipboard.readText()` (`src/main/shards/app-common/index.ts`).
//!
//! The classic clipboard API is used instead of `IClipboard`, so no COM
//! initialisation is required and the call is safe from a blocking thread.
//! Raw `#[link]` externs are used rather than the `windows` crate because this
//! crate's feature list does not include `Win32_System_DataExchange` /
//! `Win32_System_Memory` (and its `Cargo.toml` is not ours to change).

use anyhow::Result;

/// `CF_UNICODETEXT`
#[cfg(windows)]
const CF_UNICODE_TEXT: u32 = 13;

/// `clipboard.readText()`.
///
/// An empty `String` means "the clipboard holds no text" — the same answer
/// Electron gives for an empty or non-text clipboard. `Err` is reserved for the
/// clipboard being genuinely unreadable (another process owns it, the lock
/// failed), which the original surfaces as a thrown error too.
pub fn read_text() -> Result<String> {
    #[cfg(windows)]
    {
        win32::read_text()
    }

    #[cfg(not(windows))]
    {
        anyhow::bail!("clipboard reading is only implemented for win32")
    }
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    use anyhow::{Result, anyhow};

    use super::CF_UNICODE_TEXT;

    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(window: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalSize(handle: *mut c_void) -> usize;
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
    }

    pub fn read_text() -> Result<String> {
        // a NULL window opens the clipboard "owned by nothing", like Electron's read
        if unsafe { OpenClipboard(null_mut()) } == 0 {
            return Err(anyhow!(
                "OpenClipboard failed: another process owns the clipboard"
            ));
        }

        let text = read_text_while_open();

        unsafe {
            // every path has to close it, otherwise the clipboard stays locked
            CloseClipboard();
        }

        text
    }

    fn read_text_while_open() -> Result<String> {
        let handle = unsafe { GetClipboardData(CF_UNICODE_TEXT) };
        if handle.is_null() {
            // no CF_UNICODETEXT on the clipboard
            return Ok(String::new());
        }

        let size = unsafe { GlobalSize(handle) };
        let locked = unsafe { GlobalLock(handle) };
        if locked.is_null() {
            return Err(anyhow!("GlobalLock returned no data"));
        }

        let text = if size < 2 {
            Ok(String::new())
        } else {
            // the block is UTF-16 with a terminating NUL
            let units = unsafe { std::slice::from_raw_parts(locked as *const u16, size / 2) };
            let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
            Ok(String::from_utf16_lossy(&units[..end]))
        };

        unsafe {
            GlobalUnlock(handle);
        }

        text
    }
}
