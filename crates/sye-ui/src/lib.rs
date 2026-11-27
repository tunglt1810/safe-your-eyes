mod monitors;
mod overlay;
mod settings;
mod theme;
mod ui;

use std::fmt;

pub use sye_core::scheduler::{OverlayOutcome, OverlayRequest};

pub use monitors::{
    MonitorRect, OverlayViewportPlan, detect_monitors, parse_xrandr_monitors,
    plan_overlay_viewports,
};
pub use overlay::{
    OverlayKey, OverlayOutcomeCallback, OverlayShownCallback, OverlayViewModel, format_mm_ss,
    run_overlay, run_overlay_with_hook,
};
pub use settings::{
    ApplyUpdateCallback, ApplyUpdateOutcome, AutostartChangeCallback, CheckUpdatesCallback,
    OpenUrlCallback, SettingsOutcome, SettingsStatusConfig, SettingsTab, SettingsUpdateConfig,
    SettingsViewModel, SnapshotCancelCallback, SnapshotCaptureCallback, SnapshotRevealCallback,
    StatusPollCallback, SystemPrefersDarkCallback, ThemeChangeCallback, ThemeTransitionHooks,
    run_settings, run_settings_with_autostart, run_settings_with_hooks,
};
pub use theme::{DOWNLOAD_PAGE_URL, SUPPORT_URL};

#[derive(Debug)]
pub enum UiError {
    WindowInit(String),
    Config(sye_core::ConfigError),
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowInit(msg) => write!(f, "UI window initialization error: {msg}"),
            Self::Config(err) => write!(f, "UI configuration error: {err}"),
        }
    }
}

impl std::error::Error for UiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WindowInit(_) => None,
            Self::Config(err) => Some(err),
        }
    }
}

impl From<sye_core::ConfigError> for UiError {
    fn from(err: sye_core::ConfigError) -> Self {
        Self::Config(err)
    }
}

pub(crate) fn store_outcome<T>(sink: &std::sync::Mutex<Option<T>>, outcome: T) {
    let mut guard = sink
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.get_or_insert(outcome);
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn ui_error_display_and_source() {
        let win_err = UiError::WindowInit("no display".to_owned());
        assert!(win_err.to_string().contains("no display"));
        assert!(win_err.source().is_none());

        let cfg_err = sye_core::ConfigError::Io {
            message: "permission denied".to_owned(),
        };
        let ui_cfg_err = UiError::from(cfg_err);
        assert!(ui_cfg_err.to_string().contains("permission denied"));
        assert!(ui_cfg_err.source().is_some());
    }
}
