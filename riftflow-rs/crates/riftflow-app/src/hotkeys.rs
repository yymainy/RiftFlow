//! `keyboard-shortcuts-main` — the global keyboard shortcut shard.
//!
//! Port of `RiftFlow/src/main/shards/keyboard-shortcuts/`:
//! * `shortcut-registry.ts` → [`ShortcutRegistry`]
//! * `index.ts` (the pressed-key state machine) → [`KeyboardShortcutsMain`]
//! * `ipc-handlers.ts` → [`register_ipc`], wired up by [`start`]
//! * `@main/native`'s `riftflow-native-win32/input` addon → [`hook`] here.
//!   riftflow-app has no `windows` crate dependency, so the Win32 calls are raw
//!   `#[link]` externs — the same technique `app_common.rs` uses for `kernel32`.
//!
//! Availability means "a hook is actually listening on this platform": win32
//! **and** a successful `SetWindowsHookExW(WH_KEYBOARD_LL)`. The original's
//! condition (`src/main/native/index.ts:54`) ANDs that with `isElevated` — the
//! shipped addon needed elevation for `SendInput`, not for the hook, which any
//! process in the interactive session may install. Keeping that term made every
//! non-elevated run report `false` without ever attempting the hook. The genuine
//! limitation is UIPI, not installation: while a window owned by an elevated
//! process is in the foreground, its keystrokes do not reach this hook.
//!
//! Without availability `register()` drops the request and `getRegistration()`
//! answers `null`, so the settings page reports the shortcut as unregistered
//! instead of claiming a hook that is not listening.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use riftflow_ipc::{RiftFlowIpcMain, IpcError};
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

pub const NAMESPACE: &str = "keyboard-shortcuts-main";
pub const DISABLED_KEYS_TARGET_ID: &str = "riftflow-disabled-keys";
pub const DEBUG_STATEFUL_TEST_TARGET_ID: &str = "keyboard-shortcuts-main/debug-stateful-test";
pub const VK_CODE_F22: u16 = 133;

/// `KeyboardShortcutsMain.DISABLED_KEYS`. Enter is a standard key but is kept
/// away from shortcuts anyway; F22 is not a standard key at all.
const DISABLED_KEYS: [u16; 2] = [VK_CODE_F22, 13];

/// (keyCode, name, standardName, keyId) for the codes the keyboard pipeline can
/// produce: `STANDARD_KEYBOARD_KEY_CODES` ∩ `nativeInput.VKEY_MAP`. The rest of
/// `definitions.ts`' table (mouse buttons, IME and browser keys) is filtered out
/// by `isStandardKeyboardKeyCode` before anything reads it, so it is not carried
/// over — the two extra rows in [`EXTRA_KEY_ROWS`] are the exception, needed for
/// the `DISABLED_KEYS` lookup.
const KEY_ROWS: &[(u16, &str, &str, &str)] = &[
    (8, "BACK", "BACKSPACE", "Backspace"),
    (9, "TAB", "TAB", "Tab"),
    (12, "CLEAR", "NUMPAD CLEAR", "NumpadClear"),
    (13, "RETURN", "RETURN", "Enter"),
    (16, "SHIFT", "", "Shift"),
    (17, "CONTROL", "", "Control"),
    (18, "MENU", "", "Alt"),
    (19, "PAUSE", "", "Pause"),
    (20, "CAPSLOCK", "CAPS LOCK", "CapsLock"),
    (27, "ESCAPE", "ESCAPE", "Escape"),
    (32, "SPACE", "SPACE", "Space"),
    (33, "PRIOR", "PAGE UP", "PageUp"),
    (34, "NEXT", "PAGE DOWN", "PageDown"),
    (35, "END", "END", "End"),
    (36, "HOME", "HOME", "Home"),
    (37, "LEFT", "LEFT ARROW", "LeftArrow"),
    (38, "UP", "UP ARROW", "UpArrow"),
    (39, "RIGHT", "RIGHT ARROW", "RightArrow"),
    (40, "DOWN", "DOWN ARROW", "DownArrow"),
    (44, "SNAPSHOT", "PRINT SCREEN", "PrintScreen"),
    (45, "INSERT", "INS", "Insert"),
    (46, "DELETE", "DELETE", "Delete"),
    (48, "0", "0", "0"),
    (49, "1", "1", "1"),
    (50, "2", "2", "2"),
    (51, "3", "3", "3"),
    (52, "4", "4", "4"),
    (53, "5", "5", "5"),
    (54, "6", "6", "6"),
    (55, "7", "7", "7"),
    (56, "8", "8", "8"),
    (57, "9", "9", "9"),
    (65, "A", "A", "A"),
    (66, "B", "B", "B"),
    (67, "C", "C", "C"),
    (68, "D", "D", "D"),
    (69, "E", "E", "E"),
    (70, "F", "F", "F"),
    (71, "G", "G", "G"),
    (72, "H", "H", "H"),
    (73, "I", "I", "I"),
    (74, "J", "J", "J"),
    (75, "K", "K", "K"),
    (76, "L", "L", "L"),
    (77, "M", "M", "M"),
    (78, "N", "N", "N"),
    (79, "O", "O", "O"),
    (80, "P", "P", "P"),
    (81, "Q", "Q", "Q"),
    (82, "R", "R", "R"),
    (83, "S", "S", "S"),
    (84, "T", "T", "T"),
    (85, "U", "U", "U"),
    (86, "V", "V", "V"),
    (87, "W", "W", "W"),
    (88, "X", "X", "X"),
    (89, "Y", "Y", "Y"),
    (90, "Z", "Z", "Z"),
    (91, "LWIN", "LEFT META", "LeftMeta"),
    (92, "RWIN", "RIGHT META", "RightMeta"),
    (93, "APPS", "", "Apps"),
    (96, "NUMPAD0", "NUMPAD 0", "Numpad0"),
    (97, "NUMPAD1", "NUMPAD 1", "Numpad1"),
    (98, "NUMPAD2", "NUMPAD 2", "Numpad2"),
    (99, "NUMPAD3", "NUMPAD 3", "Numpad3"),
    (100, "NUMPAD4", "NUMPAD 4", "Numpad4"),
    (101, "NUMPAD5", "NUMPAD 5", "Numpad5"),
    (102, "NUMPAD6", "NUMPAD 6", "Numpad6"),
    (103, "NUMPAD7", "NUMPAD 7", "Numpad7"),
    (104, "NUMPAD8", "NUMPAD 8", "Numpad8"),
    (105, "NUMPAD9", "NUMPAD 9", "Numpad9"),
    (106, "MULTIPLY", "NUMPAD MULTIPLY", "NumpadMultiply"),
    (107, "ADD", "NUMPAD PLUS", "NumpadPlus"),
    (108, "SEPARATOR", "", "Separator"),
    (109, "SUBTRACT", "NUMPAD MINUS", "NumpadMinus"),
    (110, "DECIMAL", "NUMPAD DOT", "NumpadDot"),
    // the shipped addon's mangled `DIVIDE`/`NumpadDivide` spellings, kept because
    // `STANDARD_KEYBOARD_KEY_IDS` contains exactly these strings
    (111, "DIVkeyIdE", "NUMPAD DIVkeyIdE", "NumpadDivkeyIde"),
    (112, "F1", "F1", "F1"),
    (113, "F2", "F2", "F2"),
    (114, "F3", "F3", "F3"),
    (115, "F4", "F4", "F4"),
    (116, "F5", "F5", "F5"),
    (117, "F6", "F6", "F6"),
    (118, "F7", "F7", "F7"),
    (119, "F8", "F8", "F8"),
    (120, "F9", "F9", "F9"),
    (121, "F10", "F10", "F10"),
    (122, "F11", "F11", "F11"),
    (123, "F12", "F12", "F12"),
    (144, "NUMLOCK", "NUM LOCK", "NumLock"),
    (145, "SCROLL", "SCROLL LOCK", "ScrollLock"),
    (160, "LSHIFT", "LEFT SHIFT", "LeftShift"),
    (161, "RSHIFT", "RIGHT SHIFT", "RightShift"),
    (162, "LCONTROL", "LEFT CTRL", "LeftControl"),
    (163, "RCONTROL", "RIGHT CTRL", "RightControl"),
    (164, "LMENU", "LEFT ALT", "LeftAlt"),
    (165, "RMENU", "RIGHT ALT", "RightAlt"),
    (186, "OEM_1", "SEMICOLON", "Semicolon"),
    (187, "OEM_PLUS", "EQUALS", "Equals"),
    (188, "OEM_COMMA", "COMMA", "Comma"),
    (189, "OEM_MINUS", "MINUS", "Minus"),
    (190, "OEM_PERIOD", "DOT", "Dot"),
    (191, "OEM_2", "FORWARD SLASH", "ForwardSlash"),
    (192, "OEM_3", "SECTION", "Section"),
    (219, "OEM_4", "SQUARE BRACKET OPEN", "OpenBracket"),
    (220, "OEM_5", "BACKSLASH", "Backslash"),
    (221, "OEM_6", "SQUARE BRACKET CLOSE", "CloseBracket"),
    (222, "OEM_7", "QUOTE", "Quote"),
    (226, "OEM_102", "BACKTICK", "Backtick"),
];

