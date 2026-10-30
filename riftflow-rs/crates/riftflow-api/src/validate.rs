//! `shared/shards/riftflow-api/schemas.ts`, ported.
//!
//! Zod's job here is a gate, not a mapper: a document that does not match is
//! dropped (and its cache file deleted), so nothing but the log says it happened.
//! The port keeps that split — every function returns the value to store, or the
//! reason it was rejected. The modes are the original's too: `.strict()` for the
//! feature-gate snapshot and its rules (an unknown key, like the legacy
//! `{ enabled: true }`, rejects the whole document), `.strip()` for the bootstrap
//! document and an auto-select group's `name`, `.passthrough()` for everything
//! else, where unknown keys are kept because the renderer may read them.

use serde_json::{Map, Value, json};

use crate::config::ConfigResource;
use crate::semver;
use crate::time::iso_datetime_to_millis;

/// A document that passed validation, normalized the way zod would return it.
pub type Validated = Result<Value, String>;

/// `RiftFlowFeatureGatePlatform`
const FEATURE_GATE_PLATFORMS: [&str; 2] = ["win32", "darwin"];
/// `RiftFlowNoticeSeverity`
const NOTICE_SEVERITIES: [&str; 3] = ["low", "medium", "high"];
/// `RiftFlowContactChannelPlatform`
const CONTACT_PLATFORMS: [&str; 6] = ["qq", "telegram", "discord", "email", "website", "other"];
/// `RiftFlowApiLanguage`
const API_LANGUAGES: [&str; 2] = crate::config::RIFTFLOW_API_LANGUAGES;

fn fail<T>(path: &str, reason: &str) -> Result<T, String> {
    Err(format!("{path}: {reason}"))
}

fn as_object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, String> {
    match value {
        Value::Object(object) => Ok(object),
        _ => fail(path, "expected an object"),
    }
}

fn as_array<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, String> {
    match value {
        Value::Array(items) => Ok(items),
        _ => fail(path, "expected an array"),
    }
}

fn required<'a>(object: &'a Map<String, Value>, key: &str, path: &str) -> Result<&'a Value, String> {
    object
        .get(key)
        .ok_or_else(|| format!("{path}.{key}: required field is missing"))
}

fn required_str<'a>(object: &'a Map<String, Value>, key: &str, path: &str) -> Result<&'a str, String> {
    let value = required(object, key, path)?;
    value
        .as_str()
        .ok_or_else(|| format!("{path}.{key}: expected a string"))
}

fn required_bool(
    object: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<bool, String> {
    let value = required(object, key, path)?;
    value.as_bool().ok_or_else(|| format!("{path}.{key}: expected a boolean"))
}

fn check_number(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::Number(number) if is_plain_number(number) => Ok(()),
        _ => fail(path, "expected a number"),
    }
}

/// `z.number()` — JSON numbers only, and no NaN/Infinity (which serde_json's
/// default parser cannot even produce).
fn is_plain_number(number: &serde_json::Number) -> bool {
    number.is_i64() || number.is_u64() || number.as_f64().is_some_and(f64::is_finite)
}

fn check_string(value: &Value, path: &str) -> Result<(), String> {
    if value.is_string() {
        Ok(())
    } else {
        fail(path, "expected a string")
    }
}

fn check_enum(value: &Value, allowed: &[&str], path: &str) -> Result<(), String> {
    match value.as_str() {
        Some(text) if allowed.contains(&text) => Ok(()),
        _ => fail(path, &format!("expected one of {}", allowed.join(", "))),
    }
}

/// `z.iso.datetime({ offset: true })`, which is also what `Date.parse` reads.
fn check_timestamp(value: &Value, path: &str) -> Result<(), String> {
    let text = value.as_str().ok_or_else(|| format!("{path}: expected a string"))?;
    match iso_datetime_to_millis(text) {
        Some(_) => Ok(()),
        None => fail(path, "expected an ISO-8601 datetime with offset"),
    }
}

fn check_updated_at(object: &Map<String, Value>, path: &str) -> Result<(), String> {
    let value = required(object, "updatedAt", path)?;
    check_timestamp(value, &format!("{path}.updatedAt"))
}

/// `.strict()` — `Unrecognized key(s) in object`.
fn reject_unknown(object: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), String> {
    let unknown: Vec<&str> = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .map(String::as_str)
        .collect();

    if unknown.is_empty() {
        return Ok(());
    }
    fail(
        path,
        &format!("unrecognized key(s) in object: {}", unknown.join(", ")),
    )
}

fn is_number_value(value: &Value) -> bool {
    matches!(value, Value::Number(number) if is_plain_number(number))
}

/// `UniqueNonEmptyStringArraySchema`: `z.array(z.string().trim().min(1).max(64)).min(1)`
/// plus a uniqueness check, which zod runs on the *trimmed* items.
fn trimmed_unique_string_array(
    value: &Value,
    path: &str,
    min_items: usize,
    max_item_len: usize,
) -> Result<Vec<String>, String> {
    let items = as_array(value, path)?;
    if items.len() < min_items {
        return fail(path, &format!("expected at least {min_items} item(s)"));
    }

    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let item_path = format!("{path}[{index}]");
        let text = item
            .as_str()
            .ok_or_else(|| format!("{item_path}: expected a string"))?
            .trim();
        if text.is_empty() || text.chars().count() > max_item_len {
            return fail(
                &item_path,
                &format!("expected a string of 1..={max_item_len} characters"),
            );
        }
        if out.iter().any(|existing: &String| existing == text) {
            return fail(path, "expected unique items");
        }
        out.push(text.to_string());
    }

    Ok(out)
}

