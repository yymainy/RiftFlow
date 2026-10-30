//! `respawn-timer-main` — 复活计时。
//!
//! Port of `src/main/shards/respawn-timer/` (`index.ts`, `respawn-timer-controller.ts`,
//! `state.ts`, `context.ts`).
//!
//! The respawn data does **not** come from the LCU. It comes from the in-game
//! *Live Client Data* HTTP server: `GET https://127.0.0.1:2999/liveclientdata/playerlist`,
//! polled every 1000 ms (`RESPAWN_TIMER_POLL_INTERVAL`, `respawn-timer/context.ts:9`),
//! with the game's self-signed certificate accepted (the original's
//! `https.Agent({ rejectUnauthorized: false })`, `game-client/index.ts:53-58`; the
//! base URL is `GAME_CLIENT_BASE_URL`, `game-client/context.ts:13`). The shard only
//! ever reads two fields off the matched player: `isDead` and `respawnTimer`.
//!
//! The synced surface is `respawn-timer-main:state` prop `info` = `{ timeLeft,
//! totalTime, isDead }`, which `SidebarFixed.vue:4-7` renders as the main window's
//! circular countdown (`v-if="rts.settings.enabled && rts.info.isDead"`). The
//! always-on-top cd-timer window is a different feature (`ongoing-game-main`
//! summoner-spell cooldowns) and reads nothing here.
//!
//! There are no shard IPC methods: `RespawnTimerMain` registers none (`index.ts:66-73`
//! only applies settings and starts the watcher), and the renderer's only call is
//! `settingUtils.set('respawn-timer-main', 'enabled', …)`, which is served by the
//! generic `setting-factory-main/set` in `handlers.rs` and broadcast on `SettingsHub`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Instant;

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::LcuHttp;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

use crate::settings_registry::SettingsHub;

pub const NAMESPACE: &str = "respawn-timer-main";
const LEAGUE: &str = "league-client-main";

/// `GAME_CLIENT_BASE_URL` minus the scheme — `LcuHttp::new` builds
/// `https://127.0.0.1:<port>` from a port, which is exactly the live client data
/// server, and reuses its `danger_accept_invalid_certs(true)`.
const GAME_CLIENT_PORT: i64 = 2999;

/// The same server, for the cd-timer's game clock (`aux_windows.rs`).
pub(crate) const LIVE_CLIENT_DATA_PORT: i64 = GAME_CLIENT_PORT;

/// `GameClientHttpApiAxiosHelper.getLiveClientDataPlayerList` (`game-client/index.ts:10`).
const PLAYER_LIST_PATH: &str = "/liveclientdata/playerlist";

/// `RESPAWN_TIMER_POLL_INTERVAL`
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// `_queryRespawnTime`'s `catch {}` threw every failure away, which left the widget
/// frozen with no explanation. The reason is still reported, just not every second.
const REPORT_EVERY: usize = 10;

/// `RespawnTimerState.info` (`state.ts:16-24`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RespawnInfo {
    pub time_left: f64,
    pub total_time: f64,
    pub is_dead: bool,
}

impl RespawnInfo {
    /// The keys `state_bootstrap.rs` declares for `respawn-timer-main:state` `info`,
    /// i.e. what `useRespawnTimerStore().info` (`store.ts:9-13`) expects.
    pub fn to_json(&self) -> Value {
        json!({
            "timeLeft": self.time_left,
            "totalTime": self.total_time,
            "isDead": self.is_dead,
        })
    }
}

/// What the gameflow reaction decided, i.e. `_start`/`_stopRespawnTimerPoll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollAction {
    Start,
    Stop,
    Idle,
}

/// `_watchGameflow`'s reaction body (`respawn-timer-controller.ts:33-47`). Anything
/// that is not `InProgress` stops the poll; `InProgress` only starts it when enabled.
pub fn poll_action(phase: &str, enabled: bool) -> PollAction {
    if phase == "InProgress" {
        if enabled { PollAction::Start } else { PollAction::Idle }
    } else {
        PollAction::Stop
    }
}

/// `applyEnabledSettingSideEffect` (`respawn-timer-controller.ts:20-26`). Note it
/// does *not* stop when the game is over and the setting is still on — the reaction
/// above is what covers that case, and the shard runs both.
pub fn enabled_action(phase: &str, enabled: bool) -> PollAction {
    if enabled && phase == "InProgress" {
        PollAction::Start
    } else if !enabled {
        PollAction::Stop
    } else {
        PollAction::Idle
    }
}

