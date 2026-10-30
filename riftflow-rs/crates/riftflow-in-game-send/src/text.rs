//! Preset wording and number formatting, replacing `presets/i18n.ts` plus the
//! `Number.prototype.toFixed` / `String(rate)` calls the builders rely on.
//!
//! The templates are the `in-game-send-main.presets.*` block of
//! `shared/i18n/{zh-CN,en}/main.yaml` and the `positions.*` block of
//! `shared/i18n/{zh-CN,en}/common.yaml`, copied verbatim: the shipped renderer
//! shows these lines to the user, so their punctuation has to stay where the
//! original put it.
//!
//! The app layer passes the locale in (it is the `locale` setting, the same
//! value i18next was initialized with) — this crate holds no global.

use std::borrow::Cow;

/// The two locales the original registered resources for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    ZhCn,
    En,
}

impl Locale {
    /// i18next was initialized with `fallbackLng: 'zh-CN'`, so any tag without
    /// resources falls back to Chinese rather than to English.
    pub fn from_language_tag(tag: &str) -> Self {
        let lowered = tag.to_ascii_lowercase();
        if lowered.starts_with("en") {
            Locale::En
        } else {
            Locale::ZhCn
        }
    }

    /// Which plural suffix i18next appends for `count`.
    ///
    /// English has `one`/`other`; Chinese has `other` only, so the `_one` rows
    /// of the Chinese table are never reached even though the yaml defines them.
    fn plural_suffix(self, count: f64) -> &'static str {
        match self {
            Locale::En if count == 1.0 => "_one",
            _ => "_other",
        }
    }
}

/// `{{name}}` substitutions, in the order the original passed them to `t()`.
#[derive(Clone, Default, Debug)]
pub struct Args(Vec<(&'static str, Cow<'static, str>)>);

impl Args {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn arg(mut self, name: &'static str, value: impl Into<Cow<'static, str>>) -> Self {
        self.0.push((name, value.into()));
        self
    }

    /// `String(count)` — i18next reads `count` both for plural selection and for
    /// the `{{count}}` placeholder.
    pub fn count(self, count: f64) -> Self {
        self.arg("count", number_to_string(count))
    }}

