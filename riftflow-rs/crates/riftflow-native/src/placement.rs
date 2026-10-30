//! `getLeagueClientWindowPlacementInfo` from `native/win32-x64/src/tools/tools.cc`.
//!
//! The League client's top-level window is `RCLIENT` / `League of Legends`, and the
//! addon reads `GetWindowPlacement`'s *normal* rect divided by the window's DPI, so
//! the numbers line up with Electron's DIP-based bounds instead of raw pixels.

use std::ffi::c_void;

const APPLICATION_CLASS_NAME: &str = "RCLIENT";
const APPLICATION_NAME: &str = "League of Legends";
/// `tools.h:27` — the client's CEF child window, which has to be resized with the
/// top-level window or the client renders into the old rectangle.
const CEF_WINDOW_NAME: &str = "CefBrowserWindow";

/// `FixWindowMethodA`'s defaults when the caller passes no config.
const DEFAULT_BASE_WIDTH: i32 = 1280;
const DEFAULT_BASE_HEIGHT: i32 = 720;

/// Exactly the object the addon hands to JavaScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UxPlacement {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub width: i32,
    pub height: i32,
    pub shown_state: i32,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub is_normal: bool,
}

#[cfg(windows)]
#[repr(C)]
struct WindowPlacement {
    length: u32,
    flags: u32,
    show_cmd: u32,
    pt_min_x: i32,
    pt_min_y: i32,
    pt_max_x: i32,
    pt_max_y: i32,
    rc_normal_left: i32,
    rc_normal_top: i32,
    rc_normal_right: i32,
    rc_normal_bottom: i32,
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> *mut c_void;
    fn FindWindowExW(parent: *mut c_void, after: *mut c_void, class_name: *const u16, title: *const u16) -> *mut c_void;
    fn GetWindowPlacement(window: *mut c_void, placement: *mut WindowPlacement) -> i32;
    fn IsIconic(window: *mut c_void) -> i32;
    fn IsZoomed(window: *mut c_void) -> i32;
    fn GetDpiForWindow(window: *mut c_void) -> u32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn GetWindowLongPtrW(window: *mut c_void, index: i32) -> isize;
    fn SetWindowLongPtrW(window: *mut c_void, index: i32, value: isize) -> isize;
    fn SetWindowPos(
        window: *mut c_void,
        insert_after: *mut c_void,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The struct the addon reads back; only the fields the probe uses are declared.
#[cfg(not(windows))]
pub fn league_client_ux() -> Option<UxPlacement> {
    None
}

/// `getLeagueClientWindowPlacementInfo()` — `None` when the client window is not
/// there or the query fails, which is what the addon returned to JS.
#[cfg(windows)]
pub fn league_client_ux() -> Option<UxPlacement> {

    let class = wide(APPLICATION_CLASS_NAME);
    let title = wide(APPLICATION_NAME);

    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), title.as_ptr());
        if hwnd.is_null() {
            return None;
        }

        let mut placement = WindowPlacement {
            length: std::mem::size_of::<WindowPlacement>() as u32,
            flags: 0,
            show_cmd: 0,
            pt_min_x: 0,
            pt_min_y: 0,
            pt_max_x: 0,
            pt_max_y: 0,
            rc_normal_left: 0,
            rc_normal_top: 0,
            rc_normal_right: 0,
            rc_normal_bottom: 0,
        };
        if GetWindowPlacement(hwnd, &mut placement) == 0 {
            return None;
        }

        // `GetDpiForWindow` returns 0 when it cannot tell; the addon divided by the
        // scale regardless, so a zero dpi would divide by zero
        let dpi = GetDpiForWindow(hwnd);
        let scale = if dpi == 0 { 96.0 } else { dpi as f32 } / 96.0;

        let left = (placement.rc_normal_left as f32 / scale) as i32;
        let top = (placement.rc_normal_top as f32 / scale) as i32;
        let right = (placement.rc_normal_right as f32 / scale) as i32;
        let bottom = (placement.rc_normal_bottom as f32 / scale) as i32;

        let is_minimized = IsIconic(hwnd) != 0;
        let is_maximized = IsZoomed(hwnd) != 0;

        Some(UxPlacement {
            left,
            top,
            right,
            bottom,
            width: right - left,
            height: bottom - top,
            shown_state: placement.show_cmd as i32,
            is_minimized,
            is_maximized,
            is_normal: !is_minimized && !is_maximized,
        })
    }
}

/// `repositionToAlignLeagueClientUx`'s own guard: a minimized client collapses to a
/// ~134x37 stub, and snapping to that would park the overlay off-screen.
pub fn is_usable_as_align_target(placement: &UxPlacement) -> bool {
    !(placement.width < 200 && placement.height < 50)
}

/// `FixWindowMethodA`'s geometry maths: the addon truncates `base * zoom` to an
/// `int` and centres the result on the primary screen, and it refuses a zoom of
/// `-1` (what `/riotclient/zoom-scale` answers before the client has a scale).
pub fn scaled_window_size(
    zoom: f64,
    base_width: i32,
    base_height: i32,
    screen_width: i32,
    screen_height: i32,
) -> Result<(i32, i32, i32, i32), String> {
    if zoom == -1.0 {
        return Err("Invalid original zoom of LeagueClientUx".to_string());
    }

    let width = (base_width as f64 * zoom) as i32;
    let height = (base_height as f64 * zoom) as i32;

    Ok(((screen_width - width) / 2, (screen_height - height) / 2, width, height))
}

