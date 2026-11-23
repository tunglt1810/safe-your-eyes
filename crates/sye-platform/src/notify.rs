//! Cross-platform desktop pre-break notification adapter (`sye_core::Notifier`).

#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{Mutex, PoisonError};

use notify_rust::{Notification, Timeout};
use sye_core::{BreakType, Notifier, PlatformError};

const APP_NAME: &str = "Safe Your Eyes";
const APP_ICON: &str = "safe-your-eyes";

/// Desktop notification adapter backed by `notify-rust` (pure-Rust `zbus` on Linux).
pub struct OsNotifier {
    enabled: bool,
    #[cfg(all(unix, not(target_os = "macos")))]
    active: Mutex<Option<notify_rust::NotificationHandle>>,
}

impl std::fmt::Debug for OsNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsNotifier")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl OsNotifier {
    /// Creates an active OS notifier.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            enabled: true,
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }

    /// Creates a no-op notifier for `--headless` mode or tests.
    #[must_use]
    pub const fn noop() -> Self {
        Self {
            enabled: false,
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }
}

impl Default for OsNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats the `(summary, body)` pair for a pre-break notification.
#[must_use]
pub fn format_pre_break_notification(
    break_type: BreakType,
    title: &str,
    warning_secs: u64,
) -> (String, String) {
    let kind_label = match break_type {
        BreakType::Short => "Short break",
        BreakType::Long => "Long break",
    };
    let summary = format!("Safe Your Eyes — {kind_label} in {warning_secs}s");
    let trimmed = title.trim();
    let body = if trimmed.is_empty() {
        "Prepare to rest your eyes.".to_owned()
    } else {
        trimmed.to_owned()
    };
    (summary, body)
}

impl Notifier for OsNotifier {
    fn show_pre_break(
        &self,
        break_type: BreakType,
        title: &str,
        warning_secs: u64,
    ) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        let _ = self.dismiss();

        let (summary, body) = format_pre_break_notification(break_type, title, warning_secs);
        let timeout_ms = u32::try_from(warning_secs.saturating_mul(1000)).unwrap_or(u32::MAX);

        let mut notification = Notification::new();
        notification
            .appname(APP_NAME)
            .icon(APP_ICON)
            .summary(&summary)
            .body(&body)
            .timeout(Timeout::Milliseconds(timeout_ms));

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let handle = notification
                .show()
                .map_err(|err| PlatformError::Unavailable(err.to_string()))?;
            let mut guard = self.active.lock().unwrap_or_else(PoisonError::into_inner);
            *guard = Some(handle);
            drop(guard);
            Ok(())
        }

        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            notification
                .show()
                .map_err(|err| PlatformError::Unavailable(err.to_string()))?;
            Ok(())
        }
    }

    fn dismiss(&self) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let handle = {
                let mut guard = self.active.lock().unwrap_or_else(PoisonError::into_inner);
                guard.take()
            };
            if let Some(active) = handle {
                active.close();
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pre_break_notification_handles_short_long_and_empty_title() {
        let (summary, body) = format_pre_break_notification(BreakType::Short, "Blink", 10);
        assert_eq!(summary, "Safe Your Eyes — Short break in 10s");
        assert_eq!(body, "Blink");

        let (long_summary, long_body) = format_pre_break_notification(BreakType::Long, "   ", 15);
        assert_eq!(long_summary, "Safe Your Eyes — Long break in 15s");
        assert_eq!(long_body, "Prepare to rest your eyes.");
    }

    #[test]
    fn noop_notifier_succeeds_without_display_or_dbus() -> Result<(), Box<dyn std::error::Error>> {
        let notifier = OsNotifier::noop();
        notifier.show_pre_break(BreakType::Short, "Look away", 10)?;
        notifier.dismiss()?;
        Ok(())
    }

    #[test]
    fn os_notifier_degrades_gracefully_in_headless_env() {
        let notifier = OsNotifier::new();
        let res = notifier.show_pre_break(BreakType::Short, "Look away", 5);
        assert!(matches!(res, Ok(()) | Err(PlatformError::Unavailable(_))));
        assert!(notifier.dismiss().is_ok());
    }
}