/// Every template the ported builders use, as `(key, en, zh)`.
/// Keys are relative to `in-game-send-main.presets`; `positions.*` are the
/// separate `common` namespace entries and keep their own prefix.
const MESSAGES: &[(&str, &str, &str)] = &[
    // punctuation
    ("punctuation.lineSeparator", ": ", "："),
    ("punctuation.listSeparator", ", ", "，"),
    ("punctuation.memberSeparator", ", ", ", "),
    ("punctuation.groupSeparator", " ", " "),
    // common
    ("common.currentChampion", "Current Champion", "当前英雄"),
    ("common.thisChampion", "this champion", "本英雄"),
    ("common.noChampionSelected", "No champion selected", "尚未选择英雄"),
    ("common.noMainChampions", "No clear main champions", "无明显主玩英雄"),
    (
        "common.championWithPlayer",
        "{{champion}} ({{player}})",
        "{{champion}}（{{player}}）",
    ),
    ("common.team.friendly", "Allied", "我方"),
    ("common.team.enemy", "Enemy", "敌方"),
    (
        "common.team.labelWithName",
        "{{side}} · {{team}}",
        "{{side}} · {{team}}",
    ),
    // rating
    ("rating.noRecentMatches", "No recent matches", "近期没有对局记录"),
    (
        "rating.noChampionMatches",
        "No recent matches on {{champion}}",
        "近期没有{{champion}}对局记录",
    ),
    ("rating.matchCount_one", "{{count}} match", "{{count}}场对局"),
    ("rating.matchCount_other", "{{count}} matches", "{{count}}场对局"),
    (
        "rating.championMatchCount_one",
        "{{count}} match on {{champion}}",
        "{{champion}}{{count}}场",
    ),
    (
        "rating.championMatchCount_other",
        "{{count}} matches on {{champion}}",
        "{{champion}}{{count}}场",
    ),
    ("rating.countWithStats", "{{countText}}, {{stats}}", "{{countText}}，{{stats}}"),
    ("rating.metrics.winRate", "Win Rate {{value}}", "胜率{{value}}"),
    ("rating.metrics.kda", "KDA {{value}}", "KDA{{value}}"),
    (
        "rating.metrics.avgSoloKills",
        "Avg Solo Kills {{value}}",
        "场均单杀{{value}}",
    ),
    (
        "rating.metrics.avgVisionScore",
        "Vision {{value}}",
        "场均视野得分{{value}}",
    ),
    (
        "rating.metrics.avgChampionDamage",
        "Damage {{value}}",
        "伤害{{value}}",
    ),
    ("rating.metrics.avgDamageTaken", "Taken {{value}}", "承伤{{value}}"),
    ("rating.metrics.avgGold", "Gold {{value}}", "经济{{value}}"),
    (
        "rating.metrics.avgCsPerMinute",
        "CS {{value}}/min",
        "补兵{{value}}/分",
    ),
    (
        "rating.metrics.avgKillParticipation",
        "KP {{value}}",
        "参团率{{value}}",
    ),
    (
        "rating.metrics.avgDamageGoldEfficiency",
        "Damage/Gold {{value}}",
        "伤转比{{value}}",
    ),
    (
        "rating.metrics.mainChampions",
        "Main Champions [{{champions}}]",
        "主玩英雄[{{champions}}]",
    ),
    (
        "rating.metrics.mainPositions",
        "Main Positions [{{positions}}]",
        "主玩位置[{{positions}}]",
    ),
    // jungle
    ("jungle.noRecords", "No jungle-detail records", "没有打野明细记录"),
    (
        "jungle.noChampionRecords",
        "No jungle-detail records on {{champion}}",
        "没有{{champion}}打野明细记录",
    ),
    (
        "jungle.sampleCount_one",
        "{{count}} jungle sample",
        "打野样本{{count}}场",
    ),
    (
        "jungle.sampleCount_other",
        "{{count}} jungle samples",
        "打野样本{{count}}场",
    ),
    (
        "jungle.championSampleCount_one",
        "{{count}} jungle sample on {{champion}}",
        "{{champion}}打野样本{{count}}场",
    ),
    (
        "jungle.championSampleCount_other",
        "{{count}} jungle samples on {{champion}}",
        "{{champion}}打野样本{{count}}场",
    ),
    (
        "jungle.firstDragonAverageTime",
        ", first dragon avg {{time}}",
        "，首龙均时{{time}}",
    ),
    ("jungle.camps.blue", "Blue Buff", "蓝Buff"),
    ("jungle.camps.red", "Red Buff", "红Buff"),
    ("jungle.camps.wolves", "Wolves", "三狼"),
    ("jungle.camps.raptors", "Raptors", "锋喙鸟"),
    ("jungle.campRate", "{{camp}} {{rate}}", "{{camp}}{{rate}}"),
    ("jungle.lanes.top", "top", "上"),
    ("jungle.lanes.mid", "mid", "中"),
    ("jungle.lanes.bot", "bot", "下"),
    (
        "jungle.lanePreference.unclear",
        "no clear lane preference",
        "分布不明显",
    ),
    ("jungle.lanePreference.single", "favors {{lane}}", "偏{{lane}}"),
    (
        "jungle.lanePreference.double",
        "favors {{first}}/{{second}}",
        "偏{{first}}{{second}}",
    ),
    ("jungle.firstClear.normal", "standard", "常规开"),
    ("jungle.firstClear.invade", "invade", "入侵开"),
    ("jungle.firstClear.blue", "blue-side", "蓝方"),
    ("jungle.firstClear.red", "red-side", "红方"),
    (
        "jungle.firstClear.pattern",
        "{{side}} {{kind}} {{rate}}{{distribution}}",
        "{{side}}{{kind}}{{rate}}{{distribution}}",
    ),
    (
        "jungle.metrics.activityPreference",
        "Early {{preference}}, top {{top}} mid {{mid}} bot {{bot}}",
        "前期{{preference}}，上{{top}}中{{mid}}下{{bot}}",
    ),
    ("jungle.metrics.level3Gank", "Lv3 gank {{value}}", "3级抓{{value}}"),
    ("jungle.metrics.level4Gank", "Lv4 gank {{value}}", "4级抓{{value}}"),
    (
        "jungle.metrics.firstDragon",
        "First dragon {{value}}{{time}}",
        "一龙率{{value}}{{time}}",
    ),
    ("jungle.metrics.avgDragons", "Avg dragons {{value}}", "场均小龙{{value}}"),
    (
        "jungle.metrics.monsterControl",
        "Objectives voidgrubs {{voidgrubs}}/heralds {{heralds}}/barons {{barons}}",
        "野怪资源巢虫{{voidgrubs}}/先锋{{heralds}}/大龙{{barons}}",
    ),
    (
        "jungle.metrics.mainChampions",
        "Main Champions [{{champions}}]",
        "主玩英雄[{{champions}}]",
    ),
    // premade
    ("premade.blueTeamTitle", "Blue premades", "蓝方开黑"),
    ("premade.redTeamTitle", "Red premades", "红方开黑"),
    ("premade.teamTitle", "Premades", "开黑"),
    ("premade.blueTeamEmpty", "No blue premades", "蓝方无开黑"),
    ("premade.redTeamEmpty", "No red premades", "红方无开黑"),
    ("premade.empty", "No premade groups", "无开黑小队"),
    // common namespace: positions
    ("positions.all", "None", "无"),
    ("positions.ALL", "None", "无"),
    ("positions.top", "Top", "上路"),
    ("positions.jungle", "Jungle", "打野"),
    ("positions.mid", "Middle", "中路"),
    ("positions.middle", "Middle", "中路"),
    ("positions.bot", "Bottom", "下路"),
    ("positions.bottom", "Bottom", "下路"),
    ("positions.support", "Support", "辅助"),
    ("positions.utility", "Support", "辅助"),
    ("positions.TOP", "Top", "上路"),
    ("positions.JUNGLE", "Jungle", "打野"),
    ("positions.MID", "Middle", "中路"),
    ("positions.MIDDLE", "Middle", "中路"),
    ("positions.BOT", "Bottom", "下路"),
    ("positions.BOTTOM", "Bottom", "下路"),
    ("positions.SUPPORT", "Support", "辅助"),
    ("positions.UTILITY", "Support", "辅助"),
];

