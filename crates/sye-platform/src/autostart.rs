//! Cross-platform OS autostart entry generation and management.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use sye_core::PlatformError;

/// Descriptor for an application autostart entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartEntry {
    pub app_id: String,
    pub app_name: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
}

impl AutostartEntry {
    /// Creates a default `AutostartEntry` for `safe-your-eyes`.
    #[must_use]
    pub fn for_safe_your_eyes(executable: PathBuf) -> Self {
        Self {
            app_id: "io.github.safeyoureyes.SafeYourEyes".to_owned(),
            app_name: "Safe Your Eyes".to_owned(),
            executable,
            args: vec!["--background".to_owned()],
        }
    }
}

/// Manages enabling, disabling, and querying an OS autostart entry file.
#[derive(Debug, Clone)]
pub struct AutostartManager {
    entry_path: PathBuf,
}

impl AutostartManager {
    /// Creates an `AutostartManager` targeting an explicit file path.
    #[must_use]
    pub const fn new(entry_path: PathBuf) -> Self {
        Self { entry_path }
    }

    /// Creates an `AutostartManager` targeting the current OS's standard autostart location.
    pub fn system_default() -> Result<Self, PlatformError> {
        Ok(Self {
            entry_path: default_autostart_path()?,
        })
    }

    /// Returns the managed autostart file path.
    #[must_use]
    pub fn entry_path(&self) -> &Path {
        &self.entry_path
    }

    /// Returns `true` if the autostart entry currently exists on disk.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.entry_path.is_file()
    }

    /// Writes the platform-appropriate autostart entry file, creating parent directories if needed.
    pub fn enable(&self, entry: &AutostartEntry) -> Result<(), PlatformError> {
        if let Some(parent) = self.entry_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| PlatformError::Io(err.to_string()))?;
        }
        let content = render_for_current_os(entry);
        fs::write(&self.entry_path, content).map_err(|err| PlatformError::Io(err.to_string()))
    }

    /// Removes the autostart entry file if present. Idempotent when already disabled.
    pub fn disable(&self) -> Result<(), PlatformError> {
        match fs::remove_file(&self.entry_path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(PlatformError::Io(err.to_string())),
        }
    }

    /// Synchronizes the autostart file on disk with `enabled`.
    pub fn sync(&self, enabled: bool, entry: &AutostartEntry) -> Result<(), PlatformError> {
        if enabled {
            self.enable(entry)
        } else {
            self.disable()
        }
    }
}

#[cfg(any(target_os = "macos", test))]
const SM_STATUS_NOT_REGISTERED: isize = 0;
#[cfg(any(target_os = "macos", test))]
const SM_STATUS_ENABLED: isize = 1;
#[cfg(any(target_os = "macos", test))]
const SM_STATUS_REQUIRES_APPROVAL: isize = 2;
#[cfg(any(target_os = "macos", test))]
const SM_STATUS_NOT_FOUND: isize = 3;
#[cfg(any(target_os = "macos", test))]
const SM_APP_SERVICE_ERROR_DOMAIN: &str = "SMAppServiceErrorDomain";
#[cfg(any(target_os = "macos", test))]
const SM_ERR_JOB_NOT_FOUND: isize = 6;

fn has_custom_config_override(custom_config: Option<&Path>) -> bool {
    let Some(cfg_path) = custom_config else {
        return false;
    };
    !crate::AppPaths::resolve().is_ok_and(|p| p.config_file == cfg_path)
}

