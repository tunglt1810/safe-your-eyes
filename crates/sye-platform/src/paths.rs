//! Cross-platform application path resolution (config, session, IPC socket).

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use sye_core::PlatformError;

const APP_DIR_NAME: &str = "safe-your-eyes";

/// Resolved filesystem paths used by `safe-your-eyes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub session_file: PathBuf,
    pub socket_path: PathBuf,
}

impl AppPaths {
    /// Resolves default OS-standard paths for `safe-your-eyes`.
    pub fn resolve() -> Result<Self, PlatformError> {
        let proj = ProjectDirs::from("", "", APP_DIR_NAME).ok_or_else(|| {
            PlatformError::Unavailable("unable to determine user home directory".to_owned())
        })?;

        let config_file = proj.config_dir().join("config.toml");
        let session_file = proj.data_local_dir().join("session.toml");
        let socket_path = default_socket_path(&proj);

        Ok(Self {
            config_file,
            session_file,
            socket_path,
        })
    }

    /// Resolves paths while applying optional CLI overrides for `--config` and `--socket`.
    ///
    /// When `config_override` is supplied, `session_file` is placed alongside the overridden
    /// config file so isolated runs never touch the user's real session file.
    pub fn with_overrides(
        config_override: Option<PathBuf>,
        socket_override: Option<PathBuf>,
    ) -> Result<Self, PlatformError> {
        match (config_override, socket_override) {
            (Some(config_file), Some(socket_path)) => {
                let session_file = session_path_for_config(&config_file);
                Ok(Self {
                    config_file,
                    session_file,
                    socket_path,
                })
            }
            (config_override, socket_override) => {
                let mut paths = Self::resolve()?;
                if let Some(cfg) = config_override {
                    paths.session_file = session_path_for_config(&cfg);
                    paths.config_file = cfg;
                }
                if let Some(sock) = socket_override {
                    paths.socket_path = sock;
                }
                Ok(paths)
            }
        }
    }
}

/// Computes a sibling `session.toml` path next to an overridden config file.
#[must_use]
pub fn session_path_for_config(config_path: &Path) -> PathBuf {
    match config_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join("session.toml"),
        _ => PathBuf::from("session.toml"),
    }
}

fn default_socket_path(proj: &ProjectDirs) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let _ = proj;
        PathBuf::from(r"\\.\pipe\safe-your-eyes")
    }
    #[cfg(target_os = "macos")]
    {
        proj.cache_dir().join("ipc.sock")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        proj.runtime_dir()
            .unwrap_or_else(|| proj.cache_dir())
            .join("ipc.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestTempDir;

    #[test]
    fn resolve_returns_expected_filenames() -> Result<(), Box<dyn std::error::Error>> {
        let paths = AppPaths::resolve()?;
        assert_eq!(
            paths.config_file.file_name().and_then(|s| s.to_str()),
            Some("config.toml")
        );
        assert_eq!(
            paths.session_file.file_name().and_then(|s| s.to_str()),
            Some("session.toml")
        );
        assert!(!paths.socket_path.as_os_str().is_empty());
        Ok(())
    }

    #[test]
    fn with_overrides_replaces_config_session_and_socket() -> Result<(), Box<dyn std::error::Error>>
    {
        let tmp = TestTempDir::new("paths")?;
        let cfg = tmp.path().join("custom").join("my.toml");
        let sock = tmp.path().join("custom.sock");

        let paths = AppPaths::with_overrides(Some(cfg.clone()), Some(sock.clone()))?;
        assert_eq!(paths.config_file, cfg);
        assert_eq!(
            paths.session_file,
            tmp.path().join("custom").join("session.toml")
        );
        assert_eq!(paths.socket_path, sock);
        Ok(())
    }

    #[test]
    fn session_path_for_bare_filename_uses_relative_session() {
        assert_eq!(
            session_path_for_config(Path::new("config.toml")),
            PathBuf::from("session.toml")
        );
    }
}
