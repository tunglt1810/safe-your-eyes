//! Deterministic 7-state break scheduler integrating `BreakQueue` and F4–F9 policies.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::chime::ChimeKind;
use crate::config::{BreakType, Config};
use crate::policies::{
    DndContext, DoNotDisturbPolicy, SkipLimitPolicy, SmartPauseDecision, SmartPausePolicy,
};
use crate::queue::{BreakQueue, ResolvedBreak};
use crate::session::{SESSION_SCHEMA_VERSION, SessionState};
use crate::traits::Clock;

/// Seven deterministic states of `Scheduler`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerState {
    Start,
    Waiting,
    PreBreak,
    Break,
    Stopped,
    Quit,
    Resting,
}

/// Payload passed to `sye-ui` when displaying a fullscreen break overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayRequest {
    pub break_type: BreakType,
    pub title: String,
    pub duration_secs: u64,
    pub strict_break: bool,
    pub allow_postpone: bool,
    pub postpone_duration_secs: u64,
    pub shortcut_disable_secs: u64,
    pub fade_in_ms: u64,
    pub status_note: Option<String>,
}

/// Outcome returned by the break overlay subprocess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum OverlayOutcome {
    Completed,
    Skipped,
    Postponed { duration_secs: u64 },
    Closed,
}

/// Snapshot used by `--status --json` and IPC `IpcResponse::Status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub state: SchedulerState,
    pub enabled: bool,
    pub next_break_type: Option<BreakType>,
    pub next_break_name: Option<String>,
    pub seconds_until_next_break: u64,
    pub consecutive_skips: u32,
}

/// Snapshot used by `sye-platform` to refresh system tray icon, tooltip, and menu state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraySnapshot {
    pub state: SchedulerState,
    pub enabled: bool,
    pub has_breaks: bool,
    pub menu_locked: bool,
    pub next_break_type: Option<BreakType>,
    pub next_break_name: Option<String>,
    pub seconds_until_next_break: u64,
    pub disabled_until_unix_secs: Option<u64>,
    pub consecutive_skips: u32,
}

/// Input events processed synchronously by `Scheduler::handle_event`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerEvent {
    Start,
    Tick,
    Enable,
    Disable { duration_secs: Option<u64> },
    EnterResting,
    ExitResting { credit_idle_secs: Option<u64> },
    IdleObserved(Duration),
    TakeBreak { kind: Option<BreakType> },
    OverlayFinished(OverlayOutcome),
    DeferBreak { delay_secs: u64 },
    CancelCurrentBreak,
    ReloadConfig(Config),
    Quit,
}

/// Pure side-effect commands emitted by `Scheduler` for the host loop to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerAction {
    NotifyPreBreak {
        break_spec: ResolvedBreak,
        warning_secs: u64,
    },
    DismissNotification,
    PlayChime {
        kind: ChimeKind,
        volume: u8,
    },
    ShowOverlay(OverlayRequest),
    CloseOverlay,
    UpdateTray(TraySnapshot),
    PersistSession(SessionState),
}

/// Pure, deterministic break scheduler parameterized by a `Clock`.
#[derive(Debug)]
pub struct Scheduler<C: Clock> {
    config: Config,
    clock: C,
    rng_seed: u64,
    queue: Option<BreakQueue>,
    state: SchedulerState,
    smart_pause: SmartPausePolicy,
    skip_limit: SkipLimitPolicy,

    // Active monotonic deadlines in milliseconds.
    pre_break_at_ms: u64,
    next_break_at_ms: u64,
    break_started_ms: u64,
    break_ends_ms: u64,
    resume_at_ms: Option<u64>,
    disabled_until_unix_secs: Option<u64>,

    // Resting snapshots.
    resting_since_mono_ms: u64,
    resting_since_wall_secs: u64,
    remaining_until_break_ms: u64,
}

impl<C: Clock> Scheduler<C> {
    /// Creates a new scheduler in `SchedulerState::Start`.
    pub fn new(config: Config, clock: C, rng_seed: u64) -> Self {
        let queue = BreakQueue::from_config(&config, rng_seed);
        Self {
            config,
            clock,
            rng_seed,
            queue,
            state: SchedulerState::Start,
            smart_pause: SmartPausePolicy::new(),
            skip_limit: SkipLimitPolicy::new(0),
            pre_break_at_ms: 0,
            next_break_at_ms: 0,
            break_started_ms: 0,
            break_ends_ms: 0,
            resume_at_ms: None,
            disabled_until_unix_secs: None,
            resting_since_mono_ms: 0,
            resting_since_wall_secs: 0,
            remaining_until_break_ms: 0,
        }
    }