fn message(locale: Locale, key: &str) -> Option<&'static str> {
    MESSAGES
        .iter()
        .find(|(name, _, _)| *name == key)
        .map(|(_, en, zh)| match locale {
            Locale::En => *en,
            Locale::ZhCn => *zh,
        })
}

/// `presetT` / `presetCommonT`: interpolate one table entry.
///
/// A missing key yields the key itself, which is what i18next returns when no
/// resource and no `defaultValue` match; no message is invented for it.
pub fn preset(locale: Locale, key: &str, args: Args) -> String {
    match message(locale, key) {
        Some(template) => interpolate(template, &args),
        None => key.to_string(),
    }
}

/// `presetCommonT(key)`
pub fn common(locale: Locale, key: &str, args: Args) -> String {
    preset(locale, &format!("common.{key}"), args)
}

/// `presetT(key, { count })` — resolves the plural suffix i18next adds.
pub fn preset_plural(locale: Locale, key: &str, count: f64, args: Args) -> String {
    let plural_key = format!("{key}{}", locale.plural_suffix(count));
    preset(locale, &plural_key, args.count(count))
}

/// `presetPunctuation(key)`
pub fn punctuation(locale: Locale, kind: Punctuation) -> &'static str {
    let key = match kind {
        Punctuation::LineSeparator => "punctuation.lineSeparator",
        Punctuation::ListSeparator => "punctuation.listSeparator",
        Punctuation::MemberSeparator => "punctuation.memberSeparator",
        Punctuation::GroupSeparator => "punctuation.groupSeparator",
    };
    message(locale, key).unwrap_or("")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Punctuation {
    LineSeparator,
    ListSeparator,
    MemberSeparator,
    GroupSeparator,
}

/// `joinPresetList`
pub fn join_list(locale: Locale, values: &[String]) -> String {
    values.join(punctuation(locale, Punctuation::ListSeparator))
}

/// `joinPresetMembers`
pub fn join_members(locale: Locale, values: &[String]) -> String {
    values.join(punctuation(locale, Punctuation::MemberSeparator))
}

/// `presetPositionName(position)`: an exact `positions.<position>` hit, otherwise
/// the raw position, which is the `defaultValue` the original passed.
pub fn position_name(locale: Locale, position: &str) -> String {
    match message(locale, &format!("positions.{position}")) {
        Some(name) => name.to_string(),
        None => position.to_string(),
    }
}

fn interpolate(template: &str, args: &Args) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            // an unbalanced `{{` is data, not a placeholder
            out.push_str("{{");
            rest = after_open;
            continue;
        };
        let name = &after_open[..close];
        let value = args
            .0
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_ref())
            .unwrap_or("");
        out.push_str(value);
        rest = &after_open[close + 2..];
    }

    out.push_str(rest);
    out
}

