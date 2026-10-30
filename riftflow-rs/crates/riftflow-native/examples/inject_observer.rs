//! Observes our own injected keystrokes with a `WH_KEYBOARD_LL` hook, so the
//! `SendInput` packets can be read back from the OS queue instead of inferred from
//! whatever a particular app decides to insert.
//!
//! ```text
//! cargo run -p riftflow-native --example inject_observer
//! ```

#![cfg(windows)]

use std::ffi::c_void;
use std::sync::Mutex;

use riftflow_native::input::KeyboardInjector;

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
    fn SetWindowsHookExW(id_hook: i32, hook_proc: HookProc, module: *mut c_void, thread_id: u32) -> *mut c_void;
    fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: RawIsize) -> RawIsize;
    fn GetMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32) -> i32;
    fn PeekMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const WinMsg) -> i32;
    fn DispatchMessageW(msg: *const WinMsg) -> RawIsize;
    fn PostQuitMessage(code: u32);
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

const WH_KEYBOARD_LL: i32 = 13;
const PM_REMOVE: u32 = 0x0001;
const LLKHF_UP: u32 = 0x80;
const LLKHF_UNICODE: u32 = 0x04;
const LLKHF_INJECTED: u32 = 0x10;

static OBSERVED: Mutex<Vec<(u32, u32, u32)>> = Mutex::new(Vec::new());

unsafe extern "system" fn proc(code: i32, w_param: usize, l_param: RawIsize) -> RawIsize {
    if code >= 0 {
        let data = &*(l_param as *const KbdLlHookStruct);
        if data.flags & LLKHF_INJECTED != 0 {
            OBSERVED.lock().unwrap().push((data.vk_code, data.scan_code, data.flags));
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

fn main() {
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Ab9Z".to_string());

    let sender_text = text.clone();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let mut injector = KeyboardInjector::new();
        injector.send_text(&sender_text);
    });

    unsafe {
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            proc,
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if hook.is_null() {
            println!("OBSERVER: SetWindowsHookExW failed");
            sender.join().unwrap();
            return;
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let expected = text.encode_utf16().count() * 2;
        let mut msg = WinMsg::default();
        while std::time::Instant::now() < deadline && OBSERVED.lock().unwrap().len() < expected {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        PostQuitMessage(0);
        // drop the hook by letting the process exit; UnhookWindowsHookEx is not
        // needed for a probe that ends here
    }

    sender.join().unwrap();
    let seen = OBSERVED.lock().unwrap().clone();
    println!(
        "OBSERVER: {text:?} -> {} units expected {} events, saw {}",
        text.encode_utf16().count(),
        text.encode_utf16().count() * 2,
        seen.len()
    );
    for (vk, scan, flags) in &seen {
        let ch = char::from_u32(*scan).map(|c| c.to_string()).unwrap_or_default();
        println!(
            "  vk={vk:#04x} scan={scan:#04x} {ch:<3} up={} unicode={} injected={}",
            flags & LLKHF_UP != 0,
            flags & LLKHF_UNICODE != 0,
            flags & LLKHF_INJECTED != 0
        );
    }
    let units: Vec<u32> = seen.iter().filter(|(_, _, f)| f & LLKHF_UP == 0).map(|(_, s, _)| *s).collect();
    let wanted: Vec<u32> = text.encode_utf16().map(|u| u as u32).collect();
    println!("down-scan sequence: {units:?} expected {wanted:?} match={}", units == wanted);
}