    /// Hydrates queue indices and consecutive-skip count from a persisted `SessionState`.
    pub fn restore_session(&mut self, session: &SessionState) {
        self.skip_limit = SkipLimitPolicy::new(session.consecutive_skips);
        if let Some(queue) = self.queue.as_mut() {
            queue.restore_progress(
                session.short_index,
                session.long_index,
                session.long_remaining_secs,
            );
        }
    }

    /// Returns the current scheduler state.
    pub const fn state(&self) -> SchedulerState {
        self.state
    }

    /// Returns a reference to the active configuration.
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the next monotonic deadline when `tick()` should be invoked.
    pub fn next_deadline(&self) -> Option<Duration> {
        let ms = match self.state {
            SchedulerState::Waiting => Some(self.pre_break_at_ms),
            SchedulerState::PreBreak => Some(self.next_break_at_ms),
            SchedulerState::Break => Some(self.break_ends_ms),
            SchedulerState::Stopped => self.resume_at_ms,
            SchedulerState::Start | SchedulerState::Resting | SchedulerState::Quit => None,
        };
        ms.map(Duration::from_millis)
    }

    /// Returns `true` during the initial `shortcut_disable_secs` lock window of `Break`.
    pub fn is_shortcut_locked(&self) -> bool {
        if self.state != SchedulerState::Break {
            return false;
        }
        let lock_end = self
            .break_started_ms
            .saturating_add(self.config.shortcut_disable_secs.saturating_mul(1000));
        self.now_mono_ms() < lock_end
    }

