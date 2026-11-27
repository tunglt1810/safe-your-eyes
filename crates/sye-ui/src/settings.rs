use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use nsh_core::desktop::{
    MainWindowOptions, QuitMode, TitleBarOptions, TitleBarStyle, WindowKind, WindowPosition,
};
use nsh_core::element::desktop::{current_window, quit, set_quit_mode, title_bar, window_state};
use nsh_core::prelude::*;
use sye_core::config::{BreakSpec, BreakType, Config, ThemeMode};
use sye_core::scheduler::{SchedulerState, StatusSnapshot};
use sye_core::{Locale, UpdateInfo, UpdateStrategy, localize_exercise_name};

use crate::UiError;
#[cfg(not(feature = "msstore"))]
use crate::i18n::{UpdateStatusMessage, format_homebrew_upgrade_hint};
use crate::i18n::{
    format_config_error, format_exercise_index_oob, format_kpi_cadence, format_kpi_max_skips,
    format_support_link_caption, format_version_summary, format_waiting_a11y,
    format_window_index_oob, format_window_pattern_empty, ui_text,
};
use crate::overlay::format_mm_ss;
#[cfg(not(feature = "msstore"))]
use crate::theme::DOWNLOAD_PAGE_URL;
use crate::theme::{
    SUPPORT_URL, appearance_for_theme_mode, colors, motion, next_theme_mode, radii,
    theme_mode_for_appearance,
};
use crate::ui::{
    CountdownPillState, CountdownTone, KpiTone, app_icon, countdown_status_pill, cozy_input,
    cozy_input_with_disabled, cozy_switch, cozy_switch_with_disabled, destructive_button,
    language_toggle_button, page_heading, pastel_kpi_card, primary_button_with_disabled,
    secondary_button, section_divider, section_header, sidebar_nav_indicator, sidebar_nav_item,
    stagger_item, status_banner, support_link, support_qr_card, theme_toggle_button,
};
#[cfg(not(feature = "msstore"))]
use crate::ui::{external_link_button, version_update_badge};

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Timing,
    Policies,
    Exercises,
    About,
}

impl SettingsTab {
    const ALL: [Self; 4] = [Self::Timing, Self::Policies, Self::Exercises, Self::About];

    const fn label(self, locale: Locale) -> &'static str {
        let copy = ui_text(locale);
        match self {
            Self::Timing => copy.tab_timing,
            Self::Policies => copy.tab_policies,
            Self::Exercises => copy.tab_exercises,
            Self::About => copy.tab_about,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOutcome {
    Saved,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsViewModel {
    pub config_path: PathBuf,
    pub config: Config,
    pub system_locale: Locale,
    pub active_tab: SettingsTab,
    pub load_warning: Option<String>,
    pub validation_error: Option<String>,
    pub outcome: Option<SettingsOutcome>,
    pub draft_short_exercise: String,
    pub draft_long_exercise: String,
    pub draft_skip_window: String,
    pub draft_take_window: String,
    system_autostart: Option<bool>,
}

impl SettingsViewModel {
    #[must_use]
    pub fn load(config_path: PathBuf) -> Self {
        Self::load_with_locale(config_path, Locale::En)
    }

    #[must_use]
    pub fn load_with_locale(config_path: PathBuf, system_locale: Locale) -> Self {
        let (mut config, load_err) = Config::load_or_default(&config_path);
        if config.strict_break {
            config.limit_consecutive_skips.enabled = false;
        }
        let locale = config.language.resolve(system_locale);
        Self {
            load_warning: load_err.map(|err| format_config_error(&err, locale)),
            config_path,
            config,
            system_locale,
            active_tab: SettingsTab::Timing,
            validation_error: None,
            outcome: None,
            draft_short_exercise: String::new(),
            draft_long_exercise: String::new(),
            draft_skip_window: String::new(),
            draft_take_window: String::new(),
            system_autostart: None,
        }
    }

    #[must_use]
    pub const fn active_locale(&self) -> Locale {
        self.config.language.resolve(self.system_locale)
    }

    pub fn add_exercise(&mut self, kind: BreakType, name: &str) -> Result<(), String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            let msg = ui_text(self.active_locale()).err_exercise_empty;
            return self.fail_validation(msg);
        }
        self.breaks_mut(kind).push(BreakSpec::new(trimmed));
        self.validation_error = None;
        Ok(())
    }

    pub fn remove_exercise(&mut self, kind: BreakType, index: usize) -> Result<(), String> {
        let total = self.config.short_breaks.len() + self.config.long_breaks.len();
        if total <= 1 {
            let msg = ui_text(self.active_locale()).err_exercise_last_remaining;
            return self.fail_validation(msg);
        }
        let locale = self.active_locale();
        let list = self.breaks_mut(kind);
        if index >= list.len() {
            return self.fail_validation(format_exercise_index_oob(index, locale));
        }
        list.remove(index);
        self.validation_error = None;
        Ok(())
    }

    pub fn set_exercise_interval_override(
        &mut self,
        kind: BreakType,
        index: usize,
        interval_secs: Option<u64>,
    ) -> Result<(), String> {
        self.exercise_mut(kind, index)?.interval_secs = interval_secs;
        self.validation_error = None;
        Ok(())
    }

    pub fn set_exercise_duration_override(
        &mut self,
        kind: BreakType,
        index: usize,
        duration_secs: Option<u64>,
    ) -> Result<(), String> {
        self.exercise_mut(kind, index)?.duration_secs = duration_secs;
        self.validation_error = None;
        Ok(())
    }

    pub fn add_skip_window(&mut self, pattern: &str) -> Result<(), String> {
        self.add_window_pattern(WindowList::Skip, pattern)
    }

    pub fn remove_skip_window(&mut self, index: usize) -> Result<(), String> {
        self.remove_window_pattern(WindowList::Skip, index)
    }

    pub fn add_take_window(&mut self, pattern: &str) -> Result<(), String> {
        self.add_window_pattern(WindowList::Take, pattern)
    }

    pub fn remove_take_window(&mut self, index: usize) -> Result<(), String> {
        self.remove_window_pattern(WindowList::Take, index)
    }

    pub const fn set_strict_break(&mut self, enabled: bool) {
        self.config.strict_break = enabled;
        if enabled {
            self.config.limit_consecutive_skips.enabled = false;
        }
    }

    pub const fn set_limit_consecutive_skips(&mut self, enabled: bool) {
        if !self.config.strict_break {
            self.config.limit_consecutive_skips.enabled = enabled;
        }
    }

    pub fn persist_theme(&self) -> Result<(), String> {
        let (mut on_disk, load_err) = Config::load_or_default(&self.config_path);
        if load_err.is_some() {
            return Ok(());
        }
        if on_disk.theme == self.config.theme {
            return Ok(());
        }
        // Keep `autostart` aligned with the OS login-item state seen by the daemon so a
        // theme-only close neither clobbers OS autostart nor triggers a non-theme reload.
        if let Some(actual) = self.system_autostart {
            on_disk.autostart = actual;
        }
        on_disk.theme = self.config.theme;
        on_disk
            .save_to_path(&self.config_path)
            .map_err(|err| err.to_string())
    }

    pub fn persist_language(&self) -> Result<(), String> {
        let (mut on_disk, load_err) = Config::load_or_default(&self.config_path);
        if load_err.is_some() {
            return Ok(());
        }
        if on_disk.language == self.config.language {
            return Ok(());
        }
        if let Some(actual) = self.system_autostart {
            on_disk.autostart = actual;
        }
        on_disk.language = self.config.language;
        on_disk
            .save_to_path(&self.config_path)
            .map_err(|err| err.to_string())
    }

    pub fn validate(&mut self) -> Result<(), String> {
        if let Err(err) = self.config.validate() {
            let msg = format_config_error(&err, self.active_locale());
            return self.fail_validation(msg);
        }
        if self.config.short_breaks.is_empty() && self.config.long_breaks.is_empty() {
            let msg = ui_text(self.active_locale()).err_exercise_none_configured;
            return self.fail_validation(msg);
        }
        self.validation_error = None;
        Ok(())
    }

    pub fn reset_to_defaults(&mut self) {
        self.config = Config::reset_to_default();
        self.validation_error = None;
        self.load_warning = None;
        self.draft_short_exercise.clear();
        self.draft_long_exercise.clear();
        self.draft_skip_window.clear();
        self.draft_take_window.clear();
    }

    #[must_use]
    pub fn is_modified_from_defaults(&self) -> bool {
        self.config != Config::reset_to_default()
    }

    pub fn autosave(&mut self) -> Result<(), String> {
        self.save()?;
        self.load_warning = None;
        Ok(())
    }

    pub fn save(&mut self) -> Result<SettingsOutcome, String> {
        if self.config.strict_break {
            self.config.limit_consecutive_skips.enabled = false;
        }
        self.validate()?;
        if let Err(err) = self.config.save_to_path(&self.config_path) {
            let msg = format_config_error(&err, self.active_locale());
            return self.fail_validation(msg);
        }
        self.system_autostart = Some(self.config.autostart);
        self.validation_error = None;
        self.outcome = Some(SettingsOutcome::Saved);
        Ok(SettingsOutcome::Saved)
    }

    pub fn cancel(&mut self) -> SettingsOutcome {
        self.outcome = Some(SettingsOutcome::Cancelled);
        SettingsOutcome::Cancelled
    }

    #[must_use]
    pub const fn should_close(&self) -> bool {
        self.outcome.is_some()
    }

    #[must_use]
    pub const fn resolved_outcome(&self) -> SettingsOutcome {
        match self.outcome {
            Some(outcome) => outcome,
            None => SettingsOutcome::Cancelled,
        }
    }

    fn fail_validation<T>(&mut self, msg: impl Into<String>) -> Result<T, String> {
        let msg = msg.into();
        self.validation_error = Some(msg.clone());
        Err(msg)
    }

    fn breaks_mut(&mut self, kind: BreakType) -> &mut Vec<BreakSpec> {
        match kind {
            BreakType::Short => &mut self.config.short_breaks,
            BreakType::Long => &mut self.config.long_breaks,
        }
    }

    fn exercise_mut(&mut self, kind: BreakType, index: usize) -> Result<&mut BreakSpec, String> {
        let locale = self.active_locale();
        if index >= self.breaks_mut(kind).len() {
            return self.fail_validation(format_exercise_index_oob(index, locale));
        }
        Ok(&mut self.breaks_mut(kind)[index])
    }

    fn add_window_pattern(&mut self, list: WindowList, pattern: &str) -> Result<(), String> {
        let trimmed = pattern.trim();
        let locale = self.active_locale();
        if trimmed.is_empty() {
            let label = list.label(locale);
            return self.fail_validation(format_window_pattern_empty(label, locale));
        }
        self.window_list_mut(list).push(trimmed.to_owned());
        self.validation_error = None;
        Ok(())
    }

    fn remove_window_pattern(&mut self, list: WindowList, index: usize) -> Result<(), String> {
        let locale = self.active_locale();
        let entries = self.window_list_mut(list);
        if index >= entries.len() {
            let label = list.label(locale);
            return self.fail_validation(format_window_index_oob(label, index, locale));
        }
        entries.remove(index);
        self.validation_error = None;
        Ok(())
    }

    fn window_list_mut(&mut self, list: WindowList) -> &mut Vec<String> {
        match list {
            WindowList::Skip => &mut self.config.do_not_disturb.skip_break_windows,
            WindowList::Take => &mut self.config.do_not_disturb.take_break_windows,
        }
    }

    fn draft_exercise(&self, kind: BreakType) -> &str {
        match kind {
            BreakType::Short => &self.draft_short_exercise,
            BreakType::Long => &self.draft_long_exercise,
        }
    }

    fn draft_exercise_mut(&mut self, kind: BreakType) -> &mut String {
        match kind {
            BreakType::Short => &mut self.draft_short_exercise,
            BreakType::Long => &mut self.draft_long_exercise,
        }
    }
}

#[derive(Clone, Copy)]
enum WindowList {
    Skip,
    Take,
}

impl WindowList {
    const fn label(self, locale: Locale) -> &'static str {
        let copy = ui_text(locale);
        match self {
            Self::Skip => copy.window_list_skip,
            Self::Take => copy.window_list_take,
        }
    }
}

pub type AutostartChangeCallback = Box<dyn Fn(bool) + Send + Sync>;
pub type ThemeChangeCallback = Arc<dyn Fn(ThemeMode) + Send + Sync>;
pub type SnapshotCaptureCallback = Arc<dyn Fn() -> bool + Send + Sync>;
pub type SnapshotRevealCallback = Arc<dyn Fn([f32; 2], u64) + Send + Sync>;
pub type SnapshotCancelCallback = Arc<dyn Fn() + Send + Sync>;
pub type SystemPrefersDarkCallback = Arc<dyn Fn() -> Option<bool> + Send + Sync>;
pub type WindowReadyCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Default)]
pub struct ThemeTransitionHooks {
    pub on_theme_change: Option<ThemeChangeCallback>,
    pub capture_snapshot: Option<SnapshotCaptureCallback>,
    pub reveal_snapshot: Option<SnapshotRevealCallback>,
    pub cancel_snapshot: Option<SnapshotCancelCallback>,
    pub system_prefers_dark: Option<SystemPrefersDarkCallback>,
}

const fn may_change_resolved_dark(
    current_dark: bool,
    next_mode: ThemeMode,
    system_dark: Option<bool>,
) -> bool {
    match next_mode {
        ThemeMode::Light => current_dark,
        ThemeMode::Dark => !current_dark,
        ThemeMode::System => match system_dark {
            Some(sys) => sys != current_dark,
            None => true,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyUpdateOutcome {
    RestartingDaemon,
    UpdatedStandalone,
}

pub type CheckUpdatesCallback = Arc<dyn Fn() -> Result<Option<UpdateInfo>, String> + Send + Sync>;
pub type ApplyUpdateCallback =
    Arc<dyn Fn(&str) -> Result<ApplyUpdateOutcome, String> + Send + Sync>;
pub type OpenUrlCallback = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;
pub type StatusPollCallback = Arc<dyn Fn() -> Option<StatusSnapshot> + Send + Sync>;

const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(500);
const STATUS_MISSES_BEFORE_HIDE: u32 = 2;
const NATIVE_REVEAL_FALLBACK_CANCEL_MS: u64 = 120;

#[derive(Clone, Default)]
pub struct SettingsUpdateConfig {
    pub initial_update: Option<UpdateInfo>,
    pub strategy: Option<UpdateStrategy>,
    pub on_check_now: Option<CheckUpdatesCallback>,
    pub on_apply_update: Option<ApplyUpdateCallback>,
    pub on_open_download: Option<OpenUrlCallback>,
}

#[derive(Clone, Default)]
pub struct SettingsStatusConfig {
    pub initial_status: Option<StatusSnapshot>,
    pub on_poll_status: Option<StatusPollCallback>,
}

fn format_countdown_pill(snapshot: &StatusSnapshot, locale: Locale) -> Option<CountdownPillState> {
    let copy = ui_text(locale);
    match snapshot.state {
        SchedulerState::Quit => None,
        _ if !snapshot.enabled => Some(CountdownPillState {
            tone: CountdownTone::Disabled,
            label: copy.countdown_disabled.to_owned(),
            time: None,
            accessible_label: copy.countdown_disabled_a11y.to_owned(),
        }),
        SchedulerState::Stopped => Some(CountdownPillState {
            tone: CountdownTone::Disabled,
            label: copy.countdown_disabled.to_owned(),
            time: None,
            accessible_label: copy.countdown_disabled_a11y.to_owned(),
        }),
        SchedulerState::Waiting => {
            let kind = match snapshot.next_break_type {
                Some(BreakType::Short) => copy.countdown_short,
                Some(BreakType::Long) => copy.countdown_long,
                None => copy.countdown_next,
            };
            Some(CountdownPillState {
                tone: CountdownTone::Active,
                label: kind.to_owned(),
                time: Some(format_mm_ss(snapshot.seconds_until_next_break)),
                accessible_label: format_waiting_a11y(
                    snapshot.next_break_type,
                    snapshot.seconds_until_next_break,
                    locale,
                ),
            })
        }
        SchedulerState::PreBreak => Some(CountdownPillState {
            tone: CountdownTone::Warning,
            label: copy.countdown_starting.to_owned(),
            time: Some(format_mm_ss(snapshot.seconds_until_next_break)),
            accessible_label: copy.countdown_starting_a11y.to_owned(),
        }),
        SchedulerState::Break => Some(CountdownPillState {
            tone: CountdownTone::Break,
            label: copy.countdown_break_active.to_owned(),
            time: None,
            accessible_label: copy.countdown_break_active_a11y.to_owned(),
        }),
        SchedulerState::Resting => Some(CountdownPillState {
            tone: CountdownTone::Paused,
            label: copy.countdown_paused.to_owned(),
            time: None,
            accessible_label: copy.countdown_paused_a11y.to_owned(),
        }),
        SchedulerState::Start => Some(CountdownPillState {
            tone: CountdownTone::Disabled,
            label: copy.countdown_initializing.to_owned(),
            time: None,
            accessible_label: copy.countdown_initializing_a11y.to_owned(),
        }),
    }
}

type AutostartHook = Arc<dyn Fn(bool) + Send + Sync>;

struct SettingsLaunch {
    vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    on_autostart_change: Option<AutostartHook>,
    theme_hooks: ThemeTransitionHooks,
    update_config: SettingsUpdateConfig,
    status_config: SettingsStatusConfig,
    on_window_ready: Option<WindowReadyCallback>,
}

static SETTINGS_LAUNCH: Mutex<Option<SettingsLaunch>> = Mutex::new(None);

fn load_settings_vm(
    config_path: PathBuf,
    system_autostart: Option<bool>,
    system_locale: Locale,
) -> SettingsViewModel {
    let mut vm = SettingsViewModel::load_with_locale(config_path, system_locale);
    vm.system_autostart = system_autostart;
    if let Some(actual) = system_autostart {
        vm.config.autostart = actual;
    }
    vm
}

fn number_field_row(
    label: impl IntoReactive<SharedString>,
    unit: impl IntoReactive<SharedString>,
    read: impl Fn() -> u64 + 'static,
    write: impl Fn(u64) + 'static,
) -> impl IntoElement {
    number_field_row_with_disabled(label, unit, read, || false, write)
}

fn number_field_row_with_disabled(
    label: impl IntoReactive<SharedString>,
    unit: impl IntoReactive<SharedString>,
    read: impl Fn() -> u64 + 'static,
    disabled: impl Fn() -> bool + Copy + 'static,
    write: impl Fn(u64) + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .h(px(36.0))
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::TEXT_PRIMARY)
                .opacity(move || if disabled() { 0.45_f32 } else { 1.0_f32 }),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(view().w(px(96.0)).child(cozy_input_with_disabled(
                    "0",
                    move || read().to_string(),
                    disabled,
                    move |raw: &str| {
                        if let Ok(parsed) = raw.trim().parse::<u64>() {
                            write(parsed);
                        }
                    },
                )))
                .child(
                    view()
                        .w(px(44.0))
                        .items_end()
                        .opacity(move || if disabled() { 0.45_f32 } else { 1.0_f32 })
                        .child(
                            text(unit)
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors::TEXT_SECONDARY),
                        ),
                ),
        )
}

