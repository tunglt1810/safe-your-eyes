//! Automatic break policies: `SmartPause`, `DoNotDisturb`, and `LimitConsecutiveSkipping`.

use std::time::Duration;

use crate::config::{DoNotDisturbConfig, SkipLimitConfig, SmartPauseConfig};
use crate::scheduler::SchedulerState;

/// Deferral duration (seconds) when `SmartPauseConfig::postpone_if_active` triggers.
pub const ACTIVE_POSTPONE_SECS: u64 = 2;

/// Decision produced by `SmartPausePolicy::evaluate_idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartPauseDecision {
    None,
    /// User idle reached threshold while waiting; enter `Resting`.
    PauseResting,
    /// User returned before `short_break_interval_secs`; shift next break forward by `credit`.
    ResumeCreditBack {
        credit: Duration,
    },
    /// User was idle at least `short_break_interval_secs`; reset break cycle.
    ResumeResetCycle,
}

/// Tracks user idleness to pause and credit or reset break intervals.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SmartPausePolicy {
    idle_paused: bool,
    idle_origin: Option<Duration>,
}

impl SmartPausePolicy {
    /// Creates a fresh `SmartPausePolicy`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            idle_paused: false,
            idle_origin: None,
        }
    }

    /// Returns whether `SmartPausePolicy` currently owns a `Resting` pause.
    #[must_use]
    pub const fn is_idle_paused(&self) -> bool {
        self.idle_paused
    }

    /// Clears internal idle tracking (called when manually disabled or quitting).
    pub const fn reset(&mut self) {
        self.idle_paused = false;
        self.idle_origin = None;
    }

    /// Evaluates observed OS idle duration against the current scheduler state.
    #[must_use]
    pub fn evaluate_idle(
        &mut self,
        cfg: &SmartPauseConfig,
        state: SchedulerState,
        now: Duration,
        current_idle: Duration,
        short_break_interval_secs: u64,
    ) -> SmartPauseDecision {
        if !cfg.enabled {
            self.reset();
            return SmartPauseDecision::None;
        }

        let threshold = Duration::from_secs(cfg.idle_threshold_secs);

        if state == SchedulerState::Waiting && !self.idle_paused {
            if current_idle >= threshold {
                self.idle_paused = true;
                self.idle_origin = Some(now.saturating_sub(current_idle));
                return SmartPauseDecision::PauseResting;
            }
            return SmartPauseDecision::None;
        }

        if state == SchedulerState::Resting && self.idle_paused {
            if current_idle < threshold {
                let total_idle = self
                    .idle_origin
                    .map_or(Duration::ZERO, |origin| now.saturating_sub(origin));
                self.reset();
                if total_idle < Duration::from_secs(short_break_interval_secs) {
                    return SmartPauseDecision::ResumeCreditBack { credit: total_idle };
                }
                return SmartPauseDecision::ResumeResetCycle;
            }
            return SmartPauseDecision::None;
        }

        SmartPauseDecision::None
    }

    /// Returns a short deferral if `postpone_if_active` is enabled and the user was active
    /// within the last `ACTIVE_POSTPONE_SECS` seconds.
    #[must_use]
    pub fn should_postpone_if_active(
        cfg: &SmartPauseConfig,
        current_idle: Duration,
    ) -> Option<Duration> {
        let limit = Duration::from_secs(ACTIVE_POSTPONE_SECS);
        if cfg.enabled && cfg.postpone_if_active && current_idle < limit {
            Some(limit)
        } else {
            None
        }
    }
}

/// Snapshot of OS activity evaluated by `DoNotDisturbPolicy`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DndContext {
    pub is_fullscreen: bool,
    pub active_window_class: Option<String>,
    pub on_battery: bool,
    pub system_inhibited: bool,
}

/// Reason a break was suppressed by `DoNotDisturbPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DndReason {
    OnBattery,
    SkipWindow,
    Fullscreen,
    SystemInhibited,
}

/// Evaluation outcome of `DoNotDisturbPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DndDecision {
    Allow,
    Suppress(DndReason),
}

impl DndDecision {
    /// Returns `true` when the break should be suppressed.
    #[must_use]
    pub const fn should_suppress(self) -> bool {
        matches!(self, Self::Suppress(_))
    }
}

/// Evaluates fullscreen, window-list, battery, and system-inhibit suppression rules.
#[derive(Debug, Default, Clone, Copy)]
pub struct DoNotDisturbPolicy;

