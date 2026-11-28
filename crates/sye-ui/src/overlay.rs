use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nsh_base::button;
use nsh_core::desktop::{
    MainWindowOptions, QuitMode, WindowAttributes, WindowFrameStyle, WindowKind, WindowLevel,
    WindowPosition, Workspaces,
};
use nsh_core::element::desktop::{current_window, displays, quit, set_quit_mode};
use nsh_core::prelude::*;
use sye_core::config::BreakType;
use sye_core::localize_exercise_name;
use sye_core::scheduler::{OverlayOutcome, OverlayRequest};

use crate::UiError;
use crate::i18n::{format_overlay_unlock_hint, ui_text};
use crate::theme::{colors, radii};
use crate::ui::app_icon;

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Space,
}

#[must_use]
pub fn format_mm_ss(total_secs: u64) -> String {
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayViewModel {
    request: OverlayRequest,
    elapsed: Duration,
    outcome: Option<OverlayOutcome>,
}

impl OverlayViewModel {
    #[must_use]
    pub const fn new(request: OverlayRequest) -> Self {
        Self {
            request,
            elapsed: Duration::ZERO,
            outcome: None,
        }
    }

    #[must_use]
    pub const fn request(&self) -> &OverlayRequest {
        &self.request
    }

    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn tick(&mut self, delta: Duration) -> Option<&OverlayOutcome> {
        self.elapsed = self.elapsed.saturating_add(delta);
        self.check_completion();
        self.outcome.as_ref()
    }

    pub fn update_elapsed(&mut self, elapsed: Duration) -> Option<&OverlayOutcome> {
        self.elapsed = self.elapsed.max(elapsed);
        self.check_completion();
        self.outcome.as_ref()
    }

    #[must_use]
    pub const fn remaining(&self) -> Duration {
        Duration::from_secs(self.request.duration_secs).saturating_sub(self.elapsed)
    }

    #[must_use]
    pub const fn remaining_secs_ceil(&self) -> u64 {
        duration_secs_ceil(self.remaining())
    }

    #[must_use]
    pub fn formatted_remaining(&self) -> String {
        format_mm_ss(self.remaining_secs_ceil())
    }

    #[must_use]
    pub fn fade_in_alpha(&self) -> f32 {
        if self.request.fade_in_ms == 0 {
            return 1.0;
        }
        let fade_secs = self.request.fade_in_ms as f64 / 1000.0;
        (self.elapsed.as_secs_f64() / fade_secs).clamp(0.0, 1.0) as f32
    }

    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.request.duration_secs == 0 {
            return 1.0;
        }
        (self.elapsed.as_secs_f64() / self.request.duration_secs as f64).clamp(0.0, 1.0) as f32
    }

    #[must_use]
    pub fn remaining_ratio(&self) -> f32 {
        (1.0 - self.progress()).clamp(0.0, 1.0)
    }

    #[must_use]
    pub fn remaining_ratio_discrete(&self) -> f32 {
        if self.request.duration_secs == 0 {
            return 0.0;
        }
        (self.remaining_secs_ceil() as f64 / self.request.duration_secs as f64).clamp(0.0, 1.0)
            as f32
    }

    #[must_use]
    pub fn is_shortcut_locked(&self) -> bool {
        self.elapsed < Duration::from_secs(self.request.shortcut_disable_secs)
    }

    #[must_use]
    pub const fn lock_remaining_secs(&self) -> u64 {
        duration_secs_ceil(
            Duration::from_secs(self.request.shortcut_disable_secs).saturating_sub(self.elapsed),
        )
    }

    #[must_use]
    pub const fn show_skip_button(&self) -> bool {
        !self.request.strict_break
    }

    #[must_use]
    pub fn can_skip(&self) -> bool {
        self.outcome.is_none() && !self.request.strict_break && !self.is_shortcut_locked()
    }

    #[must_use]
    pub const fn show_postpone_button(&self) -> bool {
        self.request.allow_postpone
    }

    #[must_use]
    pub fn can_postpone(&self) -> bool {
        self.outcome.is_none() && self.request.allow_postpone && !self.is_shortcut_locked()
    }

    #[must_use]
    pub fn status_note(&self) -> Option<&str> {
        self.request
            .status_note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub fn trigger_skip(&mut self) -> Option<&OverlayOutcome> {
        if self.can_skip() {
            self.outcome = Some(OverlayOutcome::Skipped);
        }
        self.outcome.as_ref()
    }

    pub fn trigger_postpone(&mut self) -> Option<&OverlayOutcome> {
        if self.can_postpone() {
            self.outcome = Some(OverlayOutcome::Postponed {
                duration_secs: self.request.postpone_duration_secs,
            });
        }
        self.outcome.as_ref()
    }

    pub fn handle_key(&mut self, key: OverlayKey) -> Option<&OverlayOutcome> {
        match key {
            OverlayKey::Escape => self.trigger_skip(),
            OverlayKey::Space => self.trigger_postpone(),
        }
    }

    pub fn trigger_external_close(&mut self) -> Option<&OverlayOutcome> {
        if self.outcome.is_none() {
            self.outcome = Some(OverlayOutcome::Closed);
        }
        self.outcome.as_ref()
    }

    #[must_use]
    pub const fn should_close(&self) -> bool {
        self.outcome.is_some()
    }

    #[must_use]
    pub const fn outcome(&self) -> Option<&OverlayOutcome> {
        self.outcome.as_ref()
    }

    #[must_use]
    pub const fn resolved_outcome(&self) -> OverlayOutcome {
        match self.outcome {
            Some(outcome) => outcome,
            None => OverlayOutcome::Closed,
        }
    }

    fn check_completion(&mut self) {
        if self.outcome.is_none() && self.elapsed >= Duration::from_secs(self.request.duration_secs)
        {
            self.outcome = Some(OverlayOutcome::Completed);
        }
    }
}

const fn duration_secs_ceil(rem: Duration) -> u64 {
    if rem.subsec_nanos() > 0 {
        rem.as_secs().saturating_add(1)
    } else {
        rem.as_secs()
    }
}

pub type OverlayShownCallback = Box<dyn Fn() + Send>;

pub type OverlayOutcomeCallback = Arc<dyn Fn(OverlayOutcome) + Send + Sync>;

struct OverlayLaunch {
    request: OverlayRequest,
    outcome_sink: Arc<Mutex<Option<OverlayOutcome>>>,
    on_shown: Option<OverlayShownCallback>,
    on_outcome: Option<OverlayOutcomeCallback>,
}

static OVERLAY_LAUNCH: Mutex<Option<OverlayLaunch>> = Mutex::new(None);

fn base_overlay_window_attributes(interactive: bool) -> WindowAttributes {
    let mut attrs = WindowAttributes::default();
    attrs.frame = Some(WindowFrameStyle::None);
    attrs.level = Some(WindowLevel::AboveDock);
    attrs.workspaces = Some(Workspaces::All);
    attrs.resizable = Some(false);
    attrs.movable = Some(false);
    attrs.minimizable = Some(false);
    attrs.maximizable = Some(false);
    attrs.closable = Some(false);
    attrs.focusable = Some(interactive);
    attrs.activate = Some(interactive);
    attrs.close_on_escape = Some(false);
    attrs.close_on_blur = Some(false);
    attrs.close_on_press = Some(false);
    attrs.taskbar = Some(interactive && cfg!(target_os = "windows"));
    attrs.shadow = Some(false);
    attrs.transparent = Some(true);
    attrs
}

fn overlay_main_window_options_for_primary(
    primary: Option<crate::MonitorRect>,
) -> MainWindowOptions {
    let mut main_window = MainWindowOptions::default();
    if let Some(rect) = primary {
        main_window.default_size = Some(size(px(rect.width), px(rect.height)));
        main_window.default_position = Some(WindowPosition::At(point(px(rect.x), px(rect.y))));
    } else if cfg!(not(target_os = "windows")) {
        main_window.default_size = Some(size(px(1920.0), px(1080.0)));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        main_window.default_mode = Some(nsh_core::desktop::WindowMode::FullScreen);
    }
    main_window.attributes = base_overlay_window_attributes(true);
    main_window
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn apply_window_bounds(win: &nsh_core::element::desktop::WindowRef, b: Bounds<Points>) {
    if b.size.width.is_finite()
        && b.size.width.0 > 0.0
        && b.size.height.is_finite()
        && b.size.height.0 > 0.0
    {
        win.set_bounds(nsh_core::desktop::WindowFrame {
            x: Some(b.origin.x),
            y: Some(b.origin.y),
            width: Some(b.size.width),
            height: Some(b.size.height),
            animated: Some(false),
        });
    }
}

fn overlay_main_window_options() -> MainWindowOptions {
    let plan = crate::monitors::plan_overlay_viewports(&crate::monitors::detect_monitors());
    overlay_main_window_options_for_primary(plan.primary)
}

fn secondary_window_attributes() -> WindowAttributes {
    base_overlay_window_attributes(false)
}

const PROGRESS_TRACK_W: f32 = 280.0;
const PROGRESS_TRACK_H: f32 = 5.0;

#[derive(Clone, Copy)]
struct Entrance {
    fade: AnimatedValue,
    card_scale: AnimatedValue,
    remaining_scale: AnimatedValue,
}

fn remaining_scale_for(vm: &OverlayViewModel, reduced: bool) -> f32 {
    if reduced {
        vm.remaining_ratio_discrete()
    } else {
        vm.remaining_ratio()
    }
}

impl Entrance {
    fn new(vm: Signal<OverlayViewModel>) -> Self {
        let reduced = reduce_motion();
        let (alpha, remaining_scale) =
            vm.with(|m| (m.fade_in_alpha(), remaining_scale_for(m, reduced)));
        let entrance = Self {
            fade: animated_value(alpha),
            card_scale: animated_value(if reduced { 1.0 } else { 0.98 }),
            remaining_scale: animated_value(remaining_scale),
        };
        if reduced {
            let scale = entrance.remaining_scale;
            let remaining_secs = computed(move || vm.with(OverlayViewModel::remaining_secs_ceil));
            effect(move || {
                let _ = remaining_secs.get();
                scale.set(vm.with(OverlayViewModel::remaining_ratio_discrete));
            });
        }
        entrance
    }

    fn play(&self, vm: Signal<OverlayViewModel>) {
        let reduced = reduce_motion();
        let (alpha, fade_ms, remaining_scale, remaining) = vm.with(|m| {
            (
                m.fade_in_alpha(),
                m.request.fade_in_ms,
                remaining_scale_for(m, reduced),
                m.remaining(),
            )
        });
        if alpha < 1.0 {
            let _ = self
                .fade
                .timing(1.0, fade_ms as f32 * (1.0 - alpha), Easing::Linear);
        }
        if !reduced {
            let _ = self.card_scale.timing(
                1.0,
                crate::theme::motion::THEME_COLOR_MS,
                crate::theme::motion::COZY_EASE,
            );
        }
        self.remaining_scale.set(remaining_scale);
        if !reduced && remaining_scale > 0.0 {
            let _ =
                self.remaining_scale
                    .timing(0.0, remaining.as_secs_f32() * 1000.0, Easing::Linear);
        }
    }
}

fn overlay_action_button(
    label: &'static str,
    disabled: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .disabled(disabled)
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
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::OVERLAY_TEXT),
        )
}

