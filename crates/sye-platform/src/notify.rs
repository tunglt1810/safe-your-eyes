//! Cross-platform desktop pre-break notification adapter (`sye_core::Notifier`).

#[cfg(not(target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(not(target_os = "macos"))]
use notify_rust::{Notification, Timeout};
use sye_core::{BreakType, Notifier, PlatformError};

#[cfg(not(target_os = "macos"))]
const APP_NAME: &str = "Safe Your Eyes";
#[cfg(not(target_os = "macos"))]
const APP_ICON: &str = "safe-your-eyes";

/// Desktop notification adapter backed by a non-activating `AppKit` banner on macOS and `notify-rust` elsewhere.
pub struct OsNotifier {
    enabled: bool,
    #[cfg(all(unix, not(target_os = "macos")))]
    active: Mutex<Option<LinuxActive>>,
    #[cfg(not(unix))]
    active: AtomicBool,
}

#[cfg(all(unix, not(target_os = "macos")))]
struct LinuxActive {
    handle: notify_rust::NotificationHandle,
    listened_id: u32,
    user_closed: Arc<AtomicBool>,
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_close_listener(id: u32, user_closed: &Arc<AtomicBool>) {
    let user_closed = Arc::clone(user_closed);
    // `handle_action` blocks until the server reports `ActionInvoked` or `NotificationClosed`.
    let _ = std::thread::Builder::new()
        .name("sye-notify-close".to_owned())
        .spawn(move || {
            let _ = notify_rust::handle_action(id, |response: &notify_rust::ActionResponse| {
                if matches!(
                    response,
                    notify_rust::ActionResponse::Closed(notify_rust::CloseReason::Dismissed)
                ) {
                    user_closed.store(true, Ordering::Release);
                }
            });
        });
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
            #[cfg(not(unix))]
            active: AtomicBool::new(false),
        }
    }

    /// Creates a no-op notifier for `--headless` mode or tests.
    #[must_use]
    pub const fn noop() -> Self {
        Self {
            enabled: false,
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
            #[cfg(not(unix))]
            active: AtomicBool::new(false),
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

#[cfg(any(target_os = "macos", test))]
#[must_use]
pub(crate) fn compute_macos_banner_origin(
    visible_frame: crate::tray::ScreenRect,
    banner_size: (f64, f64),
) -> (f64, f64) {
    const RIGHT_MARGIN: f64 = 12.0;
    const TOP_MARGIN: f64 = 12.0;

    let min_x = visible_frame.x + RIGHT_MARGIN;
    let max_x = (visible_frame.x + visible_frame.width - banner_size.0 - RIGHT_MARGIN).max(min_x);
    let min_y = visible_frame.y + TOP_MARGIN;
    let max_y = (visible_frame.y + visible_frame.height - banner_size.1 - TOP_MARGIN).max(min_y);
    (max_x.round(), max_y.round())
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

        let (summary, body) = format_pre_break_notification(break_type, title, warning_secs);

        #[cfg(target_os = "macos")]
        {
            macos_impl::show_pre_break(&summary, &body)
        }

        #[cfg(not(target_os = "macos"))]
        {
            let timeout_ms = u32::try_from(warning_secs.saturating_mul(1000)).unwrap_or(u32::MAX);

            #[cfg(all(unix, not(target_os = "macos")))]
            {
                {
                    let mut guard = self.active.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(active) = guard.as_mut() {
                        if active.user_closed.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        active
                            .handle
                            .summary(&summary)
                            .body(&body)
                            .timeout(Timeout::Milliseconds(timeout_ms));
                        if active.handle.update().is_ok() {
                            // The server can assign a new id when the replaced notification expired.
                            let id = active.handle.id();
                            if id != active.listened_id {
                                active.listened_id = id;
                                spawn_close_listener(id, &active.user_closed);
                            }
                            return Ok(());
                        }
                        *guard = None;
                    }
                }

                let mut notification = Notification::new();
                notification
                    .appname(APP_NAME)
                    .icon(APP_ICON)
                    .summary(&summary)
                    .body(&body)
                    .timeout(Timeout::Milliseconds(timeout_ms));
                let handle = notification
                    .show()
                    .map_err(|err| PlatformError::Unavailable(err.to_string()))?;
                let user_closed = Arc::new(AtomicBool::new(false));
                let listened_id = handle.id();
                spawn_close_listener(listened_id, &user_closed);
                let mut guard = self.active.lock().unwrap_or_else(PoisonError::into_inner);
                *guard = Some(LinuxActive {
                    handle,
                    listened_id,
                    user_closed,
                });
                drop(guard);
                Ok(())
            }

            #[cfg(not(unix))]
            {
                // `notify-rust` on Windows cannot update an existing toast in place, so avoid
                // stacking a separate toast on each countdown tick.
                if self.active.swap(true, Ordering::AcqRel) {
                    return Ok(());
                }

                let mut notification = Notification::new();
                notification
                    .appname(APP_NAME)
                    .icon(APP_ICON)
                    .summary(&summary)
                    .body(&body)
                    .timeout(Timeout::Milliseconds(timeout_ms));
                if let Err(err) = notification.show() {
                    self.active.store(false, Ordering::Release);
                    return Err(PlatformError::Unavailable(err.to_string()));
                }
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
                active.handle.close();
            }
        }

        #[cfg(not(unix))]
        self.active.store(false, Ordering::Release);

        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "Non-activating AppKit NSPanel banner for in-place pre-break countdown"
)]
mod macos_impl {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::rc::{Retained, autoreleasepool};
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSEvent, NSFont, NSFontWeightRegular, NSFontWeightSemibold,
        NSImage, NSImageView, NSLineBreakMode, NSPanel, NSResponder, NSScreen, NSStatusWindowLevel,
        NSTextField, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
        NSVisualEffectState, NSVisualEffectView, NSWindowAnimationBehavior,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSData, NSObject, NSPoint, NSRect, NSSize, NSString};
    use sye_core::PlatformError;

    use super::compute_macos_banner_origin;
    use crate::tray::ScreenRect;

    const APP_ICON_PNG: &[u8] =
        include_bytes!("../../../assets/icons/hicolor/128x128/apps/safe-your-eyes.png");
    const BANNER_WIDTH: f64 = 356.0;
    const MIN_HEIGHT: f64 = 64.0;
    const CORNER_RADIUS: f64 = 16.0;
    const ICON_SIZE: f64 = 38.0;
    const ICON_X: f64 = 13.0;
    const TEXT_X: f64 = 62.0;
    const PAD_RIGHT: f64 = 14.0;
    const PAD_Y: f64 = 12.0;
    const LINE_GAP: f64 = 2.0;
    const TEXT_WIDTH: f64 = BANNER_WIDTH - TEXT_X - PAD_RIGHT;
    const MAX_FIT_HEIGHT: f64 = 10_000.0;

    // `UNUserNotificationCenter::addNotificationRequest:` re-animates a new top-right banner on
    // every countdown second and Temporary Banners auto-hide after ~5s, so macOS renders an
    // in-place non-activating `NSPanel` instead.
    static USER_DISMISSED: AtomicBool = AtomicBool::new(false);

    thread_local! {
        static BANNER: RefCell<Option<PreBreakBanner>> = const { RefCell::new(None) };
    }

    define_class!(
        #[unsafe(super(NSView, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SyePreBreakBannerClickView"]
        struct BannerClickView;

        impl BannerClickView {
            // Deliver the dismissal click immediately while another app holds focus.
            #[unsafe(method(acceptsFirstMouse:))]
            fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
                true
            }

            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, _event: &NSEvent) {
                USER_DISMISSED.store(true, Ordering::Release);
                // Hide immediately; defer panel destruction to avoid deallocating `self` inside `mouseDown:`.
                if let Some(window) = self.window() {
                    window.orderOut(None);
                }
            }
        }
    );

    struct PreBreakBanner {
        panel: Retained<NSPanel>,
        effect_view: Retained<NSVisualEffectView>,
        icon_view: Option<Retained<NSImageView>>,
        title_label: Retained<NSTextField>,
        body_label: Retained<NSTextField>,
        click_shield: Retained<BannerClickView>,
        last_summary: String,
        last_body: String,
    }

    impl PreBreakBanner {
        fn new(mtm: MainThreadMarker) -> Self {
            let initial_rect = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(BANNER_WIDTH, MIN_HEIGHT),
            );
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                initial_rect,
                NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                NSBackingStoreType::Buffered,
                false,
            );
            // SAFETY: `PreBreakBanner` owns `panel` via `Retained<NSPanel>` and hides it with
            // `orderOut` on drop, so AppKit must not auto-release it on close.
            unsafe {
                panel.setReleasedWhenClosed(false);
            }
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(true);
            panel.setWorksWhenModal(true);
            panel.setOpaque(false);
            panel.setBackgroundColor(Some(&NSColor::clearColor()));
            panel.setHasShadow(true);
            panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
            panel.setLevel(NSStatusWindowLevel);
            panel.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );

            let effect_view =
                NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), initial_rect);
            effect_view.setMaterial(NSVisualEffectMaterial::Popover);
            effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect_view.setState(NSVisualEffectState::Active);
            effect_view.setWantsLayer(true);
            if let Some(layer) = effect_view.layer() {
                layer.setCornerRadius(CORNER_RADIUS);
                layer.setMasksToBounds(true);
            }

            let icon_data = NSData::with_bytes(APP_ICON_PNG);
            let icon_view = NSImage::initWithData(NSImage::alloc(), &icon_data).map(|image| {
                image.setSize(NSSize::new(ICON_SIZE, ICON_SIZE));
                let view = NSImageView::imageViewWithImage(&image, mtm);
                effect_view.addSubview(&view);
                view
            });

            // SAFETY: `NSFontWeightSemibold` and `NSFontWeightRegular` are immutable AppKit constants.
            let (title_font, body_font) = unsafe {
                (
                    NSFont::monospacedDigitSystemFontOfSize_weight(13.0, NSFontWeightSemibold),
                    NSFont::systemFontOfSize_weight(13.0, NSFontWeightRegular),
                )
            };

            let title_label = NSTextField::labelWithString(&NSString::new(), mtm);
            title_label.setFont(Some(&title_font));
            title_label.setTextColor(Some(&NSColor::labelColor()));
            title_label.setMaximumNumberOfLines(1);
            title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            effect_view.addSubview(&title_label);

            let body_label = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
            body_label.setSelectable(false);
            body_label.setFont(Some(&body_font));
            body_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
            body_label.setMaximumNumberOfLines(2);
            body_label.setPreferredMaxLayoutWidth(TEXT_WIDTH);
            if let Some(cell) = body_label.cell() {
                cell.setTruncatesLastVisibleLine(true);
            }
            effect_view.addSubview(&body_label);

            // SAFETY: `BannerClickView` inherits `-[NSView initWithFrame:]` with no custom ivars.
            let click_shield: Retained<BannerClickView> =
                unsafe { msg_send![BannerClickView::alloc(mtm), initWithFrame: initial_rect] };
            effect_view.addSubview(&click_shield);
            panel.setContentView(Some(&effect_view));

            Self {
                panel,
                effect_view,
                icon_view,
                title_label,
                body_label,
                click_shield,
                last_summary: String::new(),
                last_body: String::new(),
            }
        }

        fn show_or_update(
            &mut self,
            summary: &str,
            body: &str,
            mtm: MainThreadMarker,
        ) -> Result<(), PlatformError> {
            let summary_changed = self.last_summary != summary;
            let body_changed = self.last_body != body;
            let was_visible = self.panel.isVisible();

            if summary_changed {
                self.last_summary.clear();
                self.last_summary.push_str(summary);
                self.title_label
                    .setStringValue(&NSString::from_str(summary));
            }
            let visible = active_screen_visible_frame(mtm).ok_or_else(|| {
                PlatformError::Unavailable("macOS display screen unavailable".to_owned())
            })?;
            let prev_frame = self.panel.frame();
            let needs_layout = !was_visible || body_changed;
            let mut banner_size = prev_frame.size;
            if needs_layout {
                if body_changed {
                    self.last_body.clear();
                    self.last_body.push_str(body);
                    self.body_label.setStringValue(&NSString::from_str(body));
                }

                let fit_bounds = NSSize::new(TEXT_WIDTH, MAX_FIT_HEIGHT);
                let title_h = self.title_label.sizeThatFits(fit_bounds).height.ceil();
                let body_h = self.body_label.sizeThatFits(fit_bounds).height.ceil();
                let text_block_h = title_h + LINE_GAP + body_h;
                let banner_h = (text_block_h + PAD_Y * 2.0).max(MIN_HEIGHT).ceil();
                banner_size = NSSize::new(BANNER_WIDTH, banner_h);

                self.effect_view
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), banner_size));
                self.click_shield
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), banner_size));
                if let Some(icon_view) = &self.icon_view {
                    let icon_y = ((banner_h - ICON_SIZE) * 0.5).round();
                    icon_view.setFrame(NSRect::new(
                        NSPoint::new(ICON_X, icon_y),
                        NSSize::new(ICON_SIZE, ICON_SIZE),
                    ));
                }

                let body_y = ((banner_h - text_block_h) * 0.5).round();
                let title_y = body_y + body_h + LINE_GAP;
                self.body_label.setFrame(NSRect::new(
                    NSPoint::new(TEXT_X, body_y),
                    NSSize::new(TEXT_WIDTH, body_h),
                ));
                self.title_label.setFrame(NSRect::new(
                    NSPoint::new(TEXT_X, title_y),
                    NSSize::new(TEXT_WIDTH, title_h),
                ));
            }

            let (ox, oy) =
                compute_macos_banner_origin(visible, (banner_size.width, banner_size.height));
            let origin_changed =
                (prev_frame.origin.x - ox).abs() > 0.5 || (prev_frame.origin.y - oy).abs() > 0.5;
            if needs_layout || origin_changed {
                self.panel
                    .setFrame_display(NSRect::new(NSPoint::new(ox, oy), banner_size), was_visible);
            }
            if !was_visible {
                self.panel.orderFrontRegardless();
            }
            Ok(())
        }
    }

    impl Drop for PreBreakBanner {
        fn drop(&mut self) {
            self.panel.orderOut(None);
        }
    }

    fn clear_banner() {
        if MainThreadMarker::new().is_some() {
            autoreleasepool(|_| {
                BANNER.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut() {
                        let _ = guard.take();
                    }
                });
            });
        }
    }

    fn ns_rect_to_screen_rect(rect: NSRect) -> ScreenRect {
        ScreenRect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }
    }

    fn active_screen_visible_frame(mtm: MainThreadMarker) -> Option<ScreenRect> {
        let cursor = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        for screen in &screens {
            let full = ns_rect_to_screen_rect(screen.frame());
            if full.contains((cursor.x, cursor.y)) {
                return Some(ns_rect_to_screen_rect(screen.visibleFrame()));
            }
        }
        NSScreen::mainScreen(mtm).map(|s| ns_rect_to_screen_rect(s.visibleFrame()))
    }

    pub(super) fn show_pre_break(summary: &str, body: &str) -> Result<(), PlatformError> {
        if USER_DISMISSED.load(Ordering::Acquire) {
            clear_banner();
            return Ok(());
        }
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            PlatformError::Unavailable(
                "macOS pre-break banner must run on the main thread".to_owned(),
            )
        })?;
        autoreleasepool(|_| {
            BANNER.with(|slot| {
                let mut guard = slot.try_borrow_mut().map_err(|_| {
                    PlatformError::Unavailable("macOS pre-break banner busy".to_owned())
                })?;
                let banner = guard.get_or_insert_with(|| PreBreakBanner::new(mtm));
                banner.show_or_update(summary, body, mtm)
            })
        })
    }

    pub(super) fn dismiss() {
        USER_DISMISSED.store(false, Ordering::Release);
        clear_banner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::ScreenRect;

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
    fn macos_banner_anchors_top_right_inside_visible_frame() {
        let visible = ScreenRect {
            x: 0.0,
            y: 54.0,
            width: 1512.0,
            height: 904.0,
        };
        assert_eq!(
            compute_macos_banner_origin(visible, (356.0, 64.0)),
            (1144.0, 882.0)
        );

        let tiny = ScreenRect {
            x: 100.0,
            y: 40.0,
            width: 300.0,
            height: 60.0,
        };
        assert_eq!(
            compute_macos_banner_origin(tiny, (356.0, 64.0)),
            (112.0, 52.0)
        );
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
