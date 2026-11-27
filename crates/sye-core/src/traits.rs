use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::config::BreakType;
use crate::policies::DndContext;
use crate::scheduler::{OverlayOutcome, OverlayRequest};

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
        self.advance_mono_ms(delta_ms);
        self.advance_wall_ms(delta_ms);
    }

    pub fn advance_secs(&self, secs: u64) {
        self.advance(Duration::from_secs(secs));
    }

    pub fn advance_wall_only(&self, delta: Duration) {
        self.advance_wall_ms(duration_to_ms_u64(delta));
    }

    pub fn set_wall_unix(&self, wall_unix: Duration) {
        self.wall_ms
            .store(duration_to_ms_u64(wall_unix), Ordering::SeqCst);
    }

    fn advance_mono_ms(&self, delta_ms: u64) {
        saturating_add_atomic(&self.mono_ms, delta_ms);
    }

    fn advance_wall_ms(&self, delta_ms: u64) {
        saturating_add_atomic(&self.wall_ms, delta_ms);
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

pub trait IdleProvider {
    fn idle_duration(&self) -> Result<Duration, PlatformError>;
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

pub trait DndProvider {
    fn query_dnd_context(&self) -> Result<DndContext, PlatformError>;

    fn query_dnd_context_with(
        &self,
        _options: DndQueryOptions,
    ) -> Result<DndContext, PlatformError> {
        self.query_dnd_context()
    }
}

pub trait Notifier {
    fn show_pre_break(
        &self,
        break_type: BreakType,
        title: &str,
        warning_secs: u64,
    ) -> Result<(), PlatformError>;
    fn dismiss(&self) -> Result<(), PlatformError>;
}

pub trait AudioPlayer {
    fn play_wav(&self, wav_bytes: &[u8]) -> Result<(), PlatformError>;
}

pub trait OverlayLauncher {
    fn launch(&mut self, request: &OverlayRequest) -> Result<(), PlatformError>;
    fn poll_outcome(&mut self) -> Result<Option<OverlayOutcome>, PlatformError>;
    fn close(&mut self) -> Result<(), PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyIdle;
    impl IdleProvider for DummyIdle {
        fn idle_duration(&self) -> Result<Duration, PlatformError> {
            Ok(Duration::from_secs(3))
        }
    }

    struct DummyDnd;
    impl DndProvider for DummyDnd {
        fn query_dnd_context(&self) -> Result<DndContext, PlatformError> {
            Ok(DndContext::default())
        }
    }

    struct DummyNotifier;
    impl Notifier for DummyNotifier {
        fn show_pre_break(
            &self,
            _break_type: BreakType,
            _title: &str,
            _warning_secs: u64,
        ) -> Result<(), PlatformError> {
            Ok(())
        }
        fn dismiss(&self) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    struct DummyAudio;
    impl AudioPlayer for DummyAudio {
        fn play_wav(&self, _wav_bytes: &[u8]) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    struct DummyOverlay;
    impl OverlayLauncher for DummyOverlay {
        fn launch(&mut self, _request: &OverlayRequest) -> Result<(), PlatformError> {
            Ok(())
        }
        fn poll_outcome(&mut self) -> Result<Option<OverlayOutcome>, PlatformError> {
            Ok(None)
        }
        fn close(&mut self) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn platform_traits_are_object_safe_and_mockable() -> Result<(), Box<dyn std::error::Error>> {
        let clock: Box<dyn Clock> = Box::new(FakeClock::default());
        assert_eq!(clock.now_monotonic(), Duration::ZERO);

        let idle: Box<dyn IdleProvider> = Box::new(DummyIdle);
        assert_eq!(idle.idle_duration()?, Duration::from_secs(3));

        let dnd: Box<dyn DndProvider> = Box::new(DummyDnd);
        assert_eq!(dnd.query_dnd_context()?, DndContext::default());

        let notifier: Box<dyn Notifier> = Box::new(DummyNotifier);
        notifier.show_pre_break(BreakType::Short, "Blink", 10)?;
        notifier.dismiss()?;

        let audio: Box<dyn AudioPlayer> = Box::new(DummyAudio);
        audio.play_wav(&[])?;

        let mut overlay: Box<dyn OverlayLauncher> = Box::new(DummyOverlay);
        assert_eq!(overlay.poll_outcome()?, None);
        overlay.close()?;
        Ok(())
    }
}
