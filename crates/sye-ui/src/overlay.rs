//! Fullscreen break overlay view model and Native Shell (`nsh`) Cozy renderer.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nsh_base::button;
use nsh_core::element::desktop::quit;
use nsh_core::prelude::*;
use sye_core::config::BreakType;
use sye_core::scheduler::{OverlayOutcome, OverlayRequest};

use crate::UiError;
use crate::theme::{colors, radii};
use crate::ui::app_icon;

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

/// Supported keyboard shortcuts on the break overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Space,
}

/// Formats a non-negative number of seconds as `MM:SS` without overflow.
pub fn format_mm_ss(total_secs: u64) -> String {
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}

/// Pure, deterministic view model driving the fullscreen break overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayViewModel {
    request: OverlayRequest,
    elapsed: Duration,
    outcome: Option<OverlayOutcome>,
}

impl OverlayViewModel {
    /// Creates a new overlay view model at `elapsed = 0`.
    pub fn new(request: OverlayRequest) -> Self {
        Self {
            request,
            elapsed: Duration::ZERO,
            outcome: None,
        }
    }

    /// Returns the underlying `OverlayRequest`.
    pub fn request(&self) -> &OverlayRequest {
        &self.request
    }

    /// Returns the elapsed duration since the overlay opened.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Advances elapsed time by `delta` (saturating) and latches completion when reached.
    pub fn tick(&mut self, delta: Duration) -> Option<&OverlayOutcome> {
        self.elapsed = self.elapsed.saturating_add(delta);
        self.check_completion();
        self.outcome.as_ref()
    }

    /// Monotonically updates elapsed time and latches completion when reached.
    pub fn update_elapsed(&mut self, elapsed: Duration) -> Option<&OverlayOutcome> {
        self.elapsed = self.elapsed.max(elapsed);
        self.check_completion();
        self.outcome.as_ref()
    }

    /// Remaining break duration (saturating at zero).
    pub fn remaining(&self) -> Duration {
        Duration::from_secs(self.request.duration_secs).saturating_sub(self.elapsed)
    }

    /// Remaining seconds rounded up so `15s` displays `00:15` until a full second passes.
    pub fn remaining_secs_ceil(&self) -> u64 {
        duration_secs_ceil(self.remaining())
    }

    /// Remaining countdown formatted as `MM:SS`.
    pub fn formatted_remaining(&self) -> String {
        format_mm_ss(self.remaining_secs_ceil())
    }

    /// Fade-in opacity factor in `0.0..=1.0`.
    pub fn fade_in_alpha(&self) -> f32 {
        if self.request.fade_in_ms == 0 {
            return 1.0;
        }
        let fade_secs = self.request.fade_in_ms as f64 / 1000.0;
        (self.elapsed.as_secs_f64() / fade_secs).clamp(0.0, 1.0) as f32
    }