fn update_and_autosave(
    vm: Signal<SettingsViewModel>,
    outcome_sink: &Arc<Mutex<Option<SettingsOutcome>>>,
    mutate: impl FnOnce(&mut SettingsViewModel),
) {
    let _ = try_update_and_autosave(vm, outcome_sink, mutate);
}

fn try_update_and_autosave(
    vm: Signal<SettingsViewModel>,
    outcome_sink: &Arc<Mutex<Option<SettingsOutcome>>>,
    mutate: impl FnOnce(&mut SettingsViewModel),
) -> bool {
    let mut saved = false;
    vm.update(|m| {
        mutate(m);
        if m.autosave().is_ok() {
            saved = true;
        }
    });
    if saved {
        crate::store_outcome(outcome_sink, SettingsOutcome::Saved);
    }
    saved
}

fn timing_section(
    vm: Signal<SettingsViewModel>,
    active_locale: Memo<Locale>,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    on_autostart_change: Option<AutostartHook>,
    animate: bool,
) -> impl IntoElement {
    let s1 = Arc::clone(&outcome_sink);
    let s2 = Arc::clone(&outcome_sink);
    let s3 = Arc::clone(&outcome_sink);
    let s4 = Arc::clone(&outcome_sink);
    let s5 = Arc::clone(&outcome_sink);
    let s6 = Arc::clone(&outcome_sink);
    let s7 = Arc::clone(&outcome_sink);
    let s8 = Arc::clone(&outcome_sink);
    let s9 = Arc::clone(&outcome_sink);
    let s10 = Arc::clone(&outcome_sink);
    let s11 = outcome_sink;

    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    0,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).timing_schedule_title,
                        move || ui_text(active_locale.get()).timing_schedule_caption,
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).short_break_interval,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.short_break_interval_secs),
                        move |v| {
                            update_and_autosave(vm, &s1, |m| {
                                m.config.short_break_interval_secs = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    2,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).short_break_duration,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.short_break_duration_secs),
                        move |v| {
                            update_and_autosave(vm, &s2, |m| {
                                m.config.short_break_duration_secs = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    3,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).long_break_interval,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.long_break_interval_secs),
                        move |v| {
                            update_and_autosave(vm, &s3, |m| m.config.long_break_interval_secs = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    4,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).long_break_duration,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.long_break_duration_secs),
                        move |v| {
                            update_and_autosave(vm, &s4, |m| m.config.long_break_duration_secs = v);
                        },
                    ),
                )),
        )
        .child(stagger_item(5, animate, section_divider()))
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    6,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).timing_controls_title,
                        move || ui_text(active_locale.get()).timing_controls_caption,
                    ),
                ))
                .child(stagger_item(
                    7,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).pre_break_warning,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.pre_break_warning_secs),
                        move |v| {
                            update_and_autosave(vm, &s5, |m| m.config.pre_break_warning_secs = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    8,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).fade_in_duration,
                        move || ui_text(active_locale.get()).unit_ms,
                        move || vm.with(|m| m.config.fade_in_ms),
                        move |v| update_and_autosave(vm, &s6, |m| m.config.fade_in_ms = v),
                    ),
                ))
                .child(stagger_item(
                    9,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).shortcut_unlock_delay,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.shortcut_disable_secs),
                        move |v| {
                            update_and_autosave(vm, &s7, |m| m.config.shortcut_disable_secs = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    10,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).postpone_duration,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.postpone_duration_secs),
                        move |v| {
                            update_and_autosave(vm, &s8, |m| m.config.postpone_duration_secs = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    11,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).randomize_exercise_order,
                        move || vm.with(|m| m.config.random_order),
                        move |v| update_and_autosave(vm, &s9, |m| m.config.random_order = v),
                    ),
                ))
                .child(stagger_item(
                    12,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).persist_session,
                        move || vm.with(|m| m.config.persist_state),
                        move |v| update_and_autosave(vm, &s10, |m| m.config.persist_state = v),
                    ),
                ))
                .child(stagger_item(
                    13,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).launch_at_login,
                        move || vm.with(|m| m.config.autostart),
                        move |v| {
                            if try_update_and_autosave(vm, &s11, |m| m.config.autostart = v)
                                && let Some(cb) = &on_autostart_change
                            {
                                cb(v);
                            }
                        },
                    ),
                )),
        )
}

fn policies_section(
    vm: Signal<SettingsViewModel>,
    active_locale: Memo<Locale>,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    animate: bool,
) -> impl IntoElement {
    let s1 = Arc::clone(&outcome_sink);
    let s2 = Arc::clone(&outcome_sink);
    let s3 = Arc::clone(&outcome_sink);
    let s4 = Arc::clone(&outcome_sink);
    let s5 = Arc::clone(&outcome_sink);
    let s6 = Arc::clone(&outcome_sink);
    let s7 = Arc::clone(&outcome_sink);
    let s8 = Arc::clone(&outcome_sink);
    let s9 = Arc::clone(&outcome_sink);
    let s10 = Arc::clone(&outcome_sink);
    let s11 = Arc::clone(&outcome_sink);
    let s12 = outcome_sink;

    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    0,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).policies_enforcement_title,
                        move || ui_text(active_locale.get()).policies_enforcement_caption,
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).strict_break,
                        move || vm.with(|m| m.config.strict_break),
                        move |v| update_and_autosave(vm, &s1, |m| m.set_strict_break(v)),
                    ),
                ))
                .child(stagger_item(
                    2,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).allow_postpone,
                        move || vm.with(|m| m.config.allow_postpone),
                        move |v| update_and_autosave(vm, &s2, |m| m.config.allow_postpone = v),
                    ),
                ))
                .child(stagger_item(
                    3,
                    animate,
                    cozy_switch_with_disabled(
                        move || ui_text(active_locale.get()).limit_consecutive_skips,
                        move || {
                            vm.with(|m| {
                                !m.config.strict_break && m.config.limit_consecutive_skips.enabled
                            })
                        },
                        move || vm.with(|m| m.config.strict_break),
                        move |v| update_and_autosave(vm, &s3, |m| m.set_limit_consecutive_skips(v)),
                    ),
                ))
                .child(stagger_item(
                    4,
                    animate,
                    number_field_row_with_disabled(
                        move || ui_text(active_locale.get()).max_consecutive_skips,
                        move || ui_text(active_locale.get()).unit_skips,
                        move || u64::from(vm.with(|m| m.config.limit_consecutive_skips.max_skips)),
                        move || {
                            vm.with(|m| {
                                m.config.strict_break || !m.config.limit_consecutive_skips.enabled
                            })
                        },
                        move |v| {
                            update_and_autosave(vm, &s4, |m| {
                                m.config.limit_consecutive_skips.max_skips =
                                    u32::try_from(v).unwrap_or(u32::MAX);
                            });
                        },
                    ),
                )),
        )
        .child(stagger_item(5, animate, section_divider()))
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    6,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).policies_smart_dnd_title,
                        move || ui_text(active_locale.get()).policies_smart_dnd_caption,
                    ),
                ))
                .child(stagger_item(
                    7,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).smart_pause_on_idle,
                        move || vm.with(|m| m.config.smart_pause.enabled),
                        move |v| update_and_autosave(vm, &s5, |m| m.config.smart_pause.enabled = v),
                    ),
                ))
                .child(stagger_item(
                    8,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).idle_threshold,
                        move || ui_text(active_locale.get()).unit_secs,
                        move || vm.with(|m| m.config.smart_pause.idle_threshold_secs),
                        move |v| {
                            update_and_autosave(vm, &s6, |m| {
                                m.config.smart_pause.idle_threshold_secs = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    9,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).dnd_fullscreen,
                        move || vm.with(|m| m.config.do_not_disturb.enabled),
                        move |v| {
                            update_and_autosave(vm, &s7, |m| m.config.do_not_disturb.enabled = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    10,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).pause_while_gaming,
                        move || vm.with(|m| m.config.do_not_disturb.while_gaming),
                        move |v| {
                            update_and_autosave(vm, &s8, |m| {
                                m.config.do_not_disturb.while_gaming = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    11,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).pause_while_on_battery,
                        move || vm.with(|m| m.config.do_not_disturb.while_on_battery),
                        move |v| {
                            update_and_autosave(vm, &s9, |m| {
                                m.config.do_not_disturb.while_on_battery = v;
                            });
                        },
                    ),
                )),
        )
        .child(stagger_item(12, animate, section_divider()))
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    13,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).policies_chime_title,
                        move || ui_text(active_locale.get()).policies_chime_caption,
                    ),
                ))
                .child(stagger_item(
                    14,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).pre_break_chime,
                        move || vm.with(|m| m.config.audible_alert.pre_break),
                        move |v| {
                            update_and_autosave(vm, &s10, |m| m.config.audible_alert.pre_break = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    15,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).post_break_chime,
                        move || vm.with(|m| m.config.audible_alert.post_break),
                        move |v| {
                            update_and_autosave(vm, &s11, |m| {
                                m.config.audible_alert.post_break = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    16,
                    animate,
                    number_field_row(
                        move || ui_text(active_locale.get()).chime_volume,
                        move || ui_text(active_locale.get()).unit_volume_range,
                        move || u64::from(vm.with(|m| m.config.audible_alert.volume)),
                        move |v| {
                            update_and_autosave(vm, &s12, |m| {
                                m.config.audible_alert.volume =
                                    u8::try_from(v.min(100)).unwrap_or(100);
                            });
                        },
                    ),
                )),
        )
}

fn exercise_tier_section(
    vm: Signal<SettingsViewModel>,
    active_locale: Memo<Locale>,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    kind: BreakType,
    base_index: usize,
    animate: bool,
) -> impl IntoElement {
    let key_prefix = match kind {
        BreakType::Short => "short",
        BreakType::Long => "long",
    };
    let initial_remaining = Rc::new(Cell::new(if animate {
        vm.with_untracked(|m| match kind {
            BreakType::Short => m.config.short_breaks.len(),
            BreakType::Long => m.config.long_breaks.len(),
        })
    } else {
        0
    }));
    let items = computed(move || {
        vm.with(|m| {
            let list = match kind {
                BreakType::Short => &m.config.short_breaks,
                BreakType::Long => &m.config.long_breaks,
            };
            list.iter()
                .map(|spec| spec.name.clone())
                .collect::<Vec<_>>()
        })
    });
    let add_sink = Arc::clone(&outcome_sink);

    view()
        .gap_3()
        .child(stagger_item(
            base_index,
            animate,
            section_header(
                move || {
                    let copy = ui_text(active_locale.get());
                    match kind {
                        BreakType::Short => copy.short_exercises_title,
                        BreakType::Long => copy.long_exercises_title,
                    }
                },
                move || {
                    let copy = ui_text(active_locale.get());
                    match kind {
                        BreakType::Short => copy.short_exercises_caption,
                        BreakType::Long => copy.long_exercises_caption,
                    }
                },
            ),
        ))
        .child(stagger_item(
            base_index + 1,
            animate,
            view()
                .flex_row()
                .gap_2()
                .items_center()
                .child(view().flex_1().child(cozy_input(
                    move || {
                        let copy = ui_text(active_locale.get());
                        match kind {
                            BreakType::Short => copy.short_exercises_placeholder.to_owned(),
                            BreakType::Long => copy.long_exercises_placeholder.to_owned(),
                        }
                    },
                    move || vm.with(|m| m.draft_exercise(kind).to_owned()),
                    move |s: &str| vm.update(|m| *m.draft_exercise_mut(kind) = s.to_owned()),
                )))
                .child(secondary_button(
                    move || ui_text(active_locale.get()).add_button,
                    move || {
                        let mut saved = false;
                        vm.update(|m| {
                            let name = std::mem::take(m.draft_exercise_mut(kind));
                            if m.add_exercise(kind, &name).is_ok() {
                                if m.autosave().is_ok() {
                                    saved = true;
                                }
                            } else {
                                *m.draft_exercise_mut(kind) = name;
                            }
                        });
                        if saved {
                            crate::store_outcome(&add_sink, SettingsOutcome::Saved);
                        }
                    },
                )),
        ))
        .child(view().gap_2().child(each(
            items,
            move |name| format!("{key_prefix}-{name}"),
            move |item, row_idx| {
                let rem = initial_remaining.get();
                let row_animate = rem > 0;
                if row_animate {
                    initial_remaining.set(rem - 1);
                }
                let mount_idx = row_idx.get_untracked();
                let remove_sink = Arc::clone(&outcome_sink);
                stagger_item(
                    base_index + 2 + mount_idx,
                    row_animate,
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .rounded(radii::ELEMENT)
                        .bg(colors::BACKGROUND_MUTED)
                        .px_3()
                        .py_2()
                        .child(
                            text(move || {
                                let raw = item.get();
                                localize_exercise_name(&raw, active_locale.get()).to_owned()
                            })
                            .text_sm()
                            .text_color(colors::TEXT_PRIMARY),
                        )
                        .child(destructive_button(
                            move || ui_text(active_locale.get()).remove_button,
                            move || {
                                let idx = row_idx.get_untracked();
                                let mut saved = false;
                                vm.update(|m| {
                                    if m.remove_exercise(kind, idx).is_ok() && m.autosave().is_ok()
                                    {
                                        saved = true;
                                    }
                                });
                                if saved {
                                    crate::store_outcome(&remove_sink, SettingsOutcome::Saved);
                                }
                            },
                        )),
                )
            },
        )))
}

fn exercises_section(
    vm: Signal<SettingsViewModel>,
    active_locale: Memo<Locale>,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    animate: bool,
) -> impl IntoElement {
    let short_count = vm.with(|m| m.config.short_breaks.len());
    let divider_idx = 2 + short_count;
    view()
        .gap_5()
        .child(exercise_tier_section(
            vm,
            active_locale,
            Arc::clone(&outcome_sink),
            BreakType::Short,
            0,
            animate,
        ))
        .child(stagger_item(divider_idx, animate, section_divider()))
        .child(exercise_tier_section(
            vm,
            active_locale,
            outcome_sink,
            BreakType::Long,
            divider_idx + 1,
            animate,
        ))
}

#[cfg(not(feature = "msstore"))]
enum UpdateWorkerResult {
    Check(Result<Option<UpdateInfo>, String>),
    Apply(Result<ApplyUpdateOutcome, String>),
}

#[cfg(not(feature = "msstore"))]
#[derive(Clone, Copy)]
struct UpdateUiState {
    available: Signal<Option<UpdateInfo>>,
    busy: Signal<bool>,
    applying: Signal<bool>,
    status_msg: Signal<Option<UpdateStatusMessage>>,
    is_error: Signal<bool>,
    strategy: UpdateStrategy,
}

#[cfg(not(feature = "msstore"))]
fn poll_update_worker(
    rx: Rc<mpsc::Receiver<UpdateWorkerResult>>,
    state: UpdateUiState,
    quit_on_restart: bool,
) {
    match rx.try_recv() {
        Ok(UpdateWorkerResult::Check(Ok(Some(info)))) => {
            state.status_msg.set(Some(UpdateStatusMessage::Available(
                info.latest_version.clone(),
            )));
            state.is_error.set(false);
            state.available.set(Some(info));
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Check(Ok(None))) => {
            state.available.set(None);
            state.status_msg.set(Some(UpdateStatusMessage::UpToDate(
                env!("CARGO_PKG_VERSION").to_owned(),
            )));
            state.is_error.set(false);
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Check(Err(err))) => {
            state
                .status_msg
                .set(Some(UpdateStatusMessage::CheckFailed(err)));
            state.is_error.set(true);
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Apply(Ok(ApplyUpdateOutcome::RestartingDaemon))) => {
            state
                .status_msg
                .set(Some(UpdateStatusMessage::RestartingDaemon));
            state.is_error.set(false);
            state.available.set(None);
            state.busy.set(false);
            state.applying.set(false);
            if quit_on_restart {
                quit();
            }
        }
        Ok(UpdateWorkerResult::Apply(Ok(ApplyUpdateOutcome::UpdatedStandalone))) => {
            state
                .status_msg
                .set(Some(UpdateStatusMessage::UpdatedStandalone));
            state.is_error.set(false);
            state.available.set(None);
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Apply(Err(err))) => {
            state
                .status_msg
                .set(Some(UpdateStatusMessage::SelfUpdateFailed(err)));
            state.is_error.set(true);
            state.busy.set(false);
            state.applying.set(false);
        }
        Err(mpsc::TryRecvError::Empty) => {
            drop(set_timeout(Duration::from_millis(25), move || {
                poll_update_worker(rx, state, quit_on_restart);
            }));
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            state
                .status_msg
                .set(Some(UpdateStatusMessage::WorkerDisconnected));
            state.is_error.set(true);
            state.busy.set(false);
            state.applying.set(false);
        }
    }
}

#[cfg(not(feature = "msstore"))]
fn start_apply_update(
    update_ui: UpdateUiState,
    apply_cb: &Option<ApplyUpdateCallback>,
    quit_on_restart: bool,
) -> bool {
    if update_ui.busy.get_untracked() {
        return false;
    }
    let Some(info) = update_ui.available.get_untracked() else {
        return false;
    };
    let Some(cb) = apply_cb.clone() else {
        return false;
    };
    update_ui.busy.set(true);
    update_ui.applying.set(true);
    update_ui.is_error.set(false);
    update_ui
        .status_msg
        .set(Some(UpdateStatusMessage::Downloading(
            info.latest_version.clone(),
        )));
    let (tx, rx) = mpsc::channel();
    let version = info.latest_version;
    let _ = std::thread::Builder::new()
        .name("sye-settings-update".to_owned())
        .spawn(move || {
            let _ = tx.send(UpdateWorkerResult::Apply(cb(&version)));
        });
    let rx = Rc::new(rx);
    drop(set_timeout(Duration::from_millis(25), move || {
        poll_update_worker(rx, update_ui, quit_on_restart);
    }));
    true
}

#[cfg(not(feature = "msstore"))]
fn trigger_update_badge_action(
    vm: Signal<SettingsViewModel>,
    has_switched_tab: Signal<bool>,
    update_ui: UpdateUiState,
    apply_cb: &Option<ApplyUpdateCallback>,
    open_download_cb: &Option<OpenUrlCallback>,
    quit_on_restart: bool,
) {
    vm.update(|m| {
        if m.active_tab != SettingsTab::About {
            has_switched_tab.set(true);
            m.active_tab = SettingsTab::About;
        }
    });
    if update_ui.busy.get_untracked() {
        return;
    }
    let Some(info) = update_ui.available.get_untracked() else {
        return;
    };
    if matches!(update_ui.strategy, UpdateStrategy::SelfUpdate { .. })
        && start_apply_update(update_ui, apply_cb, quit_on_restart)
    {
        return;
    }
    if let Some(open_cb) = open_download_cb {
        match open_cb(&info.download_url) {
            Ok(()) => {
                let message = match update_ui.strategy {
                    UpdateStrategy::HomebrewCask => UpdateStatusMessage::OpenedDownloadPageHomebrew,
                    UpdateStrategy::SelfUpdate { .. } | UpdateStrategy::ManualDownload => {
                        UpdateStatusMessage::OpenedDownloadPage
                    }
                };
                update_ui.is_error.set(false);
                update_ui.status_msg.set(Some(message));
            }
            Err(err) => {
                update_ui.is_error.set(true);
                update_ui
                    .status_msg
                    .set(Some(UpdateStatusMessage::OpenDownloadFailed(err)));
            }
        }
    }
}

fn support_section(
    active_locale: Memo<Locale>,
    animate: bool,
    start_index: usize,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_5()
        .child(
            view()
                .flex_1()
                .gap_3()
                .child(stagger_item(
                    start_index,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).support_title,
                        move || ui_text(active_locale.get()).support_caption,
                    ),
                ))
                .child(stagger_item(
                    start_index + 1,
                    animate,
                    text(move || format_support_link_caption(SUPPORT_URL, active_locale.get()))
                        .text_xs()
                        .text_color(colors::TEXT_SECONDARY),
                ))
                .child(stagger_item(
                    start_index + 2,
                    animate,
                    view().flex_row().child(support_link(move || {
                        ui_text(active_locale.get()).support_button
                    })),
                )),
        )
        .child(stagger_item(
            start_index + 3,
            animate,
            support_qr_card(move || ui_text(active_locale.get()).support_qr_caption),
        ))
}

