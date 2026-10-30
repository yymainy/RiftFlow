//! `auto-misc-main` — 自动回复、离线状态锁定、登录后的状态/段位播报。
//!
//! Ports `auto-reply-controller.ts` and `login-automation-controller.ts`.
//!
//! The original armed a `TimeoutTask` every time `chat.me` changed and let it
//! fire once per connection; here the 2 s deadline lives in the same timer slot
//! as everything else, so a reconnect cannot leave a stale one running.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Instant;

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::RequestConfig;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

use crate::settings_registry::SettingsHub;

pub const NAMESPACE: &str = "auto-misc-main";
const LEAGUE: &str = "league-client-main";

/// `CHAT_ME_SETTLE_DELAY_MS`
const CHAT_ME_SETTLE: Duration = Duration::from_millis(2_000);

#[derive(Debug, Clone)]
enum Signal {
    /// `/lol-chat/v1/conversations/:fromId/messages/:messageId`
    IncomingMessage { from_id: String, data: Value },
    /// `chat.me` or one of the settings changed
    Wake,
    Command(Command),
}

#[derive(Debug, Clone)]
enum Command {
    ApplyStatusMessage(Option<String>),
    ApplyRankedStatus(Option<Value>),
}

struct AutoMisc {
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    sender: UnboundedSender<Signal>,
}

/// `TimeoutTask` plus `_isChatReadyAutomationDone`.
#[derive(Default)]
struct Automation {
    settle: Option<Instant>,
    done: bool,
    availability: Option<String>,
}

impl Automation {
    /// `_interruptChatReadyAutomation`: cancel the pending task and mark the
    /// automation as already handled for this connection.
    fn interrupt(&mut self) {
        self.settle = None;
        self.done = true;
    }
}

pub fn start(
    league_client: Arc<LeagueClientMain>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    settings: Arc<SettingFactory>,
    hub: &Arc<SettingsHub>,
) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let this = Arc::new(AutoMisc {
        league_client,
        state_sync,
        ipc,
        settings,
        sender: sender.clone(),
    });

    register_ipc(&this)?;

    let emitter = this.league_client.emitter();

    let sender_for_messages = sender.clone();
    emitter.on(
        "/lol-chat/v1/conversations/:fromId/messages/:messageId",
        move |payload, params| {
            let from_id = params
                .get("fromId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let data = payload.get("data").cloned().unwrap_or(Value::Null);
            let _ = sender_for_messages.send(Signal::IncomingMessage { from_id, data });
        },
    )?;

    let sender_for_me = sender.clone();
    emitter.on("/lol-chat/v1/me", move |_payload, _params| {
        let _ = sender_for_me.send(Signal::Wake);
    })?;

    let mut changes = hub.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok((namespace, _key, _value)) = changes.recv().await {
            if namespace == NAMESPACE {
                let _ = sender.send(Signal::Wake);
            }
        }
    });

    tauri::async_runtime::spawn(this.run(receiver));
    Ok(())
}

fn register_ipc(this: &Arc<AutoMisc>) -> anyhow::Result<()> {
    let sender = this.sender.clone();
    this.ipc.on_call(
        NAMESPACE,
        "applyStatusMessage",
        riftflow_ipc::handler(move |_ctx, args| {
            let sender = sender.clone();
            async move {
                let message = args.first().and_then(Value::as_str).map(str::to_string);
                let _ = sender.send(Signal::Command(Command::ApplyStatusMessage(message)));
                Ok(Value::Null)
            }
        }),
    )?;

    let sender = this.sender.clone();
    this.ipc.on_call(
        NAMESPACE,
        "applyRankedStatus",
        riftflow_ipc::handler(move |_ctx, args| {
            let sender = sender.clone();
            async move {
                let status = args.first().cloned().filter(|value| value.is_object());
                let _ = sender.send(Signal::Command(Command::ApplyRankedStatus(status)));
                Ok(Value::Null)
            }
        }),
    )?;

    Ok(())
}

