//! Reproduces `hotkeys.rs`'s `mod hook` in isolation: installs a `WH_KEYBOARD_LL`
//! hook from a non-elevated process and reports what Windows actually does.
//!
//! run: `cargo run -p riftflow-app --example keyboard-hook-probe -- <seconds>`

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type raw_isize = isize;

const WH_KEYBOARD_LL: i32 = 13;
const PM_NOREMOVE: u32 = 0x0003;
const WM_USER: u32 = 0x0400;
/// posted at the hook thread to prove its message loop is still iterating
const WM_PROBE_PING: u32 = WM_USER + 7;
const LLKHF_UP: u32 = 0x80;
const LLKHF_INJECTED: u32 = 0x10;
const INPUT_KEYBOARD: u32 = 1;
const KEYEVENTF_KEYUP: u32 = 0x0002;

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
    l_param: raw_isize,
    time: u32,
    point_x: i32,
    point_y: i32,
}

#[repr(C)]
union InputUnion {
    ki: KeyboardInput,
    mi: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyboardInput {
    vk_code: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[repr(C)]
struct WinInput {
    input_type: u32,
    union: InputUnion,
}

type HookProc =
    unsafe extern "system" fn(code: i32, w_param: usize, l_param: raw_isize) -> raw_isize;

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(
        id_hook: i32,
        hook_proc: HookProc,
        module: *mut c_void,
        thread_id: u32,
    ) -> *mut c_void;
    fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: raw_isize) -> raw_isize;
    fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
    fn GetMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32) -> i32;
    fn PeekMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const WinMsg) -> i32;
    fn DispatchMessageW(msg: *const WinMsg) -> raw_isize;
    fn SendInput(count: u32, inputs: *const WinInput, size: i32) -> u32;
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    fn PostThreadMessageW(thread_id: u32, message: u32, w_param: usize, l_param: raw_isize) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetCurrentThreadId() -> u32;
    fn GetLastError() -> u32;
}

static CALLBACKS: AtomicUsize = AtomicUsize::new(0);
static DOWN_EVENTS: AtomicUsize = AtomicUsize::new(0);
static INJECTED_EVENTS: AtomicUsize = AtomicUsize::new(0);
static LAST_VK: AtomicU32 = AtomicU32::new(0);
static SINK: OnceLock<Sender<String>> = OnceLock::new();

fn describe(vk: u32, is_down: bool, injected: bool) -> String {
    format!("vk={vk} {}", if is_down { "down" } else { "up" })
        + if injected { " (injected)" } else { " (physical)" }
}

