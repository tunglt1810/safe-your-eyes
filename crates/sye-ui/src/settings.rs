//! Cozy Settings window view model and Native Shell (`nsh`) renderer.
//!
//! Applies the Cozy Design System (`DESIGN.md`): Floating Sage Sidebar, Pastel KPI
//! summary cards, Light/Dark/System theme mode switching, warm sticky toolbar,
//! and author support link (`https://buymeacoffee.com/bbeeezzzzz`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nsh_core::element::desktop::quit;
use nsh_core::prelude::*;
use sye_core::config::{BreakSpec, BreakType, Config};

use crate::UiError;
use crate::theme::{SUPPORT_URL, colors, radii};
use crate::ui::{
    KpiTone, app_icon, cozy_input, cozy_switch, destructive_button, ghost_button, page_heading,
    pastel_kpi_card, primary_button, secondary_button, section_divider, section_header,
    segmented_option, segmented_track, sidebar_nav_item, status_banner, support_link,
    support_qr_card,
};

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

/// Active section in the Cozy settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Timing,
    Policies,
    Exercises,
    About,
}

/// Terminal outcome returned by the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOutcome {
    Saved,
    Cancelled,
}

/// Pure, headless view model managing configuration editing, validation, and persistence.
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
}

impl SettingsViewModel {
    /// Loads configuration from `config_path`, falling back to defaults with a non-fatal warning
    /// if the existing file is unreadable or invalid.
    pub fn load(config_path: PathBuf) -> Self {
        let (config, load_err) = Config::load_or_default(&config_path);
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
        }
    }

    /// Appends a new break exercise to the chosen break tier.
    pub fn add_exercise(&mut self, kind: BreakType, name: impl Into<String>) -> Result<(), String> {
        let raw = name.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let msg = "Exercise name must not be empty".to_owned();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.breaks_mut(kind).push(BreakSpec::new(trimmed));
        self.validation_error = None;
        Ok(())
    }

    /// Removes the break exercise at `index`, refusing to remove the last remaining exercise
    /// across both short and long tiers.
    pub fn remove_exercise(&mut self, kind: BreakType, index: usize) -> Result<(), String> {
        let total = self.config.short_breaks.len() + self.config.long_breaks.len();
        if total <= 1 {
            let msg = "At least one break exercise must remain configured".to_owned();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        let list = self.breaks_mut(kind);
        if index >= list.len() {
            let msg = format!("Exercise index {index} out of bounds");
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        list.remove(index);
        self.validation_error = None;
        Ok(())
    }

    /// Updates the optional per-exercise interval override.
    pub fn set_exercise_interval_override(
        &mut self,
        kind: BreakType,
        index: usize,
        interval_secs: Option<u64>,
    ) -> Result<(), String> {
        let list = self.breaks_mut(kind);
        let Some(spec) = list.get_mut(index) else {
            let msg = format!("Exercise index {index} out of bounds");
            self.validation_error = Some(msg.clone());
            return Err(msg);
        };
        spec.interval_secs = interval_secs;
        self.validation_error = None;
        Ok(())
    }

    /// Updates the optional per-exercise duration override.
    pub fn set_exercise_duration_override(
        &mut self,
        kind: BreakType,
        index: usize,
        duration_secs: Option<u64>,
    ) -> Result<(), String> {
        let list = self.breaks_mut(kind);
        let Some(spec) = list.get_mut(index) else {
            let msg = format!("Exercise index {index} out of bounds");
            self.validation_error = Some(msg.clone());
            return Err(msg);
        };
        spec.duration_secs = duration_secs;
        self.validation_error = None;
        Ok(())
    }

    /// Adds a non-empty window pattern to `do_not_disturb.skip_break_windows`.
    pub fn add_skip_window(&mut self, pattern: impl Into<String>) -> Result<(), String> {
        let raw = pattern.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let msg = "Skip-break window pattern must not be empty".to_owned();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.config
            .do_not_disturb
            .skip_break_windows
            .push(trimmed.to_owned());
        self.validation_error = None;
        Ok(())
    }

    /// Removes the pattern at `index` from `do_not_disturb.skip_break_windows`.
    pub fn remove_skip_window(&mut self, index: usize) -> Result<(), String> {
        if index >= self.config.do_not_disturb.skip_break_windows.len() {
            let msg = format!("Skip-break window index {index} out of bounds");
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.config.do_not_disturb.skip_break_windows.remove(index);
        self.validation_error = None;
        Ok(())
    }

    /// Adds a non-empty window pattern to `do_not_disturb.take_break_windows`.
    pub fn add_take_window(&mut self, pattern: impl Into<String>) -> Result<(), String> {
        let raw = pattern.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let msg = "Take-break window pattern must not be empty".to_owned();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.config
            .do_not_disturb
            .take_break_windows
            .push(trimmed.to_owned());
        self.validation_error = None;
        Ok(())
    }

    /// Removes the pattern at `index` from `do_not_disturb.take_break_windows`.
    pub fn remove_take_window(&mut self, index: usize) -> Result<(), String> {
        if index >= self.config.do_not_disturb.take_break_windows.len() {
            let msg = format!("Take-break window index {index} out of bounds");
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.config.do_not_disturb.take_break_windows.remove(index);
        self.validation_error = None;
        Ok(())
    }

    /// Validates the current in-memory configuration and updates `validation_error`.
    pub fn validate(&mut self) -> Result<(), String> {
        if let Err(err) = self.config.validate() {
            let msg = err.to_string();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        if self.config.short_breaks.is_empty() && self.config.long_breaks.is_empty() {
            let msg = "At least one break exercise must be configured".to_owned();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.validation_error = None;
        Ok(())
    }

    /// Resets the in-memory configuration and drafts to factory defaults without writing to disk.
    pub fn reset_to_defaults(&mut self) {
        self.config = Config::reset_to_default();
        self.validation_error = None;
        self.load_warning = None;
        self.draft_short_exercise.clear();
        self.draft_long_exercise.clear();
        self.draft_skip_window.clear();
        self.draft_take_window.clear();
    }

    /// Validates and persists the configuration to `config_path`.
    pub fn save(&mut self) -> Result<SettingsOutcome, String> {
        self.validate()?;
        if let Err(err) = self.config.save_to_path(&self.config_path) {
            let msg = err.to_string();
            self.validation_error = Some(msg.clone());
            return Err(msg);
        }
        self.validation_error = None;
        self.outcome = Some(SettingsOutcome::Saved);
        Ok(SettingsOutcome::Saved)
    }

    /// Cancels editing without writing to disk.
    pub fn cancel(&mut self) -> SettingsOutcome {
        self.outcome = Some(SettingsOutcome::Cancelled);
        SettingsOutcome::Cancelled
    }

    /// Returns `true` when the settings window should close.
    pub fn should_close(&self) -> bool {
        self.outcome.is_some()
    }

    /// Returns the latched outcome, defaulting to `SettingsOutcome::Cancelled`.
    pub fn resolved_outcome(&self) -> SettingsOutcome {
        self.outcome.unwrap_or(SettingsOutcome::Cancelled)
    }

    fn breaks_mut(&mut self, kind: BreakType) -> &mut Vec<BreakSpec> {
        match kind {
            BreakType::Short => &mut self.config.short_breaks,
            BreakType::Long => &mut self.config.long_breaks,
        }
    }
}

struct SettingsLaunch {
    vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
}

static SETTINGS_LAUNCH: Mutex<Option<SettingsLaunch>> = Mutex::new(None);

fn store_outcome(sink: &Arc<Mutex<Option<SettingsOutcome>>>, outcome: SettingsOutcome) {
    if let Ok(mut guard) = sink.lock()
        && guard.is_none()
    {
        *guard = Some(outcome);
    }
}

fn number_field_row(
    label: &'static str,
    unit: &'static str,
    read: impl Fn() -> u64 + 'static,
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
                .text_color(colors::TEXT_PRIMARY),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(view().w(px(96.0)).child(cozy_input(
                    "0",
                    move || read().to_string(),
                    move |raw: &str| {
                        if let Ok(parsed) = raw.trim().parse::<u64>() {
                            write(parsed);
                        }
                    },
                )))
                .child(
                    view().w(px(44.0)).child(
                        text(unit)
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors::TEXT_SECONDARY),
                    ),
                ),
        )
}

fn timing_section(vm: Signal<SettingsViewModel>) -> impl IntoElement {
    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Break Schedule",
                    "Configure short and long break cadence and durations.",
                ))
                .child(number_field_row(
                    "Short break interval",
                    "secs",
                    move || vm.with(|m| m.config.short_break_interval_secs),
                    move |v| vm.update(|m| m.config.short_break_interval_secs = v),
                ))
                .child(number_field_row(
                    "Short break duration",
                    "secs",
                    move || vm.with(|m| m.config.short_break_duration_secs),
                    move |v| vm.update(|m| m.config.short_break_duration_secs = v),
                ))
                .child(number_field_row(
                    "Long break interval",
                    "secs",
                    move || vm.with(|m| m.config.long_break_interval_secs),
                    move |v| vm.update(|m| m.config.long_break_interval_secs = v),
                ))
                .child(number_field_row(
                    "Long break duration",
                    "secs",
                    move || vm.with(|m| m.config.long_break_duration_secs),
                    move |v| vm.update(|m| m.config.long_break_duration_secs = v),
                )),
        )
        .child(section_divider())
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Transitions & Controls",
                    "Fine-tune warnings, fade-in speed, postpone length, and session persistence.",
                ))
                .child(number_field_row(
                    "Pre-break warning",
                    "secs",
                    move || vm.with(|m| m.config.pre_break_warning_secs),
                    move |v| vm.update(|m| m.config.pre_break_warning_secs = v),
                ))
                .child(number_field_row(
                    "Fade-in duration",
                    "ms",
                    move || vm.with(|m| m.config.fade_in_ms),
                    move |v| vm.update(|m| m.config.fade_in_ms = v),
                ))
                .child(number_field_row(
                    "Shortcut unlock delay",
                    "secs",
                    move || vm.with(|m| m.config.shortcut_disable_secs),
                    move |v| vm.update(|m| m.config.shortcut_disable_secs = v),
                ))
                .child(number_field_row(
                    "Postpone duration",
                    "secs",
                    move || vm.with(|m| m.config.postpone_duration_secs),
                    move |v| vm.update(|m| m.config.postpone_duration_secs = v),
                ))
                .child(cozy_switch(
                    "Randomize exercise order",
                    move || vm.with(|m| m.config.random_order),
                    move |v| vm.update(|m| m.config.random_order = v),
                ))
                .child(cozy_switch(
                    "Persist session across restarts",
                    move || vm.with(|m| m.config.persist_state),
                    move |v| vm.update(|m| m.config.persist_state = v),
                )),
        )
}