/// Returns `true` when `exe` resides inside a macOS `.app/Contents/MacOS/` bundle directory.
#[must_use]
pub fn is_macos_app_bundle_executable(exe: &Path) -> bool {
    let Some(macos_dir) = exe.parent() else {
        return false;
    };
    if macos_dir.file_name().and_then(|n| n.to_str()) != Some("MacOS") {
        return false;
    }
    let Some(contents_dir) = macos_dir.parent() else {
        return false;
    };
    if contents_dir.file_name().and_then(|n| n.to_str()) != Some("Contents") {
        return false;
    }
    contents_dir
        .parent()
        .and_then(Path::extension)
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

#[cfg(any(target_os = "macos", test))]
fn is_sm_app_service_settled(enabled: bool, status: isize, domain: &str, code: isize) -> bool {
    if enabled {
        matches!(status, SM_STATUS_ENABLED | SM_STATUS_REQUIRES_APPROVAL)
    } else {
        status == SM_STATUS_NOT_REGISTERED
            || (status == SM_STATUS_NOT_FOUND
                && code == SM_ERR_JOB_NOT_FOUND
                && domain == SM_APP_SERVICE_ERROR_DOMAIN)
    }
}

#[cfg(any(target_os = "macos", test))]
const fn interpret_sm_app_service_status(status: isize) -> Option<bool> {
    match status {
        SM_STATUS_ENABLED => Some(true),
        SM_STATUS_NOT_REGISTERED => Some(false),
        // `RequiresApproval` and `NotFound` defer to `config.toml` so startup does not unregister a pending-approval item.
        _ => None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn should_bootout_launchd_job(launchctl_print_stdout: &str) -> bool {
    !launchctl_print_stdout
        .lines()
        .any(|line| line.trim() == "state = running")
}

/// Queries the OS login-item state when managed externally (such as macOS System Settings).
#[must_use]
pub fn query_system_autostart(custom_config: Option<&Path>) -> Option<bool> {
    #[cfg(target_os = "macos")]
    if !has_custom_config_override(custom_config)
        && let Ok(exe) = std::env::current_exe()
    {
        if AutostartManager::system_default().is_ok_and(|m| m.is_enabled()) {
            return None;
        }
        let resolved = exe.canonicalize().unwrap_or(exe);
        if is_macos_app_bundle_executable(&resolved) {
            return macos_login_item::query_status();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = custom_config;
    None
}

/// Synchronizes the current user's OS autostart entry for the running `safe-your-eyes` binary.
pub fn sync_system_autostart(
    enabled: bool,
    custom_config: Option<&Path>,
) -> Result<(), PlatformError> {
    let mgr = AutostartManager::system_default()?;
    let exe = std::env::current_exe().map_err(|err| PlatformError::Io(err.to_string()))?;
    let resolved_exe = exe.canonicalize().unwrap_or(exe);
    let custom_cfg = has_custom_config_override(custom_config);

    #[cfg(target_os = "macos")]
    if !custom_cfg && is_macos_app_bundle_executable(&resolved_exe) {
        return macos_login_item::sync(enabled, mgr.entry_path());
    }

    if !enabled {
        return mgr.disable();
    }
    let mut entry = AutostartEntry::for_safe_your_eyes(resolved_exe);
    if custom_cfg && let Some(cfg_path) = custom_config {
        entry.args.push("--config".to_owned());
        entry.args.push(cfg_path.to_string_lossy().into_owned());
    }
    mgr.enable(&entry)
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "ServiceManagement SMAppService dynamic loading and Objective-C messaging"
)]
mod macos_login_item {
    use std::path::Path;
    use std::process::Command;
    use std::sync::OnceLock;

    use objc2::msg_send;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSError;
    use sye_core::PlatformError;

    pub(super) fn sync(enabled: bool, legacy_plist: &Path) -> Result<(), PlatformError> {
        let res = autoreleasepool(|_| {
            let service = main_app_service()?;
            // SAFETY: `registerAndReturnError:` and `unregisterAndReturnError:` are documented
            // `SMAppService` selectors (macOS 13+) returning `BOOL` and populating `NSError**`.
            let call_res: Result<(), Retained<NSError>> = unsafe {
                if enabled {
                    msg_send![&*service, registerAndReturnError: _]
                } else {
                    msg_send![&*service, unregisterAndReturnError: _]
                }
            };
            // SAFETY: `status` is a documented `SMAppService` property returning `SMAppServiceStatus`.
            let status: isize = unsafe { msg_send![&*service, status] };
            let Err(error) = call_res else {
                return Ok(());
            };
            let domain = error.domain().to_string();
            let code = error.code();
            if super::is_sm_app_service_settled(enabled, status, &domain, code) {
                return Ok(());
            }
            let message = error.localizedDescription().to_string();
            Err(PlatformError::Io(format!("{message} ({domain} {code})")))
        });
        if res.is_ok() || !enabled {
            cleanup_legacy_launch_agent(legacy_plist);
        }
        res
    }

    pub(super) fn query_status() -> Option<bool> {
        autoreleasepool(|_| {
            let service = main_app_service().ok()?;
            // SAFETY: `status` is a documented `SMAppService` property returning `SMAppServiceStatus`.
            let status: isize = unsafe { msg_send![&*service, status] };
            super::interpret_sm_app_service_status(status)
        })
    }

    fn cleanup_legacy_launch_agent(legacy_plist: &Path) {
        if !legacy_plist.exists() {
            return;
        }
        // Delete the plist before `launchctl` runs so `launchd` cannot kill us first.
        let _ = std::fs::remove_file(legacy_plist);
        // SAFETY: `geteuid` is a thread-safe POSIX syscall with no preconditions.
        let uid = unsafe { libc::geteuid() };
        let service_target = format!("gui/{uid}/io.github.safeyoureyes.SafeYourEyes");
        if let Ok(output) = Command::new("/bin/launchctl")
            .arg("print")
            .arg(&service_target)
            .output()
            && output.status.success()
            && super::should_bootout_launchd_job(&String::from_utf8_lossy(&output.stdout))
        {
            let _ = Command::new("/bin/launchctl")
                .arg("bootout")
                .arg(&service_target)
                .output();
        }
    }

    fn main_app_service() -> Result<Retained<AnyObject>, PlatformError> {
        if !load_framework() {
            return Err(PlatformError::Unavailable(
                "ServiceManagement.framework unavailable".to_owned(),
            ));
        }
        let class = AnyClass::get(c"SMAppService").ok_or_else(|| {
            PlatformError::Unavailable("SMAppService class unavailable".to_owned())
        })?;
        // SAFETY: `+[SMAppService mainAppService]` is a documented class property on macOS 13+.
        let service: Option<Retained<AnyObject>> = unsafe { msg_send![class, mainAppService] };
        service.ok_or_else(|| {
            PlatformError::Unavailable("SMAppService.mainAppService returned nil".to_owned())
        })
    }

    fn load_framework() -> bool {
        static LOADED: OnceLock<bool> = OnceLock::new();
        *LOADED.get_or_init(|| {
            let path = c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement";
            // SAFETY: `path` is a valid NUL-terminated system framework path; handle stays loaded.
            !unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) }.is_null()
        })
    }
}

/// Resolves the standard per-user autostart file path for the current OS.
pub fn default_autostart_path() -> Result<PathBuf, PlatformError> {
    let base = BaseDirs::new().ok_or_else(|| {
        PlatformError::Unavailable("unable to determine user base directories".to_owned())
    })?;

    #[cfg(target_os = "macos")]
    {
        Ok(base
            .home_dir()
            .join("Library")
            .join("LaunchAgents")
            .join("io.github.safeyoureyes.SafeYourEyes.plist"))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(base
            .data_dir()
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("safe-your-eyes.cmd"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Ok(base
            .config_dir()
            .join("autostart")
            .join("safe-your-eyes.desktop"))
    }
}

fn render_for_current_os(entry: &AutostartEntry) -> String {
    #[cfg(target_os = "macos")]
    {
        render_macos_launch_agent_plist(entry)
    }
    #[cfg(target_os = "windows")]
    {
        render_windows_startup_cmd(entry)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        render_linux_desktop_entry(entry)
    }
}

/// Renders an XDG autostart `.desktop` file for Linux desktop environments.
#[must_use]
pub fn render_linux_desktop_entry(entry: &AutostartEntry) -> String {
    let exe = entry.executable.to_string_lossy();
    let mut exec_line = String::with_capacity(exe.len() + entry.args.len() * 16);
    push_quoted_desktop_arg(&mut exec_line, &exe);
    for arg in &entry.args {
        exec_line.push(' ');
        push_quoted_desktop_arg(&mut exec_line, arg);
    }

    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={name}\n\
         Comment=Protect your eyes from eye strain\n\
         Icon=safe-your-eyes\n\
         Exec={exec_line}\n\
         Terminal=false\n\
         Categories=Utility;\n\
         X-GNOME-Autostart-enabled=true\n",
        name = entry.app_name
    )
}

/// Renders a macOS `launchd` `LaunchAgent` `.plist` XML file.
#[must_use]
pub fn render_macos_launch_agent_plist(entry: &AutostartEntry) -> String {
    let exe = entry.executable.to_string_lossy();
    let mut out =
        String::with_capacity(512 + entry.app_id.len() * 2 + exe.len() + entry.args.len() * 32);
    out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
             <key>Label</key>\n\
             <string>",
    );
    push_escaped_xml(&mut out, &entry.app_id);
    // Associates fallback LaunchAgent jobs with the .app bundle so macOS System Settings shows the app icon.
    out.push_str(
        "</string>\n\
             <key>AssociatedBundleIdentifiers</key>\n\
             <array>\n        <string>",
    );
    push_escaped_xml(&mut out, &entry.app_id);
    out.push_str(
        "</string>\n    </array>\n\
             <key>ProgramArguments</key>\n\
             <array>\n        <string>",
    );
    push_escaped_xml(&mut out, &exe);
    out.push_str("</string>\n");
    for arg in &entry.args {
        out.push_str("        <string>");
        push_escaped_xml(&mut out, arg);
        out.push_str("</string>\n");
    }
    out.push_str(
        "    </array>\n\
             <key>RunAtLoad</key>\n\
             <true/>\n\
         </dict>\n\
         </plist>\n",
    );
    out
}

/// Renders a Windows Startup folder `.cmd` launcher script (no Registry FFI required).
#[must_use]
pub fn render_windows_startup_cmd(entry: &AutostartEntry) -> String {
    let exe = entry.executable.to_string_lossy();
    let mut cmd = String::with_capacity(32 + exe.len() + entry.args.len() * 16);
    cmd.push_str("@echo off\r\nstart \"\" \"");
    push_cmd_escaped(&mut cmd, &exe);
    cmd.push('"');
    for arg in &entry.args {
        cmd.push(' ');
        if arg.is_empty()
            || arg.chars().any(|c| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '"' | '&' | '|' | '<' | '>' | '(' | ')' | '^' | '!' | '%' | ',' | ';' | '='
                    )
            })
        {
            cmd.push('"');
            push_cmd_escaped(&mut cmd, arg);
            cmd.push('"');
        } else {
            cmd.push_str(arg);
        }
    }
    cmd.push_str("\r\n");
    cmd
}

