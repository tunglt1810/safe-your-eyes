use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const PUBLIC_DOWNLOAD_URL: &str = "https://tunglt1810.github.io/safe-your-eyes/";
pub const GITHUB_RELEASES_API_URL: &str =
    "https://api.github.com/repos/tunglt1810/safe-your-eyes/releases/latest";
pub const GITHUB_RELEASES_LATEST_URL: &str =
    "https://github.com/tunglt1810/safe-your-eyes/releases/latest";
pub const GITHUB_RELEASE_DOWNLOAD_BASE: &str =
    "https://github.com/tunglt1810/safe-your-eyes/releases/download";
pub const HOMEBREW_UPGRADE_COMMAND: &str = "brew upgrade --cask safe-your-eyes";

pub const UPDATE_CACHE_SCHEMA_VERSION: u32 = 1;
pub const UPDATE_CHECK_INTERVAL_SECS: u64 = 12 * 60 * 60;
pub const UPDATE_INITIAL_DELAY_SECS: u64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    #[must_use]
    pub fn parse_stable(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let stripped = trimmed.strip_prefix('v').unwrap_or(trimmed);
        if stripped.is_empty() || stripped.contains('-') || stripped.contains('+') {
            return None;
        }
        let mut parts = stripped.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStrategy {
    SelfUpdate { asset_name: &'static str },
    HomebrewCask,
    ManualDownload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateCacheState {
    pub schema_version: u32,
    pub checked_at_unix_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
}

impl Default for UpdateCacheState {
    fn default() -> Self {
        Self {
            schema_version: UPDATE_CACHE_SCHEMA_VERSION,
            checked_at_unix_secs: 0,
            latest_version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCacheError {
    Io { message: String },
    Parse { message: String },
    Serialize { message: String },
    UnsupportedSchemaVersion { found: u32, supported: u32 },
}

impl fmt::Display for UpdateCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { message } => write!(f, "update cache I/O error: {message}"),
            Self::Parse { message } => write!(f, "update cache parse error: {message}"),
            Self::Serialize { message } => write!(f, "update cache serialize error: {message}"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported update cache schema_version {found}; expected {supported}"
            ),
        }
    }
}

impl std::error::Error for UpdateCacheError {}

impl UpdateCacheState {
    pub fn from_toml_str(input: &str) -> Result<Self, UpdateCacheError> {
        let state: Self = toml::from_str(input).map_err(|err| UpdateCacheError::Parse {
            message: err.to_string(),
        })?;
        if state.schema_version != UPDATE_CACHE_SCHEMA_VERSION {
            return Err(UpdateCacheError::UnsupportedSchemaVersion {
                found: state.schema_version,
                supported: UPDATE_CACHE_SCHEMA_VERSION,
            });
        }
        Ok(state)
    }

    pub fn to_toml_string(&self) -> Result<String, UpdateCacheError> {
        toml::to_string_pretty(self).map_err(|err| UpdateCacheError::Serialize {
            message: err.to_string(),
        })
    }

    #[must_use]
    pub fn load_or_default(path: &Path) -> (Self, Option<UpdateCacheError>) {
        match fs::read_to_string(path) {
            Ok(content) => match Self::from_toml_str(&content) {
                Ok(state) => (state, None),
                Err(err) => (Self::default(), Some(err)),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => (Self::default(), None),
            Err(err) => (
                Self::default(),
                Some(UpdateCacheError::Io {
                    message: err.to_string(),
                }),
            ),
        }
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), UpdateCacheError> {
        let encoded = self.to_toml_string()?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| UpdateCacheError::Io {
                message: err.to_string(),
            })?;
        }
        fs::write(path, encoded).map_err(|err| UpdateCacheError::Io {
            message: err.to_string(),
        })
    }

    #[must_use]
    pub fn available_update(&self, current_version: &str) -> Option<UpdateInfo> {
        let latest_raw = self.latest_version.as_deref()?;
        if !is_newer_version(latest_raw, current_version) {
            return None;
        }
        let parsed = SemVer::parse_stable(latest_raw)?;
        Some(UpdateInfo {
            latest_version: parsed.to_string(),
            download_url: PUBLIC_DOWNLOAD_URL.to_owned(),
        })
    }

    #[must_use]
    pub const fn is_check_due(&self, now_unix_secs: u64) -> bool {
        if self.checked_at_unix_secs == 0 {
            return true;
        }
        now_unix_secs.saturating_sub(self.checked_at_unix_secs) >= UPDATE_CHECK_INTERVAL_SECS
    }
}

#[must_use]
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (
        SemVer::parse_stable(candidate),
        SemVer::parse_stable(current),
    ) {
        (Some(cand), Some(cur)) => cand > cur,
        _ => false,
    }
}

#[derive(Deserialize)]
struct GithubLatestRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
}

