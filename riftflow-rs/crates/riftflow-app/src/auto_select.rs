//! `auto-select-main` — 自动禁用 / 选用 / 预选 / 备战席换位 / 英雄交易。
//!
//! Ports `ban-pick-controller.ts`, `bench-controller.ts`, `trade-controller.ts`,
//! `action-executor.ts`, `local-message-service.ts`, `config-manager.ts` and the
//! computed half of `state.ts`. The pure rules live in `riftflow-auto-select`.
//!
//! Like `auto_gameflow.rs` this is one task owning one channel: LCU events and
//! setting writes arrive as signals, every pass recomputes the four MobX computeds
//! the original watched, and a pass that produces a different value runs the body
//! of the matching reaction — including cancelling the timer it replaced.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Instant;

use riftflow_auto_select::{
    ActiveGroupConfig, ChampionStatus, CorrectedTimer, ExpectedChampion, active_action,
    ban_completes, bench_swap_delay_ms, champion_swap_delay_ms, current_actions,
    current_session_champion_id, corrected_timer, first_unfinished_pick_action,
    get_active_group_config, get_expected_bans, get_expected_picks, get_expected_swaps, get_move,
    phase_calibrated_delay_ms, pick_completes, scoped_bench_champions, target_offset_ms,
};
use riftflow_ipc::RiftFlowIpcMain;
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::RequestConfig;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

use crate::settings_registry::SettingsHub;

pub const NAMESPACE: &str = "auto-select-main";
const LEAGUE: &str = "league-client-main";
const SGP: &str = "sgp-main";

const JUST_SHOW: &str = "just-show";
const SHOW_AND_LOCK: &str = "show-and-lock-in";
const LOCK_IMMEDIATELY: &str = "lock-in-immediately";

/// Every signal means the same thing: something one of the four computeds reads
/// has changed, so run another pass and let the reactions' own comparisons decide
/// whether anything happened.
#[derive(Debug, Clone, Copy)]
enum Signal {
    Wake,
}

/// What the reaction closures captured: the id of the action to patch, which is
/// gone again by the time the timer runs.
#[derive(Clone)]
struct BanTask {
    at: Instant,
    champion_id: i64,
    action_id: i64,
    completed: bool,
}

#[derive(Clone)]
struct PickTask {
    at: Instant,
    champion_id: i64,
    action_id: i64,
    completed: bool,
    intent: bool,
}

#[derive(Clone)]
struct BenchTask {
    at: Instant,
    champion_id: i64,
}

#[derive(Clone)]
struct SwapTask {
    at: Instant,
    trade_id: i64,
    accept: bool,
}

#[derive(Default)]
struct Timers {
    ban: Option<BanTask>,
    pick: Option<PickTask>,
    bench: Option<BenchTask>,
    swap: Option<SwapTask>,
}

impl Timers {
    fn next_deadline(&self) -> Option<Instant> {
        [
            self.ban.as_ref().map(|t| t.at),
            self.pick.as_ref().map(|t| t.at),
            self.bench.as_ref().map(|t| t.at),
            self.swap.as_ref().map(|t| t.at),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

#[derive(Clone, PartialEq)]
struct BanContext {
    move_name: &'static str,
    active_action: Value,
    expected: ExpectedChampion,
    delay_ms: f64,
    strategy: String,
}

#[derive(Clone, PartialEq)]
struct PickContext {
    move_name: &'static str,
    active_action: Value,
    first_unfinished: Option<Value>,
    expected: ExpectedChampion,
    delay_ms: f64,
    strategy: String,
}

#[derive(Clone, PartialEq)]
struct BenchContext {
    expected: ExpectedChampion,
    delay_ms: f64,
}

#[derive(Clone, PartialEq)]
struct SwapContext {
    action: &'static str,
    trade_id: i64,
    requester_champion_id: i64,
    delay_ms: f64,
}

/// The observable boxes and the `previous` arguments the reactions needed.
#[derive(Default)]
struct Loop {
    timers: Timers,
    ban_context: Option<BanContext>,
    pick_context: Option<PickContext>,
    bench_context: Option<BenchContext>,
    swap_context: Option<SwapContext>,
    /// `benchChampionCooldown`: champion id → the moment it first appeared
    bench_cooldown: Option<HashMap<i64, i64>>,
    /// the last list the bench reaction compared against, `None` before its first
    /// evaluation (a `reaction` never fires for the initial value)
    bench_seen: Option<Vec<i64>>,
    ongoing_swap: Value,
    trade_created_at: Option<i64>,
    chat_id: Value,
    in_champ_select: bool,
}

/// Everything the computeds read, taken once per pass.
struct Derived {
    group: Option<ActiveGroupConfig>,
    expected_picks: Option<Vec<ExpectedChampion>>,
    expected_bans: Option<Vec<ExpectedChampion>>,
    expected_swaps: Option<Vec<ExpectedChampion>>,
    move_name: Option<&'static str>,
    active_action: Option<Value>,
    first_unfinished: Option<Value>,
    corrected: Option<CorrectedTimer>,
    scoped_bench: Vec<i64>,
    session_champion: Option<i64>,
    slots: usize,
    ongoing_swap: Value,
    chat_id: Value,
    in_champ_select: bool,
    phase: Option<String>,
}

pub struct AutoSelect {
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    sender: UnboundedSender<Signal>,
    backlog: Arc<Mutex<Vec<String>>>,
    /// whether the champion-select notices are the english ones
    english: AtomicBool,
}

/// `onInit` — the IPC surface, the listeners, and the deciding task.
pub fn start(
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    hub: &Arc<SettingsHub>,
) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let this = Arc::new(AutoSelect {
        league_client,
        state_sync,
        ipc,
        settings,
        sender: sender.clone(),
        backlog: Arc::new(Mutex::new(Vec::new())),
        english: AtomicBool::new(false),
    });

    register_ipc(&this)?;
    watch_events(&this)?;

    let mut changes = hub.subscribe();
    let for_settings = this.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok((namespace, _key, _value)) = changes.recv().await {
            if namespace == NAMESPACE {
                let _ = for_settings.sender.send(Signal::Wake);
            }
        }
    });

    tauri::async_runtime::spawn(this.run(receiver));
    Ok(())
}

