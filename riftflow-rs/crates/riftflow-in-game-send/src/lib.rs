//! The decision core of `in-game-send-main`, ported from
//! `src/main/shards/in-game-send` + `src/shared/shards/in-game-send`.
//!
//! Everything here is pure over the ongoing-game / league-client JSON the app
//! already keeps in its synced state, so the preset wording, the puuid ordering
//! and the option merge rules can be tested without a game client, a keyboard
//! hook or a renderer.

pub mod fixed_text;
pub mod jungle;
pub mod options;
pub mod premade;
pub mod rating;
pub mod send;
pub mod selection;
pub mod snapshot;
pub mod text;

pub use fixed_text::{
    create_fixed_text_item, delete_fixed_text_item, fixed_text_lines, move_fixed_text_item,
    update_fixed_text_item, PresetEditError,
};
pub use jungle::build_jungle_lines;
pub use options::{
    clamp_send_interval, merge_jungle_options, merge_premade_options, merge_rating_options,
    normalize_fixed_text_item, normalize_fixed_text_items, rating_display_option_keys,
    FixedTextPresetItem, FixedTextPresetItemPatch, JunglePresetOptionPatch, JunglePresetOptions,
    MoveDirection, NameDisplayStrategy, PremadePresetOptionPatch, PremadePresetOptions,
    PresetTarget, RatingPresetOptionPatch, RatingPresetOptions, TargetShortcuts,
    TargetShortcutsPatch,
};
pub use premade::build_premade_lines;
pub use rating::build_rating_lines;
pub use send::{
    chat_message, decide_send, normalize_send_lines, plan_in_game_send, route_for_phase,
    run_in_game_send, InGameKeyboard, SendAction, SendPlan, SendRefusal, SendRoute, SendStep,
    ChatPhase, ENTER_KEY_CODE, ENTER_KEY_INTERNAL_DELAY,
};
pub use selection::{
    default_jungle_puuids, filter_current_premade_indices, filter_current_puuids,
    parse_premade_indices, unique_puuids,
};
pub use snapshot::{GameSnapshot, PresetPlayer, PresetTeam};
pub use text::{format_rate, to_fixed, Locale};
