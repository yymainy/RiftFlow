//! `auto-gameflow-main` — 自动接受 / 自动匹配 / 再来一局 / 自动重连。
//!
//! Ports `action-controller.ts`, `lobby-flow-controller.ts`,
//! `matchmaking-controller.ts` and the `activityStartStatus` getter of `state.ts`.
//!
//! One task owns every timer and every decision: LCU events and setting changes
//! arrive on one channel, deadlines are computed from that channel's state, and all
//! network calls are awaited inline. That is what replaces the MobX reactions, and
//! it means a cancel path cannot leave a timer behind.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Instant;

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::RequestConfig;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

use crate::settings_registry::SettingsHub;

pub const NAMESPACE: &str = "auto-gameflow-main";
const LEAGUE: &str = "league-client-main";

/// `AUTO_GAMEFLOW_PLAY_AGAIN_*`
const PLAY_AGAIN_WAIT_FOR_STATS: Duration = Duration::from_millis(10_000);
const PLAY_AGAIN_WAIT_FOR_BALLOT: Duration = Duration::from_millis(3_250);
const PLAY_AGAIN_BUFFER: Duration = Duration::from_millis(1_575);
const RECONNECT_DELAY: Duration = Duration::from_millis(10_000);
const CHAT_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
enum Signal {
    Phase(String),
    ReadyCheck(Value),
    /// an input of `activityStartStatus` changed
    ActivityInputsChanged,
    /// `lobby.receivedInvitations`, `chat.me` and the two invitation settings
    InvitationsChanged,
    /// `/lol-chat/v1/friends/:id`, which needs the event type as well as the data
    Friend { data: Value, event_type: String },
    HonorBallot,
    /// champ-select session / gameflow session / the champion-select conversation
    AramInputsChanged,
    SettingChanged(String),
    Command(Command),
}

#[derive(Debug, Clone)]
enum Command {
    CancelAutoAccept,
    CancelAutoMatchmaking,
}

#[derive(Default)]
struct Timers {
    accept: Option<Instant>,
    play_again: Option<Instant>,
    reconnect: Option<Instant>,
    search: Option<Instant>,
    /// the per-second "matching in n seconds" party-chat notice while a search is
    /// scheduled, which the original ran on its own interval
    chat_countdown: bool,
    /// `penaltyTime` captured when the search state first appeared
    penalty_time: f64,
}

pub struct AutoGameflow {
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    sender: UnboundedSender<Signal>,
}

/// `onInit` — listeners, the IPC surface, and the deciding task.
pub fn start(
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    hub: &Arc<SettingsHub>,
) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let this = Arc::new(AutoGameflow {
        league_client,
        state_sync,
        ipc,
        settings,
        sender: sender.clone(),
    });

    register_ipc(&this)?;
    watch_events(&this)?;

    let mut changes = hub.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok((namespace, key, _value)) = changes.recv().await {
            if namespace == NAMESPACE {
                let _ = sender.send(Signal::SettingChanged(key));
            }
        }
    });

    tauri::async_runtime::spawn(this.run(receiver));
    Ok(())
}

fn register_ipc(this: &Arc<AutoGameflow>) -> anyhow::Result<()> {
    let sender = this.sender.clone();
    this.ipc.on_call(NAMESPACE, "cancelAutoAccept", riftflow_ipc::handler(
        move |_ctx, _args| {
            let sender = sender.clone();
            async move {
                let _ = sender.send(Signal::Command(Command::CancelAutoAccept));
                Ok(Value::Null)
            }
        },
    ))?;

    let sender = this.sender.clone();
    this.ipc.on_call(NAMESPACE, "cancelAutoMatchmaking", riftflow_ipc::handler(
        move |_ctx, _args| {
            let sender = sender.clone();
            async move {
                let _ = sender.send(Signal::Command(Command::CancelAutoMatchmaking));
                Ok(Value::Null)
            }
        },
    ))?;

    let sync = this.state_sync.clone();
    this.ipc.on_call(
        NAMESPACE,
        "setFriendsToBeInvited",
        riftflow_ipc::handler(move |_ctx, args| {
            let sync = sync.clone();
            async move {
                let puuids = args
                    .first()
                    .cloned()
                    .filter(|value| value.is_array())
                    .unwrap_or_else(|| json!([]));
                let _ = sync.set(NAMESPACE, "state", "friendsToBeInvited", puuids);
                Ok(Value::Null)
            }
        }),
    )?;

    Ok(())
}