/// Table rows that are *not* shortcut-usable but still have to resolve, i.e.
/// `VK_F22` for the reserved-key check in `ShortcutRegistry.getRegistration`.
const EXTRA_KEY_ROWS: &[(u16, &str, &str, &str)] = &[(133, "F22", "F22", "F22")];

/// `nativeInput.MODIFIER_KEYS`
const MODIFIER_KEYS: [u16; 11] = [17, 16, 18, 160, 162, 164, 91, 161, 163, 165, 92];

/// `COMMON_MODIFIER_KEY` — the generic (non-side-specific) Alt/Shift/Control
const COMMON_MODIFIER_KEYS: [u16; 3] = [18, 16, 17];

/// `KeyboardShortcutsMain.MODIFIER_READING_ORDER`. `VK_CONTROL (17)` is missing
/// in the original, so its comparator returned `NaN` and V8's stable sort kept it
/// where it was inserted; `None` reproduces that by comparing equal to anything.
const MODIFIER_READING_ORDER: &[(u16, i32)] = &[
    (162, 0),
    (163, 1),
    (16, 2),
    (160, 3),
    (161, 4),
    (18, 5),
    (164, 6),
    (165, 7),
    (91, 8),
    (92, 9),
];

/// `COMMON_MODIFIER_VARIANTS` — releasing the generic modifier clears both sides
const COMMON_MODIFIER_VARIANTS: &[(u16, &[u16])] = &[
    (16, &[16, 160, 161]),
    (17, &[17, 162, 163]),
    (18, &[18, 164, 165]),
];

/// `nativeInput.UNIFIED_KEY_ID`
const UNIFIED_KEY_ID: &[(u16, &str)] = &[
    (16, "Shift"),
    (160, "Shift"),
    (161, "Shift"),
    (162, "Control"),
    (163, "Control"),
    (164, "Alt"),
    (165, "Alt"),
    (91, "Meta"),
    (92, "Meta"),
    (96, "0"),
    (97, "1"),
    (98, "2"),
    (99, "3"),
    (100, "4"),
    (101, "5"),
    (102, "6"),
    (103, "7"),
    (104, "8"),
    (105, "9"),
    (109, "Minus"),
    (110, "Dot"),
];

fn key_row(code: u16) -> Option<&'static (u16, &'static str, &'static str, &'static str)> {
    KEY_ROWS
        .iter()
        .chain(EXTRA_KEY_ROWS)
        .find(|row| row.0 == code)
}

/// `isStandardKeyboardKeyCode` — membership of `KEY_ROWS`
fn is_standard_keyboard_key_code(code: u16) -> bool {
    KEY_ROWS.iter().any(|row| row.0 == code)
}

fn key_id_of(code: u16) -> &'static str {
    key_row(code).map(|row| row.3).unwrap_or("")
}

fn unified_key_id_of(code: u16) -> &'static str {
    UNIFIED_KEY_ID
        .iter()
        .find(|entry| entry.0 == code)
        .map(|entry| entry.1)
        .unwrap_or_else(|| key_id_of(code))
}

fn is_modifier_key(code: u16) -> bool {
    MODIFIER_KEYS.contains(&code)
}

fn is_common_modifier_key(code: u16) -> bool {
    COMMON_MODIFIER_KEYS.contains(&code)
}

fn modifier_reading_order(code: u16) -> Option<i32> {
    MODIFIER_READING_ORDER
        .iter()
        .find(|entry| entry.0 == code)
        .map(|entry| entry.1)
}

fn common_modifier_variants(code: u16) -> &'static [u16] {
    COMMON_MODIFIER_VARIANTS
        .iter()
        .find(|entry| entry.0 == code)
        .map(|entry| entry.1)
        .unwrap_or(&[])
}

/// `STANDARD_KEYBOARD_KEY_IDS`, derived from the same table the codes come from
fn standard_key_ids() -> &'static HashSet<&'static str> {
    static IDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDS.get_or_init(|| KEY_ROWS.iter().map(|row| row.3).collect())
}

/// `isSupportedShortcutId`
fn is_supported_shortcut_id(shortcut_id: &str) -> bool {
    !shortcut_id.is_empty()
        && shortcut_id
            .split('+')
            .all(|key_id| standard_key_ids().contains(key_id))
}

