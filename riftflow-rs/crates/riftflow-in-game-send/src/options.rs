//! The persisted option shapes of `shared/shards/in-game-send`: defaults, the
//! IPC patch merge, fixed-text normalization and the send-interval clamp.
//!
//! Field names serialize in camelCase because they are the same JSON the
//! renderer's `in-game-send-main` store props and the setting service exchange —
//! the shipped bundle reads `ratingPresetOptions.nameDisplayStrategy` and so on,
//! so the wire shape cannot drift.

use serde::{Deserialize, Serialize};

pub const FIXED_TEXT_PRESET_MAX_ITEMS: usize = 100;
pub const FIXED_TEXT_PRESET_TITLE_MAX_LENGTH: usize = 64;
pub const FIXED_TEXT_PRESET_CONTENT_MAX_LENGTH: usize = 65_536;

/// `InGameSendSettings.sendInterval`'s initial value, and what the renderer's
/// number input falls back to when it is cleared.
pub const DEFAULT_SEND_INTERVAL: u64 = 65;

/// `InGameSendPresetTarget`
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PresetTarget {
    #[serde(rename = "friendly")]
    Friendly,
    #[serde(rename = "enemy")]
    Enemy,
    #[serde(rename = "all")]
    All,
}

impl PresetTarget {
    /// `IN_GAME_SEND_PRESET_TARGETS`, in order.
    pub const ALL: [PresetTarget; 3] = [PresetTarget::Friendly, PresetTarget::Enemy, PresetTarget::All];

    pub fn as_str(self) -> &'static str {
        match self {
            PresetTarget::Friendly => "friendly",
            PresetTarget::Enemy => "enemy",
            PresetTarget::All => "all",
        }
    }

    pub fn from_str_name(value: &str) -> Option<Self> {
        match value {
            "friendly" => Some(PresetTarget::Friendly),
            "enemy" => Some(PresetTarget::Enemy),
            "all" => Some(PresetTarget::All),
            _ => None,
        }
    }
}

/// `InGameSendPresetNameDisplayStrategy`
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum NameDisplayStrategy {
    #[serde(rename = "preferName")]
    PreferName,
    #[serde(rename = "preferChampionName")]
    #[default]
    PreferChampionName,
    #[serde(rename = "championNameWithName")]
    ChampionNameWithName,
}

/// `InGameSendFixedTextPresetItemMoveDirection`
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MoveDirection {
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "down")]
    Down,
}

impl MoveDirection {
    pub fn from_str_name(value: &str) -> Option<Self> {
        match value {
            "up" => Some(MoveDirection::Up),
            "down" => Some(MoveDirection::Down),
            _ => None,
        }
    }
}

/// `InGameSendPresetTargetShortcuts`
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetShortcuts {
    pub friendly: Option<String>,
    pub enemy: Option<String>,
    pub all: Option<String>,
}

impl TargetShortcuts {
    pub fn get(&self, target: PresetTarget) -> Option<&str> {
        let slot = match target {
            PresetTarget::Friendly => &self.friendly,
            PresetTarget::Enemy => &self.enemy,
            PresetTarget::All => &self.all,
        };
        slot.as_deref()
    }

    pub fn set(&mut self, target: PresetTarget, shortcut: Option<String>) {
        let slot = match target {
            PresetTarget::Friendly => &mut self.friendly,
            PresetTarget::Enemy => &mut self.enemy,
            PresetTarget::All => &mut self.all,
        };
        *slot = shortcut;
    }

    /// `{ ...current, ...patch }` for the shortcut record only.
    pub fn merge(&mut self, patch: &TargetShortcutsPatch) {
        if let Some(shortcut) = &patch.friendly {
            self.friendly = shortcut.clone();
        }
        if let Some(shortcut) = &patch.enemy {
            self.enemy = shortcut.clone();
        }
        if let Some(shortcut) = &patch.all {
            self.all = shortcut.clone();
        }
    }
}

/// `Partial<InGameSendPresetTargetShortcuts>`; an absent field leaves the current
/// shortcut alone, `Some(None)` clears it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetShortcutsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friendly: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enemy: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<Option<String>>,
}

