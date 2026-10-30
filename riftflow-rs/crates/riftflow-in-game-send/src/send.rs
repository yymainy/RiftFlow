//! Send routing and the in-game keystroke queue, ported from
//! `send-executor.ts`.
//!
//! What the executor did splits cleanly in two. The *decisions* — normalize the
//! lines, pick a channel from the game phase, order the keystrokes, honour the
//! cancel shortcut — are pure and live here. The *doing* needs Win32 input
//! injection and the LCU chat call, and stays behind [`InGameKeyboard`] plus
//! [`SendAction::Chat`], which the app layer implements; see
//! [`decide_send`] for exactly what it has to supply.

use crate::snapshot::GameSnapshot;

/// `IN_GAME_SEND_ENTER_KEY_CODE`
pub const ENTER_KEY_CODE: u32 = 13;
/// `IN_GAME_SEND_ENTER_KEY_INTERNAL_DELAY`
pub const ENTER_KEY_INTERNAL_DELAY: u64 = 20;

/// `ChatPhase`: the phases whose lines go out as one LCU chat message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatPhase {
    ChampionSelect,
    Lobby,
}

/// Which channel a send goes to, from `ongoingGame.state.queryStage.phase`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SendRoute {
    Chat(ChatPhase),
    InGame,
    /// `draft`, `unavailable`, … — the original returned `false` without sending
    NotSendable,
}

/// `routeForPhase` — `_isChatPhase` plus the two-way branch in `sendLines`.
pub fn route_for_phase(phase: &str) -> SendRoute {
    match phase {
        "champ-select" => SendRoute::Chat(ChatPhase::ChampionSelect),
        "lobby" => SendRoute::Chat(ChatPhase::Lobby),
        "in-game" => SendRoute::InGame,
        _ => SendRoute::NotSendable,
    }
}

/// One instruction of the in-game send, in the order `sendLinesInGame` performed
/// them: `sendKey(code, isDown)` → [`SendStep::SendKey`], `sendString` →
/// [`SendStep::SendText`], `sleep` → [`SendStep::Sleep`], and every
/// `if (signal.aborted)` → [`SendStep::CheckCancelled`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SendStep {
    SendKey { key_code: u32, is_down: bool },
    SendText { text: String },
    Sleep { millis: u64 },
    CheckCancelled,
}

/// Everything `sendLines` worked out before it touched the outside world.
#[derive(Clone, Debug, PartialEq)]
pub struct SendPlan {
    pub route: SendRoute,
    /// `false` when every line was blank — the original returned `false` there
    pub has_lines: bool,
    /// `chatSend(conversation.id, lines.join('\n'))` for the chat routes
    pub message: String,
    /// the conversation this phase sends into; `None` when the chat room is
    /// missing, which is the original's other silent `false`
    pub conversation_id: Option<String>,
    /// the keystroke queue for [`SendRoute::InGame`]; empty for the chat routes
    pub steps: Vec<SendStep>,
}

/// Why a send will not happen, one per early `return false` in `sendLines`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SendRefusal {
    NoLines,
    PhaseNotSendable,
    NoChatRoom,
    /// `isGameClientForeground()` said no
    NotForeground,
    /// `NATIVE_SUPPORT.nativeInput.available` said no
    InputUnavailable,
}

/// What the app layer has to do next.
#[derive(Clone, Debug, PartialEq)]
pub enum SendAction {
    /// `leagueClient.api.chat.chatSend(conversation_id, message)`
    Chat { conversation_id: String, message: String },
    /// feed these steps to an [`InGameKeyboard`] with [`run_in_game_send`]
    InGame { steps: Vec<SendStep> },
    Refused(SendRefusal),
}

/// `normalizeInGameSendLines`: blank lines go, the rest keep their spacing.
pub fn normalize_send_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect()
}

/// `lines.join('\n')` — what one chat-phase send contains.
pub fn chat_message(lines: &[String]) -> String {
    lines.join("\n")
}

/// `sendLines`, up to the point where it starts touching the outside world.
pub fn plan_in_game_send(snapshot: &GameSnapshot, lines: &[String], send_interval: u64) -> SendPlan {
    let normalized = normalize_send_lines(lines);
    let route = route_for_phase(&snapshot.phase);

    let conversation_id = match route {
        SendRoute::Chat(ChatPhase::ChampionSelect) => {
            snapshot.champion_select_conversation_id.clone()
        }
        SendRoute::Chat(ChatPhase::Lobby) => snapshot.custom_game_conversation_id.clone(),
        _ => None,
    };

    let has_lines = !normalized.is_empty();
    let message = chat_message(&normalized);

    let steps = match route {
        SendRoute::InGame if has_lines => in_game_send_steps(&normalized, send_interval),
        _ => Vec::new(),
    };

    SendPlan {
        route,
        has_lines,
        message,
        conversation_id,
        steps,
    }
}