/// the `reservedKeyIds` of `ShortcutRegistry.getRegistration`
fn reserved_key_ids() -> &'static [&'static str] {
    static RESERVED: OnceLock<Vec<&'static str>> = OnceLock::new();
    RESERVED
        .get_or_init(|| DISABLED_KEYS.iter().map(|code| key_id_of(*code)).collect())
        .iter()
        .map(|id| *id)
        // 'F22' and 'Enter' are both in the table, so the strings live on
        .collect::<Vec<_>>()
        .leak()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationType {
    Normal,
    LastActive,
    Stateful,
}

impl RegistrationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::LastActive => "last-active",
            Self::Stateful => "stateful",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShortcutKey {
    pub key_id: String,
    pub key_code: u16,
    pub is_modifier: bool,
}

/// `ShortcutDetails` from `src/shared/shards/keyboard-shortcut`
#[derive(Clone, Debug)]
pub struct ShortcutDetails {
    pub key_codes: Vec<u16>,
    pub keys: Vec<ShortcutKey>,
    pub id: String,
    pub unified_id: String,
    pub pressed: bool,
}

impl ShortcutDetails {
    pub fn to_json(&self) -> Value {
        json!({
            "keyCodes": self.key_codes,
            "keys": self
                .keys
                .iter()
                .map(|key| json!({
                    "keyId": key.key_id,
                    "keyCode": key.key_code,
                    "isModifier": key.is_modifier,
                }))
                .collect::<Vec<_>>(),
            "id": self.id,
            "unifiedId": self.unified_id,
            "pressed": self.pressed,
        })
    }
}

pub type ShortcutCallback = Arc<dyn Fn(ShortcutDetails) + Send + Sync + 'static>;

fn ignore_shortcut() -> ShortcutCallback {
    Arc::new(|_| {})
}

/// `KeyboardShortcutRegistration`, minus the `cb` the IPC handlers strip
#[derive(Clone)]
pub struct Registration {
    pub registration_type: RegistrationType,
    pub target_id: String,
    pub shortcut_id: String,
    cb: ShortcutCallback,
}

impl Registration {
    /// `{ type, targetId, shortcutId }` — the `cb` is dropped, as
    /// `ipc-handlers.ts:16` does
    pub fn to_json(&self) -> Value {
        json!({
            "type": self.registration_type.as_str(),
            "targetId": self.target_id,
            "shortcutId": self.shortcut_id,
        })
    }
}

/// `ShortcutRegistry` — `shortcut-registry.ts`, unchanged semantics:
/// one shortcut id belongs to one target, and re-registering a target for a new
/// shortcut id drops its old one.
#[derive(Default)]
pub struct ShortcutRegistry {
    registrations: RwLock<HashMap<String, Registration>>,
    by_target: RwLock<HashMap<String, String>>,
}

impl ShortcutRegistry {
    /// `register`. The original throws for an unsupported or taken shortcut id —
    /// callers (`game-client/shortcut-controller.ts`, `in-game-send`) catch that
    /// and turn it into their own failure, so the message text is part of the
    /// contract for the settings page's error toast.
    pub fn register(
        &self,
        available: bool,
        target_id: &str,
        shortcut_id: &str,
        registration_type: RegistrationType,
        cb: ShortcutCallback,
    ) -> Result<(), String> {
        if !available {
            log(&format!(
                "Native global shortcuts are unavailable, ignoring shortcut registration: {shortcut_id} ({})",
                registration_type.as_str()
            ));
            return Ok(());
        }

        if !is_supported_shortcut_id(shortcut_id) {
            return Err(format!(
                "Shortcut {shortcut_id} contains unsupported keys"
            ));
        }

        let mut registrations = self.registrations.write().unwrap();
        let mut by_target = self.by_target.write().unwrap();

        if let Some(existing) = registrations.get(shortcut_id) {
            if existing.target_id != target_id {
                return Err(format!(
                    "Shortcut {shortcut_id} is already registered for target {}",
                    existing.target_id
                ));
            }
        }

        if let Some(origin) = by_target.get(target_id).cloned() {
            if origin != shortcut_id {
                registrations.remove(&origin);
            }
        }

        registrations.insert(
            shortcut_id.to_string(),
            Registration {
                registration_type,
                target_id: target_id.to_string(),
                shortcut_id: shortcut_id.to_string(),
                cb,
            },
        );
        by_target.insert(target_id.to_string(), shortcut_id.to_string());

        log(&format!(
            "Register shortcut {shortcut_id} for target {target_id} ({})",
            registration_type.as_str()
        ));
        Ok(())
    }

    pub fn unregister(&self, shortcut_id: &str) -> bool {
        let mut registrations = self.registrations.write().unwrap();
        let Some(removed) = registrations.remove(shortcut_id) else {
            return false;
        };
        self.by_target.write().unwrap().remove(&removed.target_id);
        log(&format!(
            "Unregister shortcut {shortcut_id} for target {}",
            removed.target_id
        ));
        true
    }

    pub fn unregister_by_target_id(&self, target_id: &str) -> bool {
        let Some(shortcut_id) = self.by_target.write().unwrap().remove(target_id) else {
            return false;
        };
        self.registrations.write().unwrap().remove(&shortcut_id);
        log(&format!(
            "Unregister shortcut {shortcut_id} for target {target_id}"
        ));
        true
    }

    /// The pseudo registration answering for reserved/unsupported shortcut ids,
    /// so the settings page can say "occupied by riftflow-disabled-keys".
    fn disabled_keys_registration(shortcut_id: &str) -> Registration {
        Registration {
            registration_type: RegistrationType::Normal,
            target_id: DISABLED_KEYS_TARGET_ID.to_string(),
            shortcut_id: shortcut_id.to_string(),
            cb: ignore_shortcut(),
        }
    }

    /// `getRegistration` — note the availability gate, the reserved-key check and
    /// the fact that an unsupported id answers with the disabled-keys pseudo
    /// registration rather than `null`.
    pub fn get_registration(&self, available: bool, shortcut_id: &str) -> Option<Registration> {
        if !available {
            return None;
        }

        if reserved_key_ids()
            .iter()
            .any(|key_id| shortcut_id.contains(key_id))
            || !is_supported_shortcut_id(shortcut_id)
        {
            return Some(Self::disabled_keys_registration(shortcut_id));
        }

        self.registrations.read().unwrap().get(shortcut_id).cloned()
    }

    pub fn get_registration_by_target_id(&self, target_id: &str) -> Option<Registration> {
        if target_id == DISABLED_KEYS_TARGET_ID {
            return Some(Self::disabled_keys_registration(""));
        }

        let shortcut_id = self.by_target.read().unwrap().get(target_id).cloned();
        shortcut_id.and_then(|id| self.registrations.read().unwrap().get(&id).cloned())
    }

    /// `getActiveRegistration` — the plain map lookup the dispatcher uses, with
    /// neither the availability gate nor the reserved-key substitution.
    fn get_active_registration(&self, shortcut_id: &str) -> Option<Registration> {
        self.registrations.read().unwrap().get(shortcut_id).cloned()
    }

    pub fn clear(&self) {
        self.registrations.write().unwrap().clear();
        self.by_target.write().unwrap().clear();
    }
}

/// `nativeInput`'s `getKeyStates()` entry (`NativeKeyState`)
#[derive(Clone, Copy, Debug)]
pub struct NativeKeyState {
    pub vk_code: u16,
    pub pressed: bool,
    pub scan_code: u16,
}

/// the addon's `KeyEvent`, minus the definition fields the shard re-reads
#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub key_code: u16,
    pub is_down: bool,
    pub is_modifier: bool,
    pub is_common_modifier: bool,
}