#[must_use]
pub fn parse_latest_release_json(json: &str) -> Option<String> {
    let release: GithubLatestRelease = serde_json::from_str(json).ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    SemVer::parse_stable(&release.tag_name).map(|ver| ver.to_string())
}

#[must_use]
pub fn extract_tag_from_release_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let (_, tag) = trimmed.rsplit_once("/tag/")?;
    let clean_tag = tag.split(['?', '#']).next()?;
    SemVer::parse_stable(clean_tag).map(|ver| ver.to_string())
}

#[must_use]
pub fn resolve_release_asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("safe-your-eyes-linux-x86_64"),
        ("linux", "aarch64") => Some("safe-your-eyes-linux-aarch64"),
        ("windows", "x86_64") => Some("safe-your-eyes-windows-x86_64.exe"),
        ("windows", "aarch64") => Some("safe-your-eyes-windows-arm64.exe"),
        ("macos", "aarch64") => Some("safe-your-eyes-macos-arm64.zip"),
        ("macos", "x86_64") => Some("safe-your-eyes-macos-x86_64.zip"),
        _ => None,
    }
}

#[must_use]
pub fn release_asset_url(version: &str, asset_name: &str) -> String {
    let clean = version.trim().trim_start_matches('v');
    format!("{GITHUB_RELEASE_DOWNLOAD_BASE}/v{clean}/{asset_name}")
}

#[must_use]
pub fn release_checksums_url(version: &str) -> String {
    release_asset_url(version, "SHA256SUMS.txt")
}