/// `z.url()` narrowed to the `https:` / `mailto:` refine the schema adds. Node's
/// `new URL()` is the reference, so a URL with a space or an empty host fails.
fn check_contact_url(value: &Value, path: &str) -> Result<(), String> {
    let Some(text) = value.as_str() else {
        return fail(path, "expected a string");
    };
    let lower = text.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("https://") {
        let host = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if host.is_empty() || host.chars().any(char::is_whitespace) {
            return fail(path, "expected a valid https url");
        }
        if text.chars().any(char::is_whitespace) {
            return fail(path, "expected a valid https url");
        }
        return Ok(());
    }

    if let Some(rest) = lower.strip_prefix("mailto:") {
        if rest.is_empty() || rest.chars().any(char::is_whitespace) {
            return fail(path, "expected a valid mailto url");
        }
        return Ok(());
    }

    fail(path, "url must use https or mailto")
}

// ---------------------------------------------------------------------------
// config documents
// ---------------------------------------------------------------------------

/// `RiftFlowFeatureGateSnapshotSchema` — `.strict()`, so this is also what deletes a
/// legacy cached document.
pub fn validate_feature_gate_snapshot(value: &Value) -> Validated {
    const PATH: &str = "featureGates";
    let object = as_object(value, PATH)?;
    reject_unknown(object, &["updatedAt", "gates"], PATH)?;
    check_updated_at(object, PATH)?;

    let gates = as_object(required(object, "gates", PATH)?, &format!("{PATH}.gates"))?;
    let mut normalized = Map::new();

    for (key, rule) in gates {
        if !is_feature_gate_key(key) {
            return Err(format!("{PATH}.gates.{key}: invalid feature gate id"));
        }
        normalized.insert(
            key.clone(),
            Value::Object(validate_feature_gate_rule(rule, &format!("{PATH}.gates.{key}"))?),
        );
    }

    Ok(json!({
        "updatedAt": object.get("updatedAt"),
        "gates": Value::Object(normalized),
    }))
}

/// `^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)+$` — at least one dot, so `foo` is not a
/// gate id and `match-history.bulk-collection` is.
pub fn is_feature_gate_key(key: &str) -> bool {
    // at least one dot, and every segment `^[a-z][a-z0-9-]*$`
    key.contains('.') && key.split('.').all(is_gate_segment)
}

fn is_gate_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else { return false };
    first.is_ascii_lowercase()
        && chars.all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-')
}

fn validate_feature_gate_rule(value: &Value, path: &str) -> Result<Map<String, Value>, String> {
    let rule = as_object(value, path)?;
    reject_unknown(
        rule,
        &["platforms", "minVersionInclusive", "maxVersionExclusive", "sgpServers"],
        path,
    )?;

    let mut out = Map::new();

    if let Some(platforms) = rule.get("platforms") {
        let items = as_array(platforms, &format!("{path}.platforms"))?;
        if items.is_empty() {
            return fail(&format!("{path}.platforms"), "expected at least 1 item(s)");
        }
        let mut seen = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let item_path = format!("{path}.platforms[{index}]");
            check_enum(item, &FEATURE_GATE_PLATFORMS, &item_path)?;
            let text = item.as_str().unwrap_or_default();
            if seen.iter().any(|existing: &String| existing == text) {
                return fail(&format!("{path}.platforms"), "expected unique items");
            }
            seen.push(text.to_string());
        }
        out.insert("platforms".to_string(), Value::Array(items.clone()));
    }

    let minimum = optional_bound(rule, "minVersionInclusive", path)?;
    let maximum = optional_bound(rule, "maxVersionExclusive", path)?;
    if let Some(minimum) = &minimum {
        out.insert(
            "minVersionInclusive".to_string(),
            Value::String(minimum.clone()),
        );
    }
    if let Some(maximum) = &maximum {
        out.insert(
            "maxVersionExclusive".to_string(),
            Value::String(maximum.clone()),
        );
    }

    // the `superRefine` bounds check: min < max, both valid SemVer
    if let (Some(minimum), Some(maximum)) = (&minimum, &maximum) {
        if semver::gte(minimum, maximum).unwrap_or(false) {
            return fail(
                &format!("{path}.maxVersionExclusive"),
                "maxVersionExclusive must be greater than minVersionInclusive",
            );
        }
    }

    if let Some(servers) = rule.get("sgpServers") {
        let items = trimmed_unique_string_array(servers, &format!("{path}.sgpServers"), 1, 64)?;
        out.insert(
            "sgpServers".to_string(),
            Value::Array(items.into_iter().map(Value::String).collect()),
        );
    }

    Ok(out)
}

