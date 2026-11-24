//! Cross-platform OS adapters for `safe-your-eyes`.
//!
//! Provides genuine, lightweight adapters for system clock, system tray (F12), idle and DND
//! providers (F13), desktop notifications and WAV audio playback (F14), and single-instance local
//! IPC, OS paths, autostart, and subprocess overlay launching (F15).

#![cfg_attr(
    not(any(target_os = "windows", target_os = "macos")),
    forbid(unsafe_code)
)]

pub mod app_loop;
mod audio;
mod autostart;
mod clock;
mod dnd;
mod idle;
mod ipc;
mod notify;
mod overlay;
mod paths;
#[cfg(test)]
mod test_util;
mod tray;

pub use app_loop::PlatformEventLoop;
pub use audio::OsAudioPlayer;
pub use autostart::{
    AutostartEntry, AutostartManager, default_autostart_path, render_linux_desktop_entry,
    render_macos_launch_agent_plist, render_windows_startup_cmd, sync_system_autostart,
};
pub use clock::SystemClock;
pub use dnd::SystemDndProvider;
pub use idle::SystemIdleProvider;
pub use ipc::{DEFAULT_IPC_TIMEOUT, IpcClient, IpcError, IpcServer};
pub use notify::{OsNotifier, format_pre_break_notification};
pub use overlay::SubprocessOverlayLauncher;
pub use paths::{AppPaths, session_path_for_config};
pub use sye_core::traits;
pub use tray::{
    TrayEvent, TrayHandle, TrayIconState, format_status_label, format_tooltip,
    icon_state_for_snapshot, map_menu_id_to_event, render_eye_icon_rgba, render_tray_icon_rgba,
};
