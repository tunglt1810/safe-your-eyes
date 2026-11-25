//! Windows console handling: `safe-your-eyes.exe` is a console-subsystem binary so that CLI output
//! works in a terminal, but the daemon and its GUI subprocesses must not leave a console window open.

use std::process::Command;

/// Releases the console that Windows attached to this process. No-op on other platforms.
pub fn detach_console() {
    #[cfg(target_os = "windows")]
    {
        // SAFETY: `FreeConsole` takes no parameters and only detaches this process from its console.
        #[expect(unsafe_code, reason = "Win32 FFI")]
        let _ = unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    }
}

/// Stops a child process from allocating a new console window. No-op on other platforms.
pub fn hide_child_console(cmd: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
