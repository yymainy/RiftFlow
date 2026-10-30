//! Port spec for `src/main/shards/storage` + `src/main/shards/setting-factory`.
//! The affinity test exists because a live `RiftFlow.db` proved TypeORM's
//! `json` columns coerce numeric payloads into INTEGER cells.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riftflow_store::settings::{
    SETTINGS_SAVE_DELAY_MS, boolean, number, nullable_string, prefix_bounds, string, string_enum,
    SettingDeclaration, SettingFactory,
};
use riftflow_store::{Database, StorageState, DB_CURRENT_VERSION};
use serde_json::{json, Value};

struct TempDb {
    dir: PathBuf,
    db_path: PathBuf,
}

impl TempDb {
    fn new(tag: &str) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("riftflow-store-{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("RiftFlow.db");
        Self { dir, db_path }
    }

    /// Opens, runs the version ladder, then hands out a shared handle.
    fn initialized(&self) -> (Arc<Mutex<Database>>, StorageState) {
        let (db, state) = Database::initialize(&self.db_path).unwrap();
        db.close();
        (Arc::new(Mutex::new(Database::open(&self.db_path).unwrap())), state)
    }

    fn connection(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.db_path).unwrap()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn decl(key: &str, default: Value, schema: riftflow_store::settings::Validator) -> SettingDeclaration {
    SettingDeclaration {
        key: key.to_string(),
        default,
        schema: Some(schema),
        transform: None,
        side_effect: None,
    }
}

#[test]
fn fresh_database_creates_all_four_tables_and_reaches_version_15() {
    let temp = TempDb::new("fresh");
    let (db, state) = temp.initialized();

    let names = db.lock().unwrap().table_names().unwrap();
    for expected in ["Metadata", "Settings", "SavedPlayers", "EncounteredGames"] {
        assert!(names.contains(&expected.to_string()), "{names:?}");
    }

    assert_eq!(
        db.lock().unwrap().metadata_get("version").unwrap(),
        Some(json!(DB_CURRENT_VERSION))
    );
    assert!(!state.using_higher_version_db);

    assert_eq!(
        db.lock().unwrap().metadata_get("debugging-text-1").unwrap(),
        Some(json!("Ayano"))
    );
}

#[test]
fn numeric_json_cells_are_read_back_as_json_numbers() {
    let temp = TempDb::new("affinity");
    let (db, _state) = temp.initialized();

    // Exactly what TypeORM does with a `value json` column and a numeric
    // payload: SQLite NUMERIC affinity turns the text '0' into an INTEGER cell.
    temp.connection()
        .execute(
            "INSERT OR REPLACE INTO \"Settings\" (key, value) VALUES (?1, '0')",
            ["auto-gameflow-main/autoAcceptDelaySeconds"],
        )
        .unwrap();

    let stored_type: String = temp
        .connection()
        .query_row(
            "SELECT typeof(value) FROM \"Settings\" WHERE key = 'auto-gameflow-main/autoAcceptDelaySeconds'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored_type, "integer", "json column coerces numbers");

    let factory = SettingFactory::new(db);
    let service = factory
        .register(
            "auto-gameflow-main",
            vec![decl(
                "autoAcceptDelaySeconds",
                json!(0),
                number(),
            )],
        )
        .unwrap();
    service.apply_to_state().unwrap();

    assert_eq!(service.get("autoAcceptDelaySeconds"), json!(0));
    assert_eq!(
        factory
            .get_setting("auto-gameflow-main", "autoAcceptDelaySeconds")
            .unwrap(),
        json!(0)
    );
}

#[test]
fn version_above_current_is_read_and_only_flags_state() {
    let temp = TempDb::new("higher");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db.clone());
    db.lock()
        .unwrap()
        .metadata_set("version", &json!(99))
        .unwrap();
    factory
        .save_raw("statistics-main", "alreadyCounted", &json!(["1.5.0"]))
        .unwrap();
    drop(db);

    // reopening must neither migrate nor lose rows
    let (db2, state2) = {
        let (db, state) = Database::initialize(&temp.db_path).unwrap();
        db.close();
        (
            Arc::new(Mutex::new(Database::open(&temp.db_path).unwrap())),
            state,
        )
    };

    assert!(state2.using_higher_version_db);
    assert_eq!(
        db2.lock().unwrap().metadata_get("version").unwrap(),
        Some(json!(99))
    );

    let factory2 = SettingFactory::new(db2);
    assert_eq!(
        factory2
            .get_raw("statistics-main", "alreadyCounted")
            .unwrap()
            .unwrap(),
        json!(["1.5.0"]),
        "existing rows survive a higher-version open"
    );
}

#[tokio::test]
async fn settings_writes_are_debounced_and_only_land_on_flush() {
    let temp = TempDb::new("debounce");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    let service = factory
        .register(
            "app-common-main",
            vec![
                decl("streamerMode", json!(false), boolean()),
                decl("locale", json!("zh-CN"), string()),
            ],
        )
        .unwrap();

    service.apply_to_state().unwrap();
    assert_eq!(service.get("streamerMode"), json!(false));

    service.set("streamerMode", json!(true)).await.unwrap();
    assert_eq!(service.get("streamerMode"), json!(true), "pending overlay");
    assert_eq!(service.pending_count(), 1);
    assert_eq!(
        factory
            .get_raw("app-common-main", "streamerMode")
            .unwrap(),
        None,
        "not written before flush"
    );

    assert_eq!(service.flush().unwrap(), 1);
    assert_eq!(
        factory
            .get_raw("app-common-main", "streamerMode")
            .unwrap()
            .unwrap(),
        json!(true)
    );

    let keys: Vec<String> = factory
        .get_by_prefix("app-common-main", "")
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(keys, vec!["app-common-main/streamerMode".to_string()]);
}

#[tokio::test]
async fn setting_validation_failure_falls_back_to_default() {
    let temp = TempDb::new("validate");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    let service = factory
        .register(
            "auto-gameflow-main",
            vec![decl(
                "autoHonorStrategy",
                json!("prefer-lobby-member"),
                string_enum(&["prefer-lobby-member", "only-lobby-member", "all-member"]),
            )],
        )
        .unwrap();
    service.apply_to_state().unwrap();

    service
        .set("autoHonorStrategy", json!("not-a-real-strategy"))
        .await
        .unwrap();
    assert_eq!(service.get("autoHonorStrategy"), json!("prefer-lobby-member"));
}

#[test]
fn invalid_declared_default_is_rejected_at_register() {
    let temp = TempDb::new("bad-default");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    let registered = factory.register(
        "game-client-main",
        vec![decl("terminateShortcut", json!(42), nullable_string())],
    );

    let err = match registered {
        Err(error) => error.to_string(),
        Ok(_) => panic!("an invalid declared default must be rejected"),
    };

    assert!(
        err.starts_with("Invalid default value for setting game-client-main/terminateShortcut"),
        "{err}"
    );
}

#[test]
fn duplicate_namespace_is_rejected() {
    let temp = TempDb::new("dup-ns");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    factory
        .register("tray-main", vec![decl("a", json!(true), boolean())])
        .unwrap();

    let duplicate = factory.register("tray-main", vec![]);
    let err = match duplicate {
        Err(error) => error.to_string(),
        Ok(_) => panic!("a duplicate namespace must be rejected"),
    };
    assert_eq!(err, "namespace tray-main already created");
}

#[tokio::test]
async fn side_effect_failure_aborts_the_commit() {
    let temp = TempDb::new("side-effect");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    let service = factory
        .register(
            "game-client-main",
            vec![SettingDeclaration {
                key: "terminateShortcut".to_string(),
                default: Value::Null,
                schema: Some(nullable_string()),
                transform: None,
                side_effect: Some(Arc::new(|_ctx| {
                    Box::pin(async { Err("shortcut registration failed".to_string()) })
                })),
            }],
        )
        .unwrap();

    let err = service
        .set("terminateShortcut", json!("Ctrl+Shift+F4"))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(err, "shortcut registration failed");
    assert_eq!(service.get("terminateShortcut"), Value::Null, "not committed");
    assert_eq!(service.pending_count(), 0, "nothing scheduled");
}

#[tokio::test]
async fn null_value_is_scheduled_as_a_delete() {
    let temp = TempDb::new("null-delete");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    let service = factory
        .register(
            "self-update-main",
            vec![decl("ignoreVersion", Value::Null, nullable_string())],
        )
        .unwrap();

    service.set("ignoreVersion", json!("1.5.0")).await.unwrap();
    service.flush().unwrap();
    assert_eq!(
        factory
            .get_raw("self-update-main", "ignoreVersion")
            .unwrap()
            .unwrap(),
        json!("1.5.0")
    );

    service.set("ignoreVersion", Value::Null).await.unwrap();
    service.flush().unwrap();
    assert_eq!(
        factory
            .get_raw("self-update-main", "ignoreVersion")
            .unwrap(),
        None
    );
}

#[test]
fn prefix_bounds_reproduce_the_original_upper_bound() {
    // lower = "ns/prefix/", upper = "ns/prefix0" ('0' is the byte after '/')
    let (lower, upper) = prefix_bounds("ongoing-game-main", "champSelectHandoffSnapshot");
    assert_eq!(lower, "ongoing-game-main/champSelectHandoffSnapshot/");
    assert_eq!(upper, "ongoing-game-main/champSelectHandoffSnapshot0");

    let (lower, upper) = prefix_bounds("window-manager-main", "");
    assert_eq!(lower, "window-manager-main/");
    assert_eq!(upper, "window-manager-main0");

    // a trailing slash in the caller's prefix is stripped first
    let (lower, _) = prefix_bounds("window-manager-main/main-window", "bounds/");
    assert_eq!(lower, "window-manager-main/main-window/bounds/");
}

#[test]
fn remove_by_prefix_returns_the_affected_count() {
    let temp = TempDb::new("remove-prefix");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    factory
        .save_raw("window-manager-main/aux-window", "trackedBounds", &json!({ "x": 1 }))
        .unwrap();
    factory
        .save_raw("window-manager-main/aux-window", "quickCloseTip", &json!(true))
        .unwrap();
    factory
        .save_raw("window-manager-main/opgg-window", "trackedBounds", &json!({ "x": 2 }))
        .unwrap();

    let affected = factory
        .remove_by_prefix("window-manager-main/aux-window", "")
        .unwrap();
    assert_eq!(affected, 2);

    let remaining: Vec<String> = factory
        .get_by_prefix("window-manager-main", "")
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        remaining,
        vec!["window-manager-main/opgg-window/trackedBounds".to_string()]
    );
}

