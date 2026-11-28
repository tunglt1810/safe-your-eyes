use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use sye_core::{
    GITHUB_RELEASES_API_URL, GITHUB_RELEASES_LATEST_URL, PlatformError,
    UPDATE_CACHE_SCHEMA_VERSION, UpdateCacheState, UpdateInfo, UpdateStrategy,
    extract_tag_from_release_url, parse_latest_release_json, parse_sha256sums, release_asset_url,
    release_checksums_url, resolve_release_asset_name,
};

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CHECKSUM_BYTES: u64 = 64 * 1024;
const MAX_API_BYTES: u64 = 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 100 * 1024 * 1024;

#[must_use]
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn open_url(url: &str) -> Result<(), PlatformError> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(PlatformError::Io(format!("unsupported URL scheme: {url}")));
    }

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };

    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        crate::hide_child_console(&mut c);
        c
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| PlatformError::Io(format!("failed to open URL {url}: {err}")))
}

#[must_use]
pub fn detect_current_update_strategy() -> UpdateStrategy {
    match crate::paths::resolve_current_exe() {
        Ok(resolved) => detect_update_strategy(&resolved),
        Err(_) => UpdateStrategy::ManualDownload,
    }
}

#[must_use]
pub fn detect_update_strategy(exe_path: &Path) -> UpdateStrategy {
    detect_update_strategy_for(
        exe_path,
        std::env::consts::OS,
        std::env::consts::ARCH,
        is_parent_dir_writable(exe_path),
    )
}

#[must_use]
pub(crate) fn detect_update_strategy_for(
    exe_path: &Path,
    os: &str,
    arch: &str,
    parent_writable: bool,
) -> UpdateStrategy {
    if os == "macos" {
        if is_homebrew_cask_install(exe_path) {
            return UpdateStrategy::HomebrewCask;
        }
        return UpdateStrategy::ManualDownload;
    }

    if !parent_writable {
        return UpdateStrategy::ManualDownload;
    }

    match resolve_release_asset_name(os, arch) {
        Some(asset_name) => UpdateStrategy::SelfUpdate { asset_name },
        None => UpdateStrategy::ManualDownload,
    }
}

#[must_use]
pub(crate) fn is_homebrew_cask_install(exe_path: &Path) -> bool {
    let text = exe_path.to_string_lossy();
    if text.contains("/Caskroom/safe-your-eyes/") {
        return true;
    }
    if text.contains("/Applications/Safe Your Eyes.app/") {
        return Path::new("/opt/homebrew/Caskroom/safe-your-eyes").exists()
            || Path::new("/usr/local/Caskroom/safe-your-eyes").exists();
    }
    false
}

fn is_parent_dir_writable(exe_path: &Path) -> bool {
    let Some(parent) = exe_path.parent() else {
        return false;
    };
    if parent.as_os_str().is_empty() {
        return false;
    }
    let probe = parent.join(format!(".sye-write-probe-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn check_latest_version(current_version: &str) -> Result<String, PlatformError> {
    let user_agent = format!("safe-your-eyes/{current_version}");
    if let Ok(version) = fetch_latest_version_via_api(&user_agent) {
        return Ok(version);
    }
    fetch_latest_version_via_redirect(&user_agent)
}

fn fetch_latest_version_via_api(user_agent: &str) -> Result<String, PlatformError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(CHECK_TIMEOUT))
        .build()
        .into();

    let mut response = agent
        .get(GITHUB_RELEASES_API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", user_agent)
        .call()
        .map_err(|err| PlatformError::Io(format!("GitHub API request failed: {err}")))?;

    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_API_BYTES)
        .read_to_string()
        .map_err(|err| PlatformError::Io(format!("failed reading GitHub API body: {err}")))?;

    parse_latest_release_json(&body).ok_or_else(|| {
        PlatformError::Io("GitHub API did not return a stable release tag".to_owned())
    })
}

