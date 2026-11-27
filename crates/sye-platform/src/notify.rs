#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(all(unix, not(target_os = "macos")))]
use notify_rust::{Hint, Notification, Timeout};
use sye_core::{BreakType, Locale, Notifier, PlatformError, localize_exercise_name};

#[cfg(all(unix, not(target_os = "macos")))]
const APP_NAME: &str = "Safe Your Eyes";
#[cfg(all(unix, not(target_os = "macos")))]
const APP_ICON: &str = "safe-your-eyes";

const fn encode_locale(locale: Locale) -> u8 {
    match locale {
        Locale::En => 0,
        Locale::Vi => 1,
    }
}

const fn decode_locale(raw: u8) -> Locale {
    match raw {
        1 => Locale::Vi,
        _ => Locale::En,
    }
}

#[cfg(any(target_os = "macos", windows, test))]
const PRE_BREAK_URGENT_SECS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreBreakCountdown {
    pub(crate) remaining_secs: u64,
    pub(crate) total_secs: u64,
}

impl PreBreakCountdown {
    #[cfg(any(target_os = "macos", windows, test))]
    #[must_use]
    pub(crate) const fn is_urgent(self) -> bool {
        self.remaining_secs <= PRE_BREAK_URGENT_SECS
    }

    #[cfg(any(target_os = "macos", windows, test))]
    #[must_use]
    pub(crate) fn badge_text(self) -> String {
        // `MM:SS` needs 6 characters from 100 minutes, which do not fit in the badge.
        if self.remaining_secs >= 100 * 60 {
            format!("{}m", self.remaining_secs.div_ceil(60))
        } else if self.remaining_secs >= 60 {
            format!(
                "{}:{:02}",
                self.remaining_secs / 60,
                self.remaining_secs % 60
            )
        } else {
            format!("{}s", self.remaining_secs)
        }
    }

    #[must_use]
    pub(crate) fn progress_ratio(self) -> f64 {
        if self.total_secs == 0 {
            return 0.0;
        }
        (self.remaining_secs as f64 / self.total_secs as f64).clamp(0.0, 1.0)
    }

    #[cfg(any(all(unix, not(target_os = "macos")), test))]
    #[must_use]
    pub(crate) fn progress_percent(self) -> i32 {
        let pct = (self.progress_ratio() * 100.0).round();
        if !pct.is_finite() || pct <= 0.0 {
            0
        } else if pct >= 100.0 {
            100
        } else {
            pct as i32
        }
    }
}

pub struct OsNotifier {
    enabled: bool,
    locale: AtomicU8,
    #[cfg(all(unix, not(target_os = "macos")))]
    active: Mutex<Option<LinuxActive>>,
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
            .field("locale", &self.locale())
            .finish_non_exhaustive()
    }
}

impl OsNotifier {
    #[must_use]
    pub const fn new() -> Self {
        Self::with_locale(Locale::En)
    }

    #[must_use]
    pub const fn with_locale(locale: Locale) -> Self {
        Self {
            enabled: true,
            locale: AtomicU8::new(encode_locale(locale)),
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }

    #[must_use]
    pub const fn noop() -> Self {
        Self {
            enabled: false,
            locale: AtomicU8::new(encode_locale(Locale::En)),
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }

    pub fn set_locale(&self, locale: Locale) {
        self.locale.store(encode_locale(locale), Ordering::Release);
    }

    #[must_use]
    pub fn locale(&self) -> Locale {
        decode_locale(self.locale.load(Ordering::Acquire))
    }

    pub fn show_notice(&self, summary: &str, body: &str) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let accessible = format_notice_accessibility_label(summary, body);
            macos_impl::show_banner(summary, body, &accessible, None)
        }

        #[cfg(windows)]
        {
            let accessible = format_notice_accessibility_label(summary, body);
            windows_impl::show_banner(summary, body, &accessible, None)
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut notification = Notification::new();
            notification
                .appname(APP_NAME)
                .icon(APP_ICON)
                .summary(summary)
                .body(body)
                .timeout(Timeout::Milliseconds(5_000));
            notification
                .show()
                .map(|_| ())
                .map_err(|err| PlatformError::Unavailable(err.to_string()))
        }
    }
}

impl Default for OsNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub fn format_pre_break_notification_localized(
    break_type: BreakType,
    title: &str,
    warning_secs: u64,
    locale: Locale,
) -> (String, String) {
    let kind_label = match (locale, break_type) {
        (Locale::En, BreakType::Short) => "Short break",
        (Locale::En, BreakType::Long) => "Long break",
        (Locale::Vi, BreakType::Short) => "Nghỉ ngắn",
        (Locale::Vi, BreakType::Long) => "Nghỉ dài",
    };
    let summary = match locale {
        Locale::En => format!("Safe Your Eyes — {kind_label} in {warning_secs}s"),
        Locale::Vi => format!("Safe Your Eyes — {kind_label} sau {warning_secs} giây"),
    };
    let trimmed = title.trim();
    let body = if trimmed.is_empty() {
        match locale {
            Locale::En => "Prepare to rest your eyes.".to_owned(),
            Locale::Vi => "Chuẩn bị cho mắt nghỉ ngơi.".to_owned(),
        }
    } else {
        localize_exercise_name(trimmed, locale).to_owned()
    };
    (summary, body)
}

#[cfg(any(target_os = "macos", windows, test))]
#[must_use]
pub(crate) fn format_notice_accessibility_label(summary: &str, body: &str) -> String {
    let summary = summary.trim();
    let body = body.trim();
    match (summary.is_empty(), body.is_empty()) {
        (false, false) => format!("{summary}. {body}"),
        (false, true) => summary.to_owned(),
        (true, false) => body.to_owned(),
        (true, true) => String::new(),
    }
}

#[cfg(any(target_os = "macos", windows, test))]
#[must_use]
pub(crate) fn format_pre_break_banner_copy_localized(
    break_type: BreakType,
    title: &str,
    locale: Locale,
) -> (String, String, String) {
    let kind_label = match (locale, break_type) {
        (Locale::En, BreakType::Short) => "Short break",
        (Locale::En, BreakType::Long) => "Long break",
        (Locale::Vi, BreakType::Short) => "Nghỉ ngắn",
        (Locale::Vi, BreakType::Long) => "Nghỉ dài",
    };
    let trimmed = title.trim();
    let body = if trimmed.is_empty() {
        match locale {
            Locale::En => "Prepare to rest your eyes.".to_owned(),
            Locale::Vi => "Chuẩn bị cho mắt nghỉ ngơi.".to_owned(),
        }
    } else {
        localize_exercise_name(trimmed, locale).to_owned()
    };
    // The label is set once per banner, so it must not carry a countdown number that goes stale.
    let accessible = match locale {
        Locale::En => format!("Safe Your Eyes: {kind_label} starting soon. {body}"),
        Locale::Vi => format!("Safe Your Eyes: {kind_label} sắp bắt đầu. {body}"),
    };
    (kind_label.to_owned(), body, accessible)
}