#[test]
fn migration_markers_store_key_as_value() {
    let temp = TempDb::new("markers");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    let marker = "riftflow-migration-from-1.2.6_patch2";
    assert!(!factory.has_migration(marker).unwrap());
    factory.mark_migration(marker).unwrap();
    assert!(factory.has_migration(marker).unwrap());
    assert_eq!(
        factory.db().lock().unwrap().metadata_get("nothing").unwrap(),
        None
    );
}

#[tokio::test]
async fn unregistered_namespace_writes_go_straight_to_the_row() {
    let temp = TempDb::new("raw-ns");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db);

    factory
        .set_setting("statistics-main", "alreadyCounted", json!(["1.5.1"]))
        .await
        .unwrap();
    assert_eq!(
        factory
            .get_setting("statistics-main", "alreadyCounted")
            .unwrap(),
        json!(["1.5.1"])
    );
    assert!(
        Path::new(&temp.db_path).exists(),
        "the db file itself is the assertion target"
    );
}

/// The shell drains on a 1 s tick; without a due window that tick would either
/// write every pending change immediately or, before this test, never at all.
#[tokio::test]
async fn flush_due_writes_only_after_the_debounce_window() {
    let temp = TempDb::new("due");
    let (db, _state) = temp.initialized();
    let factory = SettingFactory::new(db.clone());

    let service = factory
        .register("app-common-main", vec![decl("streamerMode", json!(false), boolean())])
        .unwrap();
    service.apply_to_state().unwrap();

    service.set("streamerMode", json!(true)).await.unwrap();
    assert_eq!(
        service.flush_due(Instant::now()).unwrap(),
        0,
        "still inside the debounce window"
    );

    std::thread::sleep(Duration::from_millis(SETTINGS_SAVE_DELAY_MS + 50));
    assert_eq!(service.flush_due(Instant::now()).unwrap(), 1);
    assert_eq!(
        factory
            .get_raw("app-common-main", "streamerMode")
            .unwrap()
            .unwrap(),
        json!(true)
    );
}