type KeyStatesProvider = Arc<dyn Fn() -> Vec<NativeKeyState> + Send + Sync + 'static>;

/// The sets are insertion-ordered vectors, because `Array.from(set)` in the
/// original keeps insertion order and the modifier sort is stable.
#[derive(Default)]
struct PressedState {
    other: Vec<u16>,
    modifiers: Vec<u16>,
    last_active: Vec<u16>,
    active_stateful: Vec<u16>,
}

impl PressedState {
    fn has_pressed_keys(&self) -> bool {
        !self.modifiers.is_empty() || !self.other.is_empty()
    }

    fn insert(&mut self, set: &mut Vec<u16>, code: u16) {
        if !set.contains(&code) {
            set.push(code);
        }
    }
}

/// Side effects collected while the state lock is held and run after it drops,
/// so a shard's callback can call back into this shard freely.
enum Effect {
    Callback(ShortcutCallback, ShortcutDetails),
    SendEvent(&'static str, ShortcutDetails),
}

/// `KeyboardShortcutsMain`
pub struct KeyboardShortcutsMain {
    ipc: Arc<RiftFlowIpcMain>,
    registry: ShortcutRegistry,
    pressed: Mutex<PressedState>,
    key_states: KeyStatesProvider,
    available: AtomicBool,
}

impl KeyboardShortcutsMain {
    pub fn new(
        ipc: Arc<RiftFlowIpcMain>,
        available: bool,
        key_states: KeyStatesProvider,
    ) -> Self {
        Self {
            ipc,
            registry: ShortcutRegistry::default(),
            pressed: Mutex::new(PressedState::default()),
            key_states,
            available: AtomicBool::new(available),
        }
    }

    /// `NATIVE_SUPPORT.nativeInput.available` for this shard: the install result
    /// AND the hook thread still pumping, so a message loop that has since died
    /// is not reported as a listening hook.
    pub fn available(&self) -> bool {
        self.available.load(Ordering::Relaxed) && hook::is_live()
    }

    pub fn registry(&self) -> &ShortcutRegistry {
        &self.registry
    }

    // ---- public API other shards use (grep: `keyboardShortcuts.register(`) ----

    pub fn register(
        &self,
        target_id: &str,
        shortcut_id: &str,
        registration_type: RegistrationType,
        cb: impl Fn(ShortcutDetails) + Send + Sync + 'static,
    ) -> Result<(), String> {
        self.registry.register(
            self.available(),
            target_id,
            shortcut_id,
            registration_type,
            Arc::new(cb),
        )
    }

    pub fn unregister(&self, shortcut_id: &str) -> bool {
        self.registry.unregister(shortcut_id)
    }

    pub fn unregister_by_target_id(&self, target_id: &str) -> bool {
        self.registry.unregister_by_target_id(target_id)
    }

    pub fn get_registration(&self, shortcut_id: &str) -> Option<Registration> {
        self.registry.get_registration(self.available(), shortcut_id)
    }

    pub fn get_registration_by_target_id(&self, target_id: &str) -> Option<Registration> {
        self.registry.get_registration_by_target_id(target_id)
    }

    // ---- IPC-facing surface ----

    /// `KeyboardShortcutsMain.setDebugStatefulShortcut`
    pub fn set_debug_stateful_shortcut(&self, shortcut_id: Option<&str>) -> Result<Value, String> {
        self.unregister_by_target_id(DEBUG_STATEFUL_TEST_TARGET_ID);

        let Some(shortcut_id) = shortcut_id else {
            return Ok(Value::Null);
        };

        self.register(DEBUG_STATEFUL_TEST_TARGET_ID, shortcut_id, RegistrationType::Stateful, |_| {})?;

        // quirk kept: the original answers with this object even when the registry
        // dropped the request because native input is unavailable — the caller
        // sees it through `getRegistrationByTargetId`, which does report `null`
        Ok(json!({
            "type": "stateful",
            "targetId": DEBUG_STATEFUL_TEST_TARGET_ID,
            "shortcutId": shortcut_id,
        }))
    }

    /// `KeyboardShortcutsMain._getInternalVars`. The original hands back live
    /// `Set` objects; Electron's structured clone kept them, this wire is JSON, so
    /// they arrive as the sorted arrays the debug views read.
    pub fn internal_vars(&self) -> Value {
        let state = self.pressed.lock().unwrap();
        json!({
            "_pressedOtherKeys": sorted_codes(&state.other),
            "_pressedModifierKeys": sorted_codes(&state.modifiers),
            "_lastActiveShortcut": state.last_active,
            "_activeStatefulShortcut": state.active_stateful,
        })
    }

    /// `KeyboardShortcutsMain.getDebugState`
    pub fn get_debug_state(&self) -> Value {
        if !self.available() {
            return json!({
                "available": false,
                "keyStates": [],
                "pressedOtherKeys": [],
                "pressedModifierKeys": [],
                "activeShortcut": null,
                "lastActiveShortcut": null,
                "activeStatefulShortcut": null,
            });
        }

        let states = (self.key_states)();
        let native: HashMap<u16, NativeKeyState> =
            states.iter().map(|state| (state.vk_code, *state)).collect();

        let state = self.pressed.lock().unwrap();
        let mut key_states = Vec::new();

        for row in KEY_ROWS {
            let code = row.0;
            let native_state = native.get(&code);
            let mut entry = json!({
                "keyCode": code,
                "keyId": row.3,
                "unifiedKeyId": unified_key_id_of(code),
                "name": row.1,
                "standardName": row.2,
                "isModifier": is_modifier_key(code),
                "pressed": native_state.map(|s| s.pressed).unwrap_or(false)
                    || state.modifiers.contains(&code)
                    || state.other.contains(&code),
            });
            if let Some(native_state) = native_state {
                entry["scanCode"] = json!(native_state.scan_code);
            }
            key_states.push(entry);
        }

        // the iteration order of `Object.entries` is numeric here, but the sort
        // the original runs is kept for the rows a table change could reorder
        key_states.sort_by_key(|entry| entry["keyCode"].as_i64().unwrap_or_default());

        let active_shortcut = self
            .build_shortcut_details(&state.sorted_pressed_key_codes(), true)
            .to_json();
        let last_active = self.build_shortcut_details_or_null(&state.last_active, false);
        let active_stateful = self.build_shortcut_details_or_null(&state.active_stateful, true);
        let pressed_other = sorted_codes(&state.other);
        let pressed_modifiers = sorted_codes(&state.modifiers);
        drop(state);

        json!({
            "available": true,
            "keyStates": key_states,
            "pressedOtherKeys": pressed_other,
            "pressedModifierKeys": pressed_modifiers,
            "activeShortcut": active_shortcut,
            "lastActiveShortcut": last_active,
            "activeStatefulShortcut": active_stateful,
        })
    }