/// `(rate * 100).toFixed(0) + '%'` — the original's ratio rendering.
pub fn format_rate(rate: f64) -> String {
    format!("{}%", to_fixed(rate * 100.0, 0))
}

/// `String(number)` for the values i18next interpolates directly — a whole number
/// loses its fractional part, `NaN` and the infinities keep their words.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() };
    }
    if value == value.trunc() && value.abs() < 1e21 {
        return format!("{}", value as i64);
    }

    exponential_or_plain(value)
}

fn exponential_or_plain(value: f64) -> String {
    if value.abs() >= 1e21 {
        exponential(value)
    } else {
        format!("{}", value)
    }
}

/// `Number.prototype.toFixed(digits)`.
///
/// Three properties of the original matter here, because the presets print rates
/// and averages straight into chat:
/// - ties round *away from zero* on the value as the double actually holds it, so
///   `0.125 -> "0.13"` (a naive `{:.2}` gives the even-rounded `"0.12"`) while
///   `1.005 -> "1.00"` (the double is just below the tie);
/// - the sign is stripped before rounding, so `-0.0004 -> "-0.00"`;
/// - beyond 1e21 the original switches to `String(x)`, i.e. exponential notation.
pub fn to_fixed(value: f64, digits: usize) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }

    let negative = value.is_sign_negative();
    let magnitude = value.abs();

    if magnitude.is_infinite() {
        return if negative {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }

    // `Number.prototype.toFixed` throws past 100 digits; the callers use 0..=2.
    let digits = digits.min(100);

    if magnitude >= 1e21 {
        return exponential(value);
    }

    let rounded = match round_half_up_scaled(magnitude, digits) {
        Some(rounded) => rounded.to_string(),
        // the fall-back keeps working for the precisions and magnitudes whose
        // exact decimal expansion does not fit a u128
        None => format!("{}", (magnitude * 10f64.powi(digits as i32) + 0.5).floor()),
    };

    if digits == 0 {
        return format!("{sign}{rounded}", sign = prefix(negative));
    }

    let (integer, fraction) = if rounded.len() > digits {
        let split = rounded.len() - digits;
        (&rounded[..split], &rounded[split..])
    } else {
        (
            "0",
            &*format!("{}{rounded}", "0".repeat(digits - rounded.len())),
        )
    };

    format!("{sign}{integer}.{fraction}", sign = prefix(negative))
}

/// `floor(value * 10^digits + 0.5)` on the *exact* binary value of the double.
///
/// Rounding the product after a floating-point multiply is not enough: `8.835`
/// holds slightly less than `8.835`, but multiplying by 100 rounds that up to
/// exactly `883.5`, and a tie would then round up to `8.84` where the original
/// prints `8.83`. Working on the mantissa and binary exponent keeps the two
/// cases apart. `None` means the arithmetic did not fit — `NaN`, a subnormal, or
/// a precision past 10^38.
fn round_half_up_scaled(value: f64, digits: usize) -> Option<u128> {
    if value == 0.0 {
        return Some(0);
    }

    let (mantissa, exponent) = binary_decomposition(value);
    let scale = 10u128.checked_pow(digits as u32)?;
    let scaled = u128::from(mantissa).checked_mul(scale)?;

    if exponent >= 0 {
        // a whole number scaled by a power of ten stays whole
        return scaled.checked_mul(1u128.checked_shl(exponent as u32)?);
    }

    let shift = (-exponent) as u32;
    let numerator = scaled.checked_shl(1)?.checked_add(1u128.checked_shl(shift)?)?;

    Some(numerator >> (shift + 1))
}

/// `value == mantissa * 2^exponent` with a whole `mantissa`.
fn binary_decomposition(value: f64) -> (u64, i32) {
    let bits = value.to_bits();
    let raw_exponent = ((bits >> 52) & 0x7FF) as i32;
    let fraction = bits & 0xF_FFFF_FFFF_FFFF;

    if raw_exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), raw_exponent - 1075)
    }
}

fn prefix(negative: bool) -> &'static str {
    if negative {
        "-"
    } else {
        ""
    }
}

