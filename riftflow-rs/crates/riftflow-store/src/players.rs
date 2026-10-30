//! The `SavedPlayers` / `EncounteredGames` rows behind `saved-player-main`.
//!
//! TypeORM's `manager.find/findOneBy/save/remove` is replaced with the same SQL
//! shapes: the where-clauses are built from the same optional keys, the same
//! defaults apply (`pageSize` 40, order `updateAt desc`), and `save` keeps its
//! update-then-insert behaviour so a partial entity never blanks a column.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Value, json};

use crate::DB_CURRENT_VERSION;

/// `ENCOUNTERED_GAME_QUERY_DEFAULT_PAGE_SIZE`
pub const DEFAULT_PAGE_SIZE: i64 = 40;

const SAVED_PLAYER_COLUMNS: &str =
    "\"puuid\", \"selfPuuid\", \"region\", \"rsoPlatformId\", \"tag\", \"updateAt\", \"lastMetAt\"";
const ENCOUNTERED_GAME_COLUMNS: &str =
    "\"id\", \"gameId\", \"puuid\", \"selfPuuid\", \"region\", \"rsoPlatformId\", \"updateAt\", \"queueType\"";

pub type StoreResult<T> = Result<T, StoreFailure>;

/// Either a validation error the original raises as a plain `Error`, or a real
/// SQLite failure.
#[derive(Debug)]
pub enum StoreFailure {
    Invalid(&'static str),
    Database(rusqlite::Error),
    File(std::io::Error),
    Json(serde_json::Error),
    /// `RiftFlowIpcError` with a code, which the renderer shows verbatim
    Business(String, String),
}

impl StoreFailure {
    pub fn invalid(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}

impl From<rusqlite::Error> for StoreFailure {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<std::io::Error> for StoreFailure {
    fn from(value: std::io::Error) -> Self {
        Self::File(value)
    }
}

impl From<serde_json::Error> for StoreFailure {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl std::fmt::Display for StoreFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreFailure::Invalid(message) => f.write_str(message),
            StoreFailure::Database(error) => write!(f, "sqlite: {error}"),
            StoreFailure::File(error) => write!(f, "file: {error}"),
            StoreFailure::Json(error) => write!(f, "json: {error}"),
            StoreFailure::Business(message, _) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreFailure {}

// ---- readers ---------------------------------------------------------------

pub fn query_saved_player(conn: &Connection, query: &Value) -> StoreResult<Value> {
    let (puuid, self_puuid) = required_pair(query, "puuid, selfPuuid cannot be empty")?;

    let row = conn
        .query_row(
            &format!("SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" WHERE \"puuid\" = ?1 AND \"selfPuuid\" = ?2 LIMIT 1"),
            params![puuid, self_puuid],
            saved_player_from_row,
        )
        .optional()?;

    Ok(row.unwrap_or(Value::Null))
}

pub fn get_player_tags(conn: &Connection, query: &Value) -> StoreResult<Value> {
    let (puuid, self_puuid) = required_pair(query, "puuid, selfPuuid or region cannot be empty")?;

    let mut statement = conn.prepare(&format!(
        "SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" WHERE \"puuid\" = ?1"
    ))?;

    let rows = statement.query_map(params![puuid], saved_player_from_row)?;

    let tags: Vec<Value> = rows
        .filter_map(Result::ok)
        // `.filter(player => player.tag)` drops null and empty tags alike
        .filter(|player| player.get("tag").and_then(Value::as_str).is_some_and(|tag| !tag.is_empty()))
        .map(|mut player| {
            let marked_by_self = player
                .get("selfPuuid")
                .and_then(Value::as_str)
                == Some(self_puuid.as_str());
            if let Some(object) = player.as_object_mut() {
                object.insert("markedBySelf".to_string(), json!(marked_by_self));
            }
            player
        })
        .collect();

    Ok(Value::Array(tags))
}

pub fn get_all_player_tags(conn: &Connection, query: &Value) -> StoreResult<Value> {
    let page = query.get("page").and_then(Value::as_i64).unwrap_or(1);
    let requested = query.get("pageSize").and_then(Value::as_i64).unwrap_or(0);
    let page_size = if requested > 0 { requested } else { 20 };

    let mut where_sql = String::from(" WHERE \"tag\" IS NOT NULL");
    let mut args: Vec<String> = Vec::new();

    for key in ["puuid", "selfPuuid", "region", "rsoPlatformId"] {
        if let Some(value) = query.get(key).and_then(Value::as_str) {
            args.push(value.to_string());
            where_sql.push_str(&format!(" AND \"{key}\" = ?{}", args.len()));
        }
    }

    let order = match query.get("timeOrder").and_then(Value::as_str) {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    args.push(page_size.to_string());
    args.push(((page - 1) * page_size).to_string());
    let (bound, paging) = args.split_at(args.len() - 2);
    let limit_index = bound.len() + 1;

    let mut statement = conn.prepare(&format!(
        "SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\"{where_sql} ORDER BY \"updateAt\" {order} LIMIT ?{limit_index} OFFSET ?{}",
        limit_index + 1
    ))?;
    let rows = statement
        .query_map(row_params(bound, paging), saved_player_from_row)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM \"SavedPlayers\"{where_sql}"),
        row_params(bound, &[]),
        |row| row.get(0),
    )?;