    // ---- key pipeline ----

    /// `_processNativeKeyEvent`
    pub fn process_key_event(&self, event: KeyEvent) {
        self.process_key_event_from(event, true);
    }

    /// A key event the *renderer* observed rather than the hook.
    ///
    /// Measured on a live session: our `WH_KEYBOARD_LL` procedure is never called for a
    /// keystroke whose target window belongs to this process — the page reported
    /// `keydown KeyJ:74` while the hook logged nothing, and installing the hook on the
    /// window-owning thread changed nothing — so the shortcut dialog could only record
    /// keys while RiftFlow was *not* in the foreground, the opposite of where it is used.
    /// The DOM already has the whole stream, with `location` telling left modifier from
    /// right, so it is fed in here instead.
    ///
    /// Two differences from the hook path: the reconcile step exists to recover from
    /// missed hook events and would wipe a DOM-held modifier out of the chord (the hook
    /// never marked it down), and anything the hook reported within 60 ms is dropped so
    /// one press can never fire a registered shortcut twice.
    pub fn process_external_key_event(&self, key_code: u16, is_down: bool) {
        const DEDUPE_WINDOW_MS: u64 = 60;

        #[cfg(windows)]
        if hook::seen(key_code, is_down).saturating_add(DEDUPE_WINDOW_MS) > hook::now() {
            return;
        }

        #[cfg(not(windows))]
        let _ = DEDUPE_WINDOW_MS;

        self.process_key_event_from(
            KeyEvent {
                key_code,
                is_down,
                is_modifier: is_modifier_key(key_code),
                is_common_modifier: is_common_modifier_key(key_code),
            },
            false,
        );
    }

    fn process_key_event_from(&self, event: KeyEvent, reconcile: bool) {
        if !is_standard_keyboard_key_code(event.key_code) {
            return;
        }

        let mut effects = Vec::new();
        {
            let mut state = self.pressed.lock().unwrap();
            if reconcile {
                self.reconcile_pressed_state(&mut state, &mut effects);
            }

            // VK_PACKET, which the IME emits on every Unicode character
            if event.key_code == 231 {
                // (kept after the reconcile, as in the original)
            } else if event.is_common_modifier {
                self.process_common_modifier_key(&mut state, &mut effects, &event);
            } else if event.is_modifier {
                self.process_modifier_key(&mut state, &mut effects, &event);
            } else if event.is_down {
                self.process_non_modifier_key_down(&mut state, &mut effects, event.key_code);
            } else {
                self.process_non_modifier_key_up(&mut state, &mut effects, event.key_code);
            }

            self.emit_last_active_shortcut_if_needed(&mut state, &mut effects);
        }

        self.run_effects(effects);
    }