fn policies_section(vm: Signal<SettingsViewModel>) -> impl IntoElement {
    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Break Enforcement",
                    "Control whether breaks can be skipped, postponed, or strictly enforced.",
                ))
                .child(cozy_switch(
                    "Strict break (hide Skip)",
                    move || vm.with(|m| m.config.strict_break),
                    move |v| vm.update(|m| m.config.strict_break = v),
                ))
                .child(cozy_switch(
                    "Allow postpone (Space)",
                    move || vm.with(|m| m.config.allow_postpone),
                    move |v| vm.update(|m| m.config.allow_postpone = v),
                ))
                .child(cozy_switch(
                    "Limit consecutive skips",
                    move || vm.with(|m| m.config.limit_consecutive_skips.enabled),
                    move |v| vm.update(|m| m.config.limit_consecutive_skips.enabled = v),
                ))
                .child(number_field_row(
                    "Max consecutive skips",
                    "skips",
                    move || u64::from(vm.with(|m| m.config.limit_consecutive_skips.max_skips)),
                    move |v| {
                        vm.update(|m| {
                            m.config.limit_consecutive_skips.max_skips =
                                u32::try_from(v).unwrap_or(u32::MAX);
                        });
                    },
                )),
        )
        .child(section_divider())
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Smart Pause & Do Not Disturb",
                    "Automatically pause timers while idle, on battery, or in fullscreen apps.",
                ))
                .child(cozy_switch(
                    "Smart Pause on idle",
                    move || vm.with(|m| m.config.smart_pause.enabled),
                    move |v| vm.update(|m| m.config.smart_pause.enabled = v),
                ))
                .child(number_field_row(
                    "Idle threshold",
                    "secs",
                    move || vm.with(|m| m.config.smart_pause.idle_threshold_secs),
                    move |v| vm.update(|m| m.config.smart_pause.idle_threshold_secs = v),
                ))
                .child(cozy_switch(
                    "Do Not Disturb in fullscreen",
                    move || vm.with(|m| m.config.do_not_disturb.enabled),
                    move |v| vm.update(|m| m.config.do_not_disturb.enabled = v),
                ))
                .child(cozy_switch(
                    "Pause while on battery",
                    move || vm.with(|m| m.config.do_not_disturb.while_on_battery),
                    move |v| vm.update(|m| m.config.do_not_disturb.while_on_battery = v),
                )),
        )
        .child(section_divider())
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Audible Chime",
                    "Play a gentle synthesized chime before or after breaks.",
                ))
                .child(cozy_switch(
                    "Pre-break chime",
                    move || vm.with(|m| m.config.audible_alert.pre_break),
                    move |v| vm.update(|m| m.config.audible_alert.pre_break = v),
                ))
                .child(cozy_switch(
                    "Post-break chime",
                    move || vm.with(|m| m.config.audible_alert.post_break),
                    move |v| vm.update(|m| m.config.audible_alert.post_break = v),
                ))
                .child(number_field_row(
                    "Chime volume",
                    "0–100",
                    move || u64::from(vm.with(|m| m.config.audible_alert.volume)),
                    move |v| {
                        vm.update(|m| {
                            m.config.audible_alert.volume = u8::try_from(v.min(100)).unwrap_or(100);
                        });
                    },
                )),
        )
}