fn push_cmd_escaped(out: &mut String, input: &str) {
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\"\""),
            '%' => out.push_str("%%"),
            _ => out.push(ch),
        }
    }
}

fn push_quoted_desktop_arg(out: &mut String, arg: &str) {
    let needs_quotes = arg.is_empty()
        || arg.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\''
                        | '\\'
                        | '>'
                        | '<'
                        | '~'
                        | '|'
                        | '&'
                        | ';'
                        | '$'
                        | '*'
                        | '?'
                        | '#'
                        | '('
                        | ')'
                        | '`'
                        | '%'
                )
        });
    if !needs_quotes {
        out.push_str(arg);
        return;
    }
    out.push('"');
    for ch in arg.chars() {
        match ch {
            '"' | '\\' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            '%' => out.push_str("%%"),
            _ => out.push(ch),
        }
    }
    out.push('"');
}

fn push_escaped_xml(out: &mut String, input: &str) {
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestTempDir;

    #[test]
    fn linux_desktop_entry_quotes_paths_with_spaces() {
        let mut entry =
            AutostartEntry::for_safe_your_eyes(PathBuf::from("/opt/My Apps/safe-your-eyes"));
        entry.args = vec![
            "--config".to_owned(),
            "/home/user/my config.toml".to_owned(),
        ];

        let desktop = render_linux_desktop_entry(&entry);
        assert!(desktop.contains("[Desktop Entry]"));
        assert!(desktop.contains("Name=Safe Your Eyes"));
        assert!(desktop.contains("Icon=safe-your-eyes"));
        assert!(desktop.contains("X-GNOME-Autostart-enabled=true"));
        assert!(desktop.contains(
            r#"Exec="/opt/My Apps/safe-your-eyes" --config "/home/user/my config.toml""#
        ));
    }

    #[test]
    fn macos_plist_escapes_xml_special_chars() {
        let mut entry =
            AutostartEntry::for_safe_your_eyes(PathBuf::from("/Applications/Eye & Care/sye"));
        entry.args = vec!["--flag=<1>".to_owned()];

        let plist = render_macos_launch_agent_plist(&entry);
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("<key>AssociatedBundleIdentifiers</key>"));
        assert!(plist.contains("<string>io.github.safeyoureyes.SafeYourEyes</string>"));
        assert!(plist.contains("<string>/Applications/Eye &amp; Care/sye</string>"));
        assert!(plist.contains("<string>--flag=&lt;1&gt;</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
    }

    #[test]
    fn macos_app_bundle_executable_detection_and_sm_status_mapping() {
        assert!(is_macos_app_bundle_executable(Path::new(
            "/Applications/Safe Your Eyes.app/Contents/MacOS/safe-your-eyes"
        )));
        assert!(!is_macos_app_bundle_executable(Path::new(
            "/usr/local/bin/safe-your-eyes"
        )));
        assert!(!is_macos_app_bundle_executable(Path::new(
            "/Users/me/target/release/safe-your-eyes"
        )));

        assert_eq!(
            interpret_sm_app_service_status(SM_STATUS_ENABLED),
            Some(true)
        );
        assert_eq!(
            interpret_sm_app_service_status(SM_STATUS_NOT_REGISTERED),
            Some(false)
        );
        assert_eq!(
            interpret_sm_app_service_status(SM_STATUS_REQUIRES_APPROVAL),
            None
        );
        assert_eq!(interpret_sm_app_service_status(SM_STATUS_NOT_FOUND), None);

        assert!(is_sm_app_service_settled(
            true,
            SM_STATUS_ENABLED,
            SM_APP_SERVICE_ERROR_DOMAIN,
            12
        ));
        assert!(is_sm_app_service_settled(
            true,
            SM_STATUS_REQUIRES_APPROVAL,
            SM_APP_SERVICE_ERROR_DOMAIN,
            1
        ));
        assert!(is_sm_app_service_settled(
            false,
            SM_STATUS_NOT_FOUND,
            SM_APP_SERVICE_ERROR_DOMAIN,
            SM_ERR_JOB_NOT_FOUND
        ));
        assert!(!is_sm_app_service_settled(
            false,
            SM_STATUS_NOT_FOUND,
            SM_APP_SERVICE_ERROR_DOMAIN,
            2
        ));
    }

    #[test]
    fn should_bootout_launchd_job_skips_live_running_daemon() {
        let running = "gui/501/io.github.safeyoureyes.SafeYourEyes = {\n\tactive count = 1\n\tstate = running\n\tpid = 4242\n}\n";
        assert!(!should_bootout_launchd_job(running));

        let waiting = "gui/501/io.github.safeyoureyes.SafeYourEyes = {\n\tactive count = 0\n\tstate = waiting\n\tlast exit code = 0\n}\n";
        assert!(should_bootout_launchd_job(waiting));

        let not_running =
            "gui/501/io.github.safeyoureyes.SafeYourEyes = {\n\tstate = not running\n}\n";
        assert!(should_bootout_launchd_job(not_running));
        assert!(should_bootout_launchd_job(""));
    }

    #[test]
    fn windows_startup_cmd_quotes_executable_and_args() {
        let mut entry = AutostartEntry::for_safe_your_eyes(PathBuf::from(
            r"C:\Program Files\Safe Your Eyes\safe-your-eyes.exe",
        ));
        entry.args = vec![
            "--config".to_owned(),
            r"C:\Users\Me\my config.toml".to_owned(),
        ];

        let cmd = render_windows_startup_cmd(&entry);
        assert!(cmd.starts_with("@echo off\r\n"));
        assert!(cmd.contains(
            r#"start "" "C:\Program Files\Safe Your Eyes\safe-your-eyes.exe" --config "C:\Users\Me\my config.toml""#
        ));
    }

    #[test]
    fn default_entry_includes_background_flag_across_platforms() {
        let entry = AutostartEntry::for_safe_your_eyes(PathBuf::from("/usr/bin/safe-your-eyes"));
        assert_eq!(entry.args, vec!["--background"]);

        let desktop = render_linux_desktop_entry(&entry);
        assert!(desktop.contains("Exec=/usr/bin/safe-your-eyes --background"));

        let plist = render_macos_launch_agent_plist(&entry);
        assert!(plist.contains("<string>--background</string>"));

        let cmd = render_windows_startup_cmd(&entry);
        assert!(cmd.contains(r#"start "" "/usr/bin/safe-your-eyes" --background"#));
    }

    #[test]
    fn manager_enable_disable_round_trip_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("autostart")?;
        let entry_path = tmp.path().join("autostart").join("safe-your-eyes.desktop");
        let mgr = AutostartManager::new(entry_path.clone());
        let entry = AutostartEntry::for_safe_your_eyes(PathBuf::from("/usr/bin/safe-your-eyes"));

        assert!(!mgr.is_enabled());
        mgr.enable(&entry)?;
        assert!(mgr.is_enabled());
        let content = fs::read_to_string(&entry_path)?;
        assert!(content.contains("/usr/bin/safe-your-eyes"));
        assert!(content.contains("--background"));

        mgr.disable()?;
        assert!(!mgr.is_enabled());
        // Second disable must succeed idempotently.
        mgr.disable()?;

        mgr.sync(true, &entry)?;
        assert!(mgr.is_enabled());
        mgr.sync(false, &entry)?;
        assert!(!mgr.is_enabled());
        Ok(())
    }

    #[test]
    fn linux_desktop_entry_quotes_xdg_reserved_chars_without_spaces() {
        let mut entry =
            AutostartEntry::for_safe_your_eyes(PathBuf::from("/usr/bin/safe-your-eyes"));
        entry.args = vec![
            "--background".to_owned(),
            "--config".to_owned(),
            "/home/o'brien&co/config.toml".to_owned(),
        ];

        let desktop = render_linux_desktop_entry(&entry);
        assert!(desktop.contains(
            r#"Exec=/usr/bin/safe-your-eyes --background --config "/home/o'brien&co/config.toml""#
        ));
    }

    #[test]
    fn windows_startup_cmd_quotes_metacharacters_and_escapes_percent() {
        let mut entry =
            AutostartEntry::for_safe_your_eyes(PathBuf::from(r"C:\Users\100%User\sye.exe"));
        entry.args = vec![
            "--background".to_owned(),
            "--config".to_owned(),
            r"C:\Users\A&B\100%cfg.toml".to_owned(),
        ];

        let cmd = render_windows_startup_cmd(&entry);
        assert!(cmd.contains(
            r#"start "" "C:\Users\100%%User\sye.exe" --background --config "C:\Users\A&B\100%%cfg.toml""#
        ));
    }
}
