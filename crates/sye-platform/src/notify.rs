//! Cross-platform desktop pre-break notification adapter (`sye_core::Notifier`).

#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{Mutex, PoisonError};

#[cfg(not(target_os = "macos"))]
use notify_rust::{Notification, Timeout};
use sye_core::{BreakType, Notifier, PlatformError};

#[cfg(not(target_os = "macos"))]
const APP_NAME: &str = "Safe Your Eyes";
#[cfg(not(target_os = "macos"))]
const APP_ICON: &str = "safe-your-eyes";

/// Desktop notification adapter backed by `UserNotifications` on macOS and `notify-rust` elsewhere.
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

    /// Requests OS notification permission once per process when supported.
    pub fn request_authorization(&self) {
        #[cfg(target_os = "macos")]
        if self.enabled {
            macos_impl::request_authorization();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = self.enabled;
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

        #[cfg(target_os = "macos")]
        {
            macos_impl::show_pre_break(&summary, &body)
        }

        #[cfg(not(target_os = "macos"))]
        {
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

            #[cfg(not(unix))]
            {
                notification
                    .show()
                    .map_err(|err| PlatformError::Unavailable(err.to_string()))?;
                Ok(())
            }
        }
    }

    fn dismiss(&self) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        macos_impl::dismiss();

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

#[cfg(target_os = "macos")]
#[expect(
    clippy::unused_self,
    reason = "Objective-C UNUserNotificationCenterDelegate selector requires &self"
)]
#[expect(
    unsafe_code,
    reason = "UserNotifications.framework dynamic loading and Objective-C messaging"
)]
mod macos_impl {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    use block2::{DynBlock, RcBlock};
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{AnyThread, MainThreadMarker, define_class, msg_send};
    use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
    use sye_core::PlatformError;

    const PRE_BREAK_NOTIFICATION_ID: &str = "io.github.safeyoureyes.pre-break";
    // Alert/banner/list only: badges are unused and chimes play through `OsAudioPlayer`.
    const AUTHORIZATION_OPTIONS: usize = 1 << 2;
    const PRESENTATION: usize = (1 << 3) | (1 << 4);

    thread_local! {
        // `UNUserNotificationCenter.delegate` is weak, so retain the delegate on the main thread.
        static DELEGATE: RefCell<Option<Retained<SyeNotificationDelegate>>> =
            const { RefCell::new(None) };
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "SyeNotificationDelegate"]
        struct SyeNotificationDelegate;

        unsafe impl NSObjectProtocol for SyeNotificationDelegate {}