    Ok(json!({
        "data": rows,
        "page": page,
        "pageSize": if requested > 0 { json!(requested) } else { json!(page_size) },
        "total": total,
    }))
}

pub fn query_all_saved_players(conn: &Connection, query: &Value) -> StoreResult<Value> {
    let page = query.get("page").and_then(Value::as_i64).unwrap_or(1);
    let page_size = query.get("pageSize").and_then(Value::as_i64).unwrap_or(0);

    let rows = conn
        .prepare(&format!(
            "SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" LIMIT ?1 OFFSET ?2"
        ))?
        .query_map(params![page_size, (page - 1) * page_size], saved_player_from_row)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM \"SavedPlayers\"", [], |row| row.get(0))?;

    Ok(json!({ "data": rows, "count": count, "page": page, "pageSize": page_size }))
}

pub fn query_encountered_games(conn: &Connection, query: &Value) -> StoreResult<Value> {
    let page = query.get("page").and_then(Value::as_i64).filter(|n| *n > 0).unwrap_or(1);
    let page_size = query
        .get("pageSize")
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_PAGE_SIZE);

    let mut where_sql = String::from(" WHERE \"selfPuuid\" = ?1 AND \"puuid\" = ?2");
    let mut args: Vec<String> = vec![
        text(query, "selfPuuid"),
        text(query, "puuid"),
    ];

    if let Some(queue_type) = query.get("queueType").and_then(Value::as_str) {
        if !queue_type.is_empty() {
            args.push(queue_type.to_string());
            where_sql.push_str(&format!(" AND \"queueType\" = ?{}", args.len()));
        }
    }

    let order = match query.get("timeOrder").and_then(Value::as_str) {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    args.push(page_size.to_string());
    args.push(((page - 1) * page_size).to_string());
    let (bound, paging) = args.split_at(args.len() - 2);
    let limit_index = bound.len() + 1;

    let rows = conn
        .prepare(&format!(
            "SELECT {ENCOUNTERED_GAME_COLUMNS} FROM \"EncounteredGames\"{where_sql} ORDER BY \"updateAt\" {order} LIMIT ?{limit_index} OFFSET ?{}",
            limit_index + 1
        ))?
        .query_map(row_params(bound, paging), encountered_game_from_row)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM \"EncounteredGames\"{where_sql}"),
        row_params(bound, &[]),
        |row| row.get(0),
    )?;

    Ok(json!({ "data": rows, "page": page, "pageSize": page_size, "total": total }))
}

/// `querySavedPlayerWithGames`: the player, its recent games and everyone's tags.
pub fn query_saved_player_with_games(conn: &Connection, query: &Value) -> StoreResult<Value> {
    let player = query_saved_player(conn, query)?;
    if player.is_null() {
        return Ok(Value::Null);
    }

    let games = query_encountered_games(conn, query)?;
    let tags = get_player_tags(conn, query)?;

    let mut merged = player.as_object().cloned().unwrap_or_default();
    merged.insert("encounteredGames".to_string(), games);
    merged.insert("tags".to_string(), tags);
    Ok(Value::Object(merged))
}

// ---- writers ---------------------------------------------------------------

