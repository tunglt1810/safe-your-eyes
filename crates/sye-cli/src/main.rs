//! Single-binary entry point for `safe-your-eyes`.

mod cli;
mod daemon;

use std::process::ExitCode;

use sye_core::config::{BreakType, Config};
use sye_core::ipc::{IpcCommand, IpcResponse};
use sye_core::scheduler::OverlayRequest;
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
        CommandMode::InternalSettings => run_settings_and_sync_autostart(&paths.config_file),
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
                Err(IpcError::NotRunning) => run_settings_and_sync_autostart(&paths.config_file),
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
    match sye_ui::run_overlay_with_hook(
        request,
        Some(Box::new(sye_platform::mark_own_windows_fullscreen)),
    ) {
        Ok(outcome) => match serde_json::to_string(&outcome) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("Failed to serialize overlay outcome: {err}");
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run_settings_and_sync_autostart(config_file: &std::path::Path) -> ExitCode {
    let initial_system_autostart = sye_platform::query_system_autostart(Some(config_file));
    let hook_path = config_file.to_path_buf();
    let on_autostart_change = Box::new(move |enabled: bool| {
        if let Err(err) = sye_platform::sync_system_autostart(enabled, Some(&hook_path)) {
            eprintln!("Warning: failed to synchronize autostart entry: {err}");
        }
    });

    match sye_ui::run_settings_with_autostart(
        config_file.to_path_buf(),
        initial_system_autostart,
        Some(on_autostart_change),
    ) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
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
