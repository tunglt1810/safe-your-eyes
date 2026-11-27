#![cfg_attr(
    not(any(target_os = "windows", target_os = "macos")),
    forbid(unsafe_code)
)]

pub mod app_loop;
mod audio;
mod autostart;
mod clock;
mod console;
mod dnd;
mod idle;
mod ipc;
mod notify;
mod overlay;
mod paths;
#[cfg(test)]
mod test_util;
mod theme_reveal;
mod tray;
mod update;

pub use app_loop::PlatformEventLoop;
pub use audio::OsAudioPlayer;
pub use autostart::{
    AutostartEntry, AutostartManager, default_autostart_path, is_macos_app_bundle_executable,
    query_system_autostart, render_linux_desktop_entry, render_macos_launch_agent_plist,
    render_windows_run_command, render_windows_startup_cmd, sync_system_autostart,
};
pub use clock::SystemClock;
pub use console::{
    detach_console, hide_child_console, mark_own_windows_fullscreen, sync_own_windows_appearance,
    take_foreground_for_own_window,
};
pub use dnd::SystemDndProvider;
pub use idle::SystemIdleProvider;
pub use ipc::{DEFAULT_IPC_TIMEOUT, IpcClient, IpcError, IpcServer};
pub use notify::{OsNotifier, format_pre_break_notification};
pub use overlay::{OVERLAY_SHOWN_SIGNAL, SubprocessOverlayLauncher};
pub use paths::{AppPaths, session_path_for_config, update_cache_path_for_config};
pub use sye_core::traits;
pub use theme_reveal::{
    cancel_theme_transition_snapshot, capture_theme_transition_snapshot,
    circular_reveal_max_radius, map_reveal_origin_y, reveal_theme_transition_snapshot,
    system_prefers_dark,
};
pub use tray::{
    TrayEvent, TrayHandle, TrayIconState, format_status_label, format_tooltip,
    format_update_menu_label, icon_state_for_snapshot, map_menu_id_to_event, render_eye_icon_rgba,
    render_tray_icon_rgba, tray_tooltip, tray_tooltip_for_platform,
};
pub use update::{
    apply_self_update, apply_self_update_to_path, check_and_cache_update, check_latest_version,
    cleanup_current_stale_old_executable, cleanup_stale_old_executable, compute_sha256_hex,
    detect_current_update_strategy, detect_update_strategy, now_unix_secs, open_url,
    replace_executable, verify_sha256_hex,
};
