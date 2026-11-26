use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sye_core::ipc::MAX_IPC_LINE_BYTES;
use sye_core::{OverlayLauncher, OverlayOutcome, OverlayRequest, PlatformError};

pub const OVERLAY_SHOWN_SIGNAL: &str = r#"{"kind":"shown"}"#;
const OUTCOME_DRAIN_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy)]
enum OverlayPipeEvent {
    Shown,
    Outcome(OverlayOutcome),
}

#[derive(Debug)]
struct RunningOverlay {
    child: Child,
    events: mpsc::Receiver<OverlayPipeEvent>,
    shown_reported: bool,
    shown_pending: bool,
    cached_outcome: Option<OverlayOutcome>,
}

impl RunningOverlay {
    fn handle_event(&mut self, event: OverlayPipeEvent) {
        match event {
            OverlayPipeEvent::Shown => {
                if !self.shown_reported {
                    self.shown_reported = true;
                    self.shown_pending = true;
                }
            }
            OverlayPipeEvent::Outcome(outcome) => {
                if self.cached_outcome.is_none() {
                    self.cached_outcome = Some(outcome);
                }
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.handle_event(event);
        }
    }

    fn drain_outcome_bounded(&mut self, timeout: Duration) {
        self.drain_events();
        if self.cached_outcome.is_some() {
            return;
        }
        let deadline = Instant::now() + timeout;
        while self.cached_outcome.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.events.recv_timeout(remaining) {
                Ok(event) => self.handle_event(event),
                Err(mpsc::RecvTimeoutError::Disconnected | mpsc::RecvTimeoutError::Timeout) => {
                    break;
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct SubprocessOverlayLauncher {
    program: PathBuf,
    base_args: Vec<String>,
    running: Option<RunningOverlay>,
}

impl SubprocessOverlayLauncher {
    pub fn current_exe() -> Result<Self, PlatformError> {
        let exe = std::env::current_exe().map_err(|err| PlatformError::Io(err.to_string()))?;
        let resolved = exe.canonicalize().unwrap_or(exe);
        Ok(Self::with_command(resolved, Vec::new()))
    }

    #[must_use]
    pub const fn with_command(program: PathBuf, base_args: Vec<String>) -> Self {
        Self {
            program,
            base_args,
            running: None,
        }
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn poll_shown(&mut self) -> bool {
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        running.drain_events();
        std::mem::take(&mut running.shown_pending)
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

        let mut child = crate::console::hide_child_console(&mut cmd)
            .spawn()
            .map_err(|err| PlatformError::Unavailable(err.to_string()))?;
        let (tx, rx) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            let _ = thread::Builder::new()
                .name("sye-overlay-stdout".to_owned())
                .spawn(move || read_overlay_stdout(stdout, &tx));
        }
        self.running = Some(RunningOverlay {
            child,
            events: rx,
            shown_reported: false,
            shown_pending: false,
            cached_outcome: None,
        });
        Ok(())
    }

    fn poll_outcome(&mut self) -> Result<Option<OverlayOutcome>, PlatformError> {
        let Some(running) = self.running.as_mut() else {
            return Ok(None);
        };

        let status = running
            .child
            .try_wait()
            .map_err(|err| PlatformError::Io(err.to_string()))?;
        if status.is_none() {
            running.drain_events();
            return Ok(None);
        }

        let Some(mut finished) = self.running.take() else {
            return Ok(None);
        };
        finished.drain_outcome_bounded(OUTCOME_DRAIN_TIMEOUT);

        Ok(Some(
            finished.cached_outcome.unwrap_or(OverlayOutcome::Closed),
        ))
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
        Ok(())
    }
}

impl Drop for SubprocessOverlayLauncher {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn read_overlay_stdout<R: Read>(reader: R, tx: &mpsc::Sender<OverlayPipeEvent>) {
    let line_cap = u64::try_from(MAX_IPC_LINE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut buf_reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(256);
    let mut sent_shown = false;
    let mut discarding = false;

    loop {
        buf.clear();
        let Ok(read) = (&mut buf_reader).take(line_cap).read_until(b'\n', &mut buf) else {
            break;
        };
        if read == 0 {
            break;
        }
        if discarding || buf.len() > MAX_IPC_LINE_BYTES {
            discarding = buf.last() != Some(&b'\n');
            continue;
        }
        let text = String::from_utf8_lossy(&buf);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == OVERLAY_SHOWN_SIGNAL {
            if !sent_shown {
                sent_shown = true;
                let _ = tx.send(OverlayPipeEvent::Shown);
            }
            continue;
        }
        if let Some(outcome) = parse_overlay_outcome_line(trimmed.as_bytes()) {
            let _ = tx.send(OverlayPipeEvent::Outcome(outcome));
            break;
        }
    }
}

fn parse_overlay_outcome_line(raw: &[u8]) -> Option<OverlayOutcome> {
    if raw.is_empty() || raw.len() > MAX_IPC_LINE_BYTES {
        return None;
    }
    let text = String::from_utf8_lossy(raw);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<OverlayOutcome>(trimmed).ok()
}

#[cfg(test)]
pub(crate) fn parse_overlay_outcome(raw: &[u8]) -> OverlayOutcome {
    parse_overlay_outcome_line(raw).unwrap_or(OverlayOutcome::Closed)
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

    #[cfg(unix)]
    #[test]
    fn subprocess_launcher_reports_shown_before_outcome() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut launcher = SubprocessOverlayLauncher::with_command(
            PathBuf::from("/bin/sh"),
            vec![
                "-c".to_owned(),
                r#"printf '{"kind":"shown"}\n'; sleep 0.15; printf '{"kind":"completed"}\n'"#
                    .to_owned(),
                "--".to_owned(),
            ],
        );

        launcher.launch(&sample_request())?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_shown = false;
        while Instant::now() < deadline {
            if launcher.poll_shown() {
                saw_shown = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(saw_shown, "expected shown signal before subprocess exited");
        assert!(!launcher.poll_shown(), "shown signal must fire only once");
        assert_eq!(launcher.poll_outcome()?, None);

        let outcome = wait_for_outcome(&mut launcher, Duration::from_secs(3))?;
        assert_eq!(outcome, OverlayOutcome::Completed);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_launcher_handles_shown_and_outcome_back_to_back()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut launcher = SubprocessOverlayLauncher::with_command(
            PathBuf::from("/bin/sh"),
            vec![
                "-c".to_owned(),
                r#"printf '{"kind":"shown"}\n{"kind":"skipped"}\n'"#.to_owned(),
                "--".to_owned(),
            ],
        );

        launcher.launch(&sample_request())?;
        let outcome = wait_for_outcome(&mut launcher, Duration::from_secs(3))?;
        assert_eq!(outcome, OverlayOutcome::Skipped);
        Ok(())
    }

    #[test]
    fn read_overlay_stdout_handles_crlf_stray_lines_and_duplicate_shown() {
        let input = concat!(
            "debug: initializing d2d\r\n",
            "{\"kind\":\"shown\"}\r\n",
            "{\"kind\":\"shown\"}\r\n",
            "warning: frame drop\r\n",
            "{\"kind\":\"completed\"}\r\n",
            "{\"kind\":\"skipped\"}\r\n",
        );
        let (tx, rx) = mpsc::channel();
        read_overlay_stdout(input.as_bytes(), &tx);
        drop(tx);

        let events: Vec<OverlayPipeEvent> = rx.into_iter().collect();
        assert_eq!(events.len(), 2);
        assert!(matches!(events.first(), Some(OverlayPipeEvent::Shown)));
        assert!(matches!(
            events.get(1),
            Some(OverlayPipeEvent::Outcome(OverlayOutcome::Completed))
        ));
    }

    #[test]
    fn read_overlay_stdout_keeps_reading_after_many_stray_lines() {
        let mut input = Vec::new();
        while input.len() <= MAX_IPC_LINE_BYTES * 4 {
            input.extend_from_slice(b"debug: frame rendered\n");
        }
        input.extend_from_slice(b"{\"kind\":\"completed\"}\n");

        let (tx, rx) = mpsc::channel();
        read_overlay_stdout(input.as_slice(), &tx);
        drop(tx);

        let events: Vec<OverlayPipeEvent> = rx.into_iter().collect();
        assert!(matches!(
            events.as_slice(),
            [OverlayPipeEvent::Outcome(OverlayOutcome::Completed)]
        ));
    }

    #[test]
    fn read_overlay_stdout_ignores_oversized_and_garbage_lines() {
        let mut input = vec![b'x'; MAX_IPC_LINE_BYTES + 8];
        input.push(b'\n');
        input.extend_from_slice(b"{\"kind\":\"shown\"}\n");
        input.extend_from_slice(b"not-valid-json\n");

        let (tx, rx) = mpsc::channel();
        read_overlay_stdout(input.as_slice(), &tx);
        drop(tx);

        let events: Vec<OverlayPipeEvent> = rx.into_iter().collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(events.first(), Some(OverlayPipeEvent::Shown)));
    }
}
