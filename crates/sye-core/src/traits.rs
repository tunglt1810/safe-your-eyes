use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    Unsupported,
    Unavailable(String),
    Io(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "platform feature unsupported"),
            Self::Unavailable(msg) => write!(f, "platform feature unavailable: {msg}"),
            Self::Io(msg) => write!(f, "platform I/O error: {msg}"),
        }
    }
}

impl std::error::Error for PlatformError {}

pub trait Clock {
    fn now_monotonic(&self) -> Duration;

    fn now_wall_unix(&self) -> Duration;
}

#[derive(Debug)]
pub struct FakeClock {
    mono_ms: AtomicU64,
    wall_ms: AtomicU64,
}

impl FakeClock {
    #[must_use]
    pub fn new(mono: Duration, wall_unix: Duration) -> Self {
        Self {
            mono_ms: AtomicU64::new(duration_to_ms_u64(mono)),
            wall_ms: AtomicU64::new(duration_to_ms_u64(wall_unix)),
        }
    }

    pub fn advance(&self, delta: Duration) {
        let delta_ms = duration_to_ms_u64(delta);
        saturating_add_atomic(&self.mono_ms, delta_ms);
        saturating_add_atomic(&self.wall_ms, delta_ms);
    }

    pub fn advance_secs(&self, secs: u64) {
        self.advance(Duration::from_secs(secs));
    }

    pub fn advance_wall_only(&self, delta: Duration) {
        saturating_add_atomic(&self.wall_ms, duration_to_ms_u64(delta));
    }

    pub fn set_wall_unix(&self, wall_unix: Duration) {
        self.wall_ms
            .store(duration_to_ms_u64(wall_unix), Ordering::SeqCst);
    }
}

fn saturating_add_atomic(target: &AtomicU64, delta: u64) {
    let mut cur = target.load(Ordering::SeqCst);
    loop {
        let next = cur.saturating_add(delta);
        match target.compare_exchange_weak(cur, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(actual) => cur = actual,
        }
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new(Duration::ZERO, Duration::from_secs(1_700_000_000))
    }
}

impl Clock for FakeClock {
    fn now_monotonic(&self) -> Duration {
        Duration::from_millis(self.mono_ms.load(Ordering::SeqCst))
    }

    fn now_wall_unix(&self) -> Duration {
        Duration::from_millis(self.wall_ms.load(Ordering::SeqCst))
    }
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now_monotonic(&self) -> Duration {
        (**self).now_monotonic()
    }

    fn now_wall_unix(&self) -> Duration {
        (**self).now_wall_unix()
    }
}

pub(crate) fn duration_to_ms_u64(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DndQueryOptions {
    pub check_fullscreen: bool,
    pub check_gaming: bool,
}

impl Default for DndQueryOptions {
    fn default() -> Self {
        Self {
            check_fullscreen: true,
            check_gaming: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_trait_is_object_safe_and_mockable() {
        let clock: Box<dyn Clock> = Box::new(FakeClock::default());
        assert_eq!(clock.now_monotonic(), Duration::ZERO);
    }
}