/// `InGameSendRatingPresetOptions`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RatingPresetOptions {
    pub target_shortcuts: TargetShortcuts,
    pub kda: bool,
    pub win_rate: bool,
    pub avg_solo_kills: bool,
    pub avg_vision_score: bool,
    pub avg_champion_damage: bool,
    pub avg_damage_taken: bool,
    pub avg_gold: bool,
    pub avg_cs_per_minute: bool,
    pub avg_kill_participation: bool,
    pub avg_damage_gold_efficiency: bool,
    pub main_champions: bool,
    pub main_positions: bool,
    pub name_display_strategy: NameDisplayStrategy,
    pub show_current_champion: bool,
}

/// `createDefaultInGameSendRatingPresetOptions`
impl Default for RatingPresetOptions {
    fn default() -> Self {
        Self {
            target_shortcuts: TargetShortcuts::default(),
            kda: true,
            win_rate: true,
            avg_solo_kills: true,
            avg_vision_score: false,
            avg_champion_damage: false,
            avg_damage_taken: false,
            avg_gold: false,
            avg_cs_per_minute: false,
            avg_kill_participation: false,
            avg_damage_gold_efficiency: false,
            main_champions: true,
            main_positions: true,
            name_display_strategy: NameDisplayStrategy::PreferChampionName,
            show_current_champion: false,
        }
    }
}

/// `InGameSendJunglePresetOptions`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JunglePresetOptions {
    pub target_shortcuts: TargetShortcuts,
    pub activity_preference: bool,
    pub first_clear_distribution: bool,
    pub early_gank: bool,
    pub dragon_control: bool,
    pub monster_control: bool,
    pub main_champions: bool,
    pub name_display_strategy: NameDisplayStrategy,
    pub show_current_champion: bool,
}

/// `createDefaultInGameSendJunglePresetOptions`
impl Default for JunglePresetOptions {
    fn default() -> Self {
        Self {
            target_shortcuts: TargetShortcuts::default(),
            activity_preference: true,
            first_clear_distribution: true,
            early_gank: true,
            dragon_control: true,
            monster_control: true,
            main_champions: true,
            name_display_strategy: NameDisplayStrategy::PreferChampionName,
            show_current_champion: true,
        }
    }
}

/// `InGameSendPremadePresetOptions`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PremadePresetOptions {
    pub target_shortcuts: TargetShortcuts,
    pub name_display_strategy: NameDisplayStrategy,
}

/// `createDefaultInGameSendPremadePresetOptions`
impl Default for PremadePresetOptions {
    fn default() -> Self {
        Self {
            target_shortcuts: TargetShortcuts::default(),
            name_display_strategy: NameDisplayStrategy::PreferChampionName,
        }
    }
}

/// `InGameSendRatingPresetOptionPatch`
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RatingPresetOptionPatch {
    pub target_shortcuts: Option<TargetShortcutsPatch>,
    pub kda: Option<bool>,
    pub win_rate: Option<bool>,
    pub avg_solo_kills: Option<bool>,
    pub avg_vision_score: Option<bool>,
    pub avg_champion_damage: Option<bool>,
    pub avg_damage_taken: Option<bool>,
    pub avg_gold: Option<bool>,
    pub avg_cs_per_minute: Option<bool>,
    pub avg_kill_participation: Option<bool>,
    pub avg_damage_gold_efficiency: Option<bool>,
    pub main_champions: Option<bool>,
    pub main_positions: Option<bool>,
    pub name_display_strategy: Option<NameDisplayStrategy>,
    pub show_current_champion: Option<bool>,
}

/// `InGameSendJunglePresetOptionPatch`
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JunglePresetOptionPatch {
    pub target_shortcuts: Option<TargetShortcutsPatch>,
    pub activity_preference: Option<bool>,
    pub first_clear_distribution: Option<bool>,
    pub early_gank: Option<bool>,
    pub dragon_control: Option<bool>,
    pub monster_control: Option<bool>,
    pub main_champions: Option<bool>,
    pub name_display_strategy: Option<NameDisplayStrategy>,
    pub show_current_champion: Option<bool>,
}

/// `InGameSendPremadePresetOptionPatch`
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PremadePresetOptionPatch {
    pub target_shortcuts: Option<TargetShortcutsPatch>,
    pub name_display_strategy: Option<NameDisplayStrategy>,
}

/// `InGameSendFixedTextPresetItem`
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FixedTextPresetItem {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub content: String,
}

/// `InGameSendFixedTextPresetItemPatch`
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FixedTextPresetItemPatch {
    pub title: Option<String>,
    pub shortcut: Option<Option<String>>,
    pub content: Option<String>,
}

