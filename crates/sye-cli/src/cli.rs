//! Zero-dependency CLI argument parser for `safe-your-eyes`.

use std::path::PathBuf;

/// Parsed CLI command mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandMode {
    Daemon,
    Status { json: bool },
    Enable,
    Disable { minutes: Option<u64> },
    TakeBreak,
    ShortBreak,
    LongBreak,
    Settings,
    Quit,
    Version,
    Help,
    PrintDefaultConfig,
    ValidateConfig,
    InternalOverlay { payload_json: String },
    InternalSettings,
}

/// Fully parsed CLI invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliArgs {
    pub(crate) config: Option<PathBuf>,
    pub(crate) socket: Option<PathBuf>,
    pub(crate) headless: bool,
    pub(crate) background: bool,
    pub(crate) mode: CommandMode,
}

/// Errors returned while parsing CLI arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliParseError {
    pub(crate) message: String,
}

impl std::fmt::Display for CliParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliParseError {}

/// Usage text printed for `-h` / `--help`.
pub(crate) const HELP_TEXT: &str = "\
Safe Your Eyes — lightweight cross-platform eye-protection app

USAGE:
    safe-your-eyes [OPTIONS]

OPTIONS:
    --config <PATH>           Override TOML config file path
    --socket <PATH>           Override local IPC socket / pipe name
    --headless                Run daemon without tray or GUI overlay subprocesses
    --background              Start daemon silently in the tray without opening Settings
    --status                  Query running daemon status
    --json                    Output --status as JSON
    -e, --enable              Resume/enable running daemon
    -d, --disable [MINUTES]   Pause/disable daemon (until restart if omitted or 0)
    -t, --take-break          Trigger next scheduled break immediately
    -b, --short-break         Trigger immediate short break
    -l, --long-break          Trigger immediate long break
    -s, --settings            Open Cozy Settings window
    -q, --quit                Gracefully stop running daemon
    --print-default-config    Print default TOML configuration to stdout
    --validate-config         Validate configuration file and exit
    -V, --version             Print version information
    -h, --help                Print help information
";

/// Parses CLI arguments (excluding `argv[0]`).
pub(crate) fn parse_args<I, S>(args: I) -> Result<CliArgs, CliParseError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut iter = args.into_iter().map(Into::into).peekable();
    let mut config: Option<PathBuf> = None;
    let mut socket: Option<PathBuf> = None;
    let mut headless = false;
    let mut background = false;
    let mut json_flag = false;
    let mut mode: Option<CommandMode> = None;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(CliArgs {
                    config,
                    socket,
                    headless,
                    background,
                    mode: CommandMode::Help,
                });
            }
            "-V" | "--version" => {
                return Ok(CliArgs {
                    config,
                    socket,
                    headless,
                    background,
                    mode: CommandMode::Version,
                });
            }
            "--headless" => headless = true,
            "--background" => background = true,
            "--json" => json_flag = true,
            "--config" => {
                let value = iter.next().ok_or_else(|| CliParseError {
                    message: "Missing value for --config".to_owned(),
                })?;
                config = Some(parse_non_empty_path("--config", &value)?);
            }
            "--socket" => {
                let value = iter.next().ok_or_else(|| CliParseError {
                    message: "Missing value for --socket".to_owned(),
                })?;
                socket = Some(parse_non_empty_path("--socket", &value)?);
            }
            "--status" => set_mode(&mut mode, CommandMode::Status { json: false })?,
            "-e" | "--enable" => set_mode(&mut mode, CommandMode::Enable)?,
            "-d" | "--disable" => {
                let minutes = if iter.peek().is_some_and(|next| !next.starts_with('-')) {
                    let next = iter.next().unwrap_or_default();
                    Some(parse_disable_minutes(&next)?)
                } else {
                    None
                };
                set_mode(&mut mode, CommandMode::Disable { minutes })?;
            }
            "-t" | "--take-break" => set_mode(&mut mode, CommandMode::TakeBreak)?,
            "-b" | "--short-break" => {
                if matches!(mode, Some(CommandMode::LongBreak)) {
                    return Err(CliParseError {
                        message: "Cannot combine -b/--short-break with -l/--long-break".to_owned(),
                    });
                }
                set_mode(&mut mode, CommandMode::ShortBreak)?;
            }
            "-l" | "--long-break" => {
                if matches!(mode, Some(CommandMode::ShortBreak)) {
                    return Err(CliParseError {
                        message: "Cannot combine -b/--short-break with -l/--long-break".to_owned(),
                    });
                }
                set_mode(&mut mode, CommandMode::LongBreak)?;
            }
            "-s" | "--settings" => set_mode(&mut mode, CommandMode::Settings)?,
            "-q" | "--quit" => set_mode(&mut mode, CommandMode::Quit)?,
            "--print-default-config" => set_mode(&mut mode, CommandMode::PrintDefaultConfig)?,
            "--validate-config" => set_mode(&mut mode, CommandMode::ValidateConfig)?,
            "--internal-overlay" => {
                let payload_json = iter.next().ok_or_else(|| CliParseError {
                    message: "Missing JSON payload for --internal-overlay".to_owned(),
                })?;
                set_mode(&mut mode, CommandMode::InternalOverlay { payload_json })?;
            }
            "--internal-settings" => set_mode(&mut mode, CommandMode::InternalSettings)?,
            // macOS LaunchServices passes a Process Serial Number (`-psn_*`) when launching `.app` bundles.
            other if other.strip_prefix("-psn_").is_some_and(|s| !s.is_empty()) => {}
            other => {
                if let Some(val) = other.strip_prefix("--config=") {
                    config = Some(parse_non_empty_path("--config", val)?);
                } else if let Some(val) = other.strip_prefix("--socket=") {
                    socket = Some(parse_non_empty_path("--socket", val)?);
                } else if let Some(val) = other.strip_prefix("--disable=") {
                    set_mode(
                        &mut mode,
                        CommandMode::Disable {
                            minutes: Some(parse_disable_minutes(val)?),
                        },
                    )?;
                } else {
                    return Err(CliParseError {
                        message: format!("Unknown argument: {other}"),
                    });
                }
            }
        }
    }

    let resolved_mode = match mode {
        Some(CommandMode::Status { .. }) => CommandMode::Status { json: json_flag },
        other => {
            if json_flag {
                return Err(CliParseError {
                    message: "--json can only be used with --status".to_owned(),
                });
            }
            other.unwrap_or(CommandMode::Daemon)
        }
    };

    Ok(CliArgs {
        config,
        socket,
        headless,
        background,
        mode: resolved_mode,
    })
}

