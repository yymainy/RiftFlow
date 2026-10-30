//! Fixed-text preset editing, ported from the fixed-text half of
//! `preset-controller.ts`.
//!
//! These functions are the whole edit surface minus the setting service: the app
//! layer owns persistence and the keyboard-shortcut registration the controller
//! reacts to, so here an edit is a pure mutation of the item list followed by the
//! same normalization the original ran before saving.

use crate::options::{
    normalize_fixed_text_item, normalize_fixed_text_items, FixedTextPresetItem,
    FixedTextPresetItemPatch, MoveDirection, FIXED_TEXT_PRESET_MAX_ITEMS,
};

/// Why a fixed-text edit could not be applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresetEditError {
    /// `IN_GAME_SEND_FIXED_TEXT_PRESET_MAX_ITEMS` already reached.
    LimitReached,
    /// No item with that id in the list.
    NotFound,
}

impl std::fmt::Display for PresetEditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            PresetEditError::LimitReached => "Fixed text preset item limit reached",
            PresetEditError::NotFound => "Fixed text preset item not found",
        })
    }
}

impl std::error::Error for PresetEditError {}

/// `createFixedTextPresetItem`.
///
/// The id is passed in because the original generates it with `randomUUID()` in
/// the main process; this crate has no randomness of its own, so the app layer
/// supplies the new id (and must keep it stable for the shortcut target id).
pub fn create_fixed_text_item(
    items: &mut Vec<FixedTextPresetItem>,
    id: String,
) -> Result<FixedTextPresetItem, PresetEditError> {
    if items.len() >= FIXED_TEXT_PRESET_MAX_ITEMS {
        return Err(PresetEditError::LimitReached);
    }

    let item = FixedTextPresetItem {
        id,
        title: String::new(),
        shortcut: None,
        content: String::new(),
    };

    items.push(item.clone());
    normalize(items);

    Ok(item)
}

/// `updateFixedTextPresetItem`: `{ ...item, ...patch, id }` then normalize.
pub fn update_fixed_text_item(
    items: &mut Vec<FixedTextPresetItem>,
    id: &str,
    patch: &FixedTextPresetItemPatch,
) -> Result<FixedTextPresetItem, PresetEditError> {
    let index = items
        .iter()
        .position(|item| item.id == id)
        .ok_or(PresetEditError::NotFound)?;

    let current = &items[index];
    let merged = FixedTextPresetItem {
        id: id.to_string(),
        title: patch.title.clone().unwrap_or_else(|| current.title.clone()),
        shortcut: match &patch.shortcut {
            Some(shortcut) => shortcut.clone(),
            None => current.shortcut.clone(),
        },
        content: patch.content.clone().unwrap_or_else(|| current.content.clone()),
    };
    let next_item = normalize_fixed_text_item(&merged);

    items[index] = next_item.clone();
    normalize(items);

    Ok(next_item)
}

/// `deleteFixedTextPresetItem` — `false` when nothing had that id.
pub fn delete_fixed_text_item(items: &mut Vec<FixedTextPresetItem>, id: &str) -> bool {
    let before = items.len();
    items.retain(|item| item.id != id);

    if items.len() == before {
        return false;
    }

    normalize(items);
    true
}

/// `moveFixedTextPresetItem` — `false` when the item is missing or already at the
/// end the direction points to.
pub fn move_fixed_text_item(
    items: &mut Vec<FixedTextPresetItem>,
    id: &str,
    direction: MoveDirection,
) -> bool {
    let Some(index) = items.iter().position(|item| item.id == id) else {
        return false;
    };

    let next_index = match direction {
        MoveDirection::Up => index.checked_sub(1),
        MoveDirection::Down => index.checked_add(1).filter(|next| *next < items.len()),
    };

    let Some(next_index) = next_index else {
        return false;
    };

    items.swap(index, next_index);
    normalize(items);
    true
}

/// `generateFixedTextPresetLines`: the item's content split into the lines that
/// actually get sent. An unknown id produces nothing, exactly as the original's
/// `if (!item) return []`.
pub fn fixed_text_lines(items: &[FixedTextPresetItem], id: &str) -> Vec<String> {
    let Some(item) = items.iter().find(|item| item.id == id) else {
        return Vec::new();
    };

    split_lines(&item.content)
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// `content.split(/\r\n|\n|\r/)`
fn split_lines(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut characters = content.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                // a CRLF pair is one separator, not two
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                lines.push(std::mem::take(&mut current));
            }
            '\n' => lines.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }

    lines.push(current);
    lines
}

