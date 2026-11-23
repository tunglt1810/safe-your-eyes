//! Optional session persistence (`session.toml`) across daemon restarts.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Current supported session schema version.
pub const SESSION_SCHEMA_VERSION: u32 = 1;

/// Persisted scheduler progress when `Config::persist_state` is enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub schema_version: u32,
    pub short_index: usize,
    pub long_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_remaining_secs: Option<u64>,
    pub consecutive_skips: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at_unix_secs: Option<u64>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            short_index: 0,
            long_index: 0,
            long_remaining_secs: None,
            consecutive_skips: 0,
            saved_at_unix_secs: None,
        }
    }
}

/// Errors produced when loading or saving `SessionState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    Io { message: String },
    Parse { message: String },
    Serialize { message: String },
    UnsupportedSchemaVersion { found: u32, supported: u32 },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { message } => write!(f, "session I/O error: {message}"),
            Self::Parse { message } => write!(f, "session parse error: {message}"),
            Self::Serialize { message } => write!(f, "session serialize error: {message}"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported session schema_version {found}; expected {supported}"
            ),
        }
    }
}

impl std::error::Error for SessionError {}

impl SessionState {
    /// Parses and checks schema version of a TOML session string.
    pub fn from_toml_str(input: &str) -> Result<Self, SessionError> {
        let state: Self = toml::from_str(input).map_err(|err| SessionError::Parse {
            message: err.to_string(),
        })?;
        if state.schema_version != SESSION_SCHEMA_VERSION {
            return Err(SessionError::UnsupportedSchemaVersion {
                found: state.schema_version,
                supported: SESSION_SCHEMA_VERSION,
            });
        }
        Ok(state)
    }

    /// Serializes this session state to TOML.
    pub fn to_toml_string(&self) -> Result<String, SessionError> {
        toml::to_string_pretty(self).map_err(|err| SessionError::Serialize {
            message: err.to_string(),
        })
    }

    /// Loads `SessionState` from `path`, falling back to defaults on missing or corrupt files.
    #[must_use]
    pub fn load_or_default(path: &Path) -> (Self, Option<SessionError>) {
        match fs::read_to_string(path) {
            Ok(content) => match Self::from_toml_str(&content) {
                Ok(state) => (state, None),
                Err(err) => (Self::default(), Some(err)),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => (Self::default(), None),
            Err(err) => (
                Self::default(),
                Some(SessionError::Io {
                    message: err.to_string(),
                }),
            ),
        }
    }

    /// Writes `SessionState` to `path`, creating parent directories when needed.
    pub fn save_to_path(&self, path: &Path) -> Result<(), SessionError> {
        let encoded = self.to_toml_string()?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| SessionError::Io {
                message: err.to_string(),
            })?;
        }
        fs::write(path, encoded).map_err(|err| SessionError::Io {
            message: err.to_string(),
        })
    }
}

impl std::str::FromStr for SessionState {
    type Err = SessionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_toml_str(s)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn session_state_toml_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            short_index: 3,
            long_index: 1,
            long_remaining_secs: Some(1800),
            consecutive_skips: 2,
            saved_at_unix_secs: Some(1_700_000_000),
        };
        let encoded = state.to_toml_string()?;
        let decoded = SessionState::from_toml_str(&encoded)?;
        assert_eq!(decoded, state);
        Ok(())
    }

    #[test]
    fn session_state_rejects_wrong_schema_and_falls_back_on_corrupt_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let wrong_version = SessionState::from_toml_str("schema_version = 42\n");
        assert_eq!(
            wrong_version,
            Err(SessionError::UnsupportedSchemaVersion {
                found: 42,
                supported: SESSION_SCHEMA_VERSION,
            })
        );

        let repo_tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join("sye-core-session-test");
        fs::create_dir_all(&repo_tmp)?;

        let missing = repo_tmp.join("missing.toml");
        let (state, err) = SessionState::load_or_default(&missing);
        assert_eq!(state, SessionState::default());
        assert!(err.is_none());

        let corrupt = repo_tmp.join("corrupt.toml");
        fs::write(&corrupt, "short_index = \"not_a_number\"")?;
        let (state, err) = SessionState::load_or_default(&corrupt);
        assert_eq!(state, SessionState::default());
        assert!(matches!(err, Some(SessionError::Parse { .. })));

        let saved = repo_tmp.join("session.toml");
        let custom = SessionState {
            short_index: 2,
            consecutive_skips: 1,
            ..SessionState::default()
        };
        custom.save_to_path(&saved)?;
        let (loaded, load_err) = SessionState::load_or_default(&saved);
        assert!(load_err.is_none());
        assert_eq!(loaded, custom);

        let _ = fs::remove_dir_all(&repo_tmp);
        Ok(())
    }
}