/// `sendLines`' decision tree once the preconditions are known.
///
/// The order is the original's: lines, then phase, then the chat room, then — only
/// for the keyboard route — foreground before availability, since
/// `isGameClientForeground()` is awaited before `_sendLinesInGame` checks
/// `NATIVE_SUPPORT`.
pub fn decide_send(plan: &SendPlan, input_available: bool, is_foreground: bool) -> SendAction {
    if !plan.has_lines {
        return SendAction::Refused(SendRefusal::NoLines);
    }

    match plan.route {
        SendRoute::NotSendable => SendAction::Refused(SendRefusal::PhaseNotSendable),
        SendRoute::Chat(_) => match &plan.conversation_id {
            Some(conversation_id) => SendAction::Chat {
                conversation_id: conversation_id.clone(),
                message: plan.message.clone(),
            },
            None => SendAction::Refused(SendRefusal::NoChatRoom),
        },
        SendRoute::InGame => {
            if !is_foreground {
                return SendAction::Refused(SendRefusal::NotForeground);
            }
            if !input_available {
                return SendAction::Refused(SendRefusal::InputUnavailable);
            }

            SendAction::InGame {
                steps: plan.steps.clone(),
            }
        }
    }
}

/// `_sendLinesInGame`: open the chat box, type the line, submit it, wait out the
/// configured interval between lines.
pub fn in_game_send_steps(lines: &[String], send_interval: u64) -> Vec<SendStep> {
    let mut steps = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        steps.push(SendStep::CheckCancelled);
        steps.extend(press_enter());

        steps.push(SendStep::CheckCancelled);
        steps.push(SendStep::Sleep {
            millis: ENTER_KEY_INTERNAL_DELAY,
        });
        steps.push(SendStep::SendText { text: line.clone() });

        steps.push(SendStep::CheckCancelled);
        steps.push(SendStep::Sleep {
            millis: ENTER_KEY_INTERNAL_DELAY,
        });
        steps.extend(press_enter());

        steps.push(SendStep::CheckCancelled);

        if index + 1 < lines.len() {
            steps.push(SendStep::Sleep {
                millis: send_interval,
            });
        }
    }

    steps
}

/// `pressEnter` — down, `ENTER_KEY_INTERNAL_DELAY`, up.
fn press_enter() -> [SendStep; 3] {
    [
        SendStep::SendKey {
            key_code: ENTER_KEY_CODE,
            is_down: true,
        },
        SendStep::Sleep {
            millis: ENTER_KEY_INTERNAL_DELAY,
        },
        SendStep::SendKey {
            key_code: ENTER_KEY_CODE,
            is_down: false,
        },
    ]
}

/// The seam [`SendAction::InGame`] is designed against.
///
/// `riftflow-native` today exposes elevation, process discovery, foreground-window
/// checks, process control and shell dialogs — it has **no** input injection, so
/// the app layer still owes three things before an in-game send can work:
///
/// 1. `send_key` / `send_text`, the addon's
///    `nativeInput.instance.sendKey(code, isDown)` and `sendString(text)`.
///    `send_text` has to type a UTF-16 string, since the preset wording carries
///    CJK;
/// 2. `available()` for `NATIVE_SUPPORT.nativeInput.available` — a `false` here
///    must become [`SendRefusal::InputUnavailable`] before any key is injected;
/// 3. `cancelled()` wired to the `cancelShortcut` abort controller, so a
///    `CheckCancelled` can stop the queue mid-run.
///
/// `wait` is the same `sleep(ms)` the original awaited. The foreground question
/// (`GameClientMain.isGameClientForeground()`) is a separate input to
/// [`decide_send`] so the ordering of the original's two checks is preserved.
pub trait InGameKeyboard {
    fn send_key(&mut self, key_code: u32, is_down: bool);
    fn send_text(&mut self, text: &str);
    fn wait(&mut self, millis: u64);
    fn cancelled(&self) -> bool;
}

