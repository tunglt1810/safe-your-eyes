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

/// Tells the taskbar that this process's foreground window is full screen, so it stays below it.
pub fn mark_foreground_fullscreen() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
        use windows::Win32::UI::Shell::{ITaskbarList2, TaskbarList};
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        // SAFETY: plain Win32 calls; COM is initialized on the nsh UI thread that runs this hook,
        // and the window handle is checked to belong to this process before use.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if hwnd.is_null() || pid != GetCurrentProcessId() {
                return;
            }
            CoCreateInstance::<_, ITaskbarList2>(&TaskbarList, None, CLSCTX_INPROC_SERVER).and_then(
                |taskbar| {
                    taskbar.HrInit()?;
                    taskbar.MarkFullscreenWindow(windows::Win32::Foundation::HWND(hwnd), true)
                },
            )
        };
    }
}
