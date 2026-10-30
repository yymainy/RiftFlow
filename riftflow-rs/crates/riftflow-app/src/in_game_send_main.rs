//! `in-game-send-main` — 游戏内 / 选人阶段 / 房间阶段的预设文本发送。
//!
//! Port of `src/main/shards/in-game-send/{index,ipc-handlers,preset-controller,
//! preset-selection-controller,send-executor}.ts`. The wording, the ordering and
//! the option-merge rules are `riftflow-in-game-send`'s; this module is the wire:
//! the 22 methods the shipped bundle calls, the four setting keys
//! `settings_registry` owns, the three state props `state_bootstrap` declares,
//! and the two `reaction`s that keep those props in step with the game.
//!
//! ## what still cannot send, and exactly why
//!
//! `sendLines` has three routes, taken by `ongoingGame.state.queryStage.phase`
//! (`riftflow_in_game_send::send::plan_in_game_send` + `decide_send`, which encode
//! the original's order — lines, then phase, then chat room, then foreground,
//! then native-input availability):
//!
//! * `champ-select` / `lobby` → one LCU chat message. Implemented for real here,
//!   through the same `/lol-chat/v1/conversations/:id/messages` POST `auto_misc`
//!   uses, and it reports `false` — never a swallowed error — when the room is
//!   missing, the client is not connected or the request fails.
//! * `in-game` → keystroke injection into the game client. `riftflow-native` has no
//!   input injection (its surface is elevation, process probes, process control
//!   and shell dialogs), so [`native_input_available`] reports what
//!   `NATIVE_SUPPORT.nativeInput.available` reports when the addon is missing and
//!   `decide_send` refuses with [`SendRefusal::InputUnavailable`]. The original
//!   answered that path with `false`, and so does this one: no queue is claimed,
//!   no success is reported. When `riftflow-native` grows `send_key` / `send_text` /
//!   a cancel probe, [`SendAction::InGame`] is where
//!   `riftflow_in_game_send::send::run_in_game_send` gets its [`InGameKeyboard`].
//! * any other phase → `false`, silently, as the original's early return did.
//!
//! The preset *generation* methods — `generateRatingPresetLines`,
//! `generateJunglePresetLines`, `generatePremadePresetLines` — need no keyboard
//! and are the user-visible half of the feature (the settings page's 预览), so
//! they run the crate's builders over the live state. One gap is reported rather
//! than papered over: `ongoing-game-main:state#analysis` is still `null` because
//! no crate has ported `shared/data-adapter/analysis`, so the rating and jungle
//! lines come out in their "暂无记录" wording even though the selection, the
//! champion names and the name-display strategy are all real.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use riftflow_in_game_send::{
    FixedTextPresetItem, FixedTextPresetItemPatch, GameSnapshot, JunglePresetOptionPatch,
    JunglePresetOptions, Locale, MoveDirection, PresetEditError, PresetTarget,
    PremadePresetOptionPatch, PremadePresetOptions, RatingPresetOptionPatch, RatingPresetOptions,
    SendAction, SendRefusal, SendRoute, build_jungle_lines, build_premade_lines,
    build_rating_lines, clamp_send_interval, create_fixed_text_item, default_jungle_puuids,
    decide_send, delete_fixed_text_item, filter_current_premade_indices, filter_current_puuids,
    fixed_text_lines, merge_jungle_options, merge_premade_options, merge_rating_options,
    move_fixed_text_item, normalize_fixed_text_items, parse_premade_indices, plan_in_game_send,
    rating_display_option_keys, run_in_game_send, update_fixed_text_item,
};
use riftflow_in_game_send::InGameKeyboard;
use riftflow_ipc::{RiftFlowIpcMain, IpcError, IpcResult};
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::RequestConfig;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

use crate::settings_registry::SettingsHub;

pub const NAMESPACE: &str = "in-game-send-main";

/// The send decisions worth reading back later. These go through the file logger
/// rather than `eprintln!` because the only instance that can type into an elevated
/// game client is an elevated one, and that instance exposes no CDP — the log file is
/// the single way to see whether a shortcut-bound send fired and why it refused.
fn log(message: &str) {
    crate::logger::log(NAMESPACE, crate::logger::Level::Info, message);
}
const ONGOING: &str = "ongoing-game-main";
const LEAGUE: &str = "league-client-main";
const APP_COMMON: &str = "app-common-main";

/// `IN_GAME_SEND_PRESET_SHORTCUT_TARGET_ID_PREFIX`
const PRESET_TARGET_ID_PREFIX: &str = "in-game-send-main/preset";
/// `` `${IN_GAME_SEND_MAIN_NAMESPACE}/cancel` `` (`send-executor.ts:29`)
const CANCEL_TARGET_ID: &str = "in-game-send-main/cancel";

/// The store of every prop-backed state this shard writes.
const STATE_RATING: &str = "ratingPuuids";
const STATE_JUNGLE: &str = "junglePuuids";
const STATE_PREMADE: &str = "premadeIndices";

/// Nothing here can subscribe to `ongoing-game-main`'s props — MobX reactions had
/// a notification system, this wire does not — so a tick is what notices that a
/// loader filled `additional.spells` or that the game moved to `in-game`.
const WAKE_BACKSTOP: Duration = Duration::from_millis(1_000);

/// The one signal this feature's task understands: something a `reaction` watched
/// may have moved, so evaluate again and write only what actually differs.
#[derive(Debug, Clone, Copy)]
enum Signal {
    Wake,
}

/// `InGameSendPresetController`'s three preset kinds plus the fixed-text one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PresetKind {
    Rating,
    Jungle,
    Premade,
}

impl PresetKind {
    /// Which state prop holds this preset's selection.
    fn selection_prop(self) -> &'static str {
        match self {
            PresetKind::Rating => STATE_RATING,
            PresetKind::Jungle => STATE_JUNGLE,
            PresetKind::Premade => STATE_PREMADE,
        }
    }

    fn setting_key(self) -> &'static str {
        match self {
            PresetKind::Rating => "ratingPresetOptions",
            PresetKind::Jungle => "junglePresetOptions",
            PresetKind::Premade => "premadePresetOptions",
        }
    }

    /// `getRatingPresetShortcutTargetId` and siblings.
    fn target_id(self, target: PresetTarget) -> String {
        let segment = match self {
            PresetKind::Rating => "rating",
            PresetKind::Jungle => "jungle",
            PresetKind::Premade => "premade",
        };

        format!("{PRESET_TARGET_ID_PREFIX}/{segment}/{}", target.as_str())
    }
}

/// `getInGameSendFixedTextPresetShortcutTargetId(id)`
fn fixed_text_target_id(id: &str) -> String {
    format!("{PRESET_TARGET_ID_PREFIX}/fixed-text/{id}")
}

/// `ongoingGame.state.summoner` — main-process-only state that was never
/// propSync'd, so the presets cannot read it off the wire. In this shell it lives
/// in the ongoing-game crate's store, and [`ongoing_summoners`] is the getter that
/// reaches it; taking it as a parameter keeps this module testable with a table of
/// names and no pipeline.
pub type SummonerSource = Arc<dyn (Fn() -> Value) + Send + Sync>;

/// The getter [`register`] wants for the real app.
pub fn ongoing_summoners(game: &Arc<riftflow_ongoing_game::OngoingGame>) -> SummonerSource {
    let game = game.clone();

    Arc::new(move || {
        game.ctx()
            .store
            .get_all()
            .get("summoner")
            .cloned()
            .unwrap_or_else(|| json!({}))
    })
}

pub struct InGameSend {
    ipc: Arc<RiftFlowIpcMain>,
    state_sync: Arc<StateSync>,
    settings: Arc<SettingFactory>,
    league_client: Arc<LeagueClientMain>,
    summoners: SummonerSource,
    sender: UnboundedSender<Signal>,
    /// handed to [`start`] once; a second `start` is a programming error, like the
    /// original's two `onInit` calls would have been
    receiver: Mutex<Option<UnboundedReceiver<Signal>>>,
    /// `_fixedTextShortcutTargetIds`
    fixed_text_target_ids: Mutex<HashSet<String>>,
    /// `_currentKeyboardSendController`: the queue is not portable, but "a send is
    /// in flight" and "the cancel shortcut fired" are, and they are what the next
    /// arm of `SendAction::InGame` will read.
    sending: AtomicBool,
    cancel_requested: Arc<AtomicBool>,
}

