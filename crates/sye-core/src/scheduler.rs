use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::chime::ChimeKind;
use crate::config::{BreakType, Config, SkipLimitConfig};
use crate::i18n::Locale;
use crate::policies::{
    DndContext, DoNotDisturbPolicy, SkipLimitPolicy, SmartPauseDecision, SmartPausePolicy,
};
use crate::queue::{BreakQueue, ResolvedBreak};
use crate::session::{SESSION_SCHEMA_VERSION, SessionState};
use crate::traits::{Clock, duration_to_ms_u64};

// Startup grace prevents short breaks from expiring while Windows cold-starts the overlay process.
const OVERLAY_STARTUP_GRACE_MS: u64 = 10_000;
// Completion grace lets the overlay finish its own countdown and exit cleanly before fallback kill.
const OVERLAY_COMPLETION_GRACE_MS: u64 = 2_000;

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
    #[serde(default)]
    pub locale: Locale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum OverlayOutcome {
    Completed,
    Skipped,
    Postponed { duration_secs: u64 },
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub state: SchedulerState,
    pub enabled: bool,
    pub next_break_type: Option<BreakType>,
    pub next_break_name: Option<String>,
    pub seconds_until_next_break: u64,
    pub consecutive_skips: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_version: Option<String>,
}

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
    OverlayLaunching,
    OverlayShown,
    OverlayFinished(OverlayOutcome),
    DeferBreak { delay_secs: u64 },
    CancelCurrentBreak,
    ReloadConfig(Config),
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerAction {
    NotifyPreBreak {
        break_spec: ResolvedBreak,
        warning_secs: u64,
        total_secs: u64,
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

#[derive(Debug)]
pub struct Scheduler<C: Clock> {
    config: Config,
    system_locale: Locale,
    clock: C,
    rng_seed: u64,
    queue: Option<BreakQueue>,
    state: SchedulerState,
    smart_pause: SmartPausePolicy,
    skip_limit: SkipLimitPolicy,
    pre_break_notification_active: bool,
    last_pre_break_notified_secs: Option<u64>,
    pre_break_total_secs: u64,

    pre_break_at_ms: u64,
    next_break_at_ms: u64,
    countdown_started_ms: u64,
    break_duration_ms: u64,
    overlay_shown: bool,
    break_started_ms: u64,
    break_ends_ms: u64,
    resume_at_ms: Option<u64>,
    disabled_until_unix_secs: Option<u64>,

    resting_since_mono_ms: u64,
    resting_since_wall_secs: u64,
    remaining_until_break_ms: u64,
}

impl<C: Clock> Scheduler<C> {
    #[must_use]
    pub fn new(config: Config, clock: C, rng_seed: u64) -> Self {
        let queue = BreakQueue::from_config(&config, rng_seed);
        Self {
            config,
            system_locale: Locale::En,
            clock,
            rng_seed,
            queue,
            state: SchedulerState::Start,
            smart_pause: SmartPausePolicy::new(),
            skip_limit: SkipLimitPolicy::new(0),
            pre_break_notification_active: false,
            last_pre_break_notified_secs: None,
            pre_break_total_secs: 0,
            pre_break_at_ms: 0,
            next_break_at_ms: 0,
            countdown_started_ms: 0,
            break_duration_ms: 0,
            overlay_shown: false,
            break_started_ms: 0,
            break_ends_ms: 0,
            resume_at_ms: None,
            disabled_until_unix_secs: None,
            resting_since_mono_ms: 0,
            resting_since_wall_secs: 0,
            remaining_until_break_ms: 0,
        }
    }

    pub const fn set_system_locale(&mut self, locale: Locale) {
        self.system_locale = locale;
    }

    #[must_use]
    pub const fn active_locale(&self) -> Locale {
        self.config.language.resolve(self.system_locale)
    }

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

    #[must_use]
    pub const fn state(&self) -> SchedulerState {
        self.state
    }

    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub const fn next_deadline(&self) -> Option<Duration> {
        let ms = match self.state {
            SchedulerState::Waiting => Some(self.pre_break_at_ms),
            SchedulerState::PreBreak => Some(self.next_break_at_ms),
            SchedulerState::Break => Some(self.break_ends_ms),
            SchedulerState::Stopped => self.resume_at_ms,
            SchedulerState::Start | SchedulerState::Resting | SchedulerState::Quit => None,
        };
        match ms {
            Some(v) => Some(Duration::from_millis(v)),
            None => None,
        }
    }

    #[must_use]
    pub fn is_shortcut_locked(&self) -> bool {
        if self.state != SchedulerState::Break {
            return false;
        }
        let lock_end = self
            .break_started_ms
            .saturating_add(self.config.shortcut_disable_secs.saturating_mul(1000));
        self.now_mono_ms() < lock_end
    }

    #[must_use]
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
            && let Some(delay) =
                SmartPausePolicy::should_postpone_if_active(&self.config.smart_pause, idle)
        {
            return self.handle_event(SchedulerEvent::DeferBreak {
                delay_secs: delay.as_secs().max(1),
            });
        }
        Vec::new()
    }

    #[must_use]
    pub fn needs_policy_check(&self) -> bool {
        match self.state {
            SchedulerState::Waiting => self.now_mono_ms() >= self.pre_break_at_ms,
            SchedulerState::PreBreak => self.now_mono_ms() >= self.next_break_at_ms,
            SchedulerState::Start
            | SchedulerState::Break
            | SchedulerState::Stopped
            | SchedulerState::Quit
            | SchedulerState::Resting => false,
        }
    }

    #[must_use]
    pub fn tick_with_policies(
        &mut self,
        current_idle: Option<Duration>,
        dnd_ctx: Option<&DndContext>,
    ) -> Vec<SchedulerAction> {
        if self.state == SchedulerState::Quit {
            return Vec::new();
        }

        let now = self.now_mono_ms();
        match self.state {
            SchedulerState::Waiting => self.tick_waiting(now, current_idle, dnd_ctx),
            SchedulerState::PreBreak => self.tick_pre_break(now, current_idle, dnd_ctx),
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

    #[must_use]
    pub fn tick(&mut self) -> Vec<SchedulerAction> {
        self.tick_with_policies(None, None)
    }

    #[must_use]
    pub fn handle_event(&mut self, event: SchedulerEvent) -> Vec<SchedulerAction> {
        if self.state == SchedulerState::Quit {
            return Vec::new();
        }

        match event {
            SchedulerEvent::Start | SchedulerEvent::Enable => self.handle_start_or_enable(),
            SchedulerEvent::Tick => self.tick_with_policies(None, None),
            SchedulerEvent::Disable { duration_secs } => self.handle_disable(duration_secs),
            SchedulerEvent::EnterResting => self.handle_enter_resting(0),
            SchedulerEvent::ExitResting { credit_idle_secs } => {
                self.handle_exit_resting(credit_idle_secs)
            }
            SchedulerEvent::IdleObserved(idle) => self.handle_idle_observed(idle),
            SchedulerEvent::TakeBreak { kind } => self.handle_take_break(kind),
            SchedulerEvent::OverlayLaunching => self.handle_overlay_launching(),
            SchedulerEvent::OverlayShown => self.handle_overlay_shown(),
            SchedulerEvent::OverlayFinished(outcome) => self.handle_overlay_finished(outcome),
            SchedulerEvent::DeferBreak { delay_secs } => self.handle_defer_break(delay_secs),
            SchedulerEvent::CancelCurrentBreak => self.handle_cancel_current_break(),
            SchedulerEvent::ReloadConfig(new_cfg) => self.handle_reload_config(new_cfg),
            SchedulerEvent::Quit => self.handle_quit(),
        }
    }

    #[must_use]
    pub fn status_snapshot(&self) -> StatusSnapshot {
        let enabled = self.is_enabled();
        let (next_break_type, next_break_name) = self.queue.as_ref().map_or((None, None), |q| {
            (
                Some(q.current().spec.break_type),
                Some(q.current().spec.name.clone()),
            )
        });
        StatusSnapshot {
            state: self.state,
            enabled,
            next_break_type,
            next_break_name,
            seconds_until_next_break: self.seconds_until_next_break(),
            consecutive_skips: self.skip_limit.count(),
            daemon_version: None,
        }
    }

    #[must_use]
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

    #[must_use]
    pub fn session_state(&self) -> SessionState {
        let (short_index, long_index, long_remaining_secs) =
            self.queue.as_ref().map_or((0, 0, None), |q| {
                (
                    q.persisted_short_index(),
                    q.persisted_long_index(),
                    q.long_remaining_secs(),
                )
            });
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

    fn tick_waiting(
        &mut self,
        now: u64,
        current_idle: Option<Duration>,
        dnd_ctx: Option<&DndContext>,
    ) -> Vec<SchedulerAction> {
        if now < self.pre_break_at_ms {
            return Vec::new();
        }
        if self.queue.is_none() {
            self.state = SchedulerState::Stopped;
            return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        }
        if let Some(ctx) = dnd_ctx
            && DoNotDisturbPolicy::evaluate(&self.config.do_not_disturb, ctx).should_suppress()
        {
            if let Some(queue) = self.queue.as_mut() {
                queue.advance();
            }
            return self.enter_waiting_fresh();
        }
        let Some(queued) = self.queue.as_ref().map(|q| q.current().clone()) else {
            self.state = SchedulerState::Stopped;
            return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        };
        let effective_warning_secs = self.config.pre_break_warning_secs.min(queued.wait_secs);
        if effective_warning_secs == 0 {
            if let Some(idle) = current_idle
                && let Some(delay) =
                    SmartPausePolicy::should_postpone_if_active(&self.config.smart_pause, idle)
            {
                self.state = SchedulerState::PreBreak;
                self.pre_break_notification_active = false;
                self.last_pre_break_notified_secs = None;
                let delay_ms = delay.as_secs().max(1).saturating_mul(1000);
                self.next_break_at_ms = now.saturating_add(delay_ms);
                return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
            }
            return self.enter_break(now, queued.spec);
        }
        self.state = SchedulerState::PreBreak;
        self.pre_break_notification_active = true;
        self.last_pre_break_notified_secs = Some(effective_warning_secs);
        self.pre_break_total_secs = effective_warning_secs;
        let warning_ms = effective_warning_secs.saturating_mul(1000);
        // Ensure the user still receives a warning window if tick() arrived late.
        self.next_break_at_ms = self.next_break_at_ms.max(now.saturating_add(warning_ms));
        let mut actions = vec![SchedulerAction::NotifyPreBreak {
            break_spec: queued.spec,
            warning_secs: effective_warning_secs,
            total_secs: effective_warning_secs,
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

    fn tick_pre_break(
        &mut self,
        now: u64,
        current_idle: Option<Duration>,
        dnd_ctx: Option<&DndContext>,
    ) -> Vec<SchedulerAction> {
        if now < self.next_break_at_ms {
            if self.pre_break_notification_active {
                let remaining_secs = self.next_break_at_ms.saturating_sub(now).div_ceil(1000);
                if remaining_secs > 0
                    && self
                        .last_pre_break_notified_secs
                        .is_none_or(|prev| remaining_secs < prev)
                    && let Some(spec) = self.queue.as_ref().map(|q| q.current().spec.clone())
                {
                    self.last_pre_break_notified_secs = Some(remaining_secs);
                    return vec![SchedulerAction::NotifyPreBreak {
                        break_spec: spec,
                        warning_secs: remaining_secs,
                        total_secs: self.pre_break_total_secs,
                    }];
                }
            }
            return Vec::new();
        }
        if let Some(ctx) = dnd_ctx
            && DoNotDisturbPolicy::evaluate(&self.config.do_not_disturb, ctx).should_suppress()
        {
            return self.handle_cancel_current_break();
        }
        if let Some(idle) = current_idle
            && let Some(delay) =
                SmartPausePolicy::should_postpone_if_active(&self.config.smart_pause, idle)
        {
            return self.handle_defer_break(delay.as_secs().max(1));
        }
        let dismiss_notification = std::mem::take(&mut self.pre_break_notification_active);
        self.last_pre_break_notified_secs = None;
        let Some(spec) = self.queue.as_ref().map(|q| q.current().spec.clone()) else {
            self.state = SchedulerState::Stopped;
            let mut actions = Vec::new();
            if dismiss_notification {
                actions.push(SchedulerAction::DismissNotification);
            }
            actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
            return actions;
        };
        let mut actions = self.enter_break(now, spec);
        if dismiss_notification {
            actions.push(SchedulerAction::DismissNotification);
        }
        actions
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

    fn handle_enter_resting(&mut self, credit_ms: u64) -> Vec<SchedulerAction> {
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
            self.next_break_at_ms
                .saturating_sub(now_mono)
                .saturating_add(credit_ms)
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
        if self.queue.is_none() {
            self.state = SchedulerState::Stopped;
            return vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        }

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
            self.countdown_started_ms = now_mono;
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
            SmartPauseDecision::PauseResting => {
                // The countdown ran while the user was idle before the threshold was reached.
                let counted_ms = self.now_mono_ms().saturating_sub(self.countdown_started_ms);
                self.handle_enter_resting(duration_to_ms_u64(current_idle).min(counted_ms))
            }
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
        self.enter_break(now, queued.spec)
    }

    fn handle_overlay_finished(&mut self, outcome: OverlayOutcome) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::Break {
            return Vec::new();
        }

        let skip_limit_cfg = self.effective_skip_limit();
        let limit_hit = self.skip_limit.controls_disabled(&skip_limit_cfg);

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
        let delay_ms = delay_secs.max(1).saturating_mul(1000);
        self.next_break_at_ms = self.now_mono_ms().saturating_add(delay_ms);
        self.last_pre_break_notified_secs = None;
        let mut actions = Vec::new();
        if std::mem::take(&mut self.pre_break_notification_active) {
            actions.push(SchedulerAction::DismissNotification);
        }
        actions.push(SchedulerAction::UpdateTray(self.tray_snapshot()));
        actions
    }

    fn handle_cancel_current_break(&mut self) -> Vec<SchedulerAction> {
        if self.state != SchedulerState::PreBreak {
            return Vec::new();
        }
        if let Some(queue) = self.queue.as_mut() {
            queue.advance();
        }
        self.last_pre_break_notified_secs = None;
        let mut actions = Vec::new();
        if std::mem::take(&mut self.pre_break_notification_active) {
            actions.push(SchedulerAction::DismissNotification);
        }
        actions.extend(self.enter_waiting_fresh());
        actions
    }

    fn handle_reload_config(&mut self, new_cfg: Config) -> Vec<SchedulerAction> {
        let mut current_non_schedule = self.config.clone();
        current_non_schedule.theme = new_cfg.theme;
        current_non_schedule.language = new_cfg.language;
        current_non_schedule.check_for_updates = new_cfg.check_for_updates;
        if new_cfg == current_non_schedule {
            self.config.theme = new_cfg.theme;
            self.config.language = new_cfg.language;
            self.config.check_for_updates = new_cfg.check_for_updates;
            return Vec::new();
        }

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

    fn push_active_state_cleanup(&mut self, actions: &mut Vec<SchedulerAction>) {
        match self.state {
            SchedulerState::PreBreak => {
                self.last_pre_break_notified_secs = None;
                if std::mem::take(&mut self.pre_break_notification_active) {
                    actions.push(SchedulerAction::DismissNotification);
                }
            }
            SchedulerState::Break => actions.push(SchedulerAction::CloseOverlay),
            SchedulerState::Start
            | SchedulerState::Waiting
            | SchedulerState::Stopped
            | SchedulerState::Quit
            | SchedulerState::Resting => {}
        }
    }

    const fn effective_skip_limit(&self) -> SkipLimitConfig {
        SkipLimitConfig {
            enabled: !self.config.strict_break && self.config.limit_consecutive_skips.enabled,
            max_skips: self.config.limit_consecutive_skips.max_skips,
        }
    }

    fn handle_overlay_launching(&mut self) -> Vec<SchedulerAction> {
        if self.state == SchedulerState::Break && !self.overlay_shown {
            let now = self.now_mono_ms();
            self.break_ends_ms = now
                .saturating_add(self.break_duration_ms)
                .saturating_add(OVERLAY_STARTUP_GRACE_MS);
        }
        Vec::new()
    }

    fn handle_overlay_shown(&mut self) -> Vec<SchedulerAction> {
        if self.state == SchedulerState::Break && !self.overlay_shown {
            let now = self.now_mono_ms();
            self.overlay_shown = true;
            // Keep `break_started_ms` from `enter_break` so daemon poll lag never rejects a Skip/Postpone
            // sent right after the overlay unlocks its own controls.
            self.break_ends_ms = now
                .saturating_add(self.break_duration_ms)
                .saturating_add(OVERLAY_COMPLETION_GRACE_MS);
        }
        Vec::new()
    }

    fn enter_break(&mut self, now_ms: u64, spec: ResolvedBreak) -> Vec<SchedulerAction> {
        self.state = SchedulerState::Break;
        self.pre_break_notification_active = false;
        self.last_pre_break_notified_secs = None;
        self.break_duration_ms = spec.duration_secs.saturating_mul(1000);
        self.overlay_shown = false;
        self.break_started_ms = now_ms;
        self.break_ends_ms = now_ms.saturating_add(self.break_duration_ms);

        let skip_limit_cfg = self.effective_skip_limit();
        let limit_hit = self.skip_limit.controls_disabled(&skip_limit_cfg);
        let locale = self.active_locale();
        let request = OverlayRequest {
            break_type: spec.break_type,
            title: spec.name,
            duration_secs: spec.duration_secs,
            strict_break: self.config.strict_break || limit_hit,
            allow_postpone: self.config.allow_postpone && !limit_hit,
            postpone_duration_secs: self.config.postpone_duration_secs,
            shortcut_disable_secs: self.config.shortcut_disable_secs,
            fade_in_ms: self.config.fade_in_ms,
            status_note: self
                .skip_limit
                .status_label_localized(&skip_limit_cfg, locale),
            locale,
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
            self.pre_break_notification_active = false;
            self.last_pre_break_notified_secs = None;
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
        self.pre_break_notification_active = false;
        self.last_pre_break_notified_secs = None;
        self.next_break_at_ms = now.saturating_add(wait_ms);
        self.pre_break_at_ms = self.next_break_at_ms.saturating_sub(warning_ms);
        self.countdown_started_ms = now;

        let mut actions = vec![SchedulerAction::UpdateTray(self.tray_snapshot())];
        if persist && self.config.persist_state {
            actions.push(SchedulerAction::PersistSession(self.session_state()));
        }
        actions
    }

    const fn is_enabled(&self) -> bool {
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
                self.next_break_at_ms.saturating_sub(now).div_ceil(1000)
            }
            SchedulerState::Resting => self.remaining_until_break_ms.div_ceil(1000),
            SchedulerState::Start
            | SchedulerState::Break
            | SchedulerState::Stopped
            | SchedulerState::Quit => 0,
        }
    }

    fn now_mono_ms(&self) -> u64 {
        duration_to_ms_u64(self.clock.now_monotonic())
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
        let _ = sched.handle_event(SchedulerEvent::Start);

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
        let _ = sched.handle_event(SchedulerEvent::Start);
        clock.advance_secs(890);
        let _ = sched.tick();
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
        let _ = sched.handle_event(SchedulerEvent::Start);
        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
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
        let _ = sched.handle_event(SchedulerEvent::Start);
        assert!(!sched.tray_snapshot().menu_locked);

        let first_name = sched.status_snapshot().next_break_name;
        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert!(sched.tray_snapshot().menu_locked);

        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped))
                .is_empty()
        );
        assert_eq!(sched.state(), SchedulerState::Break);

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

        assert!(
            sched
                .handle_event(SchedulerEvent::TakeBreak { kind: None })
                .is_empty()
        );
        let _ = sched.handle_event(SchedulerEvent::Start);

        let _ = sched.handle_event(SchedulerEvent::Disable {
            duration_secs: Some(1800),
        });
        assert_eq!(sched.state(), SchedulerState::Stopped);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(1800)));

        clock.advance_secs(1799);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Stopped);

        clock.advance_secs(1);
        let _ = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Waiting);

        let _ = sched.handle_event(SchedulerEvent::Disable {
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
        let _ = sched.handle_event(SchedulerEvent::Start);
        let _ = sched.handle_event(SchedulerEvent::TakeBreak {
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
        let _ = sched.handle_event(SchedulerEvent::Start);

        for _ in 0..4 {
            let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
            let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Completed));
        }
        assert_eq!(
            sched.status_snapshot().next_break_type,
            Some(BreakType::Long)
        );

        let _ = sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_wall_only(Duration::from_secs(60));
        let _ = sched.handle_event(SchedulerEvent::ExitResting {
            credit_idle_secs: None,
        });
        assert_eq!(
            sched.status_snapshot().next_break_type,
            Some(BreakType::Long)
        );

        let _ = sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_wall_only(Duration::from_secs(61));
        let _ = sched.handle_event(SchedulerEvent::ExitResting {
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
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(300);
        let _ = sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_secs(120);
        clock.set_wall_unix(Duration::from_secs(100));

        let _ = sched.handle_event(SchedulerEvent::ExitResting {
            credit_idle_secs: Some(120),
        });
        assert_eq!(sched.state(), SchedulerState::Waiting);

        clock.advance_secs(300);
        let _ = sched.handle_event(SchedulerEvent::EnterResting);
        clock.advance_secs(30);
        let _ = sched.handle_event(SchedulerEvent::ExitResting {
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
        let _ = sched.handle_event(SchedulerEvent::Start);

        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped));
        assert_eq!(sched.status_snapshot().consecutive_skips, 1);

        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Postponed {
            duration_secs: 60,
        }));
        assert_eq!(sched.status_snapshot().consecutive_skips, 2);

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

        let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Completed));
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
            while_gaming: true,
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        let _ = sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);

        let defer_actions = sched.evaluate_pre_break_policies(Some(Duration::ZERO), None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 2);
        assert!(
            defer_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );

        clock.advance_secs(2);
        let _ = sched.tick();
        let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Completed));

        clock.advance_secs(890);
        let _ = sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        let dnd = DndContext {
            is_fullscreen: true,
            active_window_class: Some("VLC".to_owned()),
            ..DndContext::default()
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
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(300);
        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::from_secs(5)));
        assert_eq!(sched.state(), SchedulerState::Waiting);

        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::from_secs(300)));
        assert_eq!(sched.state(), SchedulerState::Resting);

        clock.advance_secs(20);
        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::ZERO));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 900);
    }

    #[test]
    fn smart_pause_credits_idle_before_the_pause() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(400);
        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::from_secs(300)));
        assert_eq!(sched.state(), SchedulerState::Resting);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 800);

        clock.advance_secs(60);
        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::ZERO));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 800);
    }

    #[test]
    fn smart_pause_credit_never_exceeds_the_countdown_elapsed() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(100);
        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::from_secs(300)));
        clock.advance_secs(30);
        let _ = sched.handle_event(SchedulerEvent::IdleObserved(Duration::ZERO));
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 900);
    }

    #[test]
    fn reload_config_cleans_up_active_break_and_rebuilds_queue() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);
        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
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
        let _ = sched.handle_event(SchedulerEvent::Start);
        assert_eq!(sched.state(), SchedulerState::Waiting);
        let _ = sched.status_snapshot();
        let _ = sched.tick();
    }

    fn has_pre_break_chime(actions: &[SchedulerAction]) -> bool {
        actions.iter().any(|a| {
            matches!(
                a,
                SchedulerAction::PlayChime {
                    kind: ChimeKind::PreBreak,
                    ..
                }
            )
        })
    }

    #[test]
    fn needs_policy_check_tracks_waiting_and_pre_break_deadlines() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        assert!(!sched.needs_policy_check());

        let _ = sched.handle_event(SchedulerEvent::Start);
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(!sched.needs_policy_check());

        clock.advance_secs(889);
        assert!(!sched.needs_policy_check());

        clock.advance_secs(1);
        assert!(sched.needs_policy_check());

        let _ = sched.tick_with_policies(None, None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert!(!sched.needs_policy_check());

        clock.advance_secs(9);
        assert!(!sched.needs_policy_check());

        clock.advance_secs(1);
        assert!(sched.needs_policy_check());

        let _ = sched.tick_with_policies(None, None);
        assert_eq!(sched.state(), SchedulerState::Break);
        assert!(!sched.needs_policy_check());
    }

    #[test]
    fn tick_with_policies_dnd_at_waiting_suppresses_pre_break_and_advances_queue() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);
        let first_name = sched.status_snapshot().next_break_name;

        clock.advance_secs(890);
        let dnd = DndContext {
            is_fullscreen: true,
            ..DndContext::default()
        };
        let actions = sched.tick_with_policies(None, Some(&dnd));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert_ne!(sched.status_snapshot().next_break_name, first_name);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 900);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(1780)));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(!has_pre_break_chime(&actions));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
    }

    #[test]
    fn tick_with_policies_gaming_suppresses_break_even_when_fullscreen_dnd_disabled() {
        let clock = FakeClock::default();
        let mut cfg = test_config();
        cfg.do_not_disturb.enabled = false;
        cfg.do_not_disturb.while_gaming = true;
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        let gaming_ctx = DndContext {
            game_detected: true,
            ..DndContext::default()
        };
        let actions = sched.tick_with_policies(None, Some(&gaming_ctx));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
    }

    #[test]
    fn tick_with_policies_postpone_if_active_defers_in_pre_break_without_spam() {
        let clock = FakeClock::default();
        let mut cfg = test_config();
        cfg.smart_pause.postpone_if_active = true;
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        let pre_actions = sched.tick_with_policies(Some(Duration::ZERO), None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert!(
            pre_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(has_pre_break_chime(&pre_actions));

        clock.advance_secs(10);
        let defer_1 = sched.tick_with_policies(Some(Duration::from_secs(1)), None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 2);
        assert_eq!(sched.next_deadline(), Some(Duration::from_secs(902)));
        let dismiss_count_1 = defer_1
            .iter()
            .filter(|a| matches!(a, SchedulerAction::DismissNotification))
            .count();
        assert_eq!(dismiss_count_1, 1);
        assert!(
            !defer_1
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(!has_pre_break_chime(&defer_1));
        assert!(
            !defer_1
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );

        clock.advance_secs(2);
        let defer_2 = sched.tick_with_policies(Some(Duration::ZERO), None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 2);
        assert!(
            !defer_2
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
        assert!(
            !defer_2
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(!has_pre_break_chime(&defer_2));
        assert!(
            !defer_2
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );

        clock.advance_secs(2);
        let brk_actions = sched.tick_with_policies(Some(Duration::from_secs(2)), None);
        assert_eq!(sched.state(), SchedulerState::Break);
        assert!(
            brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );
        assert!(
            !brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
        assert!(
            !brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(!has_pre_break_chime(&brk_actions));
    }

    #[test]
    fn zero_warning_postpone_if_active_defers_silently_until_idle() {
        let clock = FakeClock::default();
        let mut cfg = test_config();
        cfg.pre_break_warning_secs = 0;
        cfg.smart_pause.postpone_if_active = true;
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(900);
        let defer_actions = sched.tick_with_policies(Some(Duration::ZERO), None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 2);
        assert!(
            !defer_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::NotifyPreBreak { .. }))
        );
        assert!(
            !defer_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
        assert!(
            !defer_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );

        clock.advance_secs(2);
        let brk_actions = sched.tick_with_policies(Some(Duration::from_secs(2)), None);
        assert_eq!(sched.state(), SchedulerState::Break);
        assert!(
            brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );
        assert!(
            !brk_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );
    }

    #[test]
    fn reload_config_theme_only_preserves_countdown() {
        let clock = FakeClock::default();
        let cfg = test_config();
        let mut sched = Scheduler::new(cfg.clone(), &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);
        clock.advance_secs(120);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 780);

        let mut theme_only = cfg;
        theme_only.theme = crate::config::ThemeMode::Dark;
        let actions = sched.handle_event(SchedulerEvent::ReloadConfig(theme_only));
        assert!(actions.is_empty());
        assert_eq!(sched.config().theme, crate::config::ThemeMode::Dark);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 780);
    }

    #[test]
    fn reload_config_language_only_preserves_countdown_and_updates_locale() {
        let clock = FakeClock::default();
        let cfg = test_config();
        let mut sched = Scheduler::new(cfg.clone(), &clock, 1);
        sched.set_system_locale(Locale::En);
        let _ = sched.handle_event(SchedulerEvent::Start);
        clock.advance_secs(120);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 780);
        assert_eq!(sched.active_locale(), Locale::En);

        let mut lang_only = cfg;
        lang_only.language = crate::i18n::LanguageMode::Vi;
        let actions = sched.handle_event(SchedulerEvent::ReloadConfig(lang_only));
        assert!(actions.is_empty());
        assert_eq!(sched.config().language, crate::i18n::LanguageMode::Vi);
        assert_eq!(sched.active_locale(), Locale::Vi);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 780);
    }

    #[test]
    fn overlay_request_uses_active_locale_for_skip_note() {
        let clock = FakeClock::default();
        let cfg = Config {
            language: crate::i18n::LanguageMode::Vi,
            limit_consecutive_skips: SkipLimitConfig {
                enabled: true,
                max_skips: 2,
            },
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);
        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        clock.advance_secs(3);
        let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped));

        let actions = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        let req = actions.into_iter().find_map(|a| match a {
            SchedulerAction::ShowOverlay(r) => Some(r),
            _ => None,
        });
        assert_eq!(req.as_ref().map(|r| r.locale), Some(Locale::Vi));
        assert_eq!(
            req.as_ref().and_then(|r| r.status_note.as_deref()),
            Some("Đã bỏ qua hoặc hoãn 1/2 lần nghỉ liên tiếp")
        );
    }

    #[test]
    fn strict_break_overrides_skip_limit_for_postpone_and_status_note() {
        let clock = FakeClock::default();
        let cfg = Config {
            strict_break: true,
            allow_postpone: true,
            postpone_duration_secs: 60,
            limit_consecutive_skips: SkipLimitConfig {
                enabled: true,
                max_skips: 1,
            },
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        let first = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert!(first.iter().any(|a| matches!(
            a,
            SchedulerAction::ShowOverlay(req)
                if req.strict_break && req.allow_postpone && req.status_note.is_none()
        )));
        clock.advance_secs(3);
        let _ = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Postponed {
            duration_secs: 60,
        }));
        assert_eq!(sched.state(), SchedulerState::Waiting);

        let second = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert!(second.iter().any(|a| matches!(
            a,
            SchedulerAction::ShowOverlay(req)
                if req.strict_break && req.allow_postpone && req.status_note.is_none()
        )));
    }

    #[test]
    fn pre_break_notification_counts_down_each_second_and_dismisses_after_overlay() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 42);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        let entry_actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert!(entry_actions.iter().any(|a| matches!(
            a,
            SchedulerAction::NotifyPreBreak {
                warning_secs: 10,
                ..
            }
        )));
        assert!(has_pre_break_chime(&entry_actions));

        let mut notified_secs = vec![10_u64];
        for expected in (1_u64..=9).rev() {
            clock.advance(Duration::from_millis(250));
            assert!(sched.tick().is_empty());
            clock.advance(Duration::from_millis(750));
            let actions = sched.tick();
            assert!(!has_pre_break_chime(&actions));
            assert_eq!(actions.len(), 1);
            assert!(matches!(
                actions.first(),
                Some(SchedulerAction::NotifyPreBreak { warning_secs, total_secs: 10, .. }) if *warning_secs == expected
            ));
            notified_secs.push(expected);
        }
        assert_eq!(notified_secs, vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);

        clock.advance(Duration::from_millis(500));
        assert!(sched.tick().is_empty());

        clock.advance(Duration::from_millis(500));
        let brk_actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Break);
        let overlay_idx = brk_actions
            .iter()
            .position(|a| matches!(a, SchedulerAction::ShowOverlay(_)));
        let dismiss_idx = brk_actions
            .iter()
            .position(|a| matches!(a, SchedulerAction::DismissNotification));
        assert!(overlay_idx.is_some());
        assert!(dismiss_idx.is_some());
        assert!(overlay_idx < dismiss_idx);
    }

    #[test]
    fn deferred_pre_break_does_not_restart_countdown_notifications() {
        let clock = FakeClock::default();
        let mut cfg = test_config();
        cfg.smart_pause.postpone_if_active = true;
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        let _ = sched.tick_with_policies(Some(Duration::ZERO), None);
        assert_eq!(sched.state(), SchedulerState::PreBreak);

        clock.advance_secs(10);
        let defer_actions = sched.tick_with_policies(Some(Duration::ZERO), None);
        assert!(
            defer_actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::DismissNotification))
        );

        clock.advance_secs(1);
        assert!(sched.tick().is_empty());
    }

    #[test]
    fn seconds_until_next_break_ceils_subsecond_remaining_to_match_pre_break() {
        let clock = FakeClock::default();
        let mut sched = Scheduler::new(test_config(), &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        clock.advance_secs(890);
        let _ = sched.tick();
        assert_eq!(sched.state(), SchedulerState::PreBreak);
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 10);

        clock.advance(Duration::from_millis(9_001));
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 1);
        let warn_actions = sched.tick();
        assert!(matches!(
            warn_actions.as_slice(),
            [SchedulerAction::NotifyPreBreak {
                warning_secs: 1,
                ..
            }]
        ));
        clock.advance(Duration::from_millis(998));
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 1);
        assert!(sched.tick().is_empty());

        clock.advance(Duration::from_millis(1));
        assert_eq!(sched.status_snapshot().seconds_until_next_break, 0);
        let actions = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Break);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::ShowOverlay(_)))
        );
    }

    #[test]
    fn overlay_launching_and_shown_sync_break_deadline() {
        let clock = FakeClock::default();
        let cfg = Config {
            short_break_duration_secs: 5,
            shortcut_disable_secs: 2,
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);
        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert_eq!(sched.state(), SchedulerState::Break);

        // Subprocess startup takes 3s on Windows; OverlayLaunching prevents early expiration.
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayLaunching)
                .is_empty()
        );
        clock.advance_secs(3);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Break);

        // Overlay becomes visible at t=3s: full 5s deadline starts now, without re-locking shortcuts.
        assert!(sched.handle_event(SchedulerEvent::OverlayShown).is_empty());
        assert!(!sched.is_shortcut_locked());

        // Duplicate OverlayShown or late OverlayLaunching must not extend the deadline again.
        clock.advance_secs(1);
        assert!(sched.handle_event(SchedulerEvent::OverlayShown).is_empty());
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayLaunching)
                .is_empty()
        );

        // At t=5s (original launch + 5s), the overlay is still at 00:03 and must NOT be killed.
        clock.advance_secs(1);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Break);

        // At t=8s (shown + 5s), completion grace keeps the break alive so the overlay can exit cleanly.
        clock.advance_secs(3);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Break);

        // If the overlay hangs past completion grace (t=10s), fallback tick still recovers.
        clock.advance_secs(2);
        let done = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            done.iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );
    }

    #[test]
    fn overlay_launching_timeout_recovers_without_shown_and_events_ignored_outside_break() {
        let clock = FakeClock::default();
        let cfg = Config {
            short_break_duration_secs: 4,
            shortcut_disable_secs: 2,
            ..test_config()
        };
        let mut sched = Scheduler::new(cfg, &clock, 1);
        let _ = sched.handle_event(SchedulerEvent::Start);

        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayLaunching)
                .is_empty()
        );
        assert!(sched.handle_event(SchedulerEvent::OverlayShown).is_empty());

        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayLaunching)
                .is_empty()
        );

        clock.advance_secs(13);
        assert!(sched.tick().is_empty());
        assert_eq!(sched.state(), SchedulerState::Break);

        clock.advance_secs(1);
        let done = sched.tick();
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            done.iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );

        // Fast startup (< shortcut_disable_secs) still accepts Skip once shortcut lock expires.
        let _ = sched.handle_event(SchedulerEvent::TakeBreak { kind: None });
        assert!(
            sched
                .handle_event(SchedulerEvent::OverlayLaunching)
                .is_empty()
        );
        clock.advance(Duration::from_millis(500));
        assert!(sched.handle_event(SchedulerEvent::OverlayShown).is_empty());
        assert!(sched.is_shortcut_locked());
        clock.advance(Duration::from_millis(1500));
        assert!(!sched.is_shortcut_locked());
        let skipped = sched.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Skipped));
        assert_eq!(sched.state(), SchedulerState::Waiting);
        assert!(
            skipped
                .iter()
                .any(|a| matches!(a, SchedulerAction::CloseOverlay))
        );
    }
}
