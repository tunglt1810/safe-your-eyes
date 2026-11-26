#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(all(unix, not(target_os = "macos")))]
use notify_rust::{Notification, Timeout};
use sye_core::{BreakType, Notifier, PlatformError};

#[cfg(all(unix, not(target_os = "macos")))]
const APP_NAME: &str = "Safe Your Eyes";
#[cfg(all(unix, not(target_os = "macos")))]
const APP_ICON: &str = "safe-your-eyes";

pub struct OsNotifier {
    enabled: bool,
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
            .finish_non_exhaustive()
    }
}

impl OsNotifier {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            enabled: true,
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }

    #[must_use]
    pub const fn noop() -> Self {
        Self {
            enabled: false,
            #[cfg(all(unix, not(target_os = "macos")))]
            active: Mutex::new(None),
        }
    }

    pub fn show_notice(&self, summary: &str, body: &str) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            macos_impl::show_pre_break(summary, body)
        }

        #[cfg(windows)]
        {
            windows_impl::show_pre_break(summary, body)
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
    ) -> Result<(), PlatformError> {
        if !self.enabled {
            return Ok(());
        }

        let (summary, body) = format_pre_break_notification(break_type, title, warning_secs);

        #[cfg(target_os = "macos")]
        {
            macos_impl::show_pre_break(&summary, &body)
        }

        #[cfg(windows)]
        {
            windows_impl::show_pre_break(&summary, &body)
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let timeout_ms = u32::try_from(warning_secs.saturating_mul(1000)).unwrap_or(u32::MAX);

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
        BeginPaint, COLOR_GRAYTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW,
        CreateSolidBrush, DRAW_TEXT_FORMAT, DT_CALCRECT, DT_EDITCONTROL, DT_END_ELLIPSIS,
        DT_NOPREFIX, DT_SINGLELINE, DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FillRect,
        GetDC, GetMonitorInfoW, GetSysColor, GetTextMetricsW, HDC, HFONT, InvalidateRect,
        MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, PAINTSTRUCT, ReleaseDC,
        SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT,
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
        SWP_SHOWWINDOW, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEACTIVATE,
        WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    use super::compute_windows_banner_origin;

    const CLASS_NAME: &str = "SyePreBreakBanner";
    const WINDOW_TITLE: &str = "Safe Your Eyes";
    const APP_ICON_RESOURCE_ID: usize = 1;
    const BASE_DPI: i32 = 96;
    const BANNER_WIDTH: i32 = 356;
    const MIN_HEIGHT: i32 = 64;
    const ICON_SIZE: i32 = 38;
    const ICON_X: i32 = 13;
    const TEXT_X: i32 = 62;
    const PAD_RIGHT: i32 = 14;
    const PAD_Y: i32 = 12;
    const LINE_GAP: i32 = 2;
    const MARGIN: i32 = 12;
    const BODY_MAX_LINES: i32 = 2;
    const TITLE_FONT_WEIGHT: i32 = 600;
    const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

    struct Palette {
        background: COLORREF,
        title: COLORREF,
        body: COLORREF,
    }

    const DARK_PALETTE: Palette = Palette {
        background: 0x002C_2C2C,
        title: 0x00FF_FFFF,
        body: 0x00CF_CFCF,
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
            }
        }
    }

    fn message_font(dpi: i32, weight: Option<i32>) -> HFONT {
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
        summary: Vec<u16>,
        body: Vec<u16>,
        size: (i32, i32),
        icon_rect: RECT,
        title_rect: RECT,
        body_rect: RECT,
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
                summary: Vec::new(),
                body: Vec::new(),
                size: (0, 0),
                icon_rect: RECT::default(),
                title_rect: RECT::default(),
                body_rect: RECT::default(),
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
                if !self.icon.is_null() {
                    DestroyIcon(self.icon);
                }
            }
            self.title_font = ptr::null_mut();
            self.body_font = ptr::null_mut();
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
            self.title_font = message_font(dpi, Some(TITLE_FONT_WEIGHT));
            self.body_font = message_font(dpi, None);
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
            scale(BANNER_WIDTH - TEXT_X - PAD_RIGHT, self.dpi)
        }

        fn layout(&mut self) {
            let dpi = self.dpi;
            // SAFETY: `self.hwnd` is the live banner window.
            let hdc = unsafe { GetDC(self.hwnd) };
            let title_h = self.measure(hdc, self.title_font, &self.summary, DT_SINGLELINE);
            let body_full_h = self.measure(hdc, self.body_font, &self.body, DT_WORDBREAK);
            let body_line_h = Self::line_height(hdc, self.body_font);
            // SAFETY: releases the DC that `GetDC` returned for this window.
            unsafe { ReleaseDC(self.hwnd, hdc) };

            let body_h = body_full_h.min(body_line_h * BODY_MAX_LINES);
            let gap = scale(LINE_GAP, dpi);
            let text_block_h = title_h + gap + body_h;
            let width = scale(BANNER_WIDTH, dpi);
            let height = (text_block_h + scale(PAD_Y, dpi) * 2).max(scale(MIN_HEIGHT, dpi));
            self.size = (width, height);

            let icon = scale(ICON_SIZE, dpi);
            let icon_x = scale(ICON_X, dpi);
            let icon_y = (height - icon) / 2;
            self.icon_rect = RECT {
                left: icon_x,
                top: icon_y,
                right: icon_x + icon,
                bottom: icon_y + icon,
            };

            let text_x = scale(TEXT_X, dpi);
            let text_right = text_x + self.text_width();
            let title_top = (height - text_block_h) / 2;
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
        }

        fn show_or_update(&mut self, summary: &str, body: &str) -> Result<(), PlatformError> {
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
            let body_changed = self.body != body;
            let dpi_changed = self.dpi != dpi;
            // SAFETY: `self.hwnd` is the live banner window.
            let was_visible = unsafe { IsWindowVisible(self.hwnd) } != 0;
            self.summary = summary;
            self.body = body;
            if dpi_changed {
                self.apply_dpi(dpi);
            }
            let dark = apps_use_dark_theme();
            if self.dark != Some(dark) {
                self.apply_theme(dark);
            }
            if !was_visible || body_changed || dpi_changed {
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

    pub(super) fn show_pre_break(summary: &str, body: &str) -> Result<(), PlatformError> {
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
            guard
                .as_mut()
                .map_or(Ok(()), |banner| banner.show_or_update(summary, body))
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
    fn windows_banner_anchors_bottom_right_inside_work_area() {
        assert_eq!(
            compute_windows_banner_origin((0, 0, 1920, 1032), (356, 64), 12),
            (1552, 956)
        );
        assert_eq!(
            compute_windows_banner_origin((-1280, 0, 0, 984), (445, 80), 15),
            (-460, 889)
        );
        assert_eq!(
            compute_windows_banner_origin((100, 40, 400, 100), (356, 64), 12),
            (112, 52)
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
