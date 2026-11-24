//! Cross-platform system tray adapter using `tray-icon` (pure-Rust `ksni` on Linux).

use std::fmt;
use std::sync::mpsc::Sender;

use sye_core::{BreakType, PlatformError, SchedulerState, TraySnapshot};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

include!(concat!(env!("OUT_DIR"), "/tray_icons.rs"));

const MENU_ID_STATUS: &str = "sye.status";
const MENU_ID_ENABLE: &str = "sye.enable";
const MENU_ID_DISABLE_30M: &str = "sye.disable.30m";
const MENU_ID_DISABLE_1H: &str = "sye.disable.1h";
const MENU_ID_DISABLE_2H: &str = "sye.disable.2h";
const MENU_ID_DISABLE_3H: &str = "sye.disable.3h";
const MENU_ID_DISABLE_FOREVER: &str = "sye.disable.forever";
const MENU_ID_TAKE_ANY: &str = "sye.take.any";
const MENU_ID_TAKE_SHORT: &str = "sye.take.short";
const MENU_ID_TAKE_LONG: &str = "sye.take.long";
const MENU_ID_SETTINGS: &str = "sye.settings";
const MENU_ID_QUIT: &str = "sye.quit";

/// Visual state of the system tray eye icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconState {
    Enabled,
    Disabled,
    PreBreak,
}

/// User actions emitted from system tray menu clicks and OS app reopen events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Enable,
    Disable { minutes: Option<u64> },
    TakeBreak { kind: Option<BreakType> },
    OpenSettings,
    Reopen,
    Quit,
}

struct ActiveTray {
    #[cfg(target_os = "macos")]
    menu_timer: macos_menu_timer::MacMenuTimer,
    tray_icon: TrayIcon,
    icon_state: TrayIconState,
    last_snapshot: Option<TraySnapshot>,
    status_item: MenuItem,
    enable_item: MenuItem,
    disable_submenu: Submenu,
    disable_items: [MenuItem; 5],
    take_break_submenu: Submenu,
    take_break_items: [MenuItem; 3],
    settings_item: MenuItem,
    quit_item: MenuItem,
}

/// Handle to the OS system tray icon and menu, with graceful headless degradation.
pub struct TrayHandle {
    active: Option<ActiveTray>,
}

impl fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrayHandle")
            .field("available", &self.active.is_some())
            .finish()
    }
}

