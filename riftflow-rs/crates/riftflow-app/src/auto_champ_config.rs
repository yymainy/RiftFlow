//! `auto-champ-config-main` — 自动配置符文页与召唤师技能。
//!
//! Ports `auto-config-controller.ts`, `state.ts` and `ipc-handlers.ts`.
//!
//! Two reactions, as in the original: one applies the preset when the locked-in
//! champion changes, the other announces the configured champion list once the
//! champion-select conversation shows up.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::RequestConfig;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

use crate::settings_registry::SettingsHub;

pub const NAMESPACE: &str = "auto-champ-config-main";
const LEAGUE: &str = "league-client-main";

/// `GAME_MODE_TYPE_MAP`
const GAME_MODE_TYPE: &[(&str, &str)] = &[
    ("CLASSIC", "normal"),
    ("URF", "urf"),
    ("ARAM", "aram"),
    ("NEXUSBLITZ", "nexusblitz"),
    ("ULTBOOK", "ultbook"),
];

#[derive(Debug, Clone, Copy)]
enum Signal {
    /// anything either reaction reads: the session, the current champion, the
    /// conversation, or this shard's settings
    Wake,
}

/// The two reaction guards, so a wake-up that changes neither input runs nothing.
#[derive(PartialEq)]
struct Seen {
    /// `[champSelect.currentChampion, settings.enabled]`
    champion: (i64, bool),
    /// `[conversationId, hasGameflowSession, hasChampSelectSession]`
    conversation: (Value, bool, bool),
}

pub struct AutoChampConfig {
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    sender: UnboundedSender<Signal>,
    english: AtomicBool,
}

pub fn start(
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    hub: &Arc<SettingsHub>,
) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let this = Arc::new(AutoChampConfig {
        league_client,
        state_sync,
        ipc,
        settings,
        sender: sender.clone(),
        english: AtomicBool::new(false),
    });

    register_ipc(&this)?;

    let emitter = this.league_client.emitter();
    for uri in [
        "/lol-champ-select/v1/current-champion",
        "/lol-champ-select/v1/session",
        "/lol-gameflow/v1/session",
        "/lol-chat/v1/conversation",
    ] {
        let sender = sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(Signal::Wake);
        })?;
    }

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

fn register_ipc(this: &Arc<AutoChampConfig>) -> anyhow::Result<()> {
    let settings = this.settings.clone();
    let sender = this.sender.clone();
    this.ipc.on_call(NAMESPACE, "updateRunes", riftflow_ipc::handler(
        move |_ctx, args| {
            let settings = settings.clone();
            let sender = sender.clone();
            async move {
                update_preset(&settings, "runesV2", &args).await;
                let _ = sender.send(Signal::Wake);
                Ok(Value::Null)
            }
        },
    ))?;

    let settings = this.settings.clone();
    let sender = this.sender.clone();
    this.ipc.on_call(
        NAMESPACE,
        "updateSummonerSpells",
        riftflow_ipc::handler(move |_ctx, args| {
            let settings = settings.clone();
            let sender = sender.clone();
            async move {
                update_preset(&settings, "summonerSpells", &args).await;
                let _ = sender.send(Signal::Wake);
                Ok(Value::Null)
            }
        }),
    )?;

    Ok(())
}

impl AutoChampConfig {
    async fn run(self: Arc<Self>, mut receiver: UnboundedReceiver<Signal>) {
        self.english.store(self.locale_is_english(), Ordering::Relaxed);

        // neither reaction has `fireImmediately`, so the first pass only records
        // what is already true
        let mut seen = self.snapshot();

        loop {
            match receiver.recv().await {
                Some(Signal::Wake) => {
                    let current = self.snapshot();

                    if current.champion != seen.champion {
                        self.apply_current_champion().await;
                    }
                    if current.conversation != seen.conversation {
                        self.announce_configured().await;
                    }

                    seen = current;
                }
                None => break,
            }
        }
    }

    fn snapshot(&self) -> Seen {
        Seen {
            champion: (
                self.league("champSelect", "currentChampion")
                    .as_ref().and_then(Value::as_i64)
                    .unwrap_or_default(),
                self.flag("enabled"),
            ),
            conversation: (
                self.conversation_id().unwrap_or(Value::Null),
                self.is_object("gameflow", "session"),
                self.is_object("champSelect", "session"),
            ),
        }
    }

