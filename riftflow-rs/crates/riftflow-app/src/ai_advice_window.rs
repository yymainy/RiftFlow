//! Independent pre-game advice surface. Preferences are separate from scout visibility.
use riftflow_ipc::{handler, IpcError};
use serde_json::{json, Value};
use tauri::Manager;

const WINDOW: &str = "ai-advice-window";
const NAMESPACE: &str = "ai-advice-main";

pub fn start(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let state = app.state::<crate::AppState>();
    let window = crate::create_window(app, WINDOW, "ai-advice-window.html")?;
    if let Some(monitor) = window.current_monitor()? {
        let scale = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();
        window.set_position(tauri::Position::Logical((
            position.x as f64 / scale + size.width as f64 / scale - 880.0,
            position.y as f64 / scale + 80.0,
        ).into()))?;
    }
    let handle = app.clone();
    state.ipc.on_call(NAMESPACE, "control", handler(move |_context, args| {
        let handle = handle.clone();
        async move {
            let state = handle.state::<crate::AppState>();
            let command = args.first().and_then(Value::as_str).unwrap_or("");
            match command {
                "getAuto" => return state.settings.get_raw(NAMESPACE,"autoAdvice")
                    .map(|value|value.unwrap_or(json!(true))).map_err(IpcError::plain),
                "setAuto" => {
                    let enabled = args.get(1).and_then(Value::as_bool).ok_or_else(||IpcError::plain("Expected boolean"))?;
                    state.settings.save_raw(NAMESPACE,"autoAdvice",&json!(enabled)).map_err(IpcError::plain)?;
                    state.ipc.send_event(NAMESPACE,"auto-advice-changed",&[json!(enabled)]);
                }
                "show" | "hide" | "drag" => {
                    let window = handle.get_webview_window(WINDOW).ok_or_else(||IpcError::plain("AI advice window unavailable"))?;
                    match command {
                        "show" => window.show().map_err(IpcError::plain)?,
                        "hide" => window.hide().map_err(IpcError::plain)?,
                        _ => window.start_dragging().map_err(IpcError::plain)?,
                    }
                }
                _ => return Err(IpcError::plain("Unknown AI advice control")),
            }
            Ok(Value::Null)
        }
    }))?;
    Ok(())
}