/// The ladder's other half: a database that is already at version 10 must gain
/// `queueType` and its index, and land at 15.
#[test]
fn a_v10_database_upgrades_to_the_current_version() {
    let temp = TempDb::new("v10");
    {
        let conn = temp.connection();
        conn.execute_batch(
            r#"CREATE TABLE "Metadata" ("key" varchar PRIMARY KEY NOT NULL, "value" json NOT NULL);
               CREATE TABLE "Settings" ("key" varchar PRIMARY KEY NOT NULL, "value" json NOT NULL);
               CREATE TABLE "SavedPlayers" ("puuid" varchar NOT NULL, "selfPuuid" varchar NOT NULL,
                 "region" varchar NOT NULL, "rsoPlatformId" varchar NOT NULL, "tag" varchar,
                 "updateAt" datetime NOT NULL, "lastMetAt" datetime,
                 PRIMARY KEY ("puuid", "selfPuuid", "region", "rsoPlatformId"));
               CREATE INDEX "saved_players_update_at_index" ON "SavedPlayers" ("updateAt");
               CREATE INDEX "saved_players_last_met_at_index" ON "SavedPlayers" ("lastMetAt");
               CREATE TABLE "EncounteredGames" ("id" integer PRIMARY KEY AUTOINCREMENT NOT NULL,
                 "gameId" integer NOT NULL, "puuid" varchar NOT NULL, "selfPuuid" varchar NOT NULL,
                 "region" varchar NOT NULL, "rsoPlatformId" varchar NOT NULL, "updateAt" datetime NOT NULL);
               CREATE INDEX "encountered_games_game_id_index" ON "EncounteredGames" ("gameId");
               CREATE INDEX "encountered_games_puuid_index" ON "EncounteredGames" ("puuid");
               CREATE INDEX "encountered_games_self_puuid_index" ON "EncounteredGames" ("selfPuuid");
               CREATE INDEX "encountered_games_region_index" ON "EncounteredGames" ("region");
               CREATE INDEX "encountered_games_rso_platform_id_index" ON "EncounteredGames" ("rsoPlatformId");
               CREATE INDEX "encountered_games_update_at_index" ON "EncounteredGames" ("updateAt");
               INSERT INTO "Metadata" (key, value) VALUES ('version', '10');"#,
        )
        .unwrap();
    }

    let (db, state) = Database::initialize(&temp.db_path).unwrap();
    assert!(!state.using_higher_version_db);
    db.close();

    let conn = temp.connection();
    let raw: rusqlite::types::Value = conn
        .query_row("SELECT value FROM \"Metadata\" WHERE key = 'version'", [], |r| r.get(0))
        .unwrap();
    let version = riftflow_store::cell_to_json_text(raw);
    assert_eq!(version.parse::<i64>().unwrap(), DB_CURRENT_VERSION);

    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('EncounteredGames')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(columns.contains(&"queueType".to_string()), "v15 加了 queueType");

    let indexed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='encountered_games_queue_type'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(indexed, 1);
}

