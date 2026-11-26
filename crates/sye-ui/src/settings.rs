use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use nsh_core::desktop::{MainWindowOptions, TitleBarOptions, TitleBarStyle};
use nsh_core::element::desktop::{current_window, quit, title_bar};
use nsh_core::prelude::*;
use sye_core::config::{BreakSpec, BreakType, Config, ThemeMode};
use sye_core::{HOMEBREW_UPGRADE_COMMAND, UpdateInfo, UpdateStrategy};

use crate::UiError;
use crate::theme::{
    DOWNLOAD_PAGE_URL, SUPPORT_URL, appearance_for_theme_mode, colors, motion, next_theme_mode,
    radii, theme_mode_for_appearance,
};
use crate::ui::{
    KpiTone, app_icon, cozy_input, cozy_input_with_disabled, cozy_switch,
    cozy_switch_with_disabled, destructive_button, external_link_button, page_heading,
    pastel_kpi_card, primary_button_with_disabled, secondary_button, section_divider,
    section_header, sidebar_nav_indicator, sidebar_nav_item, stagger_item, status_banner,
    support_link, support_qr_card, theme_toggle_button, version_update_badge,
};

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

    const fn label(self) -> &'static str {
        match self {
            Self::Timing => "Timing",
            Self::Policies => "Policies",
            Self::Exercises => "Exercises",
            Self::About => "About & Support",
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
        let (mut config, load_err) = Config::load_or_default(&config_path);
        if config.strict_break {
            config.limit_consecutive_skips.enabled = false;
        }
        Self {
            config_path,
            config,
            active_tab: SettingsTab::Timing,
            load_warning: load_err.map(|err| err.to_string()),
            validation_error: None,
            outcome: None,
            draft_short_exercise: String::new(),
            draft_long_exercise: String::new(),
            draft_skip_window: String::new(),
            draft_take_window: String::new(),
            system_autostart: None,
        }
    }

    pub fn add_exercise(&mut self, kind: BreakType, name: &str) -> Result<(), String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return self.fail_validation("Exercise name must not be empty");
        }
        self.breaks_mut(kind).push(BreakSpec::new(trimmed));
        self.validation_error = None;
        Ok(())
    }

    pub fn remove_exercise(&mut self, kind: BreakType, index: usize) -> Result<(), String> {
        let total = self.config.short_breaks.len() + self.config.long_breaks.len();
        if total <= 1 {
            return self.fail_validation("At least one break exercise must remain configured");
        }
        let list = self.breaks_mut(kind);
        if index >= list.len() {
            return self.fail_validation(format!("Exercise index {index} out of bounds"));
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

    pub fn validate(&mut self) -> Result<(), String> {
        if let Err(err) = self.config.validate() {
            return self.fail_validation(err.to_string());
        }
        if self.config.short_breaks.is_empty() && self.config.long_breaks.is_empty() {
            return self.fail_validation("At least one break exercise must be configured");
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
            return self.fail_validation(err.to_string());
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
        if index >= self.breaks_mut(kind).len() {
            return self.fail_validation(format!("Exercise index {index} out of bounds"));
        }
        Ok(&mut self.breaks_mut(kind)[index])
    }

    fn add_window_pattern(&mut self, list: WindowList, pattern: &str) -> Result<(), String> {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            let label = list.label();
            return self.fail_validation(format!("{label} window pattern must not be empty"));
        }
        self.window_list_mut(list).push(trimmed.to_owned());
        self.validation_error = None;
        Ok(())
    }

    fn remove_window_pattern(&mut self, list: WindowList, index: usize) -> Result<(), String> {
        let entries = self.window_list_mut(list);
        if index >= entries.len() {
            let label = list.label();
            return self.fail_validation(format!("{label} window index {index} out of bounds"));
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
    const fn label(self) -> &'static str {
        match self {
            Self::Skip => "Skip-break",
            Self::Take => "Take-break",
        }
    }
}

pub type AutostartChangeCallback = Box<dyn Fn(bool) + Send + Sync>;
pub type ThemeChangeCallback = Arc<dyn Fn(ThemeMode) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyUpdateOutcome {
    RestartingDaemon,
    UpdatedStandalone,
}

pub type CheckUpdatesCallback = Arc<dyn Fn() -> Result<Option<UpdateInfo>, String> + Send + Sync>;
pub type ApplyUpdateCallback =
    Arc<dyn Fn(&str) -> Result<ApplyUpdateOutcome, String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct SettingsUpdateConfig {
    pub initial_update: Option<UpdateInfo>,
    pub strategy: Option<UpdateStrategy>,
    pub on_check_now: Option<CheckUpdatesCallback>,
    pub on_apply_update: Option<ApplyUpdateCallback>,
}

type AutostartHook = Arc<dyn Fn(bool) + Send + Sync>;

struct SettingsLaunch {
    vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    on_autostart_change: Option<AutostartHook>,
    on_theme_change: Option<ThemeChangeCallback>,
    update_config: SettingsUpdateConfig,
}

static SETTINGS_LAUNCH: Mutex<Option<SettingsLaunch>> = Mutex::new(None);

fn load_settings_vm(config_path: PathBuf, system_autostart: Option<bool>) -> SettingsViewModel {
    let mut vm = SettingsViewModel::load(config_path);
    vm.system_autostart = system_autostart;
    if let Some(actual) = system_autostart {
        vm.config.autostart = actual;
    }
    vm
}

fn number_field_row(
    label: &'static str,
    unit: &'static str,
    read: impl Fn() -> u64 + 'static,
    write: impl Fn(u64) + 'static,
) -> impl IntoElement {
    number_field_row_with_disabled(label, unit, read, || false, write)
}

fn number_field_row_with_disabled(
    label: &'static str,
    unit: &'static str,
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
                        "Break Schedule",
                        "Configure short and long break cadence and durations.",
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    number_field_row(
                        "Short break interval",
                        "secs",
                        move || vm.with(|m| m.config.short_break_interval_secs),
                        move |v| update_and_autosave(vm, &s1, |m| m.config.short_break_interval_secs = v),
                    ),
                ))
                .child(stagger_item(
                    2,
                    animate,
                    number_field_row(
                        "Short break duration",
                        "secs",
                        move || vm.with(|m| m.config.short_break_duration_secs),
                        move |v| update_and_autosave(vm, &s2, |m| m.config.short_break_duration_secs = v),
                    ),
                ))
                .child(stagger_item(
                    3,
                    animate,
                    number_field_row(
                        "Long break interval",
                        "secs",
                        move || vm.with(|m| m.config.long_break_interval_secs),
                        move |v| update_and_autosave(vm, &s3, |m| m.config.long_break_interval_secs = v),
                    ),
                ))
                .child(stagger_item(
                    4,
                    animate,
                    number_field_row(
                        "Long break duration",
                        "secs",
                        move || vm.with(|m| m.config.long_break_duration_secs),
                        move |v| update_and_autosave(vm, &s4, |m| m.config.long_break_duration_secs = v),
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
                        "Transitions & Controls",
                        "Fine-tune warnings, fade-in speed, postpone length, session persistence, and OS startup.",
                    ),
                ))
                .child(stagger_item(
                    7,
                    animate,
                    number_field_row(
                        "Pre-break warning",
                        "secs",
                        move || vm.with(|m| m.config.pre_break_warning_secs),
                        move |v| update_and_autosave(vm, &s5, |m| m.config.pre_break_warning_secs = v),
                    ),
                ))
                .child(stagger_item(
                    8,
                    animate,
                    number_field_row(
                        "Fade-in duration",
                        "ms",
                        move || vm.with(|m| m.config.fade_in_ms),
                        move |v| update_and_autosave(vm, &s6, |m| m.config.fade_in_ms = v),
                    ),
                ))
                .child(stagger_item(
                    9,
                    animate,
                    number_field_row(
                        "Shortcut unlock delay",
                        "secs",
                        move || vm.with(|m| m.config.shortcut_disable_secs),
                        move |v| update_and_autosave(vm, &s7, |m| m.config.shortcut_disable_secs = v),
                    ),
                ))
                .child(stagger_item(
                    10,
                    animate,
                    number_field_row(
                        "Postpone duration",
                        "secs",
                        move || vm.with(|m| m.config.postpone_duration_secs),
                        move |v| update_and_autosave(vm, &s8, |m| m.config.postpone_duration_secs = v),
                    ),
                ))
                .child(stagger_item(
                    11,
                    animate,
                    cozy_switch(
                        "Randomize exercise order",
                        move || vm.with(|m| m.config.random_order),
                        move |v| update_and_autosave(vm, &s9, |m| m.config.random_order = v),
                    ),
                ))
                .child(stagger_item(
                    12,
                    animate,
                    cozy_switch(
                        "Persist session across restarts",
                        move || vm.with(|m| m.config.persist_state),
                        move |v| update_and_autosave(vm, &s10, |m| m.config.persist_state = v),
                    ),
                ))
                .child(stagger_item(
                    13,
                    animate,
                    cozy_switch(
                        "Launch at login (startup)",
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
                        "Break Enforcement",
                        "Control whether breaks can be skipped, postponed, or strictly enforced.",
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    cozy_switch(
                        "Strict break (hide Skip)",
                        move || vm.with(|m| m.config.strict_break),
                        move |v| update_and_autosave(vm, &s1, |m| m.set_strict_break(v)),
                    ),
                ))
                .child(stagger_item(
                    2,
                    animate,
                    cozy_switch(
                        "Allow postpone (Space)",
                        move || vm.with(|m| m.config.allow_postpone),
                        move |v| update_and_autosave(vm, &s2, |m| m.config.allow_postpone = v),
                    ),
                ))
                .child(stagger_item(
                    3,
                    animate,
                    cozy_switch_with_disabled(
                        "Limit consecutive skips",
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
                        "Max consecutive skips",
                        "skips",
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
                        "Smart Pause & Do Not Disturb",
                        "Automatically pause timers while idle, on battery, or in fullscreen apps.",
                    ),
                ))
                .child(stagger_item(
                    7,
                    animate,
                    cozy_switch(
                        "Smart Pause on idle",
                        move || vm.with(|m| m.config.smart_pause.enabled),
                        move |v| update_and_autosave(vm, &s5, |m| m.config.smart_pause.enabled = v),
                    ),
                ))
                .child(stagger_item(
                    8,
                    animate,
                    number_field_row(
                        "Idle threshold",
                        "secs",
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
                        "Do Not Disturb in fullscreen",
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
                        "Pause while on battery",
                        move || vm.with(|m| m.config.do_not_disturb.while_on_battery),
                        move |v| {
                            update_and_autosave(vm, &s8, |m| {
                                m.config.do_not_disturb.while_on_battery = v;
                            });
                        },
                    ),
                )),
        )
        .child(stagger_item(11, animate, section_divider()))
        .child(
            view()
                .gap_3()
                .child(stagger_item(
                    12,
                    animate,
                    section_header(
                        "Audible Chime",
                        "Play a gentle synthesized chime before or after breaks.",
                    ),
                ))
                .child(stagger_item(
                    13,
                    animate,
                    cozy_switch(
                        "Pre-break chime",
                        move || vm.with(|m| m.config.audible_alert.pre_break),
                        move |v| {
                            update_and_autosave(vm, &s9, |m| m.config.audible_alert.pre_break = v);
                        },
                    ),
                ))
                .child(stagger_item(
                    14,
                    animate,
                    cozy_switch(
                        "Post-break chime",
                        move || vm.with(|m| m.config.audible_alert.post_break),
                        move |v| {
                            update_and_autosave(vm, &s10, |m| {
                                m.config.audible_alert.post_break = v;
                            });
                        },
                    ),
                ))
                .child(stagger_item(
                    15,
                    animate,
                    number_field_row(
                        "Chime volume",
                        "0–100",
                        move || u64::from(vm.with(|m| m.config.audible_alert.volume)),
                        move |v| {
                            update_and_autosave(vm, &s11, |m| {
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
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    kind: BreakType,
    base_index: usize,
    animate: bool,
) -> impl IntoElement {
    let (title, caption, placeholder, key_prefix) = match kind {
        BreakType::Short => (
            "Short Break Exercises",
            "Quick 15-second prompts cycled during work intervals.",
            "Add a short exercise...",
            "short",
        ),
        BreakType::Long => (
            "Long Break Exercises",
            "Restorative prompts shown after every four short breaks.",
            "Add a long exercise...",
            "long",
        ),
    };
    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;
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
            section_header(title, caption),
        ))
        .child(stagger_item(
            base_index + 1,
            animate,
            view()
                .flex_row()
                .gap_2()
                .items_center()
                .child(view().flex_1().child(cozy_input(
                    placeholder,
                    move || vm.with(|m| m.draft_exercise(kind).to_owned()),
                    move |s: &str| vm.update(|m| *m.draft_exercise_mut(kind) = s.to_owned()),
                )))
                .child(secondary_button("Add", move || {
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
                })),
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
                        .transition_colors()
                        .duration(motion::THEME_COLOR_MS)
                        .ease_bezier(bx1, by1, bx2, by2)
                        .px_3()
                        .py_2()
                        .child(
                            text(move || item.get())
                                .text_sm()
                                .text_color(colors::TEXT_PRIMARY),
                        )
                        .child(destructive_button("Remove", move || {
                            let idx = row_idx.get_untracked();
                            let mut saved = false;
                            vm.update(|m| {
                                if m.remove_exercise(kind, idx).is_ok() && m.autosave().is_ok() {
                                    saved = true;
                                }
                            });
                            if saved {
                                crate::store_outcome(&remove_sink, SettingsOutcome::Saved);
                            }
                        })),
                )
            },
        )))
}

fn exercises_section(
    vm: Signal<SettingsViewModel>,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    animate: bool,
) -> impl IntoElement {
    let short_count = vm.with(|m| m.config.short_breaks.len());
    let divider_idx = 2 + short_count;
    view()
        .gap_5()
        .child(exercise_tier_section(
            vm,
            Arc::clone(&outcome_sink),
            BreakType::Short,
            0,
            animate,
        ))
        .child(stagger_item(divider_idx, animate, section_divider()))
        .child(exercise_tier_section(
            vm,
            outcome_sink,
            BreakType::Long,
            divider_idx + 1,
            animate,
        ))
}

enum UpdateWorkerResult {
    Check(Result<Option<UpdateInfo>, String>),
    Apply(Result<ApplyUpdateOutcome, String>),
}

#[derive(Clone, Copy)]
struct UpdateUiState {
    available: Signal<Option<UpdateInfo>>,
    busy: Signal<bool>,
    applying: Signal<bool>,
    status_text: Signal<Option<String>>,
    is_error: Signal<bool>,
    strategy: UpdateStrategy,
}

fn poll_update_worker(
    rx: Rc<mpsc::Receiver<UpdateWorkerResult>>,
    state: UpdateUiState,
    quit_on_restart: bool,
) {
    match rx.try_recv() {
        Ok(UpdateWorkerResult::Check(Ok(Some(info)))) => {
            state.status_text.set(Some(format!(
                "Version v{} is available.",
                info.latest_version
            )));
            state.is_error.set(false);
            state.available.set(Some(info));
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Check(Ok(None))) => {
            state.available.set(None);
            state.status_text.set(Some(format!(
                "You are up to date (v{}).",
                env!("CARGO_PKG_VERSION")
            )));
            state.is_error.set(false);
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Check(Err(err))) => {
            state
                .status_text
                .set(Some(format!("Update check failed: {err}")));
            state.is_error.set(true);
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Apply(Ok(ApplyUpdateOutcome::RestartingDaemon))) => {
            state.status_text.set(Some(
                "Update verified. Restarting Safe Your Eyes...".to_owned(),
            ));
            state.is_error.set(false);
            state.available.set(None);
            state.busy.set(false);
            state.applying.set(false);
            if quit_on_restart {
                quit();
            }
        }
        Ok(UpdateWorkerResult::Apply(Ok(ApplyUpdateOutcome::UpdatedStandalone))) => {
            state.status_text.set(Some(
                "Update installed and verified. Restart Safe Your Eyes to use the new version."
                    .to_owned(),
            ));
            state.is_error.set(false);
            state.available.set(None);
            state.busy.set(false);
            state.applying.set(false);
        }
        Ok(UpdateWorkerResult::Apply(Err(err))) => {
            state.status_text.set(Some(format!(
                "Self-update failed: {err}. Use Download Page instead."
            )));
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
                .status_text
                .set(Some("Update task stopped unexpectedly.".to_owned()));
            state.is_error.set(true);
            state.busy.set(false);
            state.applying.set(false);
        }
    }
}

fn about_section(
    vm: Signal<SettingsViewModel>,
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
                        "Updates",
                        "Checks GitHub Releases for newer versions and verifies SHA-256 before installing.",
                    ),
                ))
                .child(stagger_item(
                    1,
                    animate,
                    text(move || {
                        if let Some(info) = update_ui.available.get() {
                            format!(
                                "Current: v{}  ->  Latest: v{}",
                                env!("CARGO_PKG_VERSION"),
                                info.latest_version
                            )
                        } else {
                            format!("Current version: v{}", env!("CARGO_PKG_VERSION"))
                        }
                    })
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors::TEXT_PRIMARY),
                ))
                .child(when(
                    move || update_ui.status_text.get().is_some() && !update_ui.is_error.get(),
                    move || {
                        status_banner(false, move || {
                            update_ui.status_text.get().unwrap_or_default()
                        })
                    },
                ))
                .child(when(
                    move || update_ui.status_text.get().is_some() && update_ui.is_error.get(),
                    move || {
                        status_banner(true, move || {
                            update_ui.status_text.get().unwrap_or_default()
                        })
                    },
                ))
                .child(when(
                    move || is_homebrew && update_ui.available.get().is_some(),
                    || {
                        text(format!("Homebrew upgrade: {HOMEBREW_UPGRADE_COMMAND}"))
                            .text_xs()
                            .text_color(colors::TEXT_SECONDARY)
                    },
                ))
                .child(stagger_item(
                    2,
                    animate,
                    cozy_switch(
                        "Check for updates automatically",
                        move || vm.with(|m| m.config.check_for_updates),
                        move |v| update_and_autosave(vm, &outcome_sink, |m| m.config.check_for_updates = v),
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
                                        if update_ui.applying.get() {
                                            "Updating..."
                                        } else {
                                            "Update & Restart"
                                        }
                                    },
                                    || false,
                                    move || {
                                        if update_ui.busy.get_untracked() {
                                            return;
                                        }
                                        let Some(info) = update_ui.available.get_untracked() else {
                                            return;
                                        };
                                        let Some(cb) = apply.clone() else {
                                            return;
                                        };
                                        update_ui.busy.set(true);
                                        update_ui.applying.set(true);
                                        update_ui.is_error.set(false);
                                        update_ui.status_text.set(Some(format!(
                                            "Downloading and verifying v{}...",
                                            info.latest_version
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
                                    },
                                )
                            },
                        ))
                        .child(when(
                            move || update_ui.available.get().is_some(),
                            move || {
                                let label = if can_self_update {
                                    "Download Page"
                                } else {
                                    "Download Update"
                                };
                                external_link_button(label, DOWNLOAD_PAGE_URL, !can_self_update)
                            },
                        ))
                        .child(secondary_button(
                            move || {
                                if update_ui.busy.get() && !update_ui.applying.get() {
                                    "Checking..."
                                } else {
                                    "Check for Updates"
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
                                    .status_text
                                    .set(Some("Checking for updates...".to_owned()));
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
        .child(
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
                            5,
                            animate,
                            section_header(
                                "Support Safe Your Eyes",
                                "Built in Rust with Native Shell (nsh) and the Cozy Design System for minimal footprint.",
                            ),
                        ))
                        .child(stagger_item(
                            6,
                            animate,
                            text(format!("Support link: {SUPPORT_URL}"))
                                .text_xs()
                                .text_color(colors::TEXT_SECONDARY),
                        ))
                        .child(stagger_item(
                            7,
                            animate,
                            view().flex_row().child(support_link("Buy Me a Coffee")),
                        )),
                )
                .child(stagger_item(8, animate, support_qr_card())),
        )
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
        None,
        SettingsUpdateConfig::default(),
    )
}