impl TrayHandle {
    /// Attempts to create and register the OS system tray icon and menu.
    ///
    /// Returns `Err(PlatformError::Unavailable(_))` when no tray host or display server is active.
    pub fn try_new(initial: &TraySnapshot, tx: Sender<TrayEvent>) -> Result<Self, PlatformError> {
        #[cfg(target_os = "macos")]
        if std::thread::current().name() != Some("main") {
            return Err(PlatformError::Unavailable(
                "macOS system tray must be initialized on the main thread".to_owned(),
            ));
        }

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::try_new_inner(initial, tx)
        }))
        .map_err(|_| PlatformError::Unavailable("system tray initialization panicked".to_owned()))?
    }

    fn try_new_inner(initial: &TraySnapshot, tx: Sender<TrayEvent>) -> Result<Self, PlatformError> {
        let status_item =
            MenuItem::with_id(MENU_ID_STATUS, format_status_label(initial), false, None);
        let enable_item = MenuItem::with_id(MENU_ID_ENABLE, "Enable Safe Your Eyes", true, None);

        let disable_30m = MenuItem::with_id(MENU_ID_DISABLE_30M, "For 30 minutes", true, None);
        let disable_1h = MenuItem::with_id(MENU_ID_DISABLE_1H, "For 1 hour", true, None);
        let disable_2h = MenuItem::with_id(MENU_ID_DISABLE_2H, "For 2 hours", true, None);
        let disable_3h = MenuItem::with_id(MENU_ID_DISABLE_3H, "For 3 hours", true, None);
        let disable_forever =
            MenuItem::with_id(MENU_ID_DISABLE_FOREVER, "Until restart", true, None);
        let disable_submenu = Submenu::with_items(
            "Disable Safe Your Eyes",
            true,
            &[
                &disable_30m,
                &disable_1h,
                &disable_2h,
                &disable_3h,
                &disable_forever,
            ],
        )
        .map_err(|err| PlatformError::Unavailable(err.to_string()))?;

        let take_any = MenuItem::with_id(MENU_ID_TAKE_ANY, "Any break", true, None);
        let take_short = MenuItem::with_id(MENU_ID_TAKE_SHORT, "Short break", true, None);
        let take_long = MenuItem::with_id(MENU_ID_TAKE_LONG, "Long break", true, None);
        let take_break_submenu = Submenu::with_items(
            "Take a break now",
            true,
            &[&take_any, &take_short, &take_long],
        )
        .map_err(|err| PlatformError::Unavailable(err.to_string()))?;

        let settings_item = MenuItem::with_id(MENU_ID_SETTINGS, "Settings", true, None);
        let quit_item = MenuItem::with_id(MENU_ID_QUIT, "Quit", true, None);

        let sep1 = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();

        let menu = Menu::with_items(&[
            &status_item,
            &sep1,
            &enable_item,
            &disable_submenu,
            &take_break_submenu,
            &sep2,
            &settings_item,
            &quit_item,
        ])
        .map_err(|err| PlatformError::Unavailable(err.to_string()))?;

        let icon_state = icon_state_for_snapshot(initial);
        let icon = build_tray_icon(icon_state)?;

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format_tooltip(initial));
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon);

        let tray_icon = builder
            .build()
            .map_err(|err| PlatformError::Unavailable(err.to_string()))?;

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(tray_event) = map_menu_id_to_event(event.id.as_ref()) {
                let _ = tx.send(tray_event);
            }
        }));

        #[cfg(target_os = "macos")]
        let menu_timer = macos_menu_timer::MacMenuTimer::new(initial, status_item.clone());

        let mut handle = Self {
            active: Some(ActiveTray {
                #[cfg(target_os = "macos")]
                menu_timer,
                tray_icon,
                icon_state,
                last_snapshot: None,
                status_item,
                enable_item,
                disable_submenu,
                disable_items: [
                    disable_30m,
                    disable_1h,
                    disable_2h,
                    disable_3h,
                    disable_forever,
                ],
                take_break_submenu,
                take_break_items: [take_any, take_short, take_long],
                settings_item,
                quit_item,
            }),
        };
        handle.update(initial)?;
        Ok(handle)
    }

    /// Creates a no-op tray handle for headless mode or environments without a tray host.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self { active: None }
    }

    /// Returns `true` if a live OS tray icon is registered.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        self.active.is_some()
    }

    /// Refreshes the tray icon, tooltip, status label, and menu item enabled states.
    pub fn update(&mut self, snapshot: &TraySnapshot) -> Result<(), PlatformError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        if !should_apply_snapshot(&mut active.last_snapshot, snapshot) {
            return Ok(());
        }

        let status_label = format_status_label(snapshot);
        #[cfg(target_os = "macos")]
        active.menu_timer.update(snapshot, &status_label);
        active.status_item.set_text(status_label);

        let unlocked = !snapshot.menu_locked;
        let can_enable = !snapshot.enabled && snapshot.has_breaks && unlocked;
        let can_disable = snapshot.enabled && unlocked;
        let can_take_break =
            snapshot.state == SchedulerState::Waiting && snapshot.has_breaks && unlocked;

        active.enable_item.set_enabled(can_enable);
        active.disable_submenu.set_enabled(can_disable);
        for item in &active.disable_items {
            item.set_enabled(can_disable);
        }
        active.take_break_submenu.set_enabled(can_take_break);
        for item in &active.take_break_items {
            item.set_enabled(can_take_break);
        }
        active.settings_item.set_enabled(unlocked);
        active.quit_item.set_enabled(unlocked);

        let _ = active.tray_icon.set_tooltip(Some(format_tooltip(snapshot)));

        let next_icon_state = icon_state_for_snapshot(snapshot);
        if next_icon_state != active.icon_state {
            let icon = build_tray_icon(next_icon_state)?;
            #[cfg(target_os = "macos")]
            let updated = active.tray_icon.set_icon_templated(Some(icon)).is_ok();
            #[cfg(not(target_os = "macos"))]
            let updated = active.tray_icon.set_icon(Some(icon)).is_ok();
            if updated {
                active.icon_state = next_icon_state;
            }
        }

        Ok(())
    }
}