/// `parseInt('abc', 10)` is NaN, and NaN compares false against both bounds, so
/// the original opens such a database untouched. Falling back to `0` instead
/// would run the whole ladder over an existing file.
#[test]
fn an_unparseable_version_is_used_as_is() {
    let temp = TempDb::new("nan");
    {
        let conn = temp.connection();
        conn.execute_batch(
            r#"CREATE TABLE "Metadata" ("key" varchar PRIMARY KEY NOT NULL, "value" json NOT NULL);
               INSERT INTO "Metadata" (key, value) VALUES ('version', 'garbage');"#,
        )
        .unwrap();
    }

    let (db, state) = Database::initialize(&temp.db_path).unwrap();
    assert!(!state.using_higher_version_db);
    db.close();

    let conn = temp.connection();
    let tables: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tables, 1, "只有 Metadata，没有被重建");
}

/// The data-loss regression: a read that fails because another process holds the
/// database used to be reported as "malformed", which renamed the user's real
/// file to a `_bk` backup and started an empty database next to it.
#[test]
fn a_locked_database_fails_loudly_instead_of_being_renamed() {
    let temp = TempDb::new("locked");
    {
        let (db, _state) = Database::initialize(&temp.db_path).unwrap();
        db.close();
    }

    let blocker = rusqlite::Connection::open(&temp.db_path).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();

    let outcome = Database::initialize(&temp.db_path);
    assert!(outcome.is_err(), "锁住时必须报错，不能自作主张重建");

    drop(blocker);
    assert!(
        temp.db_path.exists(),
        "用户的数据库文件不能被改名成备份后消失"
    );
    let backups = std::fs::read_dir(&temp.dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("_bk"))
        .count();
    assert_eq!(backups, 0, "没有重建发生就不该留下 _bk 备份");
}