fn exercises_section(vm: Signal<SettingsViewModel>) -> impl IntoElement {
    let short_items = computed(move || {
        vm.with(|m| {
            m.config
                .short_breaks
                .iter()
                .enumerate()
                .map(|(idx, spec)| (idx, spec.name.clone()))
                .collect::<Vec<_>>()
        })
    });
    let long_items = computed(move || {
        vm.with(|m| {
            m.config
                .long_breaks
                .iter()
                .enumerate()
                .map(|(idx, spec)| (idx, spec.name.clone()))
                .collect::<Vec<_>>()
        })
    });

    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Short Break Exercises",
                    "Quick 15-second prompts cycled during work intervals.",
                ))
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .items_center()
                        .child(view().flex_1().child(cozy_input(
                            "Add a short exercise...",
                            move || vm.with(|m| m.draft_short_exercise.clone()),
                            move |s: &str| vm.update(|m| m.draft_short_exercise = s.to_owned()),
                        )))
                        .child(secondary_button("Add", move || {
                            vm.update(|m| {
                                let name = m.draft_short_exercise.clone();
                                if m.add_exercise(BreakType::Short, name).is_ok() {
                                    m.draft_short_exercise.clear();
                                }
                            });
                        })),
                )
                .child(view().gap_2().child(each(
                    short_items,
                    |(idx, name)| format!("short-{idx}-{name}"),
                    move |item, _| {
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .rounded(radii::ELEMENT)
                            .bg(colors::BACKGROUND_MUTED)
                            .px_3()
                            .py_2()
                            .child(
                                text(move || item.with(|(_, name)| name.clone()))
                                    .text_sm()
                                    .text_color(colors::TEXT_PRIMARY),
                            )
                            .child(destructive_button("Remove", move || {
                                let idx = item.with(|(i, _)| *i);
                                vm.update(|m| {
                                    let _ = m.remove_exercise(BreakType::Short, idx);
                                });
                            }))
                    },
                ))),
        )
        .child(section_divider())
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Long Break Exercises",
                    "Restorative prompts shown after every four short breaks.",
                ))
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .items_center()
                        .child(view().flex_1().child(cozy_input(
                            "Add a long exercise...",
                            move || vm.with(|m| m.draft_long_exercise.clone()),
                            move |s: &str| vm.update(|m| m.draft_long_exercise = s.to_owned()),
                        )))
                        .child(secondary_button("Add", move || {
                            vm.update(|m| {
                                let name = m.draft_long_exercise.clone();
                                if m.add_exercise(BreakType::Long, name).is_ok() {
                                    m.draft_long_exercise.clear();
                                }
                            });
                        })),
                )
                .child(view().gap_2().child(each(
                    long_items,
                    |(idx, name)| format!("long-{idx}-{name}"),
                    move |item, _| {
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .rounded(radii::ELEMENT)
                            .bg(colors::BACKGROUND_MUTED)
                            .px_3()
                            .py_2()
                            .child(
                                text(move || item.with(|(_, name)| name.clone()))
                                    .text_sm()
                                    .text_color(colors::TEXT_PRIMARY),
                            )
                            .child(destructive_button("Remove", move || {
                                let idx = item.with(|(i, _)| *i);
                                vm.update(|m| {
                                    let _ = m.remove_exercise(BreakType::Long, idx);
                                });
                            }))
                    },
                ))),
        )
}

