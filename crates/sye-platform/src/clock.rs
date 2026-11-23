//! Real OS clock implementation of `sye_core::Clock`.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sye_core::Clock;

/// Monotonic and wall-clock time source backed by `std::time`.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// Creates a new `SystemClock` anchored at the current monotonic instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_monotonic(&self) -> Duration {
        self.origin.elapsed()
    }

    fn now_wall_unix(&self) -> Duration {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn monotonic_clock_is_non_decreasing() {
        let clock = SystemClock::new();
        let t1 = clock.now_monotonic();
        thread::sleep(Duration::from_millis(2));
        let t2 = clock.now_monotonic();
        assert!(t2 >= t1);
    }

    #[test]
    fn wall_clock_returns_plausible_unix_timestamp() {
        let clock = SystemClock::default();
        let wall = clock.now_wall_unix();
        assert!(wall.as_secs() > 1_700_000_000);

        let clock_ref: &SystemClock = &clock;
        assert!(clock_ref.now_wall_unix().as_secs() > 1_700_000_000);
    }
}