pub fn save_saved_player(conn: &Connection, dto: &Value) -> StoreResult<Value> {
    if text(dto, "puuid").is_empty() || text(dto, "selfPuuid").is_empty() || text(dto, "region").is_empty() {
        return Err(StoreFailure::invalid(
            "puuid, selfPuuid or region cannot be empty",
        ));
    }

    let now = now_iso();
    let puuid = text(dto, "puuid");
    let self_puuid = text(dto, "selfPuuid");
    let region = text(dto, "region");
    let rso_platform_id = text(dto, "rsoPlatformId");

    let existing = conn
        .query_row(
            &format!("SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" WHERE \"puuid\" = ?1 AND \"selfPuuid\" = ?2 AND \"region\" = ?3 AND \"rsoPlatformId\" = ?4"),
            params![puuid, self_puuid, region, rso_platform_id],
            saved_player_from_row,
        )
        .optional()?;

    if let Some(existing) = existing {
        // `save` on an entity with its primary key set updates only the columns
        // the caller actually provided
        let tag = match dto.get("tag") {
            Some(value) if !value.is_null() => Some(value.as_str().unwrap_or_default().to_string()),
            Some(_) => Some(String::new()),
            None => None,
        };
        let tag_value = tag.clone().unwrap_or_default();
        let last_met = if dto.get("encountered").and_then(Value::as_bool).unwrap_or(false) {
            Some(now.clone())
        } else {
            None
        };

        conn.execute(
            "UPDATE \"SavedPlayers\" SET \"updateAt\" = ?1, \
             \"tag\" = CASE WHEN ?2 THEN ?3 ELSE \"tag\" END, \
             \"lastMetAt\" = CASE WHEN ?4 THEN ?5 ELSE \"lastMetAt\" END \
             WHERE \"puuid\" = ?6 AND \"selfPuuid\" = ?7",
            params![
                now,
                tag.is_some(),
                tag_value,
                last_met.is_some(),
                last_met.unwrap_or_default(),
                puuid,
                self_puuid,
            ],
        )?;

        return read_saved_player(conn, &puuid, &self_puuid).map(|row| row.unwrap_or(existing));
    }

    conn.execute(
        &format!("INSERT INTO \"SavedPlayers\" ({SAVED_PLAYER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"),
        params![
            puuid,
            self_puuid,
            region,
            rso_platform_id,
            dto.get("tag").filter(|value| !value.is_null()).and_then(Value::as_str),
            now,
            dto.get("encountered").and_then(Value::as_bool).unwrap_or(false).then(|| now.clone()),
        ],
    )?;

    read_saved_player(conn, &puuid, &self_puuid).map(|row| row.unwrap_or(Value::Null))
}

pub fn delete_saved_player(conn: &Connection, query: &Value) -> StoreResult<Value> {
    if text(query, "puuid").is_empty() || text(query, "selfPuuid").is_empty() {
        return Err(StoreFailure::invalid(
            "puuid, selfPuuid or region cannot be empty",
        ));
    }

    // `remove(SavedPlayer, query)` matches on whichever keys the dto carries and
    // answers with the rows it deleted
    let mut where_sql = String::from(" WHERE \"puuid\" = ?1 AND \"selfPuuid\" = ?2");
    let mut args: Vec<String> = vec![text(query, "puuid"), text(query, "selfPuuid")];
    for key in ["region", "rsoPlatformId"] {
        if let Some(value) = query.get(key).and_then(Value::as_str) {
            args.push(value.to_string());
            where_sql.push_str(&format!(" AND \"{key}\" = ?{}", args.len()));
        }
    }

    let rows = conn
        .prepare(&format!("SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\"{where_sql}"))?
        .query_map(row_params(&args, &[]), saved_player_from_row)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    conn.execute(
        &format!("DELETE FROM \"SavedPlayers\"{where_sql}"),
        row_params(&args, &[]),
    )?;

    Ok(Value::Array(rows))
}

