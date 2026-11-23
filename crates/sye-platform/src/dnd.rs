//! Cross-platform Do-Not-Disturb context provider (`sye_core::DndProvider`).

#[cfg(any(target_os = "linux", test))]
use std::fs;
#[cfg(any(target_os = "linux", test))]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

use sye_core::{DndContext, DndProvider, PlatformError};

/// OS Do-Not-Disturb provider querying fullscreen window, window class, battery, and inhibit state.
///
/// Always degrades gracefully per signal so `query_dnd_context()` returns `Ok(DndContext)` even in
/// headless CI environments.
pub struct SystemDndProvider {
    #[cfg(target_os = "linux")]
    x11: Mutex<Option<linux_impl::X11DndState>>,
}

impl std::fmt::Debug for SystemDndProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemDndProvider").finish_non_exhaustive()
    }
}

impl SystemDndProvider {
    /// Creates a new `SystemDndProvider`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            x11: Mutex::new(None),
        }
    }
}

impl Default for SystemDndProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DndProvider for SystemDndProvider {
    fn query_dnd_context(&self) -> Result<DndContext, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            Ok(linux_impl::query_dnd_context(&self.x11))
        }
        #[cfg(target_os = "macos")]
        {
            Ok(macos_impl::query_dnd_context())
        }
        #[cfg(target_os = "windows")]
        {
            Ok(windows_impl::query_dnd_context())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Ok(DndContext::default())
        }
    }
}

/// Parses an X11 `WM_CLASS` property (`res_name\0res_class\0`), preferring `res_class`.
#[cfg(any(target_os = "linux", test))]
fn parse_wm_class(raw: &[u8]) -> Option<String> {
    let mut parts = raw
        .split(|&b| b == 0)
        .filter(|slice| !slice.is_empty())
        .filter_map(|slice| {
            let text = String::from_utf8_lossy(slice);
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        });

    let first = parts.next();
    let second = parts.next();
    second.or(first).map(|text| text.trim().to_owned())
}

/// Returns `true` if any battery entry under `power_supply_dir` has `type == Battery` and
/// `status == Discharging` (case-insensitive).
#[cfg(any(target_os = "linux", test))]
fn is_battery_discharging_in(power_supply_dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(power_supply_dir) else {
        return false;
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(kind) = fs::read_to_string(dir.join("type")) else {
            continue;
        };
        if !kind.trim().eq_ignore_ascii_case("Battery") {
            continue;
        }
        let Ok(status) = fs::read_to_string(dir.join("status")) else {
            continue;
        };
        if status.trim().eq_ignore_ascii_case("Discharging") {
            return true;
        }
    }
    false
}

/// Parses `pmset -g batt` output on macOS and returns `true` when running on discharging battery.
#[cfg(any(target_os = "macos", test))]
fn parse_macos_pmset_discharging(output: &str) -> bool {
    if output.contains("'Battery Power'") {
        return true;
    }
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("discharging") && !lower.contains("not charging") {
            return true;
        }
    }
    false
}