fn about_section(appearance: Signal<Appearance>) -> impl IntoElement {
    view()
        .gap_5()
        .child(
            view()
                .gap_3()
                .child(section_header(
                    "Cozy Appearance",
                    "Switch between warm oatmeal Light mode, dark roast Dark mode, or follow your OS.",
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            segmented_track()
                                .child(segmented_option(
                                    "Light",
                                    move || appearance.get() == Appearance::Light,
                                    move || {
                                        appearance.set(Appearance::Light);
                                        set_color_scheme(Appearance::Light);
                                    },
                                ))
                                .child(segmented_option(
                                    "Dark",
                                    move || appearance.get() == Appearance::Dark,
                                    move || {
                                        appearance.set(Appearance::Dark);
                                        set_color_scheme(Appearance::Dark);
                                    },
                                ))
                                .child(segmented_option(
                                    "System",
                                    move || appearance.get() == Appearance::System,
                                    move || {
                                        appearance.set(Appearance::System);
                                        set_color_scheme(Appearance::System);
                                    },
                                )),
                        )
                        .child(
                            text(move || match appearance.get() {
                                Appearance::Light => "Active mode: Light (Warm Oatmeal)",
                                Appearance::Dark => "Active mode: Dark (Dark Roast)",
                                Appearance::System => "Active mode: System",
                            })
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors::TEXT_SECONDARY),
                        ),
                ),
        )
        .child(section_divider())
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
                        .child(section_header(
                            "Support Safe Your Eyes",
                            "Built in Rust with Native Shell (nsh) and the Cozy Design System for minimal footprint.",
                        ))
                        .child(
                            text(format!("Support link: {SUPPORT_URL}"))
                                .text_xs()
                                .text_color(colors::TEXT_SECONDARY),
                        )
                        .child(view().flex_row().child(support_link("Buy Me a Coffee"))),
                )
                .child(support_qr_card()),
        )
}