fn optional_bound(
    rule: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<String>, String> {
    let Some(value) = rule.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let text = value
        .as_str()
        .ok_or_else(|| format!("{path}.{key}: expected a string"))?;
    if !semver::is_valid(text) {
        return fail(&format!("{path}.{key}"), &format!("{key} must be valid SemVer"));
    }
    Ok(Some(text.to_string()))
}

/// `RiftFlowSupportedQueuesConfigSchema`
pub fn validate_supported_queues(value: &Value) -> Validated {
    const PATH: &str = "supportedQueues";
    let object = as_object(value, PATH)?;
    check_updated_at(object, PATH)?;

    let queues = as_array(required(object, "queues", PATH)?, &format!("{PATH}.queues"))?;
    for (index, queue) in queues.iter().enumerate() {
        check_number(queue, &format!("{PATH}.queues[{index}]"))?;
    }

    Ok(value.clone())
}

/// `RiftFlowLeagueServersConfigSchema`
pub fn validate_league_servers(value: &Value) -> Validated {
    const PATH: &str = "leagueServers";
    let object = as_object(value, PATH)?;
    check_updated_at(object, PATH)?;

    let servers = as_object(required(object, "servers", PATH)?, &format!("{PATH}.servers"))?;
    for (server_id, endpoint) in servers {
        let path = format!("{PATH}.servers.{server_id}");
        let endpoint = as_object(endpoint, &path)?;
        required_str(endpoint, "matchHistory", &path)?;
        required_str(endpoint, "common", &path)?;
        required_bool(endpoint, "isTencent", &path)?;
        if let Some(param) = endpoint.get("regionPathParam") {
            check_string(param, &format!("{path}.regionPathParam"))?;
        }
    }

    let names = as_object(
        required(object, "serverNames", PATH)?,
        &format!("{PATH}.serverNames"),
    )?;
    for (language, entries) in names {
        let localized = as_object(entries, &format!("{PATH}.serverNames.{language}"))?;
        for (server_id, name) in localized {
            check_string(
                name,
                &format!("{PATH}.serverNames.{language}.{server_id}"),
            )?;
        }
    }

    Ok(value.clone())
}

/// `RiftFlowAutoSelectGroupsConfigSchema`
pub fn validate_auto_select_groups(value: &Value) -> Validated {
    const PATH: &str = "autoSelectGroups";
    let object = as_object(value, PATH)?;
    check_updated_at(object, PATH)?;

    let groups = as_array(required(object, "groups", PATH)?, &format!("{PATH}.groups"))?;
    let mut normalized = Vec::with_capacity(groups.len());

    for (index, group) in groups.iter().enumerate() {
        let path = format!("{PATH}.groups[{index}]");
        validate_auto_select_group(group, &path)?;

        // the group is `.passthrough()`, but its `name` is a plain object, so the
        // two declared locales are all that survives into the stored document
        let mut group = as_object(group, &path)?.clone();
        group.insert(
            "name".to_string(),
            Value::Object(Map::from_iter([
                (
                    "zh-CN".to_string(),
                    group
                        .get("name")
                        .and_then(|name| name.get("zh-CN"))
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "en".to_string(),
                    group
                        .get("name")
                        .and_then(|name| name.get("en"))
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
            ])),
        );
        normalized.push(Value::Object(group));
    }

    let mut out = object.clone();
    out.insert("groups".to_string(), Value::Array(normalized));
    Ok(Value::Object(out))
}

fn validate_auto_select_group(value: &Value, path: &str) -> Result<(), String> {
    let group = as_object(value, path)?;

    required_str(group, "groupId", path)?;

    // `name` is a plain object, so zod strips every locale it did not declare
    let name = as_object(
        required(group, "name", path)?,
        &format!("{path}.name"),
    )?;
    required_str(name, "zh-CN", &format!("{path}.name"))?;
    required_str(name, "en", &format!("{path}.name"))?;

    // `z.string().regex(/^\/lol-game-data\/assets\/.+/)` — the prefix plus one char
    const ICON_PREFIX: &str = "/lol-game-data/assets/";
    let icon_path = required_str(group, "iconPath", path)?;
    if icon_path.len() <= ICON_PREFIX.len() || !icon_path.starts_with(ICON_PREFIX) {
        return fail(
            &format!("{path}.iconPath"),
            "iconPath must match /^\\/lol-game-data\\/assets\\/.+/",
        );
    }

    required_bool(group, "isCustom", path)?;

    let servers = as_array(
        required(group, "supportedSgpServers", path)?,
        &format!("{path}.supportedSgpServers"),
    )?;
    if servers.is_empty() {
        return fail(
            &format!("{path}.supportedSgpServers"),
            "expected at least 1 item(s)",
        );
    }
    for (index, server) in servers.iter().enumerate() {
        check_string(server, &format!("{path}.supportedSgpServers[{index}]"))?;
    }

    let targets = as_array(
        required(group, "targetGameModes", path)?,
        &format!("{path}.targetGameModes"),
    )?;
    for (index, target) in targets.iter().enumerate() {
        let target_path = format!("{path}.targetGameModes[{index}]");
        let target = as_object(target, &target_path)?;
        required_str(target, "gameMode", &target_path)?;
        let queues = as_array(required(target, "queueTypes", &target_path)?, &target_path)?;
        for (queue_index, queue) in queues.iter().enumerate() {
            check_string(queue, &format!("{target_path}.queueTypes[{queue_index}]"))?;
        }
    }

    for field in [
        "positions",
        "additionalPicks",
        "additionalBans",
        "excludedPicks",
        "excludedBans",
    ] {
        let items = as_array(required(group, field, path)?, &format!("{path}.{field}"))?;
        let check: fn(&Value, &str) -> Result<(), String> = if field == "positions" {
            check_string
        } else {
            check_number
        };
        for (index, item) in items.iter().enumerate() {
            check(item, &format!("{path}.{field}[{index}]"))?;
        }
    }

    Ok(())
}

/// `isAutoSelectGroupSupportedOnSgpServer` from `shared/shards/riftflow-api/auto-select.ts`.
///
/// `league_servers` is the `leagueServers.servers` map (the shape `AutoSelectState`
/// hands the JS function), not the whole config document: an id that the map does
/// not know is unsupported even when a group lists it.
pub fn is_auto_select_group_supported_on_sgp_server(
    group: &Value,
    sgp_server_id: &str,
    league_servers: &Value,
) -> bool {
    let supported = group
        .get("supportedSgpServers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if supported.iter().any(|item| item.as_str() == Some("*")) {
        return true;
    }

    !sgp_server_id.is_empty()
        // `Object.hasOwn(leagueServers, id)`, which a null-valued key still satisfies
        && league_servers.get(sgp_server_id).is_some()
        && supported
            .iter()
            .any(|item| item.as_str() == Some(sgp_server_id))
}

// ---------------------------------------------------------------------------
// non-cached content
// ---------------------------------------------------------------------------

/// `RiftFlowNoticeSchema` — `.passthrough()`; the modal renders `content` verbatim.
pub fn validate_notice(value: &Value) -> Validated {
    const PATH: &str = "notice";
    let object = as_object(value, PATH)?;

    required_str(object, "revision", PATH)?;
    check_enum(
        required(object, "language", PATH)?,
        &API_LANGUAGES,
        &format!("{PATH}.language"),
    )?;
    check_enum(
        required(object, "severity", PATH)?,
        &NOTICE_SEVERITIES,
        &format!("{PATH}.severity"),
    )?;
    required_str(object, "summary", PATH)?;

    let content_type = required_str(object, "contentType", PATH)?;
    if content_type != "text/markdown" {
        return fail(
            &format!("{PATH}.contentType"),
            "expected the literal \"text/markdown\"",
        );
    }

    required_str(object, "content", PATH)?;
    check_updated_at(object, PATH)?;

    Ok(value.clone())
}

/// `RiftFlowContactChannelsSchema` — `.passthrough()`, with `.trim()` on the four
/// strings zod transforms.
pub fn validate_contact_channels(value: &Value) -> Validated {
    const PATH: &str = "contactChannels";
    let object = as_object(value, PATH)?;
    check_updated_at(object, PATH)?;

    let channels = as_array(required(object, "channels", PATH)?, &format!("{PATH}.channels"))?;
    let mut normalized = Vec::with_capacity(channels.len());

    for (index, channel) in channels.iter().enumerate() {
        let path = format!("{PATH}.channels[{index}]");
        let channel = as_object(channel, &path)?;

        let mut out = channel.clone();
        trim_field(&mut out, "id", 64, &path)?;
        check_enum(
            required(channel, "platform", &path)?,
            &CONTACT_PLATFORMS,
            &format!("{path}.platform"),
        )?;
        trim_field(&mut out, "name", 80, &path)?;
        trim_field(&mut out, "identifier", 120, &path)?;
        check_contact_url(required(channel, "url", &path)?, &format!("{path}.url"))?;

        match required(channel, "password", &path)? {
            Value::Null => {}
            _ => trim_field(&mut out, "password", 120, &path)?,
        };

        if !is_contact_channel_id(
            channel
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim(),
        ) {
            return fail(&format!("{path}.id"), "expected /^[a-z0-9][a-z0-9_-]*$/i");
        }

        normalized.push(Value::Object(out));
    }

    let mut out = object.clone();
    out.insert("channels".to_string(), Value::Array(normalized));
    Ok(Value::Object(out))
}

/// `z.string().trim().min(1).max(len)` applied in place, as zod's transform does.
fn trim_field(
    object: &mut Map<String, Value>,
    key: &str,
    max_len: usize,
    path: &str,
) -> Result<(), String> {
    let field_path = format!("{path}.{key}");
    let value = object
        .get(key)
        .ok_or_else(|| format!("{field_path}: required field is missing"))?;
    if value.is_null() {
        return Ok(());
    }
    let text = value
        .as_str()
        .ok_or_else(|| format!("{field_path}: expected a string"))?
        .trim();
    let length = text.chars().count();
    if length == 0 || length > max_len {
        return Err(format!(
            "{field_path}: expected a string of 1..={max_len} characters"
        ));
    }
    object.insert(key.to_string(), Value::String(text.to_string()));
    Ok(())
}

/// `^[a-z0-9][a-z0-9_-]*$`, case-insensitive.
fn is_contact_channel_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_ascii_alphanumeric())
        && chars.all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-')
}