/// `_updatePresetOptions` for rating: `{ ...current, ...patch }` with the
/// shortcut record merged one level deeper.
pub fn merge_rating_options(current: &RatingPresetOptions, patch: &RatingPresetOptionPatch) -> RatingPresetOptions {
    let mut next = current.clone();
    apply(&mut next.kda, &patch.kda);
    apply(&mut next.win_rate, &patch.win_rate);
    apply(&mut next.avg_solo_kills, &patch.avg_solo_kills);
    apply(&mut next.avg_vision_score, &patch.avg_vision_score);
    apply(&mut next.avg_champion_damage, &patch.avg_champion_damage);
    apply(&mut next.avg_damage_taken, &patch.avg_damage_taken);
    apply(&mut next.avg_gold, &patch.avg_gold);
    apply(&mut next.avg_cs_per_minute, &patch.avg_cs_per_minute);
    apply(&mut next.avg_kill_participation, &patch.avg_kill_participation);
    apply(&mut next.avg_damage_gold_efficiency, &patch.avg_damage_gold_efficiency);
    apply(&mut next.main_champions, &patch.main_champions);
    apply(&mut next.main_positions, &patch.main_positions);
    apply(&mut next.name_display_strategy, &patch.name_display_strategy);
    apply(&mut next.show_current_champion, &patch.show_current_champion);

    if let Some(shortcuts) = &patch.target_shortcuts {
        next.target_shortcuts.merge(shortcuts);
    }

    next
}

/// `_updatePresetOptions` for jungle.
pub fn merge_jungle_options(current: &JunglePresetOptions, patch: &JunglePresetOptionPatch) -> JunglePresetOptions {
    let mut next = current.clone();
    apply(&mut next.activity_preference, &patch.activity_preference);
    apply(&mut next.first_clear_distribution, &patch.first_clear_distribution);
    apply(&mut next.early_gank, &patch.early_gank);
    apply(&mut next.dragon_control, &patch.dragon_control);
    apply(&mut next.monster_control, &patch.monster_control);
    apply(&mut next.main_champions, &patch.main_champions);
    apply(&mut next.name_display_strategy, &patch.name_display_strategy);
    apply(&mut next.show_current_champion, &patch.show_current_champion);

    if let Some(shortcuts) = &patch.target_shortcuts {
        next.target_shortcuts.merge(shortcuts);
    }

    next
}

/// `_updatePresetOptions` for premade.
pub fn merge_premade_options(
    current: &PremadePresetOptions,
    patch: &PremadePresetOptionPatch,
) -> PremadePresetOptions {
    let mut next = current.clone();
    apply(&mut next.name_display_strategy, &patch.name_display_strategy);

    if let Some(shortcuts) = &patch.target_shortcuts {
        next.target_shortcuts.merge(shortcuts);
    }

    next
}

fn apply<T: Copy>(slot: &mut T, patch: &Option<T>) {
    if let Some(value) = patch {
        *slot = *value;
    }
}

/// `InGameSendRatingPresetDisplayOptionKey`, in the order the shared type lists
/// them. The app layer uses it to read one flat `{ key: boolean }` record from
/// the renderer back into a `RatingPresetOptionPatch`.
pub fn rating_display_option_keys() -> [&'static str; 12] {
    [
        "kda",
        "winRate",
        "avgSoloKills",
        "avgVisionScore",
        "avgChampionDamage",
        "avgDamageTaken",
        "avgGold",
        "avgCsPerMinute",
        "avgKillParticipation",
        "avgDamageGoldEfficiency",
        "mainChampions",
        "mainPositions",
    ]
}

/// The `sendInterval` setting transform: `Math.max(0, value)`.
///
/// A missing or non-finite value cannot survive `z.number()` upstream, so it is
/// reported as the default rather than clamped to a plausible-looking number.
pub fn clamp_send_interval(value: Option<f64>) -> u64 {
    match value {
        Some(value) if value.is_finite() => value.max(0.0).floor() as u64,
        _ => DEFAULT_SEND_INTERVAL,
    }
}

/// `normalizeInGameSendFixedTextPresetItem`
pub fn normalize_fixed_text_item(item: &FixedTextPresetItem) -> FixedTextPresetItem {
    FixedTextPresetItem {
        id: item.id.clone(),
        title: truncate_utf16(&item.title, FIXED_TEXT_PRESET_TITLE_MAX_LENGTH),
        shortcut: match item.shortcut.as_deref() {
            Some(shortcut) if !shortcut.is_empty() => Some(shortcut.to_string()),
            _ => None,
        },
        content: truncate_utf16(&item.content, FIXED_TEXT_PRESET_CONTENT_MAX_LENGTH),
    }
}