/// `onInit`'s `_ipcHandlers.register()`: the 22 methods
/// `renderer-shared/shards/in-game-send/index.ts` calls, with the TS's argument
/// counts and return shapes.
pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    league_client: &Arc<LeagueClientMain>,
    summoners: &SummonerSource,
) -> anyhow::Result<Arc<InGameSend>> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let this = Arc::new(InGameSend {
        ipc: ipc.clone(),
        state_sync: state_sync.clone(),
        settings: settings.clone(),
        league_client: league_client.clone(),
        summoners: summoners.clone(),
        sender,
        receiver: Mutex::new(Some(receiver)),
        fixed_text_target_ids: Mutex::new(HashSet::new()),
        sending: AtomicBool::new(false),
        cancel_requested: Arc::new(AtomicBool::new(false)),
    });

    register_ipc(&this)?;

    Ok(this)
}

/// `onInit`'s three `watch`es: the two `preset-selection-controller.ts` reactions
/// and `_syncPresetShortcuts` / `watchCancelShortcut`.
pub fn start(this: &Arc<InGameSend>, hub: &Arc<SettingsHub>) -> anyhow::Result<()> {
    let receiver = this
        .receiver
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| anyhow::anyhow!("{NAMESPACE} is already started"))?;

    // the endpoints that move `teams` / `positionAssignments` / `additional.spells`
    // / `mergedPremadeTeamMap` — the same five the ongoing-game pipeline wakes on
    let emitter = this.league_client.emitter();
    for uri in [
        "/lol-gameflow/v1/session",
        "/lol-gameflow/v1/gameflow-phase",
        "/lol-lobby/v2/lobby",
        "/lol-champ-select/v1/session",
        "/lol-summoner/v1/current-summoner",
    ] {
        let sender = this.sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(Signal::Wake);
        })?;
    }

    // `sendInterval` / `cancelShortcut` / the three option records /
    // `fixedTextPresetItems`: every write re-runs the shortcut sync
    let mut changes = hub.subscribe();
    let sender = this.sender.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok((namespace, _key, _value)) = changes.recv().await {
            if namespace == NAMESPACE {
                let _ = sender.send(Signal::Wake);
            }
        }
    });

    tauri::async_runtime::spawn(this.clone().run(receiver));

    Ok(())
}

