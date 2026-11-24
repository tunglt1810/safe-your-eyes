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

    /// Returns `true` when macOS launched the daemon automatically as a user Login Item.
    #[must_use]
    pub fn was_launched_as_login_item(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.inner
                .as_ref()
                .is_some_and(macos_impl::MacOsEventLoop::was_launched_as_login_item)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Yields foreground activation rights before spawning a GUI child process.
    pub fn prepare_child_activation(&self) {
        #[cfg(target_os = "macos")]
        if let Some(ref inner) = self.inner {
            inner.prepare_child_activation();
        }
    }

    /// Brings an already-running GUI child process (`pid`) to the foreground.
    pub fn activate_child(&self, pid: u32) {
        #[cfg(target_os = "macos")]
        if let Some(ref inner) = self.inner {
            inner.activate_child(pid);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = pid;
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
    use std::time::{Duration, Instant};

    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::ProtocolObject;
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
        NSApplicationDelegate, NSApplicationTerminateReply, NSEventMask, NSRunningApplication,
    };
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSDate, NSDefaultRunLoopMode, NSNotification,
        NSObject, NSObjectProtocol, ns_string,
    };

    use super::MAX_EVENTS_PER_PUMP;
    use crate::TrayEvent;

    const KEY_AE_PROP_DATA: u32 = u32::from_be_bytes(*b"prdt");
    const KEY_AE_LAUNCHED_AS_LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");

    struct DelegateIvars {
        event_tx: Sender<TrayEvent>,
        termination_requested: Arc<AtomicBool>,
        launched_as_login_item: Arc<AtomicBool>,
        did_finish_launching: Arc<AtomicBool>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = DelegateIvars]
        #[name = "SyeDaemonAppDelegate"]
        struct DaemonAppDelegate;

        unsafe impl NSObjectProtocol for DaemonAppDelegate {}

        unsafe impl NSApplicationDelegate for DaemonAppDelegate {
            #[unsafe(method(applicationDidFinishLaunching:))]
            fn did_finish_launching(&self, _notification: &NSNotification) {
                if let Some(event) =
                    NSAppleEventManager::sharedAppleEventManager().currentAppleEvent()
                {
                    // SAFETY: `paramDescriptorForKeyword:` is a documented `NSAppleEventDescriptor`
                    // selector taking an `AEKeyword` (`u32`).
                    let prop: Option<Retained<NSAppleEventDescriptor>> =
                        unsafe { msg_send![&*event, paramDescriptorForKeyword: KEY_AE_PROP_DATA] };
                    if prop.is_some_and(|d| d.enumCodeValue() == KEY_AE_LAUNCHED_AS_LOGIN_ITEM) {
                        self.ivars()
                            .launched_as_login_item
                            .store(true, Ordering::Release);
                    }
                }
                self.ivars()
                    .did_finish_launching
                    .store(true, Ordering::Release);
            }

            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn should_handle_reopen(
                &self,
                _sender: &NSApplication,
                _has_visible_windows: bool,
            ) -> bool {
                let _ = self.ivars().event_tx.send(TrayEvent::Reopen);
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
        launched_as_login_item: Arc<AtomicBool>,
        did_finish_launching: Arc<AtomicBool>,
    }

    impl MacOsEventLoop {
        pub(super) fn try_new(event_tx: Sender<TrayEvent>) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let termination_requested = Arc::new(AtomicBool::new(false));
            let launched_as_login_item = Arc::new(AtomicBool::new(false));
            let did_finish_launching = Arc::new(AtomicBool::new(false));
            let instance = autoreleasepool(|_| {
                let app = NSApplication::sharedApplication(mtm);
                app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                let delegate = DaemonAppDelegate::new(
                    mtm,
                    DelegateIvars {
                        event_tx,
                        termination_requested: Arc::clone(&termination_requested),
                        launched_as_login_item: Arc::clone(&launched_as_login_item),
                        did_finish_launching: Arc::clone(&did_finish_launching),
                    },
                );
                app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                app.finishLaunching();
                Self {
                    app,
                    _delegate: delegate,
                    termination_requested,
                    launched_as_login_item,
                    did_finish_launching,
                }
            });
            // Pump until AppKit dispatches the initial `'aevt'/'oapp'` AppleEvent and fires `applicationDidFinishLaunching:`.
            let start = Instant::now();
            while !instance.did_finish_launching.load(Ordering::Acquire) {
                let remaining = Duration::from_millis(200).saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    break;
                }
                instance.pump(remaining.min(Duration::from_millis(10)));
            }
            Some(instance)
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

        pub(super) fn was_launched_as_login_item(&self) -> bool {
            self.launched_as_login_item.load(Ordering::Acquire)
        }

        pub(super) fn prepare_child_activation(&self) {
            autoreleasepool(|_| {
                self.app
                    .yieldActivationToApplicationWithBundleIdentifier(ns_string!(
                        "io.github.safeyoureyes.SafeYourEyes"
                    ));
            });
        }

        pub(super) fn activate_child(&self, pid: u32) {
            let Ok(pid) = i32::try_from(pid) else {
                return;
            };
            autoreleasepool(|_| {
                if let Some(target) =
                    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                {
                    self.app.yieldActivationToApplication(&target);
                    let _ = target.unhide();
                    #[expect(
                        deprecated,
                        reason = "fallback flag alongside cooperative yield + ActivateAllWindows"
                    )]
                    let opts = NSApplicationActivationOptions::ActivateAllWindows
                        | NSApplicationActivationOptions::ActivateIgnoringOtherApps;
                    let _ = target.activateWithOptions(opts);
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
