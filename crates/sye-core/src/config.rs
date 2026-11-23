//! TOML configuration schema, defaults, validation, and file I/O.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Current supported configuration schema version.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Default short break exercises inspired by `SafeEyes` (13 exercises).
pub const DEFAULT_SHORT_BREAK_NAMES: [&str; 13] = [
    "Gently close your eyes",
    "Take five slow, deep breaths",
    "Rotate your eyes in clockwise direction",
    "Relax your shoulders and drop them away from your ears",
    "Blink your eyes",
    "Stretch your neck gently from side to side",
    "Roll your eyes a few times to each side",
    "Check your posture and sit back comfortably",
    "Rotate your eyes in counterclockwise direction",
    "Unclench your jaw and relax your face",
    "Focus on a point in the far distance",
    "Stretch your wrists and fingers",
    "Have some water",
];

/// Default long break exercises inspired by `SafeEyes` (6 exercises).
pub const DEFAULT_LONG_BREAK_NAMES: [&str; 6] = [
    "Stand up and stretch your back and legs",
    "Lean back at your seat and relax",
    "Walk around and refill your water",
    "Walk for a while",
    "Step away from the screen and do a short mobility stretch",
    "Rest your eyes and focus on distant objects",
];

/// Break tier classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakType {
    Short,
    Long,
}

/// Configurable break exercise with optional per-break timing overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
}

impl BreakSpec {
    /// Creates a break specification using tier-default timing.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            interval_secs: None,
            duration_secs: None,
        }
    }
}

/// `SmartPause` policy configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmartPauseConfig {
    pub enabled: bool,
    pub idle_threshold_secs: u64,
    pub postpone_if_active: bool,
}

impl Default for SmartPauseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_threshold_secs: 5,
            postpone_if_active: false,
        }
    }
}

/// `DoNotDisturb` policy configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoNotDisturbConfig {
    pub enabled: bool,
    pub skip_break_windows: Vec<String>,
    pub take_break_windows: Vec<String>,
    pub while_on_battery: bool,
}

impl Default for DoNotDisturbConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            skip_break_windows: Vec::new(),
            take_break_windows: Vec::new(),
            while_on_battery: false,
        }
    }
}

/// Audible alert configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudibleAlertConfig {
    pub pre_break: bool,
    pub post_break: bool,
    pub volume: u8,
}

impl Default for AudibleAlertConfig {
    fn default() -> Self {
        Self {
            pre_break: false,
            post_break: true,
            volume: 100,
        }
    }
}

/// `LimitConsecutiveSkipping` policy configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkipLimitConfig {
    pub enabled: bool,
    pub max_skips: u32,
}

impl Default for SkipLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_skips: 2,
        }
    }
}

/// Root application configuration serialized to TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub short_break_interval_secs: u64,
    pub short_break_duration_secs: u64,
    pub long_break_interval_secs: u64,
    pub long_break_duration_secs: u64,
    pub pre_break_warning_secs: u64,
    pub strict_break: bool,
    pub allow_postpone: bool,
    pub postpone_duration_secs: u64,
    pub random_order: bool,
    pub shortcut_disable_secs: u64,
    pub fade_in_ms: u64,
    pub persist_state: bool,
    pub autostart: bool,
    pub smart_pause: SmartPauseConfig,
    pub do_not_disturb: DoNotDisturbConfig,
    pub audible_alert: AudibleAlertConfig,
    pub limit_consecutive_skips: SkipLimitConfig,
    pub short_breaks: Vec<BreakSpec>,
    pub long_breaks: Vec<BreakSpec>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            short_break_interval_secs: 900,
            short_break_duration_secs: 15,
            long_break_interval_secs: 4500,
            long_break_duration_secs: 60,
            pre_break_warning_secs: 10,
            strict_break: false,
            allow_postpone: false,
            postpone_duration_secs: 300,
            random_order: true,
            shortcut_disable_secs: 2,
            fade_in_ms: 1500,
            persist_state: false,
            autostart: false,
            smart_pause: SmartPauseConfig::default(),
            do_not_disturb: DoNotDisturbConfig::default(),
            audible_alert: AudibleAlertConfig::default(),
            limit_consecutive_skips: SkipLimitConfig::default(),
            short_breaks: DEFAULT_SHORT_BREAK_NAMES
                .iter()
                .map(|name| BreakSpec::new(*name))
                .collect(),
            long_breaks: DEFAULT_LONG_BREAK_NAMES
                .iter()
                .map(|name| BreakSpec::new(*name))
                .collect(),
        }
    }
}