pub(crate) fn should_apply_snapshot(
    last_snapshot: &mut Option<TraySnapshot>,
    next: &TraySnapshot,
) -> bool {
    if last_snapshot.as_ref() == Some(next) {
        return false;
    }
    *last_snapshot = Some(next.clone());
    true
}

#[cfg(any(target_os = "macos", test))]
#[must_use]
pub(crate) fn advance_snapshot_countdown(
    snapshot: &TraySnapshot,
    elapsed_secs: u64,
) -> TraySnapshot {
    let mut live = snapshot.clone();
    if live.enabled
        && matches!(
            live.state,
            SchedulerState::Waiting | SchedulerState::PreBreak
        )
    {
        live.seconds_until_next_break = live.seconds_until_next_break.saturating_sub(elapsed_secs);
    }
    live
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "NSTimer block scheduling in AppKit NSEventTrackingRunLoopMode"
)]
mod macos_menu_timer {
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::time::Instant;

    use block2::RcBlock;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2_app_kit::NSEventTrackingRunLoopMode;
    use objc2_foundation::{NSRunLoop, NSTimer};
    use sye_core::TraySnapshot;
    use tray_icon::menu::MenuItem;

    use super::{advance_snapshot_countdown, format_status_label};

    struct MacMenuLiveState {
        snapshot: TraySnapshot,
        updated_at: Instant,
        last_label: String,
        status_item: MenuItem,
    }

    pub(super) struct MacMenuTimer {
        state: Rc<RefCell<MacMenuLiveState>>,
        timer: Retained<NSTimer>,
    }

    impl MacMenuTimer {
        pub(super) fn new(initial: &TraySnapshot, status_item: MenuItem) -> Self {
            let state = Rc::new(RefCell::new(MacMenuLiveState {
                snapshot: initial.clone(),
                updated_at: Instant::now(),
                last_label: format_status_label(initial),
                status_item,
            }));
            let timer_state = Rc::clone(&state);
            // AppKit enters modal `NSEventTrackingRunLoopMode` while the status bar menu is open,
            // pausing `daemon.rs`'s main loop; this timer keeps the countdown label ticking live.
            let timer = autoreleasepool(|_| {
                let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
                    let Ok(mut guard) = timer_state.try_borrow_mut() else {
                        return;
                    };
                    let elapsed_secs = guard.updated_at.elapsed().as_secs();
                    let live = advance_snapshot_countdown(&guard.snapshot, elapsed_secs);
                    let label = format_status_label(&live);
                    if label != guard.last_label {
                        guard.last_label.clone_from(&label);
                        let item = guard.status_item.clone();
                        drop(guard);
                        item.set_text(label);
                    }
                });
                // SAFETY: `TrayHandle::try_new` enforces main-thread creation on macOS, and the
                // timer is scheduled exclusively on `NSRunLoop::mainRunLoop()`, so `block` only
                // executes on the main thread.
                let timer =
                    unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.25, true, &block) };
                // SAFETY: `NSEventTrackingRunLoopMode` is an immutable AppKit constant.
                unsafe {
                    let mode = NSEventTrackingRunLoopMode;
                    NSRunLoop::mainRunLoop().addTimer_forMode(&timer, mode);
                }
                timer
            });
            Self { state, timer }
        }

        pub(super) fn update(&self, snapshot: &TraySnapshot, label: &str) {
            if let Ok(mut guard) = self.state.try_borrow_mut() {
                guard.snapshot = snapshot.clone();
                guard.updated_at = Instant::now();
                guard.last_label.clear();
                guard.last_label.push_str(label);
            }
        }
    }

    impl Drop for MacMenuTimer {
        fn drop(&mut self) {
            self.timer.invalidate();
        }
    }
}

/// Maps a `TraySnapshot` to its corresponding `TrayIconState`.
#[must_use]
pub const fn icon_state_for_snapshot(snapshot: &TraySnapshot) -> TrayIconState {
    if !snapshot.enabled {
        TrayIconState::Disabled
    } else if matches!(
        snapshot.state,
        SchedulerState::PreBreak | SchedulerState::Break
    ) {
        TrayIconState::PreBreak
    } else {
        TrayIconState::Enabled
    }
}