/// Builds the Cozy Settings UI surface for both desktop runs and headless tests.
pub(crate) fn build_settings_view(
    initial_vm: SettingsViewModel,
    outcome_sink: Arc<Mutex<Option<SettingsOutcome>>>,
    quit_on_finish: bool,
) -> impl IntoElement {
    let vm = signal(initial_vm);
    let appearance = signal(Appearance::Light);

    let active_tab = computed(move || vm.with(|m| m.active_tab));
    let warning_msg = computed(move || vm.with(|m| m.load_warning.clone()));
    let error_msg = computed(move || vm.with(|m| m.validation_error.clone()));

    let save_sink = Arc::clone(&outcome_sink);
    let cancel_sink = outcome_sink;

    let sidebar = view()
        .w(px(210.0))
        .flex_shrink_0()
        .rounded(radii::SIDEBAR)
        .bg(colors::SIDEBAR_BG)
        .p_3()
        .justify_between()
        .child(
            view()
                .gap_2()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_2()
                        .py_2()
                        .child(app_icon(36.0))
                        .child(
                            view()
                                .gap_1()
                                .child(
                                    text("Safe Your Eyes")
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(colors::TEXT_PRIMARY),
                                )
                                .child(
                                    text("Cozy Eye Care")
                                        .text_xs()
                                        .text_color(colors::TEXT_SECONDARY),
                                ),
                        ),
                )
                .child(
                    view()
                        .gap_1()
                        .child(sidebar_nav_item(
                            "Timing",
                            move || active_tab.get() == SettingsTab::Timing,
                            move || vm.update(|m| m.active_tab = SettingsTab::Timing),
                        ))
                        .child(sidebar_nav_item(
                            "Policies",
                            move || active_tab.get() == SettingsTab::Policies,
                            move || vm.update(|m| m.active_tab = SettingsTab::Policies),
                        ))
                        .child(sidebar_nav_item(
                            "Exercises",
                            move || active_tab.get() == SettingsTab::Exercises,
                            move || vm.update(|m| m.active_tab = SettingsTab::Exercises),
                        ))
                        .child(sidebar_nav_item(
                            "About & Support",
                            move || active_tab.get() == SettingsTab::About,
                            move || vm.update(|m| m.active_tab = SettingsTab::About),
                        )),
                ),
        )
        .child(
            view().px_3().py_1().child(
                text(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors::TEXT_SECONDARY),
            ),
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
            vm.with(|m| {
                format!(
                    "{}",
                    m.config.short_breaks.len() + m.config.long_breaks.len()
                )
            })
        }))
        .child(pastel_kpi_card(KpiTone::Mist, "Skip Limit", move || {
            vm.with(|m| {
                if m.config.limit_consecutive_skips.enabled {
                    format!("{} max", m.config.limit_consecutive_skips.max_skips)
                } else {
                    "Off".to_owned()
                }
            })
        }));

    let page_header = view()
        .flex_shrink_0()
        .gap_3()
        .child(page_heading("Settings", move || match active_tab.get() {
            SettingsTab::Timing => "Timing",
            SettingsTab::Policies => "Policies",
            SettingsTab::Exercises => "Exercises",
            SettingsTab::About => "About & Support",
        }))
        .child(kpi_row)
        .child(when(
            move || warning_msg.get().is_some(),
            move || status_banner(false, move || warning_msg.get().unwrap_or_default()),
        ))
        .child(when(
            move || error_msg.get().is_some(),
            move || status_banner(true, move || error_msg.get().unwrap_or_default()),
        ));

    let form_scroll = scroll().flex_1().bg(colors::BACKGROUND_SURFACE).child(
        view()
            .p_5()
            .bg(colors::BACKGROUND_SURFACE)
            .child(when(
                move || active_tab.get() == SettingsTab::Timing,
                move || timing_section(vm),
            ))
            .child(when(
                move || active_tab.get() == SettingsTab::Policies,
                move || policies_section(vm),
            ))
            .child(when(
                move || active_tab.get() == SettingsTab::Exercises,
                move || exercises_section(vm),
            ))
            .child(when(
                move || active_tab.get() == SettingsTab::About,
                move || about_section(appearance),
            )),
    );

    let sticky_toolbar = view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .justify_between()
        .px_5()
        .py_3()
        .rounded_b(radii::CONTAINER)
        .bg(colors::BACKGROUND_SURFACE)
        .border_t_1()
        .border_color(colors::BORDER)
        .child(
            view()
                .flex_row()
                .gap_2()
                .child(primary_button("Save", move || {
                    let mut saved = false;
                    vm.update(|m| {
                        if m.save().is_ok() {
                            saved = true;
                        }
                    });
                    if saved {
                        store_outcome(&save_sink, SettingsOutcome::Saved);
                        if quit_on_finish {
                            quit();
                        }
                    }
                }))
                .child(secondary_button("Reset", move || {
                    vm.update(SettingsViewModel::reset_to_defaults);
                })),
        )
        .child(ghost_button("Cancel", move || {
            vm.update(|m| {
                let _ = m.cancel();
            });
            store_outcome(&cancel_sink, SettingsOutcome::Cancelled);
            if quit_on_finish {
                quit();
            }
        }));

    // Unified Form Card keeps the scrollable form and sticky footer strictly inside one rounded border.
    let form_card = view()
        .flex_1()
        .rounded(radii::CONTAINER)
        .bg(colors::BACKGROUND_SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .overflow_hidden()
        .child(form_scroll)
        .child(sticky_toolbar);

    view()
        .flex_1()
        .bg(colors::BACKGROUND_BODY)
        .flex_row()
        .p_4()
        .gap_4()
        .child(sidebar)
        .child(view().flex_1().gap_4().child(page_header).child(form_card))
}

