#[cfg(target_os = "linux")]
use std::sync::Mutex;
use std::time::Duration;

use sye_core::PlatformError;

pub struct SystemIdleProvider {
    #[cfg(target_os = "linux")]
    x11: Mutex<Option<linux_impl::X11IdleConn>>,
    #[cfg(target_os = "linux")]
    dbus: Mutex<Option<zbus::blocking::Connection>>,
}

impl std::fmt::Debug for SystemIdleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemIdleProvider").finish_non_exhaustive()
    }
}

impl SystemIdleProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            x11: Mutex::new(None),
            #[cfg(target_os = "linux")]
            dbus: Mutex::new(None),
        }
    }

    pub fn idle_duration(&self) -> Result<Duration, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            linux_impl::query_idle_duration(&self.x11, &self.dbus)
        }
        #[cfg(target_os = "macos")]
        {
            macos_impl::query_idle_duration()
        }
        #[cfg(target_os = "windows")]
        {
            windows_impl::query_idle_duration()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(PlatformError::Unsupported)
        }
    }
}

impl Default for SystemIdleProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
fn parse_macos_hid_idle_time(output: &str) -> Option<Duration> {
    for line in output.lines() {
        if !line.contains("\"HIDIdleTime\"") {
            continue;
        }
        let Some((_, rhs)) = line.split_once('=') else {
            continue;
        };
        let Ok(nanos) = rhs.trim().parse::<u64>() else {
            continue;
        };
        return Some(Duration::from_nanos(nanos));
    }
    None
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use sye_core::PlatformError;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::screensaver;
    use x11rb::protocol::xproto::Window;
    use x11rb::rust_connection::RustConnection;

    pub(super) struct X11IdleConn {
        conn: RustConnection,
        root: Window,
    }

    impl X11IdleConn {
        fn connect() -> Option<Self> {
            let (conn, screen_num) = RustConnection::connect(None).ok()?;
            let root = conn.setup().roots.get(screen_num)?.root;
            Some(Self { conn, root })
        }

        fn query_ms(&self) -> Option<u32> {
            let reply = screensaver::query_info(&self.conn, self.root)
                .ok()?
                .reply()
                .ok()?;
            Some(reply.ms_since_user_input)
        }
    }

    pub(super) fn query_idle_duration(
        x11_cache: &Mutex<Option<X11IdleConn>>,
        dbus_cache: &Mutex<Option<zbus::blocking::Connection>>,
    ) -> Result<Duration, PlatformError> {
        // Under Wayland with XWayland, X11 MIT-SCREEN-SAVER ignores native Wayland input
        // and reports stale idle times; query the session D-Bus idle monitor first.
        let wayland_session =
            std::env::var_os("WAYLAND_DISPLAY").is_some_and(|val| !val.is_empty());

        if wayland_session {
            if let Some(dur) = query_wayland_dbus_idle(dbus_cache) {
                return Ok(dur);
            }
            if let Some(dur) = query_x11_idle(x11_cache) {
                return Ok(dur);
            }
        } else {
            if let Some(dur) = query_x11_idle(x11_cache) {
                return Ok(dur);
            }
            if let Some(dur) = query_wayland_dbus_idle(dbus_cache) {
                return Ok(dur);
            }
        }
        Err(PlatformError::Unavailable(
            "no X11 display or session D-Bus idle monitor available".to_owned(),
        ))
    }

    fn query_x11_idle(cache: &Mutex<Option<X11IdleConn>>) -> Option<Duration> {
        let ms = {
            let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(existing) = guard.as_ref()
                && let Some(ms) = existing.query_ms()
            {
                Some(ms)
            } else {
                *guard = X11IdleConn::connect();
                guard.as_ref().and_then(X11IdleConn::query_ms)
            }
        }?;
        Some(Duration::from_millis(u64::from(ms)))
    }

    fn query_dbus_idle_on(conn: &zbus::blocking::Connection) -> Option<Duration> {
        if let Ok(msg) = conn.call_method(
            Some("org.gnome.Mutter.IdleMonitor"),
            "/org/gnome/Mutter/IdleMonitor/Core",
            Some("org.gnome.Mutter.IdleMonitor"),
            "GetIdletime",
            &(),
        ) && let Ok(ms) = msg.body().deserialize::<u64>()
        {
            return Some(Duration::from_millis(ms));
        }

        if let Ok(msg) = conn.call_method(
            Some("org.freedesktop.ScreenSaver"),
            "/org/freedesktop/ScreenSaver",
            Some("org.freedesktop.ScreenSaver"),
            "GetSessionIdleTime",
            &(),
        ) && let Ok(ms) = msg.body().deserialize::<u32>()
        {
            return Some(Duration::from_millis(u64::from(ms)));
        }

        None
    }

    fn query_wayland_dbus_idle(
        cache: &Mutex<Option<zbus::blocking::Connection>>,
    ) -> Option<Duration> {
        let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = guard.as_ref()
            && let Some(dur) = query_dbus_idle_on(existing)
        {
            return Some(dur);
        }
        *guard = zbus::blocking::Connection::session().ok();
        guard.as_ref().and_then(query_dbus_idle_on)
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::time::Duration;

    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
    use sye_core::PlatformError;

    // CoreGraphics kCGAnyInputEventType (~0u32) matches any HID input event without permissions.
    const ANY_INPUT_EVENT_TYPE: CGEventType = CGEventType(u32::MAX);

    pub(super) fn query_idle_duration() -> Result<Duration, PlatformError> {
        let secs = CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            ANY_INPUT_EVENT_TYPE,
        );
        if !secs.is_finite() || secs < 0.0 {
            return Err(PlatformError::Unavailable(
                "CGEventSourceSecondsSinceLastEventType returned invalid duration".to_owned(),
            ));
        }
        Ok(Duration::from_secs_f64(secs))
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::size_of;
    use std::time::Duration;

    use sye_core::PlatformError;
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    const LASTINPUTINFO_SIZE_U32: u32 = const {
        assert!(size_of::<LASTINPUTINFO>() <= u32::MAX as usize);
        size_of::<LASTINPUTINFO>() as u32
    };

    pub(super) fn query_idle_duration() -> Result<Duration, PlatformError> {
        let mut lii = LASTINPUTINFO {
            cbSize: LASTINPUTINFO_SIZE_U32,
            dwTime: 0,
        };
        // SAFETY: `lii` is properly initialized with its exact byte size and valid for writes.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let ok = unsafe { GetLastInputInfo(&mut lii) };
        if ok == 0 {
            return Err(PlatformError::Unavailable(
                "GetLastInputInfo failed".to_owned(),
            ));
        }
        // SAFETY: `GetTickCount` has no preconditions and returns monotonic 32-bit ms since boot.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let now_ms = unsafe { GetTickCount() };
        let elapsed_ms = now_ms.wrapping_sub(lii.dwTime);
        Ok(Duration::from_millis(u64::from(elapsed_ms)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_macos_hid_idle_time_extracts_nanoseconds() {
        let sample = r#"
          | |   "HIDIdleTime" = 4250000000
          | |   "HIDParameters" = {}
        "#;
        assert_eq!(
            parse_macos_hid_idle_time(sample),
            Some(Duration::from_nanos(4_250_000_000))
        );

        assert_eq!(
            parse_macos_hid_idle_time(r#""HIDIdleTime" = 0"#),
            Some(Duration::ZERO)
        );
        assert_eq!(parse_macos_hid_idle_time("no idle key here"), None);
        assert_eq!(parse_macos_hid_idle_time(r#""HIDIdleTime" = abc"#), None);
    }

    #[test]
    fn system_idle_provider_degrades_gracefully_in_headless_env() {
        let provider = SystemIdleProvider::new();
        let res = provider.idle_duration();
        assert!(matches!(res, Ok(_) | Err(PlatformError::Unavailable(_))));
    }
}