fn fetch_latest_version_via_redirect(user_agent: &str) -> Result<String, PlatformError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(CHECK_TIMEOUT))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();

    let response = agent
        .get(GITHUB_RELEASES_LATEST_URL)
        .header("User-Agent", user_agent)
        .call()
        .map_err(|err| PlatformError::Io(format!("GitHub redirect check failed: {err}")))?;

    let location = response
        .headers()
        .get("location")
        .and_then(|val| val.to_str().ok())
        .ok_or_else(|| {
            PlatformError::Io("GitHub latest release redirect missing Location header".to_owned())
        })?;

    extract_tag_from_release_url(location)
        .ok_or_else(|| PlatformError::Io(format!("unable to parse release tag from {location}")))
}

pub fn check_and_cache_update(
    cache_path: &Path,
    current_version: &str,
    now_secs: u64,
) -> Result<Option<UpdateInfo>, PlatformError> {
    let latest = check_latest_version(current_version)?;
    let state = UpdateCacheState {
        schema_version: UPDATE_CACHE_SCHEMA_VERSION,
        checked_at_unix_secs: now_secs,
        latest_version: Some(latest),
    };
    let _ = state.save_to_path(cache_path);
    Ok(state.available_update(current_version))
}

#[must_use]
pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[must_use]
pub fn verify_sha256_hex(bytes: &[u8], expected_hex: &str) -> bool {
    let actual = compute_sha256_hex(bytes);
    let expected = expected_hex.trim();
    if actual.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in actual.bytes().zip(expected.bytes()) {
        diff |= a ^ b.to_ascii_lowercase();
    }
    diff == 0
}

pub fn apply_self_update(
    target_version: &str,
    asset_name: &str,
    current_version: &str,
) -> Result<(), PlatformError> {
    let resolved_exe = crate::paths::resolve_current_exe()
        .map_err(|err| PlatformError::Io(format!("failed to locate current executable: {err}")))?;
    apply_self_update_to_path(target_version, asset_name, current_version, &resolved_exe)
}