/// `RiftFlowReleaseSchema`
pub fn validate_release(value: &Value) -> Validated {
    const PATH: &str = "release";
    let object = as_object(value, PATH)?;

    required_str(object, "version", PATH)?;
    let published_at = required(object, "publishedAt", PATH)?;
    check_timestamp(published_at, &format!("{PATH}.publishedAt"))?;
    required_str(object, "description", PATH)?;

    let artifacts = as_array(required(object, "artifacts", PATH)?, &format!("{PATH}.artifacts"))?;
    for (index, artifact) in artifacts.iter().enumerate() {
        let path = format!("{PATH}.artifacts[{index}]");
        let artifact = as_object(artifact, &path)?;
        for field in ["platform", "arch", "fileName", "contentType"] {
            required_str(artifact, field, &path)?;
        }
        check_number(required(artifact, "size", &path)?, &format!("{path}.size"))?;
        match required(artifact, "sha256", &path)? {
            Value::Null => {}
            sha256 => check_string(sha256, &format!("{path}.sha256"))?,
        }
        // `z.url()`, and a download link has to be fetchable over https
        let url = required_str(artifact, "downloadUrl", &path)?;
        if !url.to_ascii_lowercase().starts_with("https://") {
            return fail(&format!("{path}.downloadUrl"), "expected an https url");
        }
    }

    Ok(value.clone())
}

