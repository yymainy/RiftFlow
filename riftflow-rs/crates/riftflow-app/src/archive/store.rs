use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{ensure, Context};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub struct ArchiveStore(Mutex<Connection>);

fn settled_result(game: &Value, participant: &Value) -> Option<bool> {
    let status = game["endOfGameResult"].as_str().unwrap_or("");
    if !status.is_empty() && status != "GameComplete" { return None; }
    let won = participant["stats"]["win"].as_bool();
    let has_winner = game["teams"].as_array().map_or(false, |teams| teams.iter().any(|team| team["win"] == "Win" || team["win"] == true));
    if status == "GameComplete" || has_winner || won == Some(true) { won } else { None }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

pub fn make_record(game: Value, owner: &str, region: &str, player_name: &str) -> anyhow::Result<Value> {
    ensure!(!owner.is_empty() && !region.is_empty(), "missing account or region");
    let id = game["gameId"].as_u64().filter(|id| *id > 0).context("missing game ID")?;
    let identities = game["participantIdentities"].as_array().context("missing player identities")?;
    let identity = identities.iter().find(|identity| identity["player"]["puuid"].as_str() == Some(owner))
        .context("match does not belong to this account")?;
    let participant_id = identity["participantId"].as_u64().context("missing participant ID")?;
    let participants = game["participants"].as_array().context("missing participants")?;
    let participant = participants.iter().find(|p| p["participantId"].as_u64() == Some(participant_id))
        .context("missing account statistics")?;
    let champion_id = participant["championId"].as_i64().unwrap_or(0);
    let champions: Value = serde_json::from_str(include_str!("../../assets/champion-summary.json"))?;
    let champion_name = champions.as_array().and_then(|all| all.iter().find(|c| c["id"].as_i64() == Some(champion_id)))
        .and_then(|c| c["name"].as_str()).map(str::to_owned).unwrap_or_else(|| champion_id.to_string());
    let stats = &participant["stats"];
    let complete = !participants.is_empty() && participants.len() == identities.len()
        && game["teams"].as_array().map_or(false, |t| !t.is_empty());
    let key = serde_json::to_string(&(region, owner, id))?;
    Ok(json!({
        "key": key, "owner": owner, "region": region, "gameId": id.to_string(),
        "playerName": player_name, "championId": champion_id, "championName": champion_name,
        "createdAt": game["gameCreation"].as_u64().unwrap_or(0),
        "gameMode": game["gameMode"].as_str().unwrap_or("UNKNOWN"), "queueId": game["queueId"],
        "duration": game["gameDuration"].as_u64().unwrap_or(0), "version": game["gameVersion"].as_str().unwrap_or(""),
        "kills": stats["kills"], "deaths": stats["deaths"], "assists": stats["assists"], "win": settled_result(&game, participant),
        "gold": stats["goldEarned"], "damage": stats["totalDamageDealtToChampions"],
        "cs": stats["totalMinionsKilled"].as_u64().unwrap_or(0) + stats["neutralMinionsKilled"].as_u64().unwrap_or(0),
        "source": "LCU", "savedAt": now(), "complete": complete,
        "csAt10": null, "firstDeathMinute": null, "timelineChecked": false, "firstDeathChecked": false,
        "game": game
    }))
}

fn summary(record: &Value) -> Value {
    let mut value = record.clone();
    if let Some(object) = value.as_object_mut() { object.remove("game"); }
    value
}

fn write(connection: &Connection, record: &Value) -> anyhow::Result<()> {
    connection.execute(
        "INSERT INTO matches(key, summary, record) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET summary=excluded.summary, record=excluded.record
         WHERE json_extract(matches.summary, '$.complete') != 1 OR json_extract(excluded.summary, '$.complete') = 1",
        params![record["key"].as_str().context("missing key")?, summary(record).to_string(), record.to_string()],
    )?;
    Ok(())
}

impl ArchiveStore {
    pub fn count(&self) -> anyhow::Result<u64> {
        Ok(self.0.lock().unwrap().query_row("SELECT COUNT(*) FROM matches", [], |row| row.get(0))?)
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS matches(key TEXT PRIMARY KEY, summary TEXT NOT NULL, record TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS documents(key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
        let normalized: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM documents WHERE key='result-normalization-v1')", [], |r| r.get(0))?;
        if !normalized {
            let rows = {
                let mut statement = connection.prepare("SELECT record FROM matches")?;
                let values = statement.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                values
            };
            let transaction = connection.transaction()?;
            for text in rows {
                let mut record: Value = serde_json::from_str(&text)?;
                let fresh = make_record(record["game"].clone(), record["owner"].as_str().unwrap_or(""), record["region"].as_str().unwrap_or(""), record["playerName"].as_str().unwrap_or(""))?;
                record["win"] = fresh["win"].clone();
                write(&transaction, &record)?;
            }
            transaction.execute("INSERT INTO documents(key,value) VALUES('result-normalization-v1','true')", [])?;
            transaction.commit()?;
        }
        Ok(Self(Mutex::new(connection)))
    }

    pub fn save(&self, record: &Value) -> anyhow::Result<()> { write(&self.0.lock().unwrap(), record) }

    pub fn document(&self, key: &str) -> anyhow::Result<Value> {
        use rusqlite::OptionalExtension;
        let text: Option<String> = self.0.lock().unwrap().query_row("SELECT value FROM documents WHERE key=?1", [key], |r| r.get(0)).optional()?;
        Ok(text.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or(Value::Null))
    }

    pub fn put_document(&self, key: &str, value: &Value) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute("INSERT INTO documents VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value.to_string()])?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> anyhow::Result<Value> {
        let connection = self.0.lock().unwrap();
        let text: String = connection.query_row("SELECT record FROM matches WHERE key=?1", [key], |row| row.get(0))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn records(&self) -> anyhow::Result<Vec<Value>> {
        let connection=self.0.lock().unwrap();
        let mut statement=connection.prepare("SELECT record FROM matches")?;
        let rows=statement.query_map([],|row|row.get::<_,String>(0))?;
        rows.map(|row|Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn list(&self, filter: &Value) -> anyhow::Result<Value> {
        let query = filter["query"].as_str().unwrap_or("").trim().to_lowercase();
        let mode = filter["mode"].as_str().unwrap_or("");
        let start = filter["start"].as_u64().unwrap_or(0);
        let end = filter["end"].as_u64().unwrap_or(u64::MAX);
        let page = filter["page"].as_u64().unwrap_or(1).max(1).min(1_000_000) as usize;
        let connection = self.0.lock().unwrap();
        let mut statement = connection.prepare("SELECT summary FROM matches ORDER BY json_extract(summary, '$.createdAt') DESC, key")?;
        let all = statement.query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut matched = Vec::new();
        let mut modes = std::collections::BTreeSet::new();
        for text in &all {
            let record: Value = serde_json::from_str(text)?;
            if let Some(m) = record["gameMode"].as_str() { modes.insert(m.to_string()); }
            let created = record["createdAt"].as_u64().unwrap_or(0);
            let search = format!("{} {} {} {}", record["playerName"].as_str().unwrap_or(""), record["championName"].as_str().unwrap_or(""), record["region"].as_str().unwrap_or(""), record["gameId"].as_str().unwrap_or(""));
            if (mode.is_empty() || record["gameMode"].as_str() == Some(mode))
                && created >= start && created <= end && search.to_lowercase().contains(&query) {
                matched.push(record);
            }
        }
        Ok(json!({ "total": all.len(), "matched": matched.len(), "modes": modes,
            "records": matched.into_iter().skip((page-1)*50).take(50).collect::<Vec<_>>() }))
    }

    pub fn export(&self, csv: bool) -> anyhow::Result<String> {
        let connection = self.0.lock().unwrap();
        let mut statement = connection.prepare("SELECT record FROM matches ORDER BY key")?;
        let records = statement.query_map([], |row| row.get::<_, String>(0))?
            .map(|row| -> anyhow::Result<Value> { Ok(serde_json::from_str(&row?)?) }).collect::<anyhow::Result<Vec<_>>>()?;
        if csv {
            let fields = ["region", "playerName", "gameId", "championName", "gameMode", "queueId", "createdAt", "duration", "win", "kills", "deaths", "assists", "cs", "gold", "damage", "version", "source", "savedAt", "complete"];
            let mut output = format!("\u{feff}{}\r\n", fields.join(","));
            for record in records {
                output.push_str(&fields.iter().map(|field| csv_cell(&record[*field])).collect::<Vec<_>>().join(","));
                output.push_str("\r\n");
            }
            Ok(output)
        } else {
            use rusqlite::OptionalExtension;
            let mut tools=serde_json::Map::new();
            for key in ["personal","vault"] {
                let stored:Option<String>=connection.query_row("SELECT value FROM documents WHERE key=?1",[key],|r|r.get(0)).optional()?;
                if let Some(stored)=stored {tools.insert(key.into(),serde_json::from_str::<Value>(&stored)?);}
            }
            Ok(serde_json::to_string_pretty(&json!({ "type": "riftflow-match-archive", "version": 1, "exportedAt": now(), "records": records, "tools":tools }))?)
        }
    }

    pub fn restore(&self, document: &Value) -> anyhow::Result<usize> {
        ensure!(document["type"] == "riftflow-match-archive" && document["version"] == 1, "unsupported archive format");
        let records = document["records"].as_array().context("missing archive records")?;
        ensure!(records.len() <= 20_000, "archive contains too many records");
        // Recompute metadata from the original game; validate the whole file before writing.
        let normalized = records.iter().map(|record| {
            let mut value = make_record(record["game"].clone(), record["owner"].as_str().unwrap_or(""),
                record["region"].as_str().unwrap_or(""), record["playerName"].as_str().unwrap_or(""))?;
            value["savedAt"] = record["savedAt"].as_u64().map(Value::from).unwrap_or_else(|| json!(now()));
            value["complete"] = json!(record["complete"] == true && value["complete"] == true);
            if let Some(cs) = record["csAt10"].as_u64().filter(|cs| *cs <= 1000) {
                value["csAt10"] = json!(cs);
            }
            if let Some(minute) = record["firstDeathMinute"].as_f64().filter(|minute| minute.is_finite() && (0.0..=240.0).contains(minute)) {
                value["firstDeathMinute"] = json!(minute);
            }
            value["timelineChecked"] = json!(record["timelineChecked"] == true);
            value["firstDeathChecked"] = json!(record["firstDeathChecked"] == true);
            Ok(value)
        }).collect::<anyhow::Result<Vec<Value>>>()?;
        let tools=document.get("tools").filter(|v|v.is_object());
        if let Some(tools)=tools {validate_tools(tools)?;}
        let mut connection = self.0.lock().unwrap();
        let transaction = connection.transaction()?;
        for record in &normalized { write(&transaction, record)?; }
        if let Some(tools)=tools {
            use rusqlite::OptionalExtension;
            for key in ["personal","vault"] {
                let Some(imported)=tools.get(key) else {continue};
                let previous:Option<String>=transaction.query_row("SELECT value FROM documents WHERE key=?1",[key],|r|r.get(0)).optional()?;
                let mut merged:Value=previous.map(|s|serde_json::from_str(&s)).transpose()?.unwrap_or_else(||if key=="personal" {json!({"notes":[],"goal":null})}else{json!([])});
                let additions=if key=="personal" {imported["notes"].as_array().unwrap()} else {imported.as_array().unwrap()};
                let destination=if key=="personal" {merged["notes"].as_array_mut().unwrap()} else {merged.as_array_mut().unwrap()};
                for item in additions {
                    if !destination.iter().any(|old|old["id"]==item["id"]) {destination.push(item.clone());}
                }
                if key=="personal" && merged["goal"].is_null() {merged["goal"]=imported["goal"].clone();}
                validate_tools(&json!({(key):merged}))?;
                transaction.execute("INSERT INTO documents VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,merged.to_string()])?;
            }
        }
        transaction.commit()?;
        Ok(normalized.len())
    }
}

fn validate_tools(tools:&Value)->anyhow::Result<()> {
    if let Some(personal)=tools.get("personal") {
        let notes=personal["notes"].as_array().context("invalid memory cards")?;
        ensure!(notes.len()<=2000,"too many memory cards");
        for note in notes {
            for field in ["id","matchKey","gameId","owner","region","championName","gameMode","version","role","status","fact","action"] {
                ensure!(note[field].as_str().map_or(false,|s|s.len()<=4000),"invalid memory card field");
            }
            ensure!(note["championId"].as_u64().is_some() && note["opponent"].as_u64().is_some(),"invalid champion ID");
        }
        if !personal["goal"].is_null() {
            let goal=&personal["goal"];
            for field in ["matchKey","owner","region","championName","gameMode","version","role","metric","review"] {ensure!(goal[field].as_str().map_or(false,|s|s.len()<=4000),"invalid practice goal field");}
            ensure!(goal["target"].as_f64().map_or(false,|v|(0.0..=100.0).contains(&v)) && goal["at"].as_u64().is_some(),"invalid practice target");
        }
    }
    if let Some(vault)=tools.get("vault") {
        let snapshots=vault.as_array().context("invalid settings snapshots")?;
        ensure!(snapshots.len()<=200,"too many settings snapshots");
        for snapshot in snapshots {
            for field in ["id","label","scope"] {ensure!(snapshot[field].as_str().map_or(false,|s|s.len()<=4000),"invalid snapshot metadata");}
            ensure!(snapshot["at"].as_u64().is_some(),"invalid snapshot time");
            let files=snapshot["files"].as_object().context("missing snapshot files")?;
            ensure!(!files.is_empty() && files.len()<=2,"invalid snapshot files");
            for (name,value) in files {ensure!(["game.cfg","PersistedSettings.json"].contains(&name.as_str()) && value.as_str().map_or(false,|s|s.len()<=4*1024*1024),"invalid snapshot file");}
        }
    }
    Ok(())
}

fn csv_cell(value: &Value) -> String {
    let text = if value.is_null() { String::new() } else { value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()) };
    let text = if text.trim_start().starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text };
    format!("\"{}\"", text.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aborted_games_are_unsettled_even_when_stats_default_to_loss() {
        let participant=json!({"stats":{"win":false}});
        assert_eq!(settled_result(&json!({"endOfGameResult":"Abort_TooFewPlayers","teams":[{"win":"Fail"}]}), &participant),None);
        assert_eq!(settled_result(&json!({"endOfGameResult":"GameComplete","teams":[{"win":"Fail"},{"win":"Win"}]}), &participant),Some(false));
        assert_eq!(settled_result(&json!({"teams":[{"win":"Fail"},{"win":"Fail"}]}), &participant),None);
        assert_eq!(settled_result(&json!({"endOfGameResult":"GameComplete"}), &json!({"stats":{"win":true}})),Some(true));
    }
    fn game(id: u64, owner: &str) -> Value {
        json!({"gameId":id,"gameCreation":123,"gameMode":"CLASSIC","gameVersion":"16.21",
            "participantIdentities":[{"participantId":1,"player":{"puuid":owner}}],
            "participants":[{"participantId":1,"championId":1,"stats":{"kills":3,"deaths":2,"assists":5,"win":true}}]})
    }
    #[test]
    fn isolation_restore_and_validation() {
        let store = ArchiveStore::open(Path::new(":memory:")).unwrap();
        for (region, owner) in [("CN1","one"),("CN2","one"),("CN1","two")] {
            store.save(&make_record(game(42,owner),owner,region,"=HYPERLINK(1)").unwrap()).unwrap();
        }
        assert_eq!(store.list(&json!({})).unwrap()["total"],3);
        assert_eq!(store.list(&json!({"query":"安妮"})).unwrap()["matched"],3);
        assert_eq!(store.list(&json!({"mode":"ARAM"})).unwrap()["matched"],0);
        assert!(make_record(game(42,"one"),"two","CN1","").is_err());
        let mut backup: Value = serde_json::from_str(&store.export(false).unwrap()).unwrap();
        backup["records"][0]["championName"] = json!("forged");
        let restored = ArchiveStore::open(Path::new(":memory:")).unwrap();
        assert_eq!(restored.restore(&backup).unwrap(),3);
        assert_eq!(restored.list(&json!({"query":"forged"})).unwrap()["matched"],0);
        backup["records"][1]["owner"] = json!("invalid");
        let empty = ArchiveStore::open(Path::new(":memory:")).unwrap();
        assert!(empty.restore(&backup).is_err());
        assert_eq!(empty.list(&json!({})).unwrap()["total"],0);
        assert!(store.export(true).unwrap().contains("'=HYPERLINK"));
    }
    #[test]
    fn file_archive_survives_reopen_and_deduplicates() {
        let path = std::env::temp_dir().join(format!("riftflow-archive-{}-{}.db", std::process::id(), now()));
        let record = make_record(game(10,"me"),"me","CN1","Player").unwrap();
        { let store=ArchiveStore::open(&path).unwrap();store.save(&record).unwrap();store.save(&record).unwrap(); }
        { let store=ArchiveStore::open(&path).unwrap();assert_eq!(store.list(&json!({})).unwrap()["total"],1);assert_eq!(store.get(record["key"].as_str().unwrap()).unwrap()["game"]["gameId"],10); }
        std::fs::remove_file(path).unwrap();
    }
}