pub fn apply_self_update_to_path(
    target_version: &str,
    asset_name: &str,
    current_version: &str,
    target_exe: &Path,
) -> Result<(), PlatformError> {
    let parsed_version = sye_core::SemVer::parse_stable(target_version)
        .ok_or_else(|| PlatformError::Io(format!("invalid release version: {target_version}")))?;
    let clean_version = parsed_version.to_string();
    let user_agent = format!("safe-your-eyes/{current_version}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into();

    let checksums_url = release_checksums_url(&clean_version);
    let mut sum_resp = agent
        .get(&checksums_url)
        .header("User-Agent", &user_agent)
        .call()
        .map_err(|err| PlatformError::Io(format!("failed to download SHA256SUMS.txt: {err}")))?;
    let sum_text = sum_resp
        .body_mut()
        .with_config()
        .limit(MAX_CHECKSUM_BYTES)
        .read_to_string()
        .map_err(|err| PlatformError::Io(format!("failed to read SHA256SUMS.txt: {err}")))?;

    let expected_hash = parse_sha256sums(&sum_text, asset_name).ok_or_else(|| {
        PlatformError::Io(format!(
            "SHA256SUMS.txt did not contain a checksum for {asset_name}"
        ))
    })?;

    let asset_url = release_asset_url(&clean_version, asset_name);
    let mut bin_resp = agent
        .get(&asset_url)
        .header("User-Agent", &user_agent)
        .call()
        .map_err(|err| PlatformError::Io(format!("failed to download {asset_name}: {err}")))?;
    let bytes = bin_resp
        .body_mut()
        .with_config()
        .limit(MAX_BINARY_BYTES)
        .read_to_vec()
        .map_err(|err| PlatformError::Io(format!("failed to read {asset_name}: {err}")))?;

    if !verify_sha256_hex(&bytes, &expected_hash) {
        return Err(PlatformError::Io(format!(
            "SHA-256 verification failed for {asset_name}"
        )));
    }

    replace_executable(target_exe, &bytes)
}

fn backup_executable_path(exe_path: &Path) -> PathBuf {
    let mut os_name = exe_path.as_os_str().to_os_string();
    os_name.push(".old");
    PathBuf::from(os_name)
}

fn staging_executable_path(exe_path: &Path) -> PathBuf {
    let mut os_name = exe_path.as_os_str().to_os_string();
    os_name.push(format!(".update-{}.tmp", std::process::id()));
    PathBuf::from(os_name)
}

pub fn cleanup_stale_old_executable(exe_path: &Path) {
    let old_path = backup_executable_path(exe_path);
    if old_path.exists() {
        let _ = fs::remove_file(&old_path);
    }
}

pub fn cleanup_current_stale_old_executable() {
    if let Ok(resolved) = crate::paths::resolve_current_exe() {
        cleanup_stale_old_executable(&resolved);
    }
}

pub fn replace_executable(current_exe: &Path, new_bytes: &[u8]) -> Result<(), PlatformError> {
    let staging = staging_executable_path(current_exe);
    if let Err(err) = fs::write(&staging, new_bytes) {
        let _ = fs::remove_file(&staging);
        return Err(PlatformError::Io(format!(
            "failed to write staged binary: {err}"
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = fs::metadata(current_exe).map_or(0o755, |m| m.permissions().mode() | 0o755);
        if let Err(err) = fs::set_permissions(&staging, fs::Permissions::from_mode(mode)) {
            let _ = fs::remove_file(&staging);
            return Err(PlatformError::Io(format!(
                "failed to set executable permissions: {err}"
            )));
        }
        if let Err(err) = fs::rename(&staging, current_exe) {
            let _ = fs::remove_file(&staging);
            return Err(PlatformError::Io(format!(
                "failed to replace executable: {err}"
            )));
        }
    }

    #[cfg(windows)]
    {
        let backup = backup_executable_path(current_exe);
        let _ = fs::remove_file(&backup);
        if let Err(err) = fs::rename(current_exe, &backup) {
            let _ = fs::remove_file(&staging);
            return Err(PlatformError::Io(format!(
                "failed to stage previous executable backup: {err}"
            )));
        }
        if let Err(err) = fs::rename(&staging, current_exe) {
            let _ = fs::rename(&backup, current_exe);
            let _ = fs::remove_file(&staging);
            return Err(PlatformError::Io(format!(
                "failed to move new executable into place: {err}"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestTempDir;

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            compute_sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            compute_sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(verify_sha256_hex(
            b"abc",
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        ));
        assert!(!verify_sha256_hex(
            b"abd",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ));
    }

    #[test]
    fn detects_update_strategy_across_platforms() {
        assert_eq!(
            detect_update_strategy_for(
                Path::new("/home/user/.local/bin/safe-your-eyes"),
                "linux",
                "x86_64",
                true
            ),
            UpdateStrategy::SelfUpdate {
                asset_name: "safe-your-eyes-linux-x86_64"
            }
        );
        assert_eq!(
            detect_update_strategy_for(
                Path::new("/usr/local/bin/safe-your-eyes"),
                "linux",
                "x86_64",
                false
            ),
            UpdateStrategy::ManualDownload
        );
        assert_eq!(
            detect_update_strategy_for(
                Path::new(r"C:\Users\user\safe-your-eyes.exe"),
                "windows",
                "x86_64",
                true
            ),
            UpdateStrategy::SelfUpdate {
                asset_name: "safe-your-eyes-windows-x86_64.exe"
            }
        );
        assert_eq!(
            detect_update_strategy_for(
                Path::new(
                    "/opt/homebrew/Caskroom/safe-your-eyes/0.1.9/Safe Your Eyes.app/Contents/MacOS/safe-your-eyes"
                ),
                "macos",
                "aarch64",
                true
            ),
            UpdateStrategy::HomebrewCask
        );
        assert_eq!(
            detect_update_strategy_for(
                Path::new("/Users/user/Downloads/Safe Your Eyes.app/Contents/MacOS/safe-your-eyes"),
                "macos",
                "aarch64",
                true
            ),
            UpdateStrategy::ManualDownload
        );
    }

    #[test]
    fn replace_executable_and_stale_backup_cleanup_work() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = TestTempDir::new("replace-exe")?;
        let exe = dir.path().join("safe-your-eyes");
        fs::write(&exe, b"old-binary")?;

        replace_executable(&exe, b"new-binary-payload")?;
        assert_eq!(fs::read(&exe)?, b"new-binary-payload");

        let old_backup = backup_executable_path(&exe);
        fs::write(&old_backup, b"stale")?;
        assert!(old_backup.exists());
        cleanup_stale_old_executable(&exe);
        assert!(!old_backup.exists());

        Ok(())
    }

    #[test]
    fn open_url_rejects_non_http_schemes() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
    }
}