/// `normalizeInGameSendFixedTextPresetItems`: drop id-less and duplicate-id
/// entries, then stop at the item limit.
pub fn normalize_fixed_text_items(items: &[FixedTextPresetItem]) -> Vec<FixedTextPresetItem> {
    let mut seen = Vec::new();
    let mut normalized = Vec::new();

    for item in items {
        let item = normalize_fixed_text_item(item);

        if item.id.is_empty() || seen.contains(&item.id) {
            continue;
        }

        seen.push(item.id.clone());
        normalized.push(item);

        if normalized.len() >= FIXED_TEXT_PRESET_MAX_ITEMS {
            break;
        }
    }

    normalized
}

/// `String.prototype.slice(0, limit)` counts UTF-16 units. Cutting on a code
/// point boundary instead means an astral character is kept whole rather than
/// leaving a lone surrogate behind; everything else is identical.
fn truncate_utf16(value: &str, limit: usize) -> String {
    let mut kept = String::new();
    let mut units = 0usize;

    for ch in value.chars() {
        let width = ch.len_utf16();
        if units + width > limit {
            break;
        }
        units += width;
        kept.push(ch);
    }

    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_shared_factory() {
        let rating = RatingPresetOptions::default();
        assert!(rating.kda);
        assert!(rating.win_rate);
        assert!(rating.avg_solo_kills);
        assert!(!rating.avg_vision_score);
        assert!(!rating.show_current_champion);
        assert_eq!(rating.name_display_strategy, NameDisplayStrategy::PreferChampionName);

        let jungle = JunglePresetOptions::default();
        assert!(jungle.activity_preference);
        assert!(jungle.first_clear_distribution);
        assert!(jungle.show_current_champion);

        let premade = PremadePresetOptions::default();
        assert_eq!(premade.name_display_strategy, NameDisplayStrategy::PreferChampionName);

        assert_eq!(
            rating.target_shortcuts,
            TargetShortcuts {
                friendly: None,
                enemy: None,
                all: None
            }
        );
    }

    #[test]
    fn options_serialize_with_the_renderer_property_names() {
        let value = serde_json::to_value(RatingPresetOptions::default()).unwrap();
        assert_eq!(
            value["nameDisplayStrategy"],
            json!("preferChampionName")
        );
        assert_eq!(value["avgCsPerMinute"], json!(false));
        assert_eq!(value["targetShortcuts"]["friendly"], json!(null));

        let item = serde_json::to_value(FixedTextPresetItem {
            id: "plain-id".to_string(),
            title: "hi".to_string(),
            shortcut: None,
            content: String::new(),
        })
        .unwrap();
        assert_eq!(item["shortcut"], json!(null));
    }

    #[test]
    fn patch_merges_scalars_and_keeps_omitted_keys() {
        let merged = merge_rating_options(
            &RatingPresetOptions::default(),
            &RatingPresetOptionPatch {
                win_rate: Some(false),
                name_display_strategy: Some(NameDisplayStrategy::PreferName),
                ..Default::default()
            },
        );

        assert!(!merged.win_rate);
        assert_eq!(merged.name_display_strategy, NameDisplayStrategy::PreferName);
        assert!(merged.kda, "an absent key must not clear the current value");
        assert!(merged.main_champions);
    }

    #[test]
    fn shortcut_patch_merges_per_target() {
        let current = JunglePresetOptions {
            target_shortcuts: TargetShortcuts {
                friendly: Some("Ctrl+1".to_string()),
                enemy: Some("Ctrl+2".to_string()),
                all: None,
            },
            ..Default::default()
        };

        let merged = merge_jungle_options(
            &current,
            &JunglePresetOptionPatch {
                target_shortcuts: Some(TargetShortcutsPatch {
                    enemy: Some(None),
                    all: Some(Some("Ctrl+3".to_string())),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        assert_eq!(merged.target_shortcuts.friendly.as_deref(), Some("Ctrl+1"));
        assert_eq!(merged.target_shortcuts.enemy, None);
        assert_eq!(merged.target_shortcuts.all.as_deref(), Some("Ctrl+3"));
    }

    #[test]
    fn premade_patch_carries_only_what_it_declares() {
        let merged = merge_premade_options(
            &PremadePresetOptions::default(),
            &PremadePresetOptionPatch {
                name_display_strategy: Some(NameDisplayStrategy::ChampionNameWithName),
                target_shortcuts: Some(TargetShortcutsPatch {
                    friendly: Some(Some("Ctrl+9".to_string())),
                    ..Default::default()
                }),
            },
        );

        assert_eq!(
            merged.name_display_strategy,
            NameDisplayStrategy::ChampionNameWithName
        );
        assert_eq!(merged.target_shortcuts.get(PresetTarget::Friendly), Some("Ctrl+9"));
        assert_eq!(merged.target_shortcuts.get(PresetTarget::Enemy), None);
    }

    #[test]
    fn normalize_applies_the_shared_limits() {
        let item = FixedTextPresetItem {
            id: "plain-id".to_string(),
            title: "t".repeat(FIXED_TEXT_PRESET_TITLE_MAX_LENGTH + 1),
            shortcut: Some(String::new()),
            content: "c".repeat(FIXED_TEXT_PRESET_CONTENT_MAX_LENGTH + 1),
        };

        let normalized = normalize_fixed_text_item(&item);
        assert_eq!(normalized.title.len(), FIXED_TEXT_PRESET_TITLE_MAX_LENGTH);
        assert_eq!(normalized.content.len(), FIXED_TEXT_PRESET_CONTENT_MAX_LENGTH);
        assert_eq!(normalized.shortcut, None, "an empty shortcut is stored as null");
    }

    #[test]
    fn normalize_items_drops_blank_and_duplicate_ids_and_stops_at_the_cap() {
        let mut items = vec![FixedTextPresetItem {
            id: String::new(),
            title: "invalid".to_string(),
            ..Default::default()
        }];

        for index in 0..=FIXED_TEXT_PRESET_MAX_ITEMS {
            items.push(FixedTextPresetItem {
                id: format!("item-{index}"),
                ..Default::default()
            });
        }
        items.push(FixedTextPresetItem {
            id: "item-0".to_string(),
            title: "duplicate".to_string(),
            ..Default::default()
        });

        let normalized = normalize_fixed_text_items(&items);

        assert_eq!(normalized.len(), FIXED_TEXT_PRESET_MAX_ITEMS);
        assert_eq!(normalized[0].id, "item-0");
        assert_eq!(normalized[1].id, "item-1");
        assert_eq!(normalized.last().unwrap().id, format!("item-{}", FIXED_TEXT_PRESET_MAX_ITEMS - 1));
        assert_eq!(normalized[0].title, "");
    }

    #[test]
    fn truncate_counts_utf16_units_and_keeps_pairs_whole() {
        assert_eq!(truncate_utf16("abcd", 2), "ab");
        assert_eq!(truncate_utf16("a😀b", 2), "a", "an astral pair does not fit");
        assert_eq!(truncate_utf16("a😀b", 3), "a😀");
    }

    #[test]
    fn send_interval_clamps_at_zero_and_reports_the_default_when_absent() {
        assert_eq!(clamp_send_interval(Some(-12.0)), 0);
        assert_eq!(clamp_send_interval(Some(0.0)), 0);
        assert_eq!(clamp_send_interval(Some(65.0)), 65);
        assert_eq!(clamp_send_interval(Some(3500.0)), 3500);
        assert_eq!(clamp_send_interval(Some(1.9)), 1);
        assert_eq!(clamp_send_interval(None), DEFAULT_SEND_INTERVAL);
        assert_eq!(clamp_send_interval(Some(f64::NAN)), DEFAULT_SEND_INTERVAL);
    }

    #[test]
    fn display_option_keys_are_the_shared_union() {
        let keys = rating_display_option_keys();
        assert_eq!(keys.len(), 12);
        assert_eq!(keys[0], "kda");
        assert_eq!(keys[1], "winRate");
        assert!(keys.contains(&"avgDamageGoldEfficiency"));
        assert!(keys.contains(&"mainPositions"));
    }

    #[test]
    fn target_names_round_trip() {
        for target in PresetTarget::ALL {
            assert_eq!(PresetTarget::from_str_name(target.as_str()), Some(target));
        }
        assert_eq!(PresetTarget::from_str_name("neutral"), None);
    }
}