/// `tools.fixWindowMethodA(clientZoom, config)`. `Ok(true)` mirrors the addon's
/// `Napi::Boolean::New(env, true)`; the two `Err` strings are its two throw sites.
#[cfg(windows)]
pub fn adjust_league_client_window_size(
    zoom: f64,
    base: Option<(i32, i32)>,
) -> Result<bool, String> {
    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;
    const HWND_TOP: *mut c_void = std::ptr::null_mut();
    const SWP_NOZORDER: u32 = 0x4;

    let (base_width, base_height) = base.unwrap_or((DEFAULT_BASE_WIDTH, DEFAULT_BASE_HEIGHT));

    let (x, y, width, height) = unsafe {
        scaled_window_size(
            zoom,
            base_width,
            base_height,
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
        )?
    };

    let class = wide(APPLICATION_CLASS_NAME);
    let title = wide(APPLICATION_NAME);
    let cef_class = wide(CEF_WINDOW_NAME);

    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), title.as_ptr());
        let cef = FindWindowExW(hwnd, std::ptr::null_mut(), cef_class.as_ptr(), std::ptr::null());

        if hwnd.is_null() || cef.is_null() {
            return Err("Can't find LeagueClient window".to_string());
        }

        SetWindowPos(
            hwnd,
            HWND_TOP,
            x,
            y,
            width,
            height,
            SWP_NOZORDER,
        );
        SetWindowPos(cef, HWND_TOP, 0, 0, width, height, SWP_NOZORDER);
    }

    Ok(true)
}

#[cfg(not(windows))]
pub fn adjust_league_client_window_size(_zoom: f64, _base: Option<(i32, i32)>) -> Result<bool, String> {
    Err("adjustLeagueClientWindowSize is only available on Windows".to_string())
}

/// `WS_EX_TOPMOST | WS_EX_TOOLWINDOW` in, `WS_EX_APPWINDOW` out. The two are
/// documented as mutually exclusive, and a leftover APPWINDOW bit is what puts the
/// overlay back into Alt+Tab.
fn overlay_ex_style(current: isize) -> isize {
    const WS_EX_TOPMOST: isize = 0x0000_0008;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_APPWINDOW: isize = 0x0004_0000;

    (current | WS_EX_TOPMOST | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW
}

/// `RiftFlowCdTimerWindow._applyOverlayWindowBehavior()`'s window-level half, i.e.
/// `setSkipTaskbar(true)` + `setAlwaysOnTop(true, 'screen-saver', 1)`. Electron's
/// `level`/`relativeLevel` arguments only exist on macOS, so on Windows the shipped
/// behaviour is the tool-window style plus a fresh top-most slot — that slot has to be
/// re-taken every time `League of Legends.exe` becomes the foreground window, or the
/// overlay sinks below the borderless game client.
#[cfg(windows)]
pub fn raise_overlay(window: *mut c_void) -> bool {
    const GWL_EXSTYLE: i32 = -20;
    const SWP_NOSIZE: u32 = 0x1;
    const SWP_NOMOVE: u32 = 0x2;
    const SWP_NOACTIVATE: u32 = 0x10;

    if window.is_null() {
        return false;
    }

    unsafe {
        let current = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let desired = overlay_ex_style(current);
        if current != desired {
            SetWindowLongPtrW(window, GWL_EXSTYLE, desired);
        }

        SetWindowPos(
            window,
            (-1isize) as *mut c_void, // HWND_TOPMOST
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        ) != 0
    }
}

#[cfg(not(windows))]
pub fn raise_overlay(_window: *mut c_void) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_window_is_scaled_and_centred() {
        assert_eq!(
            scaled_window_size(1.25, 1280, 720, 1920, 1080),
            Ok((160, 90, 1600, 900))
        );

        // `static_cast<int>(base * zoom)` truncates rather than rounds
        assert_eq!(scaled_window_size(1.1, 100, 100, 1000, 1000).map(|g| g.2), Ok(110));

        assert_eq!(
            scaled_window_size(-1.0, 1280, 720, 1920, 1080),
            Err("Invalid original zoom of LeagueClientUx".to_string())
        );
    }

    #[test]
    fn the_minimized_stub_is_not_an_align_target() {
        let stub = UxPlacement {
            width: 134,
            height: 37,
            ..Default::default()
        };
        assert!(!is_usable_as_align_target(&stub));

        let small_but_real = UxPlacement {
            width: 199,
            height: 50,
            ..Default::default()
        };
        assert!(is_usable_as_align_target(&small_but_real));
    }

    #[test]
    fn the_overlay_style_pair_replaces_the_app_window_bit() {
        // plain window -> top-most + tool window
        assert_eq!(overlay_ex_style(0), 0x88);
        // `WS_EX_APPWINDOW` is cleared, it cannot coexist with `WS_EX_TOOLWINDOW`
        assert_eq!(overlay_ex_style(0x0004_0000), 0x88);
        // unrelated styles survive, and re-applying is idempotent
        assert_eq!(overlay_ex_style(0x0008_0088), 0x0008_0088);
    }

    #[test]
    fn the_probe_answers_without_a_client() {        // no assertion about the client's geometry is possible from a test host, but
        // the call must not panic and must yield None when there is no `RCLIENT`
        let found = league_client_ux();
        if let Some(placement) = found {
            assert!(placement.width > 0, "a real client window has a width");
            assert_eq!(placement.right - placement.left, placement.width);
        }
    }
}
