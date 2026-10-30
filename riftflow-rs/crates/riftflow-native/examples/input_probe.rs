//! Types into whatever window currently has focus, so `SendInput` can be verified
//! against a real target instead of assumed to work.
//!
//! ```text
//! cargo run -p riftflow-native --example input_probe -- "RiftFlow测试123"
//! ```
//!
//! The harness (`tmp/verify_input.ps1`) starts Notepad, activates it, runs this,
//! then reads the document back over UI Automation.

use std::time::Duration;

use riftflow_native::input::{CancelFlag, KeyboardInjector, probe};

fn main() {
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "RiftFlow测试123".to_string());
    let delay = std::env::args()
        .nth(2)
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(600);

    std::thread::sleep(Duration::from_millis(delay));

    let (allowed, foreground) = probe();
    let mut injector = KeyboardInjector::with_cancel(CancelFlag::new());
    println!(
        "PROBE foreground={foreground:?} injectionAllowed={allowed} available={}",
        injector.available()
    );

    if !injector.available() {
        println!("PROBE: injection refused for this foreground window, nothing typed");
        return;
    }

    injector.send_text(&text);
    println!("PROBE: sent {text:?} with the Enter keystroke");
}