impl AutoMisc {
    async fn run(self: Arc<Self>, mut receiver: UnboundedReceiver<Signal>) {
        let mut automation = Automation::default();

        // `_watchChatReadyAutomations` runs with `fireImmediately`
        self.on_chat_me_changed(&mut automation).await;

        loop {
            let deadline = automation.settle;

            tokio::select! {
                biased;

                signal = receiver.recv() => match signal {
                    Some(Signal::IncomingMessage { from_id, data }) => {
                        self.auto_reply(&from_id, &data).await;
                    }
                    Some(Signal::Wake) => self.on_chat_me_changed(&mut automation).await,
                    Some(Signal::Command(Command::ApplyStatusMessage(message))) => {
                        // a manual call interrupts the automation, and its error
                        // must reach the renderer
                        automation.interrupt();
                        let text = message.unwrap_or_else(|| self.setting_text("statusMessage"));
                        let _ = self.set_status_message(&text, false).await;
                    }
                    Some(Signal::Command(Command::ApplyRankedStatus(status))) => {
                        automation.interrupt();
                        let status = status.unwrap_or_else(|| self.ranked_status());
                        let _ = self.set_ranked_status(&status, false).await;
                    }
                    None => break,
                },

                _ = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    automation.settle = None;
                    self.apply_chat_ready_automations_once(&mut automation).await;
                }
            }
        }
    }

    // ---- `_watchIncomingMessages` -------------------------------------------

    async fn auto_reply(&self, from_id: &str, data: &Value) {
        if !self.flag("autoReplyEnabled") || !self.has_setting_text("autoReplyText") {
            return;
        }

        let Some(me) = self.league("summoner", "me").filter(|value| value.is_object()) else {
            return;
        };

        if data.get("type").and_then(Value::as_str) != Some("chat") {
            return;
        }

        // the client also echoes our own messages back to us
        if data.get("fromSummonerId").and_then(Value::as_i64) == me.get("summonerId").and_then(Value::as_i64) {
            return;
        }

        if self.flag("autoReplyEnableOnAway")
            && self.chat_availability().as_deref() != Some("away")
        {
            return;
        }

        let text = self.setting_text("autoReplyText");
        match self.chat_send(from_id, &text).await {
            Ok(()) => eprintln!("[auto-misc] 已自动回复 {from_id}: {text}"),
            Err(error) => {
                eprintln!("[auto-misc] 自动回复失败: {error}");
                self.ipc.send_event(
                    NAMESPACE,
                    "error-send-failed",
                    &[json!({ "error": error })],
                );
            }
        }
    }

    // ---- `_watchOfflineStatusLock` + the chat-ready automation --------------

    async fn on_chat_me_changed(&self, automation: &mut Automation) {
        let me = self.league("chat", "me").filter(|value| value.is_object());
        let availability = me
            .as_ref()
            .and_then(|value| value.get("availability"))
            .and_then(Value::as_str)
            .map(str::to_string);

        if let (Some(previous), Some(current)) = (automation.availability.clone(), availability.clone()) {
            if previous == "offline" && (current == "away" || current == "chat") && self.flag("lockOfflineStatus") {
                eprintln!("[auto-misc] 纠正为离线状态");
                if let Err(error) = self.change_availability("offline").await {
                    eprintln!("[auto-misc] 修改状态失败: {error}");
                }
            }
        }
        automation.availability = availability;

        if !self.connected() || me.is_none() {
            // `_resetChatReadyAutomationState`
            automation.settle = None;
            automation.done = false;
            return;
        }

        if automation.done {
            return;
        }

        // `TimeoutTask.start()` re-arms on every change until it has run once
        automation.settle = Some(Instant::now() + CHAT_ME_SETTLE);
    }

    async fn apply_chat_ready_automations_once(&self, automation: &mut Automation) {
        if automation.done || !self.connected() {
            return;
        }

        if !self
            .league("chat", "me")
            .is_some_and(|value| value.is_object())
        {
            return;
        }

        automation.done = true;

        if self.flag("autoSetStatusMessageEnabled") {
            let text = self.setting_text("statusMessage");
            if let Err(error) = self.set_status_message(&text, true).await {
                eprintln!("[auto-misc] 状态签名设置失败: {error}");
            }
        }

        if self.flag("autoSetRankedStatusEnabled") {
            let status = self.ranked_status();
            if let Err(error) = self.set_ranked_status(&status, true).await {
                eprintln!("[auto-misc] 排位状态设置失败: {error}");
            }
        }
    }

    // ---- LCU calls ----------------------------------------------------------

    async fn chat_send(&self, conversation_id: &str, message: &str) -> Result<(), String> {
        let body = json!({
            "body": message,
            "fromPid": "",
            "fromSummonerId": 0,
            "id": conversation_id,
            "isHistorical": false,
            "timestamp": "",
            "type": "chat",
        });
        self.put_or_post("POST", &format!("/lol-chat/v1/conversations/{conversation_id}/messages"), Some(body)).await
    }

    async fn change_availability(&self, availability: &str) -> Result<(), String> {
        self.put_or_post(
            "PUT",
            "/lol-chat/v1/me",
            Some(json!({ "availability": availability })),
        )
        .await
    }

    async fn set_status_message(&self, message: &str, automated: bool) -> Result<(), String> {
        let result = self
            .put_or_post("PUT", "/lol-chat/v1/me", Some(json!({ "statusMessage": message })))
            .await;

        match &result {
            Ok(()) if !automated => eprintln!("[auto-misc] 已应用状态签名"),
            Err(error) if !automated => eprintln!("[auto-misc] 手动应用状态签名失败: {error}"),
            _ => {}
        }

        result
    }

    async fn set_ranked_status(&self, status: &Value, automated: bool) -> Result<(), String> {
        let tier = status.get("tier").and_then(Value::as_str).unwrap_or_default();
        let mut lol = Map::new();
        lol.insert("rankedLeagueQueue".to_string(), json!(status.get("queue").and_then(Value::as_str).unwrap_or_default()));
        lol.insert("rankedLeagueTier".to_string(), json!(tier));

        // apex tiers carry no division, and the endpoint expects the key to be
        // absent rather than empty
        if !is_apex_tier(tier) {
            lol.insert(
                "rankedLeagueDivision".to_string(),
                json!(status.get("division").and_then(Value::as_str).unwrap_or_default()),
            );
        }

        let result = self
            .put_or_post("PUT", "/lol-chat/v1/me", Some(json!({ "lol": Value::Object(lol) })))
            .await;

        match &result {
            Ok(()) if !automated => eprintln!("[auto-misc] 已应用排位状态"),
            Err(error) if !automated => eprintln!("[auto-misc] 手动应用排位状态失败: {error}"),
            _ => {}
        }

        result
    }

    async fn put_or_post(&self, method: &str, url: &str, body: Option<Value>) -> Result<(), String> {
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

        Ok(())
    }

    // ---- inputs -------------------------------------------------------------

    fn flag(&self, key: &str) -> bool {
        self.setting(key).as_bool().unwrap_or(false)
    }

    fn setting_text(&self, key: &str) -> String {
        self.setting(key).as_str().unwrap_or_default().to_string()
    }

    fn has_setting_text(&self, key: &str) -> bool {
        !self.setting_text(key).is_empty()
    }

    fn setting(&self, key: &str) -> Value {
        self.settings
            .get_service(NAMESPACE)
            .map(|service| service.get(key))
            .unwrap_or(Value::Null)
    }

    fn ranked_status(&self) -> Value {
        self.setting("rankedStatus")
    }

    fn league(&self, state_id: &str, path: &str) -> Option<Value> {
        self.state_sync.get(LEAGUE, state_id, path)
    }

    fn chat_availability(&self) -> Option<String> {
        self.league("chat", "me")
            .and_then(|me| me.get("availability").and_then(Value::as_str).map(str::to_string))
    }

    fn connected(&self) -> bool {
        self.league_client.state.connection_state() == riftflow_lcu::ConnectionState::Connected
    }
}

fn is_apex_tier(tier: &str) -> bool {
    matches!(tier, "MASTER" | "GRANDMASTER" | "CHALLENGER")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apex_tiers_carry_no_division() {
        assert!(is_apex_tier("MASTER"));
        assert!(is_apex_tier("GRANDMASTER"));
        assert!(is_apex_tier("CHALLENGER"));
        assert!(!is_apex_tier("EMERALD"));
        assert!(!is_apex_tier("IRON"));
    }

    #[test]
    fn interrupting_stops_the_pending_settle_and_wont_rerun() {
        let mut automation = Automation::default();
        automation.settle = Some(Instant::now() + CHAT_ME_SETTLE);

        automation.interrupt();

        assert!(automation.settle.is_none());
        assert!(automation.done);
    }
}
