//! Independent, non-activating opponent summary window. The existing ongoing-game
//! service owns all player queries; this module only owns window controls/preferences.
use serde_json::{Value, json};
use tauri::Manager;
use riftflow_ipc::IpcError;

const WINDOW: &str = "opponent-scout-window";
const NAMESPACE: &str = "opponent-scout-main";

pub fn start(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let state = app.state::<crate::AppState>();
    let window = crate::create_window(app, WINDOW, "opponent-scout-window.html")?;
    // Start near the right edge; the native title strip can be dragged anywhere.
    if let Some(monitor) = window.current_monitor()? {
        let scale = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();
        let x = position.x as f64 / scale + size.width as f64 / scale - 420.0;
        let y = position.y as f64 / scale + 60.0;
        window.set_position(tauri::Position::Logical((x, y).into()))?;
    }
    let handle = app.clone();
    state.ipc.on_call(NAMESPACE, "control", riftflow_ipc::handler(move |_ctx, args| {
        let handle = handle.clone();
        async move {
            let state = handle.state::<crate::AppState>();
            let command = args.first().and_then(Value::as_str).unwrap_or("");
            let window = handle.get_webview_window(WINDOW)
                .ok_or_else(|| IpcError::plain("Opponent scout window is unavailable"))?;
            match command {
                "getEnabled" => return state.settings.get_raw(NAMESPACE, "enabled")
                    .map(|value| value.unwrap_or(json!(true))).map_err(IpcError::plain),
                "setEnabled" => {
                    let enabled = args.get(1).and_then(Value::as_bool)
                        .ok_or_else(|| IpcError::plain("Expected a boolean"))?;
                    state.settings.save_raw(NAMESPACE, "enabled", &json!(enabled)).map_err(IpcError::plain)?;
                    state.ipc.send_event(NAMESPACE, "enabled-changed", &[json!(enabled)]);
                    if !enabled { window.hide().map_err(IpcError::plain)?; }
                }
                "show" => {
                    window.set_always_on_top(true).map_err(IpcError::plain)?;
                    window.show().map_err(IpcError::plain)?;
                }
                "hide" => window.hide().map_err(IpcError::plain)?,
                "drag" => window.start_dragging().map_err(IpcError::plain)?,
                "collapse" | "expand" => window.set_size(tauri::Size::Logical(
                    (400.0, if command == "collapse" { 48.0 } else { 610.0 }).into()
                )).map_err(IpcError::plain)?,
                _ => return Err(IpcError::plain("Unknown opponent scout control")),
            }
            Ok(Value::Null)
        }
    }))?;
    Ok(())
}