/// `RiftFlowApiBootstrapSchema` — `.strip()`, so only these three fields survive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootstrapDocument {
    pub generation: i64,
    pub base_urls: crate::config::RiftFlowServiceBaseUrls,
}

impl BootstrapDocument {
    /// The cached file's content — `writeToJsonConfigFile(path, bootstrap)` wrote
    /// the parsed document back out, so the schema version stays on disk.
    pub fn to_json(&self) -> Value {
        json!({
            "schemaVersion": 1,
            "generation": self.generation,
            "baseUrls": self.base_urls,
        })
    }
}

pub fn validate_bootstrap_document(value: &Value) -> Result<BootstrapDocument, String> {
    const PATH: &str = "bootstrap";
    let object = as_object(value, PATH)?;

    let version = required(object, "schemaVersion", PATH)?;
    if version.as_i64() != Some(1) {
        return fail(&format!("{PATH}.schemaVersion"), "expected the literal 1");
    }

    let generation = required(object, "generation", PATH)?;
    if !is_number_value(generation) {
        return fail(&format!("{PATH}.generation"), "expected a number");
    }
    let generation = generation.as_i64().unwrap_or_default();

    let base_urls = object.get("baseUrls").ok_or_else(|| {
        format!("{PATH}.baseUrls: required field is missing")
    })?;
    let base_urls = as_object(base_urls, &format!("{PATH}.baseUrls"))?;
    let api = required_str(base_urls, "api", &format!("{PATH}.baseUrls"))?;
    let static_assets = required_str(base_urls, "static", &format!("{PATH}.baseUrls"))?;

    Ok(BootstrapDocument {
        generation,
        base_urls: crate::config::RiftFlowServiceBaseUrls {
            api: api.to_string(),
            static_assets: static_assets.to_string(),
        },
    })
}