/// `@shared/utils/name.ts:13-23`. Both the `gameName`/`tagLine` pair and the
/// fallbacks are truthiness checks, so an empty string counts as absent — which is
/// the 国服 case (`summoner.ts:4` records that `gameName` is empty on the old ID
/// system), leaving `displayName`/`internalName` to do the matching.
pub fn riot_id_of(me: &Value) -> String {
    if !me.is_object() {
        return "-".to_string();
    }

    if let (Some(game_name), Some(tag_line)) = (non_empty(me, "gameName"), non_empty(me, "tagLine")) {
        return format!("{game_name}#{tag_line}");
    }

    non_empty(me, "displayName")
        .or_else(|| non_empty(me, "internalName"))
        .unwrap_or("-")
        .to_string()
}

/// The three-tier `playerList.find` of `respawn-timer-controller.ts:63-73`,
/// quirks included: a player carrying a non-empty `riotId` is judged on that alone,
/// so a non-matching id does **not** fall through to the legacy `summonerName`
/// comparison. `summonerName(name)` with no tag line is the identity, so tiers two
/// and three are both plain string comparisons; the last one keeps `undefined ===
/// undefined` matching, which the original also allowed.
pub fn find_self_player<'a>(players: &'a [Value], me: &Value) -> Option<&'a Value> {
    let target = riot_id_of(me);
    let my_internal_name = raw_str(me, "internalName");

    players.iter().find(|player| {
        if let Some(riot_id) = non_empty(player, "riotId") {
            return riot_id == target;
        }

        if let Some(name) = non_empty(player, "summonerName") {
            return name == target;
        }

        raw_str(player, "summonerName") == my_internal_name
    })
}

/// `respawn-timer-controller.ts:76-86`. `totalTime` is latched only on the
/// alive→dead edge, so it keeps the full respawn duration while the counter runs
/// down and stays at that value after the respawn until the next death.
pub fn respawn_info_after(player: &Value, previous: &RespawnInfo) -> RespawnInfo {
    let is_dead = player.get("isDead").and_then(Value::as_bool).unwrap_or(false);
    let timer = player.get("respawnTimer").and_then(Value::as_f64).unwrap_or(0.0);

    let total_time = if !previous.is_dead && is_dead { timer } else { previous.total_time };

    RespawnInfo { time_left: timer, total_time, is_dead }
}

/// `RespawnTimerItem.vue:62-68` — the shipped renderer formats the number itself;
/// this exists so the death log quotes exactly what the sidebar shows.
pub fn format_countdown(time_left: f64) -> String {
    let clamped = time_left.max(0.0);

    if clamped > 99.0 {
        return "99+".to_string();
    }

    format!("{}", round_half_away(clamped))
}

/// The circle's `percentage` (`RespawnTimerItem.vue:54-60`).
pub fn progress_percentage(time_left: f64, total_time: f64) -> f64 {
    let total = total_time.max(0.0);

    if total == 0.0 {
        return 0.0;
    }

    (time_left.max(0.0) / total * 100.0).min(100.0)
}

/// JS `toFixed(0)` rounds half away from zero, while Rust's `{:.0}` rounds half to
/// even — `0.5` has to print `1`, not `0`.
fn round_half_away(value: f64) -> i64 {
    if value >= 0.0 { (value + 0.5).floor() as i64 } else { (value - 0.5).ceil() as i64 }
}

fn non_empty<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|text| !text.is_empty())
}

fn raw_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

#[derive(Debug, Clone)]
enum Signal {
    /// `/lol-gameflow/v1/gameflow-phase`, whose payload carries the new phase
    Phase(String),
    /// `settings.enabled` changed — the original ran the side effect *and* the
    /// reaction, so both decisions are applied
    SettingChanged,
    /// boot, like the reaction's `fireImmediately: true`
    Reevaluate,
}

/// `_isStarted` + `_timerId`, plus the state the poll latches on.
#[derive(Default)]
struct Watch {
    polling: bool,
    next_poll: Option<Instant>,
    info: RespawnInfo,
    /// consecutive polls that produced no usable answer
    silent: usize,
}