fn normalize(items: &mut Vec<FixedTextPresetItem>) {
    *items = normalize_fixed_text_items(items);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> FixedTextPresetItem {
        FixedTextPresetItem {
            id: id.to_string(),
            title: format!("title-{id}"),
            shortcut: None,
            content: "line 1\n\n  \nline 2\r\n\t\rline 3".to_string(),
        }
    }

    #[test]
    fn creates_an_empty_item_and_appends_it() {
        let mut items = vec![item("a")];

        let created = create_fixed_text_item(&mut items, "new-id".to_string()).unwrap();

        assert_eq!(
            created,
            FixedTextPresetItem {
                id: "new-id".to_string(),
                title: String::new(),
                shortcut: None,
                content: String::new(),
            }
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].id, "new-id");
    }

    #[test]
    fn refuses_past_the_shared_limit() {
        let mut items: Vec<FixedTextPresetItem> = (0..FIXED_TEXT_PRESET_MAX_ITEMS)
            .map(|index| item(&format!("item-{index}")))
            .collect();

        let error = create_fixed_text_item(&mut items, "one-too-many".to_string())
            .expect_err("the limit must be enforced");

        assert_eq!(error, PresetEditError::LimitReached);
        assert_eq!(
            error.to_string(),
            "Fixed text preset item limit reached",
            "the renderer shows this text"
        );
        assert_eq!(items.len(), FIXED_TEXT_PRESET_MAX_ITEMS);
    }

    #[test]
    fn patches_only_the_keys_the_renderer_sent() {
        let mut items = vec![FixedTextPresetItem {
            id: "a".to_string(),
            title: "hello".to_string(),
            shortcut: Some("Ctrl+Shift+F".to_string()),
            content: "body".to_string(),
        }];

        let updated = update_fixed_text_item(
            &mut items,
            "a",
            &FixedTextPresetItemPatch {
                content: Some("new body".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(updated.title, "hello");
        assert_eq!(updated.shortcut.as_deref(), Some("Ctrl+Shift+F"));
        assert_eq!(updated.content, "new body");
        assert_eq!(items[0], updated);
    }

    #[test]
    fn an_explicit_empty_shortcut_clears_it() {
        let mut items = vec![FixedTextPresetItem {
            id: "a".to_string(),
            title: String::new(),
            shortcut: Some("Ctrl+Shift+F".to_string()),
            content: String::new(),
        }];

        let updated = update_fixed_text_item(
            &mut items,
            "a",
            &FixedTextPresetItemPatch {
                shortcut: Some(None),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(updated.shortcut, None);
    }

    #[test]
    fn updating_an_unknown_id_is_an_error() {
        let mut items = vec![item("a")];

        let error = update_fixed_text_item(&mut items, "missing", &Default::default())
            .expect_err("nothing to update");

        assert_eq!(error, PresetEditError::NotFound);
        assert_eq!(
            error.to_string(),
            "Fixed text preset item not found"
        );
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn delete_reports_whether_something_was_removed() {
        let mut items = vec![item("a"), item("b")];

        assert!(delete_fixed_text_item(&mut items, "a"));
        assert_eq!(items.len(), 1);
        assert!(!delete_fixed_text_item(&mut items, "a"));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn move_swaps_with_the_neighbour_and_refuses_the_edges() {
        let mut items = vec![item("a"), item("b"), item("c")];

        assert!(!move_fixed_text_item(&mut items, "a", MoveDirection::Up));
        assert!(move_fixed_text_item(&mut items, "b", MoveDirection::Up));
        assert_eq!(
            ids(&items),
            vec!["b".to_string(), "a".to_string(), "c".to_string()]
        );

        assert!(
            !move_fixed_text_item(&mut items, "c", MoveDirection::Down),
            "the last item cannot move down"
        );
        assert!(move_fixed_text_item(&mut items, "c", MoveDirection::Up));
        assert_eq!(
            ids(&items),
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );

        assert!(!move_fixed_text_item(&mut items, "missing", MoveDirection::Up));
    }

    #[test]
    fn edits_pass_through_the_same_normalization_as_saving() {
        let mut items = vec![FixedTextPresetItem {
            id: "a".to_string(),
            title: String::new(),
            shortcut: None,
            content: String::new(),
        }];

        update_fixed_text_item(
            &mut items,
            "a",
            &FixedTextPresetItemPatch {
                title: Some("t".repeat(200)),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(items[0].title.chars().count(), 64);
    }

    #[test]
    fn lines_split_on_every_end_of_line_style_and_drop_blanks() {
        let items = vec![item("a")];

        assert_eq!(
            fixed_text_lines(&items, "a"),
            vec!["line 1".to_string(), "line 2".to_string(), "line 3".to_string()]
        );
        assert!(fixed_text_lines(&items, "missing").is_empty());
    }

    #[test]
    fn split_lines_treats_crlf_as_one_separator() {
        assert_eq!(split_lines("a\r\nb\nc\rd"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines(""), [""]);
        assert_eq!(split_lines("only"), ["only"]);
    }

    fn ids(items: &[FixedTextPresetItem]) -> Vec<String> {
        items.iter().map(|item| item.id.clone()).collect()
    }
}
