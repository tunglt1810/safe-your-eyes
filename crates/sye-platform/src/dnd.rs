#[cfg(any(target_os = "linux", test))]
use std::fs;
#[cfg(any(target_os = "linux", test))]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

use sye_core::{DndContext, DndProvider, DndQueryOptions, PlatformError};

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
        self.query_dnd_context_with(DndQueryOptions::default())
    }

    fn query_dnd_context_with(
        &self,
        options: DndQueryOptions,
    ) -> Result<DndContext, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            Ok(linux_impl::query_dnd_context(&self.x11, options))
        }
        #[cfg(target_os = "macos")]
        {
            Ok(macos_impl::query_dnd_context(options))
        }
        #[cfg(target_os = "windows")]
        {
            Ok(windows_impl::query_dnd_context(options))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = options;
            Ok(DndContext::default())
        }
    }
}

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

#[cfg(any(target_os = "linux", test))]
fn has_linux_game_environ(raw: &[u8]) -> bool {
    raw.split(|&b| b == 0).any(|entry| {
        if let Some(val) = entry.strip_prefix(b"SteamAppId=") {
            !val.is_empty() && val != b"0"
        } else if let Some(val) = entry.strip_prefix(b"STEAM_COMPAT_DATA_PATH=") {
            !val.is_empty()
        } else if let Some(val) = entry.strip_prefix(b"LUTRIS_GAME_UUID=") {
            !val.is_empty()
        } else if let Some(val) = entry.strip_prefix(b"HEROIC_APP_NAME=") {
            !val.is_empty()
        } else {
            false
        }
    })
}