pub fn update_player_tag(conn: &Connection, dto: &Value) -> StoreResult<Value> {
    let (puuid, self_puuid) = required_pair(dto, "puuid, selfPuuid cannot be empty")?;
    let now = now_iso();
    let tag = dto.get("tag").and_then(Value::as_str);

    let existing = conn
        .query_row(
            &format!("SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" WHERE \"puuid\" = ?1 AND \"selfPuuid\" = ?2 LIMIT 1"),
            params![puuid, self_puuid],
            saved_player_from_row,
        )
        .optional()?;

    if existing.is_some() {
        conn.execute(
            "UPDATE \"SavedPlayers\" SET \"tag\" = ?1, \"updateAt\" = ?2 WHERE \"puuid\" = ?3 AND \"selfPuuid\" = ?4",
            params![tag, now, puuid, self_puuid],
        )?;
        return read_saved_player(conn, &puuid, &self_puuid).map(|row| row.unwrap_or(Value::Null));
    }

    let region = dto.get("region").and_then(Value::as_str);
    let rso_platform_id = dto.get("rsoPlatformId").and_then(Value::as_str);
    if region.is_none() || rso_platform_id.is_none() {
        return Err(StoreFailure::invalid(
            "When creating tag, rsoPlatformId, region cannot be empty",
        ));
    }

    conn.execute(
        &format!("INSERT INTO \"SavedPlayers\" ({SAVED_PLAYER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"),
        params![puuid, self_puuid, region.unwrap_or_default(), rso_platform_id.unwrap_or_default(), tag, now, None::<String>],
    )?;

    read_saved_player(conn, &puuid, &self_puuid).map(|row| row.unwrap_or(Value::Null))
}

pub fn save_encountered_game(conn: &Connection, dto: &Value) -> StoreResult<Value> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO \"EncounteredGames\" (\"gameId\", \"puuid\", \"selfPuuid\", \"region\", \"rsoPlatformId\", \"updateAt\", \"queueType\") \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            dto.get("gameId").and_then(Value::as_i64).unwrap_or_default(),
            text(dto, "puuid"),
            text(dto, "selfPuuid"),
            text(dto, "region"),
            text(dto, "rsoPlatformId"),
            now,
            dto.get("queueType").and_then(Value::as_str).unwrap_or_default(),
        ],
    )?;

    let id = conn.last_insert_rowid();
    conn.query_row(
        &format!("SELECT {ENCOUNTERED_GAME_COLUMNS} FROM \"EncounteredGames\" WHERE \"id\" = ?1"),
        params![id],
        encountered_game_from_row,
    )
    .map_err(StoreFailure::from)
}

pub fn delete_encountered_game(conn: &Connection, record_id: i64) -> StoreResult<Value> {
    let affected = conn.execute("DELETE FROM \"EncounteredGames\" WHERE \"id\" = ?1", params![record_id])?;
    Ok(json!({ "affected": affected }))
}

// ---- tagged-players file service ------------------------------------------

/// `TaggedPlayersFileService.writeToJsonFile`
pub fn export_tagged_players(conn: &Connection, path: &Path) -> StoreResult<Value> {
    let rows = conn
        .prepare(&format!(
            "SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" WHERE \"tag\" IS NOT NULL"
        ))?
        .query_map([], saved_player_from_row)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let data: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "puuid": row.get("puuid"),
                "selfPuuid": row.get("selfPuuid"),
                "region": row.get("region"),
                "rsoPlatformId": row.get("rsoPlatformId"),
                "tag": row.get("tag"),
            })
        })
        .collect();

    let content = json!({
        "databaseVersion": DB_CURRENT_VERSION,
        "type": "riftflow-tagged-players",
        "data": data,
    });

    std::fs::write(path, content.to_string())?;
    Ok(json!(path.display().to_string()))
}