pub struct RespawnTimer {
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    settings: Arc<SettingFactory>,
    sender: UnboundedSender<Signal>,
    /// the game's own HTTP server, not the LCU's — see the module header
    game_client: LcuHttp,
}

/// `onInit` — the gameflow reaction, the settings broadcast, and the deciding task.
pub fn start(
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    // kept for symmetry with the sibling shards; this shard exposes no IPC methods
    _ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    hub: &Arc<SettingsHub>,
) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let this = Arc::new(RespawnTimer {
        league_client,
        state_sync,
        settings,
        sender: sender.clone(),
        // the live client data server takes no credentials (`game-client/index.ts`
        // sends none), so the LCU client's empty `Authorization` stays unused; that
        // client also caps a request at `REQUEST_TIMEOUT_MS` and retries GETs twice,
        // neither of which the original's bare axios instance did.
        game_client: LcuHttp::new(GAME_CLIENT_PORT, String::new()),
    });

    let sender_for_phase = sender.clone();
    this.league_client.emitter().on("/lol-gameflow/v1/gameflow-phase", move |payload, _params| {
        let phase = payload
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let _ = sender_for_phase.send(Signal::Phase(phase));
    })?;

    let mut changes = hub.subscribe();
    let for_settings = this.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok((namespace, key, _value)) = changes.recv().await {
            if namespace == NAMESPACE && key == "enabled" {
                let _ = for_settings.sender.send(Signal::SettingChanged);
            }
        }
    });

    let _ = sender.send(Signal::Reevaluate);

    tauri::async_runtime::spawn(this.run(receiver));
    Ok(())
}

