//! Runs `src/hotkeys.rs` exactly as the app compiles it (via `#[path]`, so this
//! is the same source, not a copy), installs the real hook, registers a real
//! shortcut and injects the keypress, printing the availability state at each
//! step. Exists because the app's own log lines can be confounded by other
//! processes in the session; this probe is self-contained and deterministic.
//!
//! run: `cargo run -p riftflow-app --example hotkeys_self_probe`

use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use riftflow_ipc::{RiftFlowIpcMain, RendererId, RendererTransport};
use serde_json::Value;

#[path = "../src/hotkeys.rs"]
mod hotkeys;
/// `hotkeys::log` writes through the shell's logger, so the probe has to compile the
/// same module the app does rather than stub it.
#[path = "../src/logger.rs"]
mod logger;

const PROBE_RENDERER: RendererId = 1;
const SHORTCUT_ID: &str = "LeftControl+LeftShift+F10";
/// left ctrl, left shift, F10 - the side-specific codes, as a physical press reports them
const COMBO: [u16; 3] = [162, 160, 121];

struct PrintTransport;

impl RendererTransport for PrintTransport {
    fn send_to(&self, renderer: RendererId, event: &str, args: Vec<Value>) -> anyhow::Result<()> {
        println!(
            "[probe] -> renderer {renderer} {event} {}",
            serde_json::to_string(&args)?
        );
        Ok(())
    }
}

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
}

fn key_event(vk: u16, up: bool) -> WinInput {
    WinInput {
        input_type: 1,
        union: InputUnion {
            ki: KeyboardInput {
                vk_code: vk,
                scan: unsafe { MapVirtualKeyW(u32::from(vk), 0) } as u16,
                flags: if up { 0x0002 } else { 0 },
                time: 0,
                extra_info: 0,
            },
        },
    }
}

fn send(input: &WinInput) {
    unsafe {
        let inserted = SendInput(1, input, std::mem::size_of::<WinInput>() as i32);
        println!(
            "[probe] SendInput vk={} inserted={inserted}",
            input.union.ki.vk_code
        );
    }
}

fn main() {
    println!("[probe] availability before any install = {}", hotkeys::input_available());

    let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(PrintTransport)));
    ipc.renderer_register(PROBE_RENDERER, None);

    let shard = hotkeys::start(&ipc).expect("hotkeys::start");
    println!("[probe] availability after hotkeys::start = {}", hotkeys::input_available());
    println!("[probe] elevated = {}", riftflow_native::is_elevated());

    shard
        .register("probe-target", SHORTCUT_ID, hotkeys::RegistrationType::Normal, |details| {
            println!(
                "[probe] REGISTERED SHORTCUT FIRED: id={} unifiedId={} keyCodes={:?} pressed={}",
                details.id, details.unified_id, details.key_codes, details.pressed
            );
        })
        .expect("register");

    println!(
        "[probe] getRegistration({SHORTCUT_ID}) = {:?}",
        shard.get_registration(SHORTCUT_ID).map(|registration| registration.to_json())
    );

    for round in 1..=3 {
        println!("[probe] --- round {round}: pressing {SHORTCUT_ID} ---");
        for vk in COMBO {
            send(&key_event(vk, false));
            sleep(Duration::from_millis(40));
        }
        sleep(Duration::from_millis(150));
        for vk in COMBO.iter().rev() {
            send(&key_event(*vk, true));
            sleep(Duration::from_millis(40));
        }
        sleep(Duration::from_millis(700));
    }

    println!("[probe] availability at exit = {}", hotkeys::input_available());
    println!(
        "[probe] internal vars = {}",
        shard.internal_vars()
    );
}