/// `TaggedPlayersFileService.readFromJsonFile`: the three validation errors and
/// the batched upsert, in a transaction.
pub fn import_tagged_players(conn: &Connection, path: &Path) -> StoreResult<Value> {
    let raw = std::fs::read_to_string(path)?;
    let content: Value = serde_json::from_str(&raw)?;

    if content.get("type").and_then(Value::as_str) != Some("riftflow-tagged-players") {
        return Err(StoreFailure::Business(
            "The file is not a valid tagged players file".to_string(),
            "InvalidTaggedPlayersFile".to_string(),
        ));
    }

    if content
        .get("databaseVersion")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        > DB_CURRENT_VERSION
    {
        return Err(StoreFailure::Business(
            "The file is from a newer version of the application, please update the application first"
                .to_string(),
            "InvalidDatabaseVersion".to_string(),
        ));
    }

    let rows = content.get("data").and_then(Value::as_array).cloned().unwrap_or_default();

    let valid = content.get("data").map(Value::is_array).unwrap_or(false)
        && rows.iter().all(|record| {
            record.is_object()
                && record.get("puuid").and_then(Value::as_str).is_some()
                && record.get("selfPuuid").and_then(Value::as_str).is_some()
                && record.get("region").and_then(Value::as_str).is_some()
                && record.get("rsoPlatformId").and_then(Value::as_str).is_some()
                && record.get("tag").and_then(Value::as_str).is_some()
        });

    if !valid {
        return Err(StoreFailure::Business(
            "The file is not a valid tagged players file".to_string(),
            "InvalidTaggedPlayersData".to_string(),
        ));
    }

    let now = now_iso();
    let transaction = conn.unchecked_transaction()?;
    {
        let mut statement = transaction.prepare(
            &format!("INSERT INTO \"SavedPlayers\" ({SAVED_PLAYER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL) \
                      ON CONFLICT(\"puuid\", \"selfPuuid\", \"region\", \"rsoPlatformId\") DO UPDATE SET \"tag\" = excluded.\"tag\", \"updateAt\" = excluded.\"updateAt\""),
        )?;

        for record in &rows {
            statement.execute(params![
                record.get("puuid").and_then(Value::as_str).unwrap_or_default(),
                record.get("selfPuuid").and_then(Value::as_str).unwrap_or_default(),
                record.get("region").and_then(Value::as_str).unwrap_or_default(),
                record.get("rsoPlatformId").and_then(Value::as_str).unwrap_or_default(),
                record.get("tag").and_then(Value::as_str),
                now,
            ])?;
        }
    }
    transaction.commit()?;

    Ok(json!(path.display().to_string()))
}

// ---- settings file service -------------------------------------------------

/// `SettingsJsonFileService.writeSettingsToJsonFile`: every `Settings` row.
pub fn export_settings(conn: &Connection, path: &Path) -> StoreResult<Value> {
    let rows = conn
        .prepare("SELECT \"key\", \"value\" FROM \"Settings\"")?
        .query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok(json!({
                "key": key,
                "value": serde_json::from_str::<Value>(&value).unwrap_or(Value::String(value)),
            }))
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let content = json!({
        "databaseVersion": DB_CURRENT_VERSION,
        "type": "riftflow-settings",
        "data": rows,
    });

    std::fs::write(path, content.to_string())?;
    Ok(json!(path.display().to_string()))
}

/// `SettingsJsonFileService.readSettingsFromJsonFile`. The caller restarts the
/// app afterwards, exactly like `shared.global.restart()`.
pub fn import_settings(conn: &Connection, path: &Path) -> StoreResult<usize> {
    let raw = std::fs::read_to_string(path)?;
    let content: Value = serde_json::from_str(&raw)?;

    if content.get("type").and_then(Value::as_str) != Some("riftflow-settings") {
        return Err(StoreFailure::Business(
            "The file is not a valid settings file".to_string(),
            "InvalidSettingsFile".to_string(),
        ));
    }

    if content
        .get("databaseVersion")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        > DB_CURRENT_VERSION
    {
        return Err(StoreFailure::Business(
            "The file is from a newer version of the application, please update the application first"
                .to_string(),
            "InvalidDatabaseVersion".to_string(),
        ));
    }

    let rows = content.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let valid = content.get("data").map(Value::is_array).unwrap_or(false)
        && rows.iter().all(|record| {
            record.is_object()
                && record.get("key").and_then(Value::as_str).is_some()
                && record.get("value").is_some()
        });

    if !valid {
        return Err(StoreFailure::Business(
            "The file is not a valid settings file".to_string(),
            "InvalidSettingsData".to_string(),
        ));
    }

    let transaction = conn.unchecked_transaction()?;
    let mut written = 0usize;
    for record in &rows {
        let key = record.get("key").and_then(Value::as_str).unwrap_or_default();
        let value = record.get("value").cloned().unwrap_or(Value::Null);
        written += transaction.execute(
            "INSERT INTO \"Settings\" (\"key\", \"value\") VALUES (?1, ?2) ON CONFLICT(\"key\") DO UPDATE SET \"value\" = excluded.\"value\"",
            params![key, value.to_string()],
        )?;
    }
    transaction.commit()?;

    Ok(written)
}

// ---- helpers ---------------------------------------------------------------

