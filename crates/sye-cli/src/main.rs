mod cli;
mod daemon;

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sye_core::config::{BreakType, Config};
use sye_core::ipc::{IpcCommand, IpcResponse};
use sye_core::scheduler::{OverlayOutcome, OverlayRequest};
use sye_platform::{AppPaths, IpcClient, IpcError};

use crate::cli::{CommandMode, HELP_TEXT, parse_args};

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    match args.mode {
        CommandMode::Help => {
            print!("{HELP_TEXT}");
            return ExitCode::SUCCESS;
        }
        CommandMode::Version => {
            println!("safe-your-eyes {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        CommandMode::PrintDefaultConfig => {
            return match Config::default().to_toml_string() {
                Ok(toml_str) => {
                    print!("{toml_str}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("Failed to serialize default config: {err}");
                    ExitCode::FAILURE
                }
            };
        }
        CommandMode::InternalOverlay { payload_json } => {
            return run_internal_overlay(&payload_json);
        }
        _ => {}
    }

    let paths = match AppPaths::with_overrides(args.config, args.socket) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("Failed to resolve application paths: {err}");
            return ExitCode::FAILURE;
        }
    };

    match args.mode {
        CommandMode::ValidateConfig => match Config::load_from_path(&paths.config_file) {
            Ok(_) => {
                println!("Configuration is valid: {}", paths.config_file.display());
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!(
                    "Invalid configuration ({}): {err}",
                    paths.config_file.display()
                );
                ExitCode::FAILURE
            }
        },
        CommandMode::InternalSettings => run_settings_and_sync_autostart(&paths),
        CommandMode::Daemon => daemon::run_daemon(&paths, args.headless, args.background),
        CommandMode::Status { json } => send_ipc_command(
            &paths,
            &IpcCommand::Status,
            if json {
                StatusFormat::Json
            } else {
                StatusFormat::Human
            },
        ),
        CommandMode::Enable => send_ipc_command(&paths, &IpcCommand::Enable, StatusFormat::Human),
        CommandMode::Disable { minutes } => send_ipc_command(
            &paths,
            &IpcCommand::Disable { minutes },
            StatusFormat::Human,
        ),
        CommandMode::TakeBreak => send_ipc_command(
            &paths,
            &IpcCommand::TakeBreak { kind: None },
            StatusFormat::Human,
        ),
        CommandMode::ShortBreak => send_ipc_command(
            &paths,
            &IpcCommand::TakeBreak {
                kind: Some(BreakType::Short),
            },
            StatusFormat::Human,
        ),
        CommandMode::LongBreak => send_ipc_command(
            &paths,
            &IpcCommand::TakeBreak {
                kind: Some(BreakType::Long),
            },
            StatusFormat::Human,
        ),
        CommandMode::Settings => {
            match IpcClient::send_command(&paths.socket_path, &IpcCommand::OpenSettings) {
                Ok(IpcResponse::Ok { .. } | IpcResponse::Status(_)) => ExitCode::SUCCESS,
                Ok(IpcResponse::Error { message, .. }) => {
                    eprintln!("{message}");
                    ExitCode::FAILURE
                }
                Err(IpcError::NotRunning) => run_settings_and_sync_autostart(&paths),
                Err(err) => {
                    eprintln!("{err}");
                    ExitCode::FAILURE
                }
            }
        }
        CommandMode::Quit => send_ipc_command(&paths, &IpcCommand::Quit, StatusFormat::Human),
        CommandMode::Help
        | CommandMode::Version
        | CommandMode::PrintDefaultConfig
        | CommandMode::InternalOverlay { .. } => ExitCode::SUCCESS,
    }
}