#[cfg(any(target_os = "linux", test))]
fn has_linux_background_game_in(proc_root: &Path, self_pid: u32) -> bool {
    let Ok(entries) = fs::read_dir(proc_root) else {
        return false;
    };

    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == 0 || pid == self_pid {
            continue;
        }
        let proc_dir = entry.path();
        let exe_path = fs::read_link(proc_dir.join("exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|s| !s.trim().is_empty());
        let comm = fs::read_to_string(proc_dir.join("comm"))
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let process_name = exe_path
            .as_deref()
            .and_then(extract_executable_file_name)
            .or(comm);
        if sye_core::is_gaming_candidate(None, process_name.as_deref(), exe_path.as_deref()) {
            return true;
        }
        if fs::read(proc_dir.join("environ"))
            .ok()
            .is_some_and(|raw| has_linux_game_environ(&raw))
        {
            return true;
        }
    }
    false
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux", test))]
fn extract_executable_file_name(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let file_name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).trim();
    if file_name.is_empty() {
        None
    } else {
        Some(file_name.to_owned())
    }
}

#[cfg(any(target_os = "macos", test))]
fn is_macos_game_category(category: &str) -> bool {
    let lower = category.trim().to_ascii_lowercase();
    lower.starts_with("public.app-category.")
        && (lower == "public.app-category.games" || lower.ends_with("-games"))
}

#[cfg(any(target_os = "windows", test))]
const fn classify_windows_notification_state(raw_state: i32) -> (bool, bool, bool) {
    const QUNS_BUSY: i32 = 2;
    const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
    const QUNS_PRESENTATION_MODE: i32 = 4;
    match raw_state {
        QUNS_BUSY => (true, false, false),
        QUNS_RUNNING_D3D_FULL_SCREEN => (true, false, true),
        QUNS_PRESENTATION_MODE => (false, true, false),
        _ => (false, false, false),
    }
}

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
    use std::fs;
    use std::path::Path;
    use std::sync::{Mutex, PoisonError};

    use sye_core::{DndContext, DndQueryOptions, is_gaming_candidate};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};
    use x11rb::rust_connection::RustConnection;

    use super::{
        extract_executable_file_name, has_linux_background_game_in, has_linux_game_environ,
        is_battery_discharging_in, parse_wm_class,
    };

    #[derive(Default)]
    struct X11ActiveWindow {
        is_fullscreen: bool,
        window_class: Option<String>,
        pid: Option<u32>,
    }

    pub(super) struct X11DndState {
        conn: RustConnection,
        root: Window,
        atom_active_window: Atom,
        atom_wm_state: Atom,
        atom_wm_state_fullscreen: Atom,
        atom_wm_pid: Atom,
    }

    impl X11DndState {
        fn connect() -> Option<Self> {
            let (conn, screen_num) = RustConnection::connect(None).ok()?;
            let root = conn.setup().roots.get(screen_num)?.root;
            let active_cookie = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?;
            let wm_state_cookie = conn.intern_atom(false, b"_NET_WM_STATE").ok()?;
            let fullscreen_cookie = conn.intern_atom(false, b"_NET_WM_STATE_FULLSCREEN").ok()?;
            let wm_pid_cookie = conn.intern_atom(false, b"_NET_WM_PID").ok()?;

            let atom_active_window = active_cookie.reply().ok()?.atom;
            let atom_wm_state = wm_state_cookie.reply().ok()?.atom;
            let atom_wm_state_fullscreen = fullscreen_cookie.reply().ok()?.atom;
            let atom_wm_pid = wm_pid_cookie.reply().ok()?.atom;

            Some(Self {
                conn,
                root,
                atom_active_window,
                atom_wm_state,
                atom_wm_state_fullscreen,
                atom_wm_pid,
            })
        }

        fn query_active_window(&self) -> Option<X11ActiveWindow> {
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
                return Some(X11ActiveWindow::default());
            }

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
            let pid_cookie = self
                .conn
                .get_property(
                    false,
                    active_win,
                    self.atom_wm_pid,
                    AtomEnum::CARDINAL,
                    0,
                    1,
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

            let window_class = class_cookie
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| parse_wm_class(&reply.value));

            let pid = pid_cookie
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
                .filter(|&pid| pid > 0);

            Some(X11ActiveWindow {
                is_fullscreen,
                window_class,
                pid,
            })
        }
    }

    pub(super) fn query_dnd_context(
        cache: &Mutex<Option<X11DndState>>,
        options: DndQueryOptions,
    ) -> DndContext {
        let active = query_x11_window_state(cache);
        let (active_process_name, active_executable_path, proc_game_env) =
            active.pid.map_or((None, None, false), query_proc_info);
        let on_battery = if options.check_fullscreen {
            is_battery_discharging_in(Path::new("/sys/class/power_supply"))
        } else {
            false
        };
        let system_inhibited = if options.check_fullscreen {
            query_gnome_session_inhibited()
        } else {
            false
        };
        let game_detected = if options.check_gaming {
            proc_game_env
                || query_gamemode_active()
                || is_gaming_candidate(
                    active.window_class.as_deref(),
                    active_process_name.as_deref(),
                    active_executable_path.as_deref(),
                )
                || has_linux_background_game_in(Path::new("/proc"), std::process::id())
        } else {
            false
        };

        DndContext {
            is_fullscreen: active.is_fullscreen,
            active_window_class: active.window_class,
            active_process_name,
            active_executable_path,
            on_battery,
            system_inhibited,
            game_detected,
        }
    }

    fn query_x11_window_state(cache: &Mutex<Option<X11DndState>>) -> X11ActiveWindow {
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
            .unwrap_or_default()
    }

    fn query_proc_info(pid: u32) -> (Option<String>, Option<String>, bool) {
        let proc_dir = Path::new("/proc").join(pid.to_string());
        let exe_path = fs::read_link(proc_dir.join("exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|s| !s.trim().is_empty());
        let comm = fs::read_to_string(proc_dir.join("comm"))
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let process_name = exe_path
            .as_deref()
            .and_then(extract_executable_file_name)
            .or(comm);
        let has_game_env = fs::read(proc_dir.join("environ"))
            .ok()
            .is_some_and(|raw| has_linux_game_environ(&raw));
        (process_name, exe_path, has_game_env)
    }

    fn query_gnome_session_inhibited() -> bool {
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return false;
        };
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

    fn query_gamemode_active() -> bool {
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return false;
        };
        // A direct method call would D-Bus-activate `gamemoded` on hosts that install it but
        // do not run it.
        let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
            return false;
        };
        let Ok(name) = zbus::names::BusName::from_static_str("com.feralinteractive.GameMode")
        else {
            return false;
        };
        if !proxy.name_has_owner(name).unwrap_or(false) {
            return false;
        }
        let Ok(msg) = conn.call_method(
            Some("com.feralinteractive.GameMode"),
            "/com/feralinteractive/GameMode",
            Some("com.feralinteractive.GameMode"),
            "ClientCount",
            &(),
        ) else {
            return false;
        };
        msg.body().deserialize::<i32>().is_ok_and(|count| count > 0)
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::process::Command;

    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};
    use objc2_core_graphics::{CGDisplayIsCaptured, CGMainDisplayID};
    use objc2_foundation::{NSBundle, NSString, ns_string};
    use sye_core::{DndContext, DndQueryOptions, is_excluded_game_process, is_gaming_candidate};

    use super::{
        extract_executable_file_name, is_macos_game_category, parse_macos_frontmost_output,
        parse_macos_pmset_discharging,
    };

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

    #[derive(Default)]
    struct AppMetadata {
        localized_name: Option<String>,
        process_name: Option<String>,
        executable_path: Option<String>,
        is_game_category: bool,
    }

    fn inspect_running_app(app: &NSRunningApplication) -> AppMetadata {
        let localized_name = app
            .localizedName()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        let executable_path = app
            .executableURL()
            .and_then(|url| url.path())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        let process_name = executable_path
            .as_deref()
            .and_then(extract_executable_file_name)
            .or_else(|| localized_name.clone());
        let is_game_category = !is_excluded_game_process(process_name.as_deref())
            && app
                .bundleURL()
                .and_then(|url| NSBundle::bundleWithURL(&url))
                .and_then(|bundle| {
                    bundle.objectForInfoDictionaryKey(ns_string!("LSApplicationCategoryType"))
                })
                .and_then(|value| value.downcast::<NSString>().ok())
                .is_some_and(|category| is_macos_game_category(&category.to_string()));

        AppMetadata {
            localized_name,
            process_name,
            executable_path,
            is_game_category,
        }
    }

    fn query_frontmost_app_metadata() -> AppMetadata {
        autoreleasepool(|_| {
            let workspace = NSWorkspace::sharedWorkspace();
            workspace
                .frontmostApplication()
                .map_or_else(AppMetadata::default, |app| inspect_running_app(&app))
        })
    }

    fn has_background_game() -> bool {
        let self_pid = i32::try_from(std::process::id()).unwrap_or(-1);
        autoreleasepool(|_| {
            let workspace = NSWorkspace::sharedWorkspace();
            let apps = workspace.runningApplications();
            for app in apps {
                if app.isTerminated() || app.processIdentifier() == self_pid {
                    continue;
                }
                let meta = inspect_running_app(&app);
                if meta.is_game_category
                    || is_gaming_candidate(
                        None,
                        meta.process_name.as_deref(),
                        meta.executable_path.as_deref(),
                    )
                {
                    return true;
                }
            }
            false
        })
    }

    pub(super) fn query_dnd_context(options: DndQueryOptions) -> DndContext {
        let on_battery = if options.check_fullscreen {
            Command::new("pmset")
                .args(["-g", "batt"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
                .is_some_and(|stdout| parse_macos_pmset_discharging(&stdout))
        } else {
            false
        };

        let app_meta = query_frontmost_app_metadata();
        #[expect(
            deprecated,
            reason = "detects exclusive-display fullscreen games on macOS without Accessibility permission"
        )]
        let display_captured = if options.check_fullscreen || options.check_gaming {
            CGDisplayIsCaptured(CGMainDisplayID())
        } else {
            false
        };

        // Only invoke System Events AppleScript when fullscreen DND is enabled so gaming-only
        // detection stays completely permission-free on macOS.
        let (ax_fullscreen, script_app_name) = if !options.check_fullscreen || display_captured {
            (display_captured, None)
        } else {
            Command::new("osascript")
                .args(["-e", FRONTMOST_SCRIPT])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map_or((false, None), |out| {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    parse_macos_frontmost_output(&stdout)
                })
        };

        let game_detected = if options.check_gaming {
            display_captured
                || app_meta.is_game_category
                || is_gaming_candidate(
                    None,
                    app_meta.process_name.as_deref(),
                    app_meta.executable_path.as_deref(),
                )
                || has_background_game()
        } else {
            false
        };

        DndContext {
            is_fullscreen: display_captured || ax_fullscreen,
            active_window_class: script_app_name.or(app_meta.localized_name),
            active_process_name: app_meta.process_name,
            active_executable_path: app_meta.executable_path,
            on_battery,
            system_inhibited: false,
            game_detected,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::{MaybeUninit, size_of};
    use std::ptr;

    use sye_core::{DndContext, DndQueryOptions, is_gaming_candidate};
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HWND, LPARAM, RECT, TRUE};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    use super::{classify_windows_notification_state, extract_executable_file_name};

    const IGNORED_SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd"];
    const CLASS_BUF_LEN: usize = 256;
    const CLASS_BUF_LEN_I32: i32 = const {
        assert!(CLASS_BUF_LEN <= i32::MAX as usize);
        CLASS_BUF_LEN as i32
    };
    const PATH_BUF_LEN: usize = 1024;
    const PATH_BUF_LEN_U32: u32 = const {
        assert!(PATH_BUF_LEN <= u32::MAX as usize);
        PATH_BUF_LEN as u32
    };
    const MONITORINFO_SIZE_U32: u32 = const {
        assert!(size_of::<MONITORINFO>() <= u32::MAX as usize);
        size_of::<MONITORINFO>() as u32
    };

    #[derive(Default)]
    struct ForegroundWindowInfo {
        is_fullscreen: bool,
        window_class: Option<String>,
        process_name: Option<String>,
        executable_path: Option<String>,
    }

    struct BackgroundGameScan {
        self_pid: u32,
        found: bool,
    }

    pub(super) fn query_dnd_context(options: DndQueryOptions) -> DndContext {
        let on_battery = if options.check_fullscreen {
            query_on_battery()
        } else {
            false
        };
        let (quns_fullscreen, quns_inhibited, quns_game) = query_notification_state();
        let fg = query_foreground_window();
        let game_detected = if options.check_gaming {
            quns_game
                || is_gaming_candidate(
                    fg.window_class.as_deref(),
                    fg.process_name.as_deref(),
                    fg.executable_path.as_deref(),
                )
                || has_background_game()
        } else {
            false
        };
        DndContext {
            is_fullscreen: fg.is_fullscreen || quns_fullscreen,
            active_window_class: fg.window_class,
            active_process_name: fg.process_name,
            active_executable_path: fg.executable_path,
            on_battery,
            system_inhibited: quns_inhibited,
            game_detected,
        }
    }

    fn query_notification_state() -> (bool, bool, bool) {
        // SAFETY: `SHQueryUserNotificationState` takes no borrowed pointers and initializes its output.
        #[expect(unsafe_code, reason = "Win32 Shell FFI")]
        let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else {
            return (false, false, false);
        };
        classify_windows_notification_state(state.0)
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

    fn query_window_class(hwnd: HWND) -> Option<String> {
        let mut class_buf = [0u16; CLASS_BUF_LEN];
        // SAFETY: `class_buf` has capacity `CLASS_BUF_LEN_I32` matching `nMaxCount`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let len = unsafe { GetClassNameW(hwnd, class_buf.as_mut_ptr(), CLASS_BUF_LEN_I32) };
        usize::try_from(len)
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
            })
    }

    fn query_window_pid(hwnd: HWND) -> u32 {
        let mut pid: u32 = 0;
        // SAFETY: `hwnd` is a non-null window handle and `pid` is a valid writable `u32`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid
    }

    fn query_process_executable_for_pid(pid: u32) -> (Option<String>, Option<String>) {
        if pid == 0 {
            return (None, None);
        }

        // Only request `PROCESS_QUERY_LIMITED_INFORMATION` so anti-cheat protected games are not disturbed.
        // SAFETY: `OpenProcess` returns a valid handle or null on failure.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return (None, None);
        }

        let mut path_buf = [0u16; PATH_BUF_LEN];
        let mut path_len = PATH_BUF_LEN_U32;
        // SAFETY: `handle` is valid and `path_buf` has capacity `PATH_BUF_LEN_U32`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let ok = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                path_buf.as_mut_ptr(),
                &mut path_len,
            )
        };
        // SAFETY: `handle` was opened above and is closed exactly once.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe { CloseHandle(handle) };

        if ok == 0 {
            return (None, None);
        }

        let executable_path = usize::try_from(path_len)
            .ok()
            .filter(|&n| n > 0)
            .and_then(|n| path_buf.get(..n))
            .map(String::from_utf16_lossy)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let process_name = executable_path
            .as_deref()
            .and_then(extract_executable_file_name);
        (process_name, executable_path)
    }

    #[expect(unsafe_code, reason = "Win32 EnumWindows callback")]
    unsafe extern "system" fn scan_background_game_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `hwnd` is supplied by `EnumWindows`; `lparam` points to `BackgroundGameScan` on caller stack.
        let is_visible = unsafe { IsWindowVisible(hwnd) } != 0;
        if !is_visible {
            return TRUE;
        }

        let window_class = query_window_class(hwnd);
        if window_class
            .as_deref()
            .is_some_and(|c| IGNORED_SHELL_CLASSES.contains(&c))
        {
            return TRUE;
        }

        // SAFETY: `lparam` points to the live `BackgroundGameScan` struct passed to `EnumWindows`.
        let scan = unsafe { &mut *(lparam as *mut BackgroundGameScan) };
        let pid = query_window_pid(hwnd);
        if pid == 0 || pid == scan.self_pid {
            return TRUE;
        }

        let (process_name, executable_path) = query_process_executable_for_pid(pid);
        if is_gaming_candidate(
            window_class.as_deref(),
            process_name.as_deref(),
            executable_path.as_deref(),
        ) {
            scan.found = true;
            return FALSE;
        }
        TRUE
    }

    fn has_background_game() -> bool {
        // SAFETY: `GetCurrentProcessId` takes no parameters and returns the current process id.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let self_pid = unsafe { GetCurrentProcessId() };
        let mut scan = BackgroundGameScan {
            self_pid,
            found: false,
        };
        // SAFETY: `scan_background_game_proc` only runs synchronously during `EnumWindows` with `&mut scan`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe {
            EnumWindows(
                Some(scan_background_game_proc),
                ptr::from_mut(&mut scan) as LPARAM,
            )
        };
        scan.found
    }

    fn query_foreground_window() -> ForegroundWindowInfo {
        // SAFETY: `GetForegroundWindow` takes no parameters and returns a handle or null.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return ForegroundWindowInfo::default();
        }

        let window_class = query_window_class(hwnd);
        if window_class
            .as_deref()
            .is_some_and(|c| IGNORED_SHELL_CLASSES.contains(&c))
        {
            return ForegroundWindowInfo {
                window_class,
                ..ForegroundWindowInfo::default()
            };
        }

        let (process_name, executable_path) =
            query_process_executable_for_pid(query_window_pid(hwnd));

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
            return ForegroundWindowInfo {
                window_class,
                process_name,
                executable_path,
                is_fullscreen: false,
            };
        }

        // SAFETY: `hwnd` is non-null; returns monitor handle or null.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            return ForegroundWindowInfo {
                window_class,
                process_name,
                executable_path,
                is_fullscreen: false,
            };
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
            return ForegroundWindowInfo {
                window_class,
                process_name,
                executable_path,
                is_fullscreen: false,
            };
        }

        let mon = info.rcMonitor;
        let is_fullscreen = win_rect.left <= mon.left
            && win_rect.top <= mon.top
            && win_rect.right >= mon.right
            && win_rect.bottom >= mon.bottom;

        ForegroundWindowInfo {
            is_fullscreen,
            window_class,
            process_name,
            executable_path,
        }
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
    fn linux_game_environ_and_cross_platform_helpers_work() {
        assert!(has_linux_game_environ(b"HOME=/home/u\0SteamAppId=730\0"));
        assert!(!has_linux_game_environ(b"HOME=/home/u\0SteamAppId=0\0"));
        assert!(!has_linux_game_environ(b"HOME=/home/u\0SteamAppId=\0"));
        assert!(has_linux_game_environ(
            b"STEAM_COMPAT_DATA_PATH=/home/u/.steam/compatdata/123\0"
        ));
        assert!(has_linux_game_environ(b"LUTRIS_GAME_UUID=abc-123\0"));
        assert!(has_linux_game_environ(b"HEROIC_APP_NAME=Cyberpunk\0"));
        assert!(!has_linux_game_environ(b"PATH=/usr/bin\0DISPLAY=:0\0"));

        assert_eq!(
            extract_executable_file_name(r"C:\Games\Valorant\VALORANT-Win64-Shipping.exe"),
            Some("VALORANT-Win64-Shipping.exe".to_owned())
        );
        assert_eq!(
            extract_executable_file_name("/Applications/Hades.app/Contents/MacOS/Hades"),
            Some("Hades".to_owned())
        );
        assert_eq!(extract_executable_file_name("   "), None);

        assert!(is_macos_game_category("public.app-category.games"));
        assert!(is_macos_game_category("public.app-category.action-games"));
        assert!(is_macos_game_category(
            "public.app-category.role-playing-games"
        ));
        assert!(!is_macos_game_category(
            "public.app-category.developer-tools"
        ));
        assert!(!is_macos_game_category("public.app-category.productivity"));

        assert_eq!(classify_windows_notification_state(2), (true, false, false));
        assert_eq!(classify_windows_notification_state(3), (true, false, true));
        assert_eq!(classify_windows_notification_state(4), (false, true, false));
        assert_eq!(
            classify_windows_notification_state(5),
            (false, false, false)
        );
    }

    #[test]
    fn linux_background_game_scan_handles_mock_proc_tree() -> Result<(), Box<dyn std::error::Error>>
    {
        let tmp = TestTempDir::new("dnd-proc-scan")?;
        let proc_root = tmp.path();
        assert!(!has_linux_background_game_in(proc_root, 100));

        // Self PID and normal apps are ignored.
        fs::create_dir_all(proc_root.join("100"))?;
        fs::write(proc_root.join("100/comm"), "cs2\n")?;
        fs::create_dir_all(proc_root.join("200"))?;
        fs::write(proc_root.join("200/comm"), "firefox\n")?;
        fs::write(proc_root.join("200/environ"), b"HOME=/home/user\0")?;
        fs::create_dir_all(proc_root.join("201"))?;
        fs::write(proc_root.join("201/comm"), "steamwebhelper\n")?;
        fs::write(proc_root.join("201/environ"), b"SteamAppId=0\0")?;
        assert!(!has_linux_background_game_in(proc_root, 100));

        // Background process with SteamAppId environment variable matches.
        fs::create_dir_all(proc_root.join("300"))?;
        fs::write(proc_root.join("300/comm"), "wine64-preload\n")?;
        fs::write(proc_root.join("300/environ"), b"SteamAppId=1091500\0")?;
        assert!(has_linux_background_game_in(proc_root, 100));
        fs::remove_dir_all(proc_root.join("300"))?;

        // Background known game executable matches via comm.
        fs::create_dir_all(proc_root.join("400"))?;
        fs::write(proc_root.join("400/comm"), "dota2\n")?;
        assert!(has_linux_background_game_in(proc_root, 100));
        Ok(())
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