impl RespawnTimer {
    async fn run(self: Arc<Self>, mut receiver: UnboundedReceiver<Signal>) {
        let mut watch = Watch::default();

        loop {
            let deadline = watch.next_poll;

            tokio::select! {
                biased;

                signal = receiver.recv() => match signal {
                    Some(Signal::Phase(phase)) => {
                        self.apply(poll_action(&phase, self.enabled()), &mut watch);
                    }
                    Some(Signal::SettingChanged) => {
                        let phase = self.phase();
                        self.apply(enabled_action(&phase, self.enabled()), &mut watch);
                        self.apply(poll_action(&phase, self.enabled()), &mut watch);
                    }
                    Some(Signal::Reevaluate) => {
                        let phase = self.phase();
                        self.apply(poll_action(&phase, self.enabled()), &mut watch);
                    }
                    None => break,
                },

                _ = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.poll(&mut watch).await;
                    // re-armed after the request returns, so a slow game client
                    // stretches the period instead of stacking requests the way
                    // `setInterval` could
                    watch.next_poll = Some(Instant::now() + POLL_INTERVAL);
                }
            }
        }
    }

    fn apply(&self, action: PollAction, watch: &mut Watch) {
        match action {
            PollAction::Start => self.start_polling(watch),
            PollAction::Stop => self.stop_polling(watch),
            PollAction::Idle => {}
        }
    }

    /// `_startRespawnTimerPoll` — one immediate query, then the interval.
    fn start_polling(&self, watch: &mut Watch) {
        if watch.polling {
            return;
        }

        eprintln!("[respawn-timer] 开始轮询游戏内 live client data");
        watch.polling = true;
        watch.silent = 0;
        watch.next_poll = Some(Instant::now());
    }

    /// `_stopRespawnTimerPoll`, including the `info` reset it does *after* the
    /// not-started guard — a stop while idle must not clobber the current state.
    fn stop_polling(&self, watch: &mut Watch) {
        if !watch.polling {
            return;
        }

        eprintln!("[respawn-timer] 停止轮询");
        watch.polling = false;
        watch.next_poll = None;
        watch.silent = 0;
        watch.info = RespawnInfo::default();
        self.push(watch.info);
    }

    /// `_queryRespawnTime`.
    async fn poll(&self, watch: &mut Watch) {
        let Some(me) = self.league("summoner", "me").filter(|value| value.is_object()) else {
            // `logger.warn('Seems like summoner info is not loaded')`
            self.report(watch, "召唤师信息尚未加载");
            return;
        };

        match self.game_client.get(PLAYER_LIST_PATH).await {
            Ok(Value::Array(players)) => {
                let Some(player) = find_self_player(&players, &me) else {
                    let who = riot_id_of(&me);
                    self.report(watch, &format!("玩家列表中没有 {who}"));
                    return;
                };

                let previous = watch.info;
                let next = respawn_info_after(player, &previous);

                if next.is_dead && !previous.is_dead {
                    eprintln!(
                        "[respawn-timer] 阵亡，复活倒计时 {}（剩余 {:.0}%）",
                        format_countdown(next.time_left),
                        progress_percentage(next.time_left, next.total_time),
                    );
                }

                watch.info = next;
                watch.silent = 0;
                self.push(next);
            }
            Ok(other) => self.report(watch, &format!("响应不是玩家列表（{}）", kind_of(&other))),
            Err(error) => self.report(watch, &format!("{PLAYER_LIST_PATH} 不可达: {error}")),
        }
    }

    /// The honest version of the original's silent `catch {}`: a timer with no data
    /// keeps saying so instead of showing invented numbers.
    fn report(&self, watch: &mut Watch, reason: &str) {
        watch.silent += 1;

        if watch.silent == 1 || watch.silent % REPORT_EVERY == 0 {
            eprintln!("[respawn-timer] 复活计时暂无数据（{reason}），已连续 {} 次", watch.silent);
        }
    }

    fn push(&self, info: RespawnInfo) {
        if let Err(error) = self.state_sync.set(NAMESPACE, "state", "info", info.to_json()) {
            eprintln!("[respawn-timer] 状态推送失败: {error}");
        }
    }

    fn enabled(&self) -> bool {
        self.settings
            .get_service(NAMESPACE)
            .map(|service| service.get("enabled"))
            // `index.ts:44` registers `default: false`, which differs from the
            // `flag("enabled", true)` in `settings_registry.rs`; only the fallback
            // path can disagree
            .unwrap_or(Value::Null)
            .as_bool()
            .unwrap_or(false)
    }

    fn phase(&self) -> String {
        self.league("gameflow", "phase")
            .as_ref()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    fn league(&self, state_id: &str, path: &str) -> Option<Value> {
        self.state_sync.get(LEAGUE, state_id, path)
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "布尔值",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(game: &str, tag: &str, display: &str, internal: &str) -> Value {
        json!({ "gameName": game, "tagLine": tag, "displayName": display, "internalName": internal })
    }

    #[test]
    fn riot_id_prefers_the_game_name_and_tag() {
        assert_eq!(riot_id_of(&me("RiftFlow", "NA1", "Display", "riftflow")), "RiftFlow#NA1");
    }

    #[test]
    fn an_empty_tag_line_falls_back_like_the_old_id_system() {
        assert_eq!(riot_id_of(&me("RiftFlow", "", "Display", "riftflow")), "Display");
        assert_eq!(riot_id_of(&me("", "", "", "riftflow")), "riftflow");
        assert_eq!(riot_id_of(&me("", "", "", "")), "-");
        assert_eq!(riot_id_of(&Value::Null), "-");
    }

    #[test]
    fn self_is_matched_on_the_riot_id() {
        let players = json!([
            { "riotId": "Other#EUW" },
            { "riotId": "RiftFlow#NA1" },
        ]);
        let found = find_self_player(players.as_array().unwrap(), &me("RiftFlow", "NA1", "", "")).unwrap();
        assert_eq!(found["riotId"], "RiftFlow#NA1");
    }

    /// The original `return`s from the first tier as soon as `riotId` is present, so
    /// a legacy name on the same row never gets a chance.
    #[test]
    fn a_mismatching_riot_id_does_not_fall_through_to_the_name() {
        let players = json!([{ "riotId": "Someone#6666", "summonerName": "RiftFlow" }]);
        assert!(find_self_player(players.as_array().unwrap(), &me("", "", "RiftFlow", "RiftFlow")).is_none());
    }

    #[test]
    fn an_empty_riot_id_falls_back_to_the_summoner_name() {
        let players = json!([{ "riotId": "", "summonerName": "RiftFlow" }]);
        let found = find_self_player(players.as_array().unwrap(), &me("", "", "RiftFlow", "riftflow")).unwrap();
        assert_eq!(found["summonerName"], "RiftFlow");
    }

    #[test]
    fn nameless_rows_match_on_the_internal_name() {
        let players = json!([{ "team": "TEAM1" }]);
        assert!(find_self_player(players.as_array().unwrap(), &me("", "", "", "riftflow")).is_none());

        let players = json!([{ "summonerName": "riftflow" }]);
        assert!(find_self_player(players.as_array().unwrap(), &me("", "", "", "riftflow")).is_some());
    }

    #[test]
    fn total_time_latches_on_the_death_edge() {
        let dead = json!({ "isDead": true, "respawnTimer": 38.0 });
        let info = respawn_info_after(&dead, &RespawnInfo::default());

        assert_eq!(info, RespawnInfo { time_left: 38.0, total_time: 38.0, is_dead: true });
    }

    /// The count-down keeps the latched total, and the stale total survives the
    /// respawn until the next death — both as in the original.
    #[test]
    fn total_time_holds_while_the_timer_runs_down() {
        let previous = RespawnInfo { time_left: 38.0, total_time: 38.0, is_dead: true };

        let next = respawn_info_after(&json!({ "isDead": true, "respawnTimer": 12.4 }), &previous);
        assert_eq!(next, RespawnInfo { time_left: 12.4, total_time: 38.0, is_dead: true });

        let alive = respawn_info_after(&json!({ "isDead": false, "respawnTimer": 0.0 }), &next);
        assert_eq!(alive, RespawnInfo { time_left: 0.0, total_time: 38.0, is_dead: false });

        // dying again re-latches
        let dead_again = respawn_info_after(&json!({ "isDead": true, "respawnTimer": 51.0 }), &alive);
        assert_eq!(dead_again.total_time, 51.0);
    }

    #[test]
    fn countdown_caps_at_99_and_clamps_negatives() {
        assert_eq!(format_countdown(100.0), "99+");
        // the `> 99` test happens before the rounding, so 99.4 is already "99+"
        assert_eq!(format_countdown(99.4), "99+");
        assert_eq!(format_countdown(99.0), "99");
        assert_eq!(format_countdown(-3.0), "0");
    }

    #[test]
    fn countdown_rounds_half_away_from_zero() {
        assert_eq!(format_countdown(0.5), "1");
        assert_eq!(format_countdown(2.5), "3");
        assert_eq!(format_countdown(2.4), "2");
    }

    #[test]
    fn progress_is_clamped_and_zero_without_a_total() {
        assert_eq!(progress_percentage(38.0, 38.0), 100.0);
        assert_eq!(progress_percentage(19.0, 38.0), 50.0);
        assert_eq!(progress_percentage(60.0, 38.0), 100.0);
        assert_eq!(progress_percentage(10.0, 0.0), 0.0);
        assert_eq!(progress_percentage(-5.0, 38.0), 0.0);
    }

    /// The prop names `state_bootstrap.rs` declares for `respawn-timer-main` `info`,
    /// which `useRespawnTimerStore().info` (`store.ts:9-13`) reads back.
    #[test]
    fn info_json_uses_the_declared_keys() {
        let info = RespawnInfo { time_left: 12.5, total_time: 38.0, is_dead: true }.to_json();
        let object = info.as_object().unwrap();

        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["isDead", "timeLeft", "totalTime"]);

        assert_eq!(object["isDead"], json!(true));
        assert_eq!(object["timeLeft"], json!(12.5));
        assert_eq!(object["totalTime"], json!(38.0));
        assert_eq!(RespawnInfo::default().to_json()["timeLeft"], json!(0.0));
    }

    #[test]
    fn only_in_progress_starts_the_poll() {
        assert_eq!(poll_action("InProgress", true), PollAction::Start);
        assert_eq!(poll_action("InProgress", false), PollAction::Idle);
        for phase in ["None", "ChampSelect", "Completed", ""] {
            assert_eq!(poll_action(phase, true), PollAction::Stop);
            assert_eq!(poll_action(phase, false), PollAction::Stop);
        }
    }

    /// The setting side effect leaves an on-but-not-in-game combination alone, which
    /// is why the shard also runs the reaction.
    #[test]
    fn the_enabled_side_effect_starts_and_stops() {
        assert_eq!(enabled_action("InProgress", true), PollAction::Start);
        assert_eq!(enabled_action("ChampSelect", true), PollAction::Idle);
        assert_eq!(enabled_action("ChampSelect", false), PollAction::Stop);
        assert_eq!(enabled_action("InProgress", false), PollAction::Stop);
    }
}