/// The event half of every controller: the socket pushes, the task decides.
fn watch_events(this: &Arc<AutoGameflow>) -> anyhow::Result<()> {
    let emitter = this.league_client.emitter();

    let sender = this.sender.clone();
    emitter.on("/lol-gameflow/v1/gameflow-phase", move |payload, _params| {
        let phase = payload
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let _ = sender.send(Signal::Phase(phase));
    })?;

    let sender = this.sender.clone();
    emitter.on("/lol-matchmaking/v1/ready-check", move |payload, _params| {
        let data = payload.get("data").cloned().unwrap_or(Value::Null);
        let _ = sender.send(Signal::ReadyCheck(data));
    })?;

    for uri in [
        "/lol-lobby/v2/lobby",
        "/lol-matchmaking/v1/search",
        "/lol-summoner/v1/current-summoner",
    ] {
        let sender = this.sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(Signal::ActivityInputsChanged);
        })?;
    }

    for uri in [
        "/lol-lobby/v2/received-invitations",
        "/lol-chat/v1/me",
    ] {
        let sender = this.sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(Signal::InvitationsChanged);
        })?;
    }

    // a placeholder pattern: the matcher resolves `/lol-chat/v1/friends/<id>`
    let sender = this.sender.clone();
    emitter.on("/lol-chat/v1/friends/:id", move |payload, _params| {
        let data = payload.get("data").cloned().unwrap_or(Value::Null);
        let event_type = payload
            .get("eventType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let _ = sender.send(Signal::Friend { data, event_type });
    })?;

    let sender = this.sender.clone();
    emitter.on("/lol-honor-v2/v1/ballot", move |_payload, _params| {
        let _ = sender.send(Signal::HonorBallot);
    })?;

    for uri in ["/lol-champ-select/v1/session", "/lol-chat/v1/conversations/:id"] {
        let sender = this.sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(Signal::AramInputsChanged);
        })?;
    }

    Ok(())
}

impl AutoGameflow {
    fn setting(&self, key: &str) -> Value {
        self.settings
            .get_service(NAMESPACE)
            .map(|service| service.get(key))
            .unwrap_or(Value::Null)
    }

    fn flag(&self, key: &str) -> bool {
        self.setting(key).as_bool().unwrap_or(false)
    }

    fn seconds(&self, key: &str) -> f64 {
        self.setting(key).as_f64().unwrap_or_default()
    }

    fn push(&self, path: &str, value: Value) {
        let _ = self.state_sync.set(NAMESPACE, "state", path, value);
    }

    fn league(&self, state_id: &str, path: &str) -> Option<Value> {
        self.state_sync.get(LEAGUE, state_id, path)
    }

    fn will_accept_at(&self) -> i64 {
        self.state_sync
            .get(NAMESPACE, "state", "willAcceptAt")
            .and_then(|value| value.as_i64())
            .unwrap_or(-1)
    }

    fn will_search_match_at(&self) -> i64 {
        self.state_sync
            .get(NAMESPACE, "state", "willSearchMatchAt")
            .and_then(|value| value.as_i64())
            .unwrap_or(-1)
    }

    // ---- `activityStartStatus` -------------------------------------------------