    /// Evaluates `DoNotDisturb` and `SmartPause::postpone_if_active` before entering `Break`.
    ///
    /// Host loops can call this when `state == PreBreak` right before `Tick` transitions to `Break`.
    pub fn evaluate_pre_break_policies(
        &mut self,
        current_idle: Option<Duration>,
        dnd_ctx: Option<&DndContext>,
    ) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::PreBreak {
            return Vec::new();
        }
        if let Some(ctx) = dnd_ctx
            && DoNotDisturbPolicy::evaluate(&self.config.do_not_disturb, ctx).should_suppress()
        {
            return self.handle_event(SchedulerEvent::CancelCurrentBreak);
        }
        if let Some(idle) = current_idle
            && let Some(delay) = self
                .smart_pause
                .should_postpone_if_active(&self.config.smart_pause, idle)
        {
            return self.handle_event(SchedulerEvent::DeferBreak {
                delay_secs: delay.as_secs().max(1),
            });
        }
        Vec::new()
    }

    /// Convenience wrapper for `handle_event(SchedulerEvent::Tick)`.
    pub fn tick(&mut self) -> Vec<SchedulerAction> {
        self.handle_event(SchedulerEvent::Tick)
    }

    /// Processes a single event synchronously and returns emitted side-effect actions.
    pub fn handle_event(&mut self, event: SchedulerEvent) -> Vec<SchedulerAction> {
        if self.state == SchedulerState::Quit {
            return Vec::new();
        }

        match event {
            SchedulerEvent::Start | SchedulerEvent::Enable => self.handle_start_or_enable(),
            SchedulerEvent::Tick => self.handle_tick(),
            SchedulerEvent::Disable { duration_secs } => self.handle_disable(duration_secs),
            SchedulerEvent::EnterResting => self.handle_enter_resting(),
            SchedulerEvent::ExitResting { credit_idle_secs } => {
                self.handle_exit_resting(credit_idle_secs)
            }
            SchedulerEvent::IdleObserved(idle) => self.handle_idle_observed(idle),
            SchedulerEvent::TakeBreak { kind } => self.handle_take_break(kind),
            SchedulerEvent::OverlayFinished(outcome) => self.handle_overlay_finished(outcome),
            SchedulerEvent::DeferBreak { delay_secs } => self.handle_defer_break(delay_secs),
            SchedulerEvent::CancelCurrentBreak => self.handle_cancel_current_break(),
            SchedulerEvent::ReloadConfig(new_cfg) => self.handle_reload_config(new_cfg),
            SchedulerEvent::Quit => self.handle_quit(),
        }
    }

    /// Builds a `StatusSnapshot` matching the CLI `--status --json` contract.
    pub fn status_snapshot(&self) -> StatusSnapshot {
        let enabled = self.is_enabled();
        let (next_break_type, next_break_name) = self
            .queue
            .as_ref()
            .map(|q| {
                (
                    Some(q.current().spec.break_type),
                    Some(q.current().spec.name.clone()),
                )
            })
            .unwrap_or((None, None));
        StatusSnapshot {
            state: self.state,
            enabled,
            next_break_type,
            next_break_name,
            seconds_until_next_break: self.seconds_until_next_break(),
            consecutive_skips: self.skip_limit.count(),
        }
    }

    /// Builds a `TraySnapshot` for `sye-platform`.
    pub fn tray_snapshot(&self) -> TraySnapshot {
        let status = self.status_snapshot();
        let menu_locked = self.config.strict_break
            && matches!(self.state, SchedulerState::PreBreak | SchedulerState::Break);
        TraySnapshot {
            state: status.state,
            enabled: status.enabled,
            has_breaks: self.queue.is_some(),
            menu_locked,
            next_break_type: status.next_break_type,
            next_break_name: status.next_break_name,
            seconds_until_next_break: status.seconds_until_next_break,
            disabled_until_unix_secs: self.disabled_until_unix_secs,
            consecutive_skips: status.consecutive_skips,
        }
    }

    /// Builds a `SessionState` snapshot for optional persistence.
    pub fn session_state(&self) -> SessionState {
        let (short_index, long_index, long_remaining_secs) = self
            .queue
            .as_ref()
            .map(|q| {
                (
                    q.persisted_short_index(),
                    q.persisted_long_index(),
                    q.long_remaining_secs(),
                )
            })
            .unwrap_or((0, 0, None));
        SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            short_index,
            long_index,
            long_remaining_secs,
            consecutive_skips: self.skip_limit.count(),
            saved_at_unix_secs: Some(self.now_wall_secs()),
        }
    }

    fn handle_start_or_enable(&mut self) -> Vec<SchedulerAction> {
        match self.state {
            SchedulerState::Start | SchedulerState::Stopped => {
                self.resume_at_ms = None;
                self.disabled_until_unix_secs = None;
                self.smart_pause.reset();
                if self.queue.is_none() {
                    self.state = SchedulerState::Stopped;
                    return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
                }
                self.enter_waiting_fresh()
            }
            SchedulerState::Resting => self.handle_exit_resting(None),
            SchedulerState::Waiting
            | SchedulerState::PreBreak
            | SchedulerState::Break
            | SchedulerState::Quit => Vec::new(),
        }
    }

    fn handle_tick(&mut self) -> Vec<SchedulerAction> {
        let now = self.now_mono_ms();
        match self.state {
            SchedulerState::Waiting => {
                if now < self.pre_break_at_ms {
                    return Vec::new();
                }
                let Some(queued) = self.queue.as_ref().map(|q| q.current().clone()) else {
                    self.state = SchedulerState::Stopped;
                    return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
                };
                let effective_warning_secs =
                    self.config.pre_break_warning_secs.min(queued.wait_secs);
                if effective_warning_secs == 0 {
                    return self.enter_break(now, &queued.spec);
                }
                self.state = SchedulerState::PreBreak;
                let warning_ms = effective_warning_secs.saturating_mul(1000);
                // Ensure the user still receives a warning window if tick() arrived late.
                self.next_break_at_ms = self.next_break_at_ms.max(now.saturating_add(warning_ms));
                let mut actions = vec![SchedulerAction::NotifyPreBreak {
                    break_spec: queued.spec,
                    warning_secs: effective_warning_secs,
                }];
                if self.config.audible_alert.pre_break {
                    actions.push(SchedulerAction::PlayChime {
                        kind: ChimeKind::PreBreak,
                        volume: self.config.audible_alert.volume,
                    });
                }
                actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
                actions
            }
            SchedulerState::PreBreak => {
                if now < self.next_break_at_ms {
                    return Vec::new();
                }
                let Some(spec) = self.queue.as_ref().map(|q| q.current().spec.clone()) else {
                    self.state = SchedulerState::Stopped;
                    return vec![
                        SchedulerAction::DismissNotification,
                        SchedulerAction::UpdateTray(self.tray_snapshot()),
                    ];
                };
                let mut actions = vec![SchedulerAction::DismissNotification];
                actions.extend(self.enter_break(now, &spec));
                actions
            }
            SchedulerState::Break => {
                if now < self.break_ends_ms {
                    return Vec::new();
                }
                self.complete_break(true)
            }
            SchedulerState::Stopped => {
                if let Some(resume_at) = self.resume_at_ms
                    && now >= resume_at
                {
                    self.resume_at_ms = None;
                    self.disabled_until_unix_secs = None;
                    if self.queue.is_some() {
                        return self.enter_waiting_fresh();
                    }
                }
                Vec::new()
            }
            SchedulerState::Start | SchedulerState::Resting | SchedulerState::Quit => Vec::new(),
        }
    }

    fn handle_disable(&mut self, duration_secs: Option<u64>) -> Vec<SchedulerAction> {
        let mut actions = Vec::new();
        self.push_active_state_cleanup(&mut actions);

        self.smart_pause.reset();
        self.state = SchedulerState::Stopped;

        match duration_secs {
            Some(secs) if secs > 0 => {
                let now_mono = self.now_mono_ms();
                let now_wall = self.now_wall_secs();
                self.resume_at_ms = Some(now_mono.saturating_add(secs.saturating_mul(1000)));
                self.disabled_until_unix_secs = Some(now_wall.saturating_add(secs));
            }
            _ => {
                self.resume_at_ms = None;
                self.disabled_until_unix_secs = None;
            }
        }

        actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
        actions
    }

    fn handle_enter_resting(&mut self) -> Vec<SchedulerAction> {
        match self.state {
            SchedulerState::Waiting | SchedulerState::PreBreak | SchedulerState::Break => {}
            SchedulerState::Start
            | SchedulerState::Stopped
            | SchedulerState::Resting
            | SchedulerState::Quit => return Vec::new(),
        }

        let mut actions = Vec::new();
        let now_mono = self.now_mono_ms();
        self.remaining_until_break_ms = if self.state == SchedulerState::Waiting {
            self.next_break_at_ms.saturating_sub(now_mono)
        } else {
            0
        };

        self.push_active_state_cleanup(&mut actions);

        self.resting_since_mono_ms = now_mono;
        self.resting_since_wall_secs = self.now_wall_secs();
        self.state = SchedulerState::Resting;
        actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
        actions
    }

    fn handle_exit_resting(&mut self, credit_idle_secs: Option<u64>) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::Resting {
            return Vec::new();
        }
        let Some(_) = self.queue else {
            self.state = SchedulerState::Stopped;
            return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        };

        let now_mono = self.now_mono_ms();
        let now_wall = self.now_wall_secs();
        let mono_elapsed_secs = now_mono.saturating_sub(self.resting_since_mono_ms) / 1000;
        let wall_elapsed_secs = now_wall.saturating_sub(self.resting_since_wall_secs);
        let paused_secs = wall_elapsed_secs.max(mono_elapsed_secs);

        let next_long_dur = self
            .queue
            .as_ref()
            .and_then(BreakQueue::next_long_duration_secs);

        if let Some(long_dur) = next_long_dur
            && paused_secs > long_dur
        {
            if let Some(queue) = self.queue.as_mut() {
                queue.skip_long_break();
            }
            return self.enter_waiting_fresh();
        }

        if let Some(idle_s) = credit_idle_secs
            && idle_s < self.config.short_break_interval_secs
            && self.remaining_until_break_ms > 0
        {
            let remaining_ms = self.remaining_until_break_ms;
            self.state = SchedulerState::Waiting;
            self.next_break_at_ms = now_mono.saturating_add(remaining_ms);
            let warning_ms = self
                .config
                .pre_break_warning_secs
                .saturating_mul(1000)
                .min(remaining_ms);
            self.pre_break_at_ms = self.next_break_at_ms.saturating_sub(warning_ms);
            return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        }

        self.enter_waiting_fresh()
    }

    fn handle_idle_observed(&mut self, current_idle: Duration) -> Vec<SchedulerAction> {
        let now = self.clock.now_monotonic();
        let decision = self.smart_pause.evaluate_idle(
            &self.config.smart_pause,
            self.state,
            now,
            current_idle,
            self.config.short_break_interval_secs,
        );
        match decision {
            SmartPauseDecision::None => Vec::new(),
            SmartPauseDecision::PauseResting => self.handle_enter_resting(),
            SmartPauseDecision::ResumeCreditBack { credit } => {
                self.handle_exit_resting(Some(credit.as_secs()))
            }
            SmartPauseDecision::ResumeResetCycle => self.handle_exit_resting(None),
        }
    }

    fn handle_take_break(&mut self, kind: Option<BreakType>) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::Waiting {
            return Vec::new();
        }
        let Some(queued) = self.queue.as_mut().and_then(|q| q.select_manual(kind)) else {
            return Vec::new();
        };
        let now = self.now_mono_ms();
        self.enter_break(now, &queued.spec)
    }

    fn handle_overlay_finished(&mut self, outcome: OverlayOutcome) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::Break {
            return Vec::new();
        }

        let limit_hit = self
            .skip_limit
            .controls_disabled(&self.config.limit_consecutive_skips);

        match outcome {
            OverlayOutcome::Completed => self.complete_break(true),
            OverlayOutcome::Skipped => {
                if self.config.strict_break || self.is_shortcut_locked() || limit_hit {
                    return Vec::new();
                }
                self.skip_limit.record_skip();
                self.advance_after_skip_or_close(true)
            }
            OverlayOutcome::Postponed { duration_secs } => {
                if !self.config.allow_postpone || self.is_shortcut_locked() || limit_hit {
                    return Vec::new();
                }
                self.skip_limit.record_postpone();
                let delay = if duration_secs == 0 {
                    self.config.postpone_duration_secs.max(1)
                } else {
                    duration_secs
                };
                let mut actions = vec![SchedulerAction::CloseOverlay];
                actions.extend(self.enter_waiting_with_wait_secs(delay, true));
                actions
            }
            OverlayOutcome::Closed => self.advance_after_skip_or_close(false),
        }
    }

    fn handle_defer_break(&mut self, delay_secs: u64) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::PreBreak {
            return Vec::new();
        }
        let mut actions = vec![SchedulerAction::DismissNotification];
        actions.extend(self.enter_waiting_with_wait_secs(delay_secs.max(1), false));
        actions
    }

    fn handle_cancel_current_break(&mut self) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::PreBreak {
            return Vec::new();
        }
        if let Some(queue) = self.queue.as_mut() {
            queue.advance();
        }
        let mut actions = vec![SchedulerAction::DismissNotification];
        actions.extend(self.enter_waiting_fresh());
        actions
    }

    fn handle_reload_config(&mut self, new_cfg: Config) -> Vec<SchedulerAction> {
        let mut actions = Vec::new();
        self.push_active_state_cleanup(&mut actions);

        self.config = new_cfg;
        self.queue = BreakQueue::from_config(&self.config, self.rng_seed);
        self.smart_pause.reset();

        if self.state == SchedulerState::Stopped || self.queue.is_none() {
            if self.queue.is_none() {
                self.state = SchedulerState::Stopped;
                self.resume_at_ms = None;
                self.disabled_until_unix_secs = None;
            }
            actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
            return actions;
        }

        actions.extend(self.enter_waiting_fresh());
        actions
    }

    fn handle_quit(&mut self) -> Vec<SchedulerAction> {
        let mut actions = Vec::new();
        self.push_active_state_cleanup(&mut actions);
        self.smart_pause.reset();
        self.resume_at_ms = None;
        self.disabled_until_unix_secs = None;
        self.state = SchedulerState::Quit;
        if self.config.persist_state {
            actions.push(SchedulerAction::PersistSession(self.session_state()));
        }
        actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
        actions
    }

    fn push_active_state_cleanup(&self, actions: &mut Vec<SchedulerAction>) {
        match self.state {
            SchedulerState::PreBreak => actions.push(SchedulerAction::DismissNotification),
            SchedulerState::Break => actions.push(SchedulerAction::CloseOverlay),
            _ => {}
        }
    }

    fn enter_break(&mut self, now_ms: u64, spec: &ResolvedBreak) -> Vec<SchedulerAction> {
        self.state = SchedulerState::Break;
        self.break_started_ms = now_ms;
        self.break_ends_ms = now_ms.saturating_add(spec.duration_secs.saturating_mul(1000));

        let limit_hit = self
            .skip_limit
            .controls_disabled(&self.config.limit_consecutive_skips);
        let request = OverlayRequest {
            break_type: spec.break_type,
            title: spec.name.clone(),
            duration_secs: spec.duration_secs,
            strict_break: self.config.strict_break || limit_hit,
            allow_postpone: self.config.allow_postpone && !limit_hit,
            postpone_duration_secs: self.config.postpone_duration_secs,
            shortcut_disable_secs: self.config.shortcut_disable_secs,
            fade_in_ms: self.config.fade_in_ms,
            status_note: self
                .skip_limit
                .status_label(&self.config.limit_consecutive_skips),
        };
        vec![
            SchedulerAction::ShowOverlay(request),
            SchedulerAction::UpdateTray(self.tray_snapshot()),
        ]
    }

    fn complete_break(&mut self, emit_close: bool) -> Vec<SchedulerAction> {
        self.skip_limit.record_completed_break();
        if let Some(queue) = self.queue.as_mut() {
            queue.advance();
        }
        let mut actions = Vec::new();
        if emit_close {
            actions.push(SchedulerAction::CloseOverlay);
        }
        if self.config.audible_alert.post_break {
            actions.push(SchedulerAction::PlayChime {
                kind: ChimeKind::PostBreak,
                volume: self.config.audible_alert.volume,
            });
        }
        actions.extend(self.enter_waiting_fresh());
        actions
    }

    fn advance_after_skip_or_close(&mut self, emit_close: bool) -> Vec<SchedulerAction> {
        if let Some(queue) = self.queue.as_mut() {
            queue.advance();
        }
        let mut actions = Vec::new();
        if emit_close {
            actions.push(SchedulerAction::CloseOverlay);
        }
        actions.extend(self.enter_waiting_fresh());
        actions
    }

    fn enter_waiting_fresh(&mut self) -> Vec<SchedulerAction> {
        let Some(wait_secs) = self.queue.as_ref().map(|q| q.current().wait_secs) else {
            self.state = SchedulerState::Stopped;
            return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        };
        self.enter_waiting_with_wait_secs(wait_secs, true)
    }

    fn enter_waiting_with_wait_secs(
        &mut self,
        wait_secs: u64,
        persist: bool,
    ) -> Vec<SchedulerAction> {
        let now = self.now_mono_ms();
        let wait_ms = wait_secs.saturating_mul(1000);
        let warning_secs = self.config.pre_break_warning_secs.min(wait_secs);
        let warning_ms = warning_secs.saturating_mul(1000);

        self.state = SchedulerState::Waiting;
        self.next_break_at_ms = now.saturating_add(wait_ms);
        self.pre_break_at_ms = self.next_break_at_ms.saturating_sub(warning_ms);

        let mut actions = vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        if persist && self.config.persist_state {
            actions.push(SchedulerAction::PersistSession(self.session_state()));
        }
        actions
    }

    fn is_enabled(&self) -> bool {
        matches!(
            self.state,
            SchedulerState::Waiting
                | SchedulerState::PreBreak
                | SchedulerState::Break
                | SchedulerState::Resting
        )
    }

    fn seconds_until_next_break(&self) -> u64 {
        let now = self.now_mono_ms();
        match self.state {
            SchedulerState::Waiting | SchedulerState::PreBreak => {
                self.next_break_at_ms.saturating_sub(now) / 1000
            }
            SchedulerState::Resting => self.remaining_until_break_ms / 1000,
            SchedulerState::Start
            | SchedulerState::Break
            | SchedulerState::Stopped
            | SchedulerState::Quit => 0,
        }
    }

    fn now_mono_ms(&self) -> u64 {
        u64::try_from(self.clock.now_monotonic().as_millis()).unwrap_or(u64::MAX)
    }

    fn now_wall_secs(&self) -> u64 {
        self.clock.now_wall_unix().as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AudibleAlertConfig, DoNotDisturbConfig, SkipLimitConfig};
    use crate::traits::FakeClock;

    fn test_config() -> Config {
        Config {
            random_order: false,
            audible_alert: AudibleAlertConfig {
                pre_break: true,
                post_break: true,
                volume: 80,
            },
            ..Config::default()
        }
    }

    #[test]
    fn full_happy_path_cycle_start_waiting_prebreak_break_waiting() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 42);
        assert_eq!(sched.state(), SchedulerState::Start);
        assert_eq!(sched.next_deadline(), None);

        let actions = sched.handle_event(SchedulerEvent::Start);
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(890)));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::UpdateTray(_)))
        );

        clock.advance_secs(889);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Waiting);

        clock.advance_secs(1);
        let pre_actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(900)));
        assert!(pre_actions.iter().any(|a| matches!(
            a,
            SchedulerAction::NotifyPreBreak {
                warning_secs: 10,
                ..
            }
        )));
        assert!(pre_actions.iter().any(|a| matches!(
            a,
            SchedulerAction::PlayChime {
                kind: ChimeKind::PreBreak,
                volume: 80
            }
        )));

        clock.advance_secs(10);
        let brk_actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Break);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(915)));
        assert!(
            brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
        assert!(
            brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );

        clock.advance_secs(15);
        let done_actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            done_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );
        assert!(done_actions.iter().any(|a| matches!(
            a,
            SchedulerAction::PlayChime {
                kind: ChimeKind::PostBreak,
                volume: 80
            }
        )));
    }

    #[test]
    fn zero_pre_break_warning_jumps_directly_to_break() {
        let clock = FakeClock::default();
        let cfg = Config {
            pre_break_warning_secs: 0,
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(900);
        let actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Break);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );
    }

    #[test]
    fn empty_queue_refuses_to_start() {
        let clock = FakeClock::default();
        let cfg = Config {
            short_breaks: Vec::new(),
            long_breaks: Vec::new(),
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let actions = sched.handle_event(SchedulerEvent::Start);
        assert_eq!(sched.state(), SchedulerState::Stopped);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::UpdateTray(t) if !t.has_breaks))
        );
    }

    #[test]
    fn quit_is_strictly_terminal_and_cleans_up() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);
        clock.advance_secs(890);
        sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);

        let quit_actions = sched.handle_event(SchedulerEvent::Quit);
        assert_eq!(sched.state(), SchedulerState::Quit);
        assert!(
            quit_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
        assert_eq!(sched.next_deadline(), None);

        assert!(sched.handle_event(SchedulerEvent::Start).is_empty());
        assert!(sched.handle_event(SchedulerEvent::Enable).is_empty());
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Quit);
    }

    #[test]
    fn shortcut_disable_time_locks_skip_and_postpone_for_2s() {
        let clock = FakeClock::default();
        let cfg = Config {
            allow_postpone: true,
            shortcut_disable_secs: 2,
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        sched.handle_event(SchedulerEvent::Start);
        sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert_eq!(sched.state(), SchedulerState::Break);
        assert!(sched.is_shortcut_locked());

        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped))
                .is_empty()
        );
        clock.advance(Duration::from_millis(1999));
        assert!(sched.is_shortcut_locked());
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Postponed {
                    duration_secs: 60,
                }))
                .is_empty()
        );

        clock.advance(Duration::from_millis(1));
        assert!(!sched.is_shortcut_locked());
        let actions = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::PlayChime { .. }))
        );
    }

    #[test]
    fn strict_break_rejects_skip_allows_postpone_and_locks_tray() {
        let clock = FakeClock::default();
        let cfg = Config {
            strict_break: true,
            allow_postpone: true,
            shortcut_disable_secs: 0,
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        sched.handle_event(SchedulerEvent::Start);
        assert!(!sched.tray_snapshot().menu_locked);

        let first_name = sched.status_snapshot().next_break_name;
        sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert!(sched.tray_snapshot().menu_locked);

        // Skip is rejected under strict_break.
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped))
                .is_empty()
        );
        assert_eq!(sched.state(), SchedulerState::Break);

        // Postpone is accepted, preserves queue pointer, suppresses chime, falls back on 0s.
        let post_actions =
            sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Postponed {
                duration_secs: 0,
            }));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.status_snapshot().next_break_name, first_name);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 300);
        assert!(
            !post_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::PlayChime { .. }))
        );
    }

    #[test]
    fn take_break_and_timed_disable_behavior() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);

        // Ignored in Start.
        assert!(
            sched
                .handle_event(SchedulerEvent::TakeBreak { kind: None })
                .is_empty()
        );
        sched.handle_event(SchedulerEvent::Start);

        // Timed disable for 1800s.
        sched.handle_event(SchedulerEvent::Disable {
            duration_secs: Some(1800),
        });
        assert_eq!(sched.state(), SchedulerState::Stopped);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(1800)));

        clock.advance_secs(1799);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Stopped);

        clock.advance_secs(1);
        sched.tick();
        assert_eq!(sched.state(), SchedulerState::Waiting);

        // Disable until restart never auto-resumes.
        sched.handle_event(SchedulerEvent::Disable {
            duration_secs: None,
        });
        assert_eq!(sched.next_deadline(), None);
        clock.advance_secs(86_400);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Stopped);
    }

    #[test]
    fn disable_during_break_cleans_up_and_ignores_stale_overlay_finished() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);
        sched.handle_event(SchedulerEvent::TakeBreak {
            kind: Some(BreakType::Long),
        });
        assert_eq!(sched.state(), SchedulerState::Break);

        let actions = sched.handle_event(SchedulerEvent::Disable {
            duration_secs: None,
        });
        assert_eq!(sched.state(), SchedulerState::Stopped);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );

        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Completed))
                .is_empty()
        );
        assert_eq!(sched.state(), SchedulerState::Stopped);
    }

    #[test]
    fn resting_longer_than_long_duration_resets_cycle_and_boundary_exact() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);

        // Complete 4 short breaks so Long is staged next.
        for _ in 0..4 {
            sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
            sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Completed));
        }
        assert_eq!(
            sched.status_snapshot().next_break_type,
            Some(BreakType::Long)
        );

        // Exact boundary: 60s == long_break_duration_secs (60) -> NOT skipped.
        sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_wall_only(Duration::from_secs(60));
        sched.handle_event(SchedulerEvent::ExitResting {
            credit_idle_secs: None,
        });
        assert_eq!(
            sched.status_snapshot().next_break_type,
            Some(BreakType::Long)
        );

        // Strictly greater: 61s > 60s -> skip_long_break() switches back to Short.
        sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_wall_only(Duration::from_secs(61));
        sched.handle_event(SchedulerEvent::ExitResting {
            credit_idle_secs: None,
        });
        assert_eq!(
            sched.status_snapshot().next_break_type,
            Some(BreakType::Short)
        );
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 900);
    }

    #[test]
    fn resting_credit_back_and_backward_wall_clock_safety() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(300); // 600s remaining
        sched.handle_event(SchedulerEvent::EnterResting);
        // Simulate backward NTP jump while monotonic advances 120s.
        clock.advance_secs(120);
        clock.set_wall_unix(Duration::from_secs(100));

        sched.handle_event(SchedulerEvent::ExitResting {
            credit_idle_secs: Some(120),
        });
        // 120s > 60s long_dur -> resets cycle cleanly without underflow.
        assert_eq!(sched.state(), SchedulerState::Waiting);

        // Shorter idle (30s <= 60s) with credit-back preserves remaining 600s.
        clock.advance_secs(300); // 600s remaining
        sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_secs(30);
        sched.handle_event(SchedulerEvent::ExitResting {
            credit_idle_secs: Some(30),
        });
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 600);
    }

    #[test]
    fn skip_limit_enforced_in_scheduler_and_resets_on_completion() {
        let clock = FakeClock::default();
        let cfg = Config {
            allow_postpone: true,
            shortcut_disable_secs: 0,
            limit_consecutive_skips: SkipLimitConfig {
                enabled: true,
                max_skips: 2,
            },
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        sched.handle_event(SchedulerEvent::Start);

        // Skip #1
        sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped));
        assert_eq!(sched.status_snapshot().consecutive_skips, 1);

        // Postpone #2 (reaches max_skips == 2)
        sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Postponed {
            duration_secs: 60,
        }));
        assert_eq!(sched.status_snapshot().consecutive_skips, 2);

        // Next break must have strict_break = true, allow_postpone = false, and status_note set.
        let actions = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        let has_locked_overlay = actions.iter().any(|a| {
            matches!(
                a,
                SchedulerAction::ShowOverlay(req)
                    if req.strict_break
                        && !req.allow_postpone
                        && req.status_note.as_deref()
                            == Some("Skipped or postponed 2/2 breaks in a row")
            )
        });
        assert!(has_locked_overlay);

        // Both Skip and Postpone are rejected at the limit.
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped))
                .is_empty()
        );
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Postponed {
                    duration_secs: 60,
                }))
                .is_empty()
        );

        // Completing the break resets consecutive_skips to 0.
        sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Completed));
        assert_eq!(sched.status_snapshot().consecutive_skips, 0);
    }

    #[test]
    fn pre_break_policies_handle_dnd_and_active_postpone() {
        let clock = FakeClock::default();
        let mut cfg = test_config();
        cfg.smart_pause.postpone_if_active = true;
        cfg.do_not_disturb = DoNotDisturbConfig {
            enabled: true,
            skip_break_windows: vec!["vlc".to_owned()],
            take_break_windows: Vec::new(),
            while_on_battery: false,
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);

        // Active user defers break by 2s.
        let defer_actions = sched.evaluate_pre_break_policies(Some(Duration::ZERO), None);
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 2);
        assert!(
            defer_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );

        // Reach PreBreak again, then suppress via DND.
        clock.advance_secs(2);
        sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        let dnd = DndContext {
            is_fullscreen: true,
            active_window_class: Some("VLC".to_owned()),
            on_battery: false,
            system_inhibited: false,
        };
        let cancel_actions = sched.evaluate_pre_break_policies(None, Some(&dnd));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            cancel_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
    }

    #[test]
    fn idle_observed_drives_smart_pause_credit_back_and_reset() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(300); // 600s remaining
        sched.handle_event(SchedulerEvent::IdleObserved(Duration::from_secs(5)));
        assert_eq!(sched.state(), SchedulerState::Resting);

        clock.advance_secs(20);
        sched.handle_event(SchedulerEvent::IdleObserved(Duration::ZERO));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 600);
    }

    #[test]
    fn reload_config_cleans_up_active_break_and_rebuilds_queue() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);
        sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert_eq!(sched.state(), SchedulerState::Break);

        let new_cfg = Config {
            short_break_interval_secs: 600,
            ..test_config()
        };
        let actions = sched.handle_event(SchedulerEvent::ReloadConfig(new_cfg));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 600);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );
    }

    #[test]
    fn persist_state_emits_session_and_restore_session_hydrates() {
        let clock = FakeClock::default();
        let cfg = Config {
            persist_state: true,
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg.clone(), &clock, 1);
        let actions = sched.handle_event(SchedulerEvent::Start);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::PersistSession(_)))
        );

        let saved = SessionState {
            short_index: 2,
            long_index: 1,
            long_remaining_secs: Some(900),
            consecutive_skips: 1,
            ..SessionState::default()
        };
        let mut restored = Scheduler::new(cfg, &clock, 1);
        restored.restore_session(&saved);
        assert_eq!(restored.status_snapshot().consecutive_skips, 1);
        assert_eq!(
            restored.status_snapshot().next_break_type,
            Some(BreakType::Long)
        );
    }

    #[test]
    fn clock_near_u64_max_does_not_overflow_or_panic() {
        let clock = FakeClock::new(
            Duration::from_millis(u64::MAX - 500),
            Duration::from_secs(u64::MAX / 1000),
        );
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        sched.handle_event(SchedulerEvent::Start);
        assert_eq!(sched.state(), SchedulerState::Waiting);
        let _ = sched.status_snapshot();
        let _ = sched.tick();
    }
}
