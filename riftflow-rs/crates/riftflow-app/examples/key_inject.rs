//! Injects one key combination through `SendInput`, i.e. through the same OS
//! keyboard path a physical keypress uses, so the app's `WH_KEYBOARD_LL` hook can
//! be exercised without a hand on the keyboard. The hook still sees the events,
//! tagged `LLKHF_INJECTED`.
//!
//! run: `cargo run -p riftflow-app --example key_inject -- left-ctrl left-shift f12`

use std::ffi::c_void;
use std::thread::sleep;
use std::time::Duration;

const INPUT_KEYBOARD: u32 = 1;
const KEYEVENTF_KEYUP: u32 = 0x0002;

#[repr(C)]
union InputUnion {
    ki: KeyboardInput,
    rest: [u8; 32],
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

#[link(name = "user32")]
extern "system" {
    fn SendInput(count: u32, inputs: *const WinInput, size: i32) -> u32;
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    fn GetLastError() -> u32;
}

/// the side-specific codes, because a low-level hook reports `VK_LCONTROL` for a
/// physical left Ctrl while `VK_CONTROL` (17) only ever arrives on release
fn vk_of(name: &str) -> Option<u16> {
    let table = [
        ("left-ctrl", 162),
        ("right-ctrl", 163),
        ("left-shift", 160),
        ("right-shift", 161),
        ("left-alt", 164),
        ("right-alt", 165),
        ("left-win", 91),
        ("right-win", 92),
        ("space", 32),
        ("escape", 27),
        ("enter", 13),
        ("tab", 9),
        ("backspace", 8),
        ("f1", 112),
        ("f2", 113),
        ("f3", 114),
        ("f4", 115),
        ("f5", 116),
        ("f6", 117),
        ("f7", 118),
        ("f8", 119),
        ("f9", 120),
        ("f10", 121),
        ("f11", 122),
        ("f12", 123),
    ];

    let name = name.to_ascii_lowercase();
    if let Some((_, code)) = table.iter().find(|(key, _)| *key == name) {
        return Some(*code);
    }

    let bytes = name.as_bytes();
    if bytes.len() == 1 {
        let byte = bytes[0];
        if byte.is_ascii_alphabetic() {
            return Some(byte.to_ascii_uppercase() as u16 - 32 + 65);
        }
        if byte.is_ascii_digit() {
            return Some(byte as u16 - 48 + 48);
        }
    }

    name.strip_prefix("0x")
        .and_then(|hex| u16::from_str_radix(hex, 16).ok())
}

fn event(vk: u16, up: bool) -> WinInput {
    WinInput {
        input_type: INPUT_KEYBOARD,
        union: InputUnion {
            ki: KeyboardInput {
                vk_code: vk,
                scan: unsafe { MapVirtualKeyW(u32::from(vk), 0) } as u16,
                flags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                extra_info: 0,
            },
        },
    }
}

fn insert(input: &WinInput, label: &str) {
    unsafe {
        let inserted = SendInput(1, input, std::mem::size_of::<WinInput>() as i32);
        if inserted == 1 {
            println!("[inject] {label}");
        } else {
            // GetLastError is only meaningful after a failed call
            println!("[inject] {label} FAILED: inserted {inserted}, GetLastError {}", GetLastError());
        }
    }
}

fn main() {
    let keys: Vec<String> = std::env::args().skip(1).collect();
    if keys.is_empty() {
        eprintln!("usage: key_inject <key> [key ...]   e.g. key_inject left-ctrl left-shift f12");
        std::process::exit(2);
    }

    let mut vks = Vec::new();
    for key in &keys {
        match vk_of(key) {
            Some(vk) => vks.push(vk),
            None => {
                eprintln!("[inject] unknown key name {key}");
                std::process::exit(2);
            }
        }
    }

    println!("[inject] pressing {:?} as vks {vks:?}", keys);

    for vk in &vks {
        insert(&event(*vk, false), &format!("vk {vk} down"));
        sleep(Duration::from_millis(40));
    }

    sleep(Duration::from_millis(120));

    for vk in vks.iter().rev() {
        insert(&event(*vk, true), &format!("vk {vk} up"));
        sleep(Duration::from_millis(40));
    }

    // give the app's hook thread and IPC pipeline time to log before this exits
    sleep(Duration::from_millis(500));
    let _: *mut c_void = std::ptr::null_mut();
}
