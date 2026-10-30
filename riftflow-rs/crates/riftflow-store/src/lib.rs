//! Port of `src/main/shards/storage/`.
//!
//! TypeORM is replaced by rusqlite, but the on-disk shape is kept byte-compatible
//! with the existing `RiftFlow.db`, because a real user upgrades in place:
//! the same table names, the same quoted identifiers, the same `json`/`datetime`
//! declared types, and the schema version still lives in the `Metadata` row
//! `version` rather than `PRAGMA user_version`.
//!
//! Consequences reproduced here:
//! * `synchronize: false` — tables are created only by the version ladder
//!   (v10 then v15), never auto-synced from entity definitions.
//! * A `Metadata` table with no `version` row is treated as malformed and the
//!   database is recreated.
//! * A stored version above `DB_CURRENT_VERSION` is tolerated read/write, and
//!   only flips `usingHigherVersionDb` so the renderer can warn.
//! * Upgrade failures retry exactly once, by recreating and migrating from 0.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

pub mod players;
pub mod settings;

pub const DB_FILENAME: &str = "RiftFlow.db";
pub const DB_CURRENT_VERSION: i64 = 15;

/// The four TypeORM entities registered in `storage/index.ts:53`.
pub const EXPECTED_TABLES: [&str; 4] = ["Metadata", "SavedPlayers", "Settings", "EncounteredGames"];

#[derive(Default)]
pub struct StorageState {
    pub using_higher_version_db: bool,
}

pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    /// `<userData>/RiftFlow.db`, matching `app.getPath('userData')` usage.
    pub fn default_path(user_data_dir: &Path) -> PathBuf {
        user_data_dir.join(DB_FILENAME)
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
        // better-sqlite3 runs with the SQLite defaults; the app never sets
        // journal_mode or busy_timeout, so neither do we.
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// `StorageDatabaseLifecycle.initialize`
    pub fn initialize(path: &Path) -> anyhow::Result<(Self, StorageState)> {
        let mut state = StorageState::default();
        let db = Self::open(path)?;

        let mut need_to_recreate = false;
        let mut need_to_perform_upgrade = false;
        let mut current_version: i64 = 0;

        match db.check_version() {
            Ok(VersionCheck::Current { version, higher }) => {
                current_version = version;
                state.using_higher_version_db = higher;
                if higher {
                    // version > DB_CURRENT_VERSION: used as-is, no migration
                } else if version < DB_CURRENT_VERSION {
                    need_to_perform_upgrade = true;
                }
            }
            Ok(VersionCheck::Fresh) => {
                need_to_perform_upgrade = true;
                current_version = 0;
            }
            Ok(VersionCheck::Unparseable) => {
                // `parseInt` gave NaN: neither above nor below the current
                // version, so the original opens the file exactly as it is
            }
            Ok(VersionCheck::Malformed) => {
                need_to_recreate = true;
                need_to_perform_upgrade = true;
            }
            Err(error) if is_database_corruption_error(&error.to_string()) => {
                need_to_recreate = true;
            }
            Err(error) => return Err(error),
        }

        if need_to_recreate {
            db.close();
            let recreated = Self::recreate(path)?;
            recreated.run_migrations(0, &mut state)?;
            return Ok((recreated, state));
        }

        if need_to_perform_upgrade {
            if let Err(error) = db.run_migrations(current_version, &mut state) {
                // `_runMigrations` catches, destroys the data source and retries
                // once from version zero — a stray legacy object must not brick
                // the app
                eprintln!("[storage] 迁移失败（{error}），备份重建后重试一次");
                db.close();
                let recreated = Self::recreate(path)?;
                recreated.run_migrations(0, &mut state)?;
                return Ok((recreated, state));
            }
        }

        Ok((db, state))
    }

    fn check_version(&self) -> anyhow::Result<VersionCheck> {
        if !self.has_metadata_table()? {
            return Ok(VersionCheck::Fresh);
        }

        // Only "no such row" means malformed. A real failure — the file is locked
        // by the Electron build, the header is unreadable — must propagate, or a
        // transient error would rename the user's database away.
        let raw = self.conn.query_row(
            "SELECT value FROM \"Metadata\" WHERE key = 'version'",
            [],
            |row| Ok(cell_to_json_text(row.get::<_, rusqlite::types::Value>(0)?)),
        );

        let raw = match raw {
            Ok(text) => text,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(VersionCheck::Malformed),
            Err(error) => return Err(error.into()),
        };

        // `parseInt(value, 10)`: an unparseable version is NaN, and NaN compares
        // false against both bounds, so the original uses that database as-is
        let Ok(version) = raw.trim().trim_matches('"').parse::<i64>() else {
            return Ok(VersionCheck::Unparseable);
        };

        if version > DB_CURRENT_VERSION {
            return Ok(VersionCheck::Current {
                version,
                higher: true,
            });
        }

        Ok(VersionCheck::Current {
            version,
            higher: false,
        })
    }

    /// `Unparseable` mirrors `parseInt` returning NaN: no migration, no rebuild.
    #[allow(dead_code)]
    fn has_metadata_table(&self) -> anyhow::Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='Metadata'",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// `_runMigrationsInTransaction` — every pending upgrade in ONE transaction.
    fn run_migrations(&self, from_version: i64, _state: &mut StorageState) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        if from_version < 10 {
            upgrade_to_10(&tx)?;
        }
        if from_version < 15 {
            upgrade_to_15(&tx)?;
        }

        // the ladder just ran on our own words: check it produced the schema we
        // claim, rather than discovering it later through empty-looking features
        assert_schema(&tx)?;

        tx.commit()?;
        Ok(())
    }

    /// `_recreateDatabase` — rename the db and its sidecars to a timestamped
    /// backup rather than deleting them, then open a brand new file.
    fn recreate(path: &Path) -> anyhow::Result<Self> {
        let stamp = timestamp_stamp();
        let sidecars = ["", "-wal", "-shm", "-journal"];

        let parent = path.parent().unwrap_or(path);
        let extension = path
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();

        for sidecar in sidecars {
            let source = format!("{}{}", path.display(), sidecar);
            if Path::new(&source).exists() {
                let target = parent.join(format!("{stamp}_bk{extension}{sidecar}"));
                std::fs::rename(&source, &target)
                    .map_err(|e| anyhow::anyhow!("rename {source}: {e}"))?;
            }
        }

        Self::open(path)
    }

    pub fn close(self) {
        drop(self.conn);
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn metadata_get(&self, key: &str) -> anyhow::Result<Option<Value>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM \"Metadata\" WHERE key = ?1", [key], |r| {
                Ok(cell_to_json_text(r.get::<_, rusqlite::types::Value>(0)?))
            })
            .ok();
        Ok(raw.and_then(|text| serde_json::from_str(&text).ok()))
    }

    /// Reads a `json`-declared column.
    ///
    /// `json` is not a SQLite type; TypeORM emits it verbatim, and an
    /// unrecognized type name falls through to NUMERIC affinity. So a numeric
    /// payload such as the schema version is stored as an INTEGER cell, not as
    /// text — reading it with `get::<String>()` silently fails. Verified against
    /// a live `RiftFlow.db` where `Metadata.version` comes back as an integer.
    pub fn json_cell(&self, sql: &str, key: &str) -> anyhow::Result<Option<Value>> {
        let raw: Option<String> = self
            .conn
            .query_row(sql, [key], |r| {
                Ok(cell_to_json_text(r.get::<_, rusqlite::types::Value>(0)?))
            })
            .ok();
        Ok(raw.and_then(|text| serde_json::from_str(&text).ok()))
    }

    pub fn metadata_set(&self, key: &str, value: &Value) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO \"Metadata\" (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, value.to_string()],
        )?;
        Ok(())
    }

    pub fn table_names(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(names)
    }
}

