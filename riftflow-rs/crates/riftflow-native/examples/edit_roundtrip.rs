//! Injects into a classic `EDIT` control this process owns and reads it back with
//! `WM_GETTEXT`. Win11 Notepad goes through text services, which coalesced the
//! injected characters, so it cannot tell whether `SendInput` was right; a plain
//! edit control processes `WM_CHAR` directly.
//!
//! ```text
//! cargo run -p riftflow-native --example edit_roundtrip
//! ```

#![cfg(windows)]

use std::ffi::c_void;

use riftflow_native::input::KeyboardInjector;

type RawIsize = isize;
type LResult = isize;

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

#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: unsafe extern "system" fn(*mut c_void, u32, usize, RawIsize) -> LResult,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: *mut c_void,
    h_icon: *mut c_void,
    h_cursor: *mut c_void,
    h_brush: *mut c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(cls: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *const c_void,
    ) -> *mut c_void;
    fn DefWindowProcW(window: *mut c_void, message: u32, w_param: usize, l_param: RawIsize) -> LResult;
    fn DestroyWindow(window: *mut c_void) -> i32;
    fn PeekMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const WinMsg) -> i32;
    fn DispatchMessageW(msg: *const WinMsg) -> LResult;
    fn SendMessageW(window: *mut c_void, message: u32, w_param: usize, l_param: RawIsize) -> LResult;
    fn SetFocus(window: *mut c_void) -> *mut c_void;
    fn SetForegroundWindow(window: *mut c_void) -> i32;
    fn ShowWindow(window: *mut c_void, command: i32) -> i32;
    fn GetWindowTextW(window: *mut c_void, buffer: *mut u16, max_count: i32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CHILD: u32 = 0x2000_0000;
const WS_BORDER: u32 = 0x0080_0000;
const ES_AUTOHSCROLL: u32 = 0x0040;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const SW_SHOW: i32 = 5;
const PM_REMOVE: u32 = 0x0001;
const WM_GETTEXT: u32 = 0x000D;
const WM_GETTEXTLENGTH: u32 = 0x000E;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(
    window: *mut c_void,
    message: u32,
    w_param: usize,
    l_param: RawIsize,
) -> LResult {
    DefWindowProcW(window, message, w_param, l_param)
}

fn pump(rounds: usize) {
    unsafe {
        let mut msg = WinMsg::default();
        for _ in 0..rounds {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

fn read_text(window: *mut c_void) -> String {
    unsafe {
        let length = SendMessageW(window, WM_GETTEXTLENGTH, 0, 0).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        SendMessageW(
            window,
            WM_GETTEXT,
            length + 1,
            buffer.as_mut_ptr() as RawIsize,
        );
        String::from_utf16_lossy(&buffer[..length])
    }
}

fn main() {
    let expected = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Ab9Zk德莱文12".to_string());

    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class_name = wide("RiftFlowEditProbe");
        let class = WndClassW {
            style: 0,
            lpfn_wnd_proc: wndproc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: module,
            h_icon: std::ptr::null_mut(),
            h_cursor: std::ptr::null_mut(),
            h_brush: std::ptr::null_mut(),
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class_name.as_ptr(),
        };
        if RegisterClassW(&class) == 0 {
            println!("ROUNDTRIP: RegisterClassW failed");
            return;
        }

        let host = CreateWindowExW(
            WS_EX_TOPMOST,
            class_name.as_ptr(),
            wide("riftflow edit probe").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            120,
            120,
            520,
            180,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if host.is_null() {
            println!("ROUNDTRIP: host window failed");
            return;
        }

        let edit = CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL,
            10,
            10,
            480,
            60,
            host,
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if edit.is_null() {
            println!("ROUNDTRIP: edit control failed");
            return;
        }

        ShowWindow(host, SW_SHOW);
        pump(6);
        SetFocus(edit);
        SetForegroundWindow(host);
        pump(3);

        let before = read_text(edit);
        let mut injector = KeyboardInjector::new();
        println!(
            "ROUNDTRIP available={} before=[{before}]",
            injector.available()
        );
        injector.send_text(&expected);
        pump(6);

        let after = read_text(edit);
        println!("ROUNDTRIP injected {expected:?}");
        println!("ROUNDTRIP edit holds [{after}]");
        println!(
            "ROUNDTRIP match={} ({} units vs {})",
            after == expected,
            after.encode_utf16().count(),
            expected.encode_utf16().count()
        );

        DestroyWindow(host);
    }
}