/// Parses two-line `osascript` output (`<app_name>\n<true|false>`) on macOS.
#[cfg(any(target_os = "macos", test))]
fn parse_macos_frontmost_output(output: &str) -> (bool, Option<String>) {
    let mut lines = output.lines();
    let app_name = lines
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let is_fullscreen = lines
        .next()
        .map(str::trim)
        .is_some_and(|s| s.eq_ignore_ascii_case("true"));
    (is_fullscreen, app_name)
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::path::Path;
    use std::sync::{Mutex, PoisonError};

    use sye_core::DndContext;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};
    use x11rb::rust_connection::RustConnection;

    use super::{is_battery_discharging_in, parse_wm_class};

    pub(super) struct X11DndState {
        conn: RustConnection,
        root: Window,
        atom_active_window: Atom,
        atom_wm_state: Atom,
        atom_wm_state_fullscreen: Atom,
    }

    impl X11DndState {
        fn connect() -> Option<Self> {
            let (conn, screen_num) = RustConnection::connect(None).ok()?;
            let root = conn.setup().roots.get(screen_num)?.root;
            // Pipeline all three InternAtom requests into a single X11 round trip.
            let active_cookie = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?;
            let wm_state_cookie = conn.intern_atom(false, b"_NET_WM_STATE").ok()?;
            let fullscreen_cookie = conn.intern_atom(false, b"_NET_WM_STATE_FULLSCREEN").ok()?;

            let atom_active_window = active_cookie.reply().ok()?.atom;
            let atom_wm_state = wm_state_cookie.reply().ok()?.atom;
            let atom_wm_state_fullscreen = fullscreen_cookie.reply().ok()?.atom;

            Some(Self {
                conn,
                root,
                atom_active_window,
                atom_wm_state,
                atom_wm_state_fullscreen,
            })
        }

        fn query_active_window(&self) -> Option<(bool, Option<String>)> {
            let active_reply = self
                .conn
                .get_property(
                    false,
                    self.root,
                    self.atom_active_window,
                    AtomEnum::WINDOW,
                    0,
                    1,
                )
                .ok()?
                .reply()
                .ok()?;

            let active_win = active_reply.value32()?.next()?;
            if active_win == 0 {
                return Some((false, None));
            }

            // Pipeline both window property queries into a single X11 round trip.
            let state_cookie = self
                .conn
                .get_property(false, active_win, self.atom_wm_state, AtomEnum::ATOM, 0, 64)
                .ok();
            let class_cookie = self
                .conn
                .get_property(
                    false,
                    active_win,
                    AtomEnum::WM_CLASS,
                    AtomEnum::STRING,
                    0,
                    1024,
                )
                .ok();

            let is_fullscreen = state_cookie
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| {
                    reply
                        .value32()
                        .map(|mut atoms| atoms.any(|a| a == self.atom_wm_state_fullscreen))
                })
                .unwrap_or(false);

            let active_window_class = class_cookie
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| parse_wm_class(&reply.value));

            Some((is_fullscreen, active_window_class))
        }
    }

    pub(super) fn query_dnd_context(cache: &Mutex<Option<X11DndState>>) -> DndContext {
        let (is_fullscreen, active_window_class) = query_x11_window_state(cache);
        let on_battery = is_battery_discharging_in(Path::new("/sys/class/power_supply"));
        let system_inhibited = query_gnome_session_inhibited();

        DndContext {
            is_fullscreen,
            active_window_class,
            on_battery,
            system_inhibited,
        }
    }

    fn query_x11_window_state(cache: &Mutex<Option<X11DndState>>) -> (bool, Option<String>) {
        let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(existing) = guard.as_ref()
            && let Some(result) = existing.query_active_window()
        {
            return result;
        }

        *guard = X11DndState::connect();
        guard
            .as_ref()
            .and_then(X11DndState::query_active_window)
            .unwrap_or((false, None))
    }

    fn query_gnome_session_inhibited() -> bool {
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return false;
        };
        // Flag 8 corresponds to idle inhibit in org.gnome.SessionManager.IsInhibited.
        let Ok(msg) = conn.call_method(
            Some("org.gnome.SessionManager"),
            "/org/gnome/SessionManager",
            Some("org.gnome.SessionManager"),
            "IsInhibited",
            &(8u32,),
        ) else {
            return false;
        };
        msg.body().deserialize::<bool>().unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::process::Command;

    use sye_core::DndContext;

    use super::{parse_macos_frontmost_output, parse_macos_pmset_discharging};

    const FRONTMOST_SCRIPT: &str = r#"
tell application "System Events"
    set frontApp to first application process whose frontmost is true
    set appName to name of frontApp
    set isFull to "false"
    try
        if (value of attribute "AXFullScreen" of window 1 of frontApp) is true then
            set isFull to "true"
        end if
    end try
    return appName & linefeed & isFull
