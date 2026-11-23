//! Typed NDJSON local IPC protocol between CLI clients and the background daemon.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::BreakType;
use crate::scheduler::StatusSnapshot;

/// Maximum permitted byte length of a single IPC JSON line (16 KiB).
pub const MAX_IPC_LINE_BYTES: usize = 16 * 1024;

/// Command sent from a CLI client to the running daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcCommand {
    Status,
    Enable,
    Disable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minutes: Option<u64>,
    },
    TakeBreak {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<BreakType>,
    },
    OpenSettings,
    Quit,
}

/// Structured error codes returned in `IpcResponse::Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    InvalidRequest,
    UnknownCommand,
    InvalidState,
    InternalError,
}

/// Response sent from the daemon back to a CLI client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Status(StatusSnapshot),
    Error {
        code: IpcErrorCode,
        message: String,
    },
}

/// Errors produced when encoding or decoding IPC lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcCodecError {
    LineTooLong { len: usize, max: usize },
    EmptyLine,
    Serialize(String),
    Deserialize(String),
}

impl fmt::Display for IpcCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong { len, max } => {
                write!(f, "IPC line exceeds limit ({len} > {max} bytes)")
            }
            Self::EmptyLine => write!(f, "IPC line is empty"),
            Self::Serialize(msg) => write!(f, "IPC serialize error: {msg}"),
            Self::Deserialize(msg) => write!(f, "IPC deserialize error: {msg}"),
        }
    }
}

impl std::error::Error for IpcCodecError {}

/// Encodes an IPC message as a compact single-line JSON string terminated with `'\n'`.
pub fn encode_line<T: Serialize>(msg: &T) -> Result<String, IpcCodecError> {
    let mut json =
        serde_json::to_string(msg).map_err(|err| IpcCodecError::Serialize(err.to_string()))?;
    json.push('\n');
    if json.len() > MAX_IPC_LINE_BYTES {
        return Err(IpcCodecError::LineTooLong {
            len: json.len(),
            max: MAX_IPC_LINE_BYTES,
        });
    }
    Ok(json)
}

/// Decodes a single NDJSON line into an `IpcCommand`, returning a structured `IpcResponse::Error`
/// on malformed, oversized, or unknown commands.
pub fn decode_command(line: &str) -> Result<IpcCommand, IpcResponse> {
    if line.len() > MAX_IPC_LINE_BYTES {
        return Err(IpcResponse::Error {
            code: IpcErrorCode::InvalidRequest,
            message: format!("IPC line exceeds {MAX_IPC_LINE_BYTES} bytes"),
        });
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(IpcResponse::Error {
            code: IpcErrorCode::InvalidRequest,
            message: "empty IPC request".to_owned(),
        });
    }

    match serde_json::from_str::<IpcCommand>(trimmed) {
        Ok(cmd) => Ok(cmd),
        Err(err) => {
            let code = classify_command_error(trimmed);
            Err(IpcResponse::Error {
                code,
                message: err.to_string(),
            })
        }
    }
}

/// Decodes a single NDJSON line into an `IpcResponse`.
pub fn decode_response(line: &str) -> Result<IpcResponse, IpcCodecError> {
    if line.len() > MAX_IPC_LINE_BYTES {
        return Err(IpcCodecError::LineTooLong {
            len: line.len(),
            max: MAX_IPC_LINE_BYTES,
        });
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(IpcCodecError::EmptyLine);
    }
    serde_json::from_str(trimmed).map_err(|err| IpcCodecError::Deserialize(err.to_string()))
}

#[derive(Deserialize)]
struct CmdProbe<'a> {
    #[serde(borrow)]
    cmd: &'a str,
}

fn classify_command_error(trimmed: &str) -> IpcErrorCode {
    let Ok(probe) = serde_json::from_str::<CmdProbe<'_>>(trimmed) else {
        return IpcErrorCode::InvalidRequest;
    };
    match probe.cmd {
        "status" | "enable" | "disable" | "take_break" | "open_settings" | "quit" => {
            IpcErrorCode::InvalidRequest
        }
        _ => IpcErrorCode::UnknownCommand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::SchedulerState;

    #[test]
    fn ipc_command_round_trip_all_variants() -> Result<(), Box<dyn std::error::Error>> {
        let commands = [
            IpcCommand::Status,
            IpcCommand::Enable,
            IpcCommand::Disable { minutes: None },
            IpcCommand::Disable { minutes: Some(30) },
            IpcCommand::TakeBreak { kind: None },
            IpcCommand::TakeBreak {
                kind: Some(BreakType::Short),
            },
            IpcCommand::TakeBreak {
                kind: Some(BreakType::Long),
            },
            IpcCommand::OpenSettings,
            IpcCommand::Quit,
        ];

        for cmd in commands {
            let line = encode_line(&cmd)?;
            assert!(line.ends_with('\n'));
            assert_eq!(line.matches('\n').count(), 1);
            let decoded = match decode_command(&line) {
                Ok(c) => c,
                Err(resp) => return Err(format!("unexpected decode error: {resp:?}").into()),
            };
            assert_eq!(decoded, cmd);
        }
        Ok(())
    }

    #[test]
    fn ipc_status_snapshot_matches_cli_json_contract() -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = StatusSnapshot {
            state: SchedulerState::Waiting,
            enabled: true,
            next_break_type: Some(BreakType::Short),
            next_break_name: Some("Blink your eyes".to_owned()),
            seconds_until_next_break: 420,
            consecutive_skips: 1,
        };
        let value = serde_json::to_value(&snapshot)?;
        assert_eq!(value["state"], "waiting");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["next_break_type"], "short");
        assert_eq!(value["next_break_name"], "Blink your eyes");
        assert_eq!(value["seconds_until_next_break"], 420);
        assert_eq!(value["consecutive_skips"], 1);

        let line = encode_line(&IpcResponse::Status(snapshot.clone()))?;
        let decoded = decode_response(&line)?;
        assert_eq!(decoded, IpcResponse::Status(snapshot));
        Ok(())
    }

    #[test]
    fn ipc_decode_handles_unknown_malformed_and_oversized_lines() {
        let unknown = decode_command(r#"{"cmd":"reboot"}"#);
        assert!(matches!(
            unknown,
            Err(IpcResponse::Error {
                code: IpcErrorCode::UnknownCommand,
                ..
            })
        ));

        let malformed = decode_command("{bad json");
        assert!(matches!(
            malformed,
            Err(IpcResponse::Error {
                code: IpcErrorCode::InvalidRequest,
                ..
            })
        ));

        let oversized = "a".repeat(MAX_IPC_LINE_BYTES + 1);
        assert!(matches!(
            decode_command(&oversized),
            Err(IpcResponse::Error {
                code: IpcErrorCode::InvalidRequest,
                ..
            })
        ));
        assert!(matches!(
            decode_response(&oversized),
            Err(IpcCodecError::LineTooLong { .. })
        ));
    }
}
