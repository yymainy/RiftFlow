//! `saved-player-main` — 收藏的玩家、遇到的对局与玩家标签。
//!
//! Ports `saved-player/index.ts`, `ipc-handlers.ts` and
//! `tagged-players-file-service.ts`. The SQL lives in `riftflow-store::players`;
//! this module is the wire: argument shapes, error messages and the two file
//! dialogs Electron answered through `dialog.show*Dialog`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use riftflow_ipc::{RiftFlowIpcMain, IpcError, IpcResult};
use riftflow_store::players::{self, StoreFailure};
use riftflow_store::settings::SettingFactory;
use riftflow_store::Database;

pub const NAMESPACE: &str = "saved-player-main";

type Db = Arc<Mutex<Database>>;

pub fn register(ipc: &Arc<RiftFlowIpcMain>, settings: &Arc<SettingFactory>) -> anyhow::Result<()> {
    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "querySavedPlayer", query(db, players::query_saved_player))?;

    let db = settings.db().clone();
    ipc.on_call(
        NAMESPACE,
        "querySavedPlayerWithGames",
        query(db, players::query_saved_player_with_games),
    )?;

    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "saveSavedPlayer", query(db, players::save_saved_player))?;

    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "deleteSavedPlayer", query(db, players::delete_saved_player))?;

    let db = settings.db().clone();
    ipc.on_call(
        NAMESPACE,
        "queryEncounteredGames",
        query(db, players::query_encountered_games),
    )?;

    let db = settings.db().clone();
    ipc.on_call(
        NAMESPACE,
        "saveEncounteredGame",
        query(db, players::save_encountered_game),
    )?;

    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "queryAllSavedPlayers", query(db, players::query_all_saved_players))?;

    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "getPlayerTags", query(db, players::get_player_tags))?;

    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "getAllPlayerTags", query(db, players::get_all_player_tags))?;

    let db = settings.db().clone();
    ipc.on_call(NAMESPACE, "updatePlayerTag", query(db, players::update_player_tag))?;

    let db = settings.db().clone();
    ipc.on_call(
        NAMESPACE,
        "deleteEncounteredGame",
        riftflow_ipc::handler(move |_ctx, args| {
            let db = db.clone();
            async move {
                let record_id = args.first().and_then(Value::as_i64).unwrap_or_default();
                read(&db, move |conn| {
                    players::delete_encountered_game(conn, record_id)
                })
            }
        }),
    )?;

    let db = settings.db().clone();
    ipc.on_call(
        NAMESPACE,
        "exportTaggedPlayersToJsonFile",
        riftflow_ipc::handler(move |_ctx, _args| {
            let db = db.clone();
            async move {
                let Some(path) = crate::file_io::pick_path(true, "riftflow-tagged-players.json").await else {
                    return Ok(Value::Null);
                };

                write(&db, move |conn| players::export_tagged_players(conn, &path)).await
            }
        }),
    )?;

    let db = settings.db().clone();
    ipc.on_call(
        NAMESPACE,
        "importTaggedPlayersFromJsonFile",
        riftflow_ipc::handler(move |_ctx, _args| {
            let db = db.clone();
            async move {
                let Some(path) = crate::file_io::pick_path(false, "riftflow-tagged-players.json").await else {
                    return Ok(Value::Null);
                };

                write(&db, move |conn| players::import_tagged_players(conn, &path)).await
            }
        }),
    )?;

    Ok(())
}

/// The ten handlers whose whole argument is the query/save dto.
fn query<T, F>(db: Db, body: F) -> riftflow_ipc::CallHandler
where
    T: serde::Serialize,
    F: Fn(&rusqlite::Connection, &Value) -> Result<T, StoreFailure> + Send + Sync + Copy + 'static,
{
    riftflow_ipc::handler(move |_ctx, args| {
        let db = db.clone();
        async move {
            let query = args.first().cloned().unwrap_or(Value::Null);
            read(&db, move |conn| body(conn, &query))
        }
    })
}

fn read<T, F>(db: &Db, body: F) -> IpcResult<Value>
where
    T: serde::Serialize,
    F: FnOnce(&rusqlite::Connection) -> Result<T, StoreFailure>,
{
    let guard = db.lock().unwrap();

    body(guard.connection())
        .map(|value| serde_json::to_value(value).map_err(IpcError::plain))
        .unwrap_or_else(|error| Err(to_ipc_error(error)))
}

/// The file-dialog handlers: the shell dialog and the write both block, so they
/// run off the async worker.
async fn write<T, F>(db: &Db, body: F) -> IpcResult<Value>
where
    T: serde::Serialize + Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T, StoreFailure> + Send + 'static,
{
    let db = db.clone();
    tokio::task::spawn_blocking(move || read(&db, body))
        .await
        .map_err(IpcError::plain)?
}

/// `SettingsJsonFileService.writeSettingsToJsonFile`, for `setting-factory-main`.
pub async fn export_settings(db: &Db, path: PathBuf) -> IpcResult<Value> {
    write(db, move |conn| players::export_settings(conn, &path)).await
}

/// `SettingsJsonFileService.readSettingsFromJsonFile`, for `setting-factory-main`.
pub async fn import_settings(db: &Db, path: PathBuf) -> IpcResult<Value> {
    let path_for_reply = path.clone();
    write(db, move |conn| {
        players::import_settings(conn, &path).map(|_| Value::String(path_for_reply.display().to_string()))
    })
    .await
}

fn to_ipc_error(error: StoreFailure) -> IpcError {
    match error {
        StoreFailure::Business(message, code) => IpcError::business(message, code),
        other => IpcError::plain(other),
    }
}