    fn run_effects(&self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Callback(cb, details) => cb(details),
                Effect::SendEvent(name, details) => {
                    trace(&format!(
                        "send event {name} id={:?} keyCodes={:?} available={}",
                        details.id,
                        details.key_codes,
                        self.available()
                    ));
                    self.ipc
                        .send_event(NAMESPACE, name, &[details.to_json()]);
                }
            }
        }
    }

    fn build_shortcut_details(&self, key_codes: &[u16], pressed: bool) -> ShortcutDetails {
        if !self.available() {
            return ShortcutDetails {
                key_codes: key_codes.to_vec(),
                keys: Vec::new(),
                id: String::new(),
                unified_id: String::new(),
                pressed,
            };
        }

        let keys: Vec<ShortcutKey> = key_codes
            .iter()
            .map(|code| ShortcutKey {
                key_id: key_id_of(*code).to_string(),
                key_code: *code,
                is_modifier: is_modifier_key(*code),
            })
            .collect();

        let id = keys
            .iter()
            .map(|key| key.key_id.as_str())
            .collect::<Vec<_>>()
            .join("+");

        // `UNIFIED_KEY_ID[k] || VKEY_MAP[k].keyId`, then de-duplicated: the two
        // Ctrl sides collapse into one `Control`
        let mut seen: Vec<&str> = Vec::new();
        for code in key_codes {
            let id = unified_key_id_of(*code);
            if !seen.contains(&id) {
                seen.push(id);
            }
        }

        ShortcutDetails {
            key_codes: key_codes.to_vec(),
            keys,
            id,
            unified_id: seen.join("+"),
            pressed,
        }
    }

    /// `_buildShortcutDetailsOrNull`
    fn build_shortcut_details_or_null(&self, key_codes: &[u16], pressed: bool) -> Value {
        if key_codes.is_empty() {
            Value::Null
        } else {
            self.build_shortcut_details(key_codes, pressed).to_json()
        }
    }

    /// `_releaseActiveStatefulShortcut`. `stateful-shortcut-pressed` /
    /// `stateful-shortcut-released` are `events`-only in the original — they never
    /// reach the IPC wire — so only the registration callback is dispatched here.
    fn release_active_stateful_shortcut(&self, state: &mut PressedState, effects: &mut Vec<Effect>) {
        if state.active_stateful.is_empty() {
            return;
        }

        let details = self.build_shortcut_details(&state.active_stateful.clone(), false);
        if let Some(registration) = self.registry.get_active_registration(&details.id.clone()) {
            if registration.registration_type == RegistrationType::Stateful {
                effects.push(Effect::Callback(registration.cb.clone(), details));
            }
        }
        state.active_stateful.clear();
    }

    /// `_reconcilePressedStateFromNative`. Intentionally not a watchdog, just
    /// like the original: stale state is cleaned at the next real key event.
    fn reconcile_pressed_state(&self, state: &mut PressedState, effects: &mut Vec<Effect>) {
        if !self.available() || !state.has_pressed_keys() {
            return;
        }

        let pressed: HashSet<u16> = (self.key_states)()
            .into_iter()
            .filter(|entry| entry.pressed)
            .map(|entry| entry.vk_code)
            .collect();

        state.modifiers.retain(|code| pressed.contains(code));
        state.other.retain(|code| pressed.contains(code));

        if !state.active_stateful.is_empty()
            && state
                .active_stateful
                .iter()
                .any(|code| !pressed.contains(code))
        {
            self.release_active_stateful_shortcut(state, effects);
        }

        self.emit_last_active_shortcut_if_needed(state, effects);
    }

    /// `_emitLastActiveShortcutIfNeeded`
    fn emit_last_active_shortcut_if_needed(&self, state: &mut PressedState, effects: &mut Vec<Effect>) {
        if !(state.modifiers.is_empty()
            && state.other.is_empty()
            && !state.last_active.is_empty())
        {
            return;
        }

        let details = self.build_shortcut_details(&state.last_active.clone(), false);
        effects.push(Effect::SendEvent("last-active-shortcut", details.clone()));
        if let Some(registration) = self.registry.get_active_registration(&details.id) {
            if registration.registration_type == RegistrationType::LastActive {
                effects.push(Effect::Callback(registration.cb, details));
            }
        }
        state.last_active.clear();
    }

    /// `_processModifierKey`
    fn process_modifier_key(
        &self,
        state: &mut PressedState,
        effects: &mut Vec<Effect>,
        event: &KeyEvent,
    ) {
        let code = event.key_code;
        let held = state.modifiers.contains(&code);

        if held == event.is_down {
            if !event.is_down && state.active_stateful.contains(&code) {
                self.release_active_stateful_shortcut(state, effects);
            }
            return;
        }

        if event.is_down {
            state.modifiers.push(code);
        } else {
            state.modifiers.retain(|existing| *existing != code);
            if state.active_stateful.contains(&code) {
                self.release_active_stateful_shortcut(state, effects);
            }
        }
    }

    /// `_processCommonModifierKey`
    fn process_common_modifier_key(
        &self,
        state: &mut PressedState,
        effects: &mut Vec<Effect>,
        event: &KeyEvent,
    ) {
        if event.is_down {
            return;
        }

        let variants = common_modifier_variants(event.key_code);
        let variants = if variants.is_empty() {
            std::slice::from_ref(&event.key_code)
        } else {
            variants
        };

        let mut should_release = false;
        for code in variants {
            state.modifiers.retain(|existing| *existing != *code);
            if state.active_stateful.contains(code) {
                should_release = true;
            }
        }

        if should_release {
            self.release_active_stateful_shortcut(state, effects);
        }
    }

    /// `_processNonModifierKeyDown`
    fn process_non_modifier_key_down(
        &self,
        state: &mut PressedState,
        effects: &mut Vec<Effect>,
        code: u16,
    ) {
        if state.other.contains(&code) {
            return;
        }

        state.other.push(code);

        let mut modifiers = state.modifiers.clone();
        sort_modifiers(&mut modifiers);
        let mut key_codes = modifiers;
        key_codes.push(code);

        let details = self.build_shortcut_details(&key_codes, true);
        state.last_active = key_codes.clone();

        if let Some(registration) = self.registry.get_active_registration(&details.id) {
            match registration.registration_type {
                RegistrationType::Normal => {
                    effects.push(Effect::Callback(registration.cb, details.clone()))
                }
                RegistrationType::Stateful => {
                    // a changed combination releases the previous one first
                    if state.active_stateful != key_codes {
                        if !state.active_stateful.is_empty() {
                            self.release_active_stateful_shortcut(state, effects);
                        }
                        state.active_stateful = key_codes.clone();
                        effects.push(Effect::Callback(registration.cb, details.clone()));
                    }
                }
                RegistrationType::LastActive => {}
            }
        }

        // the shard's own `events.emit('shortcut')` has no Rust listeners; the
        // renderer mirror is the IPC event the original sends at the very end
        effects.push(Effect::SendEvent("shortcut", details));
    }

    /// `_processNonModifierKeyUp`
    fn process_non_modifier_key_up(
        &self,
        state: &mut PressedState,
        effects: &mut Vec<Effect>,
        code: u16,
    ) {
        state.other.retain(|existing| *existing != code);

        if !state.active_stateful.is_empty()
            && state.active_stateful.last().copied() == Some(code)
        {
            self.release_active_stateful_shortcut(state, effects);
        }
    }
}

impl PressedState {
    /// `_getSortedPressedKeyCodes`
    fn sorted_pressed_key_codes(&self) -> Vec<u16> {
        let mut modifiers = self.modifiers.clone();
        sort_modifiers(&mut modifiers);
        let mut other = sorted_codes(&self.other);
        modifiers.append(&mut other);
        modifiers
    }
}

fn sorted_codes(codes: &[u16]) -> Vec<u16> {
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted
}

/// `toSorted((a, b) => MODIFIER_READING_ORDER[a] - MODIFIER_READING_ORDER[b])`.
/// `slice::sort_by` is stable, which is what makes the missing `VK_CONTROL`
/// entry behave like V8's `NaN` comparator (it keeps insertion order).
fn sort_modifiers(codes: &mut Vec<u16>) {
    codes.sort_by(|a, b| match (modifier_reading_order(*a), modifier_reading_order(*b)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => std::cmp::Ordering::Equal,
    });
}

/// Shard-level lines go through `logger`, so they carry a timestamp and land in the
/// log file. That matters because an elevated instance exposes no CDP: the file is the
/// only window into whether a key ever reached the pipeline.
fn log(message: &str) {
    crate::logger::log(NAMESPACE, crate::logger::Level::Info, message);
}

/// `RIFTFLOW_IPC_TRACE`'s counterpart for this shard: one line per raw hook callback
/// and per shortcut decision, so a keypress can be traced without a renderer.
fn trace_enabled() -> bool {
    std::env::var("RIFTFLOW_HOTKEY_TRACE").is_ok()
}

fn trace(message: &str) {
    if trace_enabled() {
        log(message);
    }
}

/// The live shard, for the hook thread and for `app_common::native_support`.
static INSTANCE: OnceLock<Arc<KeyboardShortcutsMain>> = OnceLock::new();

/// `NATIVE_SUPPORT.nativeInput.available`, readable without a shard reference.
pub fn input_available() -> bool {
    INSTANCE.get().map(|shard| shard.available()).unwrap_or(false)
}

/// The shard other modules ask for a shortcut from
/// (`manager.getShard(KeyboardShortcutsMain.id).register(...)` equivalent).
pub fn instance() -> Option<Arc<KeyboardShortcutsMain>> {
    INSTANCE.get().cloned()
}