end tell
"#;

    pub(super) fn query_dnd_context() -> DndContext {
        let on_battery = Command::new("pmset")
            .args(["-g", "batt"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .is_some_and(|stdout| parse_macos_pmset_discharging(&stdout));

        let (is_fullscreen, active_window_class) = Command::new("osascript")
            .args(["-e", FRONTMOST_SCRIPT])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map_or((false, None), |out| {
                let stdout = String::from_utf8_lossy(&out.stdout);
                parse_macos_frontmost_output(&stdout)
            });

        DndContext {
            is_fullscreen,
            active_window_class,
            on_battery,
            system_inhibited: false,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::{MaybeUninit, size_of};

    use sye_core::DndContext;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowRect,
    };

    const IGNORED_SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd"];
    const CLASS_BUF_LEN: usize = 256;
    const CLASS_BUF_LEN_I32: i32 = const {
        assert!(CLASS_BUF_LEN <= i32::MAX as usize);
        CLASS_BUF_LEN as i32
    };
    const MONITORINFO_SIZE_U32: u32 = const {
        assert!(size_of::<MONITORINFO>() <= u32::MAX as usize);
        size_of::<MONITORINFO>() as u32
    };

    pub(super) fn query_dnd_context() -> DndContext {
        let on_battery = query_on_battery();
        let (is_fullscreen, active_window_class) = query_foreground_window();
        DndContext {
            is_fullscreen,
            active_window_class,
            on_battery,
            system_inhibited: false,
        }
    }

    fn query_on_battery() -> bool {
        let mut sps = MaybeUninit::<SYSTEM_POWER_STATUS>::uninit();
        // SAFETY: `sps.as_mut_ptr()` points to valid writable storage for `SYSTEM_POWER_STATUS`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let ok = unsafe { GetSystemPowerStatus(sps.as_mut_ptr()) };
        if ok == 0 {
            return false;
        }
        // SAFETY: `GetSystemPowerStatus` returned non-zero, initializing all fields of `sps`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let sps = unsafe { sps.assume_init() };
        sps.ACLineStatus == 0 && (sps.BatteryFlag & 128) == 0 && sps.BatteryFlag != 255
    }

    fn query_foreground_window() -> (bool, Option<String>) {
        // SAFETY: `GetForegroundWindow` takes no parameters and returns a handle or null.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return (false, None);
        }

        let mut class_buf = [0u16; CLASS_BUF_LEN];
        // SAFETY: `class_buf` has capacity `CLASS_BUF_LEN_I32` matching `nMaxCount`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let len = unsafe { GetClassNameW(hwnd, class_buf.as_mut_ptr(), CLASS_BUF_LEN_I32) };
        let class_name = usize::try_from(len)
            .ok()
            .filter(|&n| n > 0)
            .and_then(|n| class_buf.get(..n))
            .and_then(|slice| {
                let name = String::from_utf16_lossy(slice);
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    None
                } else if trimmed.len() == name.len() {
                    Some(name)
                } else {
                    Some(trimmed.to_owned())
                }
            });

        if class_name
            .as_deref()
            .is_some_and(|c| IGNORED_SHELL_CLASSES.contains(&c))
        {
            return (false, class_name);
        }

        let mut win_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `win_rect` is a valid writable `RECT`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let has_rect = unsafe { GetWindowRect(hwnd, &mut win_rect) } != 0;
        if !has_rect {
            return (false, class_name);
        }

        // SAFETY: `hwnd` is non-null; returns monitor handle or null.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            return (false, class_name);
        }

        let mut info = MONITORINFO {
            cbSize: MONITORINFO_SIZE_U32,
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            dwFlags: 0,
        };
        // SAFETY: `info.cbSize` is initialized to `MONITORINFO_SIZE_U32` and `monitor` is non-null.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let has_info = unsafe { GetMonitorInfoW(monitor, &mut info) } != 0;
        if !has_info {
            return (false, class_name);
        }

        let mon = info.rcMonitor;
        let is_fullscreen = win_rect.left <= mon.left
            && win_rect.top <= mon.top
            && win_rect.right >= mon.right
            && win_rect.bottom >= mon.bottom;

        (is_fullscreen, class_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestTempDir;

    #[test]
    fn parse_wm_class_prefers_res_class_and_handles_edge_cases() {
        assert_eq!(
            parse_wm_class(b"Navigator\0firefox\0"),
            Some("firefox".to_owned())
        );
        assert_eq!(parse_wm_class(b"vlc\0\0"), Some("vlc".to_owned()));
        assert_eq!(
            parse_wm_class(b"only_instance"),
            Some("only_instance".to_owned())
        );
        assert_eq!(parse_wm_class(b""), None);
        assert_eq!(parse_wm_class(b"\0\0"), None);
        assert!(parse_wm_class(b"bad\xff\0GoodClass\0").as_deref() == Some("GoodClass"));
    }

    #[test]
    fn battery_discharging_detection_handles_mock_sysfs_states()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("dnd-battery")?;
        let missing = tmp.path().join("does-not-exist");
        assert!(!is_battery_discharging_in(&missing));

        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty)?;
        assert!(!is_battery_discharging_in(&empty));

        let mains_only = tmp.path().join("mains");
        fs::create_dir_all(mains_only.join("AC"))?;
        fs::write(mains_only.join("AC/type"), "Mains\n")?;
        fs::write(mains_only.join("AC/status"), "Discharging\n")?;
        assert!(!is_battery_discharging_in(&mains_only));

        let charging = tmp.path().join("charging");
        fs::create_dir_all(charging.join("BAT0"))?;
        fs::write(charging.join("BAT0/type"), "Battery\n")?;
        fs::write(charging.join("BAT0/status"), "Charging\n")?;
        assert!(!is_battery_discharging_in(&charging));

        let full = tmp.path().join("full");
        fs::create_dir_all(full.join("BAT0"))?;
        fs::write(full.join("BAT0/type"), "Battery\n")?;
        fs::write(full.join("BAT0/status"), "Full\n")?;
        assert!(!is_battery_discharging_in(&full));

        let not_charging = tmp.path().join("not-charging");
        fs::create_dir_all(not_charging.join("BAT0"))?;
        fs::write(not_charging.join("BAT0/type"), "Battery\n")?;
        fs::write(not_charging.join("BAT0/status"), "Not charging\n")?;
        assert!(!is_battery_discharging_in(&not_charging));

        let discharging = tmp.path().join("discharging");
        fs::create_dir_all(discharging.join("BAT0"))?;
        fs::write(discharging.join("BAT0/type"), "Battery\n")?;
        fs::write(discharging.join("BAT0/status"), "Discharging\n")?;
        assert!(is_battery_discharging_in(&discharging));

        let dual = tmp.path().join("dual");
        fs::create_dir_all(dual.join("BROKEN"))?;
        fs::create_dir_all(dual.join("BAT0"))?;
        fs::write(dual.join("BAT0/type"), "Battery\n")?;
        fs::write(dual.join("BAT0/status"), "Full\n")?;
        fs::create_dir_all(dual.join("BAT1"))?;
        fs::write(dual.join("BAT1/type"), "Battery\n")?;
        fs::write(dual.join("BAT1/status"), "Discharging\n")?;
        assert!(is_battery_discharging_in(&dual));
        Ok(())
    }

    #[test]
    fn macos_pmset_and_frontmost_parsers_work_cross_platform() {
        let on_batt = "Now drawing from 'Battery Power'\n -InternalBattery-0\t85%; discharging; 3:12 remaining\n";
        assert!(parse_macos_pmset_discharging(on_batt));

        let on_ac =
            "Now drawing from 'AC Power'\n -InternalBattery-0\t100%; charged; 0:00 remaining\n";
        assert!(!parse_macos_pmset_discharging(on_ac));

        let not_charging =
            "Now drawing from 'AC Power'\n -InternalBattery-0\t80%; AC attached; not charging\n";
        assert!(!parse_macos_pmset_discharging(not_charging));
        assert!(!parse_macos_pmset_discharging(""));

        assert_eq!(
            parse_macos_frontmost_output("Firefox\ntrue\n"),
            (true, Some("Firefox".to_owned()))
        );
        assert_eq!(
            parse_macos_frontmost_output("Code\nfalse\n"),
            (false, Some("Code".to_owned()))
        );
        assert_eq!(parse_macos_frontmost_output(""), (false, None));
    }

    #[test]
    fn system_dnd_provider_returns_ok_in_headless_env() -> Result<(), Box<dyn std::error::Error>> {
        let provider = SystemDndProvider::new();
        let _ctx = provider.query_dnd_context()?;
        Ok(())
    }
}
