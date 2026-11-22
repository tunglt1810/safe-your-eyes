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
    pub fn for_safe_your_eyes(executable: PathBuf) -> Self {
        Self {
            app_id: "io.github.safeyoureyes.SafeYourEyes".to_owned(),
            app_name: "Safe Your Eyes".to_owned(),
            executable,
            args: Vec::new(),
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
    pub fn new(entry_path: PathBuf) -> Self {
        Self { entry_path }
    }

    /// Creates an `AutostartManager` targeting the current OS's standard autostart location.
    pub fn system_default() -> Result<Self, PlatformError> {
        Ok(Self {
            entry_path: default_autostart_path()?,
        })
    }

    /// Returns the managed autostart file path.
    pub fn entry_path(&self) -> &Path {
        &self.entry_path
    }

    /// Returns `true` if the autostart entry currently exists on disk.
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
pub fn render_linux_desktop_entry(entry: &AutostartEntry) -> String {
    let mut exec_parts = Vec::with_capacity(1 + entry.args.len());
    exec_parts.push(quote_desktop_arg(&entry.executable.to_string_lossy()));
    for arg in &entry.args {
        exec_parts.push(quote_desktop_arg(arg));
    }
    let exec_line = exec_parts.join(" ");

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

/// Renders a macOS `launchd` LaunchAgent `.plist` XML file.
pub fn render_macos_launch_agent_plist(entry: &AutostartEntry) -> String {
    let mut program_args = String::new();
    program_args.push_str("        <string>");
    program_args.push_str(&escape_xml(&entry.executable.to_string_lossy()));
    program_args.push_str("</string>\n");
    for arg in &entry.args {
        program_args.push_str("        <string>");
        program_args.push_str(&escape_xml(arg));
        program_args.push_str("</string>\n");
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
             <key>Label</key>\n\
             <string>{label}</string>\n\
             <key>ProgramArguments</key>\n\
             <array>\n\
         {program_args}\
             </array>\n\
             <key>RunAtLoad</key>\n\
             <true/>\n\
         </dict>\n\
         </plist>\n",
        label = escape_xml(&entry.app_id)
    )
}

/// Renders a Windows Startup folder `.cmd` launcher script (no Registry FFI required).
pub fn render_windows_startup_cmd(entry: &AutostartEntry) -> String {
    let exe = entry.executable.to_string_lossy().replace('"', "\"\"");
    let mut cmd = format!("@echo off\r\nstart \"\" \"{exe}\"");
    for arg in &entry.args {
        cmd.push(' ');
        if arg.contains(' ') || arg.contains('"') {
            let escaped = arg.replace('"', "\"\"");
            cmd.push('"');
            cmd.push_str(&escaped);
            cmd.push('"');
        } else {
            cmd.push_str(arg);
        }
    }
    cmd.push_str("\r\n");
    cmd
}

fn quote_desktop_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '$' | '`' | '%'));
    if !needs_quotes {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
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
    out
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
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
    out
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
        assert!(plist.contains("<string>io.github.safeyoureyes.SafeYourEyes</string>"));
        assert!(plist.contains("<string>/Applications/Eye &amp; Care/sye</string>"));
        assert!(plist.contains("<string>--flag=&lt;1&gt;</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
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

        mgr.disable()?;
        assert!(!mgr.is_enabled());
        // Second disable must succeed idempotently.
        mgr.disable()?;
        Ok(())
    }
}