enum VersionCheck {
    /// no Metadata table at all
    Fresh,
    /// Metadata table exists but has no `version` row
    Malformed,
    /// the stored version is not a number, i.e. `parseInt` → NaN: the original
    /// neither migrates nor rebuilds in that case
    Unparseable,
    Current { version: i64, higher: bool },
}

/// Converts any cell of a `json`-declared column into the JSON text that
/// `serde_json` can parse, because TypeORM's `json` columns have NUMERIC
/// affinity and coerce plain numbers into INTEGER/REAL cells.
pub fn cell_to_json_text(value: rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    match value {
        Value::Null => "null".to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(t) => t,
        Value::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
    }
}

/// `isDatabaseCorruptionError` — matches on the codes and messages the driver
/// produces, walked through cause chains in JS.
fn is_database_corruption_error(message: &str) -> bool {
    message.contains("SQLITE_CORRUPT")
        || message.contains("SQLITE_NOTADB")
        || message.contains("database disk image is malformed")
        || message.contains("file is not a database")
}

fn timestamp_stamp() -> String {
    // dayjs('YYYYMMDDHHmmssSSS') in the original
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let secs = now / 1000;
    let millis = now % 1000;
    let days = secs / 86_400;
    let mut rem = secs % 86_400;
    let hour = rem / 3600;
    rem %= 3600;
    let minute = rem / 60;
    let second = rem % 60;

    let mut year = 1970;
    let mut remaining_days = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let year_len = if leap { 366 } else { 365 };
        if remaining_days < year_len {
            break;
        }
        remaining_days -= year_len;
        year += 1;
    }

    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_lengths: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0usize;
    while remaining_days >= month_lengths[month] {
        remaining_days -= month_lengths[month];
        month += 1;
    }

    format!(
        "{year:04}{:02}{:02}{hour:02}{minute:02}{second:02}{millis:03}",
        month + 1,
        remaining_days + 1
    )
}


