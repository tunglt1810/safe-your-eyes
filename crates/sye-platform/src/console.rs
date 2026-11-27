use std::process::Command;

pub fn detach_console() {
    #[cfg(target_os = "windows")]
    {
        // SAFETY: `FreeConsole` takes no parameters and only detaches this process from its console.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    }
}

pub fn hide_child_console(cmd: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(target_os = "windows")]
fn own_visible_windows() -> Vec<usize> {
    use windows_sys::Win32::Foundation::{LPARAM, TRUE};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    #[expect(unsafe_code, reason = "Win32 FFI callback")]
    unsafe extern "system" fn collect(hwnd: *mut core::ffi::c_void, out: LPARAM) -> i32 {
        let mut pid = 0;
        // SAFETY: `hwnd` comes from `EnumWindows`; `out` is the `Vec` passed below.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == GetCurrentProcessId() && IsWindowVisible(hwnd) != 0 {
                (*(out as *mut Vec<usize>)).push(hwnd as usize);
            }
        }
        TRUE
    }

    let mut own: Vec<usize> = Vec::new();
    // SAFETY: `collect` only runs during the call, with this vector.
    #[expect(unsafe_code, reason = "Win32 FFI")]
    let _ = unsafe { EnumWindows(Some(collect), &mut own as *mut Vec<usize> as LPARAM) };
    own
}

pub fn take_foreground_for_own_window() {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_MOUSE, SendInput};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow,
            GetWindowLongW, GetWindowThreadProcessId, SetForegroundWindow, WS_EX_TOOLWINDOW,
        };

        // SAFETY: Win32 window queries and activation on HWNDs from `own_visible_windows`.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        unsafe {
            let mut fg_pid = 0;
            let fg = GetForegroundWindow();
            if !fg.is_null() {
                GetWindowThreadProcessId(fg, &mut fg_pid);
            }
            if fg_pid == GetCurrentProcessId() {
                return;
            }
            let own = own_visible_windows();
            let Some(&target) = own
                .iter()
                .find(|&&hwnd| {
                    let hwnd = hwnd as *mut core::ffi::c_void;
                    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE).cast_unsigned();
                    GetWindow(hwnd, GW_OWNER).is_null() && ex_style & WS_EX_TOOLWINDOW == 0
                })
                .or_else(|| own.first())
            else {
                return;
            };
            let hwnd = target as *mut core::ffi::c_void;
            // The Start menu and other shell surfaces hold the foreground lock, so a plain
            // `SetForegroundWindow` fails and Explorer keeps the taskbar above the overlay.
            // Windows lets the process that sent the last input event take the foreground. A
            // zero mouse move gives that right without a key press that the overlay or the
            // Start menu could act on.
            let mut input: INPUT = core::mem::zeroed();
            input.r#type = INPUT_MOUSE;
            SendInput(1, &input, core::mem::size_of::<INPUT>() as i32);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn mark_own_windows_fullscreen() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        };
        use windows::Win32::UI::Shell::{ITaskbarList2, TaskbarList};

        let own = own_visible_windows();
        // SAFETY: The hook may run off the thread that initialized COM, so initialize it here; a
        // repeat call is harmless.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            CoCreateInstance::<_, ITaskbarList2>(&TaskbarList, None, CLSCTX_INPROC_SERVER).and_then(
                |taskbar| {
                    taskbar.HrInit()?;
                    for hwnd in &own {
                        taskbar.MarkFullscreenWindow(HWND(*hwnd as *mut _), true)?;
                    }
                    Ok(())
                },
            )
        };
    }
}

pub fn sync_own_windows_appearance(mode: sye_core::config::ThemeMode) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2::rc::autoreleasepool;
        use objc2_app_kit::{
            NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
            NSAppearanceNameDarkAqua, NSApplication,
        };
        use sye_core::config::ThemeMode;

        // `nsh-platform-macos` updates the root content view on `set_color_scheme`, leaving the
        // main `NSWindow` on its launch appearance (which draws a 1px black DarkAqua frame border
        // around a Light Settings window when macOS is in Dark Mode).
        #[expect(unsafe_code, reason = "macOS AppKit NSAppearanceName constants")]
        let apply = |mtm: MainThreadMarker| {
            autoreleasepool(|_| {
                let appearance = match mode {
                    // SAFETY: `NSAppearanceNameAqua` and `NSAppearanceNameDarkAqua` are immutable
                    // AppKit string constants.
                    ThemeMode::Light => {
                        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua })
                    }
                    ThemeMode::Dark => {
                        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua })
                    }
                    ThemeMode::System => None,
                };
                let app = NSApplication::sharedApplication(mtm);
                app.setAppearance(appearance.as_deref());
                for window in app.windows() {
                    window.setAppearance(appearance.as_deref());
                }
            });
        };

        if let Some(mtm) = MainThreadMarker::new() {
            apply(mtm);
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = mode;
}