fn build_overlay_card(
    vm: Signal<OverlayViewModel>,
    entrance: &Entrance,
    finish_if_ready: impl Fn() + Clone + 'static,
) -> impl IntoElement {
    let locale = vm.with(|m| m.request().locale);
    let copy = ui_text(locale);
    let break_label: &'static str = vm.with(|m| match m.request().break_type {
        BreakType::Short => copy.overlay_short_badge,
        BreakType::Long => copy.overlay_long_badge,
    });
    let exercise_title = vm.with(|m| localize_exercise_name(&m.request().title, locale).to_owned());
    let status_note = vm.with(|m| m.status_note().map(str::to_owned));
    let show_skip = vm.with(OverlayViewModel::show_skip_button);
    let show_postpone = vm.with(OverlayViewModel::show_postpone_button);

    let finish_skip = finish_if_ready.clone();
    let finish_postpone = finish_if_ready;

    let mut actions_row = view().flex_row().items_center().justify_center().gap_3();
    if show_postpone {
        actions_row = actions_row.child(overlay_action_button(
            copy.overlay_postpone_button,
            move || vm.with(|m| !m.can_postpone()),
            move || {
                vm.update(|m| {
                    let _ = m.trigger_postpone();
                });
                finish_postpone();
            },
        ));
    }
    if show_skip {
        actions_row = actions_row.child(overlay_action_button(
            copy.overlay_skip_button,
            move || vm.with(|m| !m.can_skip()),
            move || {
                vm.update(|m| {
                    let _ = m.trigger_skip();
                });
                finish_skip();
            },
        ));
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
        .bind_scale(&entrance.card_scale)
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
            view()
                .items_center()
                .gap_3()
                .child(
                    text(move || vm.with(OverlayViewModel::formatted_remaining))
                        .text_4xl()
                        .font_weight(FontWeight::BOLD)
                        .tabular_nums()
                        .text_color(colors::ACCENT),
                )
                .child(
                    view()
                        .w(px(PROGRESS_TRACK_W))
                        .h(px(PROGRESS_TRACK_H))
                        .rounded(radii::PILL)
                        .bg(colors::OVERLAY_TRACK)
                        .overflow_hidden()
                        .accessible(true)
                        .role(AccessibilityRole::Progressbar)
                        .label(copy.overlay_progress_a11y)
                        .accessibility_value(move || {
                            vm.with(|m| {
                                Some(nsh_core::AccessibilityValue {
                                    min: Some(0.0),
                                    max: Some(m.request().duration_secs as f32),
                                    now: Some(m.remaining_secs_ceil() as f32),
                                    text: Some(Arc::from(m.formatted_remaining())),
                                })
                            })
                        })
                        .child(
                            view()
                                .w(px(PROGRESS_TRACK_W))
                                .h(px(PROGRESS_TRACK_H))
                                .rounded(radii::PILL)
                                .bg(colors::ACCENT)
                                .origin_center()
                                .bind_transform(
                                    nsh_core::TransformKind::ScaleX,
                                    &entrance.remaining_scale,
                                ),
                        ),
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

    card.child(
        text(move || {
            vm.with(|m| {
                if m.is_shortcut_locked() {
                    format_overlay_unlock_hint(m.lock_remaining_secs(), locale)
                } else if m.request().strict_break && !m.request().allow_postpone {
                    copy.overlay_strict_hint.to_owned()
                } else {
                    copy.overlay_relax_hint.to_owned()
                }
            })
        })
        .text_xs()
        .text_color(colors::OVERLAY_MUTED),
    )
    .child(actions_row)
}

fn primary_display(list: &[nsh_core::desktop::Display]) -> Option<&nsh_core::desktop::Display> {
    list.iter().find(|d| d.primary).or_else(|| list.first())
}

fn overlay_canvas(
    vm: Signal<OverlayViewModel>,
    entrance: Entrance,
    finish_if_ready: impl Fn() + Clone + 'static,
) -> impl IntoElement {
    view()
        .flex_1()
        .bg(colors::OVERLAY_CANVAS)
        .items_center()
        .justify_center()
        .p_6()
        .bind_opacity(&entrance.fade)
        .child(build_overlay_card(vm, &entrance, finish_if_ready))
}

pub(crate) fn build_overlay_view(
    request: OverlayRequest,
    outcome_sink: Arc<Mutex<Option<OverlayOutcome>>>,
    quit_on_finish: bool,
    on_shown: Option<OverlayShownCallback>,
    on_outcome: Option<OverlayOutcomeCallback>,
) -> impl IntoElement {
    let vm = signal(OverlayViewModel::new(request));

    let finish_if_ready = move || {
        let terminal = vm.with(|m| m.outcome().copied());
        if let Some(outcome) = terminal {
            let first = outcome_sink.lock().is_ok_and(|guard| guard.is_none());
            crate::store_outcome(&outcome_sink, outcome);
            if first && let Some(hook) = on_outcome.as_ref() {
                hook(outcome);
            }
            if quit_on_finish {
                quit();
            }
        }
    };

    let finish_tick = finish_if_ready.clone();
    let started_at: Arc<Mutex<Option<nsh_core::time::Instant>>> = Arc::new(Mutex::new(None));
    let shown = signal(false);
    let tick_started = Arc::clone(&started_at);
    let _timer = set_interval(Duration::from_millis(100), move || {
        let Some(started) = tick_started.lock().ok().and_then(|g| *g) else {
            return;
        };
        let elapsed = nsh_core::time::now().saturating_duration_since(started);
        vm.update(|m| {
            let _ = m.update_elapsed(elapsed);
        });
        finish_tick();
    });

    let key_opts = KeyOptions {
        scope: KeyScope::App,
        ..Default::default()
    };
    let finish_esc = finish_if_ready.clone();
    on_key_with("escape", key_opts, move |_| {
        vm.update(|m| {
            let _ = m.handle_key(OverlayKey::Escape);
        });
        finish_esc();
    });

    let finish_space = finish_if_ready.clone();
    on_key_with("space", key_opts, move |_| {
        vm.update(|m| {
            let _ = m.handle_key(OverlayKey::Space);
        });
        finish_space();
    });

    if !quit_on_finish {
        let entrance = Entrance::new(vm);
        if let Ok(mut guard) = started_at.lock() {
            *guard = Some(nsh_core::time::now());
        }
        shown.set(true);
        entrance.play(vm);
        return overlay_canvas(vm, entrance, finish_if_ready).into_element();
    }

    set_quit_mode(QuitMode::LastWindowClosed);

    let primary_opts = overlay_main_window_options();
    let initial_displays = displays();
    let initial_primary = primary_display(&initial_displays);
    let overlay_title = ui_text(vm.with(|m| m.request().locale)).overlay_window_title;
    let mut primary_win_el = nsh_core::element::desktop::window()
        .id("overlay-primary")
        .kind(WindowKind::Normal)
        .title(overlay_title)
        .attributes(primary_opts.attributes)
        .on_open_change(|e| {
            if !e.open {
                quit();
            }
        });
    if let Some(primary) = initial_primary {
        primary_win_el = primary_win_el
            .display(primary.id)
            .default_position(WindowPosition::At(primary.bounds.origin))
            .default_size(primary.bounds.size);
    } else {
        if let Some(sz) = primary_opts.default_size {
            primary_win_el = primary_win_el.default_size(sz);
        }
        if let Some(pos) = primary_opts.default_position {
            primary_win_el = primary_win_el.default_position(pos);
        }
    }
    if let Some(mode) = primary_opts.default_mode {
        primary_win_el = primary_win_el.default_mode(mode);
    }

    let on_shown_slot = Rc::new(RefCell::new(on_shown));
    let finish_primary = finish_if_ready.clone();
    let primary_win = primary_win_el.content(move || {
        let entrance = Entrance::new(vm);
        let main_win = current_window();
        let effect_win = main_win.clone();
        effect(move || {
            let disp_list = displays();
            if let Some(primary) = primary_display(&disp_list) {
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                apply_window_bounds(&effect_win, primary.bounds);
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    let _ = primary;
                    effect_win.set_mode(nsh_core::desktop::WindowMode::FullScreen);
                }
            }
        });
        main_win.focus();
        let focus_win = main_win;
        let show_started = Arc::clone(&started_at);
        let on_shown_cb = on_shown_slot.borrow_mut().take();
        drop(set_timeout(Duration::from_millis(50), move || {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                let disp_list = displays();
                if let Some(primary) = primary_display(&disp_list) {
                    apply_window_bounds(&focus_win, primary.bounds);
                }
            }
            focus_win.focus();
            if let Ok(mut guard) = show_started.lock() {
                *guard = Some(nsh_core::time::now());
            }
            shown.set(true);
            entrance.play(vm);
            if let Some(hook) = on_shown_cb {
                hook();
            }
        }));
        overlay_canvas(vm, entrance, finish_primary.clone())
    });

    let secondary_displays = computed(|| {
        let disp_list = displays();
        if disp_list.len() <= 1 {
            return Vec::new();
        }
        let primary_idx = disp_list.iter().position(|d| d.primary).unwrap_or(0);
        disp_list
            .into_iter()
            .enumerate()
            .filter_map(|(idx, d)| (idx != primary_idx).then_some(d))
            .collect::<Vec<_>>()
    });

    view()
        .child(primary_win)
        .child(each(
            secondary_displays,
            |d| d.id.0,
            move |d, _| {
                let (disp_id, disp_bounds) = d.with(|disp| (disp.id, disp.bounds));
                let finish_if_ready = finish_if_ready.clone();
                let sec_win_el = nsh_core::element::desktop::window()
                    .id(format!("overlay-secondary-{}", disp_id.0))
                    .kind(WindowKind::Popup)
                    .attributes(secondary_window_attributes())
                    .display(disp_id)
                    .default_position(WindowPosition::At(disp_bounds.origin))
                    .default_size(disp_bounds.size);
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let sec_win_el = sec_win_el.default_mode(nsh_core::desktop::WindowMode::FullScreen);
                sec_win_el.content(move || {
                    let entrance = Entrance::new(vm);
                    let ready = signal(cfg!(not(any(target_os = "macos", target_os = "windows"))));
                    let played = std::sync::atomic::AtomicBool::new(false);
                    effect(move || {
                        if ready.get()
                            && shown.get()
                            && !played.swap(true, std::sync::atomic::Ordering::AcqRel)
                        {
                            entrance.play(vm);
                        }
                    });
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    {
                        let sec_win = current_window();
                        let apply_bounds =
                            move |b: Bounds<Points>| apply_window_bounds(&sec_win, b);
                        let apply_reactive = apply_bounds.clone();
                        effect(move || {
                            apply_reactive(d.with(|disp| disp.bounds));
                        });
                        drop(set_timeout(Duration::from_millis(16), move || {
                            apply_bounds(d.with(|disp| disp.bounds));
                            ready.set(true);
                        }));
                    }
                    overlay_canvas(vm, entrance, finish_if_ready.clone())
                })
            },
        ))
        .into_element()
}