#[component]
fn SettingsRoot() -> impl IntoElement {
    let launch = SETTINGS_LAUNCH.lock().ok().and_then(|mut g| g.take());
    match launch {
        Some(launch) => build_settings_view(launch.vm, launch.outcome_sink, true).into_element(),
        None => view()
            .flex_1()
            .bg(colors::BACKGROUND_BODY)
            .items_center()
            .justify_center()
            .child(text("Safe Your Eyes — Settings").text_color(colors::TEXT_PRIMARY))
            .into_element(),
    }
}

/// Runs the Cozy Settings window using Native Shell (`nsh`) and returns `SettingsOutcome`.
pub fn run_settings(config_path: PathBuf) -> Result<SettingsOutcome, UiError> {
    let vm = SettingsViewModel::load(config_path);
    let outcome_sink = Arc::new(Mutex::new(None));

    if let Ok(mut guard) = SETTINGS_LAUNCH.lock() {
        *guard = Some(SettingsLaunch {
            vm,
            outcome_sink: Arc::clone(&outcome_sink),
        });
    } else {
        return Err(UiError::WindowInit(
            "settings launch state lock poisoned".to_owned(),
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let _ = platform::run_app(
            platform::AppSource::Rust(platform::Root::of::<SettingsRoot>()),
            platform::AppOptions {
                title: "Safe Your Eyes — Settings".to_owned(),
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                app_id: "io.github.tunglt1810.safeyoureyes.settings".to_owned(),
                size: (920, 680),
                appearance: Appearance::Light,
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
        vm.config.short_break_interval_secs = 600;
        vm.config.short_break_duration_secs = 20;
        vm.config.long_break_interval_secs = 3600;
        vm.config.long_break_duration_secs = 90;
        vm.config.pre_break_warning_secs = 15;
        vm.config.strict_break = true;
        vm.config.allow_postpone = true;
        vm.config.postpone_duration_secs = 180;
        vm.config.random_order = false;
        vm.config.shortcut_disable_secs = 3;
        vm.config.fade_in_ms = 800;
        vm.config.persist_state = true;
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
        vm.config.short_break_interval_secs = 0;
        vm.draft_short_exercise = "draft".to_owned();
        vm.draft_skip_window = "mpv".to_owned();
        assert!(vm.validate().is_err());

        vm.reset_to_defaults();
        assert_eq!(vm.config, Config::default());
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
    fn nsh_headless_settings_navigates_sidebar_and_shows_support_link() -> Result<(), String> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        #[component]
        fn TestSettings() -> impl IntoElement {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join(".tmp")
                .join("headless-settings-test.toml");
            let vm = SettingsViewModel::load(path);
            let sink = Arc::new(Mutex::new(None));
            build_settings_view(vm, sink, false)
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(900.0), Points(640.0)),
            ..EngineConfig::new(Root::of::<TestSettings>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Safe Your Eyes")?;
        assert!(app.shows("Break Schedule"));
        assert!(app.shows("Short Break"));

        app.press("About & Support")?;
        app.wait_for("Buy Me a Coffee")?;
        assert!(app.shows("Support link: https://buymeacoffee.com/bbeeezzzzz"));
        assert!(app.shows("Scan to Buy Me a Coffee"));
        Ok(())
    }
}