        impl SyeNotificationDelegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &AnyObject,
                _notification: &AnyObject,
                done: &DynBlock<dyn Fn(usize)>,
            ) {
                done.call((PRESENTATION,));
            }
        }
    );

    pub(super) fn request_authorization() {
        autoreleasepool(|_| {
            let Ok(center) = notification_center() else {
                return;
            };
            install_delegate(&center);
            ensure_authorization(&center);
        });
    }

    pub(super) fn show_pre_break(summary: &str, body: &str) -> Result<(), PlatformError> {
        autoreleasepool(|_| {
            let center = notification_center()?;
            install_delegate(&center);
            ensure_authorization(&center);

            let content_cls = AnyClass::get(c"UNMutableNotificationContent").ok_or_else(|| {
                PlatformError::Unavailable("UNMutableNotificationContent unavailable".to_owned())
            })?;
            let request_cls = AnyClass::get(c"UNNotificationRequest").ok_or_else(|| {
                PlatformError::Unavailable("UNNotificationRequest unavailable".to_owned())
            })?;

            let title_ns = NSString::from_str(summary);
            let body_ns = NSString::from_str(body);
            let id_ns = NSString::from_str(PRE_BREAK_NOTIFICATION_ID);
            let trigger: Option<&AnyObject> = None;
            let req_done = RcBlock::new(|_err: *mut NSError| {});

            // SAFETY: Documented `UserNotifications.framework` initializers, string setters, and
            // `addNotificationRequest:withCompletionHandler:` with a nil trigger for immediate delivery.
            unsafe {
                let content: Retained<AnyObject> = msg_send![content_cls, new];
                let _: () = msg_send![&*content, setTitle: &*title_ns];
                let _: () = msg_send![&*content, setBody: &*body_ns];
                let request: Retained<AnyObject> = msg_send![
                    request_cls,
                    requestWithIdentifier: &*id_ns,
                    content: &*content,
                    trigger: trigger
                ];
                let _: () = msg_send![
                    &*center,
                    addNotificationRequest: &*request,
                    withCompletionHandler: &*req_done
                ];
            }
            Ok(())
        })
    }

    pub(super) fn dismiss() {
        autoreleasepool(|_| {
            let Ok(center) = notification_center() else {
                return;
            };
            let ids =
                NSArray::from_retained_slice(&[NSString::from_str(PRE_BREAK_NOTIFICATION_ID)]);
            // SAFETY: Documented `UNUserNotificationCenter` selectors accepting `NSArray<NSString *> *`.
            unsafe {
                let _: () =
                    msg_send![&*center, removePendingNotificationRequestsWithIdentifiers: &*ids];
                let _: () = msg_send![&*center, removeDeliveredNotificationsWithIdentifiers: &*ids];
            }
        });
    }

    fn ensure_authorization(center: &AnyObject) {
        static AUTH_REQUESTED: OnceLock<()> = OnceLock::new();
        AUTH_REQUESTED.get_or_init(|| {
            let auth_done = RcBlock::new(|_granted: Bool, _err: *mut NSError| {});
            // SAFETY: `requestAuthorizationWithOptions:completionHandler:` is a documented
            // `UNUserNotificationCenter` selector accepting `UNAuthorizationOptions` and `(BOOL, NSError *)`.
            unsafe {
                let _: () = msg_send![
                    center,
                    requestAuthorizationWithOptions: AUTHORIZATION_OPTIONS,
                    completionHandler: &*auth_done
                ];
            }
        });
    }

    fn install_delegate(center: &AnyObject) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        // SAFETY: `delegate` is a documented `UNUserNotificationCenter` property.
        let current: Option<Retained<AnyObject>> = unsafe { msg_send![center, delegate] };
        if current.is_some() {
            return;
        }
        // SAFETY: `SyeNotificationDelegate` inherits `-[NSObject init]` with no ivars.
        let delegate: Retained<SyeNotificationDelegate> =
            unsafe { msg_send![SyeNotificationDelegate::alloc(), init] };
        // SAFETY: `setDelegate:` is weak; `DELEGATE` retains the instance on the main thread.
        let _: () = unsafe { msg_send![center, setDelegate: &*delegate] };
        DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
    }

    fn notification_center() -> Result<Retained<AnyObject>, PlatformError> {
        // `currentNotificationCenter` throws an Objective-C exception outside an `.app` bundle.
        let in_app_bundle = std::env::current_exe()
            .ok()
            .map(|p| p.canonicalize().unwrap_or(p))
            .is_some_and(|p| crate::is_macos_app_bundle_executable(&p));
        if !in_app_bundle || NSBundle::mainBundle().bundleIdentifier().is_none() {
            return Err(PlatformError::Unavailable(
                "macOS notifications require an .app bundle".to_owned(),
            ));
        }
        if !load_framework() {
            return Err(PlatformError::Unavailable(
                "UserNotifications.framework unavailable".to_owned(),
            ));
        }
        let class = AnyClass::get(c"UNUserNotificationCenter").ok_or_else(|| {
            PlatformError::Unavailable("UNUserNotificationCenter unavailable".to_owned())
        })?;
        // SAFETY: Verified `.app` bundle executable and non-nil `bundleIdentifier` above.
        let center: Option<Retained<AnyObject>> =
            unsafe { msg_send![class, currentNotificationCenter] };
        center.ok_or_else(|| {
            PlatformError::Unavailable(
                "UNUserNotificationCenter.currentNotificationCenter returned nil".to_owned(),
            )
        })
    }

    fn load_framework() -> bool {
        static LOADED: OnceLock<bool> = OnceLock::new();
        *LOADED.get_or_init(|| {
            let path = c"/System/Library/Frameworks/UserNotifications.framework/UserNotifications";
            // SAFETY: `path` is a valid NUL-terminated system framework path; handle stays loaded.
            !unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) }.is_null()
        })
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

    #[test]
    fn request_authorization_succeeds_for_noop_and_active_notifiers() {
        let noop = OsNotifier::noop();
        noop.request_authorization();

        let active = OsNotifier::new();
        active.request_authorization();
        active.request_authorization();
    }
}