#[component]
fn BreakOverlayRoot() -> impl IntoElement {
    let launch = OVERLAY_LAUNCH.lock().ok().and_then(|mut g| g.take());
    match launch {
        Some(launch) => build_overlay_view(
            launch.request,
            launch.outcome_sink,
            true,
            launch.on_shown,
            launch.on_outcome,
        )
        .into_element(),
        None => view()
            .flex_1()
            .bg(colors::OVERLAY_CANVAS)
            .items_center()
            .justify_center()
            .child(text("Safe Your Eyes").text_color(colors::OVERLAY_TEXT))
            .into_element(),
    }
}

pub fn run_overlay_with_hook(
    request: OverlayRequest,
    on_shown: Option<OverlayShownCallback>,
    on_outcome: Option<OverlayOutcomeCallback>,
) -> Result<OverlayOutcome, UiError> {
    let outcome_sink = Arc::new(Mutex::new(None));
    let overlay_title = ui_text(request.locale).overlay_window_title.to_owned();
    let Ok(mut guard) = OVERLAY_LAUNCH.lock() else {
        return Err(UiError::WindowInit(
            "overlay launch state lock poisoned".to_owned(),
        ));
    };
    *guard = Some(OverlayLaunch {
        request,
        outcome_sink: Arc::clone(&outcome_sink),
        on_shown,
        on_outcome,
    });
    drop(guard);

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let main_window = overlay_main_window_options();
        let initial_size = main_window.default_size.map_or((1920, 1080), |sz| {
            (
                (sz.width.0.round() as i32).max(1),
                (sz.height.0.round() as i32).max(1),
            )
        });
        let root = platform::Root::of::<BreakOverlayRoot>()
            .appearance(Appearance::Dark)
            .main_window(main_window);
        let _ = platform::run_app(
            platform::AppSource::Rust(root),
            platform::AppOptions {
                title: overlay_title,
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                app_id: "io.github.tunglt1810.safeyoureyes.overlay".to_owned(),
                size: initial_size,
                appearance: Appearance::Dark,
                #[cfg(target_os = "macos")]
                activation_policy: platform::ActivationPolicy::Accessory,
                #[cfg(target_os = "macos")]
                keep_size: true,
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
            locale: sye_core::Locale::En,
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
    fn remaining_ratios_drain_from_full_to_empty() {
        let mut req = sample_request();
        req.duration_secs = 10;
        let mut vm = OverlayViewModel::new(req);

        assert_eq!(vm.remaining_ratio(), 1.0);
        assert_eq!(vm.remaining_ratio_discrete(), 1.0);

        let _ = vm.tick(Duration::from_millis(500));
        assert!((vm.remaining_ratio() - 0.95).abs() < 1e-4);
        assert_eq!(vm.remaining_ratio_discrete(), 1.0);

        let _ = vm.tick(Duration::from_millis(2000));
        assert!((vm.remaining_ratio() - 0.75).abs() < 1e-4);
        assert!((vm.remaining_ratio_discrete() - 0.8).abs() < 1e-4);

        let _ = vm.tick(Duration::from_millis(7500));
        assert_eq!(vm.remaining_ratio(), 0.0);
        assert_eq!(vm.remaining_ratio_discrete(), 0.0);

        let mut zero_req = sample_request();
        zero_req.duration_secs = 0;
        let zero_vm = OverlayViewModel::new(zero_req);
        assert_eq!(zero_vm.remaining_ratio(), 0.0);
        assert_eq!(zero_vm.remaining_ratio_discrete(), 0.0);
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

        static OUTCOMES: Mutex<Vec<OverlayOutcome>> = Mutex::new(Vec::new());

        #[component]
        fn TestOverlay() -> impl IntoElement {
            let mut req = sample_request();
            req.shortcut_disable_secs = 0;
            req.fade_in_ms = 0;
            let sink = Arc::new(Mutex::new(None));
            let on_outcome: OverlayOutcomeCallback = Arc::new(|outcome| {
                OUTCOMES
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(outcome);
            });
            build_overlay_view(req, sink, false, None, Some(on_outcome))
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
        assert_eq!(
            *OUTCOMES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![OverlayOutcome::Skipped]
        );
        Ok(())
    }

    #[test]
    fn overlay_window_options_configure_borderless_above_dock_all_workspaces() {
        let opts = overlay_main_window_options();
        assert_eq!(opts.attributes.frame, Some(WindowFrameStyle::None));
        assert_eq!(opts.attributes.level, Some(WindowLevel::AboveDock));
        assert_eq!(opts.attributes.workspaces, Some(Workspaces::All));
        assert_eq!(opts.attributes.resizable, Some(false));
        assert_eq!(opts.attributes.movable, Some(false));
        assert_eq!(opts.attributes.minimizable, Some(false));
        assert_eq!(opts.attributes.maximizable, Some(false));
        assert_eq!(opts.attributes.closable, Some(false));
        assert_eq!(opts.attributes.focusable, Some(true));
        assert_eq!(opts.attributes.activate, Some(true));
        assert_eq!(opts.attributes.close_on_escape, Some(false));
        assert_eq!(opts.attributes.close_on_blur, Some(false));
        assert_eq!(opts.attributes.close_on_press, Some(false));
        assert_eq!(opts.attributes.taskbar, Some(cfg!(target_os = "windows")));
        assert_eq!(opts.attributes.shadow, Some(false));
        assert_eq!(opts.attributes.transparent, Some(true));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(
            opts.default_mode,
            Some(nsh_core::desktop::WindowMode::FullScreen)
        );
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        assert_eq!(opts.default_mode, None);

        let fallback = overlay_main_window_options_for_primary(None);
        #[cfg(not(target_os = "windows"))]
        assert_eq!(fallback.default_size, Some(size(px(1920.0), px(1080.0))));
        #[cfg(target_os = "windows")]
        assert_eq!(fallback.default_size, None);
        assert_eq!(fallback.default_position, None);

        let custom = overlay_main_window_options_for_primary(Some(crate::MonitorRect {
            x: 1920.0,
            y: -180.0,
            width: 2560.0,
            height: 1440.0,
            is_primary: true,
        }));
        assert_eq!(custom.default_size, Some(size(px(2560.0), px(1440.0))));
        assert_eq!(
            custom.default_position,
            Some(WindowPosition::At(point(px(1920.0), px(-180.0))))
        );

        let sec = secondary_window_attributes();
        assert_eq!(sec.frame, Some(WindowFrameStyle::None));
        assert_eq!(sec.level, Some(WindowLevel::AboveDock));
        assert_eq!(sec.workspaces, Some(Workspaces::All));
        assert_eq!(sec.closable, Some(false));
        assert_eq!(sec.close_on_escape, Some(false));
        assert_eq!(sec.close_on_blur, Some(false));
        assert_eq!(sec.close_on_press, Some(false));
        assert_eq!(sec.taskbar, Some(false));
        assert_eq!(sec.shadow, Some(false));
        assert_eq!(sec.activate, Some(false));
        assert_eq!(sec.focusable, Some(false));
        assert_eq!(sec.transparent, Some(true));
    }

    static OVERLAY_ROOT_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn nsh_headless_overlay_root_opens_visible_primary_window()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;

        let _serial = OVERLAY_ROOT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut req = sample_request();
        req.shortcut_disable_secs = 0;
        req.fade_in_ms = 0;
        let outcome_sink = Arc::new(Mutex::new(None));
        let recorded: Arc<Mutex<Vec<OverlayOutcome>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded_cb = Arc::clone(&recorded);
        let on_outcome: OverlayOutcomeCallback = Arc::new(move |outcome| {
            if let Ok(mut guard) = recorded_cb.lock() {
                guard.push(outcome);
            }
        });

        {
            let mut guard = OVERLAY_LAUNCH.lock().map_err(|e| e.to_string())?;
            *guard = Some(OverlayLaunch {
                request: req,
                outcome_sink: Arc::clone(&outcome_sink),
                on_shown: None,
                on_outcome: Some(on_outcome),
            });
        }

        let mut app = Headless::start(EngineConfig {
            environment: nsh_core::platform::Environment {
                os: nsh_core::platform::Os::MacOs,
                ..nsh_core::platform::Environment::default()
            },
            ..EngineConfig::new(Root::of::<BreakOverlayRoot>().appearance(Appearance::Dark))
        })
        .map_err(|e| e.to_string())?;

        app.wait_for("SHORT BREAK")?;
        let ids = app.window_ids()?;
        assert!(
            ids.contains(&"overlay-primary".to_owned()),
            "BreakOverlayRoot must open an explicit primary desktop window, got {ids:?}"
        );
        let state = app
            .window_state("overlay-primary")?
            .ok_or("expected overlay-primary window state")?;
        assert!(state.visible, "Primary overlay window must be visible");

        app.press("Skip (Esc)")?;
        app.settle();
        assert_eq!(
            *recorded.lock().map_err(|e| e.to_string())?,
            vec![OverlayOutcome::Skipped]
        );
        assert!(
            app.quit(),
            "Skipping break must quit the overlay subprocess"
        );
        Ok(())
    }

    #[test]
    fn nsh_engine_overlay_root_opens_window_for_every_display()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::mpsc;

        use nsh_core::desktop::{DesktopCommand, Display, DisplayId, WindowRequest};
        use nsh_core::{ApproxTextEngine, Command, MountBatch, MountItem};
        use nsh_engine::{Engine, EngineConfig, MountSink, Root};

        let _serial = OVERLAY_ROOT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        struct ChannelSink(mpsc::Sender<MountBatch>);
        impl MountSink for ChannelSink {
            fn submit(&self, batch: MountBatch) {
                let _ = self.0.send(batch);
            }
        }

        let primary_bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0)));
        let secondary_bounds =
            Bounds::new(point(px(1920.0), px(0.0)), size(px(2560.0), px(1440.0)));
        let displays = vec![
            Display::new(DisplayId(1), primary_bounds, primary_bounds, true),
            Display::new(DisplayId(2), secondary_bounds, secondary_bounds, false),
        ];

        {
            let mut guard = OVERLAY_LAUNCH.lock().map_err(|e| e.to_string())?;
            *guard = Some(OverlayLaunch {
                request: sample_request(),
                outcome_sink: Arc::new(Mutex::new(None)),
                on_shown: None,
                on_outcome: None,
            });
        }

        let (tx, rx) = mpsc::channel();
        let engine = Engine::start_with(
            EngineConfig {
                environment: nsh_core::platform::Environment {
                    os: nsh_core::platform::Os::MacOs,
                    ..nsh_core::platform::Environment::default()
                },
                ..EngineConfig::new(Root::of::<BreakOverlayRoot>().appearance(Appearance::Dark))
            },
            || ApproxTextEngine,
            ChannelSink(tx),
            move |control| {
                control.set_displays(displays);
            },
        )?;

        let mut opened: Vec<(String, Option<DisplayId>, Option<Size<Points>>)> = Vec::new();
        let mut primary_focused = false;
        let mut primary_node = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline && (opened.len() < 2 || !primary_focused) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let Ok(batch) = rx.recv_timeout(remaining.min(Duration::from_millis(100))) else {
                continue;
            };
            for item in batch.items {
                if let MountItem::Command {
                    id,
                    command: Command::Desktop(cmd),
                } = item
                {
                    match *cmd {
                        DesktopCommand::OpenWindow(open) => {
                            if open.options.id == "overlay-primary" {
                                primary_node = Some(id);
                            }
                            opened.push((
                                open.options.id,
                                open.options.display,
                                open.options.default_size,
                            ));
                        }
                        DesktopCommand::Request(WindowRequest::Focus)
                            if Some(id) == primary_node =>
                        {
                            primary_focused = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        drop(engine);

        assert!(
            opened.contains(&(
                "overlay-primary".to_owned(),
                Some(DisplayId(1)),
                Some(primary_bounds.size),
            )),
            "Expected primary display overlay window, got {opened:?}"
        );
        assert!(
            opened.contains(&(
                "overlay-secondary-2".to_owned(),
                Some(DisplayId(2)),
                Some(secondary_bounds.size),
            )),
            "Expected secondary display overlay window, got {opened:?}"
        );
        assert!(
            primary_focused,
            "Primary overlay window must request focus on launch"
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_overlay_renders_vietnamese_locale_and_default_exercise()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        #[component]
        fn ViOverlayHost() -> impl IntoElement {
            let mut req = sample_request();
            req.locale = sye_core::Locale::Vi;
            req.shortcut_disable_secs = 0;
            req.status_note = Some("Đã bỏ qua hoặc hoãn 1/2 lần nghỉ liên tiếp".to_owned());
            build_overlay_view(req, Arc::new(Mutex::new(None)), false, None, None)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(1024.0), Points(640.0)),
            ..EngineConfig::new(Root::of::<ViOverlayHost>())
        })
        .map_err(|e| e.to_string())?;

        app.wait_for("NGHỈ NGẮN")?;
        app.wait_for("Nhìn tập trung vào một điểm ở xa")?;
        app.wait_for("Đã bỏ qua hoặc hoãn 1/2 lần nghỉ liên tiếp")?;
        app.wait_for("Hoãn (Space)")?;
        app.wait_for("Bỏ qua (Esc)")?;
        Ok(())
    }
}