/// Maps a tray menu item ID to a strongly-typed `TrayEvent`.
#[must_use]
pub fn map_menu_id_to_event(id: &str) -> Option<TrayEvent> {
    match id {
        MENU_ID_ENABLE => Some(TrayEvent::Enable),
        MENU_ID_DISABLE_30M => Some(TrayEvent::Disable { minutes: Some(30) }),
        MENU_ID_DISABLE_1H => Some(TrayEvent::Disable { minutes: Some(60) }),
        MENU_ID_DISABLE_2H => Some(TrayEvent::Disable { minutes: Some(120) }),
        MENU_ID_DISABLE_3H => Some(TrayEvent::Disable { minutes: Some(180) }),
        MENU_ID_DISABLE_FOREVER => Some(TrayEvent::Disable { minutes: None }),
        MENU_ID_TAKE_ANY => Some(TrayEvent::TakeBreak { kind: None }),
        MENU_ID_TAKE_SHORT => Some(TrayEvent::TakeBreak {
            kind: Some(BreakType::Short),
        }),
        MENU_ID_TAKE_LONG => Some(TrayEvent::TakeBreak {
            kind: Some(BreakType::Long),
        }),
        MENU_ID_SETTINGS => Some(TrayEvent::OpenSettings),
        MENU_ID_QUIT => Some(TrayEvent::Quit),
        _ => None,
    }
}

/// Formats the top status menu line for the system tray.
#[must_use]
pub fn format_status_label(snapshot: &TraySnapshot) -> String {
    if !snapshot.has_breaks {
        return "No breaks configured".to_owned();
    }
    if !snapshot.enabled || snapshot.state == SchedulerState::Stopped {
        return match snapshot.disabled_until_unix_secs {
            Some(_) => "Disabled (timed)".to_owned(),
            None => "Disabled until restart".to_owned(),
        };
    }

    match snapshot.state {
        SchedulerState::Waiting => {
            let kind = match snapshot.next_break_type {
                Some(BreakType::Short) => "short break",
                Some(BreakType::Long) => "long break",
                None => "break",
            };
            let mins = snapshot.seconds_until_next_break / 60;
            let secs = snapshot.seconds_until_next_break % 60;
            format!("Next {kind} in {mins:02}:{secs:02}")
        }
        SchedulerState::PreBreak => {
            let mins = snapshot.seconds_until_next_break / 60;
            let secs = snapshot.seconds_until_next_break % 60;
            format!("Break starting in {mins:02}:{secs:02}")
        }
        SchedulerState::Break => "Break in progress".to_owned(),
        SchedulerState::Resting => "Paused (idle/resting)".to_owned(),
        SchedulerState::Start => "Starting...".to_owned(),
        SchedulerState::Stopped => "Disabled".to_owned(),
        SchedulerState::Quit => "Quitting...".to_owned(),
    }
}

/// Formats the hover tooltip for the system tray icon.
#[must_use]
pub fn format_tooltip(snapshot: &TraySnapshot) -> String {
    format!("Safe Your Eyes — {}", format_status_label(snapshot))
}

fn build_tray_icon(state: TrayIconState) -> Result<Icon, PlatformError> {
    let (rgba, width, height) = render_eye_icon_rgba(state);
    Icon::from_rgba(rgba, width, height).map_err(|err| PlatformError::Io(err.to_string()))
}

/// Returns the platform-appropriate 64x64 RGBA tray icon for the given `TrayIconState`.
///
/// Uses monochrome template icons on macOS and Cozy colored icons on Linux and Windows.
#[must_use]
pub fn render_eye_icon_rgba(state: TrayIconState) -> (Vec<u8>, u32, u32) {
    render_tray_icon_rgba(state, cfg!(target_os = "macos"))
}