pub(crate) fn build_settings_view_with_hooks(
    initial_vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    quit_on_finish: bool,
    on_autostart_change: Option<AutostartHook>,
    on_theme_change: Option<ThemeChangeCallback>,
    update_config: SettingsUpdateConfig,
) -> impl IntoElement {
    if quit_on_finish {
        let main_win = current_window();
        main_win.focus();
        let focus_win = main_win;
        drop(set_timeout(Duration::from_millis(60), move || {
            focus_win.focus();
        }));
    }

    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;
    let initial_appearance = appearance_for_theme_mode(initial_vm.config.theme);
    let vm = signal(initial_vm);
    let appearance = signal(initial_appearance);
    let has_switched_tab = signal(false);

    let update_ui = UpdateUiState {
        available: signal(update_config.initial_update),
        busy: signal(false),
        applying: signal(false),
        status_text: signal(None),
        is_error: signal(false),
        strategy: update_config
            .strategy
            .unwrap_or(UpdateStrategy::ManualDownload),
    };
    let on_check_now = update_config.on_check_now;
    let on_apply_update = update_config.on_apply_update;

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

    let theme_sink = Arc::clone(&outcome_sink);
    let timing_sink = Arc::clone(&outcome_sink);
    let policies_sink = Arc::clone(&outcome_sink);
    let exercises_sink = Arc::clone(&outcome_sink);
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
            tab.label(),
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

    let sidebar = view()
        .w(px(210.0))
        .flex_shrink_0()
        .rounded(radii::SIDEBAR)
        .bg(colors::SIDEBAR_BG)
        .border_1()
        .border_color(colors::BORDER)
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
        .p_3()
        .justify_between()
        .child(
            view()
                .gap_2()
                .child(when(
                    move || title_bar().left.0 > 0.0,
                    || view().h(px(26.0)).window_drag(true),
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
                                    text("Cozy Eye Care")
                                        .text_xs()
                                        .text_color(colors::SIDEBAR_TEXT_MUTED),
                                ),
                        ),
                )
                .child(sidebar_nav),
        )
        .child(
            view()
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
                )
                .child(when(
                    move || update_ui.available.get().is_some(),
                    move || {
                        version_update_badge(
                            move || {
                                update_ui.available.get().map_or_else(String::new, |info| {
                                    format!("↑ v{}", info.latest_version)
                                })
                            },
                            move || {
                                vm.update(|m| {
                                    if m.active_tab != SettingsTab::About {
                                        has_switched_tab.set(true);
                                        m.active_tab = SettingsTab::About;
                                    }
                                });
                            },
                        )
                    },
                )),
        );

    let kpi_row = view()
        .flex_row()
        .gap_3()
        .child(pastel_kpi_card(KpiTone::Sage, "Short Break", move || {
            vm.with(|m| {
                format!(
                    "{}m / {}s",
                    m.config.short_break_interval_secs / 60,
                    m.config.short_break_duration_secs
                )
            })
        }))
        .child(pastel_kpi_card(KpiTone::Peach, "Long Break", move || {
            vm.with(|m| {
                format!(
                    "{}m / {}s",
                    m.config.long_break_interval_secs / 60,
                    m.config.long_break_duration_secs
                )
            })
        }))
        .child(pastel_kpi_card(KpiTone::Butter, "Exercises", move || {
            vm.with(|m| (m.config.short_breaks.len() + m.config.long_breaks.len()).to_string())
        }))
        .child(pastel_kpi_card(KpiTone::Mist, "Skip Limit", move || {
            vm.with(|m| {
                if m.config.strict_break {
                    "Strict".to_owned()
                } else if m.config.limit_consecutive_skips.enabled {
                    format!("{} max", m.config.limit_consecutive_skips.max_skips)
                } else {
                    "Off".to_owned()
                }
            })
        }));

    let toggle_theme_hook = on_theme_change.as_ref().map(Arc::clone);
    let reset_theme_hook = on_theme_change;

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
                    move || active_tab.get().label(),
                    &highlight_scale,
                ))
                .child(theme_toggle_button(
                    move || theme_mode_for_appearance(appearance.get()),
                    move || {
                        let next_mode =
                            next_theme_mode(theme_mode_for_appearance(appearance.get()));
                        let next_appearance = appearance_for_theme_mode(next_mode);
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
                            crate::store_outcome(&theme_sink, SettingsOutcome::Saved);
                        }
                        appearance.set(next_appearance);
                        set_color_scheme(next_appearance);
                        if let Some(cb) = &toggle_theme_hook {
                            cb(next_mode);
                        }
                    },
                )),
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
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
        .child(
            view()
                .p_5()
                .bg(colors::BACKGROUND_SURFACE)
                .transition_colors()
                .duration(motion::THEME_COLOR_MS)
                .ease_bezier(bx1, by1, bx2, by2)
                .child(when(
                    move || active_tab.get() == SettingsTab::Timing,
                    move || {
                        timing_section(
                            vm,
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
                            Arc::clone(&exercises_sink),
                            has_switched_tab.get_untracked(),
                        )
                    },
                ))
                .child(when(
                    move || active_tab.get() == SettingsTab::About,
                    move || {
                        about_section(
                            vm,
                            Arc::clone(&about_sink),
                            update_ui,
                            on_check_now.clone(),
                            on_apply_update.clone(),
                            quit_on_finish,
                            has_switched_tab.get_untracked(),
                        )
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
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
        .child(
            text("Changes are saved automatically")
                .text_xs()
                .text_color(colors::TEXT_SECONDARY),
        )
        .child(primary_button_with_disabled(
            "Reset to defaults",
            move || !is_modified.get(),
            move || {
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
                    crate::store_outcome(&reset_sink, SettingsOutcome::Saved);
                }
                if saved && let (Some(next), Some(cb)) = (autostart_changed, &reset_hook) {
                    cb(next);
                }
                let default_mode = ThemeMode::default();
                let default_appearance = appearance_for_theme_mode(default_mode);
                appearance.set(default_appearance);
                set_color_scheme(default_appearance);
                if let Some(cb) = &reset_theme_hook {
                    cb(default_mode);
                }
            },
        ));

    let form_card = view()
        .flex_1()
        .rounded(radii::CONTAINER)
        .bg(colors::BACKGROUND_SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
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

    view()
        .flex_1()
        .bg(colors::BACKGROUND_BODY)
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
        .flex_row()
        .p_2()
        .gap_3()
        .child(sidebar)
        .child(right_pane)
}

#[component]
fn SettingsRoot() -> impl IntoElement {
    let launch = SETTINGS_LAUNCH.lock().ok().and_then(|mut g| g.take());
    match launch {
        Some(launch) => build_settings_view_with_hooks(
            launch.vm,
            launch.outcome_sink,
            true,
            launch.on_autostart_change,
            launch.on_theme_change,
            launch.update_config,
        )
        .into_element(),
        None => view()
            .flex_1()
            .bg(colors::BACKGROUND_BODY)
            .items_center()
            .justify_center()
            .child(text("Safe Your Eyes — Settings").text_color(colors::TEXT_PRIMARY))
            .into_element(),
    }
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
        None,
        SettingsUpdateConfig::default(),
    )
}

pub fn run_settings_with_hooks(
    config_path: PathBuf,
    system_autostart: Option<bool>,
    on_autostart_change: Option<AutostartChangeCallback>,
    on_theme_change: Option<ThemeChangeCallback>,
    update_config: SettingsUpdateConfig,
) -> Result<SettingsOutcome, UiError> {
    let vm = load_settings_vm(config_path, system_autostart);
    let initial_appearance = appearance_for_theme_mode(vm.config.theme);
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
        on_theme_change,
        update_config,
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
                title: "Safe Your Eyes — Settings".to_owned(),
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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
        vm.config.autostart = true;
        vm.config.smart_pause.enabled = false;
        vm.config.smart_pause.idle_threshold_secs = 30;
        vm.config.smart_pause.postpone_if_active = true;
        vm.config.do_not_disturb.enabled = false;
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
        let saw_shader_transition = Arc::new(AtomicBool::new(false));
        let shader_flag = Arc::clone(&saw_shader_transition);

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestPersistSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.observe(move |_step, batch| {
            for item in &batch.items {
                if matches!(item, MountItem::ShaderTransition { .. }) {
                    shader_flag.store(true, Ordering::Release);
                }
            }
        });
        app.wait_for("Theme: System")?;
        assert!(!app.shows("Save"));
        assert!(!app.shows("Cancel"));
        assert!(app.shows("Reset to defaults"));

        assert!(app.press("Reset to defaults").is_err());
        assert!(!save_path.exists());

        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        assert!(
            !saw_shader_transition.load(Ordering::Acquire),
            "Theme toggle must not trigger flashing ShaderTransition"
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

        let mut vm = load_settings_vm(path.clone(), Some(true));
        vm.config.short_break_interval_secs = 123;
        vm.config.theme = ThemeMode::Dark;
        vm.persist_theme()?;

        let saved = Config::load_from_path(&path)?;
        assert_eq!(saved.theme, ThemeMode::Dark);
        assert!(saved.autostart);
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

        let unchanged = load_settings_vm(path.clone(), None);
        assert!(!unchanged.config.autostart);

        let overridden_true = load_settings_vm(path.clone(), Some(true));
        assert!(overridden_true.config.autostart);

        let cfg_true = Config {
            autostart: true,
            ..Config::default()
        };
        cfg_true.save_to_path(&path)?;
        let overridden_false = load_settings_vm(path, Some(false));
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
            autostart: false,
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
            let vm = load_settings_vm(path, Some(true));
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
                None,
                SettingsUpdateConfig::default(),
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
        assert_eq!(events, vec![false, true, false, true]);

        let reloaded = Config::load_from_path(&path)?;
        assert!(reloaded.autostart);
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
            let vm = load_settings_vm(path, Some(false));
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
                None,
                SettingsUpdateConfig::default(),
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

    #[test]
    fn nsh_headless_update_badge_and_controls_flow() -> Result<(), Box<dyn std::error::Error>> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        let dir = RepoTempDir::new("headless-updates")?;
        let config_path = dir.file("updates.toml");
        static UPDATE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
        if let Ok(mut guard) = UPDATE_PATH.lock() {
            *guard = Some(config_path.clone());
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
                on_apply_update: Some(Arc::new(|_| Ok(ApplyUpdateOutcome::UpdatedStandalone))),
            };
            build_settings_view_with_hooks(vm, sink, false, None, None, update_cfg)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(920.0), Points(680.0)),
            ..EngineConfig::new(Root::of::<TestUpdateSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("↑ v9.9.9")?;
        assert!(app.shows(concat!("v", env!("CARGO_PKG_VERSION"))));

        app.press("↑ v9.9.9")?;
        app.wait_for("Updates")?;
        assert!(app.shows("Update & Restart"));
        assert!(app.shows("Download Page"));
        assert!(app.shows("Check for Updates"));
        assert!(app.shows("Check for updates automatically"));

        app.press("Check for Updates")?;
        app.wait_for("↑ v9.9.10")?;
        assert!(app.shows("Version v9.9.10 is available."));

        app.press("Check for updates automatically")?;
        drop(app);

        let reloaded = Config::load_from_path(&config_path)?;
        assert!(!reloaded.check_for_updates);
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
                Some(theme_hook),
                SettingsUpdateConfig::default(),
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
}