/// Single configuration field validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Errors produced when loading, validating, or saving `Config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Io { message: String },
    Parse { message: String },
    Serialize { message: String },
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    Validation(Vec<ValidationError>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { message } => write!(f, "config I/O error: {message}"),
            Self::Parse { message } => write!(f, "config parse error: {message}"),
            Self::Serialize { message } => write!(f, "config serialize error: {message}"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported config schema_version {found}; expected {supported}"
            ),
            Self::Validation(errors) => {
                write!(f, "config validation failed: ")?;
                for (idx, err) in errors.iter().enumerate() {
                    if idx > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{err}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Returns a fresh default configuration.
    #[must_use]
    pub fn reset_to_default() -> Self {
        Self::default()
    }

    /// Validates all configuration invariants.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: CONFIG_SCHEMA_VERSION,
            });
        }

        let mut errors = Vec::new();

        for (field, value) in [
            ("short_break_interval_secs", self.short_break_interval_secs),
            ("short_break_duration_secs", self.short_break_duration_secs),
            ("long_break_interval_secs", self.long_break_interval_secs),
            ("long_break_duration_secs", self.long_break_duration_secs),
            ("postpone_duration_secs", self.postpone_duration_secs),
        ] {
            if value == 0 {
                errors.push(ValidationError {
                    field: field.to_owned(),
                    message: "must be at least 1".to_owned(),
                });
            }
        }
        if self.short_break_interval_secs > 0
            && self.pre_break_warning_secs >= self.short_break_interval_secs
        {
            errors.push(ValidationError {
                field: "pre_break_warning_secs".to_owned(),
                message: "must be less than short_break_interval_secs".to_owned(),
            });
        }
        if self.audible_alert.volume > 100 {
            errors.push(ValidationError {
                field: "audible_alert.volume".to_owned(),
                message: "must be between 0 and 100".to_owned(),
            });
        }
        if self.smart_pause.idle_threshold_secs == 0 {
            errors.push(ValidationError {
                field: "smart_pause.idle_threshold_secs".to_owned(),
                message: "must be at least 1".to_owned(),
            });
        }
        if self.limit_consecutive_skips.enabled && self.limit_consecutive_skips.max_skips == 0 {
            errors.push(ValidationError {
                field: "limit_consecutive_skips.max_skips".to_owned(),
                message: "must be at least 1 when limit_consecutive_skips is enabled".to_owned(),
            });
        }

        validate_break_list("short_breaks", &self.short_breaks, &mut errors);
        validate_break_list("long_breaks", &self.long_breaks, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(errors))
        }
    }

    /// Parses and validates a configuration from a TOML string.
    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        let cfg: Self = toml::from_str(input).map_err(|err| ConfigError::Parse {
            message: err.to_string(),
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Serializes this configuration to a human-readable TOML string.
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|err| ConfigError::Serialize {
            message: err.to_string(),
        })
    }

    /// Loads and validates a configuration file from disk.
    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path).map_err(|err| ConfigError::Io {
            message: err.to_string(),
        })?;
        Self::from_toml_str(&content)
    }

    /// Loads configuration from `path`, falling back to `Config::default()` if missing or invalid.
    ///
    /// Returns `(Config::default(), None)` when the file does not exist, and
    /// `(Config::default(), Some(err))` when reading, parsing, or validation fails.
    #[must_use]
    pub fn load_or_default(path: &Path) -> (Self, Option<ConfigError>) {
        match fs::read_to_string(path) {
            Ok(content) => match Self::from_toml_str(&content) {
                Ok(cfg) => (cfg, None),
                Err(err) => (Self::default(), Some(err)),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => (Self::default(), None),
            Err(err) => (
                Self::default(),
                Some(ConfigError::Io {
                    message: err.to_string(),
                }),
            ),
        }
    }

    /// Writes this configuration to `path`, creating parent directories when needed.
    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let encoded = self.to_toml_string()?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| ConfigError::Io {
                message: err.to_string(),
            })?;
        }
        fs::write(path, encoded).map_err(|err| ConfigError::Io {
            message: err.to_string(),
        })
    }
}

impl std::str::FromStr for Config {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_toml_str(s)
    }
}

