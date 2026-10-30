use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use riftflow_lcu::LeagueClientMain;
use riftflow_state::StateSync;
use serde_json::{json, Value};

use super::store::{make_record, now, ArchiveStore};

pub struct ArchiveController {
    pub store: ArchiveStore,
    pub client: Arc<LeagueClientMain>,
    pub state: Arc<StateSync>,
    busy: AtomicBool,
    sync_lock: tokio::sync::Mutex<()>,
    pub vault_lock: tokio::sync::Mutex<()>,
    last_sync: Mutex<Value>,
}

struct SyncGuard<'a>(&'a AtomicBool);
pub(super) async fn archive_get(http: &riftflow_lcu::http::LcuHttp, path: &str, stage: &str) -> anyhow::Result<Value> {
    let response = http.request(riftflow_lcu::http::RequestConfig { method: "GET".into(), url: path.into(), ..Default::default() }).await
        .map_err(|error| {
            let message=error.to_string();
            if let Some(status)=message.strip_prefix("LCU request failed with status ").and_then(|s|s.parse::<u16>().ok()) {
                anyhow::anyhow!("{stage}：客户端返回 HTTP {status}，重试后仍不可用，请稍后再试")
            } else {anyhow::anyhow!("{stage}：连接中断或请求超时，请检查客户端登录状态后重试")}
        })?;
    anyhow::ensure!(response.status < 400, "{stage}：客户端返回 HTTP {}，请在游戏客户端确认战绩可打开后重试", response.status);
    Ok(response.body)
}

fn cs_at_ten_minutes(timeline: &Value, participant_id: u64) -> Option<u64> {
    let frames = timeline.get("frames").or_else(|| timeline.pointer("/info/frames"))?.as_array()?;
    let frame = frames.iter()
        .filter_map(|frame| Some((frame["timestamp"].as_u64()?, frame)))
        .filter(|(time, _)| (595_000..=605_000).contains(time))
        .min_by_key(|(time, _)| *time)?.1;
    let player = frame["participantFrames"].get(participant_id.to_string())?;
    let lane = player["minionsKilled"].as_u64()?;
    let jungle = player["jungleMinionsKilled"].as_u64()?;
    lane.checked_add(jungle).filter(|value| *value <= 1000)
}

fn first_death_minute(timeline: &Value, participant_id: u64, deaths: u64, duration: u64) -> Option<f64> {
    let frames = timeline.get("frames").or_else(|| timeline.pointer("/info/frames"))?.as_array()?;
    let death = frames.iter().filter_map(|frame| frame["events"].as_array())
        .flatten()
        .filter(|event| event["type"] == "CHAMPION_KILL" && event["victimId"].as_u64() == Some(participant_id))
        .filter_map(|event| event["timestamp"].as_u64())
        .min();
    match death {
        Some(timestamp) if timestamp <= duration.saturating_add(60) * 1000 => Some(timestamp as f64 / 60_000.0),
        None if deaths == 0 && !frames.is_empty() && duration > 0 => Some(duration as f64 / 60.0),
        _ => None,
    }.filter(|minute| *minute <= 240.0)
}

async fn fill_timeline_metric(http: &riftflow_lcu::http::LcuHttp, record: &mut Value) {
    if record["timelineChecked"] == true && record["firstDeathChecked"] == true { return; }
    if record["duration"].as_u64().unwrap_or(0) == 0 {
        record["timelineChecked"] = json!(true);
        record["firstDeathChecked"] = json!(true);
        return;
    }
    let Some(id) = record["gameId"].as_str() else { return };
    let timeline = match archive_get(http, &format!("/lol-match-history/v1/game-timelines/{id}"), "读取时间线").await {
        Ok(timeline) => timeline,
        Err(error) => {
            // A missing/forbidden timeline is a stable absence for this match.
            // Transient errors are retried on the next archive sync.
            let reason = error.to_string();
            if reason.contains("HTTP 404") || reason.contains("HTTP 403") {
                record["timelineChecked"] = json!(true);
                record["firstDeathChecked"] = json!(true);
            }
            return;
        }
    };
    let Some(participant_id) = record["game"]["participantIdentities"].as_array()
        .and_then(|items| items.iter().find(|item| item["player"]["puuid"] == record["owner"]))
        .and_then(|item| item["participantId"].as_u64()) else { return };
    record["csAt10"] = cs_at_ten_minutes(&timeline, participant_id).map_or(Value::Null, Value::from);
    record["firstDeathMinute"] = first_death_minute(&timeline, participant_id,
        record["deaths"].as_u64().unwrap_or(u64::MAX), record["duration"].as_u64().unwrap_or(0))
        .map_or(Value::Null, Value::from);
    record["timelineChecked"] = json!(true);
    record["firstDeathChecked"] = json!(true);
}
impl Drop for SyncGuard<'_> { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }

impl ArchiveController {
    pub fn new(store: ArchiveStore, client: Arc<LeagueClientMain>, state: Arc<StateSync>) -> Self {
        let previous = store.document("last-sync").ok().filter(|v| v.is_object()).unwrap_or(json!({}));
        Self { store, client, state, busy: AtomicBool::new(false), sync_lock: tokio::sync::Mutex::new(()), vault_lock: tokio::sync::Mutex::new(()), last_sync: Mutex::new(previous) }
    }

    pub fn phase(&self) -> String {
        self.state.get("league-client-main", "gameflow", "phase")
            .and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
    }

    pub fn diagnostics(&self) -> Value {
        let connection = self.client.state.connection_state();
        let auth = self.client.state.auth_public();
        let summoner = self.state.get("league-client-main", "summoner", "me").unwrap_or(Value::Null);
        let availability = self.state.get("sgp-main", "state", "availability").unwrap_or(Value::Null);
        let region = auth.as_ref().map(|a| format!("{}:{}", a.region, a.rso_platform_id)).unwrap_or_default();
        // Select individual public status fields. Never serialize connection credentials.
        let detected = self.state.get("league-client-ux-main", "state", "launchedClients")
            .and_then(|v| v.as_array().map(Vec::len)).unwrap_or(0);
        json!({ "connection": connection, "detectedClients": detected,
            "connected": connection == riftflow_lcu::ConnectionState::Connected,
            "loggedIn": summoner["puuid"].as_str().map_or(false, |p| !p.is_empty()),
            "playerName": summoner["gameName"].as_str().unwrap_or(""), "region": region,
            "phase": self.phase(), "elevated": riftflow_native::is_elevated(),
            "sgpConfigured": availability["sgpServerId"].as_str().map_or(false, |s| !s.is_empty()),
            "busy": self.busy.load(Ordering::Acquire), "lastSync": self.last_sync.lock().unwrap().clone(),
            "archiveCount": self.store.count().ok(),
            "source": "LCU", "autoSync": true, "paused": matches!(self.phase().as_str(), "InProgress" | "ChampSelect" | "ReadyCheck" | "GameStart") })
    }

    pub async fn sync(&self) -> anyhow::Result<Value> {
        let _serial = self.sync_lock.lock().await;
        self.busy.store(true, Ordering::Release);
        let _guard = SyncGuard(&self.busy);
        let outcome = self.sync_inner().await;
        match &outcome {
            Ok(result) => *self.last_sync.lock().unwrap() = result.clone(),
            Err(error) => {
                // Context contains stage/status only, never response bodies or credentials.
                let reason = format!("{error:#}");
                crate::logger::warn("riftflow-archive-main", &reason);
                *self.last_sync.lock().unwrap() = json!({ "at": now(), "success": false, "reason": reason });
            }
        }
        let _ = self.store.put_document("last-sync", &self.last_sync.lock().unwrap());
        outcome
    }