fn register_ipc(this: &Arc<AutoSelect>) -> anyhow::Result<()> {
    let settings = this.settings.clone();
    let sender = this.sender.clone();
    this.ipc.on_call(NAMESPACE, "setPickConfig", riftflow_ipc::handler(
        move |_ctx, args| {
            let settings = settings.clone();
            let sender = sender.clone();
            async move {
                write_group_config(&settings, "pickConfig", &args, empty_pick_config).await;
                let _ = sender.send(Signal::Wake);
                Ok(Value::Null)
            }
        },
    ))?;

    let settings = this.settings.clone();
    let sender = this.sender.clone();
    this.ipc.on_call(NAMESPACE, "setBanConfig", riftflow_ipc::handler(
        move |_ctx, args| {
            let settings = settings.clone();
            let sender = sender.clone();
            async move {
                write_group_config(&settings, "banConfig", &args, empty_ban_config).await;
                let _ = sender.send(Signal::Wake);
                Ok(Value::Null)
            }
        },
    ))?;

    let sync = this.state_sync.clone();
    let sender = this.sender.clone();
    this.ipc.on_call(
        NAMESPACE,
        "setTemporarilyDisabled",
        riftflow_ipc::handler(move |_ctx, args| {
            let sync = sync.clone();
            let sender = sender.clone();
            async move {
                let value = args.first().cloned().unwrap_or(Value::Bool(false));
                let _ = sync.set(NAMESPACE, "state", "temporarilyDisabled", value);
                let _ = sender.send(Signal::Wake);
                Ok(Value::Null)
            }
        }),
    )?;

    Ok(())
}

/// The endpoints whose events feed the four computeds, plus the champion-select
/// conversation for the local notices. `lc_state` owns the values; this only
/// wakes the task up.
fn watch_events(this: &Arc<AutoSelect>) -> anyhow::Result<()> {
    let emitter = this.league_client.emitter();

    for uri in [
        "/lol-champ-select/v1/session",
        "/lol-champ-select/v1/pickable-champion-ids",
        "/lol-champ-select/v1/bannable-champion-ids",
        "/lol-champ-select/v1/disabled-champion-ids",
        "/lol-champ-select/v1/ongoing-champion-swap",
        "/lol-champ-select/v1/grid-champions/:id",
        "/lol-lobby-team-builder/champ-select/v1/subset-champion-list",
        "/lol-gameflow/v1/session",
        "/lol-chat/v1/conversation",
        "/lol-summoner/v1/current-summoner",
    ] {
        let sender = this.sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(Signal::Wake);
        })?;
    }

    Ok(())
}