    fn activity_start_status(&self) -> String {
        let Some(lobby) = self.league("lobby", "lobby").filter(|value| !value.is_null()) else {
            return "unavailable".to_string();
        };

        let custom_game = self
            .league("gameflow", "session")
            .and_then(|session| {
                session
                    .pointer("/gameData/isCustomGame")
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false);
        if custom_game {
            return "unavailable".to_string();
        }

        let members = lobby.get("members").and_then(Value::as_array);
        let my_puuid = self
            .league("summoner", "me")
            .and_then(|me| me.get("puuid").and_then(Value::as_str).map(str::to_string));

        let mine = members.into_iter().flatten().find(|member| {
            member.get("puuid").and_then(Value::as_str) == my_puuid.as_deref()
        });

        match mine {
            None => return "unavailable".to_string(),
            Some(member)
                if !member
                    .get("isLeader")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) =>
            {
                return "not-the-leader".to_string();
            }
            Some(_) => {}
        }

        let penalty_waiting = self
            .league("matchmaking", "search")
            .and_then(|search| {
                search
                    .get("errors")
                    .and_then(Value::as_array)
                    .map(|errors| {
                        errors
                            .iter()
                            .filter_map(|entry| {
                                entry.get("penaltyTimeRemaining").and_then(Value::as_f64)
                            })
                            .fold(f64::NEG_INFINITY, f64::max)
                            > 0.0
                    })
            })
            .unwrap_or(false);
        if penalty_waiting {
            return "waiting-for-penalty-time".to_string();
        }

        if self.flag("autoMatchmakingWaitForInvitees") {
            let pending = lobby
                .get("invitations")
                .and_then(Value::as_array)
                .is_some_and(|invitations| {
                    invitations.iter().any(|invitation| {
                        invitation.get("state").and_then(Value::as_str) == Some("Pending")
                    })
                });
            if pending {
                return "waiting-for-invitees".to_string();
            }
        }

        let member_count = lobby.get("members").and_then(Value::as_array).map_or(0, Vec::len);
        if member_count < self.seconds("autoMatchmakingMinimumMembers") as usize {
            return "insufficient-members".to_string();
        }

        if lobby
            .get("canStartActivity")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "can-start-activity".to_string()
        } else {
            "cannot-start-activity".to_string()
        }
    }

    // ---- LCU calls -------------------------------------------------------------

    async fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<(), String> {
        let Some(http) = self.league_client.http() else {
            return Err("LCU 未连接".to_string());
        };

        let response = http
            .request(RequestConfig {
                method: method.to_string(),
                url: url.to_string(),
                data: body,
                params: Map::new(),
                headers: Map::new(),
                response_type: None,
            })
            .await
            .map_err(|error| error.to_string())?;

        if response.status >= 400 {
            return Err(format!("{method} {url} → {}", response.status));
        }

        Ok(())
    }

    async fn accept_match(&self) {
        if let Err(error) = self
            .call("POST", "/lol-matchmaking/v1/ready-check/accept", None)
            .await
        {
            // the UI toasts this event
            self.ipc
                .send_event(NAMESPACE, "error-accept-match", &[json!(error)]);
            eprintln!("[auto-gameflow] 接受对局失败: {error}");
        }
        self.clear_auto_accept();
    }

    async fn play_again(&self) {
        println!("[auto-gameflow] 再来一局：返回房间");
        if let Err(error) = self.call("POST", "/lol-lobby/v2/play-again", None).await {
            eprintln!("[auto-gameflow] 再来一局失败: {error}");
        }
    }

    async fn reconnect(&self) {
        println!("[auto-gameflow] 尝试重新连接");
        if let Err(error) = self.call("POST", "/lol-gameflow/v1/reconnect", None).await {
            eprintln!("[auto-gameflow] 重新连接失败: {error}");
        }
    }

    async fn start_matchmaking(&self) {
        if let Err(error) = self
            .call("POST", "/lol-lobby/v2/lobby/matchmaking/search", None)
            .await
        {
            self.ipc
                .send_event(NAMESPACE, "error-matchmaking", &[json!(error)]);
            eprintln!("[auto-gameflow] 开始匹配失败: {error}");
        }
    }

    async fn cancel_search(&self) {
        if let Err(error) = self
            .call("DELETE", "/lol-lobby/v2/lobby/matchmaking/search", None)
            .await
        {
            eprintln!("[auto-gameflow] 取消匹配失败: {error}");
        }
    }

    // ---- timers ----------------------------------------------------------------

    fn start_auto_accept(&self, timers: &mut Timers, delay: Duration) {
        timers.accept = Some(Instant::now() + delay);
        self.push("willAcceptAt", json!(now_ms() + delay.as_millis() as i64));
    }

    fn clear_auto_accept(&self) {
        self.push("willAcceptAt", json!(-1));
    }

    /// `cancelAutoAccept(reason)` — only logs when something was actually pending.
    fn cancel_auto_accept(&self, timers: &mut Timers, reason: &str) {
        if self.will_accept_at() > 0 {
            if timers.accept.take().is_some() {
                match reason {
                    "accepted" => println!("[auto-gameflow] 已接受对局"),
                    "declined" => println!("[auto-gameflow] 已拒绝本局"),
                    other => println!("[auto-gameflow] 自动接受已取消 - {other}"),
                }
            }
            self.clear_auto_accept();
        }
    }

    fn clear_auto_search(&self) {
        self.push("willSearchMatch", json!(false));
        self.push("willSearchMatchAt", json!(-1));
    }

    async fn cancel_auto_matchmaking(&self, timers: &mut Timers, reason: &str) {
        if !self
            .state_sync
            .get(NAMESPACE, "state", "willSearchMatch")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return;
        }

        timers.search = None;
        if timers.chat_countdown {
            timers.chat_countdown = false;
            self.send_chat_info(Some(reason)).await;
        }

        self.clear_auto_search();
        println!("[auto-gameflow] 自动匹配已取消 - {reason}");
    }

    /// `_watchAutoAccept` / `_watchAutoPlayAgain` / `_watchAutoReconnect`, keyed off
    /// the phase rather than off a mobx reaction.
    async fn on_phase(&self, phase: &str, timers: &mut Timers) {
        if self.flag("autoAcceptEnabled") {
            if phase == "ReadyCheck" {
                let delay = Duration::from_secs_f64(self.seconds("autoAcceptDelaySeconds"));
                self.start_auto_accept(timers, delay);
                println!(
                    "[auto-gameflow] ReadyCheck：将在 {} 秒后自动接受",
                    self.seconds("autoAcceptDelaySeconds")
                );
            } else {
                if timers.accept.is_some() {
                    println!("[auto-gameflow] 不在 ReadyCheck，取消即将自动接受");
                }
                self.cancel_auto_accept(timers, "不在 ReadyCheck 阶段");
            }
        } else {
            self.cancel_auto_accept(timers, "autoAcceptEnabled=false");
        }

        if self.flag("playAgainEnabled") {
            let delay = match phase {
                "WaitingForStats" => Some(PLAY_AGAIN_WAIT_FOR_STATS),
                "PreEndOfGame" => Some(PLAY_AGAIN_WAIT_FOR_BALLOT),
                "EndOfGame" => Some(PLAY_AGAIN_BUFFER),
                _ => None,
            };
            timers.play_again = delay.map(|delay| Instant::now() + delay);
        } else {
            timers.play_again = None;
        }

        if phase == "Reconnect" && self.flag("autoReconnectEnabled") {
            println!("[auto-gameflow] 稍后尝试重新连接");
            timers.reconnect = Some(Instant::now() + RECONNECT_DELAY);
        } else {
            timers.reconnect = None;
        }
    }

    async fn on_ready_check(&self, data: &Value, timers: &mut Timers) {
        let response = data.get("playerResponse").and_then(Value::as_str);
        if matches!(response, Some("Accepted") | Some("Declined")) {
            self.cancel_auto_accept(timers, &response.unwrap_or_default().to_lowercase());
        }
    }

    /// `_watchActivityStartStatus`.
    async fn on_activity_status(&self, timers: &mut Timers) {
        let status = self.activity_start_status();
        self.push("activityStartStatus", json!(status));

        if !self.flag("autoMatchmakingEnabled") {
            self.cancel_auto_matchmaking(timers, "normal").await;
            return;
        }

        match status.as_str() {
            "can-start-activity" => {
                let delay = Duration::from_secs_f64(self.seconds("autoMatchmakingDelaySeconds"));
                timers.search = Some(Instant::now() + delay);
                timers.chat_countdown = true;
                self.push("willSearchMatch", json!(true));
                self.push(
                    "willSearchMatchAt",
                    json!(now_ms() + delay.as_millis() as i64),
                );
                println!(
                    "[auto-gameflow] 将在 {} 秒后开始匹配",
                    self.seconds("autoMatchmakingDelaySeconds")
                );
                self.send_chat_info(None).await;
            }
            "unavailable" | "cannot-start-activity" => {
                self.cancel_auto_matchmaking(timers, "normal").await;
            }
            other => {
                self.cancel_auto_matchmaking(timers, other).await;
            }
        }
    }

    /// `_watchRematchCancel` — cancel the queue once it has run long enough.
    async fn on_search_changed(&self, timers: &mut Timers) {
        let search = self.league("matchmaking", "search").filter(|value| !value.is_null());

        let Some(search) = search else {
            timers.penalty_time = 0.0;
            return;
        };

        let penalty = search
            .pointer("/lowPriorityData/penaltyTime")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        if timers.penalty_time == 0.0 {
            timers.penalty_time = penalty;
        }

        let strategy = self
            .setting("autoMatchmakingRematchStrategy")
            .as_str()
            .unwrap_or("never")
            .to_string();
        let time_in_queue = search
            .get("timeInQueue")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let estimated = search
            .get("estimatedQueueTime")
            .and_then(Value::as_f64)
            .unwrap_or_default();

        let overdue = match strategy.as_str() {
            "fixed-duration" => {
                time_in_queue - timers.penalty_time >= self.seconds("autoMatchmakingRematchFixedDuration")
            }
            "estimated-duration" => time_in_queue - timers.penalty_time >= estimated,
            _ => false,
        };

        let searching = search.get("searchState").and_then(Value::as_str) == Some("Searching")
            && search
                .get("isCurrentlyInQueue")
                .and_then(Value::as_bool)
                .unwrap_or(false);

        if searching && overdue {
            self.cancel_search().await;
        }
    }

    async fn on_setting_changed(&self, key: &str, timers: &mut Timers) {
        match key {
            "autoAcceptEnabled" if !self.flag("autoAcceptEnabled") => {
                self.cancel_auto_accept(timers, "normal");
            }
            "autoMatchmakingEnabled" | "autoMatchmakingWaitForInvitees"
            | "autoMatchmakingMinimumMembers" => {
                self.on_activity_status(timers).await;
            }
            _ => {}
        }
    }

    /// `_sendAutoSearchMatchInfoInChat`, which only speaks in a party chat.
    async fn send_chat_info(&self, cancel: Option<&str>) {
        let Some(conversation) = self
            .league("chat", "conversations.customGame")
            .filter(|value| !value.is_null())
        else {
            return;
        };

        let Some(target_id) = conversation.get("id").cloned() else {
            return;
        };

        let message = match cancel {
            Some("normal") => self.localized("canceled"),
            Some("waiting-for-invitees") => self.localized("canceled-wait-for-invitees"),
            Some("not-the-leader") => self.localized("canceled-not-leader"),
            Some("waiting-for-penalty-time") => self.localized("canceled-wait-for-penalty"),
            Some(_) => self.localized("canceled"),
            None => {
                let remaining = (self.will_search_match_at() - now_ms()).max(0);
                let seconds = (remaining as f64 / 1000.0).round() as i64;
                self.localized_in(seconds)
            }
        };

        let url = format!("/lol-chat/v1/conversations/{target_id}/messages");
        let body = json!({
            "body": message,
            "fromPid": "",
            "fromSummonerId": 0,
            "id": target_id,
            "isHistorical": false,
            "timestamp": "",
            "type": "celebration",
        });

        if let Err(error) = self.call("POST", &url, Some(body)).await {
            // a failed notice must not break the flow
            eprintln!("[auto-gameflow] 队伍频道提示未发出: {error}");
        }
    }

    /// The five chat strings of `main.yaml` under `auto-gameflow-main`, in the two
    /// languages the main process registers.
    fn localized(&self, key: &str) -> String {
        let english = self.locale_is_english();
        let text = match (english, key) {
            (true, "canceled") => "Auto matchmaking canceled",
            (false, "canceled") => "自动匹配已取消",
            (true, "canceled-wait-for-invitees") => {
                "Auto matchmaking canceled, waiting for invitees"
            }
            (false, "canceled-wait-for-invitees") => "自动匹配已取消，等待被邀请者",
            (true, "canceled-not-leader") => {
                "Auto matchmaking canceled, you are not the room leader"
            }
            (false, "canceled-not-leader") => "自动匹配已取消，当前不是房间房主",
            (true, "canceled-wait-for-penalty") => {
                "Auto matchmaking canceled, waiting for penalty timer"
            }
            (false, "canceled-wait-for-penalty") => "自动匹配已取消，等待秒退计时器",
            (_, other) => other,
        };
        format!("[RiftFlow] {text}")
    }

    fn localized_in(&self, seconds: i64) -> String {
        if self.locale_is_english() {
            format!("[RiftFlow] Auto matchmaking in {seconds} seconds")
        } else {
            format!("[RiftFlow] 将在 {seconds} 秒后自动匹配")
        }
    }

    fn locale_is_english(&self) -> bool {
        self.settings
            .get_setting("app-common-main", "locale")
            .ok()
            .and_then(|locale| locale.as_str().map(str::to_string))
            .is_some_and(|locale| locale.starts_with("en"))
    }

    /// `_watchAutoInvitation`'s first reaction: leaving the lobby empties the
    /// pending-invite list.
    fn forget_invited_friends_if_lobby_gone(&self) {
        if self.league("lobby", "lobby").filter(|value| !value.is_null()).is_some() {
            return;
        }

        let pending = self
            .state_sync
            .get(NAMESPACE, "state", "friendsToBeInvited")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();

        if !pending.is_empty() {
            self.push("friendsToBeInvited", json!([]));
        }
    }

    /// `_watchAutoHandleInvitation` — one invitation per change, accept preferred.
    async fn handle_invitations(&self) {
        if !self.flag("autoHandleInvitationsEnabled") {
            return;
        }

        let invitations = self
            .league("lobby", "receivedInvitations")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        if invitations.is_empty() {
            return;
        }

        let availability = self
            .league("chat", "me")
            .and_then(|me| me.get("availability").and_then(Value::as_str).map(str::to_string));

        if self.flag("rejectInvitationWhenAway") && availability.as_deref() == Some("away") {
            println!("[auto-gameflow] 当前状态为 away，不处理邀请");
            return;
        }

        let strategies = self.setting("invitationHandlingStrategies");

        let mut candidates: Vec<(String, String)> = invitations
            .iter()
            .filter(|invitation| {
                invitation.get("state").and_then(Value::as_str) == Some("Pending")
                    && invitation
                        .get("canAcceptInvitation")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            })
            .map(|invitation| {
                let id = invitation
                    .get("invitationId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let game_type = invitation
                    .pointer("/gameConfig/inviteGameType")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let strategy = strategies
                    .get(game_type)
                    .or_else(|| strategies.get("<DEFAULT>"))
                    .and_then(Value::as_str)
                    .unwrap_or("ignore")
                    .to_string();
                (id, strategy)
            })
            .collect();

        // accept first, then decline, then whatever else — a stable sort, like `toSorted`
        candidates.sort_by_key(|(_, strategy)| match strategy.as_str() {
            "accept" => 0,
            "decline" => 1,
            _ => 2,
        });

        let Some((id, strategy)) = candidates.into_iter().next() else {
            return;
        };

        match strategy.as_str() {
            "accept" => {
                let url = format!("/lol-lobby/v2/received-invitations/{id}/accept");
                match self.call("POST", &url, None).await {
                    Ok(()) => println!("[auto-gameflow] 自动接受邀请 {id}"),
                    Err(error) => eprintln!("[auto-gameflow] 处理邀请失败: {error}"),
                }
            }
            "decline" => {
                let url = format!("/lol-lobby/v2/received-invitations/{id}/decline");
                match self.call("POST", &url, None).await {
                    Ok(()) => println!("[auto-gameflow] 自动拒绝邀请 {id}"),
                    Err(error) => eprintln!("[auto-gameflow] 处理邀请失败: {error}"),
                }
            }
            other => println!("[auto-gameflow] 按策略忽略邀请 {id} ({other})"),
        }
    }

    /// `_watchAutoSkipLeader` — hand the leader seat to someone else.
    async fn handle_skip_leader(&self, rng: &mut crate::rng::Rng) {
        if !self.flag("autoSkipLeaderEnabled") {
            return;
        }

        let Some(lobby) = self.league("lobby", "lobby").filter(|value| !value.is_null()) else {
            return;
        };

        if !lobby
            .pointer("/localMember/isLeader")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return;
        }

        let my_summoner_id = lobby.pointer("/localMember/summonerId").cloned();
        let members = lobby.get("members").and_then(Value::as_array).cloned().unwrap_or_default();

        let others: Vec<&Value> = members
            .iter()
            .filter(|member| {
                member.get("summonerId") != my_summoner_id.as_ref()
                    && !member
                        .get("isSpectator")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            })
            .collect();

        let pick_from = |ready: bool| -> Vec<Value> {
            others
                .iter()
                .filter(|member| {
                    member.get("ready").and_then(Value::as_bool).unwrap_or(false) == ready
                })
                .filter_map(|member| member.get("summonerId").cloned())
                .collect()
        };

        let ready = pick_from(true);
        let not_ready = pick_from(false);

        if ready.is_empty() && not_ready.is_empty() {
            return;
        }

        let pool = if ready.len() > not_ready.len() || !ready.is_empty() { &ready } else { &not_ready };
        // the original used an exclusive upper bound here and could never reach the
        // last candidate; this picks uniformly over the whole pool
        let Some(target) = pool.get(rng.index(pool.len())) else {
            return;
        };

        if let Some(conversation_id) = self.party_conversation_id() {
            self.chat_send(conversation_id, self.localized_skip_leader(), "celebration")
                .await;
        }

        let url = format!("/lol-lobby/v2/lobby/members/{target}/promote");
        if let Err(error) = self.call("POST", &url, None).await {
            eprintln!("[auto-gameflow] 转让房主失败: {error}");
        }
    }

    /// `_watchAutoInvitation` — invite a friend the moment they come online.
    async fn handle_friend_event(&self, data: &Value, event_type: &str) {
        let mut pending = self
            .state_sync
            .get(NAMESPACE, "state", "friendsToBeInvited")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();

        let puuid = data.get("puuid").and_then(Value::as_str).unwrap_or_default().to_string();

        let remove_pending = |pending: &mut Vec<Value>| {
            pending.retain(|entry| entry.as_str() != Some(puuid.as_str()));
        };

        if event_type == "Delete" {
            remove_pending(&mut pending);
            self.push("friendsToBeInvited", Value::Array(pending));
            return;
        }

        let lobby = self.league("lobby", "lobby");
        let in_list = pending.iter().any(|entry| entry.as_str() == Some(puuid.as_str()));
        let already_in_lobby = lobby
            .as_ref()
            .and_then(|lobby| lobby.get("members")?.as_array())
            .is_some_and(|members| {
                members.iter().any(|member| member.get("puuid").and_then(Value::as_str) == Some(&puuid))
            });

        if puuid.is_empty()
            || data.get("availability").and_then(Value::as_str) != Some("chat")
            || !lobby
                .as_ref()
                .and_then(|lobby| lobby.pointer("/localMember/allowedInviteOthers"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            || !in_list
            || already_in_lobby
        {
            return;
        }

        let name = format!(
            "{} #{}",
            data.get("gameName").and_then(Value::as_str).unwrap_or_default(),
            data.get("gameTag").and_then(Value::as_str).unwrap_or_default()
        );
        println!("[auto-gameflow] 邀请好友 {name}");

        let summoner_id = data.get("summonerId").cloned().unwrap_or(Value::Null);
        if let Err(error) = self
            .call(
                "POST",
                "/lol-lobby/v2/lobby/invitations",
                Some(json!([{ "toSummonerId": summoner_id }])),
            )
            .await
        {
            eprintln!("[auto-gameflow] 邀请失败: {error}");
        } else if let Some(conversation_id) = self.party_conversation_id() {
            self.chat_send(conversation_id, self.localized_invitation_sent(&name), "celebration")
                .await;
        }

        remove_pending(&mut pending);
        self.push("friendsToBeInvited", Value::Array(pending));
    }

    /// `AutoGameflowHonorController` — spend the vote pool, allies first.
    async fn handle_honor(&self, timers: &mut Timers, rng: &mut crate::rng::Rng) {
        let Some(ballot) = self.league("honor", "ballot").filter(|value| !value.is_null()) else {
            return;
        };

        let game_id = ballot.get("gameId").cloned().unwrap_or(Value::Null);
        if game_id.is_null() {
            return;
        }

        // a ballot on screen means the player is still in the end-of-game flow
        timers.play_again = None;

        if !self.flag("autoHonorEnabled") {
            return;
        }

        let votes = ballot
            .pointer("/votePool/votes")
            .and_then(Value::as_i64)
            .unwrap_or(0) as usize;

        let allies = eligible_puuids(&ballot, "eligibleAllies");
        let opponents = eligible_puuids(&ballot, "eligibleOpponents");

        let status = match self
            .call_and_read("GET", "/lol-lobby/v2/party/eog-status")
            .await
        {
            Ok(status) => status,
            Err(error) => {
                self.ipc
                    .send_event(NAMESPACE, "error-auto-honor", &[json!(error)]);
                eprintln!("[auto-gameflow] 读取结算状态失败: {error}");
                return;
            }
        };

        let lobby_members: Vec<String> = ["eogPlayers", "leftPlayers", "readyPlayers"]
            .iter()
            .flat_map(|key| {
                status
                    .get(*key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.as_str().map(str::to_string))
                    .collect::<Vec<String>>()
            })
            .collect();

        let mut candidates: Vec<String> = Vec::new();

        // 1. allies who were in the lobby, 2. the rest of the allies,
        // 3. opponents — each batch only fills what the previous one left over
        for pool in [
            allies
                .iter()
                .filter(|puuid| lobby_members.contains(puuid))
                .cloned()
                .collect::<Vec<String>>(),
            allies
                .iter()
                .filter(|puuid| !lobby_members.contains(puuid) && !candidates.contains(puuid))
                .cloned()
                .collect(),
            opponents
                .iter()
                .filter(|puuid| !lobby_members.contains(puuid) && !candidates.contains(puuid))
                .cloned()
                .collect(),
        ] {
            let remaining = votes.saturating_sub(candidates.len());
            if remaining == 0 || pool.is_empty() {
                continue;
            }
            for index in rng.distinct_indices(pool.len(), remaining) {
                candidates.push(pool[index].clone());
            }
        }

        // `AUTO_GAMEFLOW_HONOR_CATEGORY` holds one entry, so the original's
        // `randomInt(0, len)` always resolved to it
        for puuid in &candidates {
            if let Err(error) = self
                .call(
                    "POST",
                    "/lol-honor/v1/honor",
                    Some(json!({ "honorType": "HEART", "recipientPuuid": puuid })),
                )
                .await
            {
                self.ipc
                    .send_event(NAMESPACE, "error-auto-honor", &[json!(error)]);
                eprintln!("[auto-gameflow] 点赞失败: {error}");
                return;
            }
        }

        if let Err(error) = self.call("POST", "/lol-honor/v1/ballot", None).await {
            self.ipc
                .send_event(NAMESPACE, "error-auto-honor", &[json!(error)]);
            eprintln!("[auto-gameflow] 提交点赞失败: {error}");
            return;
        }

        println!(
            "[auto-gameflow] 自动点赞 {} 人，对局 {}",
            candidates.len(),
            game_id
        );
    }

    /// `AutoGameflowAramTeamSideController` — tell the team which side we are on.
    async fn handle_aram_team_side(&self) {
        if !self.flag("autoSendARAMTeamSideEnabled") {
            return;
        }

        let Some(conversation_id) = self
            .league("chat", "conversations.championSelect")
            .and_then(|conversation| conversation.get("id").cloned())
        else {
            return;
        };

        let session = self.league("champSelect", "session");
        let Some(team) = aram_local_team(&session) else {
            return;
        };
        if team != 1 && team != 2 {
            eprintln!("[auto-gameflow] 未知的 team {team}，跳过");
            return;
        }

        let aram_like = session
            .as_ref()
            .and_then(|session| session.get("benchEnabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && matches!(
                self.league("gameflow", "session")
                    .and_then(|session| session.pointer("/map/gameMode").and_then(Value::as_str).map(str::to_string))
                    .as_deref(),
                Some("ARAM") | Some("KIWI")
            );
        if !aram_like {
            return;
        }

        let tw = self
            .league("state", "auth")
            .and_then(|auth| auth.get("region").and_then(Value::as_str).map(str::to_string))
            .as_deref()
            == Some("TW2");

        let message = if tw {
            localized_aram_team_side_tw(team, self.locale_is_english())
        } else {
            localized_aram_team_side(team, self.locale_is_english())
        };

        let visible = self.flag("autoSendARAMTeamSideVisibleToTeam");
        let (text, kind) = if visible {
            (message, "chat")
        } else {
            (format!("[RiftFlow] {message}"), "celebration")
        };

        self.chat_send(conversation_id, text, kind).await;
    }

    // ---- chat helpers ---------------------------------------------------------

    fn party_conversation_id(&self) -> Option<Value> {
        self.league("chat", "conversations.customGame")
            .filter(|value| !value.is_null())
            .and_then(|conversation| conversation.get("id").cloned())
    }

    async fn chat_send(&self, conversation_id: Value, message: String, kind: &str) {
        let url = format!("/lol-chat/v1/conversations/{conversation_id}/messages");
        let body = json!({
            "body": message,
            "fromPid": "",
            "fromSummonerId": 0,
            "id": conversation_id,
            "isHistorical": false,
            "timestamp": "",
            "type": kind,
        });

        if let Err(error) = self.call("POST", &url, Some(body)).await {
            eprintln!("[auto-gameflow] 队伍频道消息未发出: {error}");
        }
    }

    /// `POST`/`GET` that returns the parsed body, for the controllers that read
    /// before they act.
    async fn call_and_read(&self, method: &str, url: &str) -> Result<Value, String> {
        let Some(http) = self.league_client.http() else {
            return Err("LCU 未连接".to_string());
        };

        let response = http
            .request(RequestConfig {
                method: method.to_string(),
                url: url.to_string(),
                data: None,
                params: Map::new(),
                headers: Map::new(),
                response_type: None,
            })
            .await
            .map_err(|error| error.to_string())?;

        if response.status >= 400 {
            return Err(format!("{method} {url} → {}", response.status));
        }

        Ok(response.body)
    }

    fn localized_skip_leader(&self) -> String {
        // the zh-CN value carries the `[RiftFlow]` prefix itself, the en one does not
        if self.locale_is_english() {
            "Attempting to skip becoming the leader".to_string()
        } else {
            "[RiftFlow] 尝试将房主转让".to_string()
        }
    }

    fn localized_invitation_sent(&self, name: &str) -> String {
        if self.locale_is_english() {
            format!("[RiftFlow] Invitation sent to {name}")
        } else {
            format!("[RiftFlow] 已向 {name} 发送房间邀请")
        }
    }
}

/// `ballot.eligibleAllies` / `.eligibleOpponents` minus bots, as puuid lists.
fn eligible_puuids(ballot: &Value, field: &str) -> Vec<String> {
    ballot
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|player| !player.get("botPlayer").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|player| player.get("puuid").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// `localPlayerTeam`: the `team` (1 or 2) of the cell matching `localPlayerCellId`.
fn aram_local_team(session: &Option<Value>) -> Option<i64> {
    let session = session.as_ref()?;
    let cell_id = session.get("localPlayerCellId")?;
    session
        .get("myTeam")
        .and_then(Value::as_array)?
        .iter()
        .find(|player| player.get("cellId") == Some(cell_id))
        .and_then(|player| player.get("team").and_then(Value::as_i64))
}

fn localized_aram_team_side(team: i64, english: bool) -> String {
    match (english, team) {
        (true, 1) => "Your team is in 🟦blue (↙️ left bottom)".to_string(),
        (true, _) => "Your team is in 🟥red (↗️ right top)".to_string(),
        (false, 1) => "本局游戏你的队伍位于 🟦蓝色方 (↙️ 左下侧)".to_string(),
        (false, _) => "本局游戏你的队伍位于 🟥红色方 (↗️ 右上侧)".to_string(),
    }
}

fn localized_aram_team_side_tw(team: i64, english: bool) -> String {
    match (english, team) {
        (true, 1) => "Your team is in 🟦blue (↙️ left bottom)".to_string(),
        (true, _) => "Your team is in 🟥red (↗️ right top)".to_string(),
        (false, 1) => "本局遊戲你的隊伍位於 🟦藍色方 (↙️ 左下側)".to_string(),
        (false, _) => "本局遊戲你的隊伍位於 🟥紅色方 (↗️ 右上側)".to_string(),
    }
}

impl AutoGameflow {
    /// Fire whichever deadline came due; the loop re-arms after each pass.
    async fn fire_due(&self, timers: &mut Timers) {
        let now = Instant::now();

        if timers.accept.is_some_and(|at| at <= now) {
            timers.accept = None;
            self.accept_match().await;
        }
        if timers.play_again.is_some_and(|at| at <= now) {
            timers.play_again = None;
            self.play_again().await;
        }
        if timers.reconnect.is_some_and(|at| at <= now) {
            timers.reconnect = None;
            self.reconnect().await;
        }
        if timers.search.is_some_and(|at| at <= now) {
            timers.search = None;
            timers.chat_countdown = false;
            self.clear_auto_search();
            self.start_matchmaking().await;
        }
    }

    fn next_deadline(timers: &Timers) -> Option<Instant> {
        [timers.accept, timers.play_again, timers.reconnect, timers.search]
            .into_iter()
            .flatten()
            .min()
    }

    async fn run(self: Arc<Self>, mut receiver: UnboundedReceiver<Signal>) {
        let mut timers = Timers::default();
        let mut rng = crate::rng::Rng::new();

        // the reactions all use `fireImmediately`, so the current phase and status
        // have to be applied once before any event arrives
        let phase = self
            .league("gameflow", "phase")
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "None".to_string());
        self.on_phase(&phase, &mut timers).await;
        self.on_activity_status(&mut timers).await;
        self.handle_skip_leader(&mut rng).await;
        self.handle_aram_team_side().await;

        loop {
            let chat_tick = timers
                .chat_countdown
                .then(|| tokio::time::sleep(CHAT_COUNTDOWN_INTERVAL));
            let deadline = Self::next_deadline(&timers);

            tokio::select! {
                biased;

                signal = receiver.recv() => match signal {
                    Some(Signal::Phase(phase)) => {
                        self.on_phase(&phase, &mut timers).await;
                        // a new champ-select session may mean a new ARAM side
                        self.handle_aram_team_side().await;
                    }
                    Some(Signal::ReadyCheck(data)) => self.on_ready_check(&data, &mut timers).await,
                    Some(Signal::ActivityInputsChanged) => {
                        self.on_search_changed(&mut timers).await;
                        self.on_activity_status(&mut timers).await;
                        self.handle_skip_leader(&mut rng).await;
                        self.forget_invited_friends_if_lobby_gone();
                    }
                    Some(Signal::InvitationsChanged) => self.handle_invitations().await,
                    Some(Signal::Friend { data, event_type }) => {
                        self.handle_friend_event(&data, &event_type).await;
                    }
                    Some(Signal::HonorBallot) => self.handle_honor(&mut timers, &mut rng).await,
                    Some(Signal::AramInputsChanged) => self.handle_aram_team_side().await,
                    Some(Signal::SettingChanged(key)) => self.on_setting_changed(&key, &mut timers).await,
                    Some(Signal::Command(Command::CancelAutoAccept)) => {
                        self.cancel_auto_accept(&mut timers, "normal");
                    }
                    Some(Signal::Command(Command::CancelAutoMatchmaking)) => {
                        self.cancel_auto_matchmaking(&mut timers, "normal").await;
                    }
                    None => break,
                },

                _ = async {
                    match chat_tick {
                        Some(tick) => tick.await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.send_chat_info(None).await;
                }

                _ = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.fire_due(&mut timers).await;
                }
            }
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_milliseconds_are_current() {
        // 2024-01-01, i.e. any sane clock
        assert!(now_ms() > 1_700_000_000_000);
    }

    #[test]
    fn ballot_puuids_skip_bots() {
        let ballot = json!({
            "eligibleAllies": [
                { "puuid": "a1", "botPlayer": false },
                { "puuid": "a2", "botPlayer": true }
            ],
            "eligibleOpponents": [{ "puuid": "o1" }]
        });

        assert_eq!(eligible_puuids(&ballot, "eligibleAllies"), vec!["a1".to_string()]);
        // a missing `botPlayer` is not a bot
        assert_eq!(eligible_puuids(&ballot, "eligibleOpponents"), vec!["o1".to_string()]);
    }

    #[test]
    fn aram_team_comes_from_the_matching_cell_only() {
        let session = json!({
            "localPlayerCellId": 3,
            "myTeam": [
                { "cellId": 1, "team": 1 },
                { "cellId": 3, "team": 2 }
            ]
        });
        assert_eq!(aram_local_team(&Some(session)), Some(2));

        // no session, no cell id, or no matching cell all mean "say nothing"
        assert_eq!(aram_local_team(&None), None);
        assert_eq!(aram_local_team(&Some(json!({ "myTeam": [] }))), None);
        assert_eq!(
            aram_local_team(&Some(json!({ "localPlayerCellId": 9, "myTeam": [{ "cellId": 1, "team": 1 }] }))),
            None
        );
    }

    #[test]
    fn aram_side_strings_keep_the_regional_variants() {
        assert_eq!(
            localized_aram_team_side(1, false),
            "本局游戏你的队伍位于 🟦蓝色方 (↙️ 左下侧)"
        );
        assert_eq!(
            localized_aram_team_side_tw(2, false),
            "本局遊戲你的隊伍位於 🟥紅色方 (↗️ 右上側)"
        );
        assert_eq!(
            localized_aram_team_side(2, true),
            "Your team is in 🟥red (↗️ right top)"
        );
    }
}