    // ---- `_watchCurrentChampion` -------------------------------------------

    async fn apply_current_champion(&self) {
        let champion_id = self
            .league("champSelect", "currentChampion")
            .as_ref().and_then(Value::as_i64)
            .unwrap_or_default();

        if champion_id == 0 || !self.flag("enabled") {
            return;
        }

        let Some(position) = self.local_position() else {
            return;
        };

        let game_mode = self.queue_field("gameMode");
        let queue_type = self.queue_field("type");
        let Some(key) = config_key(&game_mode, &queue_type, &position) else {
            return;
        };

        // a ranked preset falls back to the shared ranked page; summoner spells
        // deliberately do not, matching the original
        let runes_key = if queue_type.starts_with("RANKED_") && !self.has_preset("runesV2", champion_id, &key) {
            "ranked-default".to_string()
        } else {
            key.clone()
        };

        let meta = Meta { champion_id, position };

        if let Some(runes) = self.preset("runesV2", champion_id, &runes_key) {
            self.apply_runes(&runes, &meta).await;
        }
        if let Some(spells) = self.preset("summonerSpells", champion_id, &key) {
            self.apply_summoner_spells(&spells, &meta).await;
        }
    }

    async fn apply_runes(&self, runes: &Value, meta: &Meta) {
        let names = self.runes_names(runes, meta);
        let primary = style_id(runes, "primaryStyleId");
        let sub = style_id(runes, "subStyleId");
        let perks = runes.get("selectedPerkIds").cloned().unwrap_or_else(|| json!([]));

        match self.write_runes_page(&names.page_name, primary, sub, &perks).await {
            Ok(()) => {
                eprintln!("[auto-champ-config] 已更新符文页 {}", names.page_name);
                self.send_in_chat(&names.message).await;
            }
            Err(error) => {
                self.ipc.send_event(NAMESPACE, "error-runes-update", &[json!(error)]);
                eprintln!("[auto-champ-config] 符文页更新失败: {error}");
                self.send_in_chat(&names.error_message).await;
            }
        }
    }

    async fn write_runes_page(
        &self,
        page_name: &str,
        primary: i64,
        sub: i64,
        perks: &Value,
    ) -> Result<(), String> {
        let inventory = self.call("GET", "/lol-perks/v1/inventory", None).await?;

        let target_id = if inventory
            .get("canAddCustomPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let added = self
                .call(
                    "POST",
                    "/lol-perks/v1/pages/",
                    Some(json!({
                        "name": page_name,
                        "isEditable": true,
                        // the create endpoint wants the style id as a string and
                        // the update endpoint as a number — the LCU's own shapes
                        "primaryStyleId": primary.to_string(),
                    })),
                )
                .await?;
            added.get("id").cloned().ok_or("符文页创建未返回 id")?
        } else {
            let pages = self.call("GET", "/lol-perks/v1/pages", None).await?;
            let first = pages.as_array().and_then(|list| list.first()).ok_or("没有可改写的符文页")?;
            first.get("id").cloned().ok_or("符文页缺少 id")?
        };

        let page = json!({
            "id": target_id,
            "isRecommendationOverride": false,
            "isTemporary": false,
            "name": page_name,
            "primaryStyleId": primary,
            "selectedPerkIds": perks,
            "subStyleId": sub,
        });

        let id = target_id.as_i64().unwrap_or_default();
        self.call("PUT", &format!("/lol-perks/v1/pages/{id}"), Some(page)).await?;
        // `putCurrentPage(id)` sends the bare id as the whole body
        self.call("PUT", "/lol-perks/v1/currentpage", Some(target_id)).await?;

        Ok(())
    }

    async fn apply_summoner_spells(&self, spells: &Value, meta: &Meta) {
        let body = json!({ "spell1Id": style_id(spells, "spell1Id"), "spell2Id": style_id(spells, "spell2Id") });

        let result = self
            .call("PATCH", "/lol-champ-select/v1/session/my-selection", Some(body))
            .await;

        match result {
            Ok(_) => self.send_in_chat(&self.spells_message(spells, meta)).await,
            Err(error) => {
                self.ipc.send_event(NAMESPACE, "error-spells-update", &[json!(error)]);
                eprintln!("[auto-champ-config] 召唤师技能更新失败: {error}");
                self.send_in_chat(&self.spells_error_message(meta)).await;
            }
        }
    }

