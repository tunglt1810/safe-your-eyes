//! Pure-Rust core library for `safe-your-eyes`.
//!
//! Contains the dual-tier `BreakQueue`, deterministic 7-state `Scheduler`, automatic policies
//! (`SmartPausePolicy`, `DoNotDisturbPolicy`, `SkipLimitPolicy`), TOML `Config` and
//! `SessionState`, pure-Rust WAV chime synthesizer, typed NDJSON IPC protocol, and platform
//! adapter trait boundaries.

pub mod chime;
pub mod config;
pub mod ipc;
pub mod policies;
pub mod queue;
mod rng;
pub mod scheduler;
pub mod session;
pub mod traits;

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
    SmartPauseDecision, SmartPausePolicy,
};
pub use queue::{BreakQueue, QueuedBreak, ResolvedBreak};
pub use scheduler::{
    OverlayOutcome, OverlayRequest, Scheduler, SchedulerAction, SchedulerEvent, SchedulerState,
    StatusSnapshot, TraySnapshot,
};
pub use session::{SESSION_SCHEMA_VERSION, SessionError, SessionState};
pub use traits::{
    AudioPlayer, Clock, DndProvider, FakeClock, IdleProvider, Notifier, OverlayLauncher,
    PlatformError,
};