    /// Completion progress in `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if self.request.duration_secs == 0 {
            return 1.0;
        }
        (self.elapsed.as_secs_f64() / self.request.duration_secs as f64).clamp(0.0, 1.0) as f32
    }

    /// Returns `true` while accidental-input shortcut protection is still active.
    pub fn is_shortcut_locked(&self) -> bool {
        self.elapsed < Duration::from_secs(self.request.shortcut_disable_secs)
    }

    /// Remaining shortcut-lock seconds (rounded up).
    pub fn lock_remaining_secs(&self) -> u64 {
        duration_secs_ceil(
            Duration::from_secs(self.request.shortcut_disable_secs).saturating_sub(self.elapsed),
        )
    }

    /// Whether the Skip control should be shown.
    pub fn show_skip_button(&self) -> bool {
        !self.request.strict_break
    }

    /// Whether skipping is currently permitted.
    pub fn can_skip(&self) -> bool {
        self.outcome.is_none() && !self.request.strict_break && !self.is_shortcut_locked()
    }

    /// Whether the Postpone control should be shown.
    pub fn show_postpone_button(&self) -> bool {
        self.request.allow_postpone
    }

    /// Whether postponing is currently permitted.
    pub fn can_postpone(&self) -> bool {
        self.outcome.is_none() && self.request.allow_postpone && !self.is_shortcut_locked()
    }

    /// Optional non-empty trimmed status note to display on the overlay.
    pub fn status_note(&self) -> Option<&str> {
        self.request
            .status_note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Attempts to latch `OverlayOutcome::Skipped` if skipping is currently allowed.
    pub fn trigger_skip(&mut self) -> Option<&OverlayOutcome> {
        if self.can_skip() {
            self.outcome = Some(OverlayOutcome::Skipped);
        }
        self.outcome.as_ref()
    }

    /// Attempts to latch `OverlayOutcome::Postponed` if postponing is currently allowed.
    pub fn trigger_postpone(&mut self) -> Option<&OverlayOutcome> {
        if self.can_postpone() {
            self.outcome = Some(OverlayOutcome::Postponed {
                duration_secs: self.request.postpone_duration_secs,
            });
        }
        self.outcome.as_ref()
    }

    /// Dispatches a keyboard shortcut.
    pub fn handle_key(&mut self, key: OverlayKey) -> Option<&OverlayOutcome> {
        match key {
            OverlayKey::Escape => self.trigger_skip(),
            OverlayKey::Space => self.trigger_postpone(),
        }
    }

    /// Latches `OverlayOutcome::Closed` if no other outcome has been latched yet.
    pub fn trigger_external_close(&mut self) -> Option<&OverlayOutcome> {
        if self.outcome.is_none() {
            self.outcome = Some(OverlayOutcome::Closed);
        }
        self.outcome.as_ref()
    }

    /// Returns `true` once a terminal outcome has been latched.
    pub fn should_close(&self) -> bool {
        self.outcome.is_some()
    }

    /// Returns the latched outcome, if any.
    pub fn outcome(&self) -> Option<&OverlayOutcome> {
        self.outcome.as_ref()
    }

    /// Returns the latched outcome or `OverlayOutcome::Closed` if closed before latching.
    pub fn resolved_outcome(&self) -> OverlayOutcome {
        self.outcome.clone().unwrap_or(OverlayOutcome::Closed)
    }

    fn check_completion(&mut self) {
        if self.outcome.is_none() && self.elapsed >= Duration::from_secs(self.request.duration_secs)
        {
            self.outcome = Some(OverlayOutcome::Completed);
        }
    }
}

fn duration_secs_ceil(rem: Duration) -> u64 {
    if rem.subsec_nanos() > 0 {
        rem.as_secs().saturating_add(1)
    } else {
        rem.as_secs()
    }
}

struct OverlayLaunch {
    request: OverlayRequest,
    outcome_sink: Arc<Mutex<Option<OverlayOutcome>>>,
}

static OVERLAY_LAUNCH: Mutex<Option<OverlayLaunch>> = Mutex::new(None);