fn read_saved_player(conn: &Connection, puuid: &str, self_puuid: &str) -> StoreResult<Option<Value>> {
    conn.query_row(
        &format!("SELECT {SAVED_PLAYER_COLUMNS} FROM \"SavedPlayers\" WHERE \"puuid\" = ?1 AND \"selfPuuid\" = ?2 LIMIT 1"),
        params![puuid, self_puuid],
        saved_player_from_row,
    )
    .optional()
    .map_err(StoreFailure::from)
}

fn required_pair<'a>(query: &'a Value, message: &'static str) -> StoreResult<(String, String)> {
    let puuid = text(query, "puuid");
    let self_puuid = text(query, "selfPuuid");

    if puuid.is_empty() || self_puuid.is_empty() {
        return Err(StoreFailure::invalid(message));
    }

    Ok((puuid, self_puuid))
}

/// `params_from_iter` over the where-clause values plus the LIMIT/OFFSET pair.
fn row_params<'a>(bound: &'a [String], paging: &'a [String]) -> impl rusqlite::Params + 'a {
    let mut all: Vec<&'a str> = bound.iter().map(String::as_str).collect();
    all.extend(paging.iter().map(String::as_str));
    rusqlite::params_from_iter(all)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn saved_player_from_row(row: &Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "puuid": row.get::<_, Option<String>>(0)?.unwrap_or_default(),
        "selfPuuid": row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        "region": row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        "rsoPlatformId": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        "tag": row.get::<_, Option<String>>(4)?,
        "updateAt": row.get::<_, Option<String>>(5)?,
        "lastMetAt": row.get::<_, Option<String>>(6)?,
    }))
}

fn encountered_game_from_row(row: &Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "gameId": row.get::<_, i64>(1)?,
        "puuid": row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        "selfPuuid": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        "region": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        "rsoPlatformId": row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        "updateAt": row.get::<_, Option<String>>(6)?,
        "queueType": row.get::<_, Option<String>>(7)?.unwrap_or_default(),
    }))
}

/// TypeORM's sqlite driver writes `datetime` columns as ISO-8601 strings, so that
/// is what lands here too.
pub fn now_iso() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default();

    format_millis(millis)
}

