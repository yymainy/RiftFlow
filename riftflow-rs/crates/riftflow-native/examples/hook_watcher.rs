//! Watches the keyboard with a second `WH_KEYBOARD_LL` hook in its own process, so
//! a missing callback inside the app can be told apart from input that never
//! reached the system at all.
//!
//! ```text
//! cargo run -p riftflow-native --example hook_watcher -- 6
//! ```
//!
//! Prints one line per observed key. Used as the control in the A/B against the
//! app's own `[keyboard-shortcuts-main] hook callback` trace lines.

#![cfg(windows)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

type RawIsize = isize;
type HookProc = unsafe extern "system" fn(code: i32, w_param: usize, l_param: RawIsize) -> RawIsize;

#[repr(C)]
struct KbdLlHookStruct {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    time: u32,
    extra_info: *mut c_void,
}

#[repr(C)]
#[derive(Default)]
struct WinMsg {
    window: *mut c_void,
    message: u32,
    w_param: usize,
    l_param: RawIsize,
    time: u32,
    point_x: i32,
    point_y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(id_hook: i32, proc: HookProc, module: *mut c_void, thread: u32) -> *mut c_void;
    fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: RawIsize) -> RawIsize;
    fn GetMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32) -> i32;
    fn PeekMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
    fn PostQuitMessage(code: i32);
    fn SetTimer(window: *mut c_void, id: usize, elapsed: u32, cb: *mut c_void) -> usize;
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
}

const WH_KEYBOARD_LL: i32 = 13;
const PM_NOREMOVE: u32 = 3;
const LLKHF_UP: u32 = 0x80;
const WM_TIMER: u32 = 0x0113;

static SEEN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn proc(code: i32, w_param: usize, l_param: RawIsize) -> RawIsize {
    if code == 0 && l_param != 0 {
        let info = &*(l_param as *const KbdLlHookStruct);
        let injected = (info.flags & 0x10) != 0;
        let kind = if (info.flags & LLKHF_UP) == 0 { "down" } else { "up" };
        let name = MapVirtualKeyW(info.vk_code, 0);
        println!(
            "WATCHER vk={} ({name}) {kind} injected={injected}",
            info.vk_code
        );
        SEEN.fetch_add(1, Ordering::Relaxed);
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(6);

    unsafe {
        // create the queue first, exactly like the app's hook thread does
        let mut warm = WinMsg::default();
        PeekMessageW(&mut warm, std::ptr::null_mut(), 0x0400, 0x0400, PM_NOREMOVE);

        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            proc,
            GetModuleHandle(),
            0,
        );
        if hook.is_null() {
            println!("WATCHER install failed");
            return;
        }
        println!("WATCHER installed, pumping {seconds}s");

        // a timer is what lets the pump stop without another thread
        SetTimer(std::ptr::null_mut(), 1, (seconds * 1000) as u32, std::ptr::null_mut());

        let mut msg = WinMsg::default();
        loop {
            let got = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if got <= 0 {
                break;
            }
            if msg.message == WM_TIMER {
                break;
            }
        }
        PostQuitMessage(0);
        println!("WATCHER saw {} callbacks", SEEN.load(Ordering::Relaxed));
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

fn GetModuleHandle() -> *mut c_void {
    unsafe { GetModuleHandleW(std::ptr::null()) }
}
