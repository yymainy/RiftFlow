//! Personal match archive and connection diagnostics. Independent of UI state.
mod controller;
mod store;
mod personal;
mod vault;
mod batch;
mod item_batch;
mod patches;
mod patch_options;
mod patch_china;
mod assets;

use std::path::Path;
use std::sync::Arc;
use riftflow_ipc::{handler, RiftFlowIpcMain, IpcError};
use riftflow_lcu::LeagueClientMain;
use riftflow_state::StateSync;
use serde_json::{json, Value};
use controller::ArchiveController;
use store::ArchiveStore;

pub fn register(ipc: &Arc<RiftFlowIpcMain>, client: &Arc<LeagueClientMain>, state: &Arc<StateSync>, directory: &Path, settings: &Arc<riftflow_store::settings::SettingFactory>) -> anyhow::Result<()> {
    let controller = Arc::new(ArchiveController::new(ArchiveStore::open(&directory.join("match-archive.db"))?, client.clone(), state.clone()));
    for method in ["list", "get", "sync", "diagnostics", "export", "restore", "personal", "vault", "champions", "batch", "itemBatch", "patches", "assets"] {
        let controller = controller.clone();
        let settings = settings.clone();
        ipc.on_call("riftflow-archive-main", method, handler(move |_context, args| {
            let controller = controller.clone();
            let settings = settings.clone();
            async move {
                match method {
                    "assets" if args.first().and_then(Value::as_str) == Some("export") => {
                        let data = controller.store.document("skin-assets").map_err(IpcError::plain)?;
                        let rows = data["skins"].as_array().ok_or_else(|| IpcError::plain("请先同步皮肤资产"))?;
                        let Some(path) = crate::file_io::pick_path(true, "riftflow-skins.csv").await else { return Ok(json!({"cancelled":true})); };
                        let mut csv = String::from("\u{feff}Champion ID,Champion,Skin ID,Skin,Owned\r\n");
                        for row in rows {
                            let field = |name: &str| format!("\"{}\"", row[name].as_str().unwrap_or("").replace('"', "\"\""));
                            csv.push_str(&format!("{},{},{},{},{}\r\n",row["championId"],field("championName"),row["id"],field("name"),row["owned"]));
                        }
                        std::fs::write(&path,csv).map_err(IpcError::plain)?;
                        Ok(json!({"cancelled":false,"path":path}))
                    }
                    "assets" => controller.assets(args.first().and_then(Value::as_str).unwrap_or("cached")).await.map_err(IpcError::plain),
                    "patches" => controller.patches(args.first().unwrap_or(&Value::Null)).await.map_err(IpcError::plain),
                    "champions" => serde_json::from_str(include_str!("../../assets/champion-summary.json")).map_err(IpcError::plain),
                    "batch" => controller.batch(&settings, args.first().and_then(Value::as_str).unwrap_or("preview"), args.get(1).unwrap_or(&Value::Null)).await.map_err(IpcError::plain),
                    "itemBatch" => controller.item_batch(args.first().and_then(Value::as_str).unwrap_or("list"), args.get(1).unwrap_or(&Value::Null)).await.map_err(IpcError::plain),
                    "personal" => controller.personal(args.first().and_then(Value::as_str).unwrap_or("get"), args.get(1).unwrap_or(&Value::Null)).map_err(IpcError::plain),
                    "vault" => controller.vault(args.first().and_then(Value::as_str).unwrap_or("list"), args.get(1).unwrap_or(&Value::Null)).await.map_err(IpcError::plain),
                    "list" => controller.store.list(args.first().unwrap_or(&json!({}))).map_err(IpcError::plain),
                    "get" => controller.store.get(args.first().and_then(Value::as_str).unwrap_or("")).map_err(IpcError::plain),
                    "diagnostics" => Ok(controller.diagnostics()),
                    "sync" => controller.sync().await.map_err(IpcError::plain),
                    "export" => {
                        let csv = args.first().and_then(Value::as_str) == Some("csv");
                        let Some(path) = crate::file_io::pick_path(true, if csv { "riftflow-matches.csv" } else { "riftflow-matches.json" }).await else { return Ok(json!({"cancelled":true})); };
                        let contents = controller.store.export(csv).map_err(IpcError::plain)?;
                        std::fs::write(&path, contents).map_err(IpcError::plain)?;
                        Ok(json!({"cancelled":false,"path":path}))
                    }
                    "restore" => {
                        let Some(path) = crate::file_io::pick_path(false, "riftflow-matches.json").await else { return Ok(json!({"cancelled":true})); };
                        let length = std::fs::metadata(&path).map_err(IpcError::plain)?.len();
                        if length > 64*1024*1024 { return Err(IpcError::plain("Archive exceeds 64 MB")); }
                        let bytes = std::fs::read(&path).map_err(IpcError::plain)?;
                        let document = serde_json::from_slice(&bytes).map_err(IpcError::plain)?;
                        let count = controller.store.restore(&document).map_err(IpcError::plain)?;
                        Ok(json!({"cancelled":false,"count":count}))
                    }
                    _ => unreachable!(),
                }
            }
        }))?;
    }
    ArchiveController::start(controller);
    Ok(())
}