/// Builds the shard, installs the hook when the original's condition holds, and
/// registers the five `keyboard-shortcuts-main` methods. Call once at boot.
pub fn start(ipc: &Arc<RiftFlowIpcMain>) -> anyhow::Result<Arc<KeyboardShortcutsMain>> {
    let (sender, receiver) = unbounded_channel::<KeyEvent>();

    // the hook is attempted on every win32 run; `install` says so honestly when
    // Windows refuses it, and nothing else gates availability
    let wants_hook = cfg!(windows);
    let installed = wants_hook && hook::install(sender);

    let this = Arc::new(KeyboardShortcutsMain::new(
        ipc.clone(),
        installed,
        Arc::new(hook::key_states),
    ));
    let _ = INSTANCE.set(this.clone());

    register_ipc(&this)?;

    if installed {
        log("Listening for key events");
        let consumer = this.clone();
        tauri::async_runtime::spawn(run_pipeline(receiver, consumer));
    } else {
        log(
            "Native global shortcuts are unavailable, shortcut registration will be ignored",
        );
    }

    Ok(this)
}

async fn run_pipeline(mut receiver: UnboundedReceiver<KeyEvent>, this: Arc<KeyboardShortcutsMain>) {
    while let Some(event) = receiver.recv().await {
        trace(&format!(
            "pipeline event vk={} {}",
            event.key_code,
            if event.is_down { "down" } else { "up" }
        ));
        this.process_key_event(event);
    }
}

/// `KeyboardShortcutsIpcHandlers.register`
pub fn register_ipc(this: &Arc<KeyboardShortcutsMain>) -> anyhow::Result<()> {
    let shard = this.clone();
    this.ipc.on_call(NAMESPACE, "getRegistration", riftflow_ipc::handler(move |_ctx, args| {
        let shard = shard.clone();
        async move {
            let shortcut_id = arg_str(&args, 0)?;
            Ok(shard
                .get_registration(shortcut_id)
                .map(|registration| registration.to_json())
                .unwrap_or(Value::Null))
        }
    }))?;

    let shard = this.clone();
    this.ipc
        .on_call(NAMESPACE, "getRegistrationByTargetId", riftflow_ipc::handler(
            move |_ctx, args| {
                let shard = shard.clone();
                async move {
                    let target_id = arg_str(&args, 0)?;
                    Ok(shard
                        .get_registration_by_target_id(target_id)
                        .map(|registration| registration.to_json())
                        .unwrap_or(Value::Null))
                }
            },
        ))?;

    let shard = this.clone();
    this.ipc.on_call(NAMESPACE, "getDebugState", riftflow_ipc::handler(move |_ctx, _args| {
        let shard = shard.clone();
        async move { Ok(shard.get_debug_state()) }
    }))?;

    let shard = this.clone();
    this.ipc
        .on_call(NAMESPACE, "setDebugStatefulShortcut", riftflow_ipc::handler(move |_ctx, args| {
            let shard = shard.clone();
            async move {
                let shortcut_id = match args.first() {
                    None | Some(Value::Null) => None,
                    Some(Value::String(shortcut_id)) => Some(shortcut_id.as_str()),
                    Some(_) => {
                        return Err(IpcError::plain("argument 0 should be a string or null"));
                    }
                };

                shard
                    .set_debug_stateful_shortcut(shortcut_id)
                    .map_err(IpcError::plain)
            }
        }))?;

    let shard = this.clone();
    this.ipc.on_call(NAMESPACE, "_getInternalVars", riftflow_ipc::handler(move |_ctx, _args| {
        let shard = shard.clone();
        async move { Ok(shard.internal_vars()) }
    }))?;

    Ok(())
}

fn arg_str(args: &[Value], index: usize) -> Result<&str, IpcError> {
    args.get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| IpcError::plain(format!("argument {index} should be a string")))
}

/// Replaces `native/win32-x64/src/input/input.cc`: a `WH_KEYBOARD_LL` hook on its
/// own message-pumping thread, the 256-slot pressed-state array, and
/// `MapVirtualKey` for the scan codes `getDebugState` shows.
mod hook {
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::sync::mpsc::UnboundedSender;

    use super::KeyEvent;

    /// `static`, not `const`: a `const` array is re-created at every use site, so
    /// the hook proc would write one copy while `key_states()` read another and
    /// `_reconcilePressedStateFromNative` would clear every held modifier.
    #[cfg(windows)]
    static KEY_STATES: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

    static SENDER: std::sync::OnceLock<UnboundedSender<KeyEvent>> = std::sync::OnceLock::new();

    /// Whether a hook thread is currently pumping, i.e. what a repeat call to
    /// [`install`] reports instead of inventing a second answer.
    #[cfg(windows)]
    static HOOK_LIVE: AtomicBool = AtomicBool::new(false);

    /// `NATIVE_SUPPORT.nativeInput.available`'s hook term, kept for the shard's
    /// own availability so a message loop that dies mid-session is not reported
    /// as still listening.
    #[cfg(windows)]
    pub fn is_live() -> bool {
        HOOK_LIVE.load(Ordering::Relaxed)
    }

    #[cfg(not(windows))]
    pub fn is_live() -> bool {
        false
    }

    /// Wall-clock milliseconds of the last hook delivery, per (vk, direction). The
    /// renderer forwards its own key stream as a second source (see
    /// [`KeyboardShortcutsMain::process_external_key_event`]), and this is what lets the
    /// hook stay authoritative so one physical press can never fire a registered
    /// shortcut twice.
    #[cfg(windows)]
    static HOOK_SEEN_MS: [std::sync::atomic::AtomicU64; 512] =
        [const { std::sync::atomic::AtomicU64::new(0) }; 512];

