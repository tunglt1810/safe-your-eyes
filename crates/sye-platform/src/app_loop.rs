use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::TrayEvent;

#[cfg(any(target_os = "macos", target_os = "windows"))]
const MAX_EVENTS_PER_PUMP: usize = 64;

pub struct PlatformEventLoop {
    #[cfg(target_os = "macos")]
    inner: Option<macos_impl::MacOsEventLoop>,
    #[cfg(not(target_os = "macos"))]
    _event_tx: Sender<TrayEvent>,
}

impl PlatformEventLoop {
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
            windows_impl::pump(timeout);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            std::thread::sleep(timeout);
        }
    }

    pub fn finish_termination(&self) {
        #[cfg(target_os = "macos")]
        if let Some(ref inner) = self.inner {
            inner.finish_termination();
        }
    }

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

    pub fn prepare_child_activation(&self) {
        #[cfg(target_os = "macos")]
        if let Some(ref inner) = self.inner {
            inner.prepare_child_activation();
        }
        #[cfg(target_os = "windows")]
        windows_impl::prepare_child_activation();
    }

    pub fn activate_child(&self, pid: u32) {
        #[cfg(target_os = "macos")]
        if let Some(ref inner) = self.inner {
            inner.activate_child(pid);
        }
        #[cfg(target_os = "windows")]
        windows_impl::activate_child(pid);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
            self.app.setDelegate(None);
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildWindowMatch {
    Ignore,
    Fallback,
    Primary,
}

#[cfg(any(target_os = "windows", test))]
const fn classify_child_window(
    window_pid: u32,
    target_pid: u32,
    is_visible: bool,
    has_owner: bool,
    is_tool_window: bool,
) -> ChildWindowMatch {
    if window_pid != target_pid || !is_visible {
        ChildWindowMatch::Ignore
    } else if !has_owner && !is_tool_window {
        ChildWindowMatch::Primary
    } else {
        ChildWindowMatch::Fallback
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::MaybeUninit;
    use std::ptr;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{FALSE, HWND, LPARAM, TRUE, WAIT_FAILED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, BringWindowToTop, DispatchMessageW, EnumWindows,
        GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow, GetWindowLongW,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsHungAppWindow, IsIconic,
        IsWindowVisible, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
        SW_RESTORE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos,
        ShowWindow, TranslateMessage, WS_EX_TOOLWINDOW,
    };

    use super::{ChildWindowMatch, MAX_EVENTS_PER_PUMP, classify_child_window};

    struct FindWindowState {
        target_pid: u32,
        primary: HWND,
        fallback: HWND,
    }

    #[expect(unsafe_code, reason = "Win32 FFI callback")]
    unsafe extern "system" fn find_child_window_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
        let mut window_pid = 0;
        // SAFETY: `hwnd` comes from `EnumWindows`; `lparam` points to `FindWindowState` on the caller stack.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut window_pid);
            let state = &mut *(lparam as *mut FindWindowState);
            let is_visible = IsWindowVisible(hwnd) != 0;
            let has_owner = !GetWindow(hwnd, GW_OWNER).is_null();
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE).cast_unsigned();
            let is_tool = (ex_style & WS_EX_TOOLWINDOW) != 0;
            match classify_child_window(
                window_pid,
                state.target_pid,
                is_visible,
                has_owner,
                is_tool,
            ) {
                ChildWindowMatch::Ignore => TRUE,
                ChildWindowMatch::Primary => {
                    state.primary = hwnd;
                    FALSE
                }
                ChildWindowMatch::Fallback => {
                    if state.fallback.is_null() {
                        state.fallback = hwnd;
                    }
                    TRUE
                }
            }
        }
    }

    fn find_child_window(target_pid: u32) -> HWND {
        let mut state = FindWindowState {
            target_pid,
            primary: ptr::null_mut(),
            fallback: ptr::null_mut(),
        };
        // SAFETY: `find_child_window_proc` only runs during `EnumWindows` with `&mut state`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe {
            EnumWindows(
                Some(find_child_window_proc),
                ptr::from_mut(&mut state) as LPARAM,
            )
        };
        if state.primary.is_null() {
            state.fallback
        } else {
            state.primary
        }
    }

    fn drain_messages() {
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

    pub(super) fn pump(timeout: Duration) {
        let start = Instant::now();
        loop {
            drain_messages();
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            let wait_ms = u32::try_from(remaining.as_millis())
                .unwrap_or(u32::MAX)
                .max(1);
            // SAFETY: waiting on zero kernel handles with `QS_ALLINPUT` blocks only on thread messages.
            #[expect(unsafe_code, reason = "Win32 FFI")]
            let status =
                unsafe { MsgWaitForMultipleObjects(0, ptr::null(), FALSE, wait_ms, QS_ALLINPUT) };
            if status != WAIT_OBJECT_0 {
                // Keeps the daemon loop from spinning when the wait itself fails.
                if status == WAIT_FAILED {
                    std::thread::sleep(remaining);
                }
                break;
            }
        }
    }

    pub(super) fn prepare_child_activation() {
        // SAFETY: `AllowSetForegroundWindow` accepts `ASFW_ANY` to yield foreground activation rights.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    }

    pub(super) fn activate_child(pid: u32) {
        // SAFETY: Win32 foreground/window activation APIs operate on `pid` and the HWND found in this call.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        unsafe {
            let hwnd = find_child_window(pid);
            if hwnd.is_null() {
                return;
            }
            if IsIconic(hwnd) != 0 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = BringWindowToTop(hwnd);
            if SetForegroundWindow(hwnd) != 0 {
                return;
            }

            let fg = GetForegroundWindow();
            // A hung foreground thread would share its stalled input queue with the daemon loop.
            let fg_tid = if fg.is_null() || IsHungAppWindow(fg) != 0 {
                0
            } else {
                GetWindowThreadProcessId(fg, ptr::null_mut())
            };
            let cur_tid = GetCurrentThreadId();
            let target_tid = GetWindowThreadProcessId(hwnd, ptr::null_mut());

            let attached_fg =
                fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, TRUE) != 0;
            let attached_target = target_tid != 0
                && target_tid != cur_tid
                && target_tid != fg_tid
                && AttachThreadInput(cur_tid, target_tid, TRUE) != 0;

            let _ = BringWindowToTop(hwnd);
            let focused = SetForegroundWindow(hwnd) != 0;
            if !focused {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
                let _ = SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
                let _ = BringWindowToTop(hwnd);
                let _ = SetForegroundWindow(hwnd);
            }

            if attached_target {
                let _ = AttachThreadInput(cur_tid, target_tid, FALSE);
            }
            if attached_fg {
                let _ = AttachThreadInput(cur_tid, fg_tid, FALSE);
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

    #[test]
    fn classify_child_window_prefers_visible_unowned_non_tool_window() {
        assert_eq!(
            classify_child_window(42, 42, true, false, false),
            ChildWindowMatch::Primary
        );
        assert_eq!(
            classify_child_window(42, 42, true, true, false),
            ChildWindowMatch::Fallback
        );
        assert_eq!(
            classify_child_window(42, 42, true, false, true),
            ChildWindowMatch::Fallback
        );
        assert_eq!(
            classify_child_window(42, 42, false, false, false),
            ChildWindowMatch::Ignore
        );
        assert_eq!(
            classify_child_window(99, 42, true, false, false),
            ChildWindowMatch::Ignore
        );
    }
}
