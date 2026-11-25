//! Cross-platform non-blocking WAV chime playback (`sye_core::AudioPlayer`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use sye_core::{AudioPlayer, ChimeKind, PlatformError, synthesize_chime_wav};

static AUDIO_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
struct PlayerSpec {
    program: &'static str,
    args: &'static [&'static str],
}

/// Non-blocking OS audio player for synthesized WAV chimes.
///
/// Writes WAV bytes to a unique temporary file, spawns a platform player (`pw-play`/`paplay`/`aplay`
/// on Linux, `afplay` on macOS, PowerShell `Media.SoundPlayer` on Windows), and reaps the child +
/// deletes the temporary file on a background thread.
#[derive(Debug, Clone)]
pub struct OsAudioPlayer {
    temp_dir: PathBuf,
    players: &'static [PlayerSpec],
}

impl OsAudioPlayer {
    /// Creates an `OsAudioPlayer` using the OS runtime/temp directory and default platform players.
    #[must_use]
    pub fn new() -> Self {
        Self::with_temp_dir(default_audio_temp_dir())
    }

    /// Creates an `OsAudioPlayer` writing temporary WAV files under `temp_dir`.
    #[must_use]
    pub const fn with_temp_dir(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            players: DEFAULT_PLAYER_SPECS,
        }
    }

    #[cfg(test)]
    const fn with_players(temp_dir: PathBuf, players: &'static [PlayerSpec]) -> Self {
        Self { temp_dir, players }
    }

    /// Synthesizes and plays a pre-break or post-break chime at `volume` (clamped to `0..=100`).
    ///
    /// A volume of `0` returns `Ok(())` without writing a file or spawning a process.
    pub fn play_chime(&self, kind: ChimeKind, volume: u8) -> Result<(), PlatformError> {
        let clamped = volume.min(100);
        if clamped == 0 {
            return Ok(());
        }
        let wav = synthesize_chime_wav(kind, clamped);
        self.play_wav(&wav)
    }
}

impl Default for OsAudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlayer for OsAudioPlayer {
    fn play_wav(&self, wav_bytes: &[u8]) -> Result<(), PlatformError> {
        if wav_bytes.is_empty() {
            return Ok(());
        }

        fs::create_dir_all(&self.temp_dir).map_err(|err| PlatformError::Io(err.to_string()))?;

        let seq = AUDIO_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = self
            .temp_dir
            .join(format!("sye-chime-{}-{seq}.wav", std::process::id()));
        fs::write(&path, wav_bytes).map_err(|err| PlatformError::Io(err.to_string()))?;

        match spawn_first_available_player(self.players, &path) {
            Ok(mut child) => {
                let cleanup_path = path.clone();
                if thread::Builder::new()
                    .name("sye-audio-reaper".to_owned())
                    .spawn(move || {
                        let _ = child.wait();
                        let _ = fs::remove_file(&cleanup_path);
                    })
                    .is_err()
                {
                    let _ = fs::remove_file(&path);
                }
                Ok(())
            }
            Err(err) => {
                let _ = fs::remove_file(&path);
                Err(err)
            }
        }
    }
}

fn default_audio_temp_dir() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR")
            && !runtime_dir.trim().is_empty()
        {
            return PathBuf::from(runtime_dir).join("safe-your-eyes");
        }
    }
    std::env::temp_dir().join("safe-your-eyes")
}

#[cfg(target_os = "macos")]
const DEFAULT_PLAYER_SPECS: &[PlayerSpec] = &[PlayerSpec {
    program: "afplay",
    args: &[],
}];

#[cfg(target_os = "windows")]
const DEFAULT_PLAYER_SPECS: &[PlayerSpec] = &[PlayerSpec {
    program: "powershell.exe",
    args: &[
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
    ],
}];

#[cfg(all(unix, not(target_os = "macos")))]
const DEFAULT_PLAYER_SPECS: &[PlayerSpec] = &[
    PlayerSpec {
        program: "pw-play",
        args: &[],
    },
    PlayerSpec {
        program: "paplay",
        args: &[],
    },
    PlayerSpec {
        program: "aplay",
        args: &["-q"],
    },
];

fn spawn_first_available_player(
    players: &[PlayerSpec],
    wav_path: &Path,
) -> Result<Child, PlatformError> {
    let mut last_error: Option<String> = None;

    for spec in players {
        let mut cmd = Command::new(spec.program);
        cmd.args(spec.args);

        #[cfg(target_os = "windows")]
        if spec.program.eq_ignore_ascii_case("powershell.exe") {
            let escaped = wav_path.to_string_lossy().replace('\'', "''");
            cmd.arg(format!(
                "(New-Object Media.SoundPlayer '{escaped}').PlaySync()"
            ));
        } else {
            cmd.arg(wav_path);
        }

        #[cfg(not(target_os = "windows"))]
        {
            cmd.arg(wav_path);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match crate::console::hide_child_console(&mut cmd).spawn() {
            Ok(child) => return Ok(child),
            Err(err) => {
                last_error = Some(format!("{}: {err}", spec.program));
            }
        }
    }

    let detail = last_error
        .as_deref()
        .unwrap_or("no audio playback backend configured");
    Err(PlatformError::Unavailable(format!(
        "no OS audio player available ({detail})"
    )))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use super::*;
    use crate::test_util::TestTempDir;

    #[test]
    fn empty_wav_and_zero_volume_create_no_files() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("audio-empty")?;
        let player = OsAudioPlayer::with_temp_dir(tmp.path().to_path_buf());

        player.play_wav(&[])?;
        player.play_chime(ChimeKind::PreBreak, 0)?;

        let entries: Vec<_> = fs::read_dir(tmp.path())?.collect();
        assert!(entries.is_empty());
        Ok(())
    }

    #[test]
    fn missing_player_returns_unavailable_and_cleans_temp_file()
    -> Result<(), Box<dyn std::error::Error>> {
        const MISSING_PLAYER: &[PlayerSpec] = &[PlayerSpec {
            program: "sye-nonexistent-audio-player-binary",
            args: &[],
        }];
        let tmp = TestTempDir::new("audio-missing")?;
        let player = OsAudioPlayer::with_players(tmp.path().to_path_buf(), MISSING_PLAYER);

        let result = player.play_chime(ChimeKind::PreBreak, 80);
        assert!(matches!(result, Err(PlatformError::Unavailable(_))));

        let entries: Vec<_> = fs::read_dir(tmp.path())?.collect();
        assert!(entries.is_empty(), "temp WAV file must be removed on error");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn successful_spawn_cleans_up_temp_wav_asynchronously() -> Result<(), Box<dyn std::error::Error>>
    {
        const TRUE_PLAYER: &[PlayerSpec] = &[PlayerSpec {
            program: "true",
            args: &[],
        }];
        let tmp = TestTempDir::new("audio-ok")?;
        let player = OsAudioPlayer::with_players(tmp.path().to_path_buf(), TRUE_PLAYER);

        // Volume > 100 must clamp cleanly without error.
        player.play_chime(ChimeKind::PostBreak, 250)?;

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = fs::read_dir(tmp.path())?.count();
            if remaining == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err("timed out waiting for reaper thread to delete temp WAV".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
