//! On-demand subprocess implementation of `sye_core::OverlayLauncher`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use sye_core::ipc::MAX_IPC_LINE_BYTES;
use sye_core::{OverlayLauncher, OverlayOutcome, OverlayRequest, PlatformError};

/// Spawns an isolated overlay subprocess and reads its `OverlayOutcome` JSON from stdout.
#[derive(Debug)]
pub struct SubprocessOverlayLauncher {
    program: PathBuf,
    base_args: Vec<String>,
    child: Option<Child>,
}

impl SubprocessOverlayLauncher {
    /// Creates a launcher that invokes the current executable with `--internal-overlay <JSON>`.
    pub fn current_exe() -> Result<Self, PlatformError> {
        let exe = std::env::current_exe().map_err(|err| PlatformError::Io(err.to_string()))?;
        // Resolve Homebrew Cask symlinks so macOS launches the `.app` bundle executable directly.
        let resolved = exe.canonicalize().unwrap_or(exe);
        Ok(Self::with_command(resolved, Vec::new()))
    }

    /// Creates a launcher with an explicit program and leading arguments (useful for testing).
    #[must_use]
    pub const fn with_command(program: PathBuf, base_args: Vec<String>) -> Self {
        Self {
            program,
            base_args,
            child: None,
        }
    }

    /// Returns `true` if an overlay subprocess is currently tracked.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.child.is_some()
    }
}

impl OverlayLauncher for SubprocessOverlayLauncher {
    fn launch(&mut self, request: &OverlayRequest) -> Result<(), PlatformError> {
        self.close()?;

        let payload =
            serde_json::to_string(request).map_err(|err| PlatformError::Io(err.to_string()))?;

        let mut cmd = Command::new(&self.program);
        cmd.args(&self.base_args)
            .arg("--internal-overlay")
            .arg(&payload)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|err| PlatformError::Unavailable(err.to_string()))?;
        self.child = Some(child);
        Ok(())
    }

    fn poll_outcome(&mut self) -> Result<Option<OverlayOutcome>, PlatformError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };

        let status = child
            .try_wait()
            .map_err(|err| PlatformError::Io(err.to_string()))?;
        if status.is_none() {
            return Ok(None);
        }

        let Some(mut finished) = self.child.take() else {
            return Ok(None);
        };

        let mut buf = Vec::new();
        if let Some(stdout) = finished.stdout.take() {
            let max_read = u64::try_from(MAX_IPC_LINE_BYTES)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            let mut limited = stdout.take(max_read);
            let _ = limited.read_to_end(&mut buf);
        }

        Ok(Some(parse_overlay_outcome(&buf)))
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

impl Drop for SubprocessOverlayLauncher {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Parses bounded stdout bytes from the overlay subprocess into an `OverlayOutcome`.
///
/// Empty, oversized, or malformed output degrades safely to `OverlayOutcome::Closed`
/// so the scheduler never stalls when an overlay process is killed externally.
pub(crate) fn parse_overlay_outcome(raw: &[u8]) -> OverlayOutcome {
    if raw.is_empty() || raw.len() > MAX_IPC_LINE_BYTES {
        return OverlayOutcome::Closed;
    }
    let text = String::from_utf8_lossy(raw);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return OverlayOutcome::Closed;
    }
    serde_json::from_str::<OverlayOutcome>(trimmed).unwrap_or(OverlayOutcome::Closed)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::thread;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use sye_core::BreakType;

    use super::*;

    #[cfg(unix)]
    fn sample_request() -> OverlayRequest {
        OverlayRequest {
            break_type: BreakType::Short,
            title: "Look 20 feet away".to_owned(),
            duration_secs: 15,
            strict_break: false,
            allow_postpone: true,
            postpone_duration_secs: 300,
            shortcut_disable_secs: 2,
            fade_in_ms: 1500,
            status_note: None,
        }
    }

    #[cfg(unix)]
    fn wait_for_outcome(
        launcher: &mut SubprocessOverlayLauncher,
        timeout: Duration,
    ) -> Result<OverlayOutcome, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(outcome) = launcher.poll_outcome()? {
                return Ok(outcome);
            }
            if Instant::now() >= deadline {
                return Err("timed out waiting for overlay subprocess".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn parse_overlay_outcome_handles_valid_and_malformed_inputs() {
        assert_eq!(
            parse_overlay_outcome(br#"{"kind":"completed"}"#),
            OverlayOutcome::Completed
        );
        assert_eq!(
            parse_overlay_outcome(br#"{"kind":"skipped"}"#),
            OverlayOutcome::Skipped
        );
        assert_eq!(
            parse_overlay_outcome(br#"{"kind":"postponed","duration_secs":300}"#),
            OverlayOutcome::Postponed { duration_secs: 300 }
        );
        assert_eq!(parse_overlay_outcome(b""), OverlayOutcome::Closed);
        assert_eq!(parse_overlay_outcome(b"not-json"), OverlayOutcome::Closed);
        let oversized = vec![b'a'; MAX_IPC_LINE_BYTES + 1];
        assert_eq!(parse_overlay_outcome(&oversized), OverlayOutcome::Closed);
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_launcher_reads_valid_json_outcome() -> Result<(), Box<dyn std::error::Error>> {
        let mut launcher = SubprocessOverlayLauncher::with_command(
            PathBuf::from("/bin/sh"),
            vec![
                "-c".to_owned(),
                r#"printf '{"kind":"postponed","duration_secs":180}\n'"#.to_owned(),
                "--".to_owned(),
            ],
        );

        launcher.launch(&sample_request())?;
        let outcome = wait_for_outcome(&mut launcher, Duration::from_secs(3))?;
        assert_eq!(outcome, OverlayOutcome::Postponed { duration_secs: 180 });
        assert!(!launcher.is_running());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_launcher_falls_back_to_closed_on_invalid_stdout()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut launcher = SubprocessOverlayLauncher::with_command(
            PathBuf::from("/bin/sh"),
            vec!["-c".to_owned(), "echo garbage".to_owned(), "--".to_owned()],
        );

        launcher.launch(&sample_request())?;
        let outcome = wait_for_outcome(&mut launcher, Duration::from_secs(3))?;
        assert_eq!(outcome, OverlayOutcome::Closed);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_launcher_close_terminates_long_running_child()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut launcher = SubprocessOverlayLauncher::with_command(
            PathBuf::from("/bin/sh"),
            vec!["-c".to_owned(), "sleep 30".to_owned(), "--".to_owned()],
        );

        launcher.launch(&sample_request())?;
        assert_eq!(launcher.poll_outcome()?, None);
        assert!(launcher.is_running());

        launcher.close()?;
        assert!(!launcher.is_running());
        assert_eq!(launcher.poll_outcome()?, None);
        Ok(())
    }
}