#[cfg(any(windows, test))]
#[must_use]
pub(crate) const fn rgb_to_bgr(rgb: u32) -> u32 {
    ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
}

#[cfg(any(windows, test))]
#[must_use]
pub(crate) const fn compute_windows_banner_origin(
    work_area: (i32, i32, i32, i32),
    banner_size: (i32, i32),
    margin: i32,
) -> (i32, i32) {
    let (left, top, right, bottom) = work_area;
    let min_x = left + margin;
    let min_y = top + margin;
    let x = right - banner_size.0 - margin;
    let y = bottom - banner_size.1 - margin;
    (
        if x > min_x { x } else { min_x },
        if y > min_y { y } else { min_y },
    )
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
        total_secs: u64,
    ) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        let locale = self.locale();
        let countdown = PreBreakCountdown {
            remaining_secs: warning_secs,
            total_secs: total_secs.max(warning_secs).max(1),
        };

        #[cfg(target_os = "macos")]
        {
            let (banner_title, body, accessible) =
                format_pre_break_banner_copy_localized(break_type, title, locale);
            macos_impl::show_banner(&banner_title, &body, &accessible, Some(countdown))
        }

        #[cfg(windows)]
        {
            let (banner_title, body, accessible) =
                format_pre_break_banner_copy_localized(break_type, title, locale);
            windows_impl::show_banner(&banner_title, &body, &accessible, Some(countdown))
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let (summary, body) =
                format_pre_break_notification_localized(break_type, title, warning_secs, locale);
            let timeout_ms = u32::try_from(warning_secs.saturating_mul(1000)).unwrap_or(u32::MAX);
            let progress_pct = countdown.progress_percent();

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
                        .hint(Hint::CustomInt("value".to_owned(), progress_pct))
                        .timeout(Timeout::Milliseconds(timeout_ms));
                    if active.handle.update().is_ok() {
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
                .hint(Hint::CustomInt("value".to_owned(), progress_pct))
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

        #[cfg(windows)]
        windows_impl::dismiss();

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
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSEvent, NSFont, NSFontWeightBold, NSFontWeightRegular,
        NSFontWeightSemibold, NSImage, NSImageView, NSLineBreakMode, NSPanel, NSResponder,
        NSScreen, NSStatusWindowLevel, NSTextField, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowAnimationBehavior,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{
        NSData, NSObject, NSPoint, NSRect, NSSize, NSString, NSUserDefaults, ns_string,
    };
    use sye_core::PlatformError;

    use super::{PreBreakCountdown, compute_macos_banner_origin};
    use crate::tray::ScreenRect;

    const APP_ICON_PNG: &[u8] =
        include_bytes!("../../../assets/icons/hicolor/128x128/apps/safe-your-eyes.png");
    const BANNER_WIDTH: f64 = 368.0;
    const MIN_HEIGHT: f64 = 68.0;
    const CORNER_RADIUS: f64 = 16.0;
    const ICON_SIZE: f64 = 38.0;
    const ICON_X: f64 = 13.0;
    const TEXT_X: f64 = 62.0;
    const PAD_RIGHT: f64 = 14.0;
    const PAD_Y: f64 = 12.0;
    const LINE_GAP: f64 = 2.0;
    const BADGE_WIDTH: f64 = 60.0;
    const BADGE_HEIGHT: f64 = 32.0;
    const BADGE_GAP: f64 = 10.0;
    const PROGRESS_H: f64 = 3.0;
    const FULL_TEXT_WIDTH: f64 = BANNER_WIDTH - TEXT_X - PAD_RIGHT;
    const COUNTDOWN_TEXT_WIDTH: f64 = BANNER_WIDTH - TEXT_X - PAD_RIGHT - BADGE_GAP - BADGE_WIDTH;
    const MAX_FIT_HEIGHT: f64 = 10_000.0;

    static USER_DISMISSED: AtomicBool = AtomicBool::new(false);

    thread_local! {
        static BANNER: RefCell<Option<PreBreakBanner>> = const { RefCell::new(None) };
    }

    fn system_prefers_dark() -> bool {
        let defaults = NSUserDefaults::standardUserDefaults();
        defaults
            .stringForKey(ns_string!("AppleInterfaceStyle"))
            .is_some_and(|val| val.isEqualToString(ns_string!("Dark")))
    }

    fn srgb_color(hex: u32) -> Retained<NSColor> {
        let r = f64::from((hex >> 16) & 0xFF) / 255.0;
        let g = f64::from((hex >> 8) & 0xFF) / 255.0;
        let b = f64::from(hex & 0xFF) / 255.0;
        NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
    }

    fn set_layer_colors(view: &NSView, bg: &NSColor, border: Option<&NSColor>) {
        if let Some(layer) = view.layer() {
            // SAFETY: `NSColor.CGColor` returns a valid `CGColorRef` for the color's lifetime,
            // and `CALayer` retains assigned color properties.
            unsafe {
                let bg_cg: *const AnyObject = msg_send![bg, CGColor];
                let (): () = msg_send![&*layer, setBackgroundColor: bg_cg];
                if let Some(border_color) = border {
                    let border_cg: *const AnyObject = msg_send![border_color, CGColor];
                    let (): () = msg_send![&*layer, setBorderColor: border_cg];
                    layer.setBorderWidth(1.0);
                } else {
                    layer.setBorderWidth(0.0);
                }
            }
        }
    }

    define_class!(
        #[unsafe(super(NSView, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SyePreBreakBannerClickView"]
        struct BannerClickView;

        impl BannerClickView {
            #[unsafe(method(acceptsFirstMouse:))]
            fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
                true
            }

            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, _event: &NSEvent) {
                USER_DISMISSED.store(true, Ordering::Release);
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
        badge_view: Retained<NSView>,
        badge_label: Retained<NSTextField>,
        progress_track: Retained<NSView>,
        progress_fill: Retained<NSView>,
        click_shield: Retained<BannerClickView>,
        last_summary: String,
        last_body: String,
        last_has_countdown: bool,
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

            // SAFETY: AppKit font weight constants are immutable static values.
            let (title_font, body_font, badge_font) = unsafe {
                (
                    NSFont::monospacedDigitSystemFontOfSize_weight(13.0, NSFontWeightSemibold),
                    NSFont::systemFontOfSize_weight(13.0, NSFontWeightRegular),
                    NSFont::monospacedDigitSystemFontOfSize_weight(15.0, NSFontWeightBold),
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
            body_label.setPreferredMaxLayoutWidth(COUNTDOWN_TEXT_WIDTH);
            if let Some(cell) = body_label.cell() {
                cell.setTruncatesLastVisibleLine(true);
            }
            effect_view.addSubview(&body_label);

            let badge_rect = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(BADGE_WIDTH, BADGE_HEIGHT),
            );
            let badge_view = NSView::initWithFrame(NSView::alloc(mtm), badge_rect);
            badge_view.setWantsLayer(true);
            if let Some(layer) = badge_view.layer() {
                layer.setCornerRadius(BADGE_HEIGHT * 0.5);
                layer.setMasksToBounds(true);
            }
            let badge_label = NSTextField::labelWithString(&NSString::new(), mtm);
            badge_label.setFont(Some(&badge_font));
            badge_label.setMaximumNumberOfLines(1);
            badge_view.addSubview(&badge_label);
            effect_view.addSubview(&badge_view);

            let progress_rect = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(BANNER_WIDTH, PROGRESS_H),
            );
            let progress_track = NSView::initWithFrame(NSView::alloc(mtm), progress_rect);
            progress_track.setWantsLayer(true);
            effect_view.addSubview(&progress_track);

            let progress_fill = NSView::initWithFrame(NSView::alloc(mtm), progress_rect);
            progress_fill.setWantsLayer(true);
            effect_view.addSubview(&progress_fill);

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
                badge_view,
                badge_label,
                progress_track,
                progress_fill,
                click_shield,
                last_summary: String::new(),
                last_body: String::new(),
                last_has_countdown: false,
            }
        }

        fn apply_countdown_style(&self, countdown: PreBreakCountdown) {
            let dark = system_prefers_dark();
            let (bg_hex, border_hex, text_hex) = match (countdown.is_urgent(), dark) {
                (false, false) => (0xFAE1B5, 0xE5C27D, 0x5E3904),
                (false, true) => (0x443A26, 0xC99A44, 0xFCE09B),
                (true, false) => (0xEF8F63, 0xA84A24, 0x261101),
                (true, true) => (0xEE8F64, 0xF2A57E, 0x18140E),
            };
            let (track_hex, fill_hex) = if dark {
                (0x30271E, 0xEE8F64)
            } else {
                (0xE6D2BC, 0xEF8F63)
            };

            let badge_bg = srgb_color(bg_hex);
            let badge_border = srgb_color(border_hex);
            let badge_text = srgb_color(text_hex);
            set_layer_colors(&self.badge_view, &badge_bg, Some(&badge_border));
            self.badge_label.setTextColor(Some(&badge_text));

            let badge_str = countdown.badge_text();
            self.badge_label
                .setStringValue(&NSString::from_str(&badge_str));
            let fit = self
                .badge_label
                .sizeThatFits(NSSize::new(BADGE_WIDTH - 8.0, BADGE_HEIGHT));
            let lbl_w = fit.width.ceil().min(BADGE_WIDTH - 8.0);
            let lbl_h = fit.height.ceil().min(BADGE_HEIGHT);
            self.badge_label.setFrame(NSRect::new(
                NSPoint::new(
                    ((BADGE_WIDTH - lbl_w) * 0.5).round(),
                    ((BADGE_HEIGHT - lbl_h) * 0.5).round(),
                ),
                NSSize::new(lbl_w, lbl_h),
            ));

            let track_color = srgb_color(track_hex);
            let fill_color = srgb_color(fill_hex);
            set_layer_colors(&self.progress_track, &track_color, None);
            set_layer_colors(&self.progress_fill, &fill_color, None);
            let fill_w = (BANNER_WIDTH * countdown.progress_ratio()).round();
            self.progress_fill.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(fill_w, PROGRESS_H),
            ));
        }

        fn show_or_update(
            &mut self,
            summary: &str,
            body: &str,
            accessible_label: &str,
            countdown: Option<PreBreakCountdown>,
            mtm: MainThreadMarker,
        ) -> Result<(), PlatformError> {
            let summary_changed = self.last_summary != summary;
            let body_changed = self.last_body != body;
            let has_countdown = countdown.is_some();
            let mode_changed = self.last_has_countdown != has_countdown;
            let was_visible = self.panel.isVisible();

            if summary_changed {
                self.last_summary.clear();
                self.last_summary.push_str(summary);
                self.title_label
                    .setStringValue(&NSString::from_str(summary));
            }
            if !was_visible || summary_changed || body_changed || mode_changed {
                let a11y = NSString::from_str(accessible_label);
                // SAFETY: `NSView` implements `setAccessibilityLabel:` via `NSAccessibility`.
                unsafe {
                    let (): () = msg_send![&*self.effect_view, setAccessibilityLabel: &*a11y];
                }
            }
            let visible = active_screen_visible_frame(mtm).ok_or_else(|| {
                PlatformError::Unavailable("macOS display screen unavailable".to_owned())
            })?;
            let prev_frame = self.panel.frame();
            let needs_layout = !was_visible || body_changed || mode_changed;
            let mut banner_size = prev_frame.size;
            if needs_layout {
                self.last_has_countdown = has_countdown;
                let text_width = if has_countdown {
                    COUNTDOWN_TEXT_WIDTH
                } else {
                    FULL_TEXT_WIDTH
                };
                self.body_label.setPreferredMaxLayoutWidth(text_width);
                if body_changed {
                    self.last_body.clear();
                    self.last_body.push_str(body);
                    self.body_label.setStringValue(&NSString::from_str(body));
                }

                let fit_bounds = NSSize::new(text_width, MAX_FIT_HEIGHT);
                let title_h = self.title_label.sizeThatFits(fit_bounds).height.ceil();
                let body_h = self.body_label.sizeThatFits(fit_bounds).height.ceil();
                let text_block_h = title_h + LINE_GAP + body_h;
                let bottom_inset = if has_countdown { PROGRESS_H } else { 0.0 };
                let banner_h = (text_block_h + PAD_Y * 2.0 + bottom_inset)
                    .max(MIN_HEIGHT)
                    .ceil();
                let content_h = banner_h - bottom_inset;
                banner_size = NSSize::new(BANNER_WIDTH, banner_h);

                self.effect_view
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), banner_size));
                self.click_shield
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), banner_size));
                if let Some(icon_view) = &self.icon_view {
                    let icon_y = (bottom_inset + (content_h - ICON_SIZE) * 0.5).round();
                    icon_view.setFrame(NSRect::new(
                        NSPoint::new(ICON_X, icon_y),
                        NSSize::new(ICON_SIZE, ICON_SIZE),
                    ));
                }

                let body_y = (bottom_inset + (content_h - text_block_h) * 0.5).round();
                let title_y = body_y + body_h + LINE_GAP;
                self.body_label.setFrame(NSRect::new(
                    NSPoint::new(TEXT_X, body_y),
                    NSSize::new(text_width, body_h),
                ));
                self.title_label.setFrame(NSRect::new(
                    NSPoint::new(TEXT_X, title_y),
                    NSSize::new(text_width, title_h),
                ));

                self.badge_view.setHidden(!has_countdown);
                self.progress_track.setHidden(!has_countdown);
                self.progress_fill.setHidden(!has_countdown);
                if has_countdown {
                    let badge_x = BANNER_WIDTH - PAD_RIGHT - BADGE_WIDTH;
                    let badge_y = (bottom_inset + (content_h - BADGE_HEIGHT) * 0.5).round();
                    self.badge_view.setFrame(NSRect::new(
                        NSPoint::new(badge_x, badge_y),
                        NSSize::new(BADGE_WIDTH, BADGE_HEIGHT),
                    ));
                    self.progress_track.setFrame(NSRect::new(
                        NSPoint::new(0.0, 0.0),
                        NSSize::new(BANNER_WIDTH, PROGRESS_H),
                    ));
                }
            }

            if let Some(cd) = countdown {
                self.apply_countdown_style(cd);
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

    pub(super) fn show_banner(
        summary: &str,
        body: &str,
        accessible_label: &str,
        countdown: Option<PreBreakCountdown>,
    ) -> Result<(), PlatformError> {
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
                banner.show_or_update(summary, body, accessible_label, countdown, mtm)
            })
        })
    }

    pub(super) fn dismiss() {
        USER_DISMISSED.store(false, Ordering::Release);
        clear_banner();
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 non-activating popup banner for in-place pre-break countdown"
)]
mod windows_impl {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use sye_core::PlatformError;
    use windows_sys::Win32::Foundation::{
        COLORREF, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT,
        WPARAM,
    };
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, COLOR_GRAYTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW, CreatePen,
        CreateSolidBrush, DRAW_TEXT_FORMAT, DT_CALCRECT, DT_CENTER, DT_EDITCONTROL,
        DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteObject,
        DrawTextW, EndPaint, FillRect, GetDC, GetMonitorInfoW, GetSysColor, GetTextMetricsW, HDC,
        HFONT, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
        PAINTSTRUCT, PS_SOLID, ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor,
        TEXTMETRICW, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
        MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext, SystemParametersInfoForDpi,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_DROPSHADOW, CreateWindowExW, DI_NORMAL, DefWindowProcW, DestroyIcon, DestroyWindow,
        DrawIconEx, GetClientRect, GetCursorPos, HICON, HWND_TOPMOST, IDC_ARROW, IMAGE_ICON,
        IsWindowVisible, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW, MA_NOACTIVATE,
        NONCLIENTMETRICSW, RegisterClassExW, SPI_GETNONCLIENTMETRICS, SW_HIDE, SWP_NOACTIVATE,
        SWP_SHOWWINDOW, SetWindowPos, SetWindowTextW, ShowWindow, WM_ERASEBKGND, WM_LBUTTONDOWN,
        WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    };

    use super::{PreBreakCountdown, compute_windows_banner_origin, rgb_to_bgr};

    const CLASS_NAME: &str = "SyePreBreakBanner";
    const WINDOW_TITLE: &str = "Safe Your Eyes";
    const APP_ICON_RESOURCE_ID: usize = 1;
    const BASE_DPI: i32 = 96;
    const BANNER_WIDTH: i32 = 368;
    const MIN_HEIGHT: i32 = 68;
    const ICON_SIZE: i32 = 38;
    const ICON_X: i32 = 13;
    const TEXT_X: i32 = 62;
    const PAD_RIGHT: i32 = 14;
    const PAD_Y: i32 = 12;
    const LINE_GAP: i32 = 2;
    const BADGE_WIDTH: i32 = 60;
    const BADGE_HEIGHT: i32 = 32;
    const BADGE_GAP: i32 = 10;
    const PROGRESS_H: i32 = 3;
    const MARGIN: i32 = 12;
    const BODY_MAX_LINES: i32 = 2;
    const TITLE_FONT_WEIGHT: i32 = 600;
    const BADGE_FONT_WEIGHT: i32 = 700;
    const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

    struct Palette {
        background: COLORREF,
        title: COLORREF,
        body: COLORREF,
        badge_bg: COLORREF,
        badge_border: COLORREF,
        badge_text: COLORREF,
        urgent_bg: COLORREF,
        urgent_border: COLORREF,
        urgent_text: COLORREF,
        track: COLORREF,
        fill: COLORREF,
    }

    const DARK_PALETTE: Palette = Palette {
        background: 0x002C_2C2C,
        title: 0x00FF_FFFF,
        body: 0x00CF_CFCF,
        badge_bg: rgb_to_bgr(0x443A26),
        badge_border: rgb_to_bgr(0xC99A44),
        badge_text: rgb_to_bgr(0xFCE09B),
        urgent_bg: rgb_to_bgr(0xEE8F64),
        urgent_border: rgb_to_bgr(0xF2A57E),
        urgent_text: rgb_to_bgr(0x18140E),
        track: rgb_to_bgr(0x30271E),
        fill: rgb_to_bgr(0xEE8F64),
    };

    static USER_DISMISSED: AtomicBool = AtomicBool::new(false);

    thread_local! {
        static BANNER: RefCell<Option<Banner>> = const { RefCell::new(None) };
    }

    const fn size_u32<T>() -> u32 {
        size_of::<T>() as u32
    }

    const fn scale(value: i32, dpi: i32) -> i32 {
        value * dpi / BASE_DPI
    }

    fn wide_z(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn draw_text(hdc: HDC, text: &[u16], rect: &mut RECT, format: DRAW_TEXT_FORMAT) -> i32 {
        let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
        // SAFETY: `DrawTextW` writes to the text buffer only with `DT_MODIFYSTRING`, which no
        // caller passes, and `rect` is a valid writable `RECT`.
        unsafe { DrawTextW(hdc, text.as_ptr().cast_mut(), len, rect, format) }
    }

    fn apps_use_dark_theme() -> bool {
        let key = wide_z(PERSONALIZE_KEY);
        let value = wide_z(APPS_USE_LIGHT_THEME);
        let mut data: u32 = 1;
        let mut size = size_u32::<u32>();
        // SAFETY: both names are NUL-terminated and `data` is writable storage of `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&raw mut data).cast::<c_void>(),
                &mut size,
            )
        };
        status == ERROR_SUCCESS && data == 0
    }

    fn palette(dark: bool) -> Palette {
        if dark {
            return DARK_PALETTE;
        }
        // SAFETY: `GetSysColor` only reads the current system color table.
        unsafe {
            Palette {
                background: GetSysColor(COLOR_WINDOW),
                title: GetSysColor(COLOR_WINDOWTEXT),
                body: GetSysColor(COLOR_GRAYTEXT),
                badge_bg: rgb_to_bgr(0xFAE1B5),
                badge_border: rgb_to_bgr(0xE5C27D),
                badge_text: rgb_to_bgr(0x5E3904),
                urgent_bg: rgb_to_bgr(0xEF8F63),
                urgent_border: rgb_to_bgr(0xA84A24),
                urgent_text: rgb_to_bgr(0x261101),
                track: rgb_to_bgr(0xE6D2BC),
                fill: rgb_to_bgr(0xEF8F63),
            }
        }
    }

    fn message_font(dpi: i32, weight: Option<i32>, height_pct: i32) -> HFONT {
        // SAFETY: all-zero bytes are a valid `NONCLIENTMETRICSW` value.
        let mut metrics: NONCLIENTMETRICSW = unsafe { zeroed() };
        metrics.cbSize = size_u32::<NONCLIENTMETRICSW>();
        // SAFETY: `metrics` is writable storage of `cbSize` bytes. On failure the zeroed
        // `LOGFONTW` still yields the default GUI font.
        unsafe {
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                (&raw mut metrics).cast::<c_void>(),
                0,
                dpi.unsigned_abs(),
            );
        }
        let mut font = metrics.lfMessageFont;
        if let Some(weight) = weight {
            font.lfWeight = weight;
        }
        if height_pct != 100 && font.lfHeight != 0 {
            font.lfHeight = (font.lfHeight * height_pct) / 100;
        }
        // SAFETY: `font` is a fully initialized `LOGFONTW`.
        unsafe { CreateFontIndirectW(&font) }
    }

    struct DpiScope(DPI_AWARENESS_CONTEXT);

    impl DpiScope {
        fn per_monitor() -> Self {
            // SAFETY: the predefined context constant is always a valid argument.
            Self(unsafe {
                SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            })
        }
    }

    impl Drop for DpiScope {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: `self.0` is the context that `SetThreadDpiAwarenessContext` returned.
                unsafe {
                    SetThreadDpiAwarenessContext(self.0);
                }
            }
        }
    }

    struct Banner {
        hwnd: HWND,
        hinstance: HINSTANCE,
        dpi: i32,
        dark: Option<bool>,
        icon: HICON,
        title_font: HFONT,
        body_font: HFONT,
        badge_font: HFONT,
        summary: Vec<u16>,
        body: Vec<u16>,
        badge_text: Vec<u16>,
        countdown: Option<PreBreakCountdown>,
        size: (i32, i32),
        icon_rect: RECT,
        title_rect: RECT,
        body_rect: RECT,
        badge_rect: RECT,
        progress_track_rect: RECT,
    }

    impl Banner {
        fn new() -> Result<Self, PlatformError> {
            // SAFETY: a null module name returns the handle of the current executable.
            let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
            let class_name = wide_z(CLASS_NAME);
            let window_title = wide_z(WINDOW_TITLE);
            // SAFETY: all-zero bytes are a valid `WNDCLASSEXW` (null handles, no window procedure).
            let mut class: WNDCLASSEXW = unsafe { zeroed() };
            class.cbSize = size_u32::<WNDCLASSEXW>();
            class.style = CS_DROPSHADOW;
            class.lpfnWndProc = Some(wnd_proc);
            class.hInstance = hinstance;
            // SAFETY: `IDC_ARROW` is a predefined system cursor.
            class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
            class.lpszClassName = class_name.as_ptr();
            // SAFETY: `class` and `class_name` stay alive for the call. Registration fails with
            // `ERROR_CLASS_ALREADY_EXISTS` after the first banner, which is harmless.
            unsafe {
                RegisterClassExW(&class);
            }

            // SAFETY: the class is registered above and both strings are NUL-terminated.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    class_name.as_ptr(),
                    window_title.as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    hinstance,
                    ptr::null(),
                )
            };
            if hwnd.is_null() {
                // SAFETY: reads the calling thread's last-error value.
                let code = unsafe { GetLastError() };
                return Err(PlatformError::Unavailable(format!(
                    "Windows pre-break banner creation failed (error {code})"
                )));
            }

            let corner = DWMWCP_ROUND;
            // SAFETY: `corner` outlives the call and the size matches its type. Windows 10 rejects
            // the attribute and keeps square corners.
            unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                    (&raw const corner).cast::<c_void>(),
                    size_u32::<i32>(),
                );
            }

            Ok(Self {
                hwnd,
                hinstance,
                dpi: 0,
                dark: None,
                icon: ptr::null_mut(),
                title_font: ptr::null_mut(),
                body_font: ptr::null_mut(),
                badge_font: ptr::null_mut(),
                summary: Vec::new(),
                body: Vec::new(),
                badge_text: Vec::new(),
                countdown: None,
                size: (0, 0),
                icon_rect: RECT::default(),
                title_rect: RECT::default(),
                body_rect: RECT::default(),
                badge_rect: RECT::default(),
                progress_track_rect: RECT::default(),
            })
        }

        fn release_resources(&mut self) {
            // SAFETY: each handle is either null or owned by this banner and not selected into a DC.
            unsafe {
                if !self.title_font.is_null() {
                    DeleteObject(self.title_font);
                }
                if !self.body_font.is_null() {
                    DeleteObject(self.body_font);
                }
                if !self.badge_font.is_null() {
                    DeleteObject(self.badge_font);
                }
                if !self.icon.is_null() {
                    DestroyIcon(self.icon);
                }
            }
            self.title_font = ptr::null_mut();
            self.body_font = ptr::null_mut();
            self.badge_font = ptr::null_mut();
            self.icon = ptr::null_mut();
        }

        fn apply_theme(&mut self, dark: bool) {
            self.dark = Some(dark);
            let enabled = i32::from(dark);
            // SAFETY: `enabled` outlives the call and the size matches its `BOOL` type. The dark
            // attribute only recolors the DWM border and shadow.
            unsafe {
                DwmSetWindowAttribute(
                    self.hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    (&raw const enabled).cast::<c_void>(),
                    size_u32::<i32>(),
                );
            }
        }

        fn apply_dpi(&mut self, dpi: i32) {
            self.release_resources();
            self.dpi = dpi;
            self.title_font = message_font(dpi, Some(TITLE_FONT_WEIGHT), 100);
            self.body_font = message_font(dpi, None, 100);
            self.badge_font = message_font(dpi, Some(BADGE_FONT_WEIGHT), 115);
            let icon_px = scale(ICON_SIZE, dpi);
            // SAFETY: the resource ID is a valid `MAKEINTRESOURCEW` value. Test executables have
            // no icon resource and get null, so the banner draws text only.
            self.icon = unsafe {
                LoadImageW(
                    self.hinstance,
                    ptr::without_provenance(APP_ICON_RESOURCE_ID),
                    IMAGE_ICON,
                    icon_px,
                    icon_px,
                    LR_DEFAULTCOLOR,
                )
            };
        }

        fn measure(&self, hdc: HDC, font: HFONT, text: &[u16], format: DRAW_TEXT_FORMAT) -> i32 {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: self.text_width(),
                bottom: 0,
            };
            // SAFETY: `hdc` is a live DC and `font` a live font owned by this banner.
            let previous = unsafe { SelectObject(hdc, font) };
            draw_text(hdc, text, &mut rect, format | DT_CALCRECT | DT_NOPREFIX);
            // SAFETY: restores the object that `SelectObject` returned.
            unsafe { SelectObject(hdc, previous) };
            rect.bottom
        }

        fn line_height(hdc: HDC, font: HFONT) -> i32 {
            // SAFETY: all-zero bytes are a valid `TEXTMETRICW` value.
            let mut metrics: TEXTMETRICW = unsafe { zeroed() };
            // SAFETY: `hdc` is a live DC, `font` a live font, and `metrics` writable storage.
            unsafe {
                let previous = SelectObject(hdc, font);
                GetTextMetricsW(hdc, &mut metrics);
                SelectObject(hdc, previous);
            }
            metrics.tmHeight
        }

        const fn text_width(&self) -> i32 {
            let logical = if self.countdown.is_some() {
                BANNER_WIDTH - TEXT_X - PAD_RIGHT - BADGE_GAP - BADGE_WIDTH
            } else {
                BANNER_WIDTH - TEXT_X - PAD_RIGHT
            };
            scale(logical, self.dpi)
        }

        fn layout(&mut self) {
            let dpi = self.dpi;
            let has_countdown = self.countdown.is_some();
            // SAFETY: `self.hwnd` is the live banner window.
            let hdc = unsafe { GetDC(self.hwnd) };
            let title_h = self.measure(hdc, self.title_font, &self.summary, DT_SINGLELINE);
            let body_full_h = self.measure(hdc, self.body_font, &self.body, DT_WORDBREAK);
            let body_line_h = Self::line_height(hdc, self.body_font);
            // SAFETY: releases the DC that `GetDC` returned for this window.
            unsafe { ReleaseDC(self.hwnd, hdc) };

            let body_h = body_full_h.min(body_line_h * BODY_MAX_LINES);
            let gap = scale(LINE_GAP, dpi);
            let progress_h = if has_countdown {
                scale(PROGRESS_H, dpi).max(1)
            } else {
                0
            };
            let text_block_h = title_h + gap + body_h;
            let width = scale(BANNER_WIDTH, dpi);
            let height =
                (text_block_h + scale(PAD_Y, dpi) * 2 + progress_h).max(scale(MIN_HEIGHT, dpi));
            let content_h = height - progress_h;
            self.size = (width, height);

            let icon = scale(ICON_SIZE, dpi);
            let icon_x = scale(ICON_X, dpi);
            let icon_y = (content_h - icon) / 2;
            self.icon_rect = RECT {
                left: icon_x,
                top: icon_y,
                right: icon_x + icon,
                bottom: icon_y + icon,
            };

            let text_x = scale(TEXT_X, dpi);
            let text_right = text_x + self.text_width();
            let title_top = (content_h - text_block_h) / 2;
            self.title_rect = RECT {
                left: text_x,
                top: title_top,
                right: text_right,
                bottom: title_top + title_h,
            };
            let body_top = title_top + title_h + gap;
            self.body_rect = RECT {
                left: text_x,
                top: body_top,
                right: text_right,
                bottom: body_top + body_h,
            };

            if has_countdown {
                let badge_w = scale(BADGE_WIDTH, dpi);
                let badge_h = scale(BADGE_HEIGHT, dpi);
                let badge_left = width - scale(PAD_RIGHT, dpi) - badge_w;
                let badge_top = (content_h - badge_h) / 2;
                self.badge_rect = RECT {
                    left: badge_left,
                    top: badge_top,
                    right: badge_left + badge_w,
                    bottom: badge_top + badge_h,
                };
                self.progress_track_rect = RECT {
                    left: 0,
                    top: height - progress_h,
                    right: width,
                    bottom: height,
                };
            } else {
                self.badge_rect = RECT::default();
                self.progress_track_rect = RECT::default();
            }
        }

        fn show_or_update(
            &mut self,
            summary: &str,
            body: &str,
            accessible_label: &str,
            countdown: Option<PreBreakCountdown>,
        ) -> Result<(), PlatformError> {
            let mut cursor = POINT { x: 0, y: 0 };
            // SAFETY: `cursor` is writable storage for a `POINT`.
            unsafe { GetCursorPos(&mut cursor) };
            // SAFETY: `MONITOR_DEFAULTTOPRIMARY` always returns a monitor handle.
            let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY) };
            // SAFETY: all-zero bytes are a valid `MONITORINFO` value.
            let mut info: MONITORINFO = unsafe { zeroed() };
            info.cbSize = size_u32::<MONITORINFO>();
            // SAFETY: `monitor` is non-null and `info.cbSize` is initialized.
            if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
                return Err(PlatformError::Unavailable(
                    "Windows display monitor unavailable".to_owned(),
                ));
            }
            let (mut dpi_x, mut dpi_y) = (BASE_DPI.unsigned_abs(), BASE_DPI.unsigned_abs());
            // SAFETY: `monitor` is non-null and both outputs are writable. On failure the
            // outputs keep the 96 DPI default.
            unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
            let dpi = i32::try_from(dpi_x).unwrap_or(BASE_DPI);

            let summary: Vec<u16> = summary.encode_utf16().collect();
            let body: Vec<u16> = body.encode_utf16().collect();
            let summary_changed = self.summary != summary;
            let body_changed = self.body != body;
            let mode_changed = self.countdown.is_some() != countdown.is_some();
            let dpi_changed = self.dpi != dpi;
            // SAFETY: `self.hwnd` is the live banner window.
            let was_visible = unsafe { IsWindowVisible(self.hwnd) } != 0;
            self.summary = summary;
            self.body = body;
            self.countdown = countdown;
            self.badge_text =
                countdown.map_or_else(Vec::new, |cd| cd.badge_text().encode_utf16().collect());
            if !was_visible || summary_changed || body_changed || mode_changed {
                let a11y_title = wide_z(accessible_label);
                // SAFETY: `self.hwnd` is live and `a11y_title` is NUL-terminated UTF-16.
                unsafe { SetWindowTextW(self.hwnd, a11y_title.as_ptr()) };
            }
            if dpi_changed {
                self.apply_dpi(dpi);
            }
            let dark = apps_use_dark_theme();
            if self.dark != Some(dark) {
                self.apply_theme(dark);
            }
            if !was_visible || body_changed || mode_changed || dpi_changed {
                self.layout();
            }

            let work = info.rcWork;
            let (x, y) = compute_windows_banner_origin(
                (work.left, work.top, work.right, work.bottom),
                self.size,
                scale(MARGIN, self.dpi),
            );
            let show_flag = if was_visible { 0 } else { SWP_SHOWWINDOW };
            // SAFETY: `self.hwnd` is the live banner window. `SWP_NOACTIVATE` keeps the
            // foreground app focused.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    x,
                    y,
                    self.size.0,
                    self.size.1,
                    SWP_NOACTIVATE | show_flag,
                );
            }
            if was_visible {
                // SAFETY: `self.hwnd` is the live banner window.
                unsafe { InvalidateRect(self.hwnd, ptr::null(), 0) };
            }
            Ok(())
        }

        fn paint(&self, hdc: HDC) {
            let mut client = RECT::default();
            let mut title_rect = self.title_rect;
            let mut body_rect = self.body_rect;
            let colors = palette(self.dark.unwrap_or(false));
            // SAFETY: `hdc` comes from `BeginPaint` for `self.hwnd`, and every handle is live.
            unsafe {
                GetClientRect(self.hwnd, &mut client);
                let background = CreateSolidBrush(colors.background);
                FillRect(hdc, &client, background);
                DeleteObject(background);
                if !self.icon.is_null() {
                    DrawIconEx(
                        hdc,
                        self.icon_rect.left,
                        self.icon_rect.top,
                        self.icon,
                        self.icon_rect.right - self.icon_rect.left,
                        self.icon_rect.bottom - self.icon_rect.top,
                        0,
                        ptr::null_mut(),
                        DI_NORMAL,
                    );
                }
                SetBkMode(hdc, TRANSPARENT as i32);
                let previous = SelectObject(hdc, self.title_font);
                SetTextColor(hdc, colors.title);
                draw_text(
                    hdc,
                    &self.summary,
                    &mut title_rect,
                    DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                SelectObject(hdc, self.body_font);
                SetTextColor(hdc, colors.body);
                draw_text(
                    hdc,
                    &self.body,
                    &mut body_rect,
                    DT_WORDBREAK | DT_EDITCONTROL | DT_END_ELLIPSIS | DT_NOPREFIX,
                );

                if let Some(cd) = self.countdown {
                    let (badge_bg, badge_border, badge_fg) = if cd.is_urgent() {
                        (colors.urgent_bg, colors.urgent_border, colors.urgent_text)
                    } else {
                        (colors.badge_bg, colors.badge_border, colors.badge_text)
                    };
                    let mut badge_rect = self.badge_rect;
                    let pill_h = badge_rect.bottom - badge_rect.top;
                    let badge_brush = CreateSolidBrush(badge_bg);
                    let border_w = scale(1, self.dpi).max(1);
                    let badge_pen = CreatePen(PS_SOLID, border_w, badge_border);
                    let prev_brush = SelectObject(hdc, badge_brush);
                    let prev_pen = SelectObject(hdc, badge_pen);
                    RoundRect(
                        hdc,
                        badge_rect.left,
                        badge_rect.top,
                        badge_rect.right,
                        badge_rect.bottom,
                        pill_h,
                        pill_h,
                    );
                    SelectObject(hdc, prev_pen);
                    SelectObject(hdc, prev_brush);
                    DeleteObject(badge_pen);
                    DeleteObject(badge_brush);

                    SelectObject(hdc, self.badge_font);
                    SetTextColor(hdc, badge_fg);
                    draw_text(
                        hdc,
                        &self.badge_text,
                        &mut badge_rect,
                        DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX,
                    );

                    let track_brush = CreateSolidBrush(colors.track);
                    FillRect(hdc, &self.progress_track_rect, track_brush);
                    DeleteObject(track_brush);

                    let track_w = self.progress_track_rect.right - self.progress_track_rect.left;
                    let fill_w = (f64::from(track_w) * cd.progress_ratio()).round() as i32;
                    if fill_w > 0 {
                        let fill_rect = RECT {
                            left: self.progress_track_rect.left,
                            top: self.progress_track_rect.top,
                            right: (self.progress_track_rect.left + fill_w)
                                .min(self.progress_track_rect.right),
                            bottom: self.progress_track_rect.bottom,
                        };
                        let fill_brush = CreateSolidBrush(colors.fill);
                        FillRect(hdc, &fill_rect, fill_brush);
                        DeleteObject(fill_brush);
                    }
                }
                SelectObject(hdc, previous);
            }
        }
    }

    impl Drop for Banner {
        fn drop(&mut self) {
            // SAFETY: `self.hwnd` is the live banner window owned by this value.
            unsafe {
                DestroyWindow(self.hwnd);
            }
            self.release_resources();
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_LBUTTONDOWN => {
                USER_DISMISSED.store(true, Ordering::Release);
                // Hide only: `DestroyWindow` here would free the window while it dispatches.
                // SAFETY: `hwnd` is the live banner window that received this message.
                unsafe { ShowWindow(hwnd, SW_HIDE) };
                0
            }
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let painted = BANNER.with(|slot| {
                    let Ok(guard) = slot.try_borrow() else {
                        return false;
                    };
                    let Some(banner) = guard.as_ref().filter(|b| b.hwnd == hwnd) else {
                        return false;
                    };
                    // SAFETY: all-zero bytes are a valid `PAINTSTRUCT` value.
                    let mut paint: PAINTSTRUCT = unsafe { zeroed() };
                    // SAFETY: `hwnd` is the live banner window and `paint` is writable storage.
                    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
                    banner.paint(hdc);
                    // SAFETY: ends the paint cycle that `BeginPaint` started.
                    unsafe { EndPaint(hwnd, &paint) };
                    true
                });
                if painted {
                    0
                } else {
                    // SAFETY: forwards the original message arguments.
                    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
                }
            }
            // SAFETY: forwards the original message arguments.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn clear_banner() {
        let banner = BANNER.with(|slot| slot.try_borrow_mut().ok().and_then(|mut g| g.take()));
        drop(banner);
    }

    pub(super) fn show_banner(
        summary: &str,
        body: &str,
        accessible_label: &str,
        countdown: Option<PreBreakCountdown>,
    ) -> Result<(), PlatformError> {
        if USER_DISMISSED.load(Ordering::Acquire) {
            clear_banner();
            return Ok(());
        }
        let _dpi_scope = DpiScope::per_monitor();
        BANNER.with(|slot| {
            let mut guard = slot.try_borrow_mut().map_err(|_| {
                PlatformError::Unavailable("Windows pre-break banner busy".to_owned())
            })?;
            if guard.is_none() {
                *guard = Some(Banner::new()?);
            }
            guard.as_mut().map_or(Ok(()), |banner| {
                banner.show_or_update(summary, body, accessible_label, countdown)
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
        let (summary, body) =
            format_pre_break_notification_localized(BreakType::Short, "Blink", 10, Locale::En);
        assert_eq!(summary, "Safe Your Eyes — Short break in 10s");
        assert_eq!(body, "Blink");

        let (long_summary, long_body) =
            format_pre_break_notification_localized(BreakType::Long, "   ", 15, Locale::En);
        assert_eq!(long_summary, "Safe Your Eyes — Long break in 15s");
        assert_eq!(long_body, "Prepare to rest your eyes.");
    }

    #[test]
    fn format_pre_break_banner_copy_formats_title_body_and_accessibility_label() {
        let (title, body, a11y) =
            format_pre_break_banner_copy_localized(BreakType::Short, "Blink", Locale::En);
        assert_eq!(title, "Short break");
        assert_eq!(body, "Blink");
        assert_eq!(a11y, "Safe Your Eyes: Short break starting soon. Blink");

        let (long_title, long_body, long_a11y) =
            format_pre_break_banner_copy_localized(BreakType::Long, "  ", Locale::En);
        assert_eq!(long_title, "Long break");
        assert_eq!(long_body, "Prepare to rest your eyes.");
        assert_eq!(
            long_a11y,
            "Safe Your Eyes: Long break starting soon. Prepare to rest your eyes."
        );
    }

    #[test]
    fn format_pre_break_localized_translates_vietnamese_and_preserves_custom_title() {
        let (summary, body) = format_pre_break_notification_localized(
            BreakType::Short,
            "Gently close your eyes",
            10,
            Locale::Vi,
        );
        assert_eq!(summary, "Safe Your Eyes — Nghỉ ngắn sau 10 giây");
        assert_eq!(body, "Nhẹ nhàng nhắm mắt lại");

        let (custom_summary, custom_body) = format_pre_break_notification_localized(
            BreakType::Long,
            "Uống trà nóng",
            15,
            Locale::Vi,
        );
        assert_eq!(custom_summary, "Safe Your Eyes — Nghỉ dài sau 15 giây");
        assert_eq!(custom_body, "Uống trà nóng");

        let (title, body, a11y) =
            format_pre_break_banner_copy_localized(BreakType::Short, "Have some water", Locale::Vi);
        assert_eq!(title, "Nghỉ ngắn");
        assert_eq!(body, "Uống một ngụm nước");
        assert_eq!(
            a11y,
            "Safe Your Eyes: Nghỉ ngắn sắp bắt đầu. Uống một ngụm nước"
        );

        let notifier = OsNotifier::with_locale(Locale::Vi);
        assert_eq!(notifier.locale(), Locale::Vi);
        notifier.set_locale(Locale::En);
        assert_eq!(notifier.locale(), Locale::En);
    }

    #[test]
    fn format_notice_accessibility_label_combines_non_empty_parts() {
        assert_eq!(
            format_notice_accessibility_label("Update failed", "Check your connection"),
            "Update failed. Check your connection"
        );
        assert_eq!(
            format_notice_accessibility_label("Update available", "  "),
            "Update available"
        );
        assert_eq!(
            format_notice_accessibility_label("", "Ready to restart"),
            "Ready to restart"
        );
    }

    #[test]
    fn pre_break_countdown_tracks_progress_urgency_and_badge_text() {
        let at = |remaining_secs, total_secs| PreBreakCountdown {
            remaining_secs,
            total_secs,
        };

        let t10 = at(10, 10);
        assert_eq!(
            t10,
            PreBreakCountdown {
                remaining_secs: 10,
                total_secs: 10
            }
        );
        assert!(!t10.is_urgent());
        assert_eq!(t10.badge_text(), "10s");
        assert_eq!(t10.progress_percent(), 100);

        let t5 = at(5, 10);
        assert_eq!(
            t5,
            PreBreakCountdown {
                remaining_secs: 5,
                total_secs: 10
            }
        );
        assert!(!t5.is_urgent());
        assert_eq!(t5.progress_percent(), 50);

        let t3 = at(3, 10);
        assert!(t3.is_urgent());
        assert_eq!(t3.badge_text(), "3s");
        assert_eq!(t3.progress_percent(), 30);

        let long_warn = PreBreakCountdown {
            remaining_secs: 1000,
            total_secs: 1000,
        };
        assert_eq!(long_warn.badge_text(), "16:40");
        assert_eq!(
            PreBreakCountdown {
                remaining_secs: 60,
                total_secs: 60,
            }
            .badge_text(),
            "1:00"
        );
        assert_eq!(
            PreBreakCountdown {
                remaining_secs: 5999,
                total_secs: 6000,
            }
            .badge_text(),
            "99:59"
        );
        assert_eq!(
            PreBreakCountdown {
                remaining_secs: 6000,
                total_secs: 6000,
            }
            .badge_text(),
            "100m"
        );

        // A late first notification still measures against the full warning window.
        assert_eq!(at(7, 10).progress_percent(), 70);
    }

    #[test]
    fn rgb_to_bgr_converts_cozy_colors_for_win32_colorref() {
        assert_eq!(rgb_to_bgr(0xEF8F63), 0x0063_8FEF);
        assert_eq!(rgb_to_bgr(0xFAE1B5), 0x00B5_E1FA);
        assert_eq!(rgb_to_bgr(0x261101), 0x0001_1126);
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
            compute_macos_banner_origin(visible, (368.0, 68.0)),
            (1132.0, 878.0)
        );

        let tiny = ScreenRect {
            x: 100.0,
            y: 40.0,
            width: 300.0,
            height: 60.0,
        };
        assert_eq!(
            compute_macos_banner_origin(tiny, (368.0, 68.0)),
            (112.0, 52.0)
        );
    }

    #[test]
    fn windows_banner_anchors_bottom_right_inside_work_area() {
        assert_eq!(
            compute_windows_banner_origin((0, 0, 1920, 1032), (368, 68), 12),
            (1540, 952)
        );
        assert_eq!(
            compute_windows_banner_origin((-1280, 0, 0, 984), (460, 85), 15),
            (-475, 884)
        );
        assert_eq!(
            compute_windows_banner_origin((100, 40, 400, 100), (368, 68), 12),
            (112, 52)
        );
    }

    #[test]
    fn noop_notifier_succeeds_without_display_or_dbus() -> Result<(), Box<dyn std::error::Error>> {
        let notifier = OsNotifier::noop();
        notifier.show_pre_break(BreakType::Short, "Look away", 10, 10)?;
        notifier.dismiss()?;
        Ok(())
    }

    #[test]
    fn os_notifier_degrades_gracefully_in_headless_env() {
        let notifier = OsNotifier::new();
        let res = notifier.show_pre_break(BreakType::Short, "Look away", 5, 10);
        assert!(matches!(res, Ok(()) | Err(PlatformError::Unavailable(_))));
        assert!(notifier.dismiss().is_ok());
    }
}