/// The schema for one cached resource — `RIFTFLOW_API_CACHED_RESOURCES[].schema`.
pub fn validate_config(resource: ConfigResource, value: &Value) -> Validated {
    match resource {
        ConfigResource::FeatureGates => validate_feature_gate_snapshot(value),
        ConfigResource::SupportedQueues => validate_supported_queues(value),
        ConfigResource::LeagueServers => validate_league_servers(value),
        ConfigResource::AutoSelectGroups => validate_auto_select_groups(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture `config-loader.test.ts` loads from the local cache.
    fn auto_select_groups() -> Value {
        json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "groups": [{
                "groupId": "ranked",
                "name": { "zh-CN": "排位模式", "en": "Ranked" },
                "iconPath": "/lol-game-data/assets/content/src/LeagueClient/GameModeAssets/Classic_SRU/img/game-select-icon-hover.png",
                "isCustom": false,
                "supportedSgpServers": ["*"],
                "targetGameModes": [
                    { "gameMode": "CLASSIC", "queueTypes": ["RANKED_SOLO_5x5", "RANKED_FLEX_SR"] }
                ],
                "positions": ["top", "jungle", "middle", "bottom", "utility"],
                "additionalPicks": [],
                "additionalBans": [],
                "excludedPicks": [-1],
                "excludedBans": []
            }]
        })
    }

    fn notice() -> Value {
        json!({
            "revision": "notice-revision",
            "language": "zh-CN",
            "severity": "medium",
            "summary": "测试公告",
            "contentType": "text/markdown",
            "content": "# 测试公告",
            "updatedAt": "2026-07-20T04:00:00.000Z"
        })
    }

    fn contact_channels() -> Value {
        json!({
            "updatedAt": "2026-07-20T04:00:00.000Z",
            "channels": [{
                "id": "qq-outpost",
                "platform": "qq",
                "name": "据点",
                "identifier": "123456",
                "url": "https://qm.qq.com/q/example",
                "password": "riftflow"
            }]
        })
    }

    #[test]
    fn a_valid_feature_gate_snapshot_keeps_its_rules() {
        let snapshot = json!({
            "updatedAt": "2099-01-02T00:00:00.000Z",
            "gates": {
                "ongoing-game.deobfuscation": { "platforms": ["win32"] },
                "match-history.bulk": {
                    "minVersionInclusive": "1.5.0",
                    "maxVersionExclusive": "2.0.0",
                    "sgpServers": ["  TENCENT_HN1 ", "SG2"]
                }
            }
        });

        let validated = validate_feature_gate_snapshot(&snapshot).unwrap();
        assert_eq!(
            validated["gates"]["ongoing-game.deobfuscation"]["platforms"],
            json!(["win32"])
        );
        assert_eq!(
            validated["gates"]["match-history.bulk"]["maxVersionExclusive"],
            json!("2.0.0")
        );
        assert_eq!(validated["updatedAt"], json!("2099-01-02T00:00:00.000Z"));
    }

    #[test]
    fn a_legacy_feature_gate_document_is_rejected() {
        // `.strict()` is what deletes the old cache file: `enabled` is not a rule
        let legacy = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "match-history.bulk-collection": { "enabled": true } }
        });

        let error = validate_feature_gate_snapshot(&legacy).unwrap_err();
        assert!(
            error.contains("unrecognized key(s)"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn a_gate_id_needs_at_least_one_dot() {
        assert!(is_feature_gate_key("match-history.bulk-collection"));
        assert!(is_feature_gate_key("a.b.c"));
        assert!(!is_feature_gate_key("matchHistory"));
        assert!(!is_feature_gate_key("match-history."));
        assert!(!is_feature_gate_key("1.history"));
        assert!(!is_feature_gate_key(""));
    }

    #[test]
    fn an_unparseable_gate_key_is_rejected_with_its_name() {
        let bad = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "singleSegment": {} }
        });
        let error = validate_feature_gate_snapshot(&bad).unwrap_err();
        assert!(error.contains("singleSegment"), "{error}");
    }

    #[test]
    fn semver_bounds_are_validated_and_ordered() {
        let bad_version = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "a.b": { "minVersionInclusive": ">=1.5.0" } }
        });
        assert!(validate_feature_gate_snapshot(&bad_version)
            .unwrap_err()
            .contains("must be valid SemVer"));

        let inverted = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": {
                "a.b": { "minVersionInclusive": "2.0.0", "maxVersionExclusive": "1.5.0" }
            }
        });
        assert!(validate_feature_gate_snapshot(&inverted)
            .unwrap_err()
            .contains("must be greater than minVersionInclusive"));

        // equal bounds are `gte`, so also rejected
        let equal = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": {
                "a.b": { "minVersionInclusive": "1.5.0", "maxVersionExclusive": "1.5.0" }
            }
        });
        assert!(validate_feature_gate_snapshot(&equal).is_err());
    }

    #[test]
    fn gate_platforms_are_a_unique_non_empty_enum_list() {
        for platforms in [
            json!([]),
            json!(["linux"]),
            json!(["win32", "win32"]),
            json!(["win32", 3]),
        ] {
            let bad = json!({
                "updatedAt": "2099-01-01T00:00:00.000Z",
                "gates": { "a.b": { "platforms": platforms } }
            });
            assert!(validate_feature_gate_snapshot(&bad).is_err());
        }

        let good = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "a.b": { "platforms": ["win32", "darwin"] } }
        });
        assert!(validate_feature_gate_snapshot(&good).is_ok());
    }

    #[test]
    fn gate_server_lists_are_trimmed_and_unique_after_trimming() {
        let good = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "a.b": { "sgpServers": [" TENCENT_HN1 ", "SG2"] } }
        });
        let validated = validate_feature_gate_snapshot(&good).unwrap();
        assert_eq!(
            validated["gates"]["a.b"]["sgpServers"],
            json!(["TENCENT_HN1", "SG2"]),
            "zod's .trim() lands in the stored document"
        );

        let duplicate = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "a.b": { "sgpServers": ["HN1", " HN1"] } }
        });
        assert!(validate_feature_gate_snapshot(&duplicate).is_err());

        let blank = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "a.b": { "sgpServers": ["   "] } }
        });
        assert!(validate_feature_gate_snapshot(&blank).is_err());
    }

    #[test]
    fn a_feature_gate_snapshot_needs_a_real_updated_at() {
        for stamp in [json!("2099-01-01"), json!(42), json!("")] {
            let bad = json!({ "updatedAt": stamp, "gates": {} });
            assert!(validate_feature_gate_snapshot(&bad).is_err());
        }

        assert!(validate_feature_gate_snapshot(&json!({ "gates": {} })).is_err());
        assert!(validate_feature_gate_snapshot(&json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": {}
        }))
        .is_ok());
    }

    #[test]
    fn supported_queues_only_accepts_numbers() {
        assert!(validate_supported_queues(&json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "queues": [420, 1700]
        }))
        .is_ok());

        // the exact fixture from `config-loader.test.ts`
        let invalid = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "queues": ["invalid"]
        });
        assert_eq!(
            validate_supported_queues(&invalid).unwrap_err(),
            "supportedQueues.queues[0]: expected a number"
        );
    }

    #[test]
    fn a_valid_auto_select_group_document_survives() {
        let document = auto_select_groups();
        let validated = validate_auto_select_groups(&document).unwrap();
        assert_eq!(validated["groups"][0]["name"], document["groups"][0]["name"]);
        assert_eq!(validated, document);
    }

    #[test]
    fn an_extra_locale_in_a_group_name_is_stripped() {
        let document = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "groups": [{
                "groupId": "ranked",
                "name": { "zh-CN": "排位模式", "en": "Ranked", "ja": "ランク" },
                "iconPath": "/lol-game-data/assets/a.png",
                "isCustom": false,
                "supportedSgpServers": ["*"],
                "targetGameModes": [],
                "positions": [],
                "additionalPicks": [],
                "additionalBans": [],
                "excludedPicks": [],
                "excludedBans": []
            }]
        });

        let validated = validate_auto_select_groups(&document).unwrap();
        assert_eq!(
            validated["groups"][0]["name"],
            json!({ "zh-CN": "排位模式", "en": "Ranked" })
        );
    }

    #[test]
    fn an_auto_select_group_rejects_a_foreign_icon_path() {
        let mut document = auto_select_groups();
        document["groups"][0]["iconPath"] = json!("/lol-planned/assets/icon.png");
        assert!(validate_auto_select_groups(&document)
            .unwrap_err()
            .contains("iconPath"));

        let mut document = auto_select_groups();
        document["groups"][0]["iconPath"] = json!("/lol-game-data/assets/");
        assert!(validate_auto_select_groups(&document).is_err());
    }

    #[test]
    fn an_auto_select_group_keeps_its_extra_fields() {
        let mut document = auto_select_groups();
        document["groups"][0]["futureField"] = json!("kept");
        let validated = validate_auto_select_groups(&document).unwrap();
        assert_eq!(validated["groups"][0]["futureField"], json!("kept"));
    }

    #[test]
    fn an_auto_select_group_needs_its_declared_shapes() {
        let missing = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "groups": [{ "groupId": "ranked" }]
        });
        assert!(validate_auto_select_groups(&missing)
            .unwrap_err()
            .contains("name"));

        let mut document = auto_select_groups();
        document["groups"][0]["excludedPicks"] = json!(["-1"]);
        assert!(validate_auto_select_groups(&document).is_err());

        let mut document = auto_select_groups();
        document["groups"][0]["supportedSgpServers"] = json!([]);
        assert!(validate_auto_select_groups(&document).is_err());
    }

    #[test]
    fn group_support_is_star_or_an_existing_server() {
        let star = json!({ "supportedSgpServers": ["*"] });
        assert!(is_auto_select_group_supported_on_sgp_server(
            &star,
            "",
            &json!({})
        ));

        let servers = json!({ "TENCENT_HN1": { "common": "https://hn1" } });
        let listed = json!({ "supportedSgpServers": ["TENCENT_HN1", "SG2"] });
        assert!(is_auto_select_group_supported_on_sgp_server(
            &listed,
            "TENCENT_HN1",
            &servers
        ));
        // an id the league-servers config does not know is not supported
        assert!(!is_auto_select_group_supported_on_sgp_server(
            &listed,
            "KR",
            &servers
        ));
        assert!(!is_auto_select_group_supported_on_sgp_server(
            &listed,
            "",
            &servers
        ));
        assert!(!is_auto_select_group_supported_on_sgp_server(
            &json!({}),
            "TENCENT_HN1",
            &servers
        ));
    }

    #[test]
    fn support_needs_the_server_to_be_in_the_map_at_all() {
        let listing = json!({ "supportedSgpServers": ["SG2"] });
        // `Object.hasOwn` counts a null-valued key, so a stub entry still supports
        assert!(is_auto_select_group_supported_on_sgp_server(
            &listing,
            "SG2",
            &json!({ "SG2": null })
        ));
        assert!(!is_auto_select_group_supported_on_sgp_server(
            &listing,
            "SG2",
            &json!({})
        ));
    }

    #[test]
    fn league_servers_need_their_endpoint_fields() {
        let good = json!({
            "updatedAt": "2026-07-18T04:00:00.000Z",
            "servers": {
                "TENCENT_HN1": {
                    "matchHistory": "https://hn1-k8s-sgp.lol.qq.com:21019",
                    "common": "https://hn1-k8s-sgp.lol.qq.com:21019",
                    "isTencent": true
                },
                "PBE": {
                    "matchHistory": "https://usw2-red.pp.sgp.pvp.net",
                    "common": "https://pbe-red.lol.sgp.pvp.net",
                    "isTencent": false,
                    "regionPathParam": "PBE1"
                }
            },
            "serverNames": { "en": { "TENCENT_HN1": "Ionia" } }
        });
        assert!(validate_league_servers(&good).is_ok());

        let mut missing = good.clone();
        missing["servers"]["TENCENT_HN1"].as_object_mut().unwrap().remove("isTencent");
        assert!(validate_league_servers(&missing).is_err());

        let mut names = good.clone();
        names["serverNames"]["en"] = json!({ "TENCENT_HN1": 42 });
        assert!(validate_league_servers(&names).is_err());
    }

    #[test]
    fn a_notice_is_markdown_with_a_known_language_and_severity() {
        assert_eq!(validate_notice(&notice()).unwrap(), notice());

        let mut wrong_type = notice();
        wrong_type["contentType"] = json!("text/html");
        assert!(validate_notice(&wrong_type)
            .unwrap_err()
            .contains("text/markdown"));

        let mut wrong_severity = notice();
        wrong_severity["severity"] = json!("critical");
        assert!(validate_notice(&wrong_severity).is_err());

        let mut wrong_language = notice();
        wrong_language["language"] = json!("ja");
        assert!(validate_notice(&wrong_language).is_err());

        let mut no_content = notice();
        no_content.as_object_mut().unwrap().remove("content");
        assert!(validate_notice(&no_content).is_err());
    }

    #[test]
    fn a_notice_keeps_fields_the_schema_does_not_declare() {
        let mut document = notice();
        document["dismissable"] = json!(true);
        assert_eq!(
            validate_notice(&document).unwrap()["dismissable"],
            json!(true)
        );
    }

    #[test]
    fn contact_channels_accept_https_and_mailto_only() {
        assert_eq!(
            validate_contact_channels(&contact_channels()).unwrap(),
            contact_channels()
        );

        let mailto = json!({
            "updatedAt": "2026-07-20T04:00:00.000Z",
            "channels": [{
                "id": "mail",
                "platform": "email",
                "name": "Email",
                "identifier": "hi@example.com",
                "url": "mailto:hi@example.com",
                "password": null
            }]
        });
        assert!(validate_contact_channels(&mailto).is_ok());

        for url in [
            json!("ftp://example.com/x"),
            json!("http://example.com/x"),
            json!("https://"),
            json!("mailto:"),
            json!("not a url"),
        ] {
            let mut document = contact_channels();
            document["channels"][0]["url"] = url.clone();
            assert!(
                validate_contact_channels(&document).is_err(),
                "{url} should be rejected"
            );
        }
    }

    #[test]
    fn contact_channel_strings_are_trimmed_in_place() {
        let mut document = contact_channels();
        document["channels"][0]["name"] = json!("  据点  ");
        document["channels"][0]["identifier"] = json!(" 123456 ");
        document["channels"][0]["password"] = json!("  riftflow ");

        let validated = validate_contact_channels(&document).unwrap();
        assert_eq!(validated["channels"][0]["name"], json!("据点"));
        assert_eq!(validated["channels"][0]["identifier"], json!("123456"));
        assert_eq!(validated["channels"][0]["password"], json!("riftflow"));
    }

    #[test]
    fn contact_channel_ids_and_platforms_are_checked() {
        for id in ["-lead", "_lead", "has space", ""] {
            let mut document = contact_channels();
            document["channels"][0]["id"] = json!(id);
            assert!(
                validate_contact_channels(&document).is_err(),
                "{id:?} should be rejected"
            );
        }

        // the regex is case-insensitive
        let mut document = contact_channels();
        document["channels"][0]["id"] = json!("QQ-Outpost_2");
        assert!(validate_contact_channels(&document).is_ok());

        let mut document = contact_channels();
        document["channels"][0]["platform"] = json!("wechat");
        assert!(validate_contact_channels(&document).is_err());

        let mut document = contact_channels();
        document["channels"][0]["name"] = json!("   ");
        assert!(validate_contact_channels(&document).is_err());
    }

    #[test]
    fn a_release_needs_an_https_artifact_url() {
        let release = json!({
            "version": "1.5.0",
            "publishedAt": "2026-07-20T04:00:00.000Z",
            "description": "changes",
            "artifacts": [{
                "platform": "win32",
                "arch": "x64",
                "fileName": "RiftFlow Desktop-1.5.0-win.7z",
                "size": 128,
                "contentType": "application/x-7z-compressed",
                "sha256": null,
                "downloadUrl": "https://riftflow-api.yymainy.online/static/RiftFlow%20Desktop-1.5.0-win.7z"
            }]
        });
        assert_eq!(validate_release(&release).unwrap(), release);

        let mut insecure = release.clone();
        insecure["artifacts"][0]["downloadUrl"] = json!("http://riftflow-static.example/x.7z");
        assert!(validate_release(&insecure).is_err());

        let mut bad_date = release.clone();
        bad_date["publishedAt"] = json!("2026-07-20");
        assert!(validate_release(&bad_date).is_err());

        let mut size = release.clone();
        size["artifacts"][0]["size"] = json!("128");
        assert!(validate_release(&size).is_err());
    }

    #[test]
    fn a_bootstrap_document_is_the_one_supported_schema() {
        let document = json!({
            "schemaVersion": 1,
            "generation": 2,
            "baseUrls": {
                "api": "https://api-2.example.com",
                "static": "https://static-2.example.com",
                "ignored": "stripped"
            }
        });

        let parsed = validate_bootstrap_document(&document).unwrap();
        assert_eq!(parsed.generation, 2);
        assert_eq!(parsed.base_urls.api, "https://api-2.example.com");
        assert_eq!(parsed.base_urls.static_assets, "https://static-2.example.com");

        // the fixture from `base-urls.test.ts` / `bootstrap-controller.test.ts`
        assert_eq!(
            validate_bootstrap_document(&json!({ "schemaVersion": 2 })).unwrap_err(),
            "bootstrap.schemaVersion: expected the literal 1"
        );
        assert!(validate_bootstrap_document(&json!({
            "schemaVersion": 1,
            "generation": 1,
            "baseUrls": { "api": "https://a.example.com" }
        }))
        .is_err());
    }

    #[test]
    fn a_resource_picks_its_own_schema() {
        let queues = json!({ "updatedAt": "2099-01-01T00:00:00.000Z", "queues": [420] });
        assert!(validate_config(ConfigResource::SupportedQueues, &queues).is_ok());
        assert!(validate_config(ConfigResource::FeatureGates, &queues).is_err());
        assert!(validate_config(ConfigResource::AutoSelectGroups, &queues).is_err());
    }
}