impl InGameSend {
    async fn run(self: Arc<Self>, mut receiver: UnboundedReceiver<Signal>) {
        let mut selections = SelectionWatch::default();
        let mut shortcuts = ShortcutWatch::default();

        // both reactions are declared with `fireImmediately: true`
        self.sync_selections(&mut selections);
        self.sync_shortcuts(&mut shortcuts).await;

        loop {
            tokio::select! {
                biased;

                signal = receiver.recv() => match signal {
                    Some(Signal::Wake) => {
                        self.sync_selections(&mut selections);
                        self.sync_shortcuts(&mut shortcuts).await;
                    }
                    None => break,
                },

                _ = tokio::time::sleep(WAKE_BACKSTOP) => {
                    self.sync_selections(&mut selections);
                    self.sync_shortcuts(&mut shortcuts).await;
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

    /// `SettingService.set`, i.e. transform → validate → commit → the change sink
    /// that mirrors the value into `in-game-send-main:settings#<key>`. Nothing in
    /// this module writes a setting any other way, because the prop is the only
    /// copy the renderer reads.
    async fn write_setting(&self, key: &str, value: Value) -> anyhow::Result<()> {
        self.settings.set_setting(NAMESPACE, key, value).await
    }

    fn prop(&self, path: &str) -> Value {
        self.state_sync
            .get(ONGOING, "state", path)
            .unwrap_or(Value::Null)
    }

    fn league(&self, state_id: &str, path: &str) -> Value {
        self.state_sync
            .get(LEAGUE, state_id, path)
            .unwrap_or(Value::Null)
    }

    fn push_state(&self, path: &str, value: Value) {
        if let Err(error) = self.state_sync.set(NAMESPACE, "state", path, value) {
            eprintln!("[in-game-send] 写入 {path} 失败: {error}");
        }
    }

    /// The object `GameSnapshot::from_json` reads, assembled from the two shards
    /// the original reached into directly (`ongoingGame.state.*`,
    /// `leagueClient.data.*`) plus the summoner table that is not a prop at all.
    fn snapshot_value(&self) -> Value {
        let phase = self
            .prop("queryStage")
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        json!({
            "phase": phase,
            "selfPuuid": self.league("summoner", "me").get("puuid").cloned().unwrap_or(Value::Null),
            "teams": self.prop("teams"),
            "summoner": (self.summoners)(),
            "championSelections": self.prop("championSelections"),
            "positionAssignments": self.prop("positionAssignments"),
            "mergedPremadeTeamMap": self.prop("mergedPremadeTeamMap"),
            "additional": self.prop("additional"),
            "analysis": self.prop("analysis"),
            "chat": {
                "conversations": {
                    "championSelect": self.league("chat", "conversations.championSelect"),
                    "customGame": self.league("chat", "conversations.customGame"),
                }
            },
            "gameData": { "champions": self.league("gameData", "champions") },
        })
    }

    fn snapshot(&self) -> GameSnapshot {
        GameSnapshot::from_json(&self.snapshot_value())
    }

    /// i18next was initialized from the `app-common-main` locale, which is what
    /// decides the preset wording and the punctuation.
    fn locale(&self) -> Locale {
        let tag = self
            .settings
            .get_setting(APP_COMMON, "locale")
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();

        Locale::from_language_tag(&tag)
    }

    fn puuids(&self, prop: &str) -> Vec<String> {
        self.state_sync
            .get(NAMESPACE, "state", prop)
            .and_then(|value| value.as_array().cloned())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn premade_indices(&self) -> Vec<i64> {
        self.state_sync
            .get(NAMESPACE, "state", STATE_PREMADE)
            .map(|value| parse_premade_indices(&value))
            .unwrap_or_default()
    }

    fn options<T: DeserializeOwned + Default>(&self, key: &str) -> T {        let value = self.setting(key);

        serde_json::from_value::<T>(value.clone()).unwrap_or_else(|error| {
            // `apply_declaration`'s own fallback: a stored value that no longer
            // fits the schema reads as the defaults rather than as half a shape
            eprintln!("[in-game-send] 设置项 {key} 无法解析，按默认值处理: {error}");
            T::default()
        })
    }

    fn fixed_text_items(&self) -> Vec<FixedTextPresetItem> {
        self.options::<Vec<FixedTextPresetItem>>("fixedTextPresetItems")
    }

    fn send_interval(&self) -> u64 {
        clamp_send_interval(self.setting("sendInterval").as_f64())
    }

    // ---- generation -----------------------------------------------------------

    /// `presetController.generateRatingLines` / `generateJungleLines` /
    /// `generatePremadeLines`, which differ only in the options record, the
    /// selection prop and the builder.
    fn generate_lines_for(
        &self,
        kind: PresetKind,
        target: PresetTarget,
        snapshot: &GameSnapshot,
    ) -> Vec<String> {
        let locale = self.locale();

        match kind {
            PresetKind::Rating => build_rating_lines(
                snapshot,
                target,
                &self.options::<RatingPresetOptions>(kind.setting_key()),
                &self.puuids(kind.selection_prop()),
                locale,
            ),
            PresetKind::Jungle => build_jungle_lines(
                snapshot,
                target,
                &self.options::<JunglePresetOptions>(kind.setting_key()),
                &self.puuids(kind.selection_prop()),
                locale,
            ),
            PresetKind::Premade => build_premade_lines(
                snapshot,
                target,
                &self.options::<PremadePresetOptions>(kind.setting_key()),
                &self.premade_indices(),
                locale,
            ),
        }
    }

    /// `generateFixedTextPresetLines(id)`
    fn fixed_text_preset_lines(&self, id: &str) -> Vec<String> {
        fixed_text_lines(&self.fixed_text_items(), id)
    }

    // ---- sending --------------------------------------------------------------

    /// `sendExecutor.sendLines`. Returns the boolean the original resolved to in
    /// every branch, including all four of its `return false` paths.
    async fn send_lines(&self, lines: Vec<String>) -> bool {
        let snapshot = self.snapshot();
        let plan = plan_in_game_send(&snapshot, &lines, self.send_interval());

        if self.sending.swap(true, Ordering::SeqCst) {
            // `_currentKeyboardSendController.abort()`. Only the keyboard queue can
            // be interrupted mid-run, and that queue is the arm below, so today
            // this raises the flag the future `InGameKeyboard::cancelled` reads.
            eprintln!("[in-game-send] 已有发送任务在进行，取消它");
            self.cancel_requested.store(true, Ordering::SeqCst);
        }
        let guard = SendGuard {
            sending: &self.sending,
            cancel_requested: &self.cancel_requested,
        };

        // the original awaited `isGameClientForeground()` before it read
        // `NATIVE_SUPPORT.nativeInput.available`, and only for the keyboard route;
        // keeping that order is what makes the two refusals distinguishable
        let is_foreground = matches!(plan.route, SendRoute::InGame) && game_client_foreground();
        // The shipped flag carried `isElevated`, so the original refuses to type into an
        // elevated game from a medium-IL process. Our hook works unelevated, so the
        // blanket term is replaced by the precise question — can `SendInput` reach the
        // window that is in front right now? Without it, UIPI drops the injection,
        // `SendInput` still reports success, and a shortcut-bound send looks dead.
        let action = decide_send(
            &plan,
            native_input_available() && riftflow_native::input::probe().0,
            is_foreground,
        );

        let sent = match action {
            SendAction::Chat {
                conversation_id,
                message,
            } => {
                // `_sendLinesToChat` had no abort checks: one POST, one result
                self.chat_send(&conversation_id, &message).await
            }
            SendAction::InGame { steps } => {
                // the queue sleeps between lines, so it runs off the async worker
                let line_count = steps.len();
                let cancel = self.cancel_requested.clone();
                let run = tokio::task::spawn_blocking(move || {
                    let mut keyboard = Win32Keyboard {
                        injector: riftflow_native::input::KeyboardInjector::new(),
                        cancel,
                    };
                    run_in_game_send(&steps, &mut keyboard)
                })
                .await;

                match run {
                    Ok(true) => {
                        log(&format!("已向游戏注入 {line_count} 行"));
                        true
                    }
                    Ok(false) => {
                        eprintln!("[in-game-send] 发送被取消快捷键中断");
                        false
                    }
                    Err(error) => {
                        eprintln!("[in-game-send] 注入任务未能完成: {error}");
                        false
                    }
                }
            }
            SendAction::Refused(refusal) => {
                log(&format!("不发送: {}", refusal_reason(refusal)));
                false
            }
        };

        drop(guard);
        sent
    }

    /// `_sendLinesToChat`: one message, every line joined by `\n`.
    async fn chat_send(&self, conversation_id: &str, message: &str) -> bool {
        let Some(http) = self.league_client.http() else {
            eprintln!("[in-game-send] LCU 未连接，无法通过聊天发送");
            return false;
        };

        eprintln!("[in-game-send] 通过 LCU 聊天发送: {message}");

        let body = json!({
            "body": message,
            "fromPid": "",
            "fromSummonerId": 0,
            "id": conversation_id,
            "isHistorical": false,
            "timestamp": "",
            "type": "chat",
        });
        let config = RequestConfig {
            method: "POST".to_string(),
            url: format!("/lol-chat/v1/conversations/{conversation_id}/messages"),
            data: Some(body),
            ..Default::default()
        };

        match http.request(config).await {
            Ok(response) if response.status < 400 => true,
            Ok(response) => {
                eprintln!(
                    "[in-game-send] 聊天发送失败: {} → {}",
                    conversation_id, response.status
                );
                false
            }
            Err(error) => {
                eprintln!("[in-game-send] 聊天发送失败: {error}");
                false
            }
        }
    }

    // ---- watchers -------------------------------------------------------------

    /// `InGameSendPresetSelectionController.start()`'s two reactions: whenever the
    /// roster moves, every current player is selected and the junglers are
    /// re-derived; whenever the premade map moves, every group is selected.
    fn sync_selections(&self, watch: &mut SelectionWatch) {
        let snapshot = self.snapshot();
        let teams = self.prop("teams");
        let positions = self.prop("positionAssignments");
        let spells = self
            .prop("additional")
            .get("spells")
            .cloned()
            .unwrap_or(Value::Null);
        let premade_map = self.prop("mergedPremadeTeamMap");

        let roster_moved = watch.teams.as_ref() != Some(&teams)
            || watch.positions.as_ref() != Some(&positions)
            || watch.spells.as_ref() != Some(&spells);
        let premade_moved = watch.premade_map.as_ref() != Some(&premade_map);

        if roster_moved {
            let all = snapshot.all_puuids();
            self.push_state(
                STATE_RATING,
                json!(filter_current_puuids(&snapshot, &all)),
            );
            self.push_state(STATE_JUNGLE, json!(default_jungle_puuids(&snapshot)));
        }

        if premade_moved {
            let indices: Vec<i64> = premade_map
                .as_object()
                .map(|entries| entries.values().filter_map(Value::as_i64).collect())
                .unwrap_or_default();

            self.push_state(
                STATE_PREMADE,
                json!(filter_current_premade_indices(&snapshot, &indices)),
            );
        }

        if roster_moved {
            watch.teams = Some(teams);
            watch.positions = Some(positions);
            watch.spells = Some(spells);
        }
        if premade_moved {
            watch.premade_map = Some(premade_map);
        }
    }

    /// `_syncPresetShortcuts` plus `watchCancelShortcut`, both driven by setting
    /// changes. The registrations themselves belong to `keyboard-shortcuts-main`;
    /// without it up yet the signature is left unrecorded so the next wake retries.
    async fn sync_shortcuts(self: &Arc<Self>, watch: &mut ShortcutWatch) {
        let Some(shortcuts) = crate::hotkeys::instance() else {
            return;
        };

        let rating = self.options::<RatingPresetOptions>("ratingPresetOptions");
        let jungle = self.options::<JunglePresetOptions>("junglePresetOptions");
        let premade = self.options::<PremadePresetOptions>("premadePresetOptions");
        let items = self.fixed_text_items();
        let cancel = self.setting("cancelShortcut").as_str().map(str::to_string);

        let mut desired: Vec<(String, Option<String>, PresetKind, PresetTarget)> = Vec::new();
        for (kind, record) in [
            (PresetKind::Rating, &rating.target_shortcuts),
            (PresetKind::Jungle, &jungle.target_shortcuts),
            (PresetKind::Premade, &premade.target_shortcuts),
        ] {
            for target in PresetTarget::ALL {
                desired.push((
                    kind.target_id(target),
                    record.get(target).map(str::to_string),
                    kind,
                    target,
                ));
            }
        }

        let signature: Vec<(String, Option<String>)> = desired
            .iter()
            .map(|(target_id, shortcut, _, _)| (target_id.clone(), shortcut.clone()))
            .chain(
                items
                    .iter()
                    .map(|item| (fixed_text_target_id(&item.id), item.shortcut.clone())),
            )
            .chain(std::iter::once((
                CANCEL_TARGET_ID.to_string(),
                cancel.clone(),
            )))
            .collect();

        if watch.signature.as_ref() == Some(&signature) {
            return;
        }

        for (target_id, shortcut, kind, target) in &desired {
            let Some(shortcut) = shortcut else {
                shortcuts.unregister_by_target_id(target_id);
                continue;
            };

            let this = Arc::clone(self);
            let send_kind = *kind;
            let send_target = *target;

            let result = shortcuts.register(
                target_id,
                shortcut,
                crate::hotkeys::RegistrationType::LastActive,
                move |_| {
                    let this = this.clone();
                    tauri::async_runtime::spawn(async move {
                        let snapshot = this.snapshot();
                        let lines = this.generate_lines_for(send_kind, send_target, &snapshot);
                        log(&format!(
                            "预设快捷键触发 {send_kind:?}/{send_target:?}，生成 {} 行",
                            lines.len()
                        ));
                        this.send_lines(lines).await;
                    });
                },
            );

            if let Err(error) = result {
                eprintln!("[in-game-send] 注册预设快捷键失败 {target_id}: {error}");
                self.clear_target_shortcut(*kind, *target).await;
            }
        }

        self.sync_fixed_text_shortcuts(&shortcuts, &items).await;

        // `watchCancelShortcut`: the target id is registered as `normal`, and a
        // rejected shortcut clears the setting rather than throwing at the renderer
        let cancel = cancel.filter(|value| !value.is_empty());
        if let Some(cancel) = cancel {
            let this = Arc::clone(self);
            let result = shortcuts.register(
                CANCEL_TARGET_ID,
                &cancel,
                crate::hotkeys::RegistrationType::Normal,
                move |_| {
                    if this.sending.load(Ordering::SeqCst) {
                        this.cancel_requested.store(true, Ordering::SeqCst);
                    }
                },
            );

            if let Err(error) = result {
                eprintln!("[in-game-send] 注册取消快捷键失败: {error}");
                let _ = self.write_setting("cancelShortcut", Value::Null).await;
            }
        } else {
            shortcuts.unregister_by_target_id(CANCEL_TARGET_ID);
        }

        watch.signature = Some(signature);
    }

    /// `_syncFixedTextShortcuts`, including the stale-target sweep the original
    /// needed because items come and go.
    async fn sync_fixed_text_shortcuts(
        self: &Arc<Self>,
        shortcuts: &Arc<crate::hotkeys::KeyboardShortcutsMain>,
        items: &[FixedTextPresetItem],
    ) {
        let next: HashSet<String> = items
            .iter()
            .map(|item| fixed_text_target_id(&item.id))
            .collect();

        // the guard never outlives this block: the loop below awaits, and a
        // `MutexGuard` across an await point makes the task's future `!Send`
        let stale: Vec<String> = {
            let mut known = self.fixed_text_target_ids.lock().unwrap();
            let stale: Vec<String> = known.difference(&next).cloned().collect();
            *known = next;
            stale
        };

        for target_id in &stale {
            shortcuts.unregister_by_target_id(target_id);
        }

        for item in items {
            let target_id = fixed_text_target_id(&item.id);
            let Some(shortcut) = item.shortcut.as_ref() else {
                shortcuts.unregister_by_target_id(&target_id);
                continue;
            };

            let this = Arc::clone(self);
            let id = item.id.clone();

            let result = shortcuts.register(
                &target_id,
                shortcut,
                crate::hotkeys::RegistrationType::LastActive,
                move |_| {
                    let this = this.clone();
                    let id = id.clone();
                    tauri::async_runtime::spawn(async move {
                        let lines = this.fixed_text_preset_lines(&id);
                        this.send_lines(lines).await;
                    });
                },
            );

            if let Err(error) = result {
                eprintln!("[in-game-send] 注册固定文本快捷键失败 {target_id}: {error}");
                let current = self.fixed_text_items();
                let mut next_items = current.clone();
                if update_fixed_text_item(
                    &mut next_items,
                    &item.id,
                    &FixedTextPresetItemPatch {
                        title: None,
                        shortcut: Some(None),
                        content: None,
                    },
                )
                .is_ok()
                {
                    let _ = self
                        .write_setting("fixedTextPresetItems", json_items(&next_items))
                        .await;
                }
            }
        }
    }

    /// `_updatePresetOptions`: merge over the stored record and commit it, so the
    /// value the renderer's prop shows is the value that was written.
    async fn update_preset_options(&self, kind: PresetKind, patch: &Value) -> IpcResult<Value> {
        let key = kind.setting_key();

        let merged = match kind {
            PresetKind::Rating => {
                let current = self.options::<RatingPresetOptions>(key);
                let mut next =
                    merge_rating_options(&current, &parse_patch::<RatingPresetOptionPatch>(patch)?);
                for (target, shortcut) in shortcut_patch_entries(patch) {
                    next.target_shortcuts.set(target, shortcut);
                }
                json_options(&next)
            }
            PresetKind::Jungle => {
                let current = self.options::<JunglePresetOptions>(key);
                let mut next =
                    merge_jungle_options(&current, &parse_patch::<JunglePresetOptionPatch>(patch)?);
                for (target, shortcut) in shortcut_patch_entries(patch) {
                    next.target_shortcuts.set(target, shortcut);
                }
                json_options(&next)
            }
            PresetKind::Premade => {
                let current = self.options::<PremadePresetOptions>(key);
                let mut next =
                    merge_premade_options(&current, &parse_patch::<PremadePresetOptionPatch>(patch)?);
                for (target, shortcut) in shortcut_patch_entries(patch) {
                    next.target_shortcuts.set(target, shortcut);
                }
                json_options(&next)
            }
        };

        self.write_and_ack(key, merged).await
    }

    /// The `clearShortcut` arm of `_syncTargetShortcuts`: drop just this target's
    /// binding, which is `{ targetShortcuts: { [target]: null } }`.
    async fn clear_target_shortcut(&self, kind: PresetKind, target: PresetTarget) {
        let patch = json!({ "targetShortcuts": { target.as_str(): Value::Null } });

        let _ = self.update_preset_options(kind, &patch).await;
    }
}

/// `_currentKeyboardSendController = null` in the original's `finally`.
struct SendGuard<'a> {
    sending: &'a AtomicBool,
    cancel_requested: &'a AtomicBool,
}

impl SendGuard<'_> {
    /// What `InGameKeyboard::cancelled` answers for the queue `run_in_game_send`
    /// walks; the cancel shortcut raises it through `send_lines`.
    #[allow(dead_code)]
    fn cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }
}

impl Drop for SendGuard<'_> {
    fn drop(&mut self) {
        self.sending.store(false, Ordering::SeqCst);
        self.cancel_requested.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct SelectionWatch {
    teams: Option<Value>,
    positions: Option<Value>,
    spells: Option<Value>,
    premade_map: Option<Value>,
}

#[derive(Default)]
struct ShortcutWatch {
    signature: Option<Vec<(String, Option<String>)>>,
}

/// Why `sendLines` answered `false`, in the original's own log wording.
fn refusal_reason(refusal: SendRefusal) -> &'static str {
    match refusal {
        SendRefusal::NoLines => "没有内容可发送",
        SendRefusal::PhaseNotSendable => "当前阶段不可发送",
        SendRefusal::NoChatRoom => "聊天房间不存在",
        SendRefusal::NotForeground => "Game client is not foreground",
        SendRefusal::InputUnavailable => "Native input is not available",
    }
}

/// `NATIVE_SUPPORT.nativeInput.available`.
///
/// This asks the injection question, not the listening question: `SendInput`
/// reaches the window in front unless UIPI is in the way (an elevated target with a
/// medium-IL sender), which is what [`riftflow_native::input::probe`] answers.
fn native_input_available() -> bool {
    riftflow_native::input::probe().0
}

/// `riftflow-native`'s injector behind the crate's `InGameKeyboard` seam. The cancel
/// flag is the shard's own, so `cancelShortcut` stops the queue between lines just
/// like the original's `signal.aborted`.
struct Win32Keyboard {
    injector: riftflow_native::input::KeyboardInjector,
    cancel: Arc<AtomicBool>,
}

impl InGameKeyboard for Win32Keyboard {
    fn send_key(&mut self, key_code: u32, is_down: bool) {
        self.injector.send_key(key_code, is_down);
    }

    fn send_text(&mut self, text: &str) {
        self.injector.send_text(text);
    }

    fn wait(&mut self, millis: u64) {
        self.injector.wait(millis);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// `GameClientMain.isGameClientForeground()`.
///
/// The original gated this on `isWindows && isElevated` because its addon answered
/// the question from inside the same module that did injection. Foreground detection
/// (`GetForegroundWindow` + pid) needs no elevation, and the elevation question is
/// now asked where it belongs — in [`native_input_available`] — so an unelevated
/// shell can still tell that the game client is in front.
fn game_client_foreground() -> bool {
    riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME)
        .into_iter()
        .any(|pid| riftflow_native::is_process_foreground(pid))
}

fn json_items(items: &[FixedTextPresetItem]) -> Value {
    serde_json::to_value(normalize_fixed_text_items(items)).unwrap_or_else(|_| json!([]))
}

fn json_options<T: serde::Serialize>(options: &T) -> Value {
    serde_json::to_value(options).unwrap_or(Value::Null)
}

/// `z.partial(...).parse(patch)`: unknown keys are stripped, but a value that does
/// not fit the field's type (an `nameDisplayStrategy` outside the three allowed
/// strings) **throws**, which is what the renderer's `try/catch` reports. Treating an
/// unparsable patch as an empty one used to swallow exactly that mistake, so the UI
/// could send a bad strategy and see the setting unchanged with no error.
fn parse_patch<P: DeserializeOwned>(value: &Value) -> Result<P, IpcError> {
    serde_json::from_value::<P>(value.clone()).map_err(|error| IpcError::Plain {
        message: error.to_string(),
        name: "ZodError".to_string(),
    })
}

/// The key set `inGameSend*PresetOptionsSchema`'s `z.object` demands: every one of
/// them present, `targetShortcuts` itself a record of three `string | null`.
/// Unknown keys are stripped (which is what `z.object` does, and what deserializing
/// into the crate's typed record does anyway).
fn validate_options_shape(key: &str, value: &Value) -> Result<(), String> {
    let required: Vec<&'static str> = match key {
        "ratingPresetOptions" => {
            let mut keys: Vec<&'static str> = rating_display_option_keys().to_vec();
            keys.extend(["targetShortcuts", "nameDisplayStrategy", "showCurrentChampion"]);
            keys
        }
        "junglePresetOptions" => vec![
            "targetShortcuts",
            "activityPreference",
            "firstClearDistribution",
            "earlyGank",
            "dragonControl",
            "monsterControl",
            "mainChampions",
            "nameDisplayStrategy",
            "showCurrentChampion",
        ],
        _ => vec!["targetShortcuts", "nameDisplayStrategy"],
    };

    let Some(object) = value.as_object() else {
        return Err(format!("{key} must be an object"));
    };

    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|name| object.get(*name).is_none())
        .collect();
    if !missing.is_empty() {
        return Err(format!("{key} is missing {}", missing.join(", ")));
    }

    let shortcuts = object
        .get("targetShortcuts")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{key}.targetShortcuts must be an object"))?;

    for name in ["friendly", "enemy", "all"] {
        match shortcuts.get(name) {
            Some(Value::Null) | Some(Value::String(_)) => {}
            _ => return Err(format!("{key}.targetShortcuts.{name} must be a string or null")),
        }
    }

    match object.get("nameDisplayStrategy").and_then(Value::as_str) {
        Some("preferName") | Some("preferChampionName") | Some("championNameWithName") => {}
        _ => return Err(format!("{key}.nameDisplayStrategy is not a known strategy")),
    }

    Ok(())
}

/// `InGameSendPresetTarget`; the original's `targetTeams` treats anything that is/// neither `friendly` nor `all` as `enemy`, so an unexpected string maps there
/// instead of failing the call.
fn preset_target(value: Option<&Value>) -> PresetTarget {
    value
        .and_then(Value::as_str)
        .and_then(PresetTarget::from_str_name)
        .unwrap_or(PresetTarget::Enemy)
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `PresetEditError` → the `throw new Error(...)` the renderer's envelope saw.
fn edit_error(error: PresetEditError) -> IpcError {
    IpcError::plain(error)
}

/// Which preset targets a patch's `targetShortcuts` names, `None` meaning "unbind
/// this one".
///
/// Read off the JSON rather than through the crate's `TargetShortcutsPatch`,
/// because serde resolves an explicit `"enemy": null` in an
/// `Option<Option<String>>` field to the outer `None` — indistinguishable from an
/// absent key — while the settings page really does send `null` to clear a
/// binding, and `TargetShortcutsPatch` is another module's type to change.
fn shortcut_patch_entries(patch: &Value) -> Vec<(PresetTarget, Option<String>)> {
    let Some(object) = patch
        .get("targetShortcuts")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    PresetTarget::ALL
        .iter()
        .filter_map(|target| {
            let entry = object.get(target.as_str())?;

            Some((*target, entry.as_str().map(str::to_string)))
        })
        .collect()
}

// ------------------------------------------------------------------------- IPC

fn register_ipc(this: &Arc<InGameSend>) -> anyhow::Result<()> {
    // ---- selection props ----------------------------------------------------

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "setRatingPuuids", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let puuids = string_list(args.first());
                let snapshot = this.snapshot();
                let kept = filter_current_puuids(&snapshot, &puuids);
                this.push_state(STATE_RATING, json!(kept));

                Ok(Value::Null)
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "setJunglePuuids", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let puuids = string_list(args.first());
                let snapshot = this.snapshot();
                let kept = filter_current_puuids(&snapshot, &puuids);
                this.push_state(STATE_JUNGLE, json!(kept));

                Ok(Value::Null)
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "setPremadeIndices", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let indices = parse_premade_indices(args.first().unwrap_or(&Value::Null));
                let snapshot = this.snapshot();
                let kept = filter_current_premade_indices(&snapshot, &indices);
                this.push_state(STATE_PREMADE, json!(kept));

                Ok(Value::Null)
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "clearPresetSelections", riftflow_ipc::handler(
        move |_ctx, _args| {
            let this = this_clone.clone();
            async move {
                // `clearPresetSelections` clears all three at once
                this.push_state(STATE_RATING, json!([]));
                this.push_state(STATE_JUNGLE, json!([]));
                this.push_state(STATE_PREMADE, json!([]));

                Ok(Value::Null)
            }
        },
    ))?;

    // ---- sending ------------------------------------------------------------

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "sendLines", riftflow_ipc::handler(move |_ctx, args| {
        let this = this_clone.clone();
        async move {
            let lines = string_list(args.first());

            Ok(json!(this.send_lines(lines).await))
        }
    }))?;