fn validate_break_list(list_name: &str, breaks: &[BreakSpec], errors: &mut Vec<ValidationError>) {
    for (idx, item) in breaks.iter().enumerate() {
        if item.name.trim().is_empty() {
            errors.push(ValidationError {
                field: format!("{list_name}[{idx}].name"),
                message: "must not be empty".to_owned(),
            });
        }
        if item.interval_secs == Some(0) {
            errors.push(ValidationError {
                field: format!("{list_name}[{idx}].interval_secs"),
                message: "must be at least 1".to_owned(),
            });
        }
        if item.duration_secs == Some(0) {
            errors.push(ValidationError {
                field: format!("{list_name}[{idx}].duration_secs"),
                message: "must be at least 1".to_owned(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn default_config_is_valid_and_has_expected_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(cfg.short_break_interval_secs, 900);
        assert_eq!(cfg.short_break_duration_secs, 15);
        assert_eq!(cfg.long_break_interval_secs, 4500);
        assert_eq!(cfg.long_break_duration_secs, 60);
        assert_eq!(cfg.pre_break_warning_secs, 10);
        assert!(!cfg.strict_break);
        assert!(!cfg.allow_postpone);
        assert_eq!(cfg.postpone_duration_secs, 300);
        assert!(cfg.random_order);
        assert_eq!(cfg.shortcut_disable_secs, 2);
        assert_eq!(cfg.fade_in_ms, 1500);
        assert!(!cfg.persist_state);
        assert!(!cfg.autostart);
        assert_eq!(cfg.short_breaks.len(), 13);
        assert_eq!(cfg.long_breaks.len(), 6);
        assert_eq!(cfg.validate(), Ok(()));
        assert_eq!(Config::reset_to_default(), cfg);
    }

    #[test]
    fn config_toml_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let cfg = Config::default();
        let toml_text = cfg.to_toml_string()?;
        let parsed = Config::from_toml_str(&toml_text)?;
        assert_eq!(parsed, cfg);
        Ok(())
    }

    #[test]
    fn partial_toml_uses_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let parsed = Config::from_toml_str("short_break_interval_secs = 600\n")?;
        assert_eq!(parsed.short_break_interval_secs, 600);
        assert_eq!(parsed.long_break_interval_secs, 4500);
        assert_eq!(parsed.short_breaks.len(), 13);
        assert_eq!(parsed.long_breaks.len(), 6);
        Ok(())
    }

    #[test]
    fn invalid_toml_and_negative_values_return_parse_error() {
        let malformed = Config::from_toml_str("short_break_interval_secs = [invalid");
        assert!(matches!(malformed, Err(ConfigError::Parse { .. })));

        let negative = Config::from_toml_str("short_break_interval_secs = -5\n");
        assert!(matches!(negative, Err(ConfigError::Parse { .. })));
    }

    #[test]
    fn validation_rejects_invalid_fields_and_schema_version()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut cfg = Config {
            schema_version: 99,
            ..Config::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigError::UnsupportedSchemaVersion {
                found: 99,
                supported: CONFIG_SCHEMA_VERSION,
            })
        );

        cfg = Config {
            short_break_interval_secs: 0,
            short_break_duration_secs: 0,
            long_break_interval_secs: 0,
            long_break_duration_secs: 0,
            postpone_duration_secs: 0,
            audible_alert: AudibleAlertConfig {
                pre_break: false,
                post_break: true,
                volume: 101,
            },
            smart_pause: SmartPauseConfig {
                enabled: true,
                idle_threshold_secs: 0,
                postpone_if_active: false,
            },
            limit_consecutive_skips: SkipLimitConfig {
                enabled: true,
                max_skips: 0,
            },
            short_breaks: vec![BreakSpec {
                name: "   ".to_owned(),
                interval_secs: Some(0),
                duration_secs: Some(0),
            }],
            ..Config::default()
        };

        let Err(ConfigError::Validation(errs)) = cfg.validate() else {
            return Err("expected validation error".into());
        };
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"short_break_interval_secs"));
        assert!(fields.contains(&"short_break_duration_secs"));
        assert!(fields.contains(&"long_break_interval_secs"));
        assert!(fields.contains(&"long_break_duration_secs"));
        assert!(fields.contains(&"postpone_duration_secs"));
        assert!(fields.contains(&"audible_alert.volume"));
        assert!(fields.contains(&"smart_pause.idle_threshold_secs"));
        assert!(fields.contains(&"limit_consecutive_skips.max_skips"));
        assert!(fields.contains(&"short_breaks[0].name"));
        assert!(fields.contains(&"short_breaks[0].interval_secs"));
        assert!(fields.contains(&"short_breaks[0].duration_secs"));
        Ok(())
    }

    #[test]
    fn empty_break_lists_and_zero_warning_are_valid() {
        let cfg = Config {
            pre_break_warning_secs: 0,
            shortcut_disable_secs: 0,
            fade_in_ms: 0,
            short_breaks: Vec::new(),
            long_breaks: Vec::new(),
            ..Config::default()
        };
        assert_eq!(cfg.validate(), Ok(()));
    }

    #[test]
    fn load_or_default_handles_missing_and_corrupted_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let repo_tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join("sye-core-config-test");
        fs::create_dir_all(&repo_tmp)?;

        let missing = repo_tmp.join("does_not_exist.toml");
        let (cfg, err) = Config::load_or_default(&missing);
        assert_eq!(cfg, Config::default());
        assert!(err.is_none());

        let corrupt = repo_tmp.join("corrupt.toml");
        fs::write(&corrupt, "short_break_interval_secs = ???")?;
        let (cfg, err) = Config::load_or_default(&corrupt);
        assert_eq!(cfg, Config::default());
        assert!(matches!(err, Some(ConfigError::Parse { .. })));

        let saved = repo_tmp.join("saved.toml");
        let custom = Config {
            short_break_interval_secs: 600,
            ..Config::default()
        };
        custom.save_to_path(&saved)?;
        let loaded = Config::load_from_path(&saved)?;
        assert_eq!(loaded, custom);

        let _ = fs::remove_dir_all(&repo_tmp);
        Ok(())
    }
}