#[must_use]
pub fn parse_sha256sums(contents: &str, asset_name: &str) -> Option<String> {
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(file) = parts.next() else {
            continue;
        };
        let normalized_file = file.trim_start_matches('*');
        if normalized_file == asset_name
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn semver_parsing_and_comparison_reject_prereleases_and_malformed_tags() {
        assert_eq!(
            SemVer::parse_stable("v0.1.9"),
            Some(SemVer {
                major: 0,
                minor: 1,
                patch: 9
            })
        );
        assert_eq!(
            SemVer::parse_stable("1.2.3"),
            Some(SemVer {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(SemVer::parse_stable("v0.2.0-rc.1"), None);
        assert_eq!(SemVer::parse_stable("0.2"), None);
        assert_eq!(SemVer::parse_stable("v1.2.3.4"), None);
        assert_eq!(SemVer::parse_stable(""), None);

        assert!(is_newer_version("v0.1.10", "0.1.9"));
        assert!(is_newer_version("0.2.0", "0.1.9"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("0.1.9", "0.1.9"));
        assert!(!is_newer_version("0.1.8", "0.1.9"));
        assert!(!is_newer_version("v0.2.0-rc.1", "0.1.9"));
    }

    #[test]
    fn parses_github_release_json_and_redirect_url() {
        let stable_json = r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false}"#;
        assert_eq!(
            parse_latest_release_json(stable_json),
            Some("0.2.0".to_owned())
        );

        let pre_json = r#"{"tag_name":"v0.2.0-rc.1","draft":false,"prerelease":true}"#;
        assert_eq!(parse_latest_release_json(pre_json), None);

        let draft_json = r#"{"tag_name":"v0.2.0","draft":true,"prerelease":false}"#;
        assert_eq!(parse_latest_release_json(draft_json), None);

        assert_eq!(
            extract_tag_from_release_url(
                "https://github.com/tunglt1810/safe-your-eyes/releases/tag/v0.2.1"
            ),
            Some("0.2.1".to_owned())
        );
        assert_eq!(
            extract_tag_from_release_url(
                "https://github.com/tunglt1810/safe-your-eyes/releases/tag/v0.2.1?foo=1"
            ),
            Some("0.2.1".to_owned())
        );
        assert_eq!(
            extract_tag_from_release_url(
                "https://github.com/tunglt1810/safe-your-eyes/releases/tag/v0.2.1-beta.1"
            ),
            None
        );
    }

    #[test]
    fn resolves_assets_urls_and_sha256sums() {
        assert_eq!(
            resolve_release_asset_name("linux", "x86_64"),
            Some("safe-your-eyes-linux-x86_64")
        );
        assert_eq!(
            resolve_release_asset_name("linux", "aarch64"),
            Some("safe-your-eyes-linux-aarch64")
        );
        assert_eq!(
            resolve_release_asset_name("windows", "x86_64"),
            Some("safe-your-eyes-windows-x86_64.exe")
        );
        assert_eq!(
            resolve_release_asset_name("windows", "aarch64"),
            Some("safe-your-eyes-windows-arm64.exe")
        );
        assert_eq!(
            resolve_release_asset_name("macos", "aarch64"),
            Some("safe-your-eyes-macos-arm64.zip")
        );
        assert_eq!(
            resolve_release_asset_name("macos", "x86_64"),
            Some("safe-your-eyes-macos-x86_64.zip")
        );
        assert_eq!(resolve_release_asset_name("freebsd", "x86_64"), None);

        assert_eq!(
            release_asset_url("v0.2.0", "safe-your-eyes-linux-x86_64"),
            "https://github.com/tunglt1810/safe-your-eyes/releases/download/v0.2.0/safe-your-eyes-linux-x86_64"
        );
        assert_eq!(
            release_checksums_url("0.2.0"),
            "https://github.com/tunglt1810/safe-your-eyes/releases/download/v0.2.0/SHA256SUMS.txt"
        );

        let sums = "\
ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789  safe-your-eyes-linux-x86_64\n\
0000000000000000000000000000000000000000000000000000000000000001  *safe-your-eyes-windows-x86_64.exe\n\
short_bad_hash  safe-your-eyes-linux-aarch64\n";

        assert_eq!(
            parse_sha256sums(sums, "safe-your-eyes-linux-x86_64").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(
            parse_sha256sums(sums, "safe-your-eyes-windows-x86_64.exe").as_deref(),
            Some("0000000000000000000000000000000000000000000000000000000000000001")
        );
        assert_eq!(parse_sha256sums(sums, "safe-your-eyes-linux-aarch64"), None);
        assert_eq!(parse_sha256sums(sums, "missing-file"), None);
    }

    #[test]
    fn update_cache_round_trip_and_check_due() -> Result<(), Box<dyn std::error::Error>> {
        let repo_tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join("sye-core-update-cache-test");
        fs::create_dir_all(&repo_tmp)?;

        let path = repo_tmp.join("update.toml");
        let state = UpdateCacheState {
            schema_version: UPDATE_CACHE_SCHEMA_VERSION,
            checked_at_unix_secs: 1_700_000_000,
            latest_version: Some("0.2.0".to_owned()),
        };
        state.save_to_path(&path)?;

        let (loaded, err) = UpdateCacheState::load_or_default(&path);
        assert!(err.is_none());
        assert_eq!(loaded, state);

        let info = loaded.available_update("0.1.9");
        assert_eq!(
            info,
            Some(UpdateInfo {
                latest_version: "0.2.0".to_owned(),
                download_url: PUBLIC_DOWNLOAD_URL.to_owned(),
            })
        );
        assert_eq!(loaded.available_update("0.2.0"), None);
        assert_eq!(loaded.available_update("0.2.1"), None);

        assert!(!loaded.is_check_due(1_700_000_000 + UPDATE_CHECK_INTERVAL_SECS - 1));
        assert!(loaded.is_check_due(1_700_000_000 + UPDATE_CHECK_INTERVAL_SECS));
        assert!(UpdateCacheState::default().is_check_due(10));

        let _ = fs::remove_dir_all(&repo_tmp);
        Ok(())
    }
}
