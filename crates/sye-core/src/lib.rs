pub mod chime;
pub mod config;
pub mod ipc;
pub mod policies;
pub mod queue;
mod rng;
pub mod scheduler;
pub mod session;
pub mod traits;
pub mod update;

pub use chime::{
    CHIME_BITS_PER_SAMPLE, CHIME_CHANNELS, CHIME_SAMPLE_RATE, ChimeKind, synthesize_chime_wav,
};
pub use config::{
    AudibleAlertConfig, BreakSpec, BreakType, CONFIG_SCHEMA_VERSION, Config, ConfigError,
    DEFAULT_LONG_BREAK_NAMES, DEFAULT_SHORT_BREAK_NAMES, DoNotDisturbConfig, SkipLimitConfig,
    SmartPauseConfig, ThemeMode, ValidationError,
};
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
pub use traits::{
    AudioPlayer, Clock, DndProvider, DndQueryOptions, FakeClock, IdleProvider, Notifier,
    OverlayLauncher, PlatformError,
};
pub use update::{
    GITHUB_RELEASE_DOWNLOAD_BASE, GITHUB_RELEASES_API_URL, GITHUB_RELEASES_LATEST_URL,
    HOMEBREW_UPGRADE_COMMAND, PUBLIC_DOWNLOAD_URL, SemVer, UPDATE_CACHE_SCHEMA_VERSION,
    UPDATE_CHECK_INTERVAL_SECS, UPDATE_INITIAL_DELAY_SECS, UpdateCacheError, UpdateCacheState,
    UpdateInfo, UpdateStrategy, extract_tag_from_release_url, is_newer_version,
    parse_latest_release_json, parse_sha256sums, release_asset_url, release_checksums_url,
    resolve_release_asset_name,
};