/// Builds the Cozy break overlay surface for both desktop runs and headless tests.
pub(crate) fn build_overlay_view(
    request: OverlayRequest,
    outcome_sink: Arc<Mutex<Option<OverlayOutcome>>>,
    quit_on_finish: bool,
) -> impl IntoElement {
    let vm = signal(OverlayViewModel::new(request));

    let finish_if_ready = {
        let sink = Arc::clone(&outcome_sink);
        move || {
            let terminal = vm.with(|m| m.outcome().cloned());
            if let Some(outcome) = terminal {
                crate::store_outcome(&sink, outcome);
                if quit_on_finish {
                    quit();
                }
            }
        }
    };

    // Drive fade-in and countdown ticks on the NSH engine clock (supports Clock::manual in tests).
    let finish_tick = finish_if_ready.clone();
    let _timer = set_interval(Duration::from_millis(100), move || {
        vm.update(|m| {
            let _ = m.tick(Duration::from_millis(100));
        });
        finish_tick();
    });

    let finish_esc = finish_if_ready.clone();
    on_key("escape", move |_| {
        vm.update(|m| {
            let _ = m.handle_key(OverlayKey::Escape);
        });
        finish_esc();
    });

    let finish_space = finish_if_ready.clone();
    on_key("space", move |_| {
        vm.update(|m| {
            let _ = m.handle_key(OverlayKey::Space);
        });
        finish_space();
    });

    let break_label: &'static str = vm.with(|m| match m.request().break_type {
        BreakType::Short => "SHORT BREAK",
        BreakType::Long => "LONG BREAK",
    });
    let exercise_title = vm.with(|m| m.request().title.clone());
    let status_note = vm.with(|m| m.status_note().map(str::to_owned));
    let show_skip = vm.with(|m| m.show_skip_button());
    let show_postpone = vm.with(|m| m.show_postpone_button());

    let finish_skip = finish_if_ready.clone();
    let finish_postpone = finish_if_ready;

    let mut actions_row = view().flex_row().items_center().justify_center().gap_3();
    if show_postpone {
        actions_row = actions_row.child(
            button()
                .on_press(move || {
                    vm.update(|m| {
                        let _ = m.trigger_postpone();
                    });
                    finish_postpone();
                })
                .disabled(move || vm.with(|m| !m.can_postpone()))
                .h(px(40.0))
                .px_5()
                .items_center()
                .justify_center()
                .rounded(radii::PILL)
                .bg(colors::OVERLAY_TRACK)
                .border_1()
                .border_color(colors::OVERLAY_BORDER_EMPHASIZED)
                .transition()
                .duration(crate::theme::motion::CONTROL_MS)
                .ease_out()
                .hover(|s| s.opacity(0.9))
                .active(|s| s.opacity(0.8))
                .disabled_style(|s| s.opacity(0.4))
                .child(
                    text("Postpone (Space)")
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors::OVERLAY_TEXT),
                ),
        );
    }
    if show_skip {
        actions_row = actions_row.child(
            button()
                .on_press(move || {
                    vm.update(|m| {
                        let _ = m.trigger_skip();
                    });
                    finish_skip();
                })
                .disabled(move || vm.with(|m| !m.can_skip()))
                .h(px(40.0))
                .px_5()
                .items_center()
                .justify_center()
                .rounded(radii::PILL)
                .bg(colors::ACCENT)
                .transition()
                .duration(crate::theme::motion::CONTROL_MS)
                .ease_out()
                .hover(|s| s.opacity(0.92))
                .active(|s| s.opacity(0.84))
                .disabled_style(|s| s.opacity(0.4))
                .child(
                    text("Skip (Esc)")
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors::ON_ACCENT),
                ),
        );
    }

    let card_scale = animated_value(if reduce_motion() { 1.0 } else { 0.98 });
    if !reduce_motion() {
        card_scale.timing(
            1.0,
            crate::theme::motion::THEME_COLOR_MS,
            crate::theme::motion::COZY_EASE,
        );
    }

    let mut card = view()
        .w(px(560.0))
        .max_w(percentage(0.9))
        .rounded(radii::PAGE)
        .bg(colors::OVERLAY_CARD)
        .border_1()
        .border_color(colors::OVERLAY_BORDER)
        .p_8()
        .gap_5()
        .items_center()
        .bind_scale(&card_scale)
        .child(app_icon(48.0))
        .child(
            view()
                .px_3()
                .py_1()
                .rounded(radii::PILL)
                .bg(colors::ACCENT_SOFT)
                .child(
                    text(break_label)
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors::ACCENT),
                ),
        )
        .child(
            text(exercise_title)
                .text_2xl()
                .font_weight(FontWeight::BOLD)
                .text_color(colors::OVERLAY_TEXT),
        )
        .child(
            text(move || vm.with(OverlayViewModel::formatted_remaining))
                .text_4xl()
                .font_weight(FontWeight::BOLD)
                .text_color(colors::ACCENT),
        )
        .child(
            view()
                .w(px(360.0))
                .max_w(percentage(1.0))
                .h(px(8.0))
                .rounded(radii::PILL)
                .bg(colors::OVERLAY_TRACK)
                .child(
                    view()
                        .h(px(8.0))
                        .rounded(radii::PILL)
                        .bg(colors::ACCENT)
                        .transition_all()
                        .duration(180.0)
                        .ease_linear()
                        .w(move || percentage(vm.with(OverlayViewModel::progress))),
                ),
        );

    if let Some(note) = status_note {
        card = card.child(
            text(note)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::OVERLAY_MUTED),
        );
    }

    card = card
        .child(
            text(move || {
                vm.with(|m| {
                    if m.is_shortcut_locked() {
                        format!("Controls unlock in {}s", m.lock_remaining_secs())
                    } else if m.request().strict_break && !m.request().allow_postpone {
                        "Strict break active — rest your eyes".to_owned()
                    } else {
                        "Relax your gaze until the timer completes".to_owned()
                    }
                })
            })
            .text_xs()
            .text_color(colors::OVERLAY_MUTED),
        )
        .child(actions_row);

    view()
        .flex_1()
        .bg(colors::OVERLAY_CANVAS)
        .items_center()
        .justify_center()
        .p_6()
        .opacity(move || vm.with(OverlayViewModel::fade_in_alpha))
        .child(card)
}

#[component]
fn BreakOverlayRoot() -> impl IntoElement {
    let launch = OVERLAY_LAUNCH.lock().ok().and_then(|mut g| g.take());
    match launch {
        Some(launch) => {
            build_overlay_view(launch.request, launch.outcome_sink, true).into_element()
        }
        None => view()
            .flex_1()
            .bg(colors::OVERLAY_CANVAS)
            .items_center()
            .justify_center()
            .child(text("Safe Your Eyes").text_color(colors::OVERLAY_TEXT))
            .into_element(),
    }
}