impl DoNotDisturbPolicy {
    /// Evaluates `ctx` against `cfg` using strict `SafeEyes` precedence:
    /// `while_on_battery` -> `skip_break_windows` -> `take_break_windows` -> `is_fullscreen` -> `system_inhibited`.
    #[must_use]
    pub fn evaluate(cfg: &DoNotDisturbConfig, ctx: &DndContext) -> DndDecision {
        if !cfg.enabled {
            return DndDecision::Allow;
        }
        if cfg.while_on_battery && ctx.on_battery {
            return DndDecision::Suppress(DndReason::OnBattery);
        }
        let has_window_rules =
            !cfg.skip_break_windows.is_empty() || !cfg.take_break_windows.is_empty();
        if has_window_rules
            && let Some(window) = ctx.active_window_class.as_deref()
            && !window.trim().is_empty()
        {
            let active_lower = window.to_lowercase();
            if matches_any_window_lower(&active_lower, &cfg.skip_break_windows) {
                return DndDecision::Suppress(DndReason::SkipWindow);
            }
            if matches_any_window_lower(&active_lower, &cfg.take_break_windows) {
                return DndDecision::Allow;
            }
        }
        if ctx.is_fullscreen {
            return DndDecision::Suppress(DndReason::Fullscreen);
        }
        if ctx.system_inhibited {
            return DndDecision::Suppress(DndReason::SystemInhibited);
        }
        DndDecision::Allow
    }
}

fn matches_any_window_lower(active_lower: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        let trimmed = pattern.trim();
        !trimmed.is_empty() && active_lower.contains(&trimmed.to_lowercase())
    })
}

/// Tracks consecutive skipped or postponed breaks and enforces `SkipLimitConfig`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SkipLimitPolicy {
    consecutive_skips: u32,
}

impl SkipLimitPolicy {
    /// Creates a policy initialized with an existing skip count (e.g. from `SessionState`).
    #[must_use]
    pub const fn new(initial_count: u32) -> Self {
        Self {
            consecutive_skips: initial_count,
        }
    }

    /// Returns the current number of consecutive skips or postpones.
    #[must_use]
    pub const fn count(&self) -> u32 {
        self.consecutive_skips
    }

    /// Increments the consecutive skip counter on a skipped break.
    pub const fn record_skip(&mut self) {
        self.consecutive_skips = self.consecutive_skips.saturating_add(1);
    }

    /// Increments the consecutive skip counter on a postponed break.
    pub const fn record_postpone(&mut self) {
        self.consecutive_skips = self.consecutive_skips.saturating_add(1);
    }

    /// Resets the consecutive skip counter after a completed break.
    pub const fn record_completed_break(&mut self) {
        self.consecutive_skips = 0;
    }

    /// Returns `true` when Skip and Postpone controls must be disabled.
    #[must_use]
    pub const fn controls_disabled(&self, cfg: &SkipLimitConfig) -> bool {
        cfg.enabled && self.consecutive_skips >= cfg.max_skips
    }