/// Every table and index the ladder is supposed to leave behind. A half-applied
/// upgrade used to boot silently; `EXPECTED_TABLES` was declared and never read.
fn assert_schema(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    const EXPECTED_INDEXES: [&str; 9] = [
        "saved_players_update_at_index",
        "saved_players_last_met_at_index",
        "encountered_games_game_id_index",
        "encountered_games_puuid_index",
        "encountered_games_self_puuid_index",
        "encountered_games_region_index",
        "encountered_games_rso_platform_id_index",
        "encountered_games_update_at_index",
        "encountered_games_queue_type",
    ];

    for table in EXPECTED_TABLES {
        let found: i64 = tx.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        if found == 0 {
            anyhow::bail!("migration left no table {table}");
        }
    }

    for index in EXPECTED_INDEXES {
        let found: i64 = tx.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name = ?1",
            [index],
            |row| row.get(0),
        )?;
        if found == 0 {
            anyhow::bail!("migration left no index {index}");
        }
    }

    Ok(())
}

fn has_index(tx: &rusqlite::Transaction<'_>, name: &str) -> anyhow::Result<bool> {
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// `v10_LA1_2_0initializationUpgrade`
fn upgrade_to_10(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    tx.execute_batch(
        r#"
CREATE TABLE "Metadata" ("key" varchar PRIMARY KEY NOT NULL, "value" json NOT NULL);
CREATE TABLE "Settings" ("key" varchar PRIMARY KEY NOT NULL, "value" json NOT NULL);
CREATE TABLE "SavedPlayers" (
  "puuid" varchar NOT NULL,
  "selfPuuid" varchar NOT NULL,
  "region" varchar NOT NULL,
  "rsoPlatformId" varchar NOT NULL,
  "tag" varchar,
  "updateAt" datetime NOT NULL,
  "lastMetAt" datetime,
  PRIMARY KEY ("puuid", "selfPuuid", "region", "rsoPlatformId")
);
CREATE INDEX "saved_players_update_at_index" ON "SavedPlayers" ("updateAt");
CREATE INDEX "saved_players_last_met_at_index" ON "SavedPlayers" ("lastMetAt");
CREATE TABLE "EncounteredGames" (
  "id" integer PRIMARY KEY AUTOINCREMENT NOT NULL,
  "gameId" integer NOT NULL,
  "puuid" varchar NOT NULL,
  "selfPuuid" varchar NOT NULL,
  "region" varchar NOT NULL,
  "rsoPlatformId" varchar NOT NULL,
  "updateAt" datetime NOT NULL
);
CREATE INDEX "encountered_games_game_id_index" ON "EncounteredGames" ("gameId");
CREATE INDEX "encountered_games_puuid_index" ON "EncounteredGames" ("puuid");
CREATE INDEX "encountered_games_self_puuid_index" ON "EncounteredGames" ("selfPuuid");
CREATE INDEX "encountered_games_region_index" ON "EncounteredGames" ("region");
CREATE INDEX "encountered_games_rso_platform_id_index" ON "EncounteredGames" ("rsoPlatformId");
CREATE INDEX "encountered_games_update_at_index" ON "EncounteredGames" ("updateAt");
"#,
    )?;

    // The original inserts these via `json('10')`, i.e. the JSON text `10`.
    tx.execute(
        "INSERT OR REPLACE INTO \"Metadata\" (key, value) VALUES ('version', '10')",
        [],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO \"Metadata\" (key, value) VALUES ('debugging-text-0', '\"RiftFlow, Hanxven@2024\"')",
        [],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO \"Metadata\" (key, value) VALUES ('debugging-text-1', '\"Ayano\"')",
        [],
    )?;

    Ok(())
}

/// `v15_LA1_2_2Upgrade`. The version row is written even when the table is
/// missing, which is what the guarded original does.
fn upgrade_to_15(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    let has_table: i64 = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='EncounteredGames'",
        [],
        |r| r.get(0),
    )?;

    if has_table > 0 {
        let has_column: i64 = tx.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('EncounteredGames') WHERE name = 'queueType'",
            [],
            |r| r.get(0),
        )?;
        if has_column == 0 {
            tx.execute(
                "ALTER TABLE \"EncounteredGames\" ADD \"queueType\" varchar NOT NULL DEFAULT ''",
                [],
            )?;
        }

        // guarded on the index rather than the column, so a half-applied upgrade
        // cannot leave `queueType` unindexed while still claiming version 15
        if !has_index(tx, "encountered_games_queue_type")? {
            tx.execute(
                "CREATE INDEX \"encountered_games_queue_type\" ON \"EncounteredGames\" (\"queueType\")",
                [],
            )?;
        }
    }

    tx.execute(
        "UPDATE \"Metadata\" SET value = '15' WHERE key = 'version'",
        [],
    )?;

    Ok(())
}
