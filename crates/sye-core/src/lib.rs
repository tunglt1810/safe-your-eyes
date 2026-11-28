use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub mod chime;
pub mod config;
pub mod i18n;
pub mod ipc;
pub mod policies;
pub mod queue;
mod rng;
pub mod scheduler;
pub mod session;
pub mod traits;
pub mod update;

/// Shared Cozy design tokens referenced by both `sye-platform` (native banners, theme reveal)
/// and `sye-ui` (Settings, break overlay).
pub mod cozy {
    pub const STANDARD_BEZIER: (f32, f32, f32, f32) = (0.4, 0.0, 0.2, 1.0);

    pub const LIGHT_SURFACE: u32 = 0xFBF1E1;
    pub const LIGHT_BORDER_EMPHASIZED: u32 = 0xDFCBB5;
    pub const LIGHT_TEXT_PRIMARY: u32 = 0x261101;
    pub const LIGHT_TEXT_SECONDARY: u32 = 0x5B4C3B;
    pub const LIGHT_WARNING_SOFT: u32 = 0xFAE1B5;
    pub const LIGHT_YELLOW_BORDER: u32 = 0xE5C27D;
    pub const LIGHT_YELLOW_TEXT: u32 = 0x5E3904;
    pub const LIGHT_ACCENT: u32 = 0xEF8F63;
    pub const LIGHT_FOCUS_RING: u32 = 0xA84A24;
    pub const LIGHT_ON_ACCENT: u32 = 0x261101;
    pub const LIGHT_TRACK: u32 = 0xE6D2BC;

    pub const DARK_SURFACE: u32 = 0x231D16;
    pub const DARK_BORDER_EMPHASIZED: u32 = 0x52493F;
    pub const DARK_TEXT_PRIMARY: u32 = 0xFFFEF7;
    pub const DARK_TEXT_SECONDARY: u32 = 0xC2B7B1;
    pub const DARK_WARNING_SOFT: u32 = 0x443A26;
    pub const DARK_YELLOW_BORDER: u32 = 0xC99A44;
    pub const DARK_YELLOW_TEXT: u32 = 0xFCE09B;
    pub const DARK_ACCENT: u32 = 0xEE8F64;
    pub const DARK_FOCUS_RING: u32 = 0xF2A57E;
    pub const DARK_ON_ACCENT: u32 = 0x18140E;
    pub const DARK_TRACK: u32 = 0x30271E;
}

/// Decodes a 5-byte `(run_len, r, g, b, a)` RLE icon stream generated at build time.
#[must_use]
pub fn decode_rle_rgba(encoded: &[u8], expected_len: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(expected_len);
    for &[count, r, g, b, a] in encoded.as_chunks::<5>().0 {
        let rgba = [r, g, b, a];
        for _ in 0..usize::from(count) {
            pixels.extend_from_slice(&rgba);
        }
    }
    pixels.resize(expected_len, 0);
    pixels
}

pub(crate) fn write_string_atomic(path: &Path, content: &str) -> io::Result<()> {
    static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let pid = std::process::id();
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(".{pid}.{seq}.tmp"));
    let tmp_path = PathBuf::from(tmp_name);
    if let Err(err) = fs::write(&tmp_path, content) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

pub use chime::{
    CHIME_BITS_PER_SAMPLE, CHIME_CHANNELS, CHIME_SAMPLE_RATE, ChimeKind, synthesize_chime_wav,
};
pub use config::{
    AudibleAlertConfig, BreakSpec, BreakType, CONFIG_SCHEMA_VERSION, Config, ConfigError,
    DEFAULT_LONG_BREAK_NAMES, DEFAULT_SHORT_BREAK_NAMES, DoNotDisturbConfig, SkipLimitConfig,
    SmartPauseConfig, ThemeMode, ValidationError,
};
pub use i18n::{LanguageMode, Locale, localize_exercise_name};
pub use ipc::{
    IpcCodecError, IpcCommand, IpcErrorCode, IpcResponse, MAX_IPC_LINE_BYTES, decode_command,
    decode_response, encode_line,
};
pub use policies::{
    ACTIVE_POSTPONE_SECS, DndContext, DndDecision, DndReason, DoNotDisturbPolicy, SkipLimitPolicy,
    SmartPauseDecision, SmartPausePolicy, is_excluded_game_process, is_gaming_candidate,
    is_gaming_context,
};
pub use queue::{BreakQueue, QueuedBreak, ResolvedBreak};
pub use scheduler::{
    OverlayOutcome, OverlayRequest, Scheduler, SchedulerAction, SchedulerEvent, SchedulerState,
    StatusSnapshot, TraySnapshot,
};
pub use session::{SESSION_SCHEMA_VERSION, SessionError, SessionState};
pub use traits::{Clock, DndQueryOptions, FakeClock, PlatformError};
pub use update::{
    GITHUB_RELEASE_DOWNLOAD_BASE, GITHUB_RELEASES_API_URL, GITHUB_RELEASES_LATEST_URL,
    HOMEBREW_UPGRADE_COMMAND, PUBLIC_DOWNLOAD_URL, SemVer, UPDATE_CACHE_SCHEMA_VERSION,
    UPDATE_CHECK_INTERVAL_SECS, UPDATE_INITIAL_DELAY_SECS, UpdateCacheError, UpdateCacheState,
    UpdateInfo, UpdateStrategy, extract_tag_from_release_url, is_newer_version,
    parse_latest_release_json, parse_sha256sums, release_asset_url, release_checksums_url,
    resolve_release_asset_name,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_string_atomic_overwrites_and_cleans_temp_file_on_rename_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join(format!("sye-core-atomic-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;

        let file = dir.join("config.toml");
        write_string_atomic(&file, "first")?;
        write_string_atomic(&file, "second")?;
        assert_eq!(fs::read_to_string(&file)?, "second");

        // Renaming a regular file over an existing directory fails on all platforms;
        // verify the temporary file is removed on error.
        let blocking_dir = dir.join("blocked");
        fs::create_dir_all(&blocking_dir)?;
        assert!(write_string_atomic(&blocking_dir, "should-fail").is_err());

        let leftovers: Vec<_> = fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }
}