/// `String(x)` for the magnitudes where `toFixed` defers to it: the shortest
/// round-trip mantissa with an explicit, signed exponent.
fn exponential(value: f64) -> String {
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
    let exponent = exponent.trim_start_matches('+');
    let exponent = exponent.trim_start_matches('0');
    let exponent = if exponent.is_empty() { "0" } else { exponent };

    if value < 0.0 {
        format!("-{mantissa}e+{exponent}")
    } else {
        format!("{mantissa}e+{exponent}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_breaks_ties_away_from_zero() {
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(1.25, 1), "1.3");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(0.5, 0), "1");
    }

    #[test]
    fn to_fixed_keeps_the_binary_value_of_the_double() {
        // both of these look like a `.x5` tie, but the doubles behind them sit on
        // opposite sides of it, and the original rounds each on its exact value
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(2.675, 2), "2.67");
        assert_eq!(to_fixed(1.335, 2), "1.33");
        assert_eq!(to_fixed(100.345, 2), "100.34");
        assert_eq!(to_fixed(8.835, 2), "8.84");
        assert_eq!(to_fixed(2.34, 2), "2.34");
        assert_eq!(to_fixed(4.321, 2), "4.32");
        assert_eq!(to_fixed(1.7, 1), "1.7");
        assert_eq!(to_fixed(0.4, 1), "0.4");
    }

    #[test]
    fn to_fixed_formats_sign_exponent_and_specials() {
        assert_eq!(to_fixed(-0.0004, 2), "-0.00");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
        assert_eq!(to_fixed(f64::NAN, 2), "NaN");
        assert_eq!(to_fixed(f64::INFINITY, 2), "Infinity");
        assert_eq!(to_fixed(f64::NEG_INFINITY, 2), "-Infinity");
    }

    #[test]
    fn format_rate_matches_the_original_percent_text() {
        assert_eq!(format_rate(0.59), "59%");
        assert_eq!(format_rate(0.667), "67%");
        assert_eq!(format_rate(0.625), "63%");
        assert_eq!(format_rate(0.125), "13%");
        assert_eq!(format_rate(0.0), "0%");
        assert_eq!(format_rate(1.18), "118%");
    }

    #[test]
    fn language_tag_falls_back_to_chinese_like_i18next() {
        assert_eq!(Locale::from_language_tag("zh-TW"), Locale::ZhCn);
        assert_eq!(Locale::from_language_tag("en-US"), Locale::En);
        assert_eq!(Locale::from_language_tag("ja"), Locale::ZhCn);
    }

    #[test]
    fn plural_suffixes_follow_the_registered_resources() {
        assert_eq!(
            preset_plural(Locale::En, "rating.matchCount", 1.0, Args::new()),
            "1 match"
        );
        assert_eq!(
            preset_plural(Locale::En, "rating.matchCount", 50.0, Args::new()),
            "50 matches"
        );
        assert_eq!(
            preset_plural(Locale::ZhCn, "rating.matchCount", 1.0, Args::new()),
            "1场对局"
        );
        assert_eq!(
            preset_plural(
                Locale::ZhCn,
                "rating.championMatchCount",
                16.0,
                Args::new().arg("champion", "本英雄")
            ),
            "本英雄16场"
        );
    }

    #[test]
    fn interpolation_substitutes_every_named_argument() {
        assert_eq!(
            common(
                Locale::ZhCn,
                "championWithPlayer",
                Args::new().arg("champion", "盲僧").arg("player", "RIP#81406")
            ),
            "盲僧（RIP#81406）"
        );
        assert_eq!(
            common(
                Locale::En,
                "championWithPlayer",
                Args::new().arg("champion", "Lee Sin").arg("player", "RIP#81406")
            ),
            "Lee Sin (RIP#81406)"
        );
    }

    #[test]
    fn a_missing_key_is_not_replaced_with_invented_text() {
        assert_eq!(preset(Locale::ZhCn, "rating.not-a-key", Args::new()), "rating.not-a-key");
    }

    #[test]
    fn position_names_cover_both_spellings_and_pass_unknown_values_through() {
        assert_eq!(position_name(Locale::ZhCn, "MIDDLE"), "中路");
        assert_eq!(position_name(Locale::En, "UTILITY"), "Support");
        assert_eq!(position_name(Locale::En, "NONE"), "NONE");
    }

    #[test]
    fn separators_are_locale_specific() {
        assert_eq!(punctuation(Locale::ZhCn, Punctuation::LineSeparator), "：");
        assert_eq!(punctuation(Locale::En, Punctuation::LineSeparator), ": ");
        assert_eq!(
            join_list(
                Locale::ZhCn,
                &["盲僧".to_string(), "深海泰坦".to_string()]
            ),
            "盲僧，深海泰坦"
        );
        assert_eq!(
            join_members(Locale::En, &["A".to_string(), "B".to_string()]),
            "A, B"
        );
    }
}