unsafe extern "system" fn probe_proc(code: i32, _w_param: usize, l_param: raw_isize) -> raw_isize {
    if code == 0 && l_param != 0 {
        let info = &*(l_param as *const KbdLlHookStruct);
        let is_down = (info.flags & LLKHF_UP) == 0;
        let injected = (info.flags & LLKHF_INJECTED) != 0;
        CALLBACKS.fetch_add(1, Ordering::SeqCst);
        LAST_VK.store(info.vk_code, Ordering::SeqCst);
        if is_down {
            DOWN_EVENTS.fetch_add(1, Ordering::SeqCst);
        }
        if injected {
            INJECTED_EVENTS.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(sink) = SINK.get() {
            let _ = sink.send(describe(info.vk_code, is_down, injected));
        }
    }
    CallNextHookEx(null_mut(), code, _w_param, l_param)
}

fn inject_key(vk: u16) {
    let scan = unsafe { MapVirtualKeyW(u32::from(vk), 0) } as u16;
    let down = WinInput {
        input_type: INPUT_KEYBOARD,
        union: InputUnion {
            ki: KeyboardInput {
                vk_code: vk,
                scan,
                flags: 0,
                time: 0,
                extra_info: 0,
            },
        },
    };
    let up = WinInput {
        input_type: INPUT_KEYBOARD,
        union: InputUnion {
            ki: KeyboardInput {
                vk_code: vk,
                scan,
                flags: KEYEVENTF_KEYUP,
                time: 0,
                extra_info: 0,
            },
        },
    };
    unsafe {
        // INPUT's size must be reported exactly or SendInput refuses
        let n1 = SendInput(1, &down, std::mem::size_of::<WinInput>() as i32);
        let err1 = GetLastError();
        let n2 = SendInput(1, &up, std::mem::size_of::<WinInput>() as i32);
        let err2 = GetLastError();
        println!("[probe] SendInput vk={vk}: down inserted {n1} (err {err1}), up inserted {n2} (err {err2})");
    }
}

fn main() {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(12);
    // `noinject` keeps the hook up without the self-injected F13, so a run can
    // tell "another hook exists in the session" apart from "another process
    // injected a key"
    let skip_inject = std::env::args().nth(2).is_some_and(|flag| flag == "noinject");

    println!(
        "[probe] elevated = {} (the hook is installed regardless of this)",
        riftflow_native::is_elevated()
    );
    println!(
        "[probe] sizeof(INPUT) passed to SendInput = {}",
        std::mem::size_of::<WinInput>()
    );

    let (ready_sender, ready) = channel::<Option<u32>>();
    let (thread_id_sender, thread_id) = channel::<u32>();
    let (event_sender, events) = channel::<String>();
    let _ = SINK.set(event_sender);

    std::thread::Builder::new()
        .name("probe-keyboard-hook".to_string())
        .spawn(move || unsafe {
            let mut msg = WinMsg::default();
            let _ = thread_id_sender.send(GetCurrentThreadId());

            // force this thread's queue into existence before installing
            PeekMessageW(&mut msg, null_mut(), WM_USER, WM_USER, PM_NOREMOVE);

            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                probe_proc,
                GetModuleHandleW(null_mut()),
                0,
            );
            let error = if hook.is_null() { GetLastError() } else { 0 };
            let _ = ready_sender.send(if hook.is_null() { Some(error) } else { None });

            if hook.is_null() {
                return;
            }

            let mut pumped = 0usize;
            loop {
                let got = GetMessageW(&mut msg, null_mut(), 0, 0);
                if got <= 0 {
                    break;
                }
                pumped += 1;
                if msg.message == WM_PROBE_PING {
                    println!("[probe] hook thread message loop alive, pumped {pumped} messages");
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            UnhookWindowsHookEx(hook);
        })
        .expect("spawn hook thread");

    match ready.recv().unwrap() {
        None => println!("[probe] SetWindowsHookExW(WH_KEYBOARD_LL) SUCCEEDED in a non-elevated process"),
        Some(error) => {
            println!("[probe] SetWindowsHookExW(WH_KEYBOARD_LL) FAILED, GetLastError = {error}");
            std::process::exit(1);
        }
    }

    let hook_thread = thread_id.recv().unwrap();
    unsafe {
        let posted = PostThreadMessageW(hook_thread, WM_PROBE_PING, 0, 0);
        println!("[probe] PostThreadMessageW to hook thread {hook_thread} -> {posted}");
    }

    std::thread::sleep(Duration::from_millis(300));

    if skip_inject {
        println!("[probe] self-injection skipped (noinject)");
    } else {
        // VK_F13 is a key no physical layout sends on its own, so anything observed
        // here came from this probe
        inject_key(0x7C);
    }

    let started = Instant::now();
    let mut seen = Vec::new();
    while started.elapsed() < Duration::from_secs(seconds) {
        match events.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                println!("[probe] hook event: {line}");
                seen.push(line);
            }
            Err(_) => continue,
        }
    }

    println!(
        "[probe] summary: {} callbacks, {} downs, {} injected, last vk = {}, observed = {seen:?}",
        CALLBACKS.load(Ordering::SeqCst),
        DOWN_EVENTS.load(Ordering::SeqCst),
        INJECTED_EVENTS.load(Ordering::SeqCst),
        LAST_VK.load(Ordering::SeqCst),
    );
}
