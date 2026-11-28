#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(any(target_os = "macos", windows, test))]
use std::time::Instant;

#[cfg(all(unix, not(target_os = "macos")))]
use notify_rust::{Hint, Notification, Timeout};
#[cfg(any(target_os = "macos", windows, test))]
use sye_core::cozy;
use sye_core::{BreakType, Locale, PlatformError, ThemeMode, localize_exercise_name};

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

const fn encode_theme_mode(mode: ThemeMode) -> u8 {
    match mode {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    }
}

const fn decode_theme_mode(raw: u8) -> ThemeMode {
    match raw {
        1 => ThemeMode::Light,
        2 => ThemeMode::Dark,
        _ => ThemeMode::System,
    }
}

#[cfg(any(target_os = "macos", windows, test))]
#[must_use]
pub(crate) const fn resolve_dark_mode(mode: ThemeMode, system_dark: bool) -> bool {
    match mode {
        ThemeMode::System => system_dark,
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
    }
}

#[cfg(any(target_os = "macos", windows, test))]
const PRE_BREAK_URGENT_SECS: u64 = 3;

#[cfg(any(target_os = "macos", windows, test))]
const PROGRESS_DRIFT_TOLERANCE_SECS: f64 = 1.25;

#[cfg(any(target_os = "macos", windows))]
mod banner_layout {
    pub(crate) const BANNER_WIDTH: i32 = 368;
    pub(crate) const MIN_NOTICE_HEIGHT: i32 = 68;
    pub(crate) const PAD_X: i32 = 16;
    pub(crate) const PAD_Y: i32 = 12;
    pub(crate) const ICON_SIZE: i32 = 36;
    pub(crate) const ICON_GAP: i32 = 12;
    pub(crate) const TEXT_X: i32 = PAD_X + ICON_SIZE + ICON_GAP;
    pub(crate) const LINE_GAP: i32 = 4;
    pub(crate) const BADGE_WIDTH: i32 = 64;
    pub(crate) const BADGE_HEIGHT: i32 = 32;
    pub(crate) const BADGE_GAP: i32 = 12;
    pub(crate) const PROGRESS_H: i32 = 4;
    pub(crate) const PROGRESS_GAP: i32 = 12;
}

// Cozy v0.2.0 motion tokens (`tunglt1810/cozy`: `tokens/cozy.tokens.json`).
#[cfg(any(target_os = "macos", test))]
const COZY_EASE_OUT: (f32, f32, f32, f32) = (0.22, 1.0, 0.36, 1.0);
#[cfg(any(target_os = "macos", test))]
const COZY_ENTER_DURATION_SECS: f64 = 0.26;
#[cfg(any(target_os = "macos", test))]
const COZY_FAST_DURATION_SECS: f64 = 0.16;

// Cozy v0.2.0 color tokens (`tunglt1810/cozy`: `tokens/cozy.tokens.json`).
#[cfg(any(target_os = "macos", windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BannerPalette {
    pub(crate) surface: u32,
    pub(crate) border: u32,
    pub(crate) title: u32,
    pub(crate) body: u32,
    pub(crate) badge_bg: u32,
    pub(crate) badge_border: u32,
    pub(crate) badge_text: u32,
    pub(crate) urgent_bg: u32,
    pub(crate) urgent_border: u32,
    pub(crate) urgent_text: u32,
    pub(crate) track: u32,
    pub(crate) fill: u32,
}

#[cfg(any(target_os = "macos", windows, test))]
pub(crate) const COZY_LIGHT_PALETTE: BannerPalette = BannerPalette {
    surface: cozy::LIGHT_SURFACE,
    border: cozy::LIGHT_BORDER_EMPHASIZED,
    title: cozy::LIGHT_TEXT_PRIMARY,
    body: cozy::LIGHT_TEXT_SECONDARY,
    badge_bg: cozy::LIGHT_WARNING_SOFT,
    badge_border: cozy::LIGHT_YELLOW_BORDER,
    badge_text: cozy::LIGHT_YELLOW_TEXT,
    urgent_bg: cozy::LIGHT_ACCENT,
    urgent_border: cozy::LIGHT_FOCUS_RING,
    urgent_text: cozy::LIGHT_ON_ACCENT,
    track: cozy::LIGHT_TRACK,
    fill: cozy::LIGHT_ACCENT,
};

#[cfg(any(target_os = "macos", windows, test))]
pub(crate) const COZY_DARK_PALETTE: BannerPalette = BannerPalette {
    surface: cozy::DARK_SURFACE,
    border: cozy::DARK_BORDER_EMPHASIZED,
    title: cozy::DARK_TEXT_PRIMARY,
    body: cozy::DARK_TEXT_SECONDARY,
    badge_bg: cozy::DARK_WARNING_SOFT,
    badge_border: cozy::DARK_YELLOW_BORDER,
    badge_text: cozy::DARK_YELLOW_TEXT,
    urgent_bg: cozy::DARK_ACCENT,
    urgent_border: cozy::DARK_FOCUS_RING,
    urgent_text: cozy::DARK_ON_ACCENT,
    track: cozy::DARK_TRACK,
    fill: cozy::DARK_ACCENT,
};

#[cfg(any(target_os = "macos", windows, test))]
impl BannerPalette {
    #[must_use]
    pub(crate) const fn for_dark(dark: bool) -> Self {
        if dark {
            COZY_DARK_PALETTE
        } else {
            COZY_LIGHT_PALETTE
        }
    }

    #[must_use]
    pub(crate) const fn badge_colors(self, urgent: bool) -> (u32, u32, u32) {
        if urgent {
            (self.urgent_bg, self.urgent_border, self.urgent_text)
        } else {
            (self.badge_bg, self.badge_border, self.badge_text)
        }
    }
}

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

#[cfg(any(target_os = "macos", windows, test))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProgressTimeline {
    started_at: Instant,
    start_ratio: f64,
    duration_secs: f64,
    total_secs: u64,
    last_remaining_secs: u64,
}

#[cfg(any(target_os = "macos", windows, test))]
impl ProgressTimeline {
    #[must_use]
    pub(crate) fn start(now: Instant, countdown: PreBreakCountdown) -> Self {
        Self {
            started_at: now,
            start_ratio: countdown.progress_ratio(),
            duration_secs: countdown.remaining_secs as f64,
            total_secs: countdown.total_secs,
            last_remaining_secs: countdown.remaining_secs,
        }
    }

    #[must_use]
    pub(crate) fn ratio_at(self, now: Instant) -> f64 {
        if self.duration_secs <= 0.0 || self.start_ratio <= 0.0 {
            return 0.0;
        }
        let elapsed = now.saturating_duration_since(self.started_at).as_secs_f64();
        (self.start_ratio * (1.0 - elapsed / self.duration_secs)).clamp(0.0, self.start_ratio)
    }

    pub(crate) fn sync_or_restart(&mut self, now: Instant, countdown: PreBreakCountdown) -> bool {
        let expected = self.ratio_at(now);
        let target = countdown.progress_ratio();
        let drift_secs = (expected - target).abs() * (countdown.total_secs as f64);
        let needs_restart = countdown.total_secs != self.total_secs
            || countdown.remaining_secs > self.last_remaining_secs
            || drift_secs > PROGRESS_DRIFT_TOLERANCE_SECS;
        if needs_restart {
            *self = Self::start(now, countdown);
            true
        } else {
            self.last_remaining_secs = countdown.remaining_secs;
            false
        }
    }

    pub(crate) fn update_or_restart(
        slot: &mut Option<Self>,
        now: Instant,
        countdown: PreBreakCountdown,
        force_restart: bool,
    ) -> bool {
        if force_restart {
            *slot = Some(Self::start(now, countdown));
            return true;
        }
        match slot.as_mut() {
            Some(timeline) => timeline.sync_or_restart(now, countdown),
            None => {
                *slot = Some(Self::start(now, countdown));
                true
            }
        }
    }
}

