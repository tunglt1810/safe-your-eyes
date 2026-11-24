//! Main-thread OS event loop integration for the background daemon.
//!
//! `tray-icon` requires an active `NSApplication` event loop on the main thread on macOS and a
//! Win32 message pump on Windows so the status bar / notification area icon renders and responds
//! to user clicks without creating any GUI window or GPU context.

use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::TrayEvent;

#[cfg(any(target_os = "macos", target_os = "windows"))]
const MAX_EVENTS_PER_PUMP: usize = 64;

/// Platform-specific daemon event loop handle.
pub struct PlatformEventLoop {
    #[cfg(target_os = "macos")]
    inner: Option<macos_impl::MacOsEventLoop>,
    #[cfg(not(target_os = "macos"))]
    _event_tx: Sender<TrayEvent>,
}

impl PlatformEventLoop {
    /// Initializes the OS event loop before creating `TrayHandle`.
    ///
    /// On macOS, initializes `NSApplication` in `Accessory` activation policy, attaches an
    /// `NSApplicationDelegate` forwarding reopen and terminate requests to `event_tx`, and calls
    /// `finishLaunching()` when invoked on the main thread.
    #[must_use]
    pub fn new(event_tx: Sender<TrayEvent>) -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                inner: macos_impl::MacOsEventLoop::try_new(event_tx),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                _event_tx: event_tx,
            }
        }
    }

    /// Pumps pending OS events for up to `timeout` without blocking indefinitely.
    pub fn pump_events(&self, timeout: Duration) {
        #[cfg(target_os = "macos")]
        {
            if let Some(ref inner) = self.inner {
                inner.pump(timeout);
            } else {
                std::thread::sleep(timeout);
            }
        }
        #[cfg(target_os = "windows")]
        {
            let start = std::time::Instant::now();
            windows_impl::drain_messages();
            let remaining = timeout.saturating_sub(start.elapsed());
            if !remaining.is_zero() {
                std::thread::sleep(remaining);
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            std::thread::sleep(timeout);
        }
    }

    /// Completes a deferred OS termination handshake after daemon cleanup finishes.
    ///
    /// On macOS, calls `replyToApplicationShouldTerminate(true)` only if `AppKit` previously
    /// requested termination via `applicationShouldTerminate:`.
    pub fn finish_termination(&self) {
        #[cfg(target_os = "macos")]
        if let Some(ref inner) = self.inner {
            inner.finish_termination();
        }
    }
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "macOS AppKit NSApplicationDelegate definition, initialization, and runloop mode"
)]
mod macos_impl {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::ProtocolObject;
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
        NSApplicationTerminateReply, NSEventMask,
    };
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol};

    use super::MAX_EVENTS_PER_PUMP;
    use crate::TrayEvent;

    struct DelegateIvars {
        event_tx: Sender<TrayEvent>,
        termination_requested: Arc<AtomicBool>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = DelegateIvars]
        #[name = "SyeDaemonAppDelegate"]
        struct DaemonAppDelegate;

        unsafe impl NSObjectProtocol for DaemonAppDelegate {}

        unsafe impl NSApplicationDelegate for DaemonAppDelegate {
            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn should_handle_reopen(
                &self,
                _sender: &NSApplication,
                _has_visible_windows: bool,
            ) -> bool {
                let _ = self.ivars().event_tx.send(TrayEvent::OpenSettings);
                false
            }

            #[unsafe(method(applicationShouldTerminate:))]
            fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
                self.ivars()
                    .termination_requested
                    .store(true, Ordering::Release);
                let _ = self.ivars().event_tx.send(TrayEvent::Quit);
                // Return TerminateLater so daemon.rs completes cleanup before replying true without aborting OS logout/restart.
                NSApplicationTerminateReply::TerminateLater
            }
        }
    );

    impl DaemonAppDelegate {
        fn new(mtm: MainThreadMarker, ivars: DelegateIvars) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ivars);
            // SAFETY: `this` is freshly allocated with initialized ivars; `-[NSObject init]`
            // returns an initialized `DaemonAppDelegate` instance.
            unsafe { msg_send![super(this), init] }
        }
    }

    pub(super) struct MacOsEventLoop {
        app: Retained<NSApplication>,
        _delegate: Retained<DaemonAppDelegate>,
        termination_requested: Arc<AtomicBool>,
    }

    impl MacOsEventLoop {
        pub(super) fn try_new(event_tx: Sender<TrayEvent>) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let termination_requested = Arc::new(AtomicBool::new(false));
            autoreleasepool(|_| {
                let app = NSApplication::sharedApplication(mtm);
                app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                let delegate = DaemonAppDelegate::new(
                    mtm,
                    DelegateIvars {
                        event_tx,
                        termination_requested: Arc::clone(&termination_requested),
                    },
                );
                app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                app.finishLaunching();
                Some(Self {
                    app,
                    _delegate: delegate,
                    termination_requested,
                })
            })
        }

        pub(super) fn pump(&self, timeout: Duration) {
            autoreleasepool(|_| {
                // SAFETY: `NSDefaultRunLoopMode` is an immutable AppKit/Foundation static constant.
                let mode = unsafe { NSDefaultRunLoopMode };
                let until = NSDate::dateWithTimeIntervalSinceNow(timeout.as_secs_f64());
                if let Some(event) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&until),
                    mode,
                    true,
                ) {
                    self.app.sendEvent(&event);
                    for _ in 1..MAX_EVENTS_PER_PUMP {
                        let Some(queued) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(
                            NSEventMask::Any,
                            None,
                            mode,
                            true,
                        ) else {
                            break;
                        };
                        self.app.sendEvent(&queued);
                    }
                    self.app.updateWindows();
                }
            });
        }

        pub(super) fn finish_termination(&self) {
            if self.termination_requested.swap(false, Ordering::AcqRel) {
                autoreleasepool(|_| {
                    self.app.replyToApplicationShouldTerminate(true);
                });
            }
        }
    }

    impl Drop for MacOsEventLoop {
        fn drop(&mut self) {
            // Detach before dropping `_delegate` so AppKit never retains a dangling delegate ref.
            self.app.setDelegate(None);
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::MaybeUninit;
    use std::ptr;

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };

    use super::MAX_EVENTS_PER_PUMP;

    pub(super) fn drain_messages() {
        let mut msg = MaybeUninit::<MSG>::uninit();
        for _ in 0..MAX_EVENTS_PER_PUMP {
            // SAFETY: `msg.as_mut_ptr()` points to valid writable storage for a Win32 `MSG`.
            #[expect(unsafe_code, reason = "Win32 FFI")]
            let has_msg =
                unsafe { PeekMessageW(msg.as_mut_ptr(), ptr::null_mut(), 0, 0, PM_REMOVE) };
            if has_msg == 0 {
                break;
            }
            // SAFETY: `PeekMessageW` returned non-zero, so `msg` is fully initialized.
            #[expect(unsafe_code, reason = "Win32 FFI")]
            let msg = unsafe { msg.assume_init() };
            // SAFETY: `msg` is a valid message retrieved from the current thread's queue.
            #[expect(unsafe_code, reason = "Win32 FFI")]
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn platform_event_loop_pumps_and_finishes_cleanly_on_test_thread() {
        let (tx, _rx) = mpsc::channel();
        let event_loop = PlatformEventLoop::new(tx);
        event_loop.pump_events(Duration::from_millis(5));
        event_loop.finish_termination();
    }
}