#[cfg(feature = "msstore")]
fn about_section(active_locale: Memo<Locale>, animate: bool) -> impl IntoElement {
    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    0,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).updates_title,
                        move || ui_text(active_locale.get()).updates_caption,
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    text(move || {
                        format_version_summary(env!("CARGO_PKG_VERSION"), None, active_locale.get())
                    })
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors::TEXT_PRIMARY),
                )),
        )
        .child(stagger_item(2, animate, section_divider()))
        .child(support_section(active_locale, animate, 3))
}

#[cfg(not(feature = "msstore"))]
#[expect(
    clippy::too_many_arguments,
    reason = "About section wires update callbacks, active locale, and animation state"
)]
fn about_section(
    vm: Signal<SettingsViewModel>,
    active_locale: Memo<Locale>,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    update_ui: UpdateUiState,
    on_check_now: Option<CheckUpdatesCallback>,
    on_apply_update: Option<ApplyUpdateCallback>,
    quit_on_restart: bool,
    animate: bool,
) -> impl IntoElement {
    let can_self_update = matches!(update_ui.strategy, UpdateStrategy::SelfUpdate { .. });
    let is_homebrew = matches!(update_ui.strategy, UpdateStrategy::HomebrewCask);

    let check_cb = on_check_now;
    let apply_cb = on_apply_update;

    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    0,
                    animate,
                    section_header(
                        move || ui_text(active_locale.get()).updates_title,
                        move || ui_text(active_locale.get()).updates_caption,
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    text(move || {
                        let latest = update_ui.available.get().map(|info| info.latest_version);
                        format_version_summary(
                            env!("CARGO_PKG_VERSION"),
                            latest.as_deref(),
                            active_locale.get(),
                        )
                    })
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors::TEXT_PRIMARY),
                ))
                .child(when(
                    move || update_ui.status_msg.get().is_some() && !update_ui.is_error.get(),
                    move || {
                        status_banner(false, move || {
                            update_ui
                                .status_msg
                                .get()
                                .map_or_else(String::new, |msg| msg.format(active_locale.get()))
                        })
                    },
                ))
                .child(when(
                    move || update_ui.status_msg.get().is_some() && update_ui.is_error.get(),
                    move || {
                        status_banner(true, move || {
                            update_ui
                                .status_msg
                                .get()
                                .map_or_else(String::new, |msg| msg.format(active_locale.get()))
                        })
                    },
                ))
                .child(when(
                    move || is_homebrew && update_ui.available.get().is_some(),
                    move || {
                        text(move || format_homebrew_upgrade_hint(active_locale.get()))
                            .text_xs()
                            .text_color(colors::TEXT_SECONDARY)
                    },
                ))
                .child(stagger_item(
                    2,
                    animate,
                    cozy_switch(
                        move || ui_text(active_locale.get()).check_updates_auto,
                        move || vm.with(|m| m.config.check_for_updates),
                        move |v| {
                            update_and_autosave(vm, &outcome_sink, |m| {
                                m.config.check_for_updates = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    3,
                    animate,
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(when(
                            move || can_self_update && update_ui.available.get().is_some(),
                            move || {
                                let apply = apply_cb.clone();
                                primary_button_with_disabled(
                                    move || {
                                        let copy = ui_text(active_locale.get());
                                        if update_ui.applying.get() {
                                            copy.updating_button
                                        } else {
                                            copy.update_and_restart
                                        }
                                    },
                                    || false,
                                    move || {
                                        let _ =
                                            start_apply_update(update_ui, &apply, quit_on_restart);
                                    },
                                )
                            },
                        ))
                        .child(when(
                            move || update_ui.available.get().is_some(),
                            move || {
                                external_link_button(
                                    move || {
                                        let copy = ui_text(active_locale.get());
                                        if can_self_update {
                                            copy.download_page
                                        } else {
                                            copy.download_update
                                        }
                                    },
                                    DOWNLOAD_PAGE_URL,
                                    !can_self_update,
                                )
                            },
                        ))
                        .child(secondary_button(
                            move || {
                                let copy = ui_text(active_locale.get());
                                if update_ui.busy.get() && !update_ui.applying.get() {
                                    copy.checking_button
                                } else {
                                    copy.check_for_updates
                                }
                            },
                            move || {
                                if update_ui.busy.get_untracked() {
                                    return;
                                }
                                let Some(cb) = check_cb.clone() else {
                                    return;
                                };
                                update_ui.busy.set(true);
                                update_ui.is_error.set(false);
                                update_ui
                                    .status_msg
                                    .set(Some(UpdateStatusMessage::Checking));
                                let (tx, rx) = mpsc::channel();
                                let _ = std::thread::Builder::new()
                                    .name("sye-settings-check-update".to_owned())
                                    .spawn(move || {
                                        let _ = tx.send(UpdateWorkerResult::Check(cb()));
                                    });
                                let rx = Rc::new(rx);
                                drop(set_timeout(Duration::from_millis(25), move || {
                                    poll_update_worker(rx, update_ui, quit_on_restart);
                                }));
                            },
                        )),
                )),
        )
        .child(stagger_item(4, animate, section_divider()))
        .child(support_section(active_locale, animate, 5))
}

fn settings_main_window_options() -> MainWindowOptions {
    let mut tb = TitleBarOptions::default();
    tb.style = TitleBarStyle::Hidden;
    tb.height = Some(px(36.0));
    #[cfg(target_os = "macos")]
    {
        tb.controls_offset = Some(point(px(20.0), px(18.0)));
    }
    let mut main_window = MainWindowOptions::default();
    main_window.attributes.title_bar = Some(tb);
    main_window
}

type PendingNativeReveal = Rc<RefCell<Option<(u64, [f32; 2], ThemeMode)>>>;
type QueuedTheme = Rc<Cell<(u64, Option<ThemeMode>)>>;

#[derive(Clone, Default)]
struct NativeRevealState {
    pending: PendingNativeReveal,
    generation: Rc<Cell<u64>>,
    in_flight_captures: Rc<Cell<u32>>,
}

// The capture round trip defers `apply_state`, so a press that lands before it ends must cycle
// from the queued mode instead of the stale `appearance`.
fn queue_theme(queued: &QueuedTheme, mode: ThemeMode) -> u64 {
    let seq = queued.get().0.wrapping_add(1);
    queued.set((seq, Some(mode)));
    seq
}

fn settle_theme(queued: &QueuedTheme, seq: u64) {
    if queued.get().0 == seq {
        queued.set((seq, None));
    }
}

fn dispatch_theme_transition(
    current_mode: ThemeMode,
    next_mode: ThemeMode,
    resolved_origin: [f32; 2],
    hooks: &ThemeTransitionHooks,
    reveal: &NativeRevealState,
    apply_state: impl FnOnce() + 'static,
) {
    let generation = reveal.generation.get().wrapping_add(1).max(1);
    reveal.generation.set(generation);

    let system_dark = hooks.system_prefers_dark.as_ref().and_then(|cb| cb());
    if current_mode != next_mode
        && !reduce_motion()
        && may_change_resolved_dark(dark(), next_mode, system_dark)
        && let (Some(capture_cb), Some(_reveal_cb)) = (
            hooks.capture_snapshot.clone(),
            hooks.reveal_snapshot.as_ref(),
        )
    {
        let reveal = reveal.clone();
        let sync_cb = hooks.on_theme_change.clone();
        let cancel_cb = hooks.cancel_snapshot.clone();
        spawn(async move {
            if reveal.generation.get() != generation {
                return;
            }
            reveal
                .in_flight_captures
                .set(reveal.in_flight_captures.get().saturating_add(1));
            let captured = run_on_main_thread(move || capture_cb())
                .await
                .unwrap_or(false);
            reveal
                .in_flight_captures
                .set(reveal.in_flight_captures.get().saturating_sub(1));
            if reveal.generation.get() != generation {
                return;
            }
            if captured {
                *reveal.pending.borrow_mut() = Some((generation, resolved_origin, next_mode));
                let pending_timeout = Rc::clone(&reveal.pending);
                let sync_timeout = sync_cb;
                let cancel_timeout = cancel_cb;
                drop(set_timeout(
                    Duration::from_millis(NATIVE_REVEAL_FALLBACK_CANCEL_MS),
                    move || {
                        let stale = {
                            let mut guard = pending_timeout.borrow_mut();
                            if guard.as_ref().is_some_and(|(id, _, _)| *id == generation) {
                                guard.take()
                            } else {
                                None
                            }
                        };
                        if let Some((_, _, mode)) = stale {
                            drop(run_on_main_thread(move || {
                                if let Some(cb) = sync_timeout {
                                    cb(mode);
                                }
                                if let Some(cb) = cancel_timeout {
                                    cb();
                                }
                            }));
                        }
                    },
                ));
                apply_state();
            } else {
                apply_state();
                if let Some(cb) = sync_cb {
                    drop(run_on_main_thread(move || cb(next_mode)));
                }
            }
        });
    } else {
        let had_pending = reveal.pending.borrow_mut().take().is_some();
        let had_in_flight = reveal.in_flight_captures.get() > 0;
        apply_state();
        let sync_cb = hooks.on_theme_change.clone();
        let cancel_cb = if had_pending || had_in_flight {
            hooks.cancel_snapshot.clone()
        } else {
            None
        };
        if sync_cb.is_some() || cancel_cb.is_some() {
            drop(run_on_main_thread(move || {
                if let Some(cb) = sync_cb {
                    cb(next_mode);
                }
                if let Some(cb) = cancel_cb {
                    cb();
                }
            }));
        }
    }
}

#[cfg(test)]
pub(crate) fn build_settings_view(
    initial_vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    quit_on_finish: bool,
) -> impl IntoElement {
    build_settings_view_with_hooks(
        initial_vm,
        outcome_sink,
        quit_on_finish,
        None,
        ThemeTransitionHooks::default(),
        SettingsUpdateConfig::default(),
        SettingsStatusConfig::default(),
    )
}

#[cfg(test)]
pub(crate) fn build_settings_view_with_hooks(
    initial_vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    quit_on_finish: bool,
    on_autostart_change: Option<AutostartHook>,
    theme_hooks: ThemeTransitionHooks,
    update_config: SettingsUpdateConfig,
    status_config: SettingsStatusConfig,
) -> impl IntoElement {
    build_settings_view_inner(
        initial_vm,
        outcome_sink,
        quit_on_finish,
        on_autostart_change,
        theme_hooks,
        update_config,
        status_config,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "Internal builder wiring all Settings view hooks and optional window locale signal"
)]
fn build_settings_view_inner(
    initial_vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    quit_on_finish: bool,
    on_autostart_change: Option<AutostartHook>,
    theme_hooks: ThemeTransitionHooks,
    update_config: SettingsUpdateConfig,
    status_config: SettingsStatusConfig,
    window_locale: Option<Signal<Locale>>,
) -> impl IntoElement {
    if quit_on_finish {
        let main_win = current_window();
        main_win.focus();
        let focus_win = main_win;
        drop(set_timeout(Duration::from_millis(60), move || {
            focus_win.focus();
        }));
    }

    let initial_appearance = appearance_for_theme_mode(initial_vm.config.theme);
    let vm = signal(initial_vm);
    let appearance = signal(initial_appearance);
    let has_switched_tab = signal(false);
    let measured_theme_btn_center = signal(None::<[f32; 2]>);
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let reveal_origin = signal([860.0_f32, 32.0_f32]);
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let reveal_epoch = signal(0_u32);
    let native_reveal = NativeRevealState::default();
    let queued_theme: QueuedTheme = Rc::new(Cell::new((0, None)));

    let active_locale = computed(move || vm.with(SettingsViewModel::active_locale));
    if let Some(win_locale) = window_locale {
        effect(move || {
            let next = active_locale.get();
            if win_locale.get_untracked() != next {
                win_locale.set(next);
            }
        });
    }

    let poll_immediately = status_config.initial_status.is_none();
    let live_status = signal(status_config.initial_status);
    if let Some(poll_status) = status_config.on_poll_status {
        let (tx, rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let _ = std::thread::Builder::new()
            .name("sye-settings-status".to_owned())
            .spawn(move || {
                if poll_immediately && tx.send(poll_status()).is_err() {
                    return;
                }
                let mut misses = 0_u32;
                while matches!(
                    stop_rx.recv_timeout(STATUS_POLL_INTERVAL),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let snapshot = poll_status();
                    // One slow IPC reply must not hide the pill for a poll cycle.
                    misses = if snapshot.is_some() { 0 } else { misses + 1 };
                    if snapshot.is_none() && misses < STATUS_MISSES_BEFORE_HIDE {
                        continue;
                    }
                    if tx.send(snapshot).is_err() {
                        break;
                    }
                }
            });
        let _status_timer = set_interval(Duration::from_millis(100), move || {
            let _keep_alive = &stop_tx;
            let mut latest = None;
            while let Ok(snap) = rx.try_recv() {
                latest = Some(snap);
            }
            if let Some(next) = latest
                && live_status.get_untracked() != next
            {
                live_status.set(next);
            }
        });
    }
    let countdown_pill = computed(move || {
        let locale = active_locale.get();
        live_status.with(|snap| snap.as_ref().and_then(|s| format_countdown_pill(s, locale)))
    });

    #[cfg(not(feature = "msstore"))]
    let update_ui = UpdateUiState {
        available: signal(update_config.initial_update),
        busy: signal(false),
        applying: signal(false),
        status_msg: signal(None),
        is_error: signal(false),
        strategy: update_config
            .strategy
            .unwrap_or(UpdateStrategy::ManualDownload),
    };
    #[cfg(not(feature = "msstore"))]
    let on_check_now = update_config.on_check_now;
    #[cfg(not(feature = "msstore"))]
    let on_apply_update = update_config.on_apply_update;
    #[cfg(not(feature = "msstore"))]
    let on_open_download = update_config.on_open_download;
    #[cfg(feature = "msstore")]
    drop(update_config);

    let active_tab = computed(move || vm.with(|m| m.active_tab));
    let warning_msg = computed(move || vm.with(|m| m.load_warning.clone()));
    let error_msg = computed(move || vm.with(|m| m.validation_error.clone()));
    let is_modified = computed(move || vm.with(SettingsViewModel::is_modified_from_defaults));

    let highlight_scale = animated_value(1.0);
    let form_scroll_ref = element_ref();
    let prev_tab = Cell::new(None::<SettingsTab>);

    effect(move || {
        let current = active_tab.get();
        let previous = prev_tab.replace(Some(current));
        if let Some(prev) = previous
            && prev != current
        {
            form_scroll_ref.scroll_to(point(px(0.0), px(0.0)), false);
            if !reduce_motion() {
                highlight_scale.set(0.0);
                highlight_scale.timing(1.0, motion::HIGHLIGHT_SWEEP_MS, motion::COZY_EASE);
            }
        }
    });

    let prev_dark = Cell::new(None::<bool>);
    let effect_reveal = native_reveal.clone();
    let effect_theme_hooks = theme_hooks.clone();
    effect(move || {
        let now_dark = dark();
        let previous = prev_dark.replace(Some(now_dark));
        if let Some(prev) = previous
            && prev != now_dark
        {
            if let Some((_, origin, mode)) = effect_reveal.pending.borrow_mut().take() {
                let sync_cb = effect_theme_hooks.on_theme_change.clone();
                let reveal_cb = effect_theme_hooks.reveal_snapshot.clone();
                drop(run_on_main_thread(move || {
                    if let Some(cb) = sync_cb {
                        cb(mode);
                    }
                    if let Some(cb) = reveal_cb {
                        cb(origin, motion::THEME_REVEAL_MS);
                    }
                }));
            }
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            if !reduce_motion() {
                reveal_epoch.update(|epoch| *epoch = epoch.wrapping_add(1));
            }
        }
    });

    let lang_sink = Arc::clone(&outcome_sink);
    let theme_sink = Arc::clone(&outcome_sink);
    let timing_sink = Arc::clone(&outcome_sink);
    let policies_sink = Arc::clone(&outcome_sink);
    let exercises_sink = Arc::clone(&outcome_sink);
    #[cfg(not(feature = "msstore"))]
    let about_sink = Arc::clone(&outcome_sink);
    let reset_sink = outcome_sink;

    let tab_index = move || match active_tab.get() {
        SettingsTab::Timing => 0,
        SettingsTab::Policies => 1,
        SettingsTab::Exercises => 2,
        SettingsTab::About => 3,
    };

    let mut sidebar_nav = view()
        .relative()
        .gap_1()
        .child(sidebar_nav_indicator(tab_index));
    for tab in SettingsTab::ALL {
        sidebar_nav = sidebar_nav.child(sidebar_nav_item(
            move || tab.label(active_locale.get()),
            move || active_tab.get() == tab,
            move || {
                vm.update(|m| {
                    if m.active_tab != tab {
                        has_switched_tab.set(true);
                        m.active_tab = tab;
                    }
                });
            },
        ));
    }

    #[cfg(not(feature = "msstore"))]
    let badge_apply_cb = on_apply_update.clone();
    #[cfg(not(feature = "msstore"))]
    let badge_open_cb = on_open_download;
    let sidebar_footer = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .py_1()
        .child(
            text(concat!("v", env!("CARGO_PKG_VERSION")))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::SIDEBAR_TEXT_MUTED),
        );
    #[cfg(not(feature = "msstore"))]
    let sidebar_footer = sidebar_footer.child(when(
        move || update_ui.available.get().is_some(),
        move || {
            let apply_cb = badge_apply_cb.clone();
            let open_cb = badge_open_cb.clone();
            version_update_badge(
                move || {
                    update_ui
                        .available
                        .get()
                        .map_or_else(String::new, |info| format!("v{}", info.latest_version))
                },
                move || {
                    trigger_update_badge_action(
                        vm,
                        has_switched_tab,
                        update_ui,
                        &apply_cb,
                        &open_cb,
                        quit_on_finish,
                    );
                },
            )
        },
    ));
    let sidebar = view()
        .w(px(210.0))
        .flex_shrink_0()
        .rounded(radii::SIDEBAR)
        .bg(colors::SIDEBAR_BG)
        .border_1()
        .border_color(colors::BORDER)
        .px_3()
        .pb_3()
        .pt(move || px(if title_bar().left.0 > 0.0 { 0.0 } else { 12.0 }))
        .justify_between()
        .child(
            view()
                .gap_2()
                .child(when(
                    move || title_bar().left.0 > 0.0,
                    || view().h(px(38.0)).window_drag(true),
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_2()
                        .py_2()
                        .window_drag(true)
                        .child(app_icon(36.0))
                        .child(
                            view()
                                .gap_1()
                                .child(
                                    text("Safe Your Eyes")
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(colors::SIDEBAR_TEXT),
                                )
                                .child(
                                    text(move || ui_text(active_locale.get()).sidebar_subtitle)
                                        .text_xs()
                                        .text_color(colors::SIDEBAR_TEXT_MUTED),
                                ),
                        ),
                )
                .child(sidebar_nav),
        )
        .child(sidebar_footer);

    let kpi_row = view()
        .flex_row()
        .gap_3()
        .child(pastel_kpi_card(
            KpiTone::Sage,
            move || ui_text(active_locale.get()).kpi_short_break,
            move || {
                let locale = active_locale.get();
                vm.with(|m| {
                    format_kpi_cadence(
                        m.config.short_break_interval_secs,
                        m.config.short_break_duration_secs,
                        locale,
                    )
                })
            },
        ))
        .child(pastel_kpi_card(
            KpiTone::Peach,
            move || ui_text(active_locale.get()).kpi_long_break,
            move || {
                let locale = active_locale.get();
                vm.with(|m| {
                    format_kpi_cadence(
                        m.config.long_break_interval_secs,
                        m.config.long_break_duration_secs,
                        locale,
                    )
                })
            },
        ))
        .child(pastel_kpi_card(
            KpiTone::Butter,
            move || ui_text(active_locale.get()).kpi_exercises,
            move || {
                vm.with(|m| (m.config.short_breaks.len() + m.config.long_breaks.len()).to_string())
            },
        ))
        .child(pastel_kpi_card(
            KpiTone::Mist,
            move || ui_text(active_locale.get()).kpi_skip_limit,
            move || {
                let locale = active_locale.get();
                let copy = ui_text(locale);
                vm.with(|m| {
                    if m.config.strict_break {
                        copy.kpi_strict.to_owned()
                    } else if m.config.limit_consecutive_skips.enabled {
                        format_kpi_max_skips(m.config.limit_consecutive_skips.max_skips, locale)
                    } else {
                        copy.kpi_off.to_owned()
                    }
                })
            },
        ));

    let toggle_theme_hooks = theme_hooks.clone();
    let toggle_reveal = native_reveal.clone();
    let toggle_queued_theme = Rc::clone(&queued_theme);
    let reset_theme_hooks = theme_hooks;
    let reset_reveal = native_reveal;
    let reset_queued_theme = queued_theme;

    let page_header = view()
        .flex_shrink_0()
        .gap_3()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .window_drag(true)
                .child(page_heading(
                    move || active_tab.get().label(active_locale.get()),
                    &highlight_scale,
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(when(
                            move || countdown_pill.with(Option::is_some),
                            move || {
                                countdown_status_pill(
                                    move || {
                                        countdown_pill.with(|pill| {
                                            pill.as_ref()
                                                .map_or(CountdownTone::Disabled, |state| state.tone)
                                        })
                                    },
                                    move || {
                                        countdown_pill.with(|pill| {
                                            pill.as_ref().map_or_else(String::new, |state| {
                                                state.label.clone()
                                            })
                                        })
                                    },
                                    move || {
                                        countdown_pill.with(|pill| {
                                            pill.as_ref().and_then(|state| state.time.clone())
                                        })
                                    },
                                    move || {
                                        countdown_pill.with(|pill| {
                                            pill.as_ref().map_or_else(String::new, |state| {
                                                state.accessible_label.clone()
                                            })
                                        })
                                    },
                                )
                            },
                        ))
                        .child(language_toggle_button(
                            move || vm.with(|m| m.config.language),
                            move || active_locale.get(),
                            move || {
                                let next_lang = vm.with_untracked(|m| m.config.language.next());
                                let mut saved = false;
                                vm.update(|m| {
                                    m.config.language = next_lang;
                                    if m.autosave().is_ok() {
                                        saved = true;
                                    } else {
                                        let _ = m.persist_language();
                                    }
                                });
                                if saved {
                                    crate::store_outcome(&lang_sink, SettingsOutcome::Saved);
                                }
                            },
                        ))
                        .child(theme_toggle_button(
                            move || theme_mode_for_appearance(appearance.get()),
                            move || active_locale.get(),
                            measured_theme_btn_center,
                            move |origin| {
                                let resolved_origin = origin.unwrap_or_else(|| {
                                    let win = window_state()
                                        .map_or_else(window, |s| s.content_bounds.size);
                                    [(win.width.0 - 60.0).max(0.0), title_bar().top.0 + 32.0]
                                });
                                #[cfg(any(target_os = "linux", target_os = "windows"))]
                                reveal_origin.set(resolved_origin);
                                let current_mode =
                                    toggle_queued_theme.get().1.unwrap_or_else(|| {
                                        theme_mode_for_appearance(appearance.get_untracked())
                                    });
                                let next_mode = next_theme_mode(current_mode);
                                let next_appearance = appearance_for_theme_mode(next_mode);
                                let sink = Arc::clone(&theme_sink);
                                let queued = Rc::clone(&toggle_queued_theme);
                                let seq = queue_theme(&queued, next_mode);
                                let apply_theme = move || {
                                    settle_theme(&queued, seq);
                                    let mut saved = false;
                                    vm.update(|m| {
                                        m.config.theme = next_mode;
                                        if m.autosave().is_ok() {
                                            saved = true;
                                        } else {
                                            let _ = m.persist_theme();
                                        }
                                    });
                                    if saved {
                                        crate::store_outcome(&sink, SettingsOutcome::Saved);
                                    }
                                    appearance.set(next_appearance);
                                    set_color_scheme(next_appearance);
                                };

                                dispatch_theme_transition(
                                    current_mode,
                                    next_mode,
                                    resolved_origin,
                                    &toggle_theme_hooks,
                                    &toggle_reveal,
                                    apply_theme,
                                );
                            },
                        )),
                ),
        )
        .child(kpi_row)
        .child(when(
            move || warning_msg.get().is_some(),
            move || status_banner(false, move || warning_msg.get().unwrap_or_default()),
        ))
        .child(when(
            move || error_msg.get().is_some(),
            move || status_banner(true, move || error_msg.get().unwrap_or_default()),
        ));

    let timing_hook = on_autostart_change.as_ref().map(Arc::clone);
    let reset_hook = on_autostart_change;

    let form_scroll = scroll()
        .element_ref(&form_scroll_ref)
        .flex_1()
        .bg(colors::BACKGROUND_SURFACE)
        .child(
            view()
                .p_5()
                .bg(colors::BACKGROUND_SURFACE)
                .child(when(
                    move || active_tab.get() == SettingsTab::Timing,
                    move || {
                        timing_section(
                            vm,
                            active_locale,
                            Arc::clone(&timing_sink),
                            timing_hook.as_ref().map(Arc::clone),
                            has_switched_tab.get_untracked(),
                        )
                    },
                ))
                .child(when(
                    move || active_tab.get() == SettingsTab::Policies,
                    move || {
                        policies_section(
                            vm,
                            active_locale,
                            Arc::clone(&policies_sink),
                            has_switched_tab.get_untracked(),
                        )
                    },
                ))
                .child(when(
                    move || active_tab.get() == SettingsTab::Exercises,
                    move || {
                        exercises_section(
                            vm,
                            active_locale,
                            Arc::clone(&exercises_sink),
                            has_switched_tab.get_untracked(),
                        )
                    },
                ))
                .child(when(
                    move || active_tab.get() == SettingsTab::About,
                    move || {
                        #[cfg(feature = "msstore")]
                        {
                            about_section(active_locale, has_switched_tab.get_untracked())
                        }
                        #[cfg(not(feature = "msstore"))]
                        {
                            about_section(
                                vm,
                                active_locale,
                                Arc::clone(&about_sink),
                                update_ui,
                                on_check_now.clone(),
                                on_apply_update.clone(),
                                quit_on_finish,
                                has_switched_tab.get_untracked(),
                            )
                        }
                    },
                )),
        );

    let sticky_toolbar = view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .justify_between()
        .px_5()
        .py_3()
        .bg(colors::BACKGROUND_SURFACE)
        .border_t_1()
        .border_color(colors::BORDER)
        .child(
            text(move || ui_text(active_locale.get()).autosave_note)
                .text_xs()
                .text_color(colors::TEXT_SECONDARY),
        )
        .child(primary_button_with_disabled(
            move || ui_text(active_locale.get()).reset_to_defaults,
            move || !is_modified.get(),
            move || {
                let default_mode = ThemeMode::default();
                let default_appearance = appearance_for_theme_mode(default_mode);
                let resolved_origin =
                    measured_theme_btn_center
                        .get_untracked()
                        .unwrap_or_else(|| {
                            let win = window_state().map_or_else(window, |s| s.content_bounds.size);
                            [(win.width.0 - 60.0).max(0.0), title_bar().top.0 + 32.0]
                        });
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                reveal_origin.set(resolved_origin);

                let current_mode = reset_queued_theme
                    .get()
                    .1
                    .unwrap_or_else(|| theme_mode_for_appearance(appearance.get_untracked()));
                let sink = Arc::clone(&reset_sink);
                let autostart_cb = reset_hook.as_ref().map(Arc::clone);
                let queued = Rc::clone(&reset_queued_theme);
                let seq = queue_theme(&queued, default_mode);
                let apply_reset = move || {
                    settle_theme(&queued, seq);
                    let mut autostart_changed = None;
                    let mut saved = false;
                    vm.update(|m| {
                        let prev = m.config.autostart;
                        m.reset_to_defaults();
                        if m.autosave().is_ok() {
                            saved = true;
                        }
                        if m.config.autostart != prev {
                            autostart_changed = Some(m.config.autostart);
                        }
                    });
                    if saved {
                        crate::store_outcome(&sink, SettingsOutcome::Saved);
                    }
                    if saved && let (Some(next), Some(cb)) = (autostart_changed, &autostart_cb) {
                        cb(next);
                    }
                    appearance.set(default_appearance);
                    set_color_scheme(default_appearance);
                };

                dispatch_theme_transition(
                    current_mode,
                    default_mode,
                    resolved_origin,
                    &reset_theme_hooks,
                    &reset_reveal,
                    apply_reset,
                );
            },
        ));

    let form_card = view()
        .flex_1()
        .rounded(radii::CONTAINER)
        .bg(colors::BACKGROUND_SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .overflow_hidden()
        .child(form_scroll)
        .child(sticky_toolbar);

    let right_pane = view()
        .flex_1()
        .gap_3()
        .child(when(
            move || title_bar().right.0 > 0.0,
            || {
                view()
                    .h(move || px((title_bar().top.0 - 8.0).max(0.0)))
                    .w_full()
                    .window_drag(true)
            },
        ))
        .child(page_header)
        .child(form_card);

    let root = view()
        .flex_1()
        .bg(colors::BACKGROUND_BODY)
        .flex_row()
        .p_2()
        .gap_3()
        .child(sidebar)
        .child(right_pane);

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let root = if let Some(shader) = crate::theme::theme_reveal_shader() {
        view()
            .flex_1()
            .child(
                shader_transition()
                    .flex_1()
                    .source(shader)
                    .duration(Duration::from_millis(motion::THEME_REVEAL_MS))
                    .uniform("origin", move || reveal_origin.get())
                    .value(move || {
                        nsh_core::shader::TransitionKey::number(f64::from(reveal_epoch.get()))
                    })
                    .child(root),
            )
            .into_element()
    } else {
        root.into_element()
    };

    root
}

#[component]
fn SettingsRoot() -> impl IntoElement {
    set_quit_mode(QuitMode::LastWindowClosed);
    let initial_launch = SETTINGS_LAUNCH.lock().ok().and_then(|mut g| g.take());
    let initial_locale = initial_launch
        .as_ref()
        .map_or(Locale::En, |launch| launch.vm.active_locale());
    let window_locale = signal(initial_locale);
    let launch_slot = Rc::new(RefCell::new(initial_launch));
    let opts = settings_main_window_options();
    nsh_core::element::desktop::window()
        .id("settings")
        .kind(WindowKind::Normal)
        .title(move || {
            ui_text(window_locale.get())
                .settings_window_title
                .to_owned()
        })
        .default_size(size(px(920.0), px(680.0)))
        .default_position(WindowPosition::Center)
        .attributes(opts.attributes)
        .on_open_change(|e| {
            if !e.open {
                quit();
            }
        })
        .content(move || match launch_slot.borrow_mut().take() {
            Some(launch) => {
                if let Some(on_window_ready) = launch.on_window_ready {
                    on_window_ready();
                    drop(set_timeout(Duration::from_millis(60), move || {
                        on_window_ready();
                    }));
                }
                build_settings_view_inner(
                    launch.vm,
                    launch.outcome_sink,
                    true,
                    launch.on_autostart_change,
                    launch.theme_hooks,
                    launch.update_config,
                    launch.status_config,
                    Some(window_locale),
                )
                .into_element()
            }
            None => view()
                .flex_1()
                .bg(colors::BACKGROUND_BODY)
                .items_center()
                .justify_center()
                .child(
                    text(move || ui_text(window_locale.get()).settings_window_title)
                        .text_color(colors::TEXT_PRIMARY),
                )
                .into_element(),
        })
}

pub fn run_settings(config_path: PathBuf) -> Result<SettingsOutcome, UiError> {
    run_settings_with_autostart(config_path, None, None)
}

pub fn run_settings_with_autostart(
    config_path: PathBuf,
    system_autostart: Option<bool>,
    on_autostart_change: Option<AutostartChangeCallback>,
) -> Result<SettingsOutcome, UiError> {
    run_settings_with_hooks(
        config_path,
        system_autostart,
        on_autostart_change,
        ThemeTransitionHooks::default(),
        SettingsUpdateConfig::default(),
        SettingsStatusConfig::default(),
        None,
        Locale::En,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "Entry point connecting all OS hooks and resolved system locale into the Settings window"
)]
pub fn run_settings_with_hooks(
    config_path: PathBuf,
    system_autostart: Option<bool>,
    on_autostart_change: Option<AutostartChangeCallback>,
    theme_hooks: ThemeTransitionHooks,
    update_config: SettingsUpdateConfig,
    status_config: SettingsStatusConfig,
    on_window_ready: Option<WindowReadyCallback>,
    system_locale: Locale,
) -> Result<SettingsOutcome, UiError> {
    let vm = load_settings_vm(config_path, system_autostart, system_locale);
    let initial_appearance = appearance_for_theme_mode(vm.config.theme);
    let initial_title = ui_text(vm.active_locale()).settings_window_title.to_owned();
    let outcome_sink = Arc::new(Mutex::new(None));
    let on_autostart_change: Option<AutostartHook> = on_autostart_change.map(Arc::from);

    let Ok(mut guard) = SETTINGS_LAUNCH.lock() else {
        return Err(UiError::WindowInit(
            "settings launch state lock poisoned".to_owned(),
        ));
    };
    *guard = Some(SettingsLaunch {
        vm,
        outcome_sink: Arc::clone(&outcome_sink),
        on_autostart_change,
        theme_hooks,
        update_config,
        status_config,
        on_window_ready,
    });
    drop(guard);

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let root = platform::Root::of::<SettingsRoot>()
            .appearance(initial_appearance)
            .main_window(settings_main_window_options());
        let _ = platform::run_app(
            platform::AppSource::Rust(root),
            platform::AppOptions {
                title: initial_title,
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                app_id: "io.github.tunglt1810.safeyoureyes.settings".to_owned(),
                size: (920, 680),
                appearance: initial_appearance,
                ..Default::default()
            },
        );
    }

    let resolved = outcome_sink
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .unwrap_or(SettingsOutcome::Cancelled);
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(not(feature = "msstore"))]
    use sye_core::HOMEBREW_UPGRADE_COMMAND;

    use super::*;

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct RepoTempDir {
        path: PathBuf,
    }

    impl RepoTempDir {
        fn new(label: &str) -> std::io::Result<Self> {
            let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join(".tmp")
                .join(format!(
                    "sye-ui-settings-{label}-{}-{id}",
                    std::process::id()
                ));
            fs::create_dir_all(&root)?;
            Ok(Self { path: root })
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for RepoTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn load_missing_file_uses_defaults_without_warning() -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("missing")?;
        let path = dir.file("config.toml");

        let vm = SettingsViewModel::load(path);
        assert_eq!(vm.config, Config::default());
        assert_eq!(vm.load_warning, None);
        assert_eq!(vm.validation_error, None);
        assert_eq!(vm.active_tab, SettingsTab::Timing);
        assert!(!vm.should_close());
        Ok(())
    }

    #[test]
    fn load_corrupt_file_falls_back_to_defaults_with_warning()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("corrupt")?;
        let path = dir.file("config.toml");
        fs::write(&path, "not valid toml === [[[")?;

        let vm = SettingsViewModel::load(path);
        assert_eq!(vm.config, Config::default());
        assert!(vm.load_warning.is_some());
        Ok(())
    }

    #[test]
    fn save_round_trips_all_edited_fields() -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("roundtrip")?;
        let path = dir.file("nested").join("config.toml");

        let mut vm = SettingsViewModel::load(path.clone());
        vm.config.theme = ThemeMode::Dark;
        vm.config.short_break_interval_secs = 600;
        vm.config.short_break_duration_secs = 20;
        vm.config.long_break_interval_secs = 3600;
        vm.config.long_break_duration_secs = 90;
        vm.config.pre_break_warning_secs = 15;
        vm.config.strict_break = false;
        vm.config.allow_postpone = true;
        vm.config.postpone_duration_secs = 180;
        vm.config.random_order = false;
        vm.config.shortcut_disable_secs = 3;
        vm.config.fade_in_ms = 800;
        vm.config.persist_state = true;
        vm.config.autostart = false;
        vm.config.smart_pause.enabled = false;
        vm.config.smart_pause.idle_threshold_secs = 30;
        vm.config.smart_pause.postpone_if_active = true;
        vm.config.do_not_disturb.enabled = false;
        vm.config.do_not_disturb.while_gaming = false;
        vm.config.do_not_disturb.while_on_battery = true;
        vm.add_skip_window("vlc")?;
        vm.add_take_window("firefox")?;
        vm.config.audible_alert.pre_break = true;
        vm.config.audible_alert.post_break = false;
        vm.config.audible_alert.volume = 65;
        vm.config.limit_consecutive_skips.enabled = true;
        vm.config.limit_consecutive_skips.max_skips = 3;

        let new_idx = vm.config.short_breaks.len();
        vm.add_exercise(BreakType::Short, "Look out the window")?;
        vm.set_exercise_interval_override(BreakType::Short, new_idx, Some(720))?;
        vm.set_exercise_duration_override(BreakType::Short, new_idx, Some(25))?;

        let outcome = vm.save()?;
        assert_eq!(outcome, SettingsOutcome::Saved);
        assert!(vm.should_close());
        assert_eq!(vm.resolved_outcome(), SettingsOutcome::Saved);

        let reloaded = Config::load_from_path(&path)?;
        assert_eq!(reloaded, vm.config);
        assert_eq!(reloaded.theme, ThemeMode::Dark);
        Ok(())
    }

    #[test]
    fn cancel_does_not_write_to_disk() -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("cancel")?;
        let path = dir.file("config.toml");

        let mut vm = SettingsViewModel::load(path.clone());
        vm.config.short_break_interval_secs = 42;
        assert_eq!(vm.cancel(), SettingsOutcome::Cancelled);
        assert!(vm.should_close());
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn reset_to_defaults_restores_in_memory_without_writing()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("reset")?;
        let path = dir.file("config.toml");

        let mut vm = SettingsViewModel::load(path.clone());
        vm.config.theme = ThemeMode::Dark;
        vm.config.short_break_interval_secs = 0;
        vm.draft_short_exercise = "draft".to_owned();
        vm.draft_skip_window = "mpv".to_owned();
        assert!(vm.validate().is_err());

        vm.reset_to_defaults();
        assert_eq!(vm.config, Config::default());
        assert_eq!(vm.config.theme, ThemeMode::System);
        assert_eq!(vm.validation_error, None);
        assert!(vm.draft_short_exercise.is_empty());
        assert!(vm.draft_skip_window.is_empty());
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn exercise_add_remove_and_last_remaining_guard() -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("exercises")?;
        let mut vm = SettingsViewModel::load(dir.file("config.toml"));

        assert!(vm.add_exercise(BreakType::Short, "   ").is_err());
        assert!(vm.remove_exercise(BreakType::Short, 999).is_err());
        assert!(
            vm.set_exercise_interval_override(BreakType::Long, 999, Some(60))
                .is_err()
        );
        assert!(
            vm.set_exercise_duration_override(BreakType::Long, 999, Some(10))
                .is_err()
        );

        vm.config.short_breaks = vec![BreakSpec::new("Only short")];
        vm.config.long_breaks.clear();
        assert!(vm.remove_exercise(BreakType::Short, 0).is_err());
        assert_eq!(vm.config.short_breaks.len(), 1);

        vm.add_exercise(BreakType::Long, "Walk")?;
        vm.remove_exercise(BreakType::Short, 0)?;
        assert!(vm.config.short_breaks.is_empty());
        assert_eq!(vm.config.long_breaks.len(), 1);
        Ok(())
    }

    #[test]
    fn window_lists_validate_and_remove() -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("windows")?;
        let mut vm = SettingsViewModel::load(dir.file("config.toml"));

        assert!(vm.add_skip_window("   ").is_err());
        assert!(vm.add_take_window("").is_err());
        assert!(vm.remove_skip_window(0).is_err());
        assert!(vm.remove_take_window(0).is_err());

        vm.add_skip_window("  zoom  ")?;
        vm.add_take_window("  terminal  ")?;
        assert_eq!(vm.config.do_not_disturb.skip_break_windows, vec!["zoom"]);
        assert_eq!(
            vm.config.do_not_disturb.take_break_windows,
            vec!["terminal"]
        );

        vm.remove_skip_window(0)?;
        vm.remove_take_window(0)?;
        assert!(vm.config.do_not_disturb.skip_break_windows.is_empty());
        assert!(vm.config.do_not_disturb.take_break_windows.is_empty());
        Ok(())
    }

    #[test]
    fn validate_rejects_empty_exercise_lists_and_invalid_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("validate")?;
        let mut vm = SettingsViewModel::load(dir.file("config.toml"));

        vm.config.short_breaks.clear();
        vm.config.long_breaks.clear();
        assert!(vm.save().is_err());
        assert!(!vm.should_close());
        assert!(vm.validation_error.is_some());

        vm.reset_to_defaults();
        vm.config.audible_alert.volume = 101;
        assert!(vm.validate().is_err());
        assert!(vm.validation_error.is_some());
        Ok(())
    }

    #[test]
    fn save_to_unwritable_path_surfaces_error_without_closing()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("unwritable")?;
        let dir_as_file = dir.path.clone();
        let mut vm = SettingsViewModel::load(dir_as_file);

        let res = vm.save();
        assert!(res.is_err());
        assert!(!vm.should_close());
        assert!(vm.validation_error.is_some());
        Ok(())
    }

    #[test]
    fn settings_main_window_uses_hidden_titlebar() {
        let opts = settings_main_window_options();
        let tb = opts.attributes.title_bar;
        assert!(tb.is_some());
        if let Some(tb) = tb {
            assert_eq!(tb.style, TitleBarStyle::Hidden);
            assert_eq!(tb.height, Some(px(36.0)));
        }
    }

    #[test]
    fn nsh_headless_settings_navigates_sidebar_cycles_theme_and_toggles_switch()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-nav")?;
        static NAV_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        if let Ok(mut guard) = NAV_PATH.lock() {
            *guard = Some(dir.file("headless-settings-test.toml"));
        }

        #[component]
        fn TestSettings() -> impl IntoElement {
            let path = NAV_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Safe Your Eyes")?;
        let sidebar_node = app
            .screen()
            .layout_nodes()
            .into_iter()
            .find(|n| (n.bounds.size.width.0 - 210.0).abs() < 0.01)
            .ok_or("expected 210pt Floating Sage Sidebar node")?;
        assert!(
            (sidebar_node.bounds.origin.x.0 - 8.0).abs() < 0.01
                && (sidebar_node.bounds.origin.y.0 - 8.0).abs() < 0.01
                && (sidebar_node.bounds.size.height.0 - 664.0).abs() < 0.01,
            "Expected 8pt concentric outer inset for sidebar in 920x680 window, got {:?}",
            sidebar_node.bounds
        );
        assert!(!app.shows("Settings"));
        assert!(app.shows("Timing"));
        assert!(app.shows("Break Schedule"));
        assert!(app.shows("Short Break"));
        assert!(app.shows("Launch at login (startup)"));
        assert!(app.shows("Theme: System"));

        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        app.press("Theme: Dark")?;
        app.wait_for("Theme: System")?;

        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        assert!(!app.shows("Settings"));
        assert!(app.shows("Theme: System"));
        app.press("Strict break (hide Skip)")?;
        app.wait_for("Strict")?;

        app.press("Exercises")?;
        app.wait_for("Short Break Exercises")?;
        assert!(app.shows("Theme: System"));

        app.press("About & Support")?;
        app.wait_for("Buy Me a Coffee")?;
        assert!(!app.shows("Cozy Appearance"));
        assert!(app.shows("Theme: System"));
        assert!(app.shows("Support link: https://buymeacoffee.com/bbeeezzzzz"));
        assert!(app.shows("Scan to Buy Me a Coffee"));
        Ok(())
    }

    #[test]
    fn nsh_headless_settings_autosave_and_reset_to_defaults()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_core::MountItem;
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-autosave")?;
        let save_path = dir.file("autosaved.toml");

        static TEST_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

        #[component]
        fn TestPersistSettings() -> impl IntoElement {
            let path = TEST_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        if let Ok(mut guard) = TEST_PATH.lock() {
            *guard = Some(save_path.clone());
        }
        let shader_transitions = Arc::new(Mutex::new(Vec::<Duration>::new()));
        let shader_log = Arc::clone(&shader_transitions);

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestPersistSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.observe(move |_step, batch| {
            for item in &batch.items {
                if let MountItem::ShaderTransition { duration, .. } = item
                    && let Ok(mut guard) = shader_log.lock()
                {
                    guard.push(*duration);
                }
            }
        });
        app.wait_for("Theme: System")?;
        assert!(!app.shows("Save"));
        assert!(!app.shows("Cancel"));
        assert!(app.shows("Reset to defaults"));

        assert!(app.press("Reset to defaults").is_err());
        assert!(!save_path.exists());

        // Headless starts in Light mode: System -> Light is a no-op and must not trigger wave.
        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        assert!(
            shader_transitions
                .lock()
                .map_err(|e| e.to_string())?
                .is_empty(),
            "No-op System -> Light transition on a Light system must not trigger reveal"
        );

        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        let transitions = shader_transitions
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        assert_eq!(
            transitions,
            vec![Duration::from_millis(motion::THEME_REVEAL_MS)],
            "Light -> Dark must trigger one 480ms circular ShaderTransition on Linux/Windows"
        );
        #[cfg(target_os = "macos")]
        assert!(
            transitions.is_empty(),
            "macOS uses native Core Animation circular reveal instead of nsh ShaderTransition"
        );
        assert_eq!(Config::load_from_path(&save_path)?.theme, ThemeMode::Dark);

        app.press("Reset to defaults")?;
        app.wait_for("Theme: System")?;
        assert_eq!(Config::load_from_path(&save_path)?.theme, ThemeMode::System);
        assert!(app.press("Reset to defaults").is_err());

        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        app.press("Strict break (hide Skip)")?;
        drop(app);

        let reloaded = Config::load_from_path(&save_path)?;
        assert_eq!(reloaded.theme, ThemeMode::Light);
        assert!(reloaded.strict_break);
        Ok(())
    }

    #[test]
    fn strict_break_auto_disables_skip_limit_and_updates_kpi()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("strict-vs-skip")?;
        let path = dir.file("strict-vs-skip.toml");

        let mut vm = SettingsViewModel::load(path.clone());
        assert!(!vm.is_modified_from_defaults());
        vm.set_limit_consecutive_skips(true);
        assert!(vm.config.limit_consecutive_skips.enabled);
        assert!(vm.is_modified_from_defaults());
        vm.set_strict_break(true);
        assert!(vm.config.strict_break);
        assert!(!vm.config.limit_consecutive_skips.enabled);
        vm.set_limit_consecutive_skips(true);
        assert!(!vm.config.limit_consecutive_skips.enabled);
        vm.set_strict_break(false);
        vm.set_limit_consecutive_skips(true);
        assert!(vm.config.limit_consecutive_skips.enabled);
        vm.reset_to_defaults();
        assert!(!vm.is_modified_from_defaults());

        static STRICT_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        if let Ok(mut guard) = STRICT_PATH.lock() {
            *guard = Some(path.clone());
        }

        #[component]
        fn TestStrictSettings() -> impl IntoElement {
            let path = STRICT_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestStrictSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Timing")?;
        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        assert!(app.shows("Off"));

        app.press("Limit consecutive skips")?;
        app.wait_for("2 max")?;

        app.press("Strict break (hide Skip)")?;
        app.wait_for("Strict")?;
        assert!(!app.shows("2 max"));
        drop(app);

        let saved = Config::load_from_path(&path)?;
        assert!(saved.strict_break);
        assert!(!saved.limit_consecutive_skips.enabled);

        let mut legacy = Config {
            strict_break: true,
            ..Config::default()
        };
        legacy.limit_consecutive_skips.enabled = true;
        legacy.save_to_path(&path)?;
        let loaded_legacy = SettingsViewModel::load(path);
        assert!(loaded_legacy.config.strict_break);
        assert!(!loaded_legacy.config.limit_consecutive_skips.enabled);
        Ok(())
    }

    #[test]
    fn persist_theme_preserves_system_autostart_baseline() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = RepoTempDir::new("persist-theme-autostart")?;
        let path = dir.file("theme-autostart.toml");

        let mut vm = load_settings_vm(path.clone(), Some(false), Locale::En);
        vm.config.short_break_interval_secs = 123;
        vm.config.theme = ThemeMode::Dark;
        vm.persist_theme()?;

        let saved = Config::load_from_path(&path)?;
        assert_eq!(saved.theme, ThemeMode::Dark);
        assert!(!saved.autostart);
        assert_eq!(saved.short_break_interval_secs, 900);
        Ok(())
    }

    #[test]
    fn config_validation_errors_are_translated_to_vietnamese()
    -> Result<(), Box<dyn std::error::Error>> {
        let cfg = Config {
            short_break_interval_secs: 10,
            short_break_duration_secs: 0,
            pre_break_warning_secs: 10,
            audible_alert: sye_core::config::AudibleAlertConfig {
                volume: 101,
                ..Config::default().audible_alert
            },
            smart_pause: sye_core::config::SmartPauseConfig {
                idle_threshold_secs: 0,
                ..Config::default().smart_pause
            },
            limit_consecutive_skips: sye_core::config::SkipLimitConfig {
                enabled: true,
                max_skips: 0,
            },
            short_breaks: vec![BreakSpec::new("  ")],
            ..Config::default()
        };
        let err = cfg.validate().err().ok_or("config must be invalid")?;
        let vi = format_config_error(&err, Locale::Vi);
        assert!(vi.starts_with("Cấu hình không hợp lệ: "), "{vi}");
        assert!(!vi.contains("must"), "untranslated core message: {vi}");
        assert_eq!(format_config_error(&err, Locale::En), err.to_string());
        Ok(())
    }

    #[test]
    fn persist_language_preserves_system_autostart_baseline()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("persist-lang-autostart")?;
        let path = dir.file("lang-autostart.toml");

        let mut vm = load_settings_vm(path.clone(), Some(false), Locale::En);
        vm.config.short_break_interval_secs = 123;
        vm.config.language = sye_core::LanguageMode::Vi;
        vm.persist_language()?;

        let saved = Config::load_from_path(&path)?;
        assert_eq!(saved.language, sye_core::LanguageMode::Vi);
        assert!(!saved.autostart);
        assert_eq!(saved.short_break_interval_secs, 900);
        Ok(())
    }

    #[test]
    fn nsh_headless_tab_switch_triggers_staggered_list_item_animations()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_core::MountItem;
        use nsh_core::animation::{AnimationOp, Driver};
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("stagger")?;
        static STAGGER_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        if let Ok(mut guard) = STAGGER_PATH.lock() {
            *guard = Some(dir.file("stagger.toml"));
        }

        #[component]
        fn TestStaggerSettings() -> impl IntoElement {
            let path = STAGGER_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        let observed_delays: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed_delays);

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestStaggerSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.observe(move |_step, batch| {
            for item in &batch.items {
                if let MountItem::Animation(AnimationOp::Start {
                    driver:
                        Driver::Timing {
                            duration_ms,
                            delay_ms,
                            ..
                        },
                    ..
                }) = item
                    && (*duration_ms - motion::ROW_ENTER_MS).abs() < 0.01
                    && let Ok(mut guard) = sink.lock()
                {
                    guard.push(*delay_ms);
                }
            }
        });

        app.wait_for("Break Schedule")?;
        assert!(
            observed_delays
                .lock()
                .map_err(|e| e.to_string())?
                .is_empty(),
            "Initial settings load must not trigger staggered row entrance"
        );

        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        let delays = observed_delays.lock().map_err(|e| e.to_string())?.clone();
        assert!(
            delays.contains(&0.0) && delays.contains(&28.0) && delays.contains(&56.0),
            "Expected Cozy 28ms staggered delays on tab switch, got {delays:?}"
        );
        assert!(
            delays.iter().all(|&d| d <= 336.0),
            "Expected staggered delays capped at 12 * 28ms = 336ms, got {delays:?}"
        );

        app.press("Exercises")?;
        app.wait_for("Short Break Exercises")?;
        assert!(app.shows("19"));
        if let Ok(mut guard) = observed_delays.lock() {
            guard.clear();
        }
        app.press("Remove >> nth=0")?;
        app.wait_for("18")?;
        assert!(
            observed_delays
                .lock()
                .map_err(|e| e.to_string())?
                .is_empty(),
            "In-place exercise edits must not re-trigger tab-switch stagger animation"
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_reopen_dark_switches_to_system_and_resets()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_core::MountItem;
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-reopen-dark")?;
        let dark_path = dir.file("dark.toml");
        let dark_cfg = Config {
            theme: ThemeMode::Dark,
            ..Config::default()
        };
        dark_cfg.save_to_path(&dark_path)?;

        static DARK_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static IS_DARK_NOW: Mutex<Option<bool>> = Mutex::new(None);

        #[component]
        fn TestDarkSettings() -> impl IntoElement {
            let path = DARK_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            effect(move || {
                let d = dark();
                if let Ok(mut guard) = IS_DARK_NOW.lock() {
                    *guard = Some(d);
                }
            });
            build_settings_view(vm, sink, false)
        }

        if let Ok(mut guard) = DARK_PATH.lock() {
            *guard = Some(dark_path);
        }
        let observed_appearances: Arc<Mutex<Vec<Appearance>>> = Arc::new(Mutex::new(Vec::new()));
        let app_sink = Arc::clone(&observed_appearances);

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(
                Root::of::<TestDarkSettings>()
                    .appearance(Appearance::Dark)
                    .main_window(settings_main_window_options()),
            )
        })
        .map_err(|e| e.to_string())?;
        app.observe(move |_step, batch| {
            for item in &batch.items {
                if let MountItem::Appearance { appearance } = item
                    && let Ok(mut guard) = app_sink.lock()
                {
                    guard.push(*appearance);
                }
            }
        });

        app.wait_for("Theme: Dark")?;
        assert_eq!(
            IS_DARK_NOW.lock().ok().and_then(|g| *g),
            Some(true),
            "Saved Dark config must render dark on startup"
        );

        app.press("Theme: Dark")?;
        app.wait_for("Theme: System")?;
        app.settle();
        assert_eq!(
            IS_DARK_NOW.lock().ok().and_then(|g| *g),
            Some(false),
            "Switching Dark -> System on Light OS must flip dark() back to false"
        );
        let seen = observed_appearances
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        assert!(
            seen.contains(&Appearance::System),
            "Expected MountItem::Appearance(System) after Dark -> System, got {seen:?}"
        );

        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        app.settle();
        assert_eq!(IS_DARK_NOW.lock().ok().and_then(|g| *g), Some(true));

        app.press("Reset to defaults")?;
        app.wait_for("Theme: System")?;
        app.settle();
        assert_eq!(
            IS_DARK_NOW.lock().ok().and_then(|g| *g),
            Some(false),
            "Reset from Dark must restore System (Light OS) appearance"
        );
        Ok(())
    }

    #[test]
    fn load_settings_vm_overrides_autostart_when_system_value_present()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = RepoTempDir::new("system-autostart")?;
        let path = dir.file("config.toml");
        let cfg = Config {
            autostart: false,
            ..Config::default()
        };
        cfg.save_to_path(&path)?;

        let unchanged = load_settings_vm(path.clone(), None, Locale::En);
        assert!(!unchanged.config.autostart);

        let overridden_true = load_settings_vm(path.clone(), Some(true), Locale::En);
        assert!(overridden_true.config.autostart);

        let cfg_true = Config {
            autostart: true,
            ..Config::default()
        };
        cfg_true.save_to_path(&path)?;
        let overridden_false = load_settings_vm(path, Some(false), Locale::En);
        assert!(!overridden_false.config.autostart);
        Ok(())
    }

    #[test]
    fn nsh_headless_autostart_override_and_live_callback_sync()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-autostart")?;
        let path = dir.file("autostart.toml");
        let cfg = Config {
            autostart: true,
            ..Config::default()
        };
        cfg.save_to_path(&path)?;

        static AUTOSTART_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static AUTOSTART_EVENTS: Mutex<Vec<bool>> = Mutex::new(Vec::new());

        #[component]
        fn TestAutostartSettings() -> impl IntoElement {
            let path = AUTOSTART_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = load_settings_vm(path, Some(false), Locale::En);
            let sink = Arc::new(Mutex::new(None));
            let hook: AutostartHook = Arc::new(|enabled| {
                if let Ok(mut guard) = AUTOSTART_EVENTS.lock() {
                    guard.push(enabled);
                }
            });
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                Some(hook),
                ThemeTransitionHooks::default(),
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        if let Ok(mut guard) = AUTOSTART_PATH.lock() {
            *guard = Some(path.clone());
        }
        if let Ok(mut guard) = AUTOSTART_EVENTS.lock() {
            guard.clear();
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestAutostartSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Launch at login (startup)")?;

        app.press("Launch at login (startup)")?;
        app.press("Launch at login (startup)")?;
        app.press("Reset to defaults")?;
        app.press("Launch at login (startup)")?;
        drop(app);

        let events = AUTOSTART_EVENTS.lock().map_err(|e| e.to_string())?.clone();
        assert_eq!(events, vec![true, false, true, false]);

        let reloaded = Config::load_from_path(&path)?;
        assert!(!reloaded.autostart);
        Ok(())
    }

    #[test]
    fn nsh_headless_policies_gaming_auto_detect_toggle_autosaves()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-gaming-toggle")?;
        let path = dir.file("gaming-toggle.toml");

        static GAMING_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

        #[component]
        fn TestGamingToggleSettings() -> impl IntoElement {
            let path = GAMING_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                ThemeTransitionHooks::default(),
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        if let Ok(mut guard) = GAMING_PATH.lock() {
            *guard = Some(path.clone());
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestGamingToggleSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Policies")?;
        app.press("Policies")?;
        app.wait_for("Pause while gaming (auto-detect)")?;

        app.press("Pause while gaming (auto-detect)")?;
        let disabled = Config::load_from_path(&path)?;
        assert!(!disabled.do_not_disturb.while_gaming);

        app.press("Reset to defaults")?;
        let restored = Config::load_from_path(&path)?;
        assert!(restored.do_not_disturb.while_gaming);
        drop(app);
        Ok(())
    }

    #[test]
    fn nsh_headless_autostart_autosaves_without_revert_on_window_close()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-autostart-autosave")?;
        let path = dir.file("autostart-autosave.toml");
        let cfg = Config {
            autostart: false,
            ..Config::default()
        };
        cfg.save_to_path(&path)?;

        static AUTOSAVE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static AUTOSAVE_EVENTS: Mutex<Vec<bool>> = Mutex::new(Vec::new());

        #[component]
        fn TestAutostartAutosave() -> impl IntoElement {
            let path = AUTOSAVE_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = load_settings_vm(path, Some(false), Locale::En);
            let sink = Arc::new(Mutex::new(None));
            let hook: AutostartHook = Arc::new(|enabled| {
                if let Ok(mut guard) = AUTOSAVE_EVENTS.lock() {
                    guard.push(enabled);
                }
            });
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                Some(hook),
                ThemeTransitionHooks::default(),
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        if let Ok(mut guard) = AUTOSAVE_PATH.lock() {
            *guard = Some(path.clone());
        }
        if let Ok(mut guard) = AUTOSAVE_EVENTS.lock() {
            guard.clear();
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestAutostartAutosave>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Launch at login (startup)")?;
        app.press("Launch at login (startup)")?;
        drop(app);

        let events = AUTOSAVE_EVENTS.lock().map_err(|e| e.to_string())?.clone();
        assert_eq!(events, vec![true]);

        let reloaded = Config::load_from_path(&path)?;
        assert!(reloaded.autostart);
        Ok(())
    }

    #[test]
    fn nsh_headless_tab_switch_resets_form_scroll_to_top() -> Result<(), Box<dyn std::error::Error>>
    {
        use nsh_core::Command;
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-scroll-reset")?;
        static SCROLL_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        if let Ok(mut guard) = SCROLL_PATH.lock() {
            *guard = Some(dir.file("scroll-reset.toml"));
        }

        #[component]
        fn TestScrollResetSettings() -> impl IntoElement {
            let path = SCROLL_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestScrollResetSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Break Schedule")?;

        let reset_cmd = Command::ScrollTo {
            x: Points::ZERO,
            y: Points::ZERO,
            animated: false,
        };
        assert!(!app.screen().sent(&reset_cmd));

        app.scroll("Break Schedule", Points(200.0))?;
        app.clear_commands();
        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        assert!(app.screen().sent(&reset_cmd));
        assert!(
            app.scroll("Break Enforcement", Points(-20.0)).is_err(),
            "Scroll offset must reset to 0 after switching from Timing to Policies"
        );

        app.press("Exercises")?;
        app.wait_for("Short Break Exercises")?;
        app.scroll("Short Break Exercises", Points(300.0))?;
        app.clear_commands();
        app.press("Timing")?;
        app.wait_for("Break Schedule")?;
        assert!(app.screen().sent(&reset_cmd));
        assert!(
            app.scroll("Break Schedule", Points(-20.0)).is_err(),
            "Scroll offset must reset to 0 after switching from Exercises to Timing"
        );
        Ok(())
    }

    #[cfg(not(feature = "msstore"))]
    #[test]
    fn nsh_headless_update_badge_and_controls_flow() -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-updates")?;
        let config_path = dir.file("updates.toml");
        static UPDATE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static APPLIED_VERSIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());
        if let Ok(mut guard) = UPDATE_PATH.lock() {
            *guard = Some(config_path.clone());
        }
        if let Ok(mut guard) = APPLIED_VERSIONS.lock() {
            guard.clear();
        }

        #[component]
        fn TestUpdateSettings() -> impl IntoElement {
            let path = UPDATE_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let update_cfg = SettingsUpdateConfig {
                initial_update: Some(UpdateInfo {
                    latest_version: "9.9.9".to_owned(),
                    download_url: DOWNLOAD_PAGE_URL.to_owned(),
                }),
                strategy: Some(UpdateStrategy::SelfUpdate {
                    asset_name: "safe-your-eyes-linux-x86_64",
                }),
                on_check_now: Some(Arc::new(|| {
                    Ok(Some(UpdateInfo {
                        latest_version: "9.9.10".to_owned(),
                        download_url: DOWNLOAD_PAGE_URL.to_owned(),
                    }))
                })),
                on_apply_update: Some(Arc::new(|ver| {
                    if let Ok(mut guard) = APPLIED_VERSIONS.lock() {
                        guard.push(ver.to_owned());
                    }
                    Ok(ApplyUpdateOutcome::UpdatedStandalone)
                })),
                on_open_download: None,
            };
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                ThemeTransitionHooks::default(),
                update_cfg,
                SettingsStatusConfig::default(),
            )
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestUpdateSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("v9.9.9")?;
        assert!(app.shows(concat!("v", env!("CARGO_PKG_VERSION"))));

        app.press("About & Support")?;
        app.wait_for("Updates")?;
        assert!(app.shows("Update & Restart"));
        assert!(app.shows("Download Page"));
        assert!(app.shows("Check for Updates"));
        assert!(app.shows("Check for updates automatically"));

        app.press("Check for Updates")?;
        app.wait_for("v9.9.10")?;
        assert!(app.shows("Version v9.9.10 is available."));

        app.press("v9.9.10")?;
        app.wait_for(
            "Update installed and verified. Restart Safe Your Eyes to use the new version.",
        )?;
        assert_eq!(
            APPLIED_VERSIONS
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            vec!["9.9.10".to_owned()]
        );

        app.press("Check for updates automatically")?;
        drop(app);

        let reloaded = Config::load_from_path(&config_path)?;
        assert!(!reloaded.check_for_updates);
        Ok(())
    }

    #[cfg(feature = "msstore")]
    #[test]
    fn nsh_headless_msstore_hides_in_app_updater_and_keeps_support()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-msstore")?;
        let config_path = dir.file("msstore.toml");
        static MSSTORE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        if let Ok(mut guard) = MSSTORE_PATH.lock() {
            *guard = Some(config_path);
        }

        #[component]
        fn TestMsStoreSettings() -> impl IntoElement {
            let path = MSSTORE_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let update_cfg = SettingsUpdateConfig {
                initial_update: Some(UpdateInfo {
                    latest_version: "9.9.9".to_owned(),
                    download_url: sye_core::PUBLIC_DOWNLOAD_URL.to_owned(),
                }),
                strategy: Some(UpdateStrategy::SelfUpdate {
                    asset_name: "safe-your-eyes-windows-x86_64.exe",
                }),
                on_check_now: None,
                on_apply_update: None,
                on_open_download: None,
            };
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                ThemeTransitionHooks::default(),
                update_cfg,
                SettingsStatusConfig::default(),
            )
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestMsStoreSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for(concat!("v", env!("CARGO_PKG_VERSION")))?;
        assert!(!app.shows("v9.9.9"));

        app.press("About & Support")?;
        app.wait_for("Updates are managed by Microsoft Store.")?;
        assert!(app.shows("Updates"));
        assert!(app.shows(concat!("Current version: v", env!("CARGO_PKG_VERSION"))));
        assert!(app.shows("Buy Me a Coffee"));
        assert!(app.shows("Scan to Buy Me a Coffee"));
        assert!(!app.shows("Check for Updates"));
        assert!(!app.shows("Check for updates automatically"));
        assert!(!app.shows("Update & Restart"));
        assert!(!app.shows("Download Page"));
        assert!(!app.shows("Download Update"));
        Ok(())
    }

    #[test]
    fn may_change_resolved_dark_detects_noop_explicit_modes() {
        assert!(!may_change_resolved_dark(false, ThemeMode::Light, None));
        assert!(may_change_resolved_dark(false, ThemeMode::Dark, None));
        assert!(may_change_resolved_dark(false, ThemeMode::System, None));
        assert!(!may_change_resolved_dark(
            false,
            ThemeMode::System,
            Some(false)
        ));
        assert!(may_change_resolved_dark(
            false,
            ThemeMode::System,
            Some(true)
        ));
        assert!(may_change_resolved_dark(true, ThemeMode::Light, None));
        assert!(!may_change_resolved_dark(true, ThemeMode::Dark, None));
        assert!(may_change_resolved_dark(true, ThemeMode::System, None));
        assert!(may_change_resolved_dark(
            true,
            ThemeMode::System,
            Some(false)
        ));
        assert!(!may_change_resolved_dark(
            true,
            ThemeMode::System,
            Some(true)
        ));
    }

    #[cfg(not(feature = "msstore"))]
    #[test]
    fn nsh_headless_update_badge_opens_download_page_for_homebrew_cask()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-homebrew-badge")?;
        let config_path = dir.file("homebrew.toml");
        static HOMEBREW_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static OPENED_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
        if let Ok(mut guard) = HOMEBREW_PATH.lock() {
            *guard = Some(config_path);
        }
        if let Ok(mut guard) = OPENED_URLS.lock() {
            guard.clear();
        }

        #[component]
        fn TestHomebrewBadgeSettings() -> impl IntoElement {
            let path = HOMEBREW_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let update_cfg = SettingsUpdateConfig {
                initial_update: Some(UpdateInfo {
                    latest_version: "9.9.9".to_owned(),
                    download_url: DOWNLOAD_PAGE_URL.to_owned(),
                }),
                strategy: Some(UpdateStrategy::HomebrewCask),
                on_check_now: None,
                on_apply_update: None,
                on_open_download: Some(Arc::new(|url| {
                    if let Ok(mut guard) = OPENED_URLS.lock() {
                        guard.push(url.to_owned());
                    }
                    Ok(())
                })),
            };
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                ThemeTransitionHooks::default(),
                update_cfg,
                SettingsStatusConfig::default(),
            )
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestHomebrewBadgeSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("v9.9.9")?;

        app.press("v9.9.9")?;
        app.wait_for("Updates")?;
        app.wait_for(&format!(
            "Opened download page. Or run: {HOMEBREW_UPGRADE_COMMAND}"
        ))?;

        app.press("v9.9.9")?;
        assert_eq!(
            OPENED_URLS.lock().map(|g| g.clone()).unwrap_or_default(),
            vec![DOWNLOAD_PAGE_URL.to_owned(), DOWNLOAD_PAGE_URL.to_owned()]
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_theme_change_hook_fires_on_toggle_and_reset()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-theme-hook")?;
        let path = dir.file("theme-hook.toml");

        static THEME_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static THEME_EVENTS: Mutex<Vec<ThemeMode>> = Mutex::new(Vec::new());

        #[component]
        fn TestThemeHookSettings() -> impl IntoElement {
            let path = THEME_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let theme_hook: ThemeChangeCallback = Arc::new(|mode| {
                if let Ok(mut guard) = THEME_EVENTS.lock() {
                    guard.push(mode);
                }
            });
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                ThemeTransitionHooks {
                    on_theme_change: Some(theme_hook),
                    ..ThemeTransitionHooks::default()
                },
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        if let Ok(mut guard) = THEME_PATH.lock() {
            *guard = Some(path);
        }
        if let Ok(mut guard) = THEME_EVENTS.lock() {
            guard.clear();
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestThemeHookSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: System")?;

        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        app.press("Reset to defaults")?;
        app.wait_for("Theme: System")?;
        drop(app);

        let events = THEME_EVENTS.lock().map_err(|e| e.to_string())?.clone();
        assert_eq!(
            events,
            vec![ThemeMode::Light, ThemeMode::Dark, ThemeMode::System]
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_native_snapshot_reveal_runs_capture_then_reveal_on_flip()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-native-reveal")?;
        let path = dir.file("native-reveal.toml");

        static REVEAL_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static REVEAL_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

        #[component]
        fn TestNativeRevealSettings() -> impl IntoElement {
            let path = REVEAL_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let hooks = ThemeTransitionHooks {
                on_theme_change: Some(Arc::new(|mode| {
                    if let Ok(mut guard) = REVEAL_TRACE.lock() {
                        guard.push(format!("sync:{mode:?}"));
                    }
                })),
                capture_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = REVEAL_TRACE.lock() {
                        guard.push("capture".to_owned());
                    }
                    true
                })),
                reveal_snapshot: Some(Arc::new(|origin, duration_ms| {
                    if let Ok(mut guard) = REVEAL_TRACE.lock() {
                        let valid_origin = origin[0] > 0.0 && origin[1] > 0.0;
                        guard.push(format!("reveal:{valid_origin}:{duration_ms}"));
                    }
                })),
                cancel_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = REVEAL_TRACE.lock() {
                        guard.push("cancel".to_owned());
                    }
                })),
                system_prefers_dark: None,
            };
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                hooks,
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        if let Ok(mut guard) = REVEAL_PATH.lock() {
            *guard = Some(path);
        }
        if let Ok(mut guard) = REVEAL_TRACE.lock() {
            guard.clear();
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestNativeRevealSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: System")?;

        // System (Light) -> Light: no-op, skips capture/reveal and syncs directly.
        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        // Light -> Dark: captures snapshot before switch, then syncs & reveals after dark() flips.
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        // Reset (Dark -> System Light): captures snapshot, then syncs & reveals.
        app.press("Reset to defaults")?;
        app.wait_for("Theme: System")?;
        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        app.press("Strict break (hide Skip)")?;
        app.press("Reset to defaults")?;
        app.settle();
        drop(app);

        let trace = REVEAL_TRACE.lock().map_err(|e| e.to_string())?.clone();
        assert_eq!(
            trace,
            vec![
                "sync:Light",
                "capture",
                "sync:Dark",
                "reveal:true:480",
                "capture",
                "sync:System",
                "reveal:true:480",
                "sync:System",
            ]
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_native_reveal_skips_when_reduce_motion_or_capture_fails()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Environment, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-reveal-skip")?;
        let path_rm = dir.file("reduce-motion.toml");
        let path_fail = dir.file("capture-fail.toml");

        static SKIP_CFG: Mutex<Option<(PathBuf, bool)>> = Mutex::new(None);
        static SKIP_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

        #[component]
        fn TestRevealSkipSettings() -> impl IntoElement {
            let (path, capture_ok) = SKIP_CFG
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let hooks = ThemeTransitionHooks {
                on_theme_change: Some(Arc::new(|mode| {
                    if let Ok(mut guard) = SKIP_TRACE.lock() {
                        guard.push(format!("sync:{mode:?}"));
                    }
                })),
                capture_snapshot: Some(Arc::new(move || {
                    if let Ok(mut guard) = SKIP_TRACE.lock() {
                        guard.push(format!("capture:{capture_ok}"));
                    }
                    capture_ok
                })),
                reveal_snapshot: Some(Arc::new(|_, _| {
                    if let Ok(mut guard) = SKIP_TRACE.lock() {
                        guard.push("reveal".to_owned());
                    }
                })),
                cancel_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = SKIP_TRACE.lock() {
                        guard.push("cancel".to_owned());
                    }
                })),
                system_prefers_dark: None,
            };
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                hooks,
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        // Part A: reduce_motion = true skips capture and reveal.
        if let Ok(mut guard) = SKIP_CFG.lock() {
            *guard = Some((path_rm.clone(), true));
        }
        if let Ok(mut guard) = SKIP_TRACE.lock() {
            guard.clear();
        }
        let env_rm = Environment {
            reduce_motion: true,
            ..Environment::default()
        };
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            environment: env_rm,
            ..EngineConfig::new(Root::of::<TestRevealSkipSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: System")?;
        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        drop(app);

        assert_eq!(
            SKIP_TRACE.lock().map_err(|e| e.to_string())?.clone(),
            vec!["sync:Light", "sync:Dark"]
        );
        assert_eq!(Config::load_from_path(&path_rm)?.theme, ThemeMode::Dark);

        // Part B: capture_snapshot returning false still switches theme and runs on_theme_change.
        if let Ok(mut guard) = SKIP_CFG.lock() {
            *guard = Some((path_fail.clone(), false));
        }
        if let Ok(mut guard) = SKIP_TRACE.lock() {
            guard.clear();
        }
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestRevealSkipSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: System")?;
        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        drop(app);

        assert_eq!(
            SKIP_TRACE.lock().map_err(|e| e.to_string())?.clone(),
            vec!["sync:Light", "capture:false", "sync:Dark"]
        );
        assert_eq!(Config::load_from_path(&path_fail)?.theme, ThemeMode::Dark);
        Ok(())
    }

    #[test]
    fn nsh_headless_native_reveal_uses_system_prefers_dark_and_fallback_cancel()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_core::platform::ColorScheme;
        use nsh_engine::{EngineConfig, Environment, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-reveal-sysdark")?;
        let path_known = dir.file("known.toml");
        let path_unknown = dir.file("unknown.toml");
        let dark_cfg = Config {
            theme: ThemeMode::Dark,
            ..Config::default()
        };
        dark_cfg.save_to_path(&path_known)?;
        dark_cfg.save_to_path(&path_unknown)?;

        static SYS_CFG: Mutex<Option<(PathBuf, Option<bool>)>> = Mutex::new(None);
        static SYS_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

        #[component]
        fn TestSysDarkSettings() -> impl IntoElement {
            let (path, sys_dark) = SYS_CFG
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let hooks = ThemeTransitionHooks {
                on_theme_change: Some(Arc::new(|mode| {
                    if let Ok(mut guard) = SYS_TRACE.lock() {
                        guard.push(format!("sync:{mode:?}"));
                    }
                })),
                capture_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = SYS_TRACE.lock() {
                        guard.push("capture".to_owned());
                    }
                    true
                })),
                reveal_snapshot: Some(Arc::new(|_, _| {
                    if let Ok(mut guard) = SYS_TRACE.lock() {
                        guard.push("reveal".to_owned());
                    }
                })),
                cancel_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = SYS_TRACE.lock() {
                        guard.push("cancel".to_owned());
                    }
                })),
                system_prefers_dark: sys_dark
                    .map(|d| -> SystemPrefersDarkCallback { Arc::new(move || Some(d)) }),
            };
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                hooks,
                SettingsUpdateConfig::default(),
                SettingsStatusConfig::default(),
            )
        }

        // Part A: Dark OS + Dark -> System with system_prefers_dark = Some(true) skips capture.
        if let Ok(mut guard) = SYS_CFG.lock() {
            *guard = Some((path_known, Some(true)));
        }
        if let Ok(mut guard) = SYS_TRACE.lock() {
            guard.clear();
        }
        let dark_env = Environment {
            color_scheme: ColorScheme::Dark,
            ..Environment::default()
        };
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            environment: dark_env,
            ..EngineConfig::new(Root::of::<TestSysDarkSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: Dark")?;
        app.press("Theme: Dark")?;
        app.wait_for("Theme: System")?;
        drop(app);

        assert_eq!(
            SYS_TRACE.lock().map_err(|e| e.to_string())?.clone(),
            vec!["sync:System"]
        );

        // Part B: Dark OS + Dark -> System with unknown system_prefers_dark falls back to cancel.
        if let Ok(mut guard) = SYS_CFG.lock() {
            *guard = Some((path_unknown, None));
        }
        if let Ok(mut guard) = SYS_TRACE.lock() {
            guard.clear();
        }
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            environment: dark_env,
            ..EngineConfig::new(Root::of::<TestSysDarkSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: Dark")?;
        app.press("Theme: Dark")?;
        app.wait_for("Theme: System")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if SYS_TRACE
                .lock()
                .ok()
                .is_some_and(|g| g.iter().any(|entry| entry == "cancel"))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
            app.settle();
        }
        drop(app);

        assert_eq!(
            SYS_TRACE.lock().map_err(|e| e.to_string())?.clone(),
            vec!["capture", "sync:System", "cancel"]
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_rapid_dispatch_discards_stale_capture_and_cancels_pending()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        static RAPID_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

        #[component]
        fn TestRapidDispatch() -> impl IntoElement {
            let reveal = NativeRevealState::default();
            let hooks = ThemeTransitionHooks {
                on_theme_change: Some(Arc::new(|mode| {
                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                        guard.push(format!("sync:{mode:?}"));
                    }
                })),
                capture_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                        guard.push("capture".to_owned());
                    }
                    true
                })),
                reveal_snapshot: Some(Arc::new(|_, _| {
                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                        guard.push("reveal".to_owned());
                    }
                })),
                cancel_snapshot: Some(Arc::new(|| {
                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                        guard.push("cancel".to_owned());
                    }
                })),
                system_prefers_dark: Some(Arc::new(|| Some(false))),
            };

            let r1 = reveal.clone();
            let h1 = hooks.clone();
            let r2 = reveal.clone();
            let h2 = hooks.clone();
            let r3 = reveal;
            let h3 = hooks;

            view()
                .gap_2()
                .child(
                    view()
                        .on_press(move || {
                            // Overlapping async Dark superseded immediately by sync System.
                            dispatch_theme_transition(
                                ThemeMode::Light,
                                ThemeMode::Dark,
                                [100.0, 20.0],
                                &h1,
                                &r1,
                                || {
                                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                                        guard.push("apply:Dark".to_owned());
                                    }
                                },
                            );
                            dispatch_theme_transition(
                                ThemeMode::Dark,
                                ThemeMode::System,
                                [100.0, 20.0],
                                &h1,
                                &r1,
                                || {
                                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                                        guard.push("apply:System".to_owned());
                                    }
                                },
                            );
                        })
                        .child(text("Overlap")),
                )
                .child(
                    view()
                        .on_press(move || {
                            // Capture succeeds without flipping dark(), leaving pending snapshot.
                            dispatch_theme_transition(
                                ThemeMode::Light,
                                ThemeMode::Dark,
                                [100.0, 20.0],
                                &h2,
                                &r2,
                                || {
                                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                                        guard.push("apply:PendingDark".to_owned());
                                    }
                                },
                            );
                        })
                        .child(text("StartPending")),
                )
                .child(
                    view()
                        .on_press(move || {
                            // Sync transition while pending snapshot exists must cancel it.
                            dispatch_theme_transition(
                                ThemeMode::Dark,
                                ThemeMode::System,
                                [100.0, 20.0],
                                &h3,
                                &r3,
                                || {
                                    if let Ok(mut guard) = RAPID_TRACE.lock() {
                                        guard.push("apply:CancelToSystem".to_owned());
                                    }
                                },
                            );
                        })
                        .child(text("SyncOverride")),
                )
        }

        if let Ok(mut guard) = RAPID_TRACE.lock() {
            guard.clear();
        }
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(400.0), Points(200.0)),
            ..EngineConfig::new(Root::of::<TestRapidDispatch>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Overlap")?;

        app.press("Overlap")?;
        app.settle();
        assert_eq!(
            RAPID_TRACE.lock().map_err(|e| e.to_string())?.clone(),
            vec!["apply:System", "sync:System"]
        );

        if let Ok(mut guard) = RAPID_TRACE.lock() {
            guard.clear();
        }
        app.press("StartPending")?;
        app.settle();
        app.press("SyncOverride")?;
        app.settle();
        drop(app);

        assert_eq!(
            RAPID_TRACE.lock().map_err(|e| e.to_string())?.clone(),
            vec![
                "capture",
                "apply:PendingDark",
                "apply:CancelToSystem",
                "sync:System",
                "cancel",
            ]
        );
        Ok(())
    }

    #[test]
    fn format_countdown_pill_covers_all_scheduler_states() {
        let base = StatusSnapshot {
            state: SchedulerState::Waiting,
            enabled: true,
            next_break_type: Some(BreakType::Short),
            next_break_name: Some("Blink".to_owned()),
            seconds_until_next_break: 872,
            consecutive_skips: 0,
            daemon_version: None,
        };
        assert_eq!(
            format_countdown_pill(&base, Locale::En),
            Some(CountdownPillState {
                tone: CountdownTone::Active,
                label: "Short".to_owned(),
                time: Some("14:32".to_owned()),
                accessible_label: "Next short break in 15 minutes".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(&base, Locale::Vi),
            Some(CountdownPillState {
                tone: CountdownTone::Active,
                label: "Nghỉ ngắn".to_owned(),
                time: Some("14:32".to_owned()),
                accessible_label: "Nghỉ ngắn tiếp theo sau 15 phút".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    next_break_type: Some(BreakType::Long),
                    seconds_until_next_break: 4500,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Active,
                label: "Long".to_owned(),
                time: Some("75:00".to_owned()),
                accessible_label: "Next long break in 75 minutes".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    next_break_type: None,
                    seconds_until_next_break: 65,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Active,
                label: "Next".to_owned(),
                time: Some("01:05".to_owned()),
                accessible_label: "Next break in 2 minutes".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    state: SchedulerState::PreBreak,
                    seconds_until_next_break: 8,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Warning,
                label: "Starting".to_owned(),
                time: Some("00:08".to_owned()),
                accessible_label: "Break starting soon".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    state: SchedulerState::Break,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Break,
                label: "Break active".to_owned(),
                time: None,
                accessible_label: "Break active".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    state: SchedulerState::Resting,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Paused,
                label: "Paused (idle)".to_owned(),
                time: None,
                accessible_label: "Break timer paused while idle".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    state: SchedulerState::Stopped,
                    enabled: false,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Disabled,
                label: "Disabled".to_owned(),
                time: None,
                accessible_label: "Break timer disabled".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    enabled: false,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Disabled,
                label: "Disabled".to_owned(),
                time: None,
                accessible_label: "Break timer disabled".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    state: SchedulerState::Start,
                    ..base.clone()
                },
                Locale::En,
            ),
            Some(CountdownPillState {
                tone: CountdownTone::Disabled,
                label: "Initializing".to_owned(),
                time: None,
                accessible_label: "Break timer initializing".to_owned(),
            })
        );
        assert_eq!(
            format_countdown_pill(
                &StatusSnapshot {
                    state: SchedulerState::Quit,
                    ..base
                },
                Locale::En,
            ),
            None
        );
    }

    #[test]
    fn nsh_headless_header_countdown_pill_renders_left_of_theme_and_updates_live()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-countdown-pill")?;
        let path = dir.file("countdown.toml");

        let initial_snapshot = StatusSnapshot {
            state: SchedulerState::Waiting,
            enabled: true,
            next_break_type: Some(BreakType::Short),
            next_break_name: Some("Look away".to_owned()),
            seconds_until_next_break: 872,
            consecutive_skips: 0,
            daemon_version: None,
        };
        let polled_snapshot: Arc<Mutex<Option<StatusSnapshot>>> =
            Arc::new(Mutex::new(Some(StatusSnapshot {
                seconds_until_next_break: 871,
                ..initial_snapshot.clone()
            })));

        static PILL_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        static PILL_INITIAL: Mutex<Option<StatusSnapshot>> = Mutex::new(None);
        static PILL_SOURCE: Mutex<Option<Arc<Mutex<Option<StatusSnapshot>>>>> = Mutex::new(None);

        #[component]
        fn TestCountdownSettings() -> impl IntoElement {
            let path = PILL_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let initial = PILL_INITIAL.lock().ok().and_then(|mut g| g.take());
            let source = PILL_SOURCE.lock().ok().and_then(|mut g| g.take());
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            let on_poll_status = source.map(|src| -> StatusPollCallback {
                Arc::new(move || src.lock().ok().and_then(|g| g.clone()))
            });
            build_settings_view_with_hooks(
                vm,
                sink,
                false,
                None,
                ThemeTransitionHooks::default(),
                SettingsUpdateConfig::default(),
                SettingsStatusConfig {
                    initial_status: initial,
                    on_poll_status,
                },
            )
        }

        if let Ok(mut guard) = PILL_PATH.lock() {
            *guard = Some(path);
        }
        if let Ok(mut guard) = PILL_INITIAL.lock() {
            *guard = Some(initial_snapshot.clone());
        }
        if let Ok(mut guard) = PILL_SOURCE.lock() {
            *guard = Some(Arc::clone(&polled_snapshot));
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestCountdownSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("14:32")?;
        assert!(app.shows("Short"));
        assert!(app.shows("Theme: System"));

        let timer_label = |screen: &nsh_testing::Screen| -> Result<Option<String>, String> {
            let Some(tree) = screen.accessibility(None)? else {
                return Ok(None);
            };
            let mut stack = vec![&tree];
            while let Some(node) = stack.pop() {
                if node.role == Some(AccessibilityRole::Timer) {
                    return Ok(node.label.clone());
                }
                stack.extend(node.children.iter());
            }
            Ok(None)
        };

        assert_eq!(
            timer_label(app.screen())?,
            Some("Next short break in 15 minutes".to_owned())
        );

        let nodes = app.screen().layout_nodes();
        let pill_time_node = nodes
            .iter()
            .find(|n| {
                n.text
                    .as_ref()
                    .is_some_and(|t| t.text.visible_text() == "14:32")
            })
            .ok_or("expected countdown pill time text node")?;
        let theme_node = nodes
            .iter()
            .find(|n| {
                n.text
                    .as_ref()
                    .is_some_and(|t| t.text.visible_text() == "Theme: System")
            })
            .ok_or("expected theme toggle text node")?;
        assert!(
            pill_time_node.bounds.origin.x.0 < theme_node.bounds.origin.x.0,
            "Countdown pill must sit to the left of Theme toggle, got pill={:?}, theme={:?}",
            pill_time_node.bounds,
            theme_node.bounds
        );
        assert!(
            (pill_time_node.bounds.origin.y.0 - theme_node.bounds.origin.y.0).abs() < 2.0,
            "Countdown pill must align vertically with Theme toggle, got pill={:?}, theme={:?}",
            pill_time_node.bounds,
            theme_node.bounds
        );

        app.press("Policies")?;
        app.wait_for("Break Enforcement")?;
        app.wait_for("14:31")?;

        if let Ok(mut guard) = polled_snapshot.lock() {
            *guard = Some(StatusSnapshot {
                state: SchedulerState::Resting,
                ..initial_snapshot
            });
        }
        app.wait_for("Paused (idle)")?;
        assert_eq!(
            timer_label(app.screen())?,
            Some("Break timer paused while idle".to_owned())
        );

        if let Ok(mut guard) = polled_snapshot.lock() {
            *guard = None;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while app.shows("Paused (idle)") && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            app.settle();
        }
        assert!(
            !app.shows("Paused (idle)"),
            "Countdown pill must hide when daemon status becomes unavailable"
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_settings_root_opens_visible_desktop_window_and_quits_on_close()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-settings-root")?;
        let path = dir.file("settings-root.toml");
        let vm = SettingsViewModel::load(path);
        let outcome_sink = Arc::new(Mutex::new(None));
        let ready_calls = Arc::new(AtomicU64::new(0));
        let ready_counter = Arc::clone(&ready_calls);

        {
            let mut guard = SETTINGS_LAUNCH.lock().map_err(|e| e.to_string())?;
            *guard = Some(SettingsLaunch {
                vm,
                outcome_sink,
                on_autostart_change: None,
                theme_hooks: ThemeTransitionHooks::default(),
                update_config: SettingsUpdateConfig::default(),
                status_config: SettingsStatusConfig::default(),
                on_window_ready: Some(Arc::new(move || {
                    ready_counter.fetch_add(1, Ordering::SeqCst);
                })),
            });
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            environment: nsh_core::platform::Environment {
                os: nsh_core::platform::Os::MacOs,
                ..nsh_core::platform::Environment::default()
            },
            ..EngineConfig::new(
                Root::of::<SettingsRoot>().main_window(settings_main_window_options()),
            )
        })
        .map_err(|e| e.to_string())?;

        app.wait_for("Safe Your Eyes")?;
        assert!(
            ready_calls.load(Ordering::SeqCst) >= 1,
            "SettingsRoot must invoke on_window_ready when opening the Settings window"
        );
        let ids = app.window_ids()?;
        assert_eq!(
            ids,
            vec!["settings".to_owned()],
            "SettingsRoot must open an explicit desktop window under nsh 0.7 window model v2"
        );
        let state = app
            .window_state("settings")?
            .ok_or("expected settings window state")?;
        assert!(state.visible, "Settings window must be visible");
        assert!(!app.quit());

        app.press_window_close("settings")?;
        app.settle();
        assert!(
            app.quit(),
            "Closing the Settings window must quit the settings subprocess"
        );
        Ok(())
    }

    #[test]
    fn nsh_headless_language_switcher_translates_live_persists_and_resets()
    -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-language-switch")?;
        let path = dir.file("lang.toml");
        let mut initial_cfg = Config::default();
        initial_cfg
            .short_breaks
            .push(BreakSpec::new("Uống trà xanh"));
        initial_cfg.save_to_path(&path)?;

        static LANG_TEST_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        {
            let mut guard = LANG_TEST_PATH.lock().map_err(|e| e.to_string())?;
            *guard = Some(path.clone());
        }

        #[component]
        fn TestLanguageSettings() -> impl IntoElement {
            let cfg_path = LANG_TEST_PATH
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_default();
            let vm = SettingsViewModel::load_with_locale(cfg_path, Locale::En);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(960.0), Points(720.0)),
            ..EngineConfig::new(Root::of::<TestLanguageSettings>())
        })
        .map_err(|e| e.to_string())?;

        // Initial mode is System resolved against Locale::En -> "Auto" + English labels.
        app.wait_for("Auto")?;
        app.wait_for("Break Schedule")?;
        app.wait_for("Theme: System")?;

        // Cycle System -> En.
        app.press("Auto")?;
        app.wait_for("EN")?;
        assert_eq!(
            Config::load_from_path(&path)?.language,
            sye_core::LanguageMode::En
        );

        // Cycle En -> Vi and verify instant live Vietnamese UI translation.
        app.press("EN")?;
        app.wait_for("VI")?;
        app.wait_for("Lịch nghỉ")?;
        app.wait_for("Giao diện: Hệ thống")?;
        app.wait_for("Mọi thay đổi được tự động lưu")?;
        assert_eq!(
            Config::load_from_path(&path)?.language,
            sye_core::LanguageMode::Vi
        );

        // Switch to Exercises tab and verify default exercises translate while custom stays verbatim.
        app.press("Bài tập")?;
        app.wait_for("Bài tập nghỉ ngắn")?;
        app.wait_for("Nhẹ nhàng nhắm mắt lại")?;
        app.wait_for("Uống trà xanh")?;

        // Reset to defaults restores LanguageMode::System (resolved to Locale::En).
        app.press("Khôi phục mặc định")?;
        app.wait_for("Auto")?;
        app.wait_for("Short Break Exercises")?;
        app.wait_for("Gently close your eyes")?;
        assert_eq!(
            Config::load_from_path(&path)?.language,
            sye_core::LanguageMode::System
        );
        Ok(())
    }
}