impl AutoSelect {
    async fn run(self: Arc<Self>, mut receiver: UnboundedReceiver<Signal>) {
        let mut loop_state = Loop::default();
        self.english.store(self.locale_is_english(), Ordering::Relaxed);

        // `_setupState` awaited `fillAutoBanPickConfig` before the watchers ran
        self.fill_config().await;

        // the two reactions declared with `fireImmediately`, and the first
        // evaluation of every computed, on whatever the client already reported
        self.recompute(&mut loop_state).await;

        loop {
            let deadline = loop_state.timers.next_deadline();

            tokio::select! {
                biased;

                signal = receiver.recv() => match signal {
                    Some(Signal::Wake) => self.recompute(&mut loop_state).await,
                    None => break,
                },

                _ = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.fire_due(&mut loop_state).await;
                }
            }
        }
    }

    // ---- inputs ---------------------------------------------------------------

    fn setting(&self, key: &str) -> Value {
        self.settings
            .get_service(NAMESPACE)
            .map(|service| service.get(key))
            .unwrap_or(Value::Null)
    }

    fn push(&self, path: &str, value: Value) {
        let _ = self.state_sync.set(NAMESPACE, "state", path, value);
    }

    fn league(&self, state_id: &str, path: &str) -> Option<Value> {
        self.state_sync.get(LEAGUE, state_id, path)
    }

    fn delayed(&self, path: &str) -> Option<Value> {
        self.state_sync
            .get(NAMESPACE, "state", path)
            .filter(|value| value.is_object())
    }

    /// One pass of everything the original had as reactions on `AutoSelectState`.
    async fn recompute(&self, s: &mut Loop) {
        let now = now_ms();
        let derived = self.derive(now);

        self.push_computed(&derived);
        self.run_local_message_reactions(&derived, s).await;
        self.track_bench_cooldown(&derived, s, now);
        self.track_trade_timestamp(&derived, s, now);

        self.react_ban(&derived, s).await;
        self.react_pick(&derived, s).await;
        self.react_bench(&derived, s, now).await;
        self.react_swap(&derived, s, now).await;
    }

    fn derive(&self, now: i64) -> Derived {
        let session = self
            .league("champSelect", "session")
            .filter(|value| value.is_object());
        let timer = session
            .as_ref()
            .and_then(|value| value.get("timer"))
            .cloned()
            .filter(|value| value.is_object());

        let groups = self
            .state_sync
            .get(NAMESPACE, "state", "groups")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();

        let pick_config = object_of(&self.setting("pickConfig"));
        let ban_config = object_of(&self.setting("banConfig"));

        let availability = self.state_sync.get(SGP, "state", "availability");
        let sgp_server_id = availability
            .as_ref()
            .and_then(|value| value.get("sgpServerId"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let league_servers = self
            .state_sync
            .get(SGP, "state", "leagueServers")
            .and_then(|value| value.get("servers").cloned())
            .unwrap_or(Value::Null);

        let gameflow = self.league("gameflow", "session");
        let game_mode = gameflow
            .as_ref()
            .and_then(|value| value.pointer("/gameData/queue/gameMode"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let queue_type = gameflow
            .as_ref()
            .and_then(|value| value.pointer("/gameData/queue/type"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let is_custom_game = gameflow
            .as_ref()
            .and_then(|value| value.pointer("/gameData/isCustomGame"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || session
                .as_ref()
                .and_then(|value| value.get("isCustomGame"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

        let temporarily_disabled = self
            .state_sync
            .get(NAMESPACE, "state", "temporarilyDisabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let group = get_active_group_config(
            &groups,
            game_mode.as_deref(),
            queue_type.as_deref(),
            is_custom_game,
            &sgp_server_id,
            &league_servers,
            temporarily_disabled,
            &pick_config,
            &ban_config,
        )
        .flatten();

        let grid = self
            .league("champSelect", "gridChampions")
            .unwrap_or_else(|| json!({}));
        let pickable = ids_of(
            &self
                .league("champSelect", "currentPickableChampionIds")
                .unwrap_or(Value::Null),
        );
        let bannable = ids_of(
            &self
                .league("champSelect", "currentBannableChampionIds")
                .unwrap_or(Value::Null),
        );
        let subset = ids_of(
            &self
                .league("lobbyTeamBuilder", "champSelect.subsetChampionList")
                .unwrap_or(Value::Null),
        );

        let assigned_position = member_of(session.as_ref())
            .and_then(|member| member.get("assignedPosition"))
            .and_then(Value::as_str);

        let allow_subset = flag(session.as_ref(), "allowSubsetChampionPicks");
        let bench_enabled = flag(session.as_ref(), "benchEnabled");
        let allow_duplicates = flag(session.as_ref(), "allowDuplicatePicks");

        let expected_picks = get_expected_picks(
            group.as_ref(),
            assigned_position,
            game_mode.as_deref(),
            &grid,
            &pickable,
            allow_duplicates,
            allow_subset,
            &subset,
        );
        let bench: Vec<Value> = session
            .as_ref()
            .and_then(|value| value.get("benchChampions"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let scoped_bench = scoped_bench_champions(
            &subset,
            timer.as_ref(),
            &bench,
        );
        let expected_swaps = get_expected_swaps(
            group.as_ref(),
            bench_enabled,
            &scoped_bench,
            assigned_position,
            &pickable,
            allow_subset,
            riftflow_auto_select::timer_phase(timer.as_ref()) == Some("BAN_PICK"),
            &subset,
        );
        let expected_bans = get_expected_bans(group.as_ref(), assigned_position, &grid, &bannable);

        let current = current_actions(session.as_ref());
        let acting = active_action(&current);
        let first_unfinished = first_unfinished_pick_action(session.as_ref());
        let is_picking = is_type(acting.as_ref(), "pick");
        let is_banning = is_type(acting.as_ref(), "ban");
        let is_voting = is_type(acting.as_ref(), "vote");
        let session_champion = current_session_champion_id(session.as_ref());

        let is_planning = phase_of(timer.as_ref()) == Some("PLANNING");
        let is_pick_intenting =
            is_planning || (first_unfinished.is_some() && !is_picking && !is_banning);

        let move_name = get_move(riftflow_auto_select::MoveInputs {
            is_pick_intenting,
            active_action: acting.as_ref(),
            is_picking_now: is_picking,
            is_banning_now: is_banning,
            is_voting_now: is_voting,
            allow_subset_champion_picks: allow_subset,
            bench_enabled,
            current_session_champion_id: session_champion,
            timer: timer.as_ref(),
        })
        .map(|m| m.as_str());

        let conversation = self
            .league("chat", "conversations.championSelect")
            .filter(|value| value.is_object());
        let chat_id = conversation
            .and_then(|value| value.get("id").cloned())
            .unwrap_or(Value::Null);

        Derived {
            slots: my_team_of(session.as_ref()).len(),
            ongoing_swap: self
                .league("champSelect", "ongoingChampionSwap")
                .unwrap_or(Value::Null),
            corrected: corrected_timer(timer.as_ref(), now),
            session_champion,
            move_name,
            active_action: acting,
            first_unfinished,
            group,
            expected_picks,
            expected_bans,
            expected_swaps,
            scoped_bench,
            chat_id,
            in_champ_select: session.is_some(),
            phase: phase_of(timer.as_ref()).map(str::to_string),
        }
    }

    fn push_computed(&self, d: &Derived) {
        self.push("expectedPicks", expected_json(d.expected_picks.as_ref()));
        self.push("expectedBans", expected_json(d.expected_bans.as_ref()));
        self.push("expectedSwaps", expected_json(d.expected_swaps.as_ref()));
        self.push(
            "activeGroupConfigId",
            match &d.group {
                Some(group) => json!(group.group_id),
                None => Value::Null,
            },
        );
    }

    // ---- `_localMessage.watch()` ---------------------------------------------

    async fn run_local_message_reactions(&self, d: &Derived, s: &mut Loop) {
        let had_chat = s.chat_id.is_string() && !s.chat_id.as_str().is_some_and(str::is_empty);
        let has_chat = d.chat_id.is_string() && !d.chat_id.as_str().is_some_and(str::is_empty);

        if !had_chat && has_chat {
            self.flush_backlog().await;
        }
        s.chat_id = d.chat_id.clone();

        if s.in_champ_select && !d.in_champ_select {
            self.backlog.lock().unwrap().clear();
            self.push("temporarilyDisabled", Value::Bool(false));
        }
        s.in_champ_select = d.in_champ_select;
    }

    async fn flush_backlog(&self) {
        loop {
            let message = {
                let mut guard = self.backlog.lock().unwrap();
                if guard.is_empty() {
                    break;
                }
                guard.remove(0)
            };

            self.notice(message).await;
        }
    }

    /// `AutoSelectLocalMessageService.send` — the LCU's `celebration` type is the
    /// notice only this player sees, and it only exists inside a draft.
    async fn notice(&self, text: String) {
        let in_champ_select = self
            .league("champSelect", "session")
            .is_some_and(|value| value.is_object());
        if !in_champ_select {
            return;
        }

        let Some(chat_id) = self.chat_id() else {
            self.backlog.lock().unwrap().push(text);
            return;
        };

        self.chat_send(&chat_id, text).await;
    }

    fn chat_id(&self) -> Option<Value> {
        self.league("chat", "conversations.championSelect")
            .and_then(|conversation| conversation.get("id").cloned())
            .filter(|id| !id.is_null())
    }

    async fn chat_send(&self, conversation_id: &Value, message: String) {
        let Some(http) = self.league_client.http() else {
            return;
        };

        let id = match conversation_id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let body = json!({
            "body": format!("[RiftFlow] {message}"),
            "fromPid": "",
            "fromSummonerId": 0,
            "id": id,
            "isHistorical": false,
            "timestamp": "",
            "type": "celebration",
        });
        let config = RequestConfig {
            method: "POST".to_string(),
            url: format!("/lol-chat/v1/conversations/{id}/messages"),
            data: Some(body),
            ..Default::default()
        };

        // the original swallows the failure: a notice never blocks a draft
        let _ = http.request(config).await;
    }

    // ---- `bench-controller.ts`'s cooldown box --------------------------------

    fn track_bench_cooldown(&self, d: &Derived, s: &mut Loop, now: i64) {
        if s.bench_seen.is_none() {
            // a `reaction` evaluates without firing for its first value
            s.bench_seen = Some(d.scoped_bench.clone());
            return;
        }

        if s.bench_seen.as_deref() == Some(d.scoped_bench.as_slice()) {
            return;
        }

        s.bench_seen = Some(d.scoped_bench.clone());
        s.bench_cooldown = Some(track_cooldown(s.bench_cooldown.as_ref(), &d.scoped_bench, now));
    }

    // ---- `trade-controller.ts`'s timestamp box -------------------------------

    /// The first reaction of `AutoSelectTradeController.watch`, with the `previous`
    /// value of `state.ongoingChampionSwap`.
    fn track_trade_timestamp(&self, d: &Derived, s: &mut Loop, now: i64) {
        if s.ongoing_swap == d.ongoing_swap {
            return;
        }

        let received = d
            .ongoing_swap
            .get("state")
            .and_then(Value::as_str)
            == Some("RECEIVED");

        if s.ongoing_swap.is_null() && !d.ongoing_swap.is_null() && received {
            s.trade_created_at = Some(now);
        } else {
            s.trade_created_at = None;
        }

        s.ongoing_swap = d.ongoing_swap.clone();
    }

    // ---- ban -----------------------------------------------------------------

    fn ban_context(&self, d: &Derived) -> Option<BanContext> {
        let group = d.group.as_ref()?;
        let expected = d.expected_bans.as_ref()?;

        if !group.ban.enabled || group.temporarily_disabled {
            return None;
        }

        if !matches!(d.move_name, Some("complete-ban" | "show-ban")) {
            return None;
        }
        let acting = d.active_action.as_ref()?;

        let expected_ban = expected
            .iter()
            .find(|champion| champion.status == ChampionStatus::Bannable)
            .copied()?;

        let configured_ms = group.ban.delay_seconds * 1_000.0;
        let is_complete = d.move_name == Some("complete-ban");
        let offset_ms = target_offset_ms(&group.ban.strategy, is_complete, configured_ms);

        Some(BanContext {
            move_name: d.move_name?,
            active_action: acting.clone(),
            expected: expected_ban,
            delay_ms: phase_calibrated_delay_ms(
                configured_ms,
                offset_ms,
                d.corrected,
            ),
            strategy: group.ban.strategy.clone(),
        })
    }

    async fn react_ban(&self, d: &Derived, s: &mut Loop) {
        let context = self.ban_context(d);
        if context == s.ban_context {
            return;
        }
        s.ban_context = context.clone();

        let Some(context) = context else {
            if s.timers.ban.is_some() {
                s.timers.ban = None;
                self.push("delayedBan", Value::Null);
            }
            return;
        };

        let task = self.delayed("delayedBan");
        let action_champion = champion_id_of(Some(&context.active_action));

        if context.move_name == "complete-ban"
            && context.strategy == JUST_SHOW
            && action_champion != 0
        {
            if task.is_some() {
                s.timers.ban = None;
                self.push("delayedBan", Value::Null);

                if action_champion != context.expected.id {
                    eprintln!(
                        "[auto-select] Already banned {}, didn't change it, move={}",
                        self.champion_name(action_champion),
                        context.move_name
                    );
                }
                return;
            }
        }

        // the 空选 → 亮出 → 锁定 transition is a few hundred ms wide; ignore it
        if context.strategy == LOCK_IMMEDIATELY
            && context.move_name == "complete-ban"
            && task_number(&task, "championId") == Some(context.expected.id)
        {
            return;
        }

        let completed = ban_completes(&context.strategy, context.move_name == "complete-ban");
        let previous_id = task_number(&task, "championId");
        let previous_completed = task.as_ref().and_then(|t| t.get("completed")).and_then(Value::as_bool);
        s.timers.ban = None;

        if task.is_none()
            || previous_id != Some(context.expected.id)
            || previous_completed != Some(completed)
        {
            let name = self.champion_name(context.expected.id);
            let seconds = seconds_text(context.delay_ms);
            let text = if completed {
                self.localized("complete-ban", &name, &seconds, "")
            } else {
                self.localized("show-ban", &name, &seconds, "")
            };
            self.notice(text).await;
            eprintln!(
                "[auto-select] Delayed ban champion={name} completed={completed} move={} strategy={} delayedMs={}",
                context.move_name, context.strategy, context.delay_ms
            );
        }

        let now = now_ms();
        let action_id = context
            .active_action
            .get("id")
            .and_then(Value::as_i64)
            .unwrap_or_default();

        self.push(
            "delayedBan",
            delayed_task_json(
                &task,
                json!({
                    "isPickIntent": false,
                    "completed": completed,
                    "championId": context.expected.id,
                    "delayMs": context.delay_ms,
                }),
                now,
            ),
        );
        s.timers.ban = Some(BanTask {
            at: Instant::now() + duration_of(context.delay_ms),
            champion_id: context.expected.id,
            action_id,
            completed,
        });
    }

    // ---- pick ----------------------------------------------------------------

    fn pick_context(&self, d: &Derived) -> Option<PickContext> {
        let group = d.group.as_ref()?;
        let expected = d.expected_picks.as_ref()?;

        if !group.pick.enabled || group.temporarily_disabled {
            return None;
        }

        let move_name = d.move_name?;
        if !matches!(
            move_name,
            "pick-intent" | "complete-pick" | "complete-subset-pick" | "show-pick" | "show-subset-pick"
        ) {
            return None;
        }
        let acting = d.active_action.as_ref()?;

        if move_name == "pick-intent"
            && (d.first_unfinished.is_none() || !group.pick.show_intent)
        {
            return None;
        }

        let expected_pick = expected
            .iter()
            .find(|champion| {
                matches!(
                    champion.status,
                    ChampionStatus::Pickable | ChampionStatus::SubsetPickable
                ) || (group.pick.ignore_intent && champion.status == ChampionStatus::PickIntented)
            })
            .copied()?;

        let configured_ms = group.pick.delay_seconds * 1_000.0;
        let offset_ms = target_offset_ms(&group.pick.strategy, move_name == "complete-pick", configured_ms);

        Some(PickContext {
            move_name,
            active_action: acting.clone(),
            first_unfinished: d.first_unfinished.clone(),
            expected: expected_pick,
            delay_ms: phase_calibrated_delay_ms(configured_ms, offset_ms, d.corrected),
            strategy: group.pick.strategy.clone(),
        })
    }

    async fn react_pick(&self, d: &Derived, s: &mut Loop) {
        let context = self.pick_context(d);
        if context == s.pick_context {
            return;
        }
        s.pick_context = context.clone();

        let Some(context) = context else {
            if s.timers.pick.is_some() {
                s.timers.pick = None;
                self.push("delayedPick", Value::Null);
            }
            return;
        };

        let task = self.delayed("delayedPick");
        let action_champion = champion_id_of(Some(&context.active_action));

        if context.move_name == "complete-pick"
            && context.strategy == JUST_SHOW
            && action_champion != 0
        {
            if task.is_some() {
                s.timers.pick = None;
                self.push("delayedPick", Value::Null);
            }

            if action_champion != context.expected.id {
                eprintln!(
                    "[auto-select] Already picked {}, didn't change it, move={}",
                    self.champion_name(action_champion),
                    context.move_name
                );
            }
            return;
        }

        if context.move_name == "pick-intent" {
            let already_intented = context
                .first_unfinished
                .as_ref()
                .map(|action| champion_id_of(Some(action)) != 0)
                .unwrap_or(true);

            if already_intented {
                if task.is_some() {
                    s.timers.pick = None;
                    self.push("delayedPick", Value::Null);
                }
                return;
            }
        }

        if context.strategy == LOCK_IMMEDIATELY
            && context.move_name == "complete-pick"
            && task_number(&task, "championId") == Some(context.expected.id)
        {
            return;
        }

        let completed = pick_completes(context.move_name, &context.strategy);
        let previous_id = task_number(&task, "championId");
        let previous_completed = task
            .as_ref()
            .and_then(|t| t.get("completed"))
            .and_then(Value::as_bool);
        s.timers.pick = None;

        if task.is_none()
            || previous_id != Some(context.expected.id)
            || previous_completed != Some(completed)
        {
            let name = self.champion_name(context.expected.id);
            let seconds = seconds_text(context.delay_ms);
            let key = if context.move_name == "pick-intent" {
                "intent-pick"
            } else if completed {
                "complete-pick"
            } else {
                "show-pick"
            };
            self.notice(self.localized(key, &name, &seconds, "")).await;
            eprintln!(
                "[auto-select] Delayed {} champion={name} completed={completed} move={} strategy={} delayedMs={}",
                if context.move_name == "pick-intent" { "pick intent" } else { "pick" },
                context.move_name,
                context.strategy,
                context.delay_ms
            );
        }

        let now = now_ms();
        if context.move_name == "pick-intent" {
            let Some(action_id) = context
                .first_unfinished
                .as_ref()
                .and_then(|action| action.get("id").and_then(Value::as_i64))
            else {
                self.push("delayedPick", Value::Null);
                return;
            };

            self.push(
                "delayedPick",
                delayed_task_json(
                    &task,
                    json!({
                        "isPickIntent": true,
                        "completed": false,
                        "championId": context.expected.id,
                        "delayMs": context.delay_ms,
                    }),
                    now,
                ),
            );
            s.timers.pick = Some(PickTask {
                at: Instant::now() + duration_of(context.delay_ms),
                champion_id: context.expected.id,
                action_id,
                completed: false,
                intent: true,
            });
            return;
        }

        self.push(
            "delayedPick",
            delayed_task_json(
                &task,
                json!({
                    "isPickIntent": false,
                    "completed": completed,
                    "championId": context.expected.id,
                    "delayMs": context.delay_ms,
                }),
                now,
            ),
        );
        s.timers.pick = Some(PickTask {
            at: Instant::now() + duration_of(context.delay_ms),
            champion_id: context.expected.id,
            action_id: context
                .active_action
                .get("id")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            completed,
            intent: false,
        });
    }

    // ---- bench swap ----------------------------------------------------------

    fn bench_context(&self, d: &Derived, s: &Loop, now: i64) -> Option<BenchContext> {
        let cooldown = s.bench_cooldown.as_ref()?;
        let group = d.group.as_ref()?;
        let expected = d.expected_swaps.as_ref()?;

        if !group.pick.enabled || group.temporarily_disabled || d.session_champion.is_none() {
            return None;
        }

        if !matches!(d.move_name, Some("bench-swap" | "subset-bench-swap")) {
            return None;
        }

        let swap_index = expected.iter().position(|champion| {
            matches!(
                champion.status,
                ChampionStatus::Swappable | ChampionStatus::SubsetSwappable
            )
        })?;

        let held = d.session_champion.unwrap_or_default();
        let current_index = expected.iter().position(|champion| champion.id == held);

        // nothing suitable in hand → lock the first preference; otherwise only
        // trade up when the config asks for the first available champion
        let should_swap = match current_index {
            None => true,
            Some(index) => {
                group.pick.bench_select_first_available_champion && swap_index < index
            }
        };
        if !should_swap {
            return None;
        }

        let champion = expected[swap_index];
        let first_seen = *cooldown.get(&champion.id)?;

        Some(BenchContext {
            expected: champion,
            delay_ms: bench_swap_delay_ms(
                group.pick.bench_swap_accumulated_delay_seconds * 1_000.0,
                first_seen,
                now,
                d.corrected.map(|timer| timer.remaining_ms),
            ),
        })
    }

    async fn react_bench(&self, d: &Derived, s: &mut Loop, now: i64) {
        let context = self.bench_context(d, s, now);
        if context == s.bench_context {
            return;
        }
        s.bench_context = context.clone();

        let Some(context) = context else {
            if s.timers.bench.is_some() {
                s.timers.bench = None;
                self.push("delayedBenchSwap", Value::Null);
            }
            return;
        };

        let task = self.delayed("delayedBenchSwap");
        s.timers.bench = None;

        if task.is_none() || task_number(&task, "championId") != Some(context.expected.id) {
            let name = self.champion_name(context.expected.id);
            self.notice(
                self.localized("bench-swap", &name, &seconds_text(context.delay_ms), ""),
            )
            .await;
        }

        self.push(
            "delayedBenchSwap",
            delayed_task_json(
                &task,
                json!({
                    "championId": context.expected.id,
                    "delayMs": context.delay_ms,
                }),
                now,
            ),
        );
        s.timers.bench = Some(BenchTask {
            at: Instant::now() + duration_of(context.delay_ms),
            champion_id: context.expected.id,
        });
    }

    // ---- champion swap (trade) -----------------------------------------------

    fn swap_context(&self, d: &Derived, s: &Loop, now: i64) -> Option<SwapContext> {
        let created_at = s.trade_created_at?;
        if d.slots == 0 {
            return None;
        }

        let group = d.group.as_ref()?;
        if !group.pick.bench_handle_trade_enabled {
            return None;
        }
        let expected = d.expected_swaps.as_ref()?;
        if !d.ongoing_swap.is_object() {
            return None;
        }

        let trade_id = d.ongoing_swap.get("id").and_then(Value::as_i64)?;
        let her_champion_id = d
            .ongoing_swap
            .get("requesterChampionId")
            .and_then(Value::as_i64)?;

        let configured_ms = group.pick.delay_seconds * 1_000.0;
        let time_left = if d.phase.as_deref() == Some("FINALIZATION") {
            d.corrected.map(|timer| timer.remaining_ms)
        } else {
            None
        };

        let her_index = expected
            .iter()
            .position(|champion| champion.id == her_champion_id);

        let held = d.session_champion.unwrap_or_default();
        let hand_index = expected.iter().position(|champion| champion.id == held);

        // a champion that is not on my list at all is always refused
        let action = match her_index {
            None => "decline",
            Some(her) => {
                if hand_index.is_none()
                    || (group.pick.bench_select_first_available_champion && her < hand_index.unwrap_or(0))
                {
                    "accept"
                } else {
                    "decline"
                }
            }
        };

        // the refusal of an unknown champion keeps the original's un-clamped delay
        let delay_ms = champion_swap_delay_ms(
            configured_ms,
            created_at,
            now,
            time_left,
            her_index.is_some(),
        );

        Some(SwapContext {
            action,
            trade_id,
            requester_champion_id: her_champion_id,
            delay_ms,
        })
    }

    async fn react_swap(&self, d: &Derived, s: &mut Loop, now: i64) {
        let context = self.swap_context(d, s, now);
        if context == s.swap_context {
            return;
        }
        s.swap_context = context.clone();

        let Some(context) = context else {
            if s.timers.swap.is_some() {
                s.timers.swap = None;
                self.push("delayedChampionSwap", Value::Null);
            }
            return;
        };

        let task = self.delayed("delayedChampionSwap");
        s.timers.swap = None;

        let differs = task.is_none()
            || task
                .as_ref()
                .and_then(|t| t.get("action").and_then(Value::as_str))
                != Some(context.action)
            || task_number(&task, "tradeId") != Some(context.trade_id)
            || task_number(&task, "requesterChampionId") != Some(context.requester_champion_id);

        if differs {
            let name = self.champion_name(context.requester_champion_id);
            self.notice(self.localized(
                &format!("{}-champion-swap", context.action),
                &name,
                &seconds_text(context.delay_ms),
                "",
            ))
            .await;
        }

        self.push(
            "delayedChampionSwap",
            delayed_task_json(
                &task,
                json!({
                    "action": context.action,
                    "tradeId": context.trade_id,
                    "requesterChampionId": context.requester_champion_id,
                    "delayMs": context.delay_ms,
                }),
                now,
            ),
        );
        s.timers.swap = Some(SwapTask {
            at: Instant::now() + duration_of(context.delay_ms),
            trade_id: context.trade_id,
            accept: context.action == "accept",
        });
    }

    // ---- executor ------------------------------------------------------------

    async fn fire_due(&self, s: &mut Loop) {
        let now = Instant::now();

        // the computed the task came from is *not* reset here: the original's
        // reaction only re-runs when an input changes, and a call that the server
        // refused must not be retried by the next unrelated event
        if s.timers.ban.as_ref().is_some_and(|task| task.at <= now) {
            let task = s.timers.ban.take().unwrap();
            self.execute_ban(&task).await;
            self.push("delayedBan", Value::Null);
        }

        if s.timers.pick.as_ref().is_some_and(|task| task.at <= now) {
            let task = s.timers.pick.take().unwrap();
            self.execute_pick(&task).await;
            self.push("delayedPick", Value::Null);
        }

        if s.timers.bench.as_ref().is_some_and(|task| task.at <= now) {
            let task = s.timers.bench.take().unwrap();
            self.execute_bench_swap(&task).await;
            self.push("delayedBenchSwap", Value::Null);
        }

        if s.timers.swap.as_ref().is_some_and(|task| task.at <= now) {
            let task = s.timers.swap.take().unwrap();
            self.execute_champion_swap(&task).await;
            self.push("delayedChampionSwap", Value::Null);
        }
    }

    async fn execute_ban(&self, task: &BanTask) {
        let name = self.champion_name(task.champion_id);
        eprintln!("[auto-select] Banning {name} completed={}", task.completed);

        let body = json!({ "type": "ban", "championId": task.champion_id, "completed": task.completed });
        if let Err(reason) = self
            .call(
                "PATCH",
                &format!("/lol-champ-select/v1/session/actions/{}", task.action_id),
                Some(body),
            )
            .await
        {
            eprintln!("[auto-select] 自动禁用失败 {name}: {reason}");
            self.notice(self.localized("error-ban", &name, "", &reason)).await;
        }
    }

    async fn execute_pick(&self, task: &PickTask) {
        let name = self.champion_name(task.champion_id);
        let url = format!("/lol-champ-select/v1/session/actions/{}", task.action_id);

        let (body, key, verb) = if task.intent {
            eprintln!("[auto-select] Intenting {name}");
            (json!({ "championId": task.champion_id }), "error-intent", "预选")
        } else {
            eprintln!("[auto-select] Picking {name} completed={}", task.completed);
            (
                json!({ "type": "pick", "championId": task.champion_id, "completed": task.completed }),
                "error-pick",
                "选择",
            )
        };

        if let Err(reason) = self.call("PATCH", &url, Some(body)).await {
            eprintln!("[auto-select] 自动{verb}失败 {name}: {reason}");
            self.notice(self.localized(key, &name, "", &reason)).await;
        }
    }

    async fn execute_bench_swap(&self, task: &BenchTask) {
        let name = self.champion_name(task.champion_id);
        eprintln!("[auto-select] Swapped champion: {name}");

        if let Err(reason) = self
            .call(
                "POST",
                &format!("/lol-champ-select/v1/session/bench/swap/{}", task.champion_id),
                None,
            )
            .await
        {
            eprintln!("[auto-select] 自动交换英雄失败 {name}: {reason}");
        }
    }

    async fn execute_champion_swap(&self, task: &SwapTask) {
        let verb = if task.accept { "accept" } else { "decline" };
        if let Err(reason) = self
            .call(
                "POST",
                &format!(
                    "/lol-champ-select/v1/session/champion-swaps/{}/{}",
                    task.trade_id, verb
                ),
                None,
            )
            .await
        {
            eprintln!("[auto-select] 自动{verb}交换请求失败: {reason}");
        }
    }

    async fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<(), String> {
        let Some(http) = self.league_client.http() else {
            return Err("LCU 未连接".to_string());
        };

        let config = RequestConfig {
            method: method.to_string(),
            url: url.to_string(),
            data: body,
            ..Default::default()
        };

        let response = http
            .request(config)
            .await
            .map_err(|error| error.to_string())?;

        if response.status >= 400 {
            return Err(format!("{method} {url} → {}", response.status));
        }

        Ok(())
    }

    // ---- config-manager.ts ---------------------------------------------------

    /// `fillAutoBanPickConfig`: every group needs an entry so the settings page can
    /// render its tabs, even for groups the player never configured.
    async fn fill_config(&self) {
        let groups: Vec<Value> = self
            .state_sync
            .get(NAMESPACE, "state", "groups")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();

        let mut pick = object_of(&self.setting("pickConfig"));
        let mut ban = object_of(&self.setting("banConfig"));
        let mut modified = false;

        for group in &groups {
            let Some(id) = group.get("groupId").and_then(Value::as_str) else {
                continue;
            };

            if !pick.contains_key(id) {
                pick.insert(id.to_string(), empty_pick_config());
                modified = true;
            }
            if !ban.contains_key(id) {
                ban.insert(id.to_string(), empty_ban_config());
                modified = true;
            }
        }

        if modified {
            let _ = self
                .settings
                .set_setting(NAMESPACE, "pickConfig", Value::Object(pick))
                .await;
            let _ = self
                .settings
                .set_setting(NAMESPACE, "banConfig", Value::Object(ban))
                .await;
        }
    }

    // ---- misc ----------------------------------------------------------------

    fn champion_name(&self, id: i64) -> String {
        // `gameData.championName(id)` — a name the LCU does not know falls back to
        // the id, which is how "勇敢举动" (-3) shows up outside 斗魂竞技场
        let name = self
            .league("gameData", "champions")
            .and_then(|champions| {
                champions
                    .get(&id.to_string())
                    .and_then(|entry| entry.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| id.to_string());

        format!("{name} ({id})")
    }

    fn localized(&self, key: &str, champion: &str, seconds: &str, reason: &str) -> String {
        notice_text(self.english.load(Ordering::Relaxed), key, champion, seconds, reason)
    }

    fn locale_is_english(&self) -> bool {
        self.settings
            .get_setting("app-common-main", "locale")
            .ok()
            .and_then(|locale| locale.as_str().map(str::to_string))
            .is_some_and(|locale| locale.starts_with("en"))
    }
}

// ---- helpers ---------------------------------------------------------------

/// `auto-select-main.*` from `shared/i18n/{en,zh-CN}/main.yaml`, for the notices
/// the LCU posts into the champion-select channel.
fn notice_text(english: bool, key: &str, champion: &str, seconds: &str, reason: &str) -> String {
    let template = match (english, key) {
        (true, "show-ban") => "Will show ban {{champion}} in {{seconds}} seconds",
        (false, "show-ban") => "将在 {{seconds}} 秒后预禁用 {{champion}}",
        (true, "complete-ban") => "Will complete ban {{champion}} in {{seconds}} seconds",
        (false, "complete-ban") => "将在 {{seconds}} 秒后完成禁用 {{champion}}",
        (true, "show-pick") => "Will show pick {{champion}} in {{seconds}} seconds",
        (false, "show-pick") => "将在 {{seconds}} 秒后亮出 {{champion}}",
        (true, "complete-pick") => "Will complete pick {{champion}} in {{seconds}} seconds",
        (false, "complete-pick") => "将在 {{seconds}} 秒后锁定 {{champion}}",
        (true, "intent-pick") => "Will intent pick {{champion}} in {{seconds}} seconds",
        (false, "intent-pick") => "将在 {{seconds}} 秒后预选 {{champion}}",
        (true, "bench-swap") => "Will swap {{champion}} in {{seconds}} seconds",
        (false, "bench-swap") => "即将在 {{seconds}} 秒后交换 {{champion}}",
        (true, "accept-champion-swap") => {
            "Will accept swap request for {{champion}} in {{seconds}} seconds"
        }
        (false, "accept-champion-swap") => "将在 {{seconds}} 秒后接受交换请求 {{champion}}",
        (true, "decline-champion-swap") => {
            "Will decline swap request for {{champion}} in {{seconds}} seconds"
        }
        (false, "decline-champion-swap") => "将在 {{seconds}} 秒后拒绝交换请求 {{champion}}",
        (true, "error-ban") => {
            "Failed to auto ban champion, target champion {{champion}}: {{reason}}"
        }
        (false, "error-ban") => "尝试自动禁用英雄时失败，目标英雄 {{champion}}：{{reason}}",
        (true, "error-pick") => {
            "Failed to auto pick champion, target champion {{champion}}: {{reason}}"
        }
        (false, "error-pick") => "尝试自动选择英雄时失败，目标英雄 {{champion}}：{{reason}}",
        (true, "error-intent") => {
            "Failed to auto intent champion, target champion {{champion}}: {{reason}}"
        }
        (false, "error-intent") => "尝试自动预选英雄失败，目标英雄 {{champion}}：{{reason}}",
        (_, other) => other,
    };

    template
        .replace("{{champion}}", champion)
        .replace("{{seconds}}", seconds)
        .replace("{{reason}}", reason)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

fn duration_of(ms: f64) -> Duration {
    // a negative delay is how the original fires "immediately"; `Duration` cannot
    // represent one, so it becomes zero
    Duration::from_millis(ms.max(0.0) as u64)
}

fn seconds_text(ms: f64) -> String {
    format!("{:.1}", ms / 1_000.0)
}

fn object_of(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn ids_of(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.as_i64())
        .collect()
}

fn expected_json(list: Option<&Vec<ExpectedChampion>>) -> Value {
    match list {
        None => Value::Null,
        Some(entries) => Value::Array(
            entries
                .iter()
                .map(|entry| json!({ "id": entry.id, "status": entry.status.as_str() }))
                .collect(),
        ),
    }
}

fn flag(session: Option<&Value>, field: &str) -> bool {
    session
        .and_then(|value| value.get(field))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn phase_of(timer: Option<&Value>) -> Option<&str> {
    riftflow_auto_select::timer_phase(timer)
}

fn is_type(action: Option<&Value>, kind: &str) -> bool {
    action.and_then(|value| value.get("type")).and_then(Value::as_str) == Some(kind)
}

fn champion_id_of(action: Option<&Value>) -> i64 {
    action
        .and_then(|value| value.get("championId"))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

fn my_team_of(session: Option<&Value>) -> Vec<Value> {
    session
        .and_then(|value| value.get("myTeam"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `state.member`: the myTeam entry whose cell is this player's.
fn member_of(session: Option<&Value>) -> Option<&Value> {
    let session = session?;
    let cell = session.get("localPlayerCellId").and_then(Value::as_i64)?;

    session
        .get("myTeam")
        .and_then(Value::as_array)?
        .iter()
        .find(|member| member.get("cellId").and_then(Value::as_i64) == Some(cell))
}

fn task_number(task: &Option<Value>, field: &str) -> Option<i64> {
    task.as_ref()
        .and_then(|value| value.get(field))
        .and_then(Value::as_i64)
}

/// `trackCooldown`: keep each champion's original timestamp, stamp the new ones.
fn track_cooldown(previous: Option<&HashMap<i64, i64>>, current: &[i64], now: i64) -> HashMap<i64, i64> {
    current
        .iter()
        .map(|id| {
            let seen = previous.and_then(|map| map.get(id)).copied().unwrap_or(now);
            (*id, seen)
        })
        .collect()
}

/// The pushed copy of a delayed task: the same fields the reaction wrote, with
/// `startAt` carried over from the task it replaces.
fn delayed_task_json(previous: &Option<Value>, mut fields: Value, now: i64) -> Value {
    let start_at = previous
        .as_ref()
        .and_then(|value| value.get("startAt"))
        .and_then(Value::as_i64)
        .unwrap_or(now);

    let Some(map) = fields.as_object_mut() else {
        return fields;
    };

    map.insert("startAt".to_string(), json!(start_at));
    map.insert("finishAt".to_string(), json!(now + map["delayMs"].as_f64().unwrap_or(0.0) as i64));
    Value::Object(map.clone())
}

/// `createNewEmptyPickConfig`.
fn empty_pick_config() -> Value {
    json!({
        "enabled": false,
        "champions": { "top": [], "jungle": [], "middle": [], "bottom": [], "utility": [], "default": [] },
        "ignoreIntent": false,
        "showIntent": false,
        "delaySeconds": 0,
        "strategy": SHOW_AND_LOCK,
        "benchHandleTradeEnabled": false,
        "benchSelectFirstAvailableChampion": false,
        "benchSwapAccumulatedDelaySeconds": 2.9
    })
}

/// `createNewEmptyBanConfig`.
fn empty_ban_config() -> Value {
    json!({
        "enabled": false,
        "champions": { "top": [], "jungle": [], "middle": [], "bottom": [], "utility": [], "default": [] },
        "delaySeconds": 0,
        "strategy": SHOW_AND_LOCK
    })
}

/// `_.mergeWith(prev, patch, (a, b) => Array.isArray(a) ? b : undefined)`: lists
/// of champion ids replace wholesale, everything else merges key by key.
fn merge_config(base: &Value, patch: &Value) -> Value {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            let mut out = base.clone();

            for (key, value) in patch {
                let merged = match out.get(key) {
                    Some(existing @ Value::Object(_)) => merge_config(existing, value),
                    // an array (or a scalar) on either side is simply replaced
                    _ => value.clone(),
                };
                out.insert(key.clone(), merged);
            }

            Value::Object(out)
        }
        _ => patch.clone(),
    }
}

/// `setPickConfig` / `setBanConfig` on the settings object, then the debounced
/// write the original's `settingService.set` performed.
async fn write_group_config(
    settings: &Arc<SettingFactory>,
    key: &str,
    args: &[Value],
    empty: fn() -> Value,
) {
    let Some(group_id) = args.first().and_then(Value::as_str) else {
        return;
    };
    let patch = args.get(1).cloned().unwrap_or(Value::Null);

    let current = settings
        .get_setting(NAMESPACE, key)
        .ok()
        .unwrap_or_else(|| json!({}));

    let mut all = object_of(&current);
    let base = all.get(group_id).cloned().unwrap_or_else(empty);
    all.insert(group_id.to_string(), merge_config(&base, &patch));

    let _ = settings.set_setting(NAMESPACE, key, Value::Object(all)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn champion_id_lists_replace_instead_of_merging() {
        let base = json!({
            "enabled": true,
            "delaySeconds": 3,
            "champions": { "default": [1, 2, 3] }
        });
        let patch = json!({ "champions": { "default": [9] } });

        let merged = merge_config(&base, &patch);
        assert_eq!(merged["champions"]["default"], json!([9]));
        // untouched keys survive
        assert_eq!(merged["enabled"], json!(true));
        assert_eq!(merged["delaySeconds"], json!(3));
    }

    #[test]
    fn a_patch_of_the_wrong_shape_is_ignored_key_by_key() {
        let base = empty_pick_config();
        let merged = merge_config(&base, &json!({ "strategy": "just-show", "nope": 1 }));

        assert_eq!(merged["strategy"], json!("just-show"));
        assert_eq!(merged["benchSwapAccumulatedDelaySeconds"], json!(2.9));
    }

    #[test]
    fn cooldowns_survive_a_reorder_but_not_a_reappearance() {
        let mut first = HashMap::new();
        first.insert(7, 1_000i64);
        first.insert(9, 1_200);

        // champion 7 is still on the bench, so its timestamp is kept
        let kept = track_cooldown(Some(&first), &[9, 7], 5_000);
        assert_eq!(kept.get(&7), Some(&1_000));
        assert_eq!(kept.get(&9), Some(&1_200));

        // 8 arrives later and gets stamped with the current time
        let grown = track_cooldown(Some(&kept), &[9, 7, 8], 6_000);
        assert_eq!(grown.get(&8), Some(&6_000));
        assert_eq!(grown.get(&7), Some(&1_000));

        // dropped from the bench, then back on: a fresh timestamp
        let gone = track_cooldown(Some(&grown), &[9], 7_000);
        assert_eq!(gone.get(&9), Some(&1_200));
        let back = track_cooldown(Some(&gone), &[7], 8_000);
        assert_eq!(back.get(&7), Some(&8_000));
    }

    #[test]
    fn delayed_task_keeps_its_start_time() {
        let previous = json!({ "startAt": 123, "championId": 5 });
        let pushed = delayed_task_json(
            &Some(previous),
            json!({ "championId": 6, "delayMs": 1_500.0 }),
            900,
        );

        assert_eq!(pushed["startAt"], json!(123));
        assert_eq!(pushed["finishAt"], json!(2_400));
        assert_eq!(pushed["championId"], json!(6));

        let fresh = delayed_task_json(&None, json!({ "championId": 6, "delayMs": 0.0 }), 50);
        assert_eq!(fresh["startAt"], json!(50));
        assert_eq!(fresh["finishAt"], json!(50));
    }

    #[test]
    fn negative_delays_become_an_immediate_timer() {
        assert_eq!(duration_of(-1_100.0), Duration::ZERO);
        assert_eq!(duration_of(250.4), Duration::from_millis(250));
        assert_eq!(seconds_text(2_900.0), "2.9");
        assert_eq!(seconds_text(0.0), "0.0");
    }

    #[test]
    fn expected_champions_serialize_as_the_renderers_shape() {
        assert_eq!(expected_json(None), Value::Null);

        let list = vec![ExpectedChampion {
            id: riftflow_auto_select::CHEERY_BRAVERY_ID,
            status: ChampionStatus::Pickable,
        }];
        assert_eq!(
            expected_json(Some(&list)),
            json!([{ "id": -3, "status": "pickable" }])
        );
    }

    #[test]
    fn localized_notices_interpolate_like_i18next() {
        assert_eq!(
            notice_text(true, "show-pick", "Ahri (103)", "3.0", ""),
            "Will show pick Ahri (103) in 3.0 seconds"
        );
        assert_eq!(
            notice_text(false, "complete-ban", "亚索 (157)", "1.5", ""),
            "将在 1.5 秒后完成禁用 亚索 (157)"
        );
        assert_eq!(
            notice_text(false, "intent-pick", "亚索 (157)", "0.0", ""),
            "将在 0.0 秒后预选 亚索 (157)"
        );
        assert_eq!(
            notice_text(
                false,
                "error-pick",
                "亚索 (157)",
                "",
                "Request failed with status code 400"
            ),
            "尝试自动选择英雄时失败，目标英雄 亚索 (157)：Request failed with status code 400"
        );
        assert_eq!(
            notice_text(true, "decline-champion-swap", "阿狸 (103)", "2.0", ""),
            "Will decline swap request for 阿狸 (103) in 2.0 seconds"
        );
    }
}