fn parse_non_empty_path(flag: &str, value: &str) -> Result<PathBuf, CliParseError> {
    if value.trim().is_empty() {
        return Err(CliParseError {
            message: format!("{flag} path must not be empty"),
        });
    }
    Ok(PathBuf::from(value))
}

fn parse_disable_minutes(raw: &str) -> Result<u64, CliParseError> {
    raw.parse::<u64>().map_err(|_| CliParseError {
        message: format!("Invalid minutes value for --disable: '{raw}'"),
    })
}

fn set_mode(slot: &mut Option<CommandMode>, next: CommandMode) -> Result<(), CliParseError> {
    if slot.is_some() {
        return Err(CliParseError {
            message: "Conflicting command flags specified".to_owned(),
        });
    }
    *slot = Some(next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_combined_short_and_long_break() {
        let err = parse_args(["-b", "-l"]).err();
        assert_eq!(
            err.map(|e| e.message),
            Some("Cannot combine -b/--short-break with -l/--long-break".to_owned())
        );

        let err2 = parse_args(["--long-break", "--short-break"]).err();
        assert_eq!(
            err2.map(|e| e.message),
            Some("Cannot combine -b/--short-break with -l/--long-break".to_owned())
        );
    }

    #[test]
    fn parses_disable_with_and_without_minutes() -> Result<(), CliParseError> {
        let a = parse_args(["--disable"])?;
        assert_eq!(a.mode, CommandMode::Disable { minutes: None });

        let b = parse_args(["-d", "30"])?;
        assert_eq!(b.mode, CommandMode::Disable { minutes: Some(30) });

        let c = parse_args(["--disable", "--socket", "/tmp/a.sock"])?;
        assert_eq!(c.mode, CommandMode::Disable { minutes: None });
        assert_eq!(c.socket, Some(PathBuf::from("/tmp/a.sock")));
        Ok(())
    }

    #[test]
    fn parses_status_json_in_either_order() -> Result<(), CliParseError> {
        let a = parse_args(["--status", "--json"])?;
        assert_eq!(a.mode, CommandMode::Status { json: true });

        let b = parse_args(["--json", "--status"])?;
        assert_eq!(b.mode, CommandMode::Status { json: true });
        Ok(())
    }

    #[test]
    fn parses_background_flag_and_ignores_macos_psn_args() -> Result<(), CliParseError> {
        let bg = parse_args(["--background", "--config", "custom.toml"])?;
        assert!(bg.background);
        assert!(!bg.headless);
        assert_eq!(bg.mode, CommandMode::Daemon);
        assert_eq!(bg.config, Some(PathBuf::from("custom.toml")));

        let psn = parse_args(["-psn_0_123456"])?;
        assert!(!psn.background);
        assert!(!psn.headless);
        assert_eq!(psn.mode, CommandMode::Daemon);

        let combined = parse_args(["-psn_0_987654", "--background"])?;
        assert!(combined.background);
        assert_eq!(combined.mode, CommandMode::Daemon);
        assert!(parse_args(["-psn_"]).is_err());
        assert!(HELP_TEXT.contains("--background"));
        Ok(())
    }
}