/// Runs the Cozy fullscreen break overlay using Native Shell (`nsh`) and returns `OverlayOutcome`.
pub fn run_overlay(request: OverlayRequest) -> Result<OverlayOutcome, UiError> {
    let outcome_sink = Arc::new(Mutex::new(None));
    if let Ok(mut guard) = OVERLAY_LAUNCH.lock() {
        *guard = Some(OverlayLaunch {
            request,
            outcome_sink: Arc::clone(&outcome_sink),
        });
    } else {
        return Err(UiError::WindowInit(
            "overlay launch state lock poisoned".to_owned(),
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let _ = platform::run_app(
            platform::AppSource::Rust(platform::Root::of::<BreakOverlayRoot>()),
            platform::AppOptions {
                title: "Safe Your Eyes — Break".to_owned(),
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                app_id: "io.github.tunglt1810.safeyoureyes.overlay".to_owned(),
                size: (960, 640),
                appearance: Appearance::Dark,
                ..Default::default()
            },
        );
    }

    let resolved = outcome_sink
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .unwrap_or(OverlayOutcome::Closed);
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> OverlayRequest {
        OverlayRequest {
            break_type: BreakType::Short,
            title: "Focus on a point in the far distance".to_owned(),
            duration_secs: 15,
            strict_break: false,
            allow_postpone: true,
            postpone_duration_secs: 300,
            shortcut_disable_secs: 2,
            fade_in_ms: 1500,
            status_note: Some("  Take a deep breath  ".to_owned()),
        }
    }

    #[test]
    fn format_mm_ss_handles_zero_minutes_hours_and_max() {
        assert_eq!(format_mm_ss(0), "00:00");
        assert_eq!(format_mm_ss(5), "00:05");
        assert_eq!(format_mm_ss(60), "01:00");
        assert_eq!(format_mm_ss(75), "01:15");
        assert_eq!(format_mm_ss(3661), "61:01");
        assert_eq!(format_mm_ss(u64::MAX), "307445734561825860:15");
    }

    #[test]
    fn countdown_ceil_and_auto_complete() {
        let mut vm = OverlayViewModel::new(sample_request());
        assert_eq!(vm.remaining_secs_ceil(), 15);
        assert_eq!(vm.formatted_remaining(), "00:15");
        assert!(!vm.should_close());

        assert_eq!(vm.tick(Duration::from_millis(1)), None);
        assert_eq!(vm.remaining_secs_ceil(), 15);
        assert_eq!(vm.formatted_remaining(), "00:15");

        assert_eq!(vm.tick(Duration::from_millis(999)), None);
        assert_eq!(vm.remaining_secs_ceil(), 14);
        assert_eq!(vm.formatted_remaining(), "00:14");

        assert_eq!(
            vm.tick(Duration::from_secs(14)),
            Some(&OverlayOutcome::Completed)
        );
        assert_eq!(vm.remaining_secs_ceil(), 0);
        assert_eq!(vm.formatted_remaining(), "00:00");
        assert!(vm.should_close());
        assert_eq!(vm.resolved_outcome(), OverlayOutcome::Completed);
    }

    #[test]
    fn update_elapsed_is_monotonic() {
        let mut vm = OverlayViewModel::new(sample_request());
        let _ = vm.update_elapsed(Duration::from_secs(5));
        let _ = vm.update_elapsed(Duration::from_secs(2));
        assert_eq!(vm.elapsed(), Duration::from_secs(5));
    }

    #[test]
    fn fade_in_alpha_and_progress_behave_at_boundaries() {
        let mut req = sample_request();
        req.fade_in_ms = 1000;
        req.duration_secs = 10;
        let mut vm = OverlayViewModel::new(req);

        assert_eq!(vm.fade_in_alpha(), 0.0);
        assert_eq!(vm.progress(), 0.0);

        let _ = vm.tick(Duration::from_millis(500));
        assert!((vm.fade_in_alpha() - 0.5).abs() < 1e-4);
        assert!((vm.progress() - 0.05).abs() < 1e-4);

        let _ = vm.tick(Duration::from_secs(2));
        assert_eq!(vm.fade_in_alpha(), 1.0);
        assert!((vm.progress() - 0.25).abs() < 1e-4);

        let mut zero_req = sample_request();
        zero_req.fade_in_ms = 0;
        zero_req.duration_secs = 0;
        let mut zero_vm = OverlayViewModel::new(zero_req);
        assert_eq!(zero_vm.fade_in_alpha(), 1.0);
        assert_eq!(zero_vm.progress(), 1.0);
        assert_eq!(
            zero_vm.tick(Duration::ZERO),
            Some(&OverlayOutcome::Completed)
        );
    }

    #[test]
    fn shortcut_lock_blocks_skip_and_postpone_until_elapsed() {
        let mut vm = OverlayViewModel::new(sample_request());
        assert!(vm.is_shortcut_locked());
        assert_eq!(vm.lock_remaining_secs(), 2);
        assert!(!vm.can_skip());
        assert!(!vm.can_postpone());
        assert_eq!(vm.handle_key(OverlayKey::Escape), None);
        assert_eq!(vm.handle_key(OverlayKey::Space), None);

        let _ = vm.tick(Duration::from_millis(1500));
        assert!(vm.is_shortcut_locked());
        assert_eq!(vm.lock_remaining_secs(), 1);
        assert_eq!(vm.trigger_skip(), None);

        let _ = vm.tick(Duration::from_millis(500));
        assert!(!vm.is_shortcut_locked());
        assert_eq!(vm.lock_remaining_secs(), 0);
        assert!(vm.can_skip());
        assert!(vm.can_postpone());
        assert_eq!(
            vm.handle_key(OverlayKey::Escape),
            Some(&OverlayOutcome::Skipped)
        );
    }

    #[test]
    fn strict_break_hides_and_disables_skip() {
        let mut req = sample_request();
        req.strict_break = true;
        req.shortcut_disable_secs = 0;
        let mut vm = OverlayViewModel::new(req);

        assert!(!vm.show_skip_button());
        assert!(!vm.can_skip());
        assert_eq!(vm.handle_key(OverlayKey::Escape), None);
        assert!(vm.show_postpone_button());
        assert!(vm.can_postpone());
        assert_eq!(
            vm.handle_key(OverlayKey::Space),
            Some(&OverlayOutcome::Postponed { duration_secs: 300 })
        );
    }

    #[test]
    fn postpone_disabled_hides_and_blocks_postpone() {
        let mut req = sample_request();
        req.allow_postpone = false;
        req.shortcut_disable_secs = 0;
        let mut vm = OverlayViewModel::new(req);

        assert!(!vm.show_postpone_button());
        assert!(!vm.can_postpone());
        assert_eq!(vm.handle_key(OverlayKey::Space), None);
        assert_eq!(vm.trigger_postpone(), None);
    }

    #[test]
    fn outcome_latching_is_idempotent_first_wins() {
        let mut req = sample_request();
        req.shortcut_disable_secs = 0;
        let mut vm = OverlayViewModel::new(req);

        assert_eq!(
            vm.trigger_postpone(),
            Some(&OverlayOutcome::Postponed { duration_secs: 300 })
        );
        assert_eq!(
            vm.trigger_skip(),
            Some(&OverlayOutcome::Postponed { duration_secs: 300 })
        );
        assert_eq!(
            vm.trigger_external_close(),
            Some(&OverlayOutcome::Postponed { duration_secs: 300 })
        );
        assert_eq!(
            vm.tick(Duration::from_secs(60)),
            Some(&OverlayOutcome::Postponed { duration_secs: 300 })
        );
    }

    #[test]
    fn external_close_latches_closed_when_unlatched() {
        let mut vm = OverlayViewModel::new(sample_request());
        assert_eq!(vm.resolved_outcome(), OverlayOutcome::Closed);
        assert_eq!(vm.trigger_external_close(), Some(&OverlayOutcome::Closed));
        assert!(vm.should_close());
    }

    #[test]
    fn status_note_trims_and_ignores_blank() {
        let vm = OverlayViewModel::new(sample_request());
        assert_eq!(vm.status_note(), Some("Take a deep breath"));

        let mut blank_req = sample_request();
        blank_req.status_note = Some("   \t\n ".to_owned());
        let blank_vm = OverlayViewModel::new(blank_req);
        assert_eq!(blank_vm.status_note(), None);
    }

    #[test]
    fn nsh_headless_overlay_renders_title_countdown_and_skip() -> Result<(), String> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        #[component]
        fn TestOverlay() -> impl IntoElement {
            let mut req = sample_request();
            req.shortcut_disable_secs = 0;
            req.fade_in_ms = 0;
            let sink = Arc::new(Mutex::new(None));
            build_overlay_view(req, sink, false)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(960.0), Points(640.0)),
            ..EngineConfig::new(Root::of::<TestOverlay>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("SHORT BREAK")?;
        assert!(app.shows("Focus on a point in the far distance"));
        assert!(app.shows("00:15"));
        assert!(app.shows("Skip (Esc)"));
        assert!(app.shows("Postpone (Space)"));
        app.press("Skip (Esc)")?;
        Ok(())
    }
}