    // ---- `_watchChampionSelectConversation` --------------------------------

    async fn announce_configured(&self) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        if !self.flag("enabled")
            || self.phase().as_deref() != Some("ChampSelect")
            || !self.is_object("gameflow", "session")
            || !self.is_object("champSelect", "session")
        {
            return;
        }

        let Some(position) = self.local_position() else {
            return;
        };

        let game_mode = self.queue_field("gameMode");
        let queue_type = self.queue_field("type");
        let Some(key) = config_key(&game_mode, &queue_type, &position) else {
            return;
        };

        let is_ranked = queue_type.starts_with("RANKED_");
        let mut ids = configured_champions(&self.preset_map("runesV2"), &key, is_ranked);
        ids.extend(configured_champions(&self.preset_map("summonerSpells"), &key, is_ranked));
        ids.sort_unstable();
        ids.dedup();

        let names: Vec<String> = ids
            .iter()
            .take(16)
            .map(|id| self.champion_name(*id))
            .collect();

        let english = self.english.load(Ordering::Relaxed);
        let message = if names.is_empty() {
            notice_text(english, "auto-config-enabled-no-champion", "")
        } else {
            notice_text(english, "auto-config-enabled", &names.join(", "))
        };

        let _ = conversation_id;
        self.send_in_chat(&message).await;
    }

    // ---- naming ------------------------------------------------------------

    fn runes_names(&self, runes: &Value, meta: &Meta) -> PageNames {
        let champion = self.champion_name(meta.champion_id);
        let detail = format!(
            "{} / {} ({})",
            style_name(runes, "primaryStyleId", &self.league("gameData", "perkstyles")),
            style_name(runes, "subStyleId", &self.league("gameData", "perkstyles")),
            runes
                .get("selectedPerkIds")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|id| self.perk_name(id.as_i64().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(", ")
        );

        match self.position_name(&meta.position) {
            Some(position) => PageNames {
                page_name: format!("[RiftFlow] {champion} - {position}"),
                message: runes_message(self.english(), &champion, Some(&position), &detail),
                error_message: runes_error(self.english(), &champion, Some(&position)),
            },
            None => PageNames {
                page_name: format!("[RiftFlow] {champion}"),
                message: runes_message(self.english(), &champion, None, &detail),
                error_message: runes_error(self.english(), &champion, None),
            },
        }
    }

    fn spells_message(&self, spells: &Value, meta: &Meta) -> String {
        let champion = self.champion_name(meta.champion_id);
        let detail = format!(
            "[{}] [{}]",
            self.spell_name(style_id(spells, "spell1Id")),
            self.spell_name(style_id(spells, "spell2Id"))
        );

        match self.position_name(&meta.position) {
            Some(position) => spells_message(self.english(), &champion, Some(&position), &detail),
            None => spells_message(self.english(), &champion, None, &detail),
        }
    }

    fn spells_error_message(&self, meta: &Meta) -> String {
        let champion = self.champion_name(meta.champion_id);
        match self.position_name(&meta.position) {
            Some(position) => spells_error(self.english(), &champion, Some(&position)),
            None => spells_error(self.english(), &champion, None),
        }
    }

    fn english(&self) -> bool {
        self.english.load(Ordering::Relaxed)
    }

    fn position_name(&self, position: &str) -> Option<String> {
        position_name(position, self.english()).map(str::to_string)
    }

    fn champion_name(&self, id: i64) -> String {
        self.game_data_name("champions", id)
    }

    fn perk_name(&self, id: i64) -> String {
        self.game_data_name("perks", id)
    }

    fn spell_name(&self, id: i64) -> String {
        self.game_data_name("summonerSpells", id)
    }

    fn game_data_name(&self, record: &str, id: i64) -> String {
        self.league("gameData", record)
            .and_then(|entries| {
                entries
                    .get(&id.to_string())
                    .and_then(|entry| entry.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| id.to_string())
    }

    // ---- inputs ------------------------------------------------------------

    fn flag(&self, key: &str) -> bool {
        self.setting(key).as_bool().unwrap_or(false)
    }

    fn setting(&self, key: &str) -> Value {
        self.settings
            .get_service(NAMESPACE)
            .map(|service| service.get(key))
            .unwrap_or(Value::Null)
    }

    fn preset_map(&self, key: &str) -> Map<String, Value> {
        self.setting(key).as_object().cloned().unwrap_or_default()
    }

    fn preset(&self, key: &str, champion_id: i64, config_key: &str) -> Option<Value> {
        self.preset_map(key)
            .get(&champion_id.to_string())?
            .get(config_key)
            .filter(|value| value.is_object())
            .cloned()
    }

    fn has_preset(&self, key: &str, champion_id: i64, config_key: &str) -> bool {
        self.preset(key, champion_id, config_key).is_some()
    }

    fn league(&self, state_id: &str, path: &str) -> Option<Value> {
        self.state_sync.get(LEAGUE, state_id, path)
    }

    fn is_object(&self, state_id: &str, path: &str) -> bool {
        self.league(state_id, path).is_some_and(|value| value.is_object())
    }

    fn phase(&self) -> Option<String> {
        self.league("gameflow", "phase").as_ref().and_then(Value::as_str).map(str::to_string)
    }

    fn queue_field(&self, field: &str) -> String {
        self.league("gameflow", "session")
            .and_then(|session| {
                session
                    .pointer(&format!("/gameData/queue/{field}"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default()
    }

    /// `session.myTeam.find(cellId === localPlayerCellId).assignedPosition`
    fn local_position(&self) -> Option<String> {
        let session = self.league("champSelect", "session")?;
        let cell = session.get("localPlayerCellId").and_then(Value::as_i64)?;

        session
            .get("myTeam")
            .and_then(Value::as_array)?
            .iter()
            .find(|player| player.get("cellId").and_then(Value::as_i64) == Some(cell))
            .and_then(|player| player.get("assignedPosition"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn conversation_id(&self) -> Option<Value> {
        self.league("chat", "conversations.championSelect")
            .filter(|value| value.is_object())
            .and_then(|conversation| conversation.get("id").cloned())
    }

    async fn send_in_chat(&self, message: &str) {
        let Some(id) = self.conversation_id() else {
            return;
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

        if let Err(error) = self
            .call(
                "POST",
                &format!("/lol-chat/v1/conversations/{id}/messages"),
                Some(body),
            )
            .await
        {
            self.ipc.send_event(NAMESPACE, "error-chat-send", &[json!(error)]);
            eprintln!("[auto-champ-config] 频道消息未发出: {error}");
        }
    }

    async fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<Value, String> {
        let Some(http) = self.league_client.http() else {
            return Err("LCU 未连接".to_string());
        };

        let config = RequestConfig {
            method: method.to_string(),
            url: url.to_string(),
            data: body,
            ..Default::default()
        };

        let response = http.request(config).await.map_err(|error| error.to_string())?;
        if response.status >= 400 {
            return Err(format!("{method} {url} → {}", response.status));
        }

        Ok(response.body)
    }

    fn locale_is_english(&self) -> bool {
        self.settings
            .get_setting("app-common-main", "locale")
            .ok()
            .and_then(|locale| locale.as_str().map(str::to_string))
            .is_some_and(|locale| locale.starts_with("en"))
    }
}

struct Meta {
    champion_id: i64,
    position: String,
}

struct PageNames {
    page_name: String,
    message: String,
    error_message: String,
}

fn style_id(config: &Value, field: &str) -> i64 {
    config.get(field).and_then(Value::as_i64).unwrap_or_default()
}

fn style_name(config: &Value, field: &str, perkstyles: &Option<Value>) -> String {
    let id = style_id(config, field);

    perkstyles
        .as_ref()
        .and_then(|styles| {
            styles
                .pointer(&format!("/styles/{id}/name"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| id.to_string())
}

/// The preset key for a mode/position pair, `None` when the mode has none.
fn config_key(game_mode: &str, queue_type: &str, position: &str) -> Option<String> {
    if game_mode == "CLASSIC" {
        return Some(if queue_type.starts_with("RANKED_") {
            format!("ranked-{position}")
        } else {
            "normal".to_string()
        });
    }

    GAME_MODE_TYPE
        .iter()
        .find(|(mode, _)| *mode == game_mode)
        .map(|(_, key)| (*key).to_string())
}

/// Every champion with a preset for this key — plus, in ranked, the ones that
/// only configured the shared `ranked-default` page.
fn configured_champions(presets: &Map<String, Value>, key: &str, is_ranked: bool) -> Vec<i64> {
    presets
        .iter()
        .filter(|(_, configs)| {
            configs.get(key).is_some_and(|value| !value.is_null())
                || (is_ranked
                    && configs
                        .get("ranked-default")
                        .is_some_and(|value| !value.is_null()))
        })
        .filter_map(|(champion_id, _)| champion_id.parse::<i64>().ok())
        .collect()
}

/// `positions.*` from `shared/i18n/{en,zh-CN}/common.yaml`.
fn position_name(position: &str, english: bool) -> Option<&'static str> {
    const TABLE: &[(&str, &str, &str)] = &[
        ("all", "None", "无"),
        ("top", "Top", "上路"),
        ("jungle", "Jungle", "打野"),
        ("mid", "Middle", "中路"),
        ("middle", "Middle", "中路"),
        ("bot", "Bottom", "下路"),
        ("bottom", "Bottom", "下路"),
        ("support", "Support", "辅助"),
        ("utility", "Support", "辅助"),
    ];

    let lowered = position.to_ascii_lowercase();
    TABLE
        .iter()
        .find(|(key, _, _)| *key == lowered)
        .map(|(_, en, zh)| if english { *en } else { *zh })
}

/// `auto-champ-config-main.*` from `shared/i18n/{en,zh-CN}/main.yaml`.
fn notice_text(english: bool, key: &str, names: &str) -> String {
    match (english, key) {
        (true, "auto-config-enabled") => {
            format!("Auto configuration enabled, configured champions: {names}")
        }
        (false, "auto-config-enabled") => format!("已启用自动配置，已配置英雄：{names}"),
        (true, _) => "Auto configuration enabled, but no champion has been configured".to_string(),
        (false, _) => "已启用自动配置，但未配置任何英雄".to_string(),
    }
}

fn runes_message(english: bool, name: &str, position: Option<&str>, detail: &str) -> String {
    match (english, position) {
        (true, Some(position)) => format!("{name} - {position} rune setup has been updated to {detail}"),
        (true, None) => format!("{name} rune setup has been updated to {detail}"),
        (false, Some(position)) => format!("{name} - {position} 符文已更新为 {detail}"),
        (false, None) => format!("{name} 符文已更新为 {detail}"),
    }
}

fn runes_error(english: bool, name: &str, position: Option<&str>) -> String {
    match (english, position) {
        (true, Some(position)) => format!("Failed to update rune setup for {name} - {position}"),
        (true, None) => format!("Failed to update rune setup for {name}"),
        (false, Some(position)) => format!("为 {name} - {position} 更新符文失败"),
        (false, None) => format!("为 {name} 更新符文失败"),
    }
}

fn spells_message(english: bool, name: &str, position: Option<&str>, detail: &str) -> String {
    match (english, position) {
        (true, Some(position)) => format!("{name} - {position} summoner spells have been updated to {detail}"),
        (true, None) => format!("{name} summoner spells have been updated to {detail}"),
        (false, Some(position)) => format!("{name} - {position} 召唤师技能已更新为 {detail}"),
        (false, None) => format!("{name} 召唤师技能已更新为 {detail}"),
    }
}

fn spells_error(english: bool, name: &str, position: Option<&str>) -> String {
    match (english, position) {
        (true, Some(position)) => format!("Failed to update summoner spells for {name} - {position}"),
        (true, None) => format!("Failed to update summoner spells for {name}"),
        (false, Some(position)) => format!("为 {name} - {position} 更新召唤师技能失败"),
        (false, None) => format!("为 {name} 更新召唤师技能失败"),
    }
}


/// `settings.updateRunes(championId, key, config)` then the debounced write.
async fn update_preset(settings: &Arc<SettingFactory>, key: &str, args: &[Value]) {
    let Some(champion_id) = args.first().and_then(Value::as_i64) else {
        return;
    };
    let Some(config_key) = args.get(1).and_then(Value::as_str) else {
        return;
    };
    let value = args.get(2).cloned().unwrap_or(Value::Null);

    let current = settings
        .get_setting(NAMESPACE, key)
        .ok()
        .unwrap_or_else(|| json!({}));

    let mut all = current.as_object().cloned().unwrap_or_default();
    let mut champion = all
        .get(&champion_id.to_string())
        .and_then(|entry| entry.as_object().cloned())
        .unwrap_or_default();

    champion.insert(config_key.to_string(), value);
    all.insert(champion_id.to_string(), Value::Object(champion));

    let _ = settings.set_setting(NAMESPACE, key, Value::Object(all)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranked_presets_are_position_scoped() {
        assert_eq!(config_key("CLASSIC", "RANKED_SOLO_5x5", "middle").as_deref(), Some("ranked-middle"));
        assert_eq!(config_key("CLASSIC", "NORMAL", "middle").as_deref(), Some("normal"));
        assert_eq!(config_key("CLASSIC", "", "top").as_deref(), Some("normal"));
    }

    #[test]
    fn only_known_modes_have_a_key() {
        assert_eq!(config_key("ARAM", "450", "").as_deref(), Some("aram"));
        assert_eq!(config_key("URF", "1900", "").as_deref(), Some("urf"));
        assert_eq!(config_key("NEXUSBLITZ", "1200", "").as_deref(), Some("nexusblitz"));
        assert_eq!(config_key("ULTBOOK", "1020", "").as_deref(), Some("ultbook"));
        assert_eq!(config_key("CHERRY", "1700", "").as_deref(), None);
        assert_eq!(config_key("SWARM", "1700", "").as_deref(), None);
    }

    #[test]
    fn the_announcement_lists_ranked_defaults_too() {
        let presets = json!({
            "222": { "ranked-middle": { "spell1Id": 4, "spell2Id": 14 } },
            "157": { "ranked-default": { "spell1Id": 4, "spell2Id": 14 } },
            "103": { "ranked-top": null },
            "99": { "normal": { "spell1Id": 4, "spell2Id": 14 } }
        })
        .as_object()
        .cloned()
        .unwrap();

        let ranked = configured_champions(&presets, "ranked-middle", true);
        assert_eq!(ranked, vec![157, 222], "本位置的配置 + ranked-default");

        let strict = configured_champions(&presets, "ranked-middle", false);
        assert_eq!(strict, vec![222], "非排位时不回落到 ranked-default");

        assert_eq!(configured_champions(&presets, "normal", false), vec![99]);
    }

    #[test]
    fn position_names_cover_both_casings_and_locales() {
        assert_eq!(position_name("MIDDLE", true), Some("Middle"));
        assert_eq!(position_name("utility", false), Some("辅助"));
        assert_eq!(position_name("top", false), Some("上路"));
        assert_eq!(position_name("", true), None);
        assert_eq!(position_name("nonsense", true), None);
    }

    #[test]
    fn notice_strings_follow_the_locale() {
        assert_eq!(
            notice_text(true, "auto-config-enabled", "Ahri (103), Yasuo (157)"),
            "Auto configuration enabled, configured champions: Ahri (103), Yasuo (157)"
        );
        assert_eq!(
            notice_text(false, "auto-config-enabled-no-champion", ""),
            "已启用自动配置，但未配置任何英雄"
        );
        assert_eq!(
            runes_message(true, "Ahri", Some("Middle"), "8000 / 8100 (8005, 8009)"),
            "Ahri - Middle rune setup has been updated to 8000 / 8100 (8005, 8009)"
        );
        assert_eq!(
            spells_message(false, "亚索", None, "[闪现] [传送]"),
            "亚索 召唤师技能已更新为 [闪现] [传送]"
        );
        assert_eq!(
            runes_error(true, "Ahri", Some("Top")),
            "Failed to update rune setup for Ahri - Top"
        );
        assert_eq!(spells_error(false, "亚索", None), "为 亚索 更新召唤师技能失败");
    }

    #[test]
    fn a_null_preset_is_not_applied_but_still_counts_as_unset() {
        let presets = json!({ "222": { "ranked-middle": null } })
            .as_object()
            .cloned()
            .unwrap();
        assert!(configured_champions(&presets, "ranked-middle", false).is_empty());
    }
}