pub struct OsNotifier {
    enabled: bool,
    locale: AtomicU8,
    theme_mode: AtomicU8,
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
            .field("theme_mode", &self.theme_mode())
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
            theme_mode: AtomicU8::new(encode_theme_mode(ThemeMode::System)),
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }

    #[must_use]
    pub const fn noop() -> Self {
        Self {
            enabled: false,
            locale: AtomicU8::new(encode_locale(Locale::En)),
            theme_mode: AtomicU8::new(encode_theme_mode(ThemeMode::System)),
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

    pub fn set_theme_mode(&self, mode: ThemeMode) {
        self.theme_mode
            .store(encode_theme_mode(mode), Ordering::Release);
    }

    #[must_use]
    pub fn theme_mode(&self) -> ThemeMode {
        decode_theme_mode(self.theme_mode.load(Ordering::Acquire))
    }

    pub fn show_notice(&self, summary: &str, body: &str) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let accessible = format_notice_accessibility_label(summary, body);
            macos_impl::show_banner(summary, body, &accessible, None, self.theme_mode())
        }

        #[cfg(windows)]
        {
            let accessible = format_notice_accessibility_label(summary, body);
            windows_impl::show_banner(summary, body, &accessible, None, self.theme_mode())
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

fn pre_break_copy_parts(
    break_type: BreakType,
    title: &str,
    locale: Locale,
) -> (&'static str, String) {
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
    (kind_label, body)
}

#[must_use]
pub fn format_pre_break_notification_localized(
    break_type: BreakType,
    title: &str,
    warning_secs: u64,
    locale: Locale,
) -> (String, String) {
    let (kind_label, body) = pre_break_copy_parts(break_type, title, locale);
    let summary = match locale {
        Locale::En => format!("Safe Your Eyes — {kind_label} in {warning_secs}s"),
        Locale::Vi => format!("Safe Your Eyes — {kind_label} sau {warning_secs} giây"),
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
    let (kind_label, body) = pre_break_copy_parts(break_type, title, locale);
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
    const SCREEN_MARGIN: f64 = 16.0;

    let min_x = visible_frame.x + SCREEN_MARGIN;
    let max_x = (visible_frame.x + visible_frame.width - banner_size.0 - SCREEN_MARGIN).max(min_x);
    let min_y = visible_frame.y + SCREEN_MARGIN;
    let max_y = (visible_frame.y + visible_frame.height - banner_size.1 - SCREEN_MARGIN).max(min_y);
    (max_x.round(), max_y.round())
}

impl OsNotifier {
    pub fn show_pre_break(
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
            macos_impl::show_banner(
                &banner_title,
                &body,
                &accessible,
                Some(countdown),
                self.theme_mode(),
            )
        }

        #[cfg(windows)]
        {
            let (banner_title, body, accessible) =
                format_pre_break_banner_copy_localized(break_type, title, locale);
            windows_impl::show_banner(
                &banner_title,
                &body,
                &accessible,
                Some(countdown),
                self.theme_mode(),
            )
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

    pub fn dismiss(&self) -> Result<(), PlatformError> {
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
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSBackingStoreType, NSColor, NSEvent, NSFont, NSFontWeightBold, NSFontWeightRegular,
        NSFontWeightSemibold, NSImage, NSImageView, NSLineBreakMode, NSPanel, NSResponder,
        NSScreen, NSStatusWindowLevel, NSTextField, NSView, NSWindowAnimationBehavior,
        NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGMutablePath, CGPath};
    use objc2_foundation::{
        NSData, NSNumber, NSObject, NSPoint, NSRect, NSSize, NSString, ns_string,
    };
    use objc2_quartz_core::{
        CAAnimation, CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction,
        kCACornerCurveContinuous, kCAMediaTimingFunctionLinear,
    };
    use sye_core::{PlatformError, ThemeMode};

    use super::banner_layout as layout;
    use super::{
        BannerPalette, COZY_EASE_OUT, COZY_ENTER_DURATION_SECS, COZY_FAST_DURATION_SECS,
        PreBreakCountdown, ProgressTimeline, compute_macos_banner_origin, resolve_dark_mode,
    };
    use crate::theme_reveal::{quiet_ca, system_prefers_dark};
    use crate::tray::{ScreenRect, ns_rect_to_screen_rect};

    const APP_ICON_PNG: &[u8] =
        include_bytes!("../../../assets/icons/hicolor/128x128/apps/safe-your-eyes.png");
    const BANNER_WIDTH: f64 = layout::BANNER_WIDTH as f64;
    const MIN_NOTICE_HEIGHT: f64 = layout::MIN_NOTICE_HEIGHT as f64;
    const CORNER_RADIUS: f64 = 16.0;
    const SHADOW_PAD_X: f64 = 16.0;
    const SHADOW_PAD_TOP: f64 = 12.0;
    const SHADOW_PAD_BOTTOM: f64 = 24.0;
    const SHADOW_RADIUS: f64 = 8.0;
    const SHADOW_OFFSET_Y: f64 = -4.0;
    const PAD_X: f64 = layout::PAD_X as f64;
    const PAD_Y: f64 = layout::PAD_Y as f64;
    const ICON_SIZE: f64 = layout::ICON_SIZE as f64;
    const TEXT_X: f64 = layout::TEXT_X as f64;
    const LINE_GAP: f64 = layout::LINE_GAP as f64;
    const BADGE_WIDTH: f64 = layout::BADGE_WIDTH as f64;
    const BADGE_HEIGHT: f64 = layout::BADGE_HEIGHT as f64;
    const BADGE_GAP: f64 = layout::BADGE_GAP as f64;
    const PROGRESS_H: f64 = layout::PROGRESS_H as f64;
    const PROGRESS_GAP: f64 = layout::PROGRESS_GAP as f64;
    const PROGRESS_TRACK_WIDTH: f64 = BANNER_WIDTH - PAD_X * 2.0;
    const FULL_TEXT_WIDTH: f64 = BANNER_WIDTH - TEXT_X - PAD_X;
    const COUNTDOWN_TEXT_WIDTH: f64 = BANNER_WIDTH - TEXT_X - PAD_X - BADGE_GAP - BADGE_WIDTH;
    const MAX_FIT_HEIGHT: f64 = 10_000.0;

    static USER_DISMISSED: AtomicBool = AtomicBool::new(false);

    thread_local! {
        static BANNER: RefCell<Option<PreBreakBanner>> = const { RefCell::new(None) };
    }

    fn system_reduce_motion() -> bool {
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }

    fn srgb_color_alpha(hex: u32, alpha: f64) -> Retained<NSColor> {
        let r = f64::from((hex >> 16) & 0xFF) / 255.0;
        let g = f64::from((hex >> 8) & 0xFF) / 255.0;
        let b = f64::from(hex & 0xFF) / 255.0;
        NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, alpha)
    }

    fn srgb_color(hex: u32) -> Retained<NSColor> {
        srgb_color_alpha(hex, 1.0)
    }

    fn cozy_ease_out() -> Retained<CAMediaTimingFunction> {
        let (x1, y1, x2, y2) = COZY_EASE_OUT;
        CAMediaTimingFunction::functionWithControlPoints(x1, y1, x2, y2)
    }

    fn set_ca_layer_colors(layer: &CALayer, bg: &NSColor, border: Option<&NSColor>) {
        // SAFETY: `NSColor.CGColor` returns a valid `CGColorRef` for the color's lifetime,
        // and `CALayer` retains assigned color properties.
        unsafe {
            let bg_cg: *const AnyObject = msg_send![bg, CGColor];
            let (): () = msg_send![layer, setBackgroundColor: bg_cg];
            if let Some(border_color) = border {
                let border_cg: *const AnyObject = msg_send![border_color, CGColor];
                let (): () = msg_send![layer, setBorderColor: border_cg];
                layer.setBorderWidth(1.0);
            } else {
                layer.setBorderWidth(0.0);
            }
        }
    }

    fn set_view_colors(view: &NSView, bg: &NSColor, border: Option<&NSColor>) {
        if let Some(layer) = view.layer() {
            set_ca_layer_colors(&layer, bg, border);
        }
    }

    fn clear_view_background(view: &NSView) {
        if let Some(layer) = view.layer() {
            // SAFETY: null `CGColorRef` clears the layer background so transparent areas stay clear.
            unsafe {
                let clear_cg: *const AnyObject = ptr::null();
                let (): () = msg_send![&*layer, setBackgroundColor: clear_cg];
            }
        }
    }

    fn set_view_border_only(view: &NSView, border: &NSColor) {
        if let Some(layer) = view.layer() {
            // SAFETY: null background keeps the masked layer clear while `CALayer` retains `borderColor`.
            unsafe {
                let clear_cg: *const AnyObject = ptr::null();
                let (): () = msg_send![&*layer, setBackgroundColor: clear_cg];
                let border_cg: *const AnyObject = msg_send![border, CGColor];
                let (): () = msg_send![&*layer, setBorderColor: border_cg];
                layer.setBorderWidth(1.0);
            }
        }
    }

    fn set_view_shadow(view: &NSView, shadow: &NSColor) {
        if let Some(layer) = view.layer() {
            // SAFETY: `NSColor.CGColor` returns a valid `CGColorRef` retained by `CALayer`.
            unsafe {
                let shadow_cg: *const AnyObject = msg_send![shadow, CGColor];
                let (): () = msg_send![&*layer, setShadowColor: shadow_cg];
            }
            layer.setShadowOpacity(1.0);
            layer.setShadowRadius(SHADOW_RADIUS);
            layer.setShadowOffset(CGSize::new(0.0, SHADOW_OFFSET_Y));
        }
    }

    // An explicit path stops Core Animation from re-rendering the shadow off-screen on every
    // frame of the progress drain animation.
    fn set_shadow_path(view: &NSView, size: NSSize) {
        let Some(layer) = view.layer() else {
            return;
        };
        let path = CGMutablePath::new();
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), size);
        // SAFETY: a null transform means the identity transform.
        unsafe {
            CGMutablePath::add_rounded_rect(
                Some(&path),
                ptr::null(),
                rect,
                CORNER_RADIUS,
                CORNER_RADIUS,
            );
        }
        let shadow_path: &CGPath = &path;
        layer.setShadowPath(Some(shadow_path));
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
        root_view: Retained<NSView>,
        shadow_view: Retained<NSView>,
        card_view: Retained<NSView>,
        icon_view: Option<Retained<NSImageView>>,
        title_label: Retained<NSTextField>,
        body_label: Retained<NSTextField>,
        badge_view: Retained<NSView>,
        badge_label: Retained<NSTextField>,
        progress_track: Retained<NSView>,
        progress_fill_layer: Retained<CALayer>,
        click_shield: Retained<BannerClickView>,
        card_size: NSSize,
        last_summary: String,
        last_body: String,
        last_has_countdown: bool,
        last_dark: Option<bool>,
        last_urgent: Option<bool>,
        last_reduce_motion: bool,
        timeline: Option<ProgressTimeline>,
    }

    impl PreBreakBanner {
        fn new(mtm: MainThreadMarker) -> Self {
            let initial_card_size = NSSize::new(BANNER_WIDTH, MIN_NOTICE_HEIGHT);
            let initial_card_rect = NSRect::new(NSPoint::new(0.0, 0.0), initial_card_size);
            let initial_panel_size = NSSize::new(
                BANNER_WIDTH + SHADOW_PAD_X * 2.0,
                MIN_NOTICE_HEIGHT + SHADOW_PAD_TOP + SHADOW_PAD_BOTTOM,
            );
            let initial_panel_rect = NSRect::new(NSPoint::new(0.0, 0.0), initial_panel_size);
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                initial_panel_rect,
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
            // Borderless `NSPanel` window shadows follow the rectangular frame on a layer-backed
            // content view, leaving faint corner halos; draw the rounded Cozy shadow in `shadow_view`.
            panel.setHasShadow(false);
            panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
            panel.setLevel(NSStatusWindowLevel);
            panel.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );

            let root_view = NSView::initWithFrame(NSView::alloc(mtm), initial_panel_rect);
            panel.setContentView(Some(&root_view));
            root_view.setWantsLayer(true);
            if let Some(layer) = root_view.layer() {
                layer.setMasksToBounds(false);
            }
            clear_view_background(&root_view);

            let shadow_view = NSView::initWithFrame(
                NSView::alloc(mtm),
                NSRect::new(
                    NSPoint::new(SHADOW_PAD_X, SHADOW_PAD_BOTTOM),
                    initial_card_size,
                ),
            );
            shadow_view.setWantsLayer(true);
            if let Some(layer) = shadow_view.layer() {
                layer.setCornerRadius(CORNER_RADIUS);
                // SAFETY: `kCACornerCurveContinuous` is an immutable QuartzCore constant.
                layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
                layer.setMasksToBounds(false);
            }
            set_shadow_path(&shadow_view, initial_card_size);
            root_view.addSubview(&shadow_view);

            let card_view = NSView::initWithFrame(NSView::alloc(mtm), initial_card_rect);
            card_view.setWantsLayer(true);
            if let Some(layer) = card_view.layer() {
                layer.setCornerRadius(CORNER_RADIUS);
                // SAFETY: `kCACornerCurveContinuous` is an immutable QuartzCore constant.
                layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
                layer.setMasksToBounds(true);
            }
            shadow_view.addSubview(&card_view);

            let icon_data = NSData::with_bytes(APP_ICON_PNG);
            let icon_view = NSImage::initWithData(NSImage::alloc(), &icon_data).map(|image| {
                image.setSize(NSSize::new(ICON_SIZE, ICON_SIZE));
                let view = NSImageView::imageViewWithImage(&image, mtm);
                card_view.addSubview(&view);
                view
            });

            // SAFETY: AppKit font weight constants are immutable static values.
            let (title_font, body_font, badge_font) = unsafe {
                (
                    NSFont::monospacedDigitSystemFontOfSize_weight(14.0, NSFontWeightSemibold),
                    NSFont::systemFontOfSize_weight(12.0, NSFontWeightRegular),
                    NSFont::monospacedDigitSystemFontOfSize_weight(14.0, NSFontWeightBold),
                )
            };

            let title_label = NSTextField::labelWithString(&NSString::new(), mtm);
            title_label.setFont(Some(&title_font));
            title_label.setMaximumNumberOfLines(1);
            title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            card_view.addSubview(&title_label);

            let body_label = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
            body_label.setSelectable(false);
            body_label.setFont(Some(&body_font));
            body_label.setMaximumNumberOfLines(2);
            body_label.setPreferredMaxLayoutWidth(COUNTDOWN_TEXT_WIDTH);
            if let Some(cell) = body_label.cell() {
                cell.setTruncatesLastVisibleLine(true);
            }
            card_view.addSubview(&body_label);

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
            card_view.addSubview(&badge_view);

            let progress_rect = NSRect::new(
                NSPoint::new(PAD_X, PAD_Y),
                NSSize::new(PROGRESS_TRACK_WIDTH, PROGRESS_H),
            );
            let progress_track = NSView::initWithFrame(NSView::alloc(mtm), progress_rect);
            progress_track.setWantsLayer(true);
            let progress_fill_layer = CALayer::new();
            quiet_ca(|| {
                progress_fill_layer.setAnchorPoint(CGPoint::new(0.0, 0.5));
                progress_fill_layer.setPosition(CGPoint::new(0.0, PROGRESS_H * 0.5));
                progress_fill_layer.setBounds(CGRect::new(
                    CGPoint::new(0.0, 0.0),
                    CGSize::new(0.0, PROGRESS_H),
                ));
                progress_fill_layer.setCornerRadius(PROGRESS_H * 0.5);
                if let Some(track_layer) = progress_track.layer() {
                    track_layer.setCornerRadius(PROGRESS_H * 0.5);
                    track_layer.setMasksToBounds(true);
                    track_layer.addSublayer(&progress_fill_layer);
                }
            });
            card_view.addSubview(&progress_track);

            // SAFETY: `BannerClickView` inherits `-[NSView initWithFrame:]` with no custom ivars.
            let click_shield: Retained<BannerClickView> =
                unsafe { msg_send![BannerClickView::alloc(mtm), initWithFrame: initial_card_rect] };
            card_view.addSubview(&click_shield);

            Self {
                panel,
                root_view,
                shadow_view,
                card_view,
                icon_view,
                title_label,
                body_label,
                badge_view,
                badge_label,
                progress_track,
                progress_fill_layer,
                click_shield,
                card_size: initial_card_size,
                last_summary: String::new(),
                last_body: String::new(),
                last_has_countdown: false,
                last_dark: None,
                last_urgent: None,
                last_reduce_motion: false,
                timeline: None,
            }
        }

        fn apply_card_theme(&self, dark: bool, palette: BannerPalette) {
            // SAFETY: `NSAppearanceNameDarkAqua` and `NSAppearanceNameAqua` are immutable AppKit constants.
            let appearance_name = unsafe {
                if dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            let appearance = NSAppearance::appearanceNamed(appearance_name);
            self.panel.setAppearance(appearance.as_deref());

            let surface = srgb_color(palette.surface);
            let border = srgb_color(palette.border);
            let shadow = if dark {
                srgb_color_alpha(0x18140E, 0.48)
            } else {
                srgb_color_alpha(0x6E4A2A, 0.22)
            };
            let title = srgb_color(palette.title);
            let body = srgb_color(palette.body);
            let track = srgb_color(palette.track);
            let fill = srgb_color(palette.fill);

            self.title_label.setTextColor(Some(&title));
            self.body_label.setTextColor(Some(&body));
            quiet_ca(|| {
                clear_view_background(&self.root_view);
                set_view_colors(&self.shadow_view, &surface, None);
                set_view_shadow(&self.shadow_view, &shadow);
                set_view_border_only(&self.card_view, &border);
                set_view_colors(&self.progress_track, &track, None);
                set_ca_layer_colors(&self.progress_fill_layer, &fill, None);
            });
        }

        fn start_progress_drain(&self, start_width: f64, duration_secs: f64) {
            let key = ns_string!("cozyProgressDrain");
            self.progress_fill_layer.removeAnimationForKey(key);

            if start_width <= 0.0 || duration_secs <= 0.0 {
                quiet_ca(|| {
                    self.progress_fill_layer.setBounds(CGRect::new(
                        CGPoint::new(0.0, 0.0),
                        CGSize::new(0.0, PROGRESS_H),
                    ));
                });
                return;
            }

            // Set the model layer to the final drained width before adding the explicit animation
            // so the bar never snaps back when the animation completes.
            quiet_ca(|| {
                self.progress_fill_layer.setBounds(CGRect::new(
                    CGPoint::new(0.0, 0.0),
                    CGSize::new(0.0, PROGRESS_H),
                ));
            });

            let animation =
                CABasicAnimation::animationWithKeyPath(Some(ns_string!("bounds.size.width")));
            animation.setDuration(duration_secs);
            // SAFETY: `kCAMediaTimingFunctionLinear` is an immutable QuartzCore constant.
            let linear =
                CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionLinear });
            animation.setTimingFunction(Some(&linear));
            let from_num = NSNumber::numberWithDouble(start_width);
            let to_num = NSNumber::numberWithDouble(0.0);
            // SAFETY: `bounds.size.width` expects numeric `NSNumber` values.
            unsafe {
                animation.setFromValue(Some(from_num.as_ref()));
                animation.setToValue(Some(to_num.as_ref()));
            }
            let ca_animation: &CAAnimation = &animation;
            self.progress_fill_layer
                .addAnimation_forKey(ca_animation, Some(key));
        }

        fn play_entrance_animation(&self) {
            let Some(layer) = self.root_view.layer() else {
                return;
            };
            quiet_ca(|| {
                layer.setOpacity(1.0);
            });
            let fade = CABasicAnimation::animationWithKeyPath(Some(ns_string!("opacity")));
            fade.setDuration(COZY_ENTER_DURATION_SECS);
            let timing = cozy_ease_out();
            fade.setTimingFunction(Some(&timing));
            let from_num = NSNumber::numberWithDouble(0.0);
            let to_num = NSNumber::numberWithDouble(1.0);
            // SAFETY: `opacity` expects numeric `NSNumber` values.
            unsafe {
                fade.setFromValue(Some(from_num.as_ref()));
                fade.setToValue(Some(to_num.as_ref()));
            }
            let ca_animation: &CAAnimation = &fade;
            layer.addAnimation_forKey(ca_animation, Some(ns_string!("cozyBannerEnter")));
        }

        fn animate_layer_color(
            layer: &CALayer,
            key_path: &NSString,
            anim_key: &NSString,
            from: &NSColor,
            to: &NSColor,
        ) {
            let animation = CABasicAnimation::animationWithKeyPath(Some(key_path));
            animation.setDuration(COZY_FAST_DURATION_SECS);
            let timing = cozy_ease_out();
            animation.setTimingFunction(Some(&timing));
            // SAFETY: `NSColor.CGColor` returns a valid toll-free bridged `CGColorRef` kept alive
            // by `from`/`to`; `CABasicAnimation` retains assigned `fromValue`/`toValue`.
            unsafe {
                let from_cg: *const AnyObject = msg_send![from, CGColor];
                let to_cg: *const AnyObject = msg_send![to, CGColor];
                animation.setFromValue(from_cg.as_ref());
                animation.setToValue(to_cg.as_ref());
            }
            let ca_animation: &CAAnimation = &animation;
            layer.addAnimation_forKey(ca_animation, Some(anim_key));
        }

        fn apply_countdown_style(
            &mut self,
            countdown: PreBreakCountdown,
            palette: BannerPalette,
            was_visible: bool,
            mode_changed: bool,
            theme_changed: bool,
            reduce_motion: bool,
        ) {
            let urgent = countdown.is_urgent();
            let (bg_hex, border_hex, text_hex) = palette.badge_colors(urgent);
            let badge_bg = srgb_color(bg_hex);
            let badge_border = srgb_color(border_hex);
            let badge_text = srgb_color(text_hex);

            let animate_badge = was_visible
                && !mode_changed
                && !theme_changed
                && !reduce_motion
                && self.last_urgent.is_some_and(|prev| prev != urgent);
            quiet_ca(|| {
                set_view_colors(&self.badge_view, &badge_bg, Some(&badge_border));
            });
            if animate_badge && let Some(layer) = self.badge_view.layer() {
                let (prev_bg_hex, prev_border_hex, _) = palette.badge_colors(!urgent);
                let prev_bg = srgb_color(prev_bg_hex);
                let prev_border = srgb_color(prev_border_hex);
                Self::animate_layer_color(
                    &layer,
                    ns_string!("backgroundColor"),
                    ns_string!("cozyBadgeBg"),
                    &prev_bg,
                    &badge_bg,
                );
                Self::animate_layer_color(
                    &layer,
                    ns_string!("borderColor"),
                    ns_string!("cozyBadgeBorder"),
                    &prev_border,
                    &badge_border,
                );
            }
            self.last_urgent = Some(urgent);
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

            if reduce_motion {
                self.timeline = None;
                self.progress_fill_layer
                    .removeAnimationForKey(ns_string!("cozyProgressDrain"));
                let fill_w = (PROGRESS_TRACK_WIDTH * countdown.progress_ratio()).round();
                quiet_ca(|| {
                    self.progress_fill_layer.setBounds(CGRect::new(
                        CGPoint::new(0.0, 0.0),
                        CGSize::new(fill_w, PROGRESS_H),
                    ));
                });
                return;
            }

            let now = Instant::now();
            let force_restart = !was_visible || mode_changed || self.last_reduce_motion;
            if ProgressTimeline::update_or_restart(
                &mut self.timeline,
                now,
                countdown,
                force_restart,
            ) && let Some(timeline) = self.timeline
            {
                let start_w = PROGRESS_TRACK_WIDTH * timeline.start_ratio;
                self.start_progress_drain(start_w, timeline.duration_secs);
            }
        }

        fn show_or_update(
            &mut self,
            summary: &str,
            body: &str,
            accessible_label: &str,
            countdown: Option<PreBreakCountdown>,
            theme_mode: ThemeMode,
            mtm: MainThreadMarker,
        ) -> Result<(), PlatformError> {
            let summary_changed = self.last_summary != summary;
            let body_changed = self.last_body != body;
            let has_countdown = countdown.is_some();
            let mode_changed = self.last_has_countdown != has_countdown;
            let was_visible = self.panel.isVisible();
            let dark = resolve_dark_mode(theme_mode, system_prefers_dark().unwrap_or(false));
            let reduce_motion = system_reduce_motion();
            let palette = BannerPalette::for_dark(dark);

            let theme_changed = self.last_dark != Some(dark);
            if theme_changed {
                self.last_dark = Some(dark);
                self.apply_card_theme(dark, palette);
            }

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
                    let (): () = msg_send![&*self.card_view, setAccessibilityLabel: &*a11y];
                }
            }
            let visible = active_screen_visible_frame(mtm).ok_or_else(|| {
                PlatformError::Unavailable("macOS display screen unavailable".to_owned())
            })?;
            let prev_frame = self.panel.frame();
            let needs_layout = !was_visible || body_changed || mode_changed;
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
                let row_h =
                    text_block_h
                        .max(ICON_SIZE)
                        .max(if has_countdown { BADGE_HEIGHT } else { 0.0 });
                let (banner_h, row_bottom) = if has_countdown {
                    let h = (PAD_Y + row_h + PROGRESS_GAP + PROGRESS_H + PAD_Y).ceil();
                    (h, PAD_Y + PROGRESS_H + PROGRESS_GAP)
                } else {
                    let h = (PAD_Y + row_h + PAD_Y).max(MIN_NOTICE_HEIGHT).ceil();
                    (h, (h - row_h) * 0.5)
                };
                let card_size = NSSize::new(BANNER_WIDTH, banner_h);
                self.card_size = card_size;
                let panel_size = NSSize::new(
                    BANNER_WIDTH + SHADOW_PAD_X * 2.0,
                    banner_h + SHADOW_PAD_TOP + SHADOW_PAD_BOTTOM,
                );

                self.root_view
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), panel_size));
                self.shadow_view.setFrame(NSRect::new(
                    NSPoint::new(SHADOW_PAD_X, SHADOW_PAD_BOTTOM),
                    card_size,
                ));
                quiet_ca(|| set_shadow_path(&self.shadow_view, card_size));
                self.card_view
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), card_size));
                self.click_shield
                    .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), card_size));
                if let Some(icon_view) = &self.icon_view {
                    let icon_y = (row_bottom + (row_h - ICON_SIZE) * 0.5).round();
                    icon_view.setFrame(NSRect::new(
                        NSPoint::new(PAD_X, icon_y),
                        NSSize::new(ICON_SIZE, ICON_SIZE),
                    ));
                }

                let body_y = (row_bottom + (row_h - text_block_h) * 0.5).round();
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
                if has_countdown {
                    let badge_x = BANNER_WIDTH - PAD_X - BADGE_WIDTH;
                    let badge_y = (row_bottom + (row_h - BADGE_HEIGHT) * 0.5).round();
                    self.badge_view.setFrame(NSRect::new(
                        NSPoint::new(badge_x, badge_y),
                        NSSize::new(BADGE_WIDTH, BADGE_HEIGHT),
                    ));
                    self.progress_track.setFrame(NSRect::new(
                        NSPoint::new(PAD_X, PAD_Y),
                        NSSize::new(PROGRESS_TRACK_WIDTH, PROGRESS_H),
                    ));
                    quiet_ca(|| {
                        self.progress_fill_layer
                            .setPosition(CGPoint::new(0.0, PROGRESS_H * 0.5));
                    });
                }
            }

            if let Some(cd) = countdown {
                self.apply_countdown_style(
                    cd,
                    palette,
                    was_visible,
                    mode_changed,
                    theme_changed,
                    reduce_motion,
                );
            } else {
                self.timeline = None;
                self.last_urgent = None;
                self.progress_fill_layer
                    .removeAnimationForKey(ns_string!("cozyProgressDrain"));
            }
            self.last_reduce_motion = reduce_motion;

            let (card_x, card_y) =
                compute_macos_banner_origin(visible, (self.card_size.width, self.card_size.height));
            let ox = card_x - SHADOW_PAD_X;
            let oy = card_y - SHADOW_PAD_BOTTOM;
            let panel_size = NSSize::new(
                self.card_size.width + SHADOW_PAD_X * 2.0,
                self.card_size.height + SHADOW_PAD_TOP + SHADOW_PAD_BOTTOM,
            );
            let origin_changed =
                (prev_frame.origin.x - ox).abs() > 0.5 || (prev_frame.origin.y - oy).abs() > 0.5;
            if needs_layout || origin_changed {
                self.panel
                    .setFrame_display(NSRect::new(NSPoint::new(ox, oy), panel_size), was_visible);
            }
            if !was_visible {
                self.panel.orderFrontRegardless();
                if !reduce_motion {
                    self.play_entrance_animation();
                }
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
        theme_mode: ThemeMode,
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
                banner.show_or_update(summary, body, accessible_label, countdown, theme_mode, mtm)
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
    use std::time::Instant;

    use sye_core::{PlatformError, ThemeMode};
    use windows_sys::Win32::Foundation::{
        COLORREF, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, S_OK,
        WPARAM,
    };
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
        CreatePen, CreateSolidBrush, DRAW_TEXT_FORMAT, DT_CALCRECT, DT_CENTER, DT_EDITCONTROL,
        DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteDC,
        DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, GetMonitorInfoW, GetStockObject,
        GetTextMetricsW, HDC, HFONT, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
        MonitorFromPoint, NULL_BRUSH, PAINTSTRUCT, PS_INSIDEFRAME, PS_SOLID, Rectangle, ReleaseDC,
        RoundRect, SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetViewportOrgEx, TEXTMETRICW,
        TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
        MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext, SystemParametersInfoForDpi,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_DROPSHADOW, CreateWindowExW, DI_NORMAL, DefWindowProcW, DestroyIcon, DestroyWindow,
        DrawIconEx, GCL_STYLE, GetClientRect, GetCursorPos, HICON, HWND_TOPMOST, IDC_ARROW,
        IMAGE_ICON, IsWindowVisible, KillTimer, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW,
        MA_NOACTIVATE, NONCLIENTMETRICSW, RegisterClassExW, SPI_GETCLIENTAREAANIMATION,
        SPI_GETNONCLIENTMETRICS, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetClassLongPtrW,
        SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, WM_ERASEBKGND,
        WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    use super::banner_layout::{
        BADGE_GAP, BADGE_HEIGHT, BADGE_WIDTH, BANNER_WIDTH, ICON_SIZE, LINE_GAP, MIN_NOTICE_HEIGHT,
        PAD_X, PAD_Y, PROGRESS_GAP, PROGRESS_H, TEXT_X,
    };
    use super::{
        BannerPalette, PreBreakCountdown, ProgressTimeline, compute_windows_banner_origin,
        resolve_dark_mode, rgb_to_bgr,
    };
    use crate::console::encode_wide_nul as wide_z;

    const CLASS_NAME: &str = "SyePreBreakBanner";
    const WINDOW_TITLE: &str = "Safe Your Eyes";
    const APP_ICON_RESOURCE_ID: usize = 1;
    const BASE_DPI: i32 = 96;
    const MARGIN: i32 = 16;
    const BODY_MAX_LINES: i32 = 2;
    const TITLE_FONT_WEIGHT: i32 = 600;
    const BADGE_FONT_WEIGHT: i32 = 700;
    const PROGRESS_TIMER_ID: usize = 1;
    const PROGRESS_FRAME_MS: u32 = 16;
    const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

    #[derive(Clone, Copy)]
    struct WinPalette {
        surface: COLORREF,
        border: COLORREF,
        title: COLORREF,
        body: COLORREF,
        track: COLORREF,
        fill: COLORREF,
    }

    impl WinPalette {
        const fn from_cozy(palette: BannerPalette) -> Self {
            Self {
                surface: rgb_to_bgr(palette.surface),
                border: rgb_to_bgr(palette.border),
                title: rgb_to_bgr(palette.title),
                body: rgb_to_bgr(palette.body),
                track: rgb_to_bgr(palette.track),
                fill: rgb_to_bgr(palette.fill),
            }
        }
    }

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

    unsafe fn fill_round_rect(
        hdc: HDC,
        rect: RECT,
        radius: i32,
        fill: COLORREF,
        border: COLORREF,
        border_w: i32,
    ) {
        // SAFETY: `hdc` is a live GDI device context; brush and pen are restored and deleted.
        unsafe {
            let brush = CreateSolidBrush(fill);
            let pen = CreatePen(PS_SOLID, border_w, border);
            let prev_brush = SelectObject(hdc, brush);
            let prev_pen = SelectObject(hdc, pen);
            RoundRect(
                hdc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius,
                radius,
            );
            SelectObject(hdc, prev_pen);
            SelectObject(hdc, prev_brush);
            DeleteObject(pen);
            DeleteObject(brush);
        }
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

    fn windows_reduce_motion() -> bool {
        let mut enabled: i32 = 1;
        // SAFETY: `enabled` is writable `BOOL` storage for `SPI_GETCLIENTAREAANIMATION`.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&raw mut enabled).cast::<c_void>(),
                0,
            )
        };
        ok != 0 && enabled == 0
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
        dwm_rounded: bool,
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
        timeline: Option<ProgressTimeline>,
        last_reduce_motion: bool,
        timer_active: bool,
        last_fill_w: i32,
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
            // the attribute and keeps square corners with a GDI Cozy border fallback.
            let dwm_rounded = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                    (&raw const corner).cast::<c_void>(),
                    size_u32::<i32>(),
                ) == S_OK
            };
            // DWM already draws a rounded shadow. `CS_DROPSHADOW` adds a square one behind the
            // rounded corners, so the class gets it only when DWM rounding is not available.
            if !dwm_rounded {
                // SAFETY: `hwnd` is the live banner window created above.
                unsafe {
                    SetClassLongPtrW(hwnd, GCL_STYLE, CS_DROPSHADOW as isize);
                }
            }

            Ok(Self {
                hwnd,
                hinstance,
                dwm_rounded,
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
                timeline: None,
                last_reduce_motion: false,
                timer_active: false,
                last_fill_w: -1,
                size: (0, 0),
                icon_rect: RECT::default(),
                title_rect: RECT::default(),
                body_rect: RECT::default(),
                badge_rect: RECT::default(),
                progress_track_rect: RECT::default(),
            })
        }

        fn start_progress_timer(&mut self) {
            if !self.timer_active {
                // SAFETY: `self.hwnd` is the live banner window; timer delivers `WM_TIMER` to `wnd_proc`.
                let id = unsafe { SetTimer(self.hwnd, PROGRESS_TIMER_ID, PROGRESS_FRAME_MS, None) };
                self.timer_active = id != 0;
            }
        }

        fn stop_progress_timer(&mut self) {
            if self.timer_active {
                // SAFETY: `self.hwnd` is the live banner window that owns `PROGRESS_TIMER_ID`.
                unsafe {
                    KillTimer(self.hwnd, PROGRESS_TIMER_ID);
                }
                self.timer_active = false;
            }
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
            let border = WinPalette::from_cozy(BannerPalette::for_dark(dark)).border;
            // SAFETY: both values outlive the calls and match their DWM attribute types.
            unsafe {
                DwmSetWindowAttribute(
                    self.hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    (&raw const enabled).cast::<c_void>(),
                    size_u32::<i32>(),
                );
                DwmSetWindowAttribute(
                    self.hwnd,
                    DWMWA_BORDER_COLOR as u32,
                    (&raw const border).cast::<c_void>(),
                    size_u32::<COLORREF>(),
                );
            }
        }

        fn apply_dpi(&mut self, dpi: i32) {
            self.release_resources();
            self.dpi = dpi;
            self.title_font = message_font(dpi, Some(TITLE_FONT_WEIGHT), 108);
            self.body_font = message_font(dpi, None, 96);
            self.badge_font = message_font(dpi, Some(BADGE_FONT_WEIGHT), 112);
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
                BANNER_WIDTH - TEXT_X - PAD_X - BADGE_GAP - BADGE_WIDTH
            } else {
                BANNER_WIDTH - TEXT_X - PAD_X
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
            let pad_x = scale(PAD_X, dpi);
            let pad_y = scale(PAD_Y, dpi);
            let gap = scale(LINE_GAP, dpi);
            let icon = scale(ICON_SIZE, dpi);
            let badge_w = scale(BADGE_WIDTH, dpi);
            let badge_h = scale(BADGE_HEIGHT, dpi);
            let progress_h = scale(PROGRESS_H, dpi).max(2);
            let progress_gap = scale(PROGRESS_GAP, dpi);
            let text_block_h = title_h + gap + body_h;
            let row_h = text_block_h
                .max(icon)
                .max(if has_countdown { badge_h } else { 0 });

            let width = scale(BANNER_WIDTH, dpi);
            let (height, row_top) = if has_countdown {
                (pad_y + row_h + progress_gap + progress_h + pad_y, pad_y)
            } else {
                let h = (pad_y * 2 + row_h).max(scale(MIN_NOTICE_HEIGHT, dpi));
                (h, (h - row_h) / 2)
            };
            self.size = (width, height);

            let icon_y = row_top + (row_h - icon) / 2;
            self.icon_rect = RECT {
                left: pad_x,
                top: icon_y,
                right: pad_x + icon,
                bottom: icon_y + icon,
            };

            let text_x = scale(TEXT_X, dpi);
            let text_right = text_x + self.text_width();
            let title_top = row_top + (row_h - text_block_h) / 2;
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
                let badge_left = width - pad_x - badge_w;
                let badge_top = row_top + (row_h - badge_h) / 2;
                self.badge_rect = RECT {
                    left: badge_left,
                    top: badge_top,
                    right: badge_left + badge_w,
                    bottom: badge_top + badge_h,
                };
                let track_top = height - pad_y - progress_h;
                self.progress_track_rect = RECT {
                    left: pad_x,
                    top: track_top,
                    right: width - pad_x,
                    bottom: track_top + progress_h,
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
            theme_mode: ThemeMode,
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
            let reduce_motion = windows_reduce_motion();
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
            let dark = resolve_dark_mode(theme_mode, apps_use_dark_theme());
            if self.dark != Some(dark) {
                self.apply_theme(dark);
            }
            if !was_visible || body_changed || mode_changed || dpi_changed {
                self.layout();
            }

            if let Some(cd) = countdown {
                if reduce_motion {
                    self.timeline = None;
                    self.stop_progress_timer();
                } else {
                    let now = Instant::now();
                    let force_restart = !was_visible || mode_changed || self.last_reduce_motion;
                    let _ = ProgressTimeline::update_or_restart(
                        &mut self.timeline,
                        now,
                        cd,
                        force_restart,
                    );
                    if self.timeline.is_some_and(|tl| tl.ratio_at(now) > 0.0) {
                        self.start_progress_timer();
                    } else {
                        self.stop_progress_timer();
                    }
                }
            } else {
                self.timeline = None;
                self.stop_progress_timer();
            }
            self.last_reduce_motion = reduce_motion;

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

        fn paint(&self, hdc: HDC, dirty: RECT) {
            let mut client = RECT::default();
            // SAFETY: `self.hwnd` is the live banner window and `client` is writable storage.
            unsafe { GetClientRect(self.hwnd, &mut client) };
            let track = self.progress_track_rect;
            let progress_only = self.countdown.is_some()
                && track.right > track.left
                && track.bottom > track.top
                && dirty.left >= track.left
                && dirty.top >= track.top
                && dirty.right <= track.right
                && dirty.bottom <= track.bottom;
            let area = if progress_only { track } else { client };
            let width = area.right - area.left;
            let height = area.bottom - area.top;
            if width <= 0 || height <= 0 {
                return;
            }
            // Double-buffer GDI draws so 60fps progress bar updates never flicker.
            // SAFETY: `hdc` comes from `BeginPaint` for `self.hwnd`, and every handle is released.
            unsafe {
                let mem_dc = CreateCompatibleDC(hdc);
                let mem_bmp = if mem_dc.is_null() {
                    ptr::null_mut()
                } else {
                    CreateCompatibleBitmap(hdc, width, height)
                };
                if !mem_dc.is_null() && !mem_bmp.is_null() {
                    let prev_bmp = SelectObject(mem_dc, mem_bmp);
                    SetViewportOrgEx(mem_dc, -area.left, -area.top, ptr::null_mut());
                    self.paint_area(mem_dc, client, progress_only);
                    BitBlt(
                        hdc, area.left, area.top, width, height, mem_dc, area.left, area.top,
                        SRCCOPY,
                    );
                    SelectObject(mem_dc, prev_bmp);
                    DeleteObject(mem_bmp);
                    DeleteDC(mem_dc);
                } else {
                    if !mem_dc.is_null() {
                        DeleteDC(mem_dc);
                    }
                    self.paint_area(hdc, client, progress_only);
                }
            }
        }

        fn paint_area(&self, hdc: HDC, client: RECT, progress_only: bool) {
            if !progress_only {
                self.paint_surface(hdc, client);
                return;
            }
            let colors = WinPalette::from_cozy(BannerPalette::for_dark(self.dark.unwrap_or(false)));
            // SAFETY: `hdc` is a live GDI device context and the brush is deleted after use.
            unsafe {
                let background = CreateSolidBrush(colors.surface);
                FillRect(hdc, &self.progress_track_rect, background);
                DeleteObject(background);
            }
            self.paint_progress(hdc, &colors);
        }

        fn progress_fill_width(&self, now: Instant) -> i32 {
            let track_w = self.progress_track_rect.right - self.progress_track_rect.left;
            let ratio = self.countdown.map_or(0.0, |cd| {
                self.timeline
                    .map_or_else(|| cd.progress_ratio(), |tl| tl.ratio_at(now))
            });
            (f64::from(track_w) * ratio).round() as i32
        }

        fn paint_progress(&self, hdc: HDC, colors: &WinPalette) {
            let track = self.progress_track_rect;
            let track_w = track.right - track.left;
            let track_h = track.bottom - track.top;
            if track_w <= 0 || track_h <= 0 {
                return;
            }
            // SAFETY: `hdc` is a live GDI device context and every handle is owned or restored.
            unsafe {
                fill_round_rect(hdc, track, track_h, colors.track, colors.track, 1);

                let fill_w = self.progress_fill_width(Instant::now());
                if fill_w > 0 {
                    let fill_rect = RECT {
                        right: (track.left + fill_w).min(track.right),
                        ..track
                    };
                    let radius = track_h.min(fill_w);
                    fill_round_rect(hdc, fill_rect, radius, colors.fill, colors.fill, 1);
                }
            }
        }

        fn paint_surface(&self, hdc: HDC, client: RECT) {
            let mut title_rect = self.title_rect;
            let mut body_rect = self.body_rect;
            let cozy = BannerPalette::for_dark(self.dark.unwrap_or(false));
            let colors = WinPalette::from_cozy(cozy);
            // SAFETY: `hdc` is a live GDI device context and every handle is owned or restored.
            unsafe {
                let background = CreateSolidBrush(colors.surface);
                FillRect(hdc, &client, background);
                DeleteObject(background);

                if !self.dwm_rounded {
                    let border_w = scale(1, self.dpi).max(1);
                    let border_pen = CreatePen(PS_INSIDEFRAME, border_w, colors.border);
                    let prev_pen = SelectObject(hdc, border_pen);
                    let prev_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                    Rectangle(hdc, client.left, client.top, client.right, client.bottom);
                    SelectObject(hdc, prev_brush);
                    SelectObject(hdc, prev_pen);
                    DeleteObject(border_pen);
                }

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
                    let (bg_hex, border_hex, text_hex) = cozy.badge_colors(cd.is_urgent());
                    let (badge_bg, badge_border, badge_fg) = (
                        rgb_to_bgr(bg_hex),
                        rgb_to_bgr(border_hex),
                        rgb_to_bgr(text_hex),
                    );
                    let mut badge_rect = self.badge_rect;
                    let pill_h = badge_rect.bottom - badge_rect.top;
                    let border_w = scale(1, self.dpi).max(1);
                    fill_round_rect(hdc, badge_rect, pill_h, badge_bg, badge_border, border_w);

                    SelectObject(hdc, self.badge_font);
                    SetTextColor(hdc, badge_fg);
                    draw_text(
                        hdc,
                        &self.badge_text,
                        &mut badge_rect,
                        DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX,
                    );

                    self.paint_progress(hdc, &colors);
                }
                SelectObject(hdc, previous);
            }
        }
    }

    impl Drop for Banner {
        fn drop(&mut self) {
            self.stop_progress_timer();
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
                BANNER.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(banner) = guard.as_mut().filter(|b| b.hwnd == hwnd)
                    {
                        banner.stop_progress_timer();
                    }
                });
                // Hide only: `DestroyWindow` here would free the window while it dispatches.
                // SAFETY: `hwnd` is the live banner window that received this message.
                unsafe {
                    KillTimer(hwnd, PROGRESS_TIMER_ID);
                    ShowWindow(hwnd, SW_HIDE);
                }
                0
            }
            WM_TIMER if wparam == PROGRESS_TIMER_ID => {
                BANNER.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(banner) = guard.as_mut().filter(|b| b.hwnd == hwnd)
                    {
                        let now = Instant::now();
                        let fill_w = banner.progress_fill_width(now);
                        if fill_w != banner.last_fill_w {
                            banner.last_fill_w = fill_w;
                            let track = banner.progress_track_rect;
                            // SAFETY: `hwnd` is the live banner window and `track` is a client rect.
                            unsafe { InvalidateRect(hwnd, &track, 0) };
                        }
                        let still_running =
                            banner.timeline.is_some_and(|tl| tl.ratio_at(now) > 0.0);
                        if !still_running {
                            banner.stop_progress_timer();
                        }
                    }
                });
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
                    banner.paint(hdc, paint.rcPaint);
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
        theme_mode: ThemeMode,
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
                banner.show_or_update(summary, body, accessible_label, countdown, theme_mode)
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
            (1128.0, 874.0)
        );

        let tiny = ScreenRect {
            x: 100.0,
            y: 40.0,
            width: 300.0,
            height: 60.0,
        };
        assert_eq!(
            compute_macos_banner_origin(tiny, (368.0, 68.0)),
            (116.0, 56.0)
        );
    }

    #[test]
    fn windows_banner_anchors_bottom_right_inside_work_area() {
        assert_eq!(
            compute_windows_banner_origin((0, 0, 1920, 1032), (368, 68), 16),
            (1536, 948)
        );
        assert_eq!(
            compute_windows_banner_origin((-1280, 0, 0, 984), (460, 85), 20),
            (-480, 879)
        );
        assert_eq!(
            compute_windows_banner_origin((100, 40, 400, 100), (368, 68), 16),
            (116, 56)
        );
    }

    #[test]
    fn cozy_banner_palette_matches_tokens_and_avoids_pure_white_or_black() {
        let light = BannerPalette::for_dark(false);
        assert_eq!(light, COZY_LIGHT_PALETTE);
        assert_eq!(light.surface, 0xFBF1E1);
        assert_eq!(light.border, 0xDFCBB5);
        assert_eq!(light.title, 0x261101);
        assert_eq!(light.body, 0x5B4C3B);
        assert_eq!(light.badge_colors(false), (0xFAE1B5, 0xE5C27D, 0x5E3904));
        assert_eq!(light.badge_colors(true), (0xEF8F63, 0xA84A24, 0x261101));
        assert_eq!(light.track, 0xE6D2BC);
        assert_eq!(light.fill, 0xEF8F63);

        let dark = BannerPalette::for_dark(true);
        assert_eq!(dark, COZY_DARK_PALETTE);
        assert_eq!(dark.surface, 0x231D16);
        assert_eq!(dark.border, 0x52493F);
        assert_eq!(dark.title, 0xFFFEF7);
        assert_eq!(dark.body, 0xC2B7B1);
        assert_eq!(dark.badge_colors(false), (0x443A26, 0xC99A44, 0xFCE09B));
        assert_eq!(dark.badge_colors(true), (0xEE8F64, 0xF2A57E, 0x18140E));
        assert_eq!(dark.track, 0x30271E);
        assert_eq!(dark.fill, 0xEE8F64);

        for p in [light, dark] {
            for color in [
                p.surface,
                p.border,
                p.title,
                p.body,
                p.badge_bg,
                p.badge_border,
                p.badge_text,
                p.urgent_bg,
                p.urgent_border,
                p.urgent_text,
                p.track,
                p.fill,
            ] {
                assert_ne!(color, 0x000000);
                assert_ne!(color, 0xFFFFFF);
            }
        }

        assert_eq!(COZY_EASE_OUT, (0.22, 1.0, 0.36, 1.0));
        assert!((COZY_ENTER_DURATION_SECS - 0.26).abs() < 1e-9);
        assert!((COZY_FAST_DURATION_SECS - 0.16).abs() < 1e-9);
    }

    #[test]
    fn theme_mode_resolves_explicit_and_system_modes_and_round_trips_on_notifier() {
        assert!(!resolve_dark_mode(ThemeMode::Light, true));
        assert!(!resolve_dark_mode(ThemeMode::Light, false));
        assert!(resolve_dark_mode(ThemeMode::Dark, false));
        assert!(resolve_dark_mode(ThemeMode::Dark, true));
        assert!(!resolve_dark_mode(ThemeMode::System, false));
        assert!(resolve_dark_mode(ThemeMode::System, true));

        let notifier = OsNotifier::new();
        assert_eq!(notifier.theme_mode(), ThemeMode::System);
        notifier.set_theme_mode(ThemeMode::Dark);
        assert_eq!(notifier.theme_mode(), ThemeMode::Dark);
        notifier.set_theme_mode(ThemeMode::Light);
        assert_eq!(notifier.theme_mode(), ThemeMode::Light);
        notifier.set_theme_mode(ThemeMode::System);
        assert_eq!(notifier.theme_mode(), ThemeMode::System);
    }

    #[test]
    fn progress_timeline_interpolates_continuously_without_stuttering_on_second_ticks() {
        use std::time::Duration;

        let t0 = Instant::now();
        let mut tl = ProgressTimeline::start(
            t0,
            PreBreakCountdown {
                remaining_secs: 10,
                total_secs: 10,
            },
        );
        assert!((tl.ratio_at(t0) - 1.0).abs() < 1e-9);
        assert!((tl.ratio_at(t0 + Duration::from_millis(250)) - 0.975).abs() < 1e-6);
        assert!((tl.ratio_at(t0 + Duration::from_millis(500)) - 0.95).abs() < 1e-6);
        assert!((tl.ratio_at(t0 + Duration::from_secs(5)) - 0.5).abs() < 1e-6);
        assert!((tl.ratio_at(t0 + Duration::from_secs(10)) - 0.0).abs() < 1e-9);
        assert!((tl.ratio_at(t0 + Duration::from_secs(12)) - 0.0).abs() < 1e-9);

        assert!(!tl.sync_or_restart(
            t0 + Duration::from_millis(1030),
            PreBreakCountdown {
                remaining_secs: 9,
                total_secs: 10,
            },
        ));
        assert!(!tl.sync_or_restart(
            t0 + Duration::from_secs(9),
            PreBreakCountdown {
                remaining_secs: 1,
                total_secs: 10,
            },
        ));

        let t_drift = t0 + Duration::from_secs(3);
        assert!(tl.sync_or_restart(
            t_drift,
            PreBreakCountdown {
                remaining_secs: 4,
                total_secs: 10,
            },
        ));
        assert!((tl.ratio_at(t_drift) - 0.4).abs() < 1e-9);
        assert!((tl.ratio_at(t_drift + Duration::from_secs(2)) - 0.2).abs() < 1e-6);
        assert!((tl.ratio_at(t_drift + Duration::from_secs(4)) - 0.0).abs() < 1e-9);

        assert!(tl.sync_or_restart(
            t_drift + Duration::from_secs(1),
            PreBreakCountdown {
                remaining_secs: 8,
                total_secs: 10,
            },
        ));
        assert!(tl.sync_or_restart(
            t_drift + Duration::from_secs(2),
            PreBreakCountdown {
                remaining_secs: 7,
                total_secs: 15,
            },
        ));

        let mut slot: Option<ProgressTimeline> = None;
        let cd10 = PreBreakCountdown {
            remaining_secs: 10,
            total_secs: 10,
        };
        assert!(ProgressTimeline::update_or_restart(
            &mut slot, t0, cd10, false
        ));
        assert!(!ProgressTimeline::update_or_restart(
            &mut slot,
            t0 + Duration::from_secs(1),
            PreBreakCountdown {
                remaining_secs: 9,
                total_secs: 10,
            },
            false,
        ));
        assert!(ProgressTimeline::update_or_restart(
            &mut slot,
            t0 + Duration::from_secs(2),
            PreBreakCountdown {
                remaining_secs: 8,
                total_secs: 10,
            },
            true,
        ));
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