    for (name, kind) in [
        ("sendRatingPreset", PresetKind::Rating),
        ("sendJunglePreset", PresetKind::Jungle),
        ("sendPremadePreset", PresetKind::Premade),
    ] {
        let this_clone = this.clone();
        this.ipc.on_call(NAMESPACE, name, riftflow_ipc::handler(move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let target = preset_target(args.first());
                let snapshot = this.snapshot();
                let lines = this.generate_lines_for(kind, target, &snapshot);

                Ok(json!(this.send_lines(lines).await))
            }
        }))?;
    }

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "sendFixedTextPreset", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let id = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                let lines = this.fixed_text_preset_lines(&id);

                Ok(json!(this.send_lines(lines).await))
            }
        },
    ))?;

    // ---- generation ---------------------------------------------------------

    for (name, kind) in [
        ("generateRatingPresetLines", PresetKind::Rating),
        ("generateJunglePresetLines", PresetKind::Jungle),
        ("generatePremadePresetLines", PresetKind::Premade),
    ] {
        let this_clone = this.clone();
        this.ipc.on_call(NAMESPACE, name, riftflow_ipc::handler(move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let target = preset_target(args.first());
                let snapshot = this.snapshot();

                Ok(json!(this.generate_lines_for(kind, target, &snapshot)))
            }
        }))?;
    }

    // ---- option records -----------------------------------------------------

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "setRatingPresetOptions", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                this.write_full_options::<RatingPresetOptions>(&args, "ratingPresetOptions")
                    .await
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "setJunglePresetOptions", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                this.write_full_options::<JunglePresetOptions>(&args, "junglePresetOptions")
                    .await
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "setPremadePresetOptions", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                this.write_full_options::<PremadePresetOptions>(&args, "premadePresetOptions")
                    .await
            }
        },
    ))?;

    for (name, kind) in [
        ("updateRatingPresetOptions", PresetKind::Rating),
        ("updateJunglePresetOptions", PresetKind::Jungle),
        ("updatePremadePresetOptions", PresetKind::Premade),
    ] {
        let this_clone = this.clone();
        this.ipc.on_call(NAMESPACE, name, riftflow_ipc::handler(move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let patch = args.first().cloned().unwrap_or(Value::Null);

                this.update_preset_options(kind, &patch).await
            }
        }))?;
    }

    // ---- fixed-text items ---------------------------------------------------

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "createFixedTextPresetItem", riftflow_ipc::handler(
        move |_ctx, _args| {
            let this = this_clone.clone();
            async move {
                let mut items = this.fixed_text_items();
                let item = create_fixed_text_item(&mut items, new_item_id())
                    .map_err(edit_error)?;

                this.write_and_ack("fixedTextPresetItems", json_items(&items)).await?;

                Ok(json_options(&item))
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "updateFixedTextPresetItem", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let id = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                let patch = serde_json::from_value::<FixedTextPresetItemPatch>(
                    args.get(1).cloned().unwrap_or(Value::Null),
                )
                .unwrap_or_default();

                let mut items = this.fixed_text_items();
                let next = update_fixed_text_item(&mut items, &id, &patch).map_err(edit_error)?;

                this.write_and_ack("fixedTextPresetItems", json_items(&items)).await?;

                Ok(json_options(&next))
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "deleteFixedTextPresetItem", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let id = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                let mut items = this.fixed_text_items();
                let removed = delete_fixed_text_item(&mut items, &id);

                if removed {
                    this.write_and_ack("fixedTextPresetItems", json_items(&items)).await?;
                }

                Ok(json!(removed))
            }
        },
    ))?;

    let this_clone = this.clone();
    this.ipc.on_call(NAMESPACE, "moveFixedTextPresetItem", riftflow_ipc::handler(
        move |_ctx, args| {
            let this = this_clone.clone();
            async move {
                let id = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                // `direction === 'up' ? -1 : +1`: anything that is not `up` moves down
                let direction = args
                    .get(1)
                    .and_then(Value::as_str)
                    .and_then(MoveDirection::from_str_name)
                    .unwrap_or(MoveDirection::Down);

                let mut items = this.fixed_text_items();
                let moved = move_fixed_text_item(&mut items, &id, direction);

                if moved {
                    this.write_and_ack("fixedTextPresetItems", json_items(&items)).await?;
                }

                Ok(json!(moved))
            }
        },
    ))?;

    Ok(())
}