pub fn disable_native_titlebar_drag_for_own_windows() {
    #[cfg(target_os = "macos")]
    {
        use block2::RcBlock;
        use objc2::rc::autoreleasepool;
        use objc2::runtime::{AnyClass, Imp, Sel};
        use objc2::{ClassType, MainThreadMarker, sel};
        use objc2_app_kit::{NSApplication, NSView, NSWindowStyleMask};
        use objc2_foundation::{NSArray, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize};

        // AppKit treats the `NSTitlebarContainerView` strip of a `FullSizeContentView` window as a
        // system drag region unless a content view claims it via
        // `_opaqueRectForWindowMoveWhenInTitlebar`. `nsh-platform-macos` 0.7.1 does not implement
        // that SPI on `NshView`, so on macOS 27.2+ WindowServer starts a native titlebar drag
        // before `nsh` calls `performWindowDragWithEvent:`, causing AppKit frame re-fits and
        // pointer jumps across stacked displays.
        #[expect(unsafe_code, reason = "macOS Objective-C method implementation ABI")]
        unsafe extern "C-unwind" fn opaque_rect_for_window_move_when_in_titlebar(
            this: &NSView,
            _cmd: Sel,
        ) -> NSRect {
            if let Some(window) = this.window()
                && window
                    .styleMask()
                    .contains(NSWindowStyleMask::FullSizeContentView)
            {
                return this.bounds();
            }
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
        }

        fn apply_on_main(mtm: MainThreadMarker) {
            autoreleasepool(|_| {
                let sel = sel!(_opaqueRectForWindowMoveWhenInTitlebar);
                if let Some(nsh_view_cls) = AnyClass::get(c"NshView")
                    && let Some(ns_view_method) = NSView::class().instance_method(sel)
                {
                    // SAFETY: `ns_view_method` is AppKit's
                    // `-[NSView _opaqueRectForWindowMoveWhenInTitlebar]` returning `NSRect`.
                    // `opaque_rect_for_window_move_when_in_titlebar` matches that C-unwind ABI,
                    // and `class_addMethod` only adds an override if `NshView` lacks its own.
                    #[expect(
                        unsafe_code,
                        reason = "macOS Objective-C runtime class_addMethod for NshView"
                    )]
                    unsafe {
                        let types =
                            objc2::ffi::method_getTypeEncoding(std::ptr::from_ref(ns_view_method));
                        if !types.is_null() {
                            let imp: Imp = std::mem::transmute::<
                                unsafe extern "C-unwind" fn(&NSView, Sel) -> NSRect,
                                Imp,
                            >(
                                opaque_rect_for_window_move_when_in_titlebar
                            );
                            let _ = objc2::ffi::class_addMethod(
                                std::ptr::from_ref(nsh_view_cls).cast_mut(),
                                sel,
                                imp,
                                types,
                            );
                        }
                    }
                }

                let app = NSApplication::sharedApplication(mtm);
                for window in app.windows() {
                    if !window
                        .styleMask()
                        .contains(NSWindowStyleMask::FullSizeContentView)
                    {
                        continue;
                    }
                    if let Some(content) = window.contentView() {
                        content.setNeedsLayout(true);
                        // SAFETY: Main-thread getter; AppKit keeps the frame superview alive while
                        // `content` is attached to the window.
                        #[expect(unsafe_code, reason = "NSView::superview getter")]
                        let frame_view = unsafe { content.superview() };
                        if let Some(frame_view) = frame_view {
                            frame_view.setNeedsLayout(true);
                        }
                    }
                }
            });
        }

        if let Some(mtm) = MainThreadMarker::new() {
            apply_on_main(mtm);
        } else {
            let block = RcBlock::new(|| {
                if let Some(mtm) = MainThreadMarker::new() {
                    apply_on_main(mtm);
                }
            });
            // SAFETY: `NSRunLoopCommonModes` is an immutable Foundation constant and
            // `-[NSRunLoop performInModes:block:]` copies the block onto the main run loop.
            #[expect(unsafe_code, reason = "Foundation main run loop dispatch FFI")]
            unsafe {
                let modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
                NSRunLoop::mainRunLoop().performInModes_block(&modes, &block);
            }
        }
    }
}