    // Monotonic, and one call inside the hook proc — `LowLevelHooksTimeout` is why
    // nothing slower than this belongs in there.
    #[cfg(windows)]
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount64() -> u64;
    }

    #[cfg(windows)]
    fn now_ms() -> u64 {
        unsafe { GetTickCount64() }
    }

    #[cfg(not(windows))]
    fn now_ms() -> u64 {
        0
    }

    /// When the hook last reported this exact key transition; `0` means never.
    pub fn seen(key_code: u16, is_down: bool) -> u64 {
        #[cfg(windows)]
        {
            HOOK_SEEN_MS[(key_code as usize % 256) * 2 + usize::from(!is_down)]
                .load(Ordering::Relaxed)
        }

        #[cfg(not(windows))]
        {
            let _ = (key_code, is_down);
            0
        }
    }

    /// The same clock [`seen`] is stamped with.
    #[cfg(windows)]
    pub fn now() -> u64 {
        now_ms()
    }

    fn note_seen(key_code: u32, is_down: bool) {
        #[cfg(windows)]
        {
            HOOK_SEEN_MS[(key_code as usize % 256) * 2 + usize::from(!is_down)]
                .store(now_ms(), Ordering::Relaxed);
        }

        #[cfg(not(windows))]
        let _ = (key_code, is_down);
    }

    /// Installs the hook and reports whether it is live. Must be called once:
    /// the event channel is a process-wide singleton, as the addon was.
    #[cfg(windows)]
    pub fn install(sender: UnboundedSender<KeyEvent>) -> bool {
        use std::ffi::c_void;
        use std::ptr::null_mut;

        // LPARAM is pointer-sized and signed (LONG_PTR)
        type raw_isize = isize;
        use std::sync::mpsc::channel;

        const WH_KEYBOARD_LL: i32 = 13;
        const WM_KEYDOWN: usize = 0x0100;
        const WM_SYSKEYDOWN: usize = 0x0104;
        const PM_NOREMOVE: u32 = 0x0003;
        const WM_USER: u32 = 0x0400;

        #[repr(C)]
        struct KbdLlHookStruct {
            vk_code: u32,
            scan_code: u32,
            flags: u32,
            time: u32,
            extra_info: *mut c_void,
        }

        #[repr(C)]
        #[derive(Default)]
        struct WinMsg {
            window: *mut c_void,
            message: u32,
            w_param: usize,
            l_param: raw_isize,
            time: u32,
            point_x: i32,
            point_y: i32,
        }

        type HookProc = unsafe extern "system" fn(code: i32, w_param: usize, l_param: raw_isize) -> raw_isize;

        #[link(name = "user32")]
        extern "system" {
            fn SetWindowsHookExW(
                id_hook: i32,
                hook_proc: HookProc,
                module: *mut c_void,
                thread_id: u32,
            ) -> *mut c_void;
            fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: raw_isize) -> raw_isize;
            fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
            fn GetMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32) -> i32;
            fn PeekMessageW(msg: *mut WinMsg, window: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
            fn TranslateMessage(msg: *const WinMsg) -> i32;
            fn DispatchMessageW(msg: *const WinMsg) -> raw_isize;
            fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
            fn PostThreadMessageW(thread_id: u32, message: u32, w_param: usize, l_param: raw_isize) -> i32;
        }

        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut c_void;
            fn GetLastError() -> u32;
            fn GetCurrentThreadId() -> u32;
        }

        const LLKHF_UP: u32 = 0x80;

        unsafe extern "system" fn keyboard_proc(
            code: i32,
            w_param: usize,
            l_param: raw_isize,
        ) -> raw_isize {
            // HC_ACTION
            if code == 0 && l_param != 0 {
                let info = &*(l_param as *const KbdLlHookStruct);
                let key = info.vk_code;
                let is_down = (info.flags & LLKHF_UP) == 0;
                let _ = (w_param, WM_KEYDOWN, WM_SYSKEYDOWN);
                super::trace(&format!(
                    "hook callback vk={key} {}",
                    if is_down { "down" } else { "up" }
                ));

                if let Ok(slot) = usize::try_from(key) {
                    if slot < KEY_STATES.len() {
                        KEY_STATES[slot].store(is_down, Ordering::Relaxed);
                        note_seen(key, is_down);
                    }
                }

                if let Some(sender) = SENDER.get() {
                    if let Ok(key_code) = u16::try_from(key) {
                        let _ = sender.send(KeyEvent {
                            key_code,
                            is_down,
                            is_modifier: super::is_modifier_key(key_code),
                            is_common_modifier: super::is_common_modifier_key(key_code),
                        });
                    }
                }
            }

            // Windows 7 and later ignore the handle parameter
            CallNextHookEx(null_mut(), code, w_param, l_param)
        }

        if SENDER.set(sender).is_err() {
            // the event channel is a process-wide singleton, so a second call
            // answers with the first attempt's outcome instead of a fresh `true`
            return HOOK_LIVE.load(Ordering::Relaxed);
        }

        let (ready_sender, ready) = channel::<Result<(), u32>>();

        let spawned = std::thread::Builder::new()
            .name("riftflow-keyboard-hook".to_string())
            .spawn(move || unsafe {
                let mut msg = WinMsg::default();
                // create this thread's queue before installing, like input.cc:84
                PeekMessageW(
                    &mut msg,
                    null_mut(),
                    WM_USER,
                    WM_USER,
                    PM_NOREMOVE,
                );

                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    keyboard_proc,
                    GetModuleHandleW(std::ptr::null()),
                    0,
                );

                if hook.is_null() {
                    let _ = ready_sender.send(Err(GetLastError()));
                    return;
                }

                HOOK_LIVE.store(true, Ordering::Relaxed);
                let _ = ready_sender.send(Ok(()));

                // Wake this thread once after the install, so its first
                // `GetMessageW` returns and the queue is exercised from the very
                // start of the session.
                PostThreadMessageW(GetCurrentThreadId(), WM_USER + 1, 0, 0);

                let mut pumped = 0usize;
                loop {
                    let got = GetMessageW(&mut msg, null_mut(), 0, 0);
                    if got <= 0 {
                        let error = GetLastError();
                        super::log(&format!(
                            "keyboard hook message loop stopped after {pumped} messages (GetLastError {error})"
                        ));
                        break;
                    }
                    pumped += 1;
                    if pumped == 1 {
                        super::trace("keyboard hook thread pumped its first message");
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                // the loop only ends when Windows refuses the pump, so from here
                // on no key event reaches the shard any more
                HOOK_LIVE.store(false, Ordering::Relaxed);
                UnhookWindowsHookEx(hook);
            });

        match spawned {
            Err(error) => {
                super::log(&format!("keyboard hook thread failed to start: {error}"));
                false
            }
            Ok(_) => match ready.recv() {
                Ok(Ok(())) => true,
                Ok(Err(error)) => {
                    super::log(&format!(
                        "SetWindowsHookExW(WH_KEYBOARD_LL) failed with Windows error {error}"
                    ));
                    false
                }
                Err(_) => {
                    super::log("keyboard hook thread exited before reporting an install result");
                    false
                }
            },
        }
    }

    #[cfg(not(windows))]
    pub fn install(_sender: UnboundedSender<KeyEvent>) -> bool {
        false
    }

    /// `getKeyStates()` — all 256 virtual keys, pressed flag from the hook's own
    /// tracking and scan code from `MapVirtualKey(vk, MAPVK_VK_TO_VSC)`.
    #[cfg(windows)]
    pub fn key_states() -> Vec<super::NativeKeyState> {
        (0..256u16)
            .map(|vk| super::NativeKeyState {
                vk_code: vk,
                pressed: KEY_STATES[usize::from(vk)].load(Ordering::Relaxed),
                // kept for the F22-style rows the debug page still labels
                // MAPVK_VK_TO_VSC is 0
                scan_code: unsafe { MapVirtualKeyScan(u32::from(vk), 0) } as u16,
            })
            .collect()
    }

    #[cfg(windows)]
    #[link(name = "user32")]
    extern "system" {
        #[link_name = "MapVirtualKeyW"]
        fn MapVirtualKeyScan(code: u32, map_type: u32) -> u32;
    }

    #[cfg(not(windows))]
    pub fn key_states() -> Vec<super::NativeKeyState> {
        Vec::new()
    }
}
