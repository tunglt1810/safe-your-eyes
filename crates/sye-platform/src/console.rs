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

/// Tells the taskbar that this process's visible top-level windows are full screen, so it stays
/// below them.
pub fn mark_own_windows_fullscreen() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        };
        use windows::Win32::UI::Shell::{ITaskbarList2, TaskbarList};
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
        // SAFETY: `collect` only runs during the call, with this vector. The hook may run off the
        // thread that initialized COM, so initialize it here; a repeat call is harmless.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe {
            EnumWindows(Some(collect), &mut own as *mut Vec<usize> as LPARAM);
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