    /// Returns an optional status note for the break overlay.
    #[must_use]
    pub fn status_label(&self, cfg: &SkipLimitConfig) -> Option<String> {
        if !cfg.enabled {
            return None;
        }
        if self.consecutive_skips == 0 && cfg.max_skips > 0 {
            return None;
        }
        Some(format!(
            "Skipped or postponed {}/{} breaks in a row",
            self.consecutive_skips, cfg.max_skips
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_pause_disabled_is_noop() {
        let mut policy = SmartPausePolicy::new();
        let cfg = SmartPauseConfig {
            enabled: false,
            idle_threshold_secs: 5,
            postpone_if_active: true,
        };
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(100),
                Duration::from_secs(30),
                900,
            ),
            SmartPauseDecision::None
        );
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::ZERO),
            None
        );
    }

    #[test]
    fn smart_pause_enters_resting_and_credits_or_resets() {
        let mut policy = SmartPausePolicy::new();
        assert_eq!(SmartPauseConfig::default().idle_threshold_secs, 300);
        let cfg = SmartPauseConfig {
            idle_threshold_secs: 5,
            ..SmartPauseConfig::default()
        };

        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(100),
                Duration::from_secs(4),
                900,
            ),
            SmartPauseDecision::None
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(105),
                Duration::from_secs(5),
                900,
            ),
            SmartPauseDecision::PauseResting
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(220),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::ResumeCreditBack {
                credit: Duration::from_secs(120),
            }
        );

        // Exact boundary total_idle == short_break_interval_secs -> ResumeResetCycle.
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(500),
                Duration::from_secs(5),
                900,
            ),
            SmartPauseDecision::PauseResting
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(1395),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::ResumeResetCycle
        );
    }

    #[test]
    fn smart_pause_does_not_resume_manual_stop_or_external_resting() {
        let mut policy = SmartPausePolicy::new();
        let cfg = SmartPauseConfig::default();

        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(50),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::None
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Stopped,
                Duration::from_secs(50),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::None
        );

        // Reset prevents stale resume after manual disable.
        let _ = policy.evaluate_idle(
            &cfg,
            SchedulerState::Waiting,
            Duration::from_secs(100),
            Duration::from_secs(10),
            900,
        );
        policy.reset();
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(120),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::None
        );
    }

    #[test]
    fn smart_pause_postpone_if_active_and_no_underflow() {
        let mut policy = SmartPausePolicy::new();
        let cfg = SmartPauseConfig {
            enabled: true,
            idle_threshold_secs: 5,
            postpone_if_active: true,
        };
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::from_secs(0)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::from_secs(1)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::from_secs(2)),
            None
        );
        // now < current_idle does not underflow.
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::ZERO,
                Duration::from_secs(10),
                900,
            ),
            SmartPauseDecision::PauseResting
        );
    }

    #[test]
    fn dnd_precedence_and_matching_rules() {
        let disabled = DoNotDisturbConfig {
            enabled: false,
            while_on_battery: true,
            skip_break_windows: vec!["vlc".to_owned()],
            take_break_windows: Vec::new(),
        };
        let full_ctx = DndContext {
            is_fullscreen: true,
            active_window_class: Some("VLC Media Player".to_owned()),
            on_battery: true,
            system_inhibited: true,
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&disabled, &full_ctx),
            DndDecision::Allow
        );

        let cfg = DoNotDisturbConfig {
            enabled: true,
            while_on_battery: true,
            skip_break_windows: vec!["vlc".to_owned(), String::new(), "   ".to_owned()],
            take_break_windows: vec!["vlc".to_owned(), "code".to_owned()],
        };
        // Battery beats window lists.
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &full_ctx),
            DndDecision::Suppress(DndReason::OnBattery)
        );

        // skip_break_windows beats take_break_windows even when not fullscreen.
        let both_lists = DndContext {
            is_fullscreen: false,
            active_window_class: Some("VLC Media Player".to_owned()),
            on_battery: false,
            system_inhibited: false,
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &both_lists),
            DndDecision::Suppress(DndReason::SkipWindow)
        );

        // take_break_windows overrides fullscreen and system_inhibited.
        let allowed_fullscreen = DndContext {
            is_fullscreen: true,
            active_window_class: Some("Code-OSS".to_owned()),
            on_battery: false,
            system_inhibited: true,
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &allowed_fullscreen),
            DndDecision::Allow
        );

        // Empty/whitespace skip patterns do not match Firefox; fullscreen still suppresses.
        let firefox_full = DndContext {
            is_fullscreen: true,
            active_window_class: Some("Firefox".to_owned()),
            on_battery: false,
            system_inhibited: false,
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &firefox_full),
            DndDecision::Suppress(DndReason::Fullscreen)
        );

        // Non-fullscreen + system_inhibited -> SystemInhibited.
        let inhibited = DndContext {
            is_fullscreen: false,
            active_window_class: None,
            on_battery: false,
            system_inhibited: true,
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &inhibited),
            DndDecision::Suppress(DndReason::SystemInhibited)
        );
    }

    #[test]
    fn skip_limit_tracks_locks_and_resets() {
        let disabled = SkipLimitConfig {
            enabled: false,
            max_skips: 2,
        };
        let mut policy = SkipLimitPolicy::new(0);
        for _ in 0..5 {
            policy.record_skip();
        }
        assert!(!policy.controls_disabled(&disabled));
        assert_eq!(policy.status_label(&disabled), None);

        let enabled = SkipLimitConfig {
            enabled: true,
            max_skips: 2,
        };
        let mut policy = SkipLimitPolicy::new(0);
        assert!(!policy.controls_disabled(&enabled));
        assert_eq!(policy.status_label(&enabled), None);

        policy.record_skip();
        assert!(!policy.controls_disabled(&enabled));
        assert_eq!(
            policy.status_label(&enabled).as_deref(),
            Some("Skipped or postponed 1/2 breaks in a row")
        );

        policy.record_postpone();
        assert!(policy.controls_disabled(&enabled));
        assert_eq!(
            policy.status_label(&enabled).as_deref(),
            Some("Skipped or postponed 2/2 breaks in a row")
        );

        policy.record_completed_break();
        assert_eq!(policy.count(), 0);
        assert!(!policy.controls_disabled(&enabled));
        assert_eq!(policy.status_label(&enabled), None);

        // Saturating increment and max_skips == 0 runtime safety.
        let mut max_policy = SkipLimitPolicy::new(u32::MAX);
        max_policy.record_skip();
        assert_eq!(max_policy.count(), u32::MAX);

        let zero_cfg = SkipLimitConfig {
            enabled: true,
            max_skips: 0,
        };
        assert!(SkipLimitPolicy::new(0).controls_disabled(&zero_cfg));
    }
}