/// Runs a queue produced by [`in_game_send_steps`].
///
/// `false` means the cancel shortcut fired at one of the `CheckCancelled` points,
/// which is the original's `!signal.aborted` result; `true` means every line went
/// out.
pub fn run_in_game_send(steps: &[SendStep], keyboard: &mut dyn InGameKeyboard) -> bool {
    for step in steps {
        match step {
            SendStep::SendKey { key_code, is_down } => keyboard.send_key(*key_code, *is_down),
            SendStep::SendText { text } => keyboard.send_text(text),
            SendStep::Sleep { millis } => keyboard.wait(*millis),
            SendStep::CheckCancelled => {
                if keyboard.cancelled() {
                    return false;
                }
            }
        }
    }

    !keyboard.cancelled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn snapshot(phase: &str, champion_select: Option<&str>, custom_game: Option<&str>) -> GameSnapshot {
        GameSnapshot::from_json(&json!({
            "phase": phase,
            "chat": { "conversations": {
                "championSelect": champion_select.map(|id| json!({ "id": id })),
                "customGame": custom_game.map(|id| json!({ "id": id }))
            } }
        }))
    }

    #[test]
    fn blank_lines_are_dropped_without_rewriting_the_rest() {
        assert_eq!(
            normalize_send_lines(&lines(&["one", "", "  ", " two "])),
            lines(&["one", " two "])
        );
        assert!(normalize_send_lines(&[]).is_empty());
        assert_eq!(chat_message(&lines(&["one", "two"])), "one\ntwo");
    }

    #[test]
    fn each_phase_routes_to_its_channel() {
        assert_eq!(
            route_for_phase("champ-select"),
            SendRoute::Chat(ChatPhase::ChampionSelect)
        );
        assert_eq!(route_for_phase("lobby"), SendRoute::Chat(ChatPhase::Lobby));
        assert_eq!(route_for_phase("in-game"), SendRoute::InGame);
        assert_eq!(route_for_phase("draft"), SendRoute::NotSendable);
        assert_eq!(route_for_phase("unavailable"), SendRoute::NotSendable);
        assert_eq!(route_for_phase(""), SendRoute::NotSendable);
    }

    #[test]
    fn chat_phases_merge_every_line_into_one_message() {
        let plan = plan_in_game_send(
            &snapshot("champ-select", Some("champ-select-chat"), Some("lobby-chat")),
            &lines(&["one", "", "two"]),
            65,
        );

        assert_eq!(plan.route, SendRoute::Chat(ChatPhase::ChampionSelect));
        assert_eq!(plan.message, "one\ntwo");
        assert_eq!(plan.conversation_id.as_deref(), Some("champ-select-chat"));
        assert!(plan.steps.is_empty(), "a chat send never touches the keyboard");

        assert_eq!(
            decide_send(&plan, true, true),
            SendAction::Chat {
                conversation_id: "champ-select-chat".to_string(),
                message: "one\ntwo".to_string()
            }
        );

        let lobby = plan_in_game_send(
            &snapshot("lobby", Some("champ-select-chat"), Some("lobby-chat")),
            &lines(&["one", "two"]),
            65,
        );
        assert_eq!(lobby.conversation_id.as_deref(), Some("lobby-chat"));
    }

    #[test]
    fn a_missing_chat_room_is_a_silent_refusal() {
        let plan = plan_in_game_send(&snapshot("lobby", None, None), &lines(&["one"]), 65);

        assert_eq!(plan.conversation_id, None);
        assert_eq!(decide_send(&plan, true, true), SendAction::Refused(SendRefusal::NoChatRoom));
    }

    #[test]
    fn an_unsendable_phase_and_empty_lines_produce_no_queue() {
        let draft = plan_in_game_send(&snapshot("draft", None, None), &lines(&["one"]), 65);
        assert_eq!(draft.route, SendRoute::NotSendable);
        assert!(draft.steps.is_empty());
        assert_eq!(
            decide_send(&draft, true, true),
            SendAction::Refused(SendRefusal::PhaseNotSendable)
        );

        let empty = plan_in_game_send(&snapshot("in-game", None, None), &lines(&["", "   "]), 65);
        assert!(!empty.has_lines);
        assert!(empty.steps.is_empty());
        assert_eq!(decide_send(&empty, true, true), SendAction::Refused(SendRefusal::NoLines));
    }

    #[test]
    fn the_keyboard_route_checks_foreground_before_availability() {
        let plan = plan_in_game_send(&snapshot("in-game", None, None), &lines(&["one"]), 65);

        assert_eq!(
            decide_send(&plan, false, false),
            SendAction::Refused(SendRefusal::NotForeground)
        );
        assert_eq!(
            decide_send(&plan, false, true),
            SendAction::Refused(SendRefusal::InputUnavailable)
        );
        assert_eq!(
            decide_send(&plan, true, true),
            SendAction::InGame {
                steps: in_game_send_steps(&lines(&["one"]), 65)
            }
        );
    }

    #[test]
    fn the_in_game_queue_opens_types_and_submits_each_line() {
        let plan = plan_in_game_send(
            &snapshot("in-game", None, None),
            &lines(&["one", "two"]),
            456,
        );

        let enter_down = SendStep::SendKey {
            key_code: ENTER_KEY_CODE,
            is_down: true,
        };
        let enter_up = SendStep::SendKey {
            key_code: ENTER_KEY_CODE,
            is_down: false,
        };
        let internal = SendStep::Sleep {
            millis: ENTER_KEY_INTERNAL_DELAY,
        };

        assert_eq!(
            plan.steps,
            vec![
                SendStep::CheckCancelled,
                enter_down.clone(),
                internal.clone(),
                enter_up.clone(),
                SendStep::CheckCancelled,
                internal.clone(),
                SendStep::SendText { text: "one".to_string() },
                SendStep::CheckCancelled,
                internal.clone(),
                enter_down.clone(),
                internal.clone(),
                enter_up.clone(),
                SendStep::CheckCancelled,
                SendStep::Sleep { millis: 456 },
                SendStep::CheckCancelled,
                enter_down.clone(),
                internal.clone(),
                enter_up.clone(),
                SendStep::CheckCancelled,
                internal.clone(),
                SendStep::SendText { text: "two".to_string() },
                SendStep::CheckCancelled,
                internal.clone(),
                enter_down,
                internal,
                enter_up,
                SendStep::CheckCancelled,
            ]
        );
    }

    #[test]
    fn the_interval_is_waited_out_only_between_lines() {
        let single = in_game_send_steps(&lines(&["one"]), 456);
        assert!(!single.contains(&SendStep::Sleep { millis: 456 }));

        let double = in_game_send_steps(&lines(&["one", "two"]), 456);
        assert_eq!(
            double
                .iter()
                .filter(|step| **step == SendStep::Sleep { millis: 456 })
                .count(),
            1
        );

        let triple = in_game_send_steps(&lines(&["a", "b", "c"]), 10);
        assert_eq!(
            triple
                .iter()
                .filter(|step| **step == SendStep::Sleep { millis: 10 })
                .count(),
            2
        );
    }

    #[test]
    fn a_full_queue_injects_every_line_and_waits_the_documented_delays() {
        let steps = in_game_send_steps(&lines(&["one", "two"]), 65);
        let mut keyboard = FakeKeyboard::new(usize::MAX);

        assert!(run_in_game_send(&steps, &mut keyboard));
        assert_eq!(keyboard.texts, lines(&["one", "two"]));
        assert_eq!(
            keyboard.waits.take(),
            vec![20u64, 20, 20, 20, 65, 20, 20, 20, 20]
        );
        assert_eq!(keyboard.keys_sent.get(), 8);
    }

    #[test]
    fn a_cancel_during_the_queue_stops_the_send() {
        let steps = in_game_send_steps(&lines(&["one", "two"]), 65);

        // abort at the second checkpoint, i.e. right after the chat box opened
        let mut keyboard = FakeKeyboard::new(2);
        assert!(!run_in_game_send(&steps, &mut keyboard));
        assert!(keyboard.texts.is_empty());
        assert_eq!(keyboard.keys_sent.get(), 2);

        // abort only once the second line is about to start
        let mut keyboard = FakeKeyboard::new(5);
        assert!(!run_in_game_send(&steps, &mut keyboard));
        assert_eq!(keyboard.texts, lines(&["one"]));
    }

    #[test]
    fn a_cancel_fired_after_the_last_step_reports_failure() {
        let steps = in_game_send_steps(&lines(&["one"]), 65);
        let mut keyboard = FakeKeyboard::new(5);

        assert!(!run_in_game_send(&steps, &mut keyboard));
        assert_eq!(keyboard.texts, lines(&["one"]));
    }

    /// Stands in for the Win32 injection the app layer will provide: it reports
    /// itself cancelled from the `nth_cancel`th checkpoint onwards.
    struct FakeKeyboard {
        nth_cancel: usize,
        cancel_hits: Cell<usize>,
        texts: Vec<String>,
        waits: Cell<Vec<u64>>,
        keys_sent: Cell<usize>,
    }

    impl FakeKeyboard {
        fn new(nth_cancel: usize) -> Self {
            Self {
                nth_cancel,
                cancel_hits: Cell::new(0),
                texts: Vec::new(),
                waits: Cell::new(Vec::new()),
                keys_sent: Cell::new(0),
            }
        }
    }

    impl InGameKeyboard for FakeKeyboard {
        fn send_key(&mut self, key_code: u32, is_down: bool) {
            assert_eq!(key_code, ENTER_KEY_CODE);
            let _ = is_down;
            self.keys_sent.set(self.keys_sent.get() + 1);
        }

        fn send_text(&mut self, text: &str) {
            self.texts.push(text.to_string());
        }

        fn wait(&mut self, millis: u64) {
            let mut waits = self.waits.take();
            waits.push(millis);
            self.waits.set(waits);
        }

        fn cancelled(&self) -> bool {
            let hits = self.cancel_hits.get() + 1;
            self.cancel_hits.set(hits);
            hits >= self.nth_cancel
        }
    }
}