/// Returns the 64x64 RGBA tray icon for `state`, choosing either the monochrome macOS template
/// variant (`template == true`) or the Cozy colored variant (`template == false`).
#[must_use]
pub fn render_tray_icon_rgba(state: TrayIconState, template: bool) -> (Vec<u8>, u32, u32) {
    let encoded = match (template, state) {
        (false, TrayIconState::Enabled) => COLOR_ENABLED_RLE,
        (false, TrayIconState::PreBreak) => COLOR_PRE_BREAK_RLE,
        (false, TrayIconState::Disabled) => COLOR_DISABLED_RLE,
        (true, TrayIconState::Enabled) => TEMPLATE_ENABLED_RLE,
        (true, TrayIconState::PreBreak) => TEMPLATE_PRE_BREAK_RLE,
        (true, TrayIconState::Disabled) => TEMPLATE_DISABLED_RLE,
    };
    (decode_rle_rgba(encoded), ICON_SIZE, ICON_SIZE)
}

const ICON_RGBA_LEN: usize = (ICON_SIZE as usize) * (ICON_SIZE as usize) * 4;

fn decode_rle_rgba(encoded: &[u8]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(ICON_RGBA_LEN);
    for &[count, r, g, b, a] in encoded.as_chunks::<5>().0 {
        let rgba = [r, g, b, a];
        for _ in 0..usize::from(count) {
            pixels.extend_from_slice(&rgba);
        }
    }
    pixels.resize(ICON_RGBA_LEN, 0);
    pixels
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn sample_snapshot(state: SchedulerState, enabled: bool) -> TraySnapshot {
        TraySnapshot {
            state,
            enabled,
            has_breaks: true,
            menu_locked: false,
            next_break_type: Some(BreakType::Short),
            next_break_name: Some("Blink".to_owned()),
            seconds_until_next_break: 890,
            disabled_until_unix_secs: None,
            consecutive_skips: 0,
        }
    }

    #[test]
    fn eye_icons_are_valid_rgba_and_visually_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let expected_bytes = (ICON_SIZE * ICON_SIZE * 4) as usize;
        for template in [false, true] {
            let (enabled, w1, h1) = render_tray_icon_rgba(TrayIconState::Enabled, template);
            let (disabled, w2, h2) = render_tray_icon_rgba(TrayIconState::Disabled, template);
            let (pre_break, w3, h3) = render_tray_icon_rgba(TrayIconState::PreBreak, template);

            assert_eq!((w1, h1), (ICON_SIZE, ICON_SIZE));
            assert_eq!((w2, h2), (ICON_SIZE, ICON_SIZE));
            assert_eq!((w3, h3), (ICON_SIZE, ICON_SIZE));
            assert_eq!(enabled.len(), expected_bytes);
            assert_eq!(disabled.len(), expected_bytes);
            assert_eq!(pre_break.len(), expected_bytes);

            assert_ne!(enabled, disabled);
            assert_ne!(enabled, pre_break);
            assert_ne!(disabled, pre_break);

            if template {
                for rgba in [&enabled, &disabled, &pre_break] {
                    let pixels = rgba.as_chunks::<4>().0;
                    assert!(
                        pixels
                            .iter()
                            .all(|&[r, g, b, _]| r == 0 && g == 0 && b == 0),
                        "macOS template tray icons must encode shape in alpha with RGB=(0,0,0)"
                    );
                    assert!(
                        pixels.iter().any(|&[_, _, _, a]| a > 0),
                        "template tray icon must contain visible pixels"
                    );
                }
            }

            Icon::from_rgba(enabled, w1, h1)?;
            Icon::from_rgba(disabled, w2, h2)?;
            Icon::from_rgba(pre_break, w3, h3)?;
        }

        let (default_enabled, w, h) = render_eye_icon_rgba(TrayIconState::Enabled);
        assert_eq!((w, h), (ICON_SIZE, ICON_SIZE));
        Icon::from_rgba(default_enabled, w, h)?;
        Ok(())
    }

    #[test]
    fn map_menu_id_covers_all_interactive_items() {
        assert_eq!(
            map_menu_id_to_event(MENU_ID_ENABLE),
            Some(TrayEvent::Enable)
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_DISABLE_30M),
            Some(TrayEvent::Disable { minutes: Some(30) })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_DISABLE_1H),
            Some(TrayEvent::Disable { minutes: Some(60) })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_DISABLE_2H),
            Some(TrayEvent::Disable { minutes: Some(120) })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_DISABLE_3H),
            Some(TrayEvent::Disable { minutes: Some(180) })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_DISABLE_FOREVER),
            Some(TrayEvent::Disable { minutes: None })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_TAKE_ANY),
            Some(TrayEvent::TakeBreak { kind: None })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_TAKE_SHORT),
            Some(TrayEvent::TakeBreak {
                kind: Some(BreakType::Short)
            })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_TAKE_LONG),
            Some(TrayEvent::TakeBreak {
                kind: Some(BreakType::Long)
            })
        );
        assert_eq!(
            map_menu_id_to_event(MENU_ID_SETTINGS),
            Some(TrayEvent::OpenSettings)
        );
        assert_eq!(map_menu_id_to_event(MENU_ID_QUIT), Some(TrayEvent::Quit));
        assert_eq!(map_menu_id_to_event(MENU_ID_STATUS), None);
        assert_eq!(map_menu_id_to_event("unknown"), None);
    }

    #[test]
    fn status_and_tooltip_format_all_scheduler_states() {
        let waiting = sample_snapshot(SchedulerState::Waiting, true);
        assert_eq!(
            format_tooltip(&waiting),
            "Safe Your Eyes — Next short break in 14:50"
        );
        assert_eq!(icon_state_for_snapshot(&waiting), TrayIconState::Enabled);

        let mut pre = sample_snapshot(SchedulerState::PreBreak, true);
        pre.seconds_until_next_break = 10;
        assert_eq!(format_status_label(&pre), "Break starting in 00:10");
        assert_eq!(icon_state_for_snapshot(&pre), TrayIconState::PreBreak);

        let resting = sample_snapshot(SchedulerState::Resting, true);
        assert_eq!(format_status_label(&resting), "Paused (idle/resting)");

        let mut stopped = sample_snapshot(SchedulerState::Stopped, false);
        assert_eq!(format_status_label(&stopped), "Disabled until restart");
        assert_eq!(icon_state_for_snapshot(&stopped), TrayIconState::Disabled);

        stopped.disabled_until_unix_secs = Some(1_800_000_000);
        assert_eq!(format_status_label(&stopped), "Disabled (timed)");

        stopped.has_breaks = false;
        assert_eq!(format_status_label(&stopped), "No breaks configured");
    }

    #[test]
    fn unavailable_tray_handle_is_noop() -> Result<(), Box<dyn std::error::Error>> {
        let mut tray = TrayHandle::unavailable();
        assert!(!tray.is_available());
        tray.update(&sample_snapshot(SchedulerState::Waiting, true))?;
        Ok(())
    }

    #[test]
    fn try_new_degrades_cleanly_in_headless_env() {
        let (tx, _rx) = mpsc::channel();
        let snap = sample_snapshot(SchedulerState::Waiting, true);
        let res = TrayHandle::try_new(&snap, tx);
        assert!(matches!(res, Ok(_) | Err(PlatformError::Unavailable(_))));
    }

    #[test]
    fn snapshot_dedup_and_live_countdown_progression() {
        let mut last = None;
        let waiting = sample_snapshot(SchedulerState::Waiting, true);
        assert!(should_apply_snapshot(&mut last, &waiting));
        assert!(!should_apply_snapshot(&mut last, &waiting));

        let next_sec = advance_snapshot_countdown(&waiting, 1);
        assert_eq!(next_sec.seconds_until_next_break, 889);
        assert_eq!(format_status_label(&next_sec), "Next short break in 14:49");
        assert!(should_apply_snapshot(&mut last, &next_sec));
        assert!(!should_apply_snapshot(&mut last, &next_sec));

        let saturated = advance_snapshot_countdown(&waiting, 5_000);
        assert_eq!(saturated.seconds_until_next_break, 0);
        assert_eq!(format_status_label(&saturated), "Next short break in 00:00");

        let mut pre = sample_snapshot(SchedulerState::PreBreak, true);
        pre.seconds_until_next_break = 5;
        assert_eq!(
            format_status_label(&advance_snapshot_countdown(&pre, 2)),
            "Break starting in 00:03"
        );

        let resting = sample_snapshot(SchedulerState::Resting, true);
        assert_eq!(advance_snapshot_countdown(&resting, 30), resting);

        let disabled = sample_snapshot(SchedulerState::Waiting, false);
        assert_eq!(advance_snapshot_countdown(&disabled, 30), disabled);
    }
}