fn run_internal_overlay(payload_json: &str) -> ExitCode {
    let request: OverlayRequest = match serde_json::from_str(payload_json) {
        Ok(req) => req,
        Err(err) => {
            eprintln!("Invalid overlay payload JSON: {err}");
            return ExitCode::FAILURE;
        }
    };
    let written = Arc::new(AtomicBool::new(false));
    let hook_written = Arc::clone(&written);
    let on_outcome: sye_ui::OverlayOutcomeCallback = Arc::new(move |outcome| {
        if write_overlay_outcome(outcome) {
            hook_written.store(true, Ordering::SeqCst);
        }
    });
    let on_shown: sye_ui::OverlayShownCallback = Box::new(|| {
        write_overlay_shown();
        sye_platform::take_foreground_for_own_window();
        sye_platform::mark_own_windows_fullscreen();
    });
    match sye_ui::run_overlay_with_hook(request, Some(on_shown), Some(on_outcome)) {
        Ok(outcome) => {
            if written.load(Ordering::SeqCst) || write_overlay_outcome(outcome) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run_settings_and_sync_autostart(paths: &AppPaths) -> ExitCode {
    // An older daemon left running across an in-place upgrade spawns the newly installed binary
    // for Settings; hand off to a fresh daemon so tray and break overlay match this version.
    if std::env::var_os(daemon::HANDOFF_ENV_VAR).is_none()
        && let Some(snapshot) = query_daemon_status(&paths.socket_path)
        && daemon::should_replace_running_daemon(&snapshot, daemon::CURRENT_VERSION)
        && daemon::hand_off_to_new_daemon(paths)
    {
        return ExitCode::SUCCESS;
    }

    let config_file = &paths.config_file;
    let initial_system_autostart = sye_platform::query_system_autostart(Some(config_file));
    let hook_path = config_file.clone();
    let on_autostart_change = Box::new(move |enabled: bool| {
        if let Err(err) = sye_platform::sync_system_autostart(enabled, Some(&hook_path)) {
            eprintln!("Warning: failed to synchronize autostart entry: {err}");
        }
    });

    #[cfg(not(feature = "msstore"))]
    let update_config = {
        let current_version = env!("CARGO_PKG_VERSION");
        let (cached_update, _) =
            sye_core::UpdateCacheState::load_or_default(&paths.update_cache_file);
        let initial_update = cached_update.available_update(current_version);
        let strategy = sye_platform::detect_current_update_strategy();

        let check_cache_path = paths.update_cache_file.clone();
        let on_check_now: sye_ui::CheckUpdatesCallback = Arc::new(move || {
            sye_platform::check_and_cache_update(
                &check_cache_path,
                current_version,
                sye_platform::now_unix_secs(),
            )
            .map_err(|err| err.to_string())
        });

        let on_apply_update: Option<sye_ui::ApplyUpdateCallback> = match strategy {
            sye_core::UpdateStrategy::SelfUpdate { asset_name } => {
                let socket_path = paths.socket_path.clone();
                Some(Arc::new(move |target_version: &str| {
                    sye_platform::apply_self_update(target_version, asset_name, current_version)
                        .map_err(|err| err.to_string())?;
                    match IpcClient::send_command(&socket_path, &IpcCommand::Restart) {
                        Ok(IpcResponse::Ok { .. }) => {
                            Ok(sye_ui::ApplyUpdateOutcome::RestartingDaemon)
                        }
                        Ok(IpcResponse::Status(_)) | Err(IpcError::NotRunning) => {
                            Ok(sye_ui::ApplyUpdateOutcome::UpdatedStandalone)
                        }
                        Ok(IpcResponse::Error { message, .. }) => Err(message),
                        Err(err) => Err(err.to_string()),
                    }
                }))
            }
            sye_core::UpdateStrategy::HomebrewCask | sye_core::UpdateStrategy::ManualDownload => {
                None
            }
        };

        let on_open_download: sye_ui::OpenUrlCallback =
            Arc::new(|url: &str| sye_platform::open_url(url).map_err(|err| err.to_string()));

        sye_ui::SettingsUpdateConfig {
            initial_update,
            strategy: Some(strategy),
            on_check_now: Some(on_check_now),
            on_apply_update,
            on_open_download: Some(on_open_download),
        }
    };
    #[cfg(feature = "msstore")]
    let update_config = sye_ui::SettingsUpdateConfig::default();

    let status_socket_path = paths.socket_path.clone();
    let status_config = sye_ui::SettingsStatusConfig {
        initial_status: None,
        on_poll_status: Some(Arc::new(move || query_daemon_status(&status_socket_path))),
    };

    #[cfg(target_os = "macos")]
    let theme_hooks = sye_ui::ThemeTransitionHooks {
        on_theme_change: Some(Arc::new(sye_platform::sync_own_windows_appearance)),
        system_prefers_dark: Some(Arc::new(sye_platform::system_prefers_dark)),
        capture_snapshot: Some(Arc::new(sye_platform::capture_theme_transition_snapshot)),
        reveal_snapshot: Some(Arc::new(sye_platform::reveal_theme_transition_snapshot)),
        cancel_snapshot: Some(Arc::new(sye_platform::cancel_theme_transition_snapshot)),
    };
    #[cfg(not(target_os = "macos"))]
    let theme_hooks = sye_ui::ThemeTransitionHooks::default();

    #[cfg(target_os = "macos")]
    let on_window_ready: Option<sye_ui::WindowReadyCallback> = Some(Arc::new(
        sye_platform::disable_native_titlebar_drag_for_own_windows,
    ));
    #[cfg(not(target_os = "macos"))]
    let on_window_ready: Option<sye_ui::WindowReadyCallback> = None;

    let system_locale = sye_platform::detect_system_locale();

    match sye_ui::run_settings_with_hooks(
        config_file.clone(),
        initial_system_autostart,
        Some(on_autostart_change),
        theme_hooks,
        update_config,
        status_config,
        on_window_ready,
        system_locale,
    ) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn query_daemon_status(
    socket_path: &std::path::Path,
) -> Option<sye_core::scheduler::StatusSnapshot> {
    match IpcClient::send_command_with_timeout(
        socket_path,
        &IpcCommand::Status,
        std::time::Duration::from_millis(250),
    ) {
        Ok(IpcResponse::Status(snapshot)) => Some(snapshot),
        _ => None,
    }
}

fn write_overlay_line(line: &str) -> bool {
    use std::io::Write as _;

    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{line}").is_ok() && stdout.flush().is_ok()
}

fn write_overlay_shown() {
    let _ = write_overlay_line(sye_platform::OVERLAY_SHOWN_SIGNAL);
}

fn write_overlay_outcome(outcome: OverlayOutcome) -> bool {
    match serde_json::to_string(&outcome) {
        Ok(line) => write_overlay_line(&line),
        Err(err) => {
            eprintln!("Failed to serialize overlay outcome: {err}");
            false
        }
    }
}

#[derive(Clone, Copy)]
enum StatusFormat {
    Human,
    Json,
}

fn send_ipc_command(paths: &AppPaths, cmd: &IpcCommand, status_format: StatusFormat) -> ExitCode {
    match IpcClient::send_command(&paths.socket_path, cmd) {
        Ok(IpcResponse::Status(snapshot)) => match status_format {
            StatusFormat::Json => match serde_json::to_string(&snapshot) {
                Ok(line) => {
                    println!("{line}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("Failed to serialize status JSON: {err}");
                    ExitCode::FAILURE
                }
            },
            StatusFormat::Human => {
                println!("{}", daemon::format_human_status(&snapshot));
                ExitCode::SUCCESS
            }
        },
        Ok(IpcResponse::Ok { message }) => {
            if let Some(msg) = message {
                println!("{msg}");
            }
            ExitCode::SUCCESS
        }
        Ok(IpcResponse::Error { message, .. }) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
        Err(IpcError::NotRunning) => {
            eprintln!("Safe Your Eyes is not running");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