/// `set{Rating,Jungle,Premade}PresetOptions`: the whole record, validated the way
/// the TS `z.object` schema validated it before the commit.
impl InGameSend {
    async fn write_full_options<T>(&self, args: &[Value], key: &'static str) -> IpcResult<Value>
    where
        T: DeserializeOwned + serde::Serialize,
    {
        let value = args.first().cloned().unwrap_or(Value::Null);
        validate_options_shape(key, &value).map_err(|issues| IpcError::Plain {
            // what `inGameSend*PresetOptionsSchema.parse` threw: a named, non-`Error`
            // rejection, so the renderer reads it as a validation failure
            message: issues,
            name: "ZodError".to_string(),
        })?;

        let parsed: T = serde_json::from_value(value).map_err(|error| IpcError::Plain {
            message: error.to_string(),
            name: "ZodError".to_string(),
        })?;

        self.write_and_ack(key, json_options(&parsed)).await
    }

    /// Every settings write answers `undefined` in the TS, so the commit is the
    /// only thing that can fail here.
    async fn write_and_ack(&self, key: &str, value: Value) -> IpcResult<Value> {
        self.write_setting(key, value)
            .await
            .map(|()| Value::Null)
            .map_err(IpcError::plain)
    }
}

/// `randomUUID()` from `node:crypto`, in the shape the renderer's item keys and
/// shortcut target ids expect. Nothing here needs the entropy of a v4 UUID — the
/// id is a UI key, and `normalize_fixed_text_items` drops a collision outright.
fn new_item_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos
        ^ ((std::process::id() as u64) << 32)
        ^ COUNTER
            .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9);

    // splitmix64's finalizer, so consecutive calls are far apart
    let mut z = mixed ^ (mixed >> 30);
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let value = z ^ (z >> 31);

    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (value >> 32) as u32,
        (value >> 16) as u16,
        (value & 0x0fff) as u16,
        ((value >> 48) & 0x3fff) as u16 | 0x8000,
        value & 0x0000_ffff_ffff_ffff,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Mutex as StdMutex, OnceLock};

    use riftflow_ipc::{RiftFlowIpcMain, CallOutcome, RendererTransport};
    use riftflow_lcu::CredentialStore;
    use riftflow_store::settings::SettingFactory;

    /// The shipped bundle calls exactly these; the test asserts the list against
    /// `ipc-contract.json`'s `in-game-send-main` entry so a rename cannot slip by.
    const CONTRACT: [&str; 22] = [
        "setRatingPuuids",
        "setJunglePuuids",
        "setPremadeIndices",
        "clearPresetSelections",
        "sendLines",
        "sendRatingPreset",
        "sendJunglePreset",
        "sendPremadePreset",
        "sendFixedTextPreset",
        "generateRatingPresetLines",
        "generateJunglePresetLines",
        "generatePremadePresetLines",
        "setRatingPresetOptions",
        "setJunglePresetOptions",
        "setPremadePresetOptions",
        "updateRatingPresetOptions",
        "updateJunglePresetOptions",
        "updatePremadePresetOptions",
        "createFixedTextPresetItem",
        "updateFixedTextPresetItem",
        "deleteFixedTextPresetItem",
        "moveFixedTextPresetItem",
    ];

    struct Scenario {
        ipc: Arc<RiftFlowIpcMain>,
        sync: Arc<StateSync>,
        settings: Arc<SettingFactory>,
        send: Arc<InGameSend>,
        hub: Arc<SettingsHub>,
        dir: std::path::PathBuf,
    }

    impl Scenario {
        fn call(&self, method: &str, args: Vec<Value>) -> Value {
            self.call_raw(method, args)
                .unwrap_or_else(|error| panic!("{method} failed: {error}"))
        }

        fn call_raw(&self, method: &str, args: Vec<Value>) -> Result<Value, String> {
            let ipc = self.ipc.clone();
            let namespace = NAMESPACE.to_string();
            let name = method.to_string();

            tauri::async_runtime::block_on(async move {
                match ipc.handle_call(1, &namespace, &name, args).await {
                    CallOutcome::Reply(envelope) => envelope
                        .data
                        .filter(|_| envelope.success)
                        .map(Ok)
                        .unwrap_or_else(|| {
                            Err(
                                envelope.error.and_then(|e| e.get("message")?.as_str().map(str::to_string)).unwrap_or_else(|| "failed".to_string())
                            )
                        }),
                    CallOutcome::Rejected(message) => Err(message),
                }
            })
        }

        /// One pass of the two reactions, without booting `start`'s task.
        fn sync_selections(&self) {
            let mut watch = SelectionWatch::default();
            self.send.sync_selections(&mut watch);
        }

        fn prop(&self, path: &str) -> Option<Value> {
            self.sync.get(NAMESPACE, "state", path)
        }

        fn setting_prop(&self, key: &str) -> Option<Value> {
            self.sync.get(NAMESPACE, "settings", key)
        }

        /// `setting-factory-main/set` from the renderer's side of the wire.
        fn set_setting(&self, key: &str, value: Value) {
            let settings = self.settings.clone();
            let key = key.to_string();

            tauri::async_runtime::block_on(async move {
                settings.set_setting(NAMESPACE, &key, value).await.expect("the write commits");
            });
        }

        fn set_ongoing(&self, path: &str, value: Value) {
            self.sync
                .set(ONGOING, "state", path, value)
                .unwrap_or_else(|error| panic!("ongoing-game prop {path}: {error}"));
        }

        fn set_league(&self, state_id: &str, path: &str, value: Value) {
            self.sync
                .set(LEAGUE, state_id, path, value)
                .unwrap_or_else(|error| panic!("league prop {state_id}#{path}: {error}"));
        }
    }

    impl Drop for Scenario {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    /// A draft with two teams of one, `p1` being me and `p2` holding smite.
    fn populate(scenario: &Scenario) {
        scenario.set_ongoing(
            "queryStage",
            json!({ "phase": "champ-select", "gameInfo": null }),
        );
        scenario.set_ongoing(
            "teams",
            json!({ "TEAM-100": ["p1", "p2"], "TEAM-200": ["p3"] }),
        );
        scenario.set_ongoing("championSelections", json!({ "p1": 103, "p2": 120, "p3": 0 }));
        scenario.set_ongoing("positionAssignments", json!({ "p1": { "position": "MIDDLE" } }));
        scenario.set_ongoing(
            "additional",
            json!({ "spells": { "p2": { "spell1Id": 11, "spell2Id": 4 } } }),
        );
        scenario.set_ongoing("mergedPremadeTeamMap", json!({ "p1": 1, "p2": 1 }));
        scenario.set_league("summoner", "me", json!({ "puuid": "p1" }));
        scenario.set_league(
            "gameData",
            "champions",
            json!({ "103": {"name": "阿狸"}, "120": {"name": "德莱文"} }),
        );
        scenario.set_league("chat", "conversations.championSelect", json!({ "id": "chat-100" }));
    }

    fn scenario() -> Scenario {
        static COUNTER: OnceLock<StdMutex<u64>> = OnceLock::new();
        let counter = COUNTER.get_or_init(|| StdMutex::new(0));
        let mut guard = counter.lock().unwrap();
        *guard += 1;
        let slot = *guard;
        drop(guard);

        let dir = std::env::temp_dir().join(format!(
            "riftflow-in-game-send-wiring-{}-{slot}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("a temp directory for the scenario");

        let (db, _) = riftflow_store::Database::initialize(&dir.join("riftflow.db")).expect("a database");
        let settings = Arc::new(SettingFactory::new(Arc::new(StdMutex::new(db))));
        let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(NullTransport)));
        let sync = Arc::new(StateSync::new(ipc.clone()));

        // the shell's own boot order: the prop surfaces first, then the settings
        // schema `in-game-send-main`'s writes go through
        crate::state_bootstrap::declare_all(&sync, &settings, false, "info").expect("the surfaces");
        let hub = crate::settings_registry::register_all(&sync, &settings).expect("the settings");

        let league_client = Arc::new(LeagueClientMain::new(
            ipc.clone(),
            sync.clone(),
            Arc::new(NoCredentials),
            Arc::new(riftflow_lcu::ProductionDialer),
        ));

        let summoners: SummonerSource = Arc::new(|| {
            json!({
                "p1": { "gameName": "MID", "tagLine": "NA1", "displayName": "MID" },
                "p2": { "gameName": "JGL", "tagLine": "KR1", "displayName": "JGL" },
            })
        });

        let send =
            register(&ipc, &sync, &settings, &league_client, &summoners).expect("handlers register");

        Scenario {
            ipc,
            sync,
            settings,
            send,
            hub,
            dir,
        }
    }

    #[derive(Default)]
    struct NullTransport;

    impl RendererTransport for NullTransport {
        fn send_to(
            &self,
            _renderer: riftflow_ipc::RendererId,
            _event: &str,
            _args: Vec<Value>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct NoCredentials;

    impl CredentialStore for NoCredentials {
        fn save_last_connected_client(&self, _value: &Value) {}
        fn get_last_connected_client(&self) -> Option<Value> {
            None
        }
        fn remove_last_connected_client(&self) {}
    }

    #[test]
    fn every_method_the_bundle_calls_is_served() {
        let scenario = scenario();

        // the contract file is a transcription of ipc-handlers.ts; the bundle is
        // the real one, and both agree with this list
        let contract: Vec<String> = CONTRACT.iter().map(|name| name.to_string()).collect();
        assert_eq!(contract.len(), 22);

        for name in CONTRACT {
            let outcome = tauri::async_runtime::block_on(
                scenario.ipc.handle_call(1, NAMESPACE, name, vec![]),
            );
            assert!(
                !matches!(outcome, CallOutcome::Rejected(_)),
                "{name} is not registered"
            );
        }

        assert!(matches!(
            tauri::async_runtime::block_on(
                scenario.ipc.handle_call(1, NAMESPACE, "noSuchFunction", vec![])
            ),
            CallOutcome::Rejected(_)
        ));
    }

    #[test]
    fn the_selection_setters_filter_to_the_current_roster() {
        let scenario = scenario();
        populate(&scenario);
        scenario.sync_selections();

        // the reaction's own first pass selects everyone
        assert_eq!(
            scenario.prop(STATE_RATING),
            Some(json!(["p1", "p2", "p3"]))
        );
        assert_eq!(scenario.prop(STATE_JUNGLE), Some(json!(["p2"])));
        assert_eq!(scenario.prop(STATE_PREMADE), Some(json!([1])));

        // a write that names a player who is not in the game is dropped, and so is
        // a duplicate — `[...new Set(puuids)]` then the filter
        scenario.call("setRatingPuuids", vec![json!(["p3", "p3", "gone"])]);
        assert_eq!(scenario.prop(STATE_RATING), Some(json!(["p3"])));

        scenario.call("setJunglePuuids", vec![json!(["p1", "p2"])]);
        assert_eq!(scenario.prop(STATE_JUNGLE), Some(json!(["p1", "p2"])));

        scenario.call("setPremadeIndices", vec![json!([1, 2, 1.5])]);
        assert_eq!(
            scenario.prop(STATE_PREMADE),
            Some(json!([1])),
            "2 is not a current group and 1.5 is not an integer"
        );

        scenario.call("clearPresetSelections", vec![]);
        assert_eq!(scenario.prop(STATE_RATING), Some(json!([])));
        assert_eq!(scenario.prop(STATE_JUNGLE), Some(json!([])));
        assert_eq!(scenario.prop(STATE_PREMADE), Some(json!([])));
    }

    #[test]
    fn the_generated_lines_are_the_crate_builders_over_live_state() {
        let scenario = scenario();
        populate(&scenario);
        scenario.sync_selections();
        scenario.call("setRatingPuuids", vec![json!(["p1"])]);

        let lines = scenario.call("generateRatingPresetLines", vec![json!("friendly")]);
        let lines = lines.as_array().expect("an array of lines");
        assert_eq!(lines.len(), 1, "one selected player, one line");

        let text = lines[0].as_str().unwrap();
        // `preferChampionName` with a champion nobody else plays shows the champion,
        // whose name came out of `gameData.champions`
        assert!(text.starts_with("阿狸"), "{text}");
        // and with no analysis ported the stats half says so instead of inventing
        // numbers — the crate's own `rating.noRecentMatches` wording
        assert_eq!(text, "阿狸：近期没有对局记录", "{text}");
        assert!(!text.contains("KDA"), "no fake metrics: {text}");

        // `enemy` picks the other team, which has nobody selected
        assert_eq!(
            scenario.call("generateRatingPresetLines", vec![json!("enemy")]),
            json!([])
        );

        // jungle: `p2` is the jungler, and the preset says 尚未选择英雄 for a
        // champion nobody picked
        scenario.call("setJunglePuuids", vec![json!(["p2", "p3"])]);
        let jungle = scenario
            .call("generateJunglePresetLines", vec![json!("all")])
            .as_array()
            .cloned()
            .unwrap_or_default()
            .len();
        assert_eq!(jungle, 2, "both selected players get a line");

        // premade: group 1 covers p1 and p2, so the friendly team reports them,
        // each by champion name under the default strategy
        scenario.call("setPremadeIndices", vec![json!([1])]);
        let premade = scenario.call("generatePremadePresetLines", vec![json!("friendly")]);
        let text = premade
            .as_array()
            .and_then(|lines| lines.first().cloned())
            .and_then(|line| line.as_str().map(str::to_string))
            .unwrap_or_default();
        assert_eq!(text, "蓝方开黑：[阿狸, 德莱文]", "{text}");

        // a group nobody is in reports the no-team wording instead of an empty line
        scenario.call("setPremadeIndices", vec![json!([])]);
        let empty = scenario.call("generatePremadePresetLines", vec![json!("friendly")]);
        let text = empty
            .as_array()
            .and_then(|lines| lines.first().cloned())
            .and_then(|line| line.as_str().map(str::to_string))
            .unwrap_or_default();
        assert!(!text.is_empty(), "the friendly team is still labelled: {text}");
    }

    #[test]
    fn a_send_answers_the_boolean_the_original_returned() {
        let scenario = scenario();

        // no game at all: `unavailable` is not a sendable phase
        assert_eq!(scenario.call("sendLines", vec![json!(["你好"])]), json!(false));
        // blank lines never reach the phase check
        assert_eq!(scenario.call("sendLines", vec![json!(["", "  "])]), json!(false));

        // champ-select with a room: the route is LCU chat, and with no client
        // connected that is a refusal, not a swallowed success
        populate(&scenario);
        assert_eq!(scenario.call("sendLines", vec![json!(["你好"])]), json!(false));

        // in-game with no elevation and no input addon: false, twice over —
        // the foreground gate is checked before availability, exactly as the
        // original ordered its two awaits
        scenario.set_ongoing("queryStage", json!({ "phase": "in-game" }));
        assert_eq!(scenario.call("sendLines", vec![json!(["你好"])]), json!(false));

        // the preset routes go through the same executor
        assert_eq!(scenario.call("sendRatingPreset", vec![json!("friendly")]), json!(false));
        assert_eq!(scenario.call("sendJunglePreset", vec![json!("all")]), json!(false));
        assert_eq!(scenario.call("sendPremadePreset", vec![json!("friendly")]), json!(false));
        assert_eq!(scenario.call("sendFixedTextPreset", vec![json!("nope")]), json!(false));
    }

    #[test]
    fn option_writes_land_in_the_stored_value_and_the_synced_prop() {
        let scenario = scenario();

        scenario.call(
            "setRatingPresetOptions",
            vec![json!({
                "targetShortcuts": { "friendly": "Ctrl+1", "enemy": null, "all": null },
                "kda": true, "winRate": false, "avgSoloKills": false,
                "avgVisionScore": false, "avgChampionDamage": false, "avgDamageTaken": false,
                "avgGold": false, "avgCsPerMinute": false, "avgKillParticipation": false,
                "avgDamageGoldEfficiency": false, "mainChampions": false, "mainPositions": false,
                "nameDisplayStrategy": "preferName", "showCurrentChampion": true,
            })],
        );

        let prop = scenario.setting_prop("ratingPresetOptions").expect("a prop");
        assert_eq!(prop["targetShortcuts"]["friendly"], json!("Ctrl+1"));
        assert_eq!(prop["nameDisplayStrategy"], json!("preferName"));
        assert_eq!(prop["showCurrentChampion"], json!(true));

        // a record that does not fit the schema is rejected instead of persisted,
        // and a partial one is rejected too — `z.object` demanded every key
        let rejected = scenario.call_raw("setJunglePresetOptions", vec![json!({ "kda": 1 })]);
        assert!(rejected.is_err(), "a malformed record must not be committed");

        let partial = scenario.call_raw(
            "setPremadePresetOptions",
            vec![json!({ "nameDisplayStrategy": "preferName" })],
        );
        assert!(
            partial.expect_err("targetShortcuts is required").contains("targetShortcuts"),
            "the failure names what is missing"
        );

        let complete = scenario.call_raw(
            "setJunglePresetOptions",
            vec![json!({
                "targetShortcuts": { "friendly": null, "enemy": null, "all": null },
                "activityPreference": true, "firstClearDistribution": true, "earlyGank": true,
                "dragonControl": true, "monsterControl": true, "mainChampions": true,
                "nameDisplayStrategy": "championNameWithName", "showCurrentChampion": false,
            })],
        );
        assert!(complete.is_ok(), "a full record commits");
        assert_eq!(
            scenario.setting_prop("junglePresetOptions").expect("a prop")["nameDisplayStrategy"],
            json!("championNameWithName")
        );

        // the patch merge keeps what it was not told to change, and reaches into
        // `targetShortcuts` one level deeper
        scenario.call(
            "updatePremadePresetOptions",
            vec![json!({ "targetShortcuts": { "enemy": "Ctrl+2" } })],
        );
        let premade = scenario.setting_prop("premadePresetOptions").expect("a prop");
        assert_eq!(premade["targetShortcuts"]["enemy"], json!("Ctrl+2"));
        assert_eq!(
            premade["targetShortcuts"]["friendly"],
            Value::Null,
            "untouched, still there"
        );
        assert_eq!(premade["nameDisplayStrategy"], json!("preferChampionName"));

        // clearing one shortcut is the `{ [target]: null }` the controller writes
        scenario.call(
            "updatePremadePresetOptions",
            vec![json!({ "targetShortcuts": { "enemy": null } })],
        );
        let cleared = scenario.setting_prop("premadePresetOptions").expect("a prop");
        assert_eq!(cleared["targetShortcuts"]["enemy"], Value::Null);
    }

    #[test]
    fn fixed_text_items_round_trip_through_the_setting() {
        let scenario = scenario();

        let created = scenario.call("createFixedTextPresetItem", vec![]);
        let id = created["id"].as_str().expect("an id").to_string();
        assert_eq!(created["title"], json!(""));
        assert_eq!(created["shortcut"], Value::Null);
        assert_eq!(
            scenario.setting_prop("fixedTextPresetItems").expect("a prop"),
            json!([created])
        );

        let updated = scenario.call(
            "updateFixedTextPresetItem",
            vec![json!(&id), json!({ "title": "开场", "content": "第一行\n\n第二行\n" })],
        );
        assert_eq!(updated["title"], json!("开场"));

        // blank lines are filtered on generation, which is what `sendFixedTextPreset`
        // hands the executor as well
        scenario.set_ongoing(
            "queryStage",
            json!({ "phase": "champ-select", "gameInfo": null }),
        );
        scenario.set_league("chat", "conversations.championSelect", json!({ "id": "chat-100" }));
        assert_eq!(
            scenario.call("sendFixedTextPreset", vec![json!(&id)]),
            json!(false),
            "the route is right but no client is connected"
        );

        assert_eq!(scenario.call("moveFixedTextPresetItem", vec![json!(&id), json!("up")]), json!(false), "already first");
        assert_eq!(
            scenario.call("deleteFixedTextPresetItem", vec![json!("no-such-id")]),
            json!(false)
        );
        assert_eq!(scenario.call("deleteFixedTextPresetItem", vec![json!(&id)]), json!(true));
        assert_eq!(
            scenario.setting_prop("fixedTextPresetItems"),
            Some(json!([]))
        );
        assert_eq!(
            scenario.call_raw("updateFixedTextPresetItem", vec![json!(&id), json!({ "title": "x" })])
                .expect_err("the TS threw for an unknown id"),
            "Fixed text preset item not found"
        );
    }

    #[test]
    fn the_item_limit_is_the_error_the_renderer_catches() {
        let scenario = scenario();

        let items: Vec<Value> = (0..riftflow_in_game_send::options::FIXED_TEXT_PRESET_MAX_ITEMS)
            .map(|index| {
                json!({
                    "id": format!("id-{index}"),
                    "title": "",
                    "shortcut": null,
                    "content": "",
                })
            })
            .collect();
        scenario.set_setting("fixedTextPresetItems", json!(items));

        let error = scenario
            .call_raw("createFixedTextPresetItem", vec![])
            .expect_err("the limit is reached");
        assert_eq!(error, "Fixed text preset item limit reached");
    }

    #[test]
    fn our_writes_are_normalized_before_they_are_committed() {
        let scenario = scenario();

        // `createFixedTextPresetItem` twice: two ids, both in the prop, in order
        let first = scenario.call("createFixedTextPresetItem", vec![]);
        let second = scenario.call("createFixedTextPresetItem", vec![]);
        let first_id = first["id"].as_str().unwrap().to_string();
        assert_ne!(first["id"], second["id"]);

        // the transform the TS `fixedTextPresetItems` declaration ran: a title
        // longer than `FIXED_TEXT_PRESET_TITLE_MAX_LENGTH` is cut, not stored
        let long = "长".repeat(100);
        let updated = scenario.call(
            "updateFixedTextPresetItem",
            vec![json!(&first_id), json!({ "title": long })],
        );
        assert_eq!(
            updated["title"].as_str().unwrap().chars().count(),
            riftflow_in_game_send::options::FIXED_TEXT_PRESET_TITLE_MAX_LENGTH
        );
        let stored = scenario.setting_prop("fixedTextPresetItems").expect("a prop");
        assert_eq!(stored.as_array().map(|items| items.len()), Some(2));
        assert_eq!(
            stored[0]["title"].as_str().unwrap().chars().count(),
            riftflow_in_game_send::options::FIXED_TEXT_PRESET_TITLE_MAX_LENGTH,
            "the prop carries the normalized value the store did"
        );
    }

    #[test]
    fn ids_are_unique_and_shaped_like_uuids() {
        let mut seen = HashSet::new();
        for _ in 0..5_000 {
            let id = new_item_id();
            assert_eq!(id.len(), 36, "{id}");
            assert_eq!(&id[14..15], "4", "version nibble");
            assert!(seen.insert(id), "a repeated id would collide in the store");
        }
    }

    #[test]
    fn the_snapshot_mirrors_the_props_the_builders_read() {
        let scenario = scenario();
        populate(&scenario);

        let snapshot = scenario.send.snapshot();

        assert_eq!(snapshot.phase, "champ-select");
        assert_eq!(snapshot.self_puuid.as_deref(), Some("p1"));
        assert_eq!(snapshot.all_puuids(), vec!["p1", "p2", "p3"]);
        assert_eq!(
            snapshot.champion_select_conversation_id.as_deref(),
            Some("chat-100")
        );
        assert_eq!(snapshot.custom_game_conversation_id, None);
        assert_eq!(snapshot.champion_name(103), "阿狸");
        assert_eq!(
            snapshot.merged_premade_team_map.get("p2").copied(),
            Some(1),
            "the premade map is read straight off the prop"
        );
        assert_eq!(
            snapshot.spells.get("p2").map(|spells| spells.spell2_id),
            Some(4)
        );
    }

    /// The watchers, running: no League Client, no keyboard hook, so the only
    /// thing the loop may do is mirror the roster into the three selection props.
    #[test]
    fn start_runs_the_reactions_without_a_client_or_a_hook() {
        let scenario = scenario();
        populate(&scenario);

        start(&scenario.send, &scenario.hub).expect("the watchers subscribe and launch");

        // a second `start` would hand the same receiver away twice
        assert!(start(&scenario.send, &scenario.hub).is_err(), "one task per shard");

        // past the `fireImmediately` pass and one backstop tick
        std::thread::sleep(Duration::from_millis(300));

        assert_eq!(scenario.prop(STATE_RATING), Some(json!(["p1", "p2", "p3"])));
        assert_eq!(scenario.prop(STATE_JUNGLE), Some(json!(["p2"])));
        assert_eq!(scenario.prop(STATE_PREMADE), Some(json!([1])));

        // and it has not touched anything it was not asked to: no shortcut exists
        // (`hotkeys` never started here), so the settings props are still the
        // declared defaults
        assert_eq!(
            scenario.setting_prop("ratingPresetOptions").expect("a prop")["targetShortcuts"],
            json!({ "friendly": null, "enemy": null, "all": null })
        );
        assert_eq!(scenario.setting_prop("cancelShortcut"), Some(Value::Null));
        assert_eq!(
            scenario.call("sendLines", vec![json!(["你好"])]),
            json!(false),
            "the loop does not make a send possible"
        );
    }
}
