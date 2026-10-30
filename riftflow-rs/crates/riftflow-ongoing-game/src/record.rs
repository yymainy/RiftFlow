//! `shared/utils/ranked.ts` `parseSelectedRole`, plus the record helpers every
//! prop needs. Rust has no JS object semantics, so ordered records are carried
//! as `Vec<(String, T)>` and only turned into a JSON object on the way out.

use serde::Serialize;
use serde_json::{Map, Value, json};

pub type JsonObject = Map<String, Value>;

/// `Record<string, T>` with the original's insertion order kept.
pub type Pairs<T> = Vec<(String, T)>;

pub fn object<T: Serialize>(pairs: &[(String, T)]) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.clone(), serde_json::to_value(value).unwrap_or(Value::Null));
    }
    Value::Object(map)
}

/// `Object.entries` of a JSON object. Key order follows `serde_json`'s map, which
/// is sorted: the original's records here are `TEAM-100`/`TEAM-200`/`TEAM-ALL`/
/// `LOBBY`/puuid keys, none of which the renderer reads in a specific order.
pub fn entries(value: &Value) -> Vec<(String, Value)> {
    match value {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Vec::new(),
    }
}

pub fn as_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

pub fn str_field(value: &Value, key: &str) -> String {
    as_str(value.get(key)).unwrap_or_default().to_string()
}

pub fn i64_field(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or_default()
}

pub fn bool_field(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// `ParsedRole` — the five dotted segments of `selectedRole`.
pub fn parse_selected_role(role: Option<&str>) -> Value {
    let none = || json!({
        "current": "NONE",
        "assignmentReason": "NONE",
        "primary": "NONE",
        "secondary": "NONE",
        "fill": "NONE",
    });

    let Some(role) = role else {
        return none();
    };

    let segments: Vec<&str> = role.split('.').collect();
    if segments.len() != 4 && segments.len() != 5 {
        return none();
    }

    json!({
        "current": segments[0],
        "assignmentReason": segments[1],
        "primary": segments[2],
        "secondary": segments[3],
        "fill": segments.get(4).copied().unwrap_or("NONE"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_four_segment_role_has_no_fill() {
        assert_eq!(
            parse_selected_role(Some("JUNGLE.PRIMARY.JUNGLE.TOP")),
            json!({
                "current": "JUNGLE",
                "assignmentReason": "PRIMARY",
                "primary": "JUNGLE",
                "secondary": "TOP",
                "fill": "NONE",
            })
        );
    }

    #[test]
    fn a_five_segment_role_keeps_its_fill() {
        assert_eq!(parse_selected_role(Some("TOP.FILL_SECONDARY.TOP.JUNGLE.NONE")).get("fill"), Some(&json!("NONE")));
        assert_eq!(
            parse_selected_role(Some("BOTTOM.SECONDARY.TOP.BOTTOM.AUTOFILL")).get("fill"),
            Some(&json!("AUTOFILL"))
        );
    }

    #[test]
    fn anything_but_four_or_five_segments_is_all_none() {
        let expected = json!({
            "current": "NONE",
            "assignmentReason": "NONE",
            "primary": "NONE",
            "secondary": "NONE",
            "fill": "NONE",
        });

        assert_eq!(parse_selected_role(None), expected);
        assert_eq!(parse_selected_role(Some("")), expected);
        assert_eq!(parse_selected_role(Some("TOP.JUNGLE")), expected);
        assert_eq!(parse_selected_role(Some("A.B.C.D.E.F")), expected);
    }

    #[test]
    fn records_keep_the_order_they_were_built_in() {
        let pairs: Pairs<Vec<String>> = vec![
            ("TEAM-100".to_string(), vec!["a".to_string()]),
            ("TEAM-200".to_string(), vec![]),
        ];

        assert_eq!(
            object(&pairs),
            json!({ "TEAM-100": ["a"], "TEAM-200": [] })
        );
    }
}