fn format_millis(millis: i64) -> String {
    let seconds = millis.div_euclid(1000);
    let fraction = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{fraction:03}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// Howard Hinnant's `civil_from_days`: the date a POSIX day number maps to.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };

    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // the same shape `upgrade_to_10` + the queueType migration produce
        conn.execute_batch(
            r#"CREATE TABLE "SavedPlayers" (
                 "puuid" varchar NOT NULL, "selfPuuid" varchar NOT NULL,
                 "region" varchar NOT NULL, "rsoPlatformId" varchar NOT NULL,
                 "tag" varchar, "updateAt" datetime NOT NULL, "lastMetAt" datetime,
                 PRIMARY KEY ("puuid", "selfPuuid", "region", "rsoPlatformId"));
               CREATE TABLE "EncounteredGames" (
                 "id" integer PRIMARY KEY AUTOINCREMENT NOT NULL, "gameId" integer NOT NULL,
                 "puuid" varchar NOT NULL, "selfPuuid" varchar NOT NULL, "region" varchar NOT NULL,
                 "rsoPlatformId" varchar NOT NULL, "updateAt" datetime NOT NULL,
                 "queueType" varchar NOT NULL DEFAULT '');
               CREATE TABLE "Settings" ("key" varchar PRIMARY KEY NOT NULL, "value" json NOT NULL);"#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn iso_timestamps_match_the_driver_shape() {
        // 2001-09-09T01:46:40.000Z, the classic epoch sample
        assert_eq!(format_millis(999_999_999_999), "2001-09-09T01:46:39.999Z");
        assert_eq!(format_millis(1_735_689_600_000), "2025-01-01T00:00:00.000Z");
        // before 1970 still counts backwards correctly
        assert_eq!(format_millis(-86_400_000), "1969-12-31T00:00:00.000Z");
    }

    #[test]
    fn saving_a_known_player_keeps_the_columns_the_dto_omits() {
        let conn = database();
        let saved = save_saved_player(
            &conn,
            &json!({ "puuid": "p1", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1",
                     "tag": "care", "encountered": true }),
        )
        .unwrap();
        assert_eq!(saved["tag"], json!("care"));
        assert!(saved["lastMetAt"].is_string());

        // a later save without `tag` must not clear it, but does refresh updateAt
        let again = save_saved_player(
            &conn,
            &json!({ "puuid": "p1", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1",
                     "encountered": false }),
        )
        .unwrap();
        assert_eq!(again["tag"], json!("care"));
        assert_eq!(again["lastMetAt"], saved["lastMetAt"]);
    }

    #[test]
    fn validation_errors_match_the_original_messages() {
        let conn = database();
        let error = query_saved_player(&conn, &json!({ "puuid": "p1" })).unwrap_err();
        assert_eq!(error.to_string(), "puuid, selfPuuid cannot be empty");

        let error = save_saved_player(&conn, &json!({ "puuid": "p1", "selfPuuid": "me" })).unwrap_err();
        assert_eq!(error.to_string(), "puuid, selfPuuid or region cannot be empty");

        let error = update_player_tag(&conn, &json!({ "puuid": "x", "selfPuuid": "y", "tag": "t" })).unwrap_err();
        assert_eq!(error.to_string(), "When creating tag, rsoPlatformId, region cannot be empty");
    }

    #[test]
    fn tags_are_filtered_paged_and_attributed() {
        let conn = database();
        for (index, tag) in [("a", Some("good")), ("b", None), ("c", Some("bad"))] {
            save_saved_player(
                &conn,
                &json!({ "puuid": format!("p{index}"), "selfPuuid": if index == "c" { "me" } else { "other" },
                         "region": "TENCENT", "rsoPlatformId": "HN1", "tag": tag, "encountered": true }),
            )
            .unwrap();
        }

        let all = get_all_player_tags(&conn, &json!({ "page": 1, "pageSize": 10 })).unwrap();
        assert_eq!(all["total"], json!(2), "未标记的不计入");
        assert_eq!(all["data"].as_array().unwrap().len(), 2);

        let mine = get_player_tags(&conn, &json!({ "puuid": "pc", "selfPuuid": "me" })).unwrap();
        assert_eq!(mine[0]["markedBySelf"], json!(true));

        let others = get_player_tags(&conn, &json!({ "puuid": "pa", "selfPuuid": "me" })).unwrap();
        assert_eq!(others[0]["markedBySelf"], json!(false));
    }

    #[test]
    fn encountered_games_page_and_filter_by_queue() {
        let conn = database();
        for queue in ["RANKED_SOLO_5x5", "ARAM", "ARAM"] {
            save_encountered_game(
                &conn,
                &json!({ "gameId": 1, "puuid": "p1", "selfPuuid": "me", "region": "TENCENT",
                         "rsoPlatformId": "HN1", "queueType": queue }),
            )
            .unwrap();
        }

        let all = query_encountered_games(&conn, &json!({ "puuid": "p1", "selfPuuid": "me" })).unwrap();
        assert_eq!(all["total"], json!(3));
        assert_eq!(all["pageSize"], json!(DEFAULT_PAGE_SIZE));

        let aram = query_encountered_games(
            &conn,
            &json!({ "puuid": "p1", "selfPuuid": "me", "queueType": "ARAM", "pageSize": 1, "timeOrder": "asc" }),
        )
        .unwrap();
        assert_eq!(aram["total"], json!(2));
        assert_eq!(aram["data"].as_array().unwrap().len(), 1);
        assert_eq!(aram["data"][0]["queueType"], json!("ARAM"));

        let deleted = delete_encountered_game(&conn, aram["data"][0]["id"].as_i64().unwrap()).unwrap();
        assert_eq!(deleted["affected"], json!(1));
    }

    #[test]
    fn deleting_a_player_returns_the_removed_rows() {
        let conn = database();
        save_saved_player(
            &conn,
            &json!({ "puuid": "p1", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1", "tag": "x" }),
        )
        .unwrap();

        let removed = delete_saved_player(&conn, &json!({ "puuid": "p1", "selfPuuid": "me" })).unwrap();
        assert_eq!(removed.as_array().unwrap().len(), 1);
        assert!(query_saved_player(&conn, &json!({ "puuid": "p1", "selfPuuid": "me" })).unwrap().is_null());
    }

    #[test]
    fn with_games_nests_the_lists_the_player_card_reads() {
        let conn = database();
        save_saved_player(
            &conn,
            &json!({ "puuid": "p1", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1", "tag": "x" }),
        )
        .unwrap();
        save_encountered_game(
            &conn,
            &json!({ "gameId": 7, "puuid": "p1", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1" }),
        )
        .unwrap();

        let merged = query_saved_player_with_games(&conn, &json!({ "puuid": "p1", "selfPuuid": "me" })).unwrap();
        assert_eq!(merged["tag"], json!("x"));
        assert_eq!(merged["encounteredGames"]["total"], json!(1));
        assert_eq!(merged["tags"].as_array().unwrap().len(), 1);

        save_saved_player(&conn, &json!({ "puuid": "p2", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1" })).unwrap();
        let missing = query_saved_player_with_games(&conn, &json!({ "puuid": "nope", "selfPuuid": "me" })).unwrap();
        assert!(missing.is_null());
    }

    #[test]
    fn tagged_players_round_trip_through_a_file() {
        let conn = database();
        save_saved_player(
            &conn,
            &json!({ "puuid": "p1", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1", "tag": "keep" }),
        )
        .unwrap();
        save_saved_player(
            &conn,
            &json!({ "puuid": "p2", "selfPuuid": "me", "region": "TENCENT", "rsoPlatformId": "HN1" }),
        )
        .unwrap();

        let path = std::env::temp_dir().join("riftflow-tagged-roundtrip.json");
        export_tagged_players(&conn, &path).unwrap();
        let content: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["type"], json!("riftflow-tagged-players"));
        assert_eq!(content["databaseVersion"], json!(DB_CURRENT_VERSION));
        assert_eq!(content["data"].as_array().unwrap().len(), 1, "只导出有 tag 的");

        let other = database();
        import_tagged_players(&other, &path).unwrap();
        let rows = get_all_player_tags(&other, &json!({ "page": 1, "pageSize": 10 })).unwrap();
        assert_eq!(rows["data"][0]["tag"], json!("keep"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_bad_file_is_rejected_with_the_original_codes() {
        let conn = database();
        let path = std::env::temp_dir().join("riftflow-tagged-bad.json");

        std::fs::write(&path, r#"{"type":"nope","data":[]}"#).unwrap();
        let failure = import_tagged_players(&conn, &path).unwrap_err();
        assert!(matches!(failure, StoreFailure::Business(_, code) if code == "InvalidTaggedPlayersFile"));

        std::fs::write(
            &path,
            format!(r#"{{"type":"riftflow-tagged-players","databaseVersion":{},"data":[]}}"#, DB_CURRENT_VERSION + 1),
        )
        .unwrap();
        let failure = import_tagged_players(&conn, &path).unwrap_err();
        assert!(matches!(failure, StoreFailure::Business(_, code) if code == "InvalidDatabaseVersion"));

        std::fs::write(
            &path,
            r#"{"type":"riftflow-tagged-players","databaseVersion":1,"data":[{"puuid":"p"}]}"#,
        )
        .unwrap();
        let failure = import_tagged_players(&conn, &path).unwrap_err();
        assert!(matches!(failure, StoreFailure::Business(_, code) if code == "InvalidTaggedPlayersData"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn settings_export_import_upserts_by_key() {
        let conn = database();
        conn.execute(
            "INSERT INTO \"Settings\" (\"key\", \"value\") VALUES (?1, ?2)",
            params!["auto-gameflow-main:autoAcceptEnabled", "true"],
        )
        .unwrap();

        let path = std::env::temp_dir().join("riftflow-settings-roundtrip.json");
        export_settings(&conn, &path).unwrap();
        let content: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["type"], json!("riftflow-settings"));
        assert_eq!(content["data"][0]["key"], json!("auto-gameflow-main:autoAcceptEnabled"));
        assert_eq!(content["data"][0]["value"], json!(true));

        let other = database();
        other
            .execute("INSERT INTO \"Settings\" (\"key\", \"value\") VALUES (?1, ?2)", params!("k", "1"))
            .unwrap();
        let written = import_settings(&other, &path).unwrap();
        assert_eq!(written, 1);
        let value: String = other
            .query_row("SELECT \"value\" FROM \"Settings\" WHERE \"key\" = ?1", params!["auto-gameflow-main:autoAcceptEnabled"], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "true");

        std::fs::remove_file(&path).ok();
    }

}