    async fn sync_inner(&self) -> anyhow::Result<Value> {
        anyhow::ensure!(!matches!(self.phase().as_str(), "InProgress" | "ChampSelect" | "ReadyCheck" | "GameStart"), "Archive synchronization pauses during champion select and gameplay");
        let http = self.client.http().ok_or_else(|| anyhow::anyhow!("Start and sign in to the League client first"))?;
        let auth = self.client.state.auth_public().ok_or_else(|| anyhow::anyhow!("No active connection"))?;
        let region = format!("{}:{}", auth.region, auth.rso_platform_id);
        let summoner = archive_get(&http, "/lol-summoner/v1/current-summoner", "读取当前账号").await?;
        let owner = summoner["puuid"].as_str().filter(|p| !p.is_empty()).ok_or_else(|| anyhow::anyhow!("Waiting for client login"))?;
        let player_name = format!("{}#{}", summoner["gameName"].as_str().or(summoner["displayName"].as_str()).unwrap_or(""), summoner["tagLine"].as_str().unwrap_or(""));
        let history = archive_get(&http, &format!("/lol-match-history/v1/products/lol/{owner}/matches?begIndex=0&endIndex=19"), "读取战绩列表").await?;
        let games = history.pointer("/games/games").and_then(Value::as_array).ok_or_else(|| anyhow::anyhow!("Match history returned an unexpected response"))?;
        let mut saved = 0;
        let mut skipped = 0;
        let mut incomplete = 0;
        let mut failures = Vec::new();
        for game in games.iter().take(20) {
            if matches!(self.phase().as_str(), "InProgress" | "ChampSelect" | "ReadyCheck" | "GameStart") { break; }
            let initial = make_record(game.clone(), owner, &region, &player_name);
            let mut record = match initial {
                Ok(record) => record,
                Err(_) => {
                    let id = game["gameId"].as_u64().unwrap_or(0);
                    match archive_get(&http, &format!("/lol-match-history/v1/games/{id}"), "补全战绩身份").await.and_then(|d| make_record(d, owner, &region, &player_name)) {
                        Ok(record) => record,
                        Err(_) => { failures.push(json!({"gameId":id.to_string(),"reason":"缺少本账号身份或统计，未保存"})); continue; }
                    }
                }
            };
            let key = record["key"].as_str().unwrap();
            if let Ok(mut old) = self.store.get(key) {
                if old["complete"] == true {
                    if old["timelineChecked"] != true || old["firstDeathChecked"] != true {
                        fill_timeline_metric(&http, &mut old).await;
                        self.store.save(&old)?;
                    }
                    skipped += 1;
                    continue;
                }
            }
            // History pages may contain ten-player summaries without full details.
            // Only a successful detail request can promote a new entry to complete.
            record["complete"] = json!(false);
            {
                match archive_get(&http, &format!("/lol-match-history/v1/games/{}", record["gameId"].as_str().unwrap()), "读取单局详情").await {
                    Ok(detail) => match make_record(detail, owner, &region, &player_name) {
                        Ok(full) => record = full,
                        Err(_) => failures.push(json!({"gameId":record["gameId"],"reason":"详情缺少本账号身份或统计，保留摘要"})),
                    },
                    Err(error) => failures.push(json!({"gameId":record["gameId"],"reason":error.to_string()})),
                }
            }
            if record["complete"] != true { incomplete += 1; }
            fill_timeline_metric(&http, &mut record).await;
            self.store.save(&record)?;
            saved += 1;
        }
        Ok(json!({ "at": now(), "success": failures.len() < games.len().max(1), "saved": saved, "skipped": skipped, "incomplete": incomplete, "failures": failures, "region": region, "playerName": player_name }))
    }

    pub fn start(this: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut last_account = String::new();
            let mut last_phase = String::new();
            let mut attempted = Instant::now() - Duration::from_secs(301);
            loop {
                let phase = this.phase();
                let account = this.state.get("league-client-main", "summoner", "me")
                    .and_then(|s| s["puuid"].as_str().map(str::to_owned)).unwrap_or_default();
                if !account.is_empty() && this.client.http().is_some()
                    && !matches!(phase.as_str(), "InProgress" | "ChampSelect" | "ReadyCheck" | "GameStart")
                    && (account != last_account || (last_phase == "InProgress" && phase != last_phase) || attempted.elapsed() >= Duration::from_secs(300)) {
                    attempted = Instant::now();
                    if account != last_account || last_phase == "InProgress" {
                        let _ = this.vault("autoSnapshot", &json!({"label":"连接或赛后自动快照"})).await;
                    }
                    last_account = account;
                    let _ = this.sync().await;
                }
                last_phase = phase;
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        });
    }
}

#[cfg(test)]
mod timeline_tests {
    use super::*;

    #[test]
    fn reads_only_a_real_ten_minute_participant_frame() {
        let data = json!({"frames": [
            {"timestamp": 540000, "participantFrames": {"2": {"minionsKilled": 70, "jungleMinionsKilled": 4}}},
            {"timestamp": 600010, "participantFrames": {"1": {"minionsKilled": 40, "jungleMinionsKilled": 0}, "2": {"minionsKilled": 82, "jungleMinionsKilled": 7}}},
            {"timestamp": 660000, "participantFrames": {"2": {"minionsKilled": 96, "jungleMinionsKilled": 8}}}
        ]});
        assert_eq!(cs_at_ten_minutes(&data, 2), Some(89));
        assert_eq!(cs_at_ten_minutes(&data, 3), None);
        assert_eq!(cs_at_ten_minutes(&json!({"frames": [
            {"timestamp": 540000, "participantFrames": {"2": {"minionsKilled": 70, "jungleMinionsKilled": 4}}}
        ]}), 2), None);
        assert_eq!(cs_at_ten_minutes(&json!({"frames": [
            {"timestamp": 660000, "participantFrames": {"2": {"minionsKilled": 96, "jungleMinionsKilled": 8}}}
        ]}), 2), None);
        let events = json!({"frames":[{"events":[
            {"type":"CHAMPION_KILL","victimId":1,"timestamp":180000},
            {"type":"CHAMPION_KILL","victimId":2,"timestamp":420000},
            {"type":"CHAMPION_KILL","victimId":2,"timestamp":900000}
        ]}]});
        assert_eq!(first_death_minute(&events, 2, 2, 1200), Some(7.0));
        assert_eq!(first_death_minute(&events, 3, 0, 1200), Some(20.0));
        assert_eq!(first_death_minute(&events, 3, 1, 1200), None);
    }
}
