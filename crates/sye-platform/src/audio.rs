use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use sye_core::{ChimeKind, PlatformError, synthesize_chime_wav};

static AUDIO_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
struct PlayerSpec {
    program: &'static str,
    args: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct OsAudioPlayer {
    temp_dir: PathBuf,
    players: &'static [PlayerSpec],
}

impl Default for OsAudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl OsAudioPlayer {
    #[must_use]
    pub fn new() -> Self {
        Self::with_temp_dir(default_audio_temp_dir())
    }

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

    pub fn play_chime(&self, kind: ChimeKind, volume: u8) -> Result<(), PlatformError> {
        let clamped = volume.min(100);
        if clamped == 0 {
            return Ok(());
        }
        let wav = synthesize_chime_wav(kind, clamped);
        self.play_wav(&wav)
    }

    pub fn play_wav(&self, wav_bytes: &[u8]) -> Result<(), PlatformError> {
        if wav_bytes.is_empty() {
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        if self.players.is_empty() {
            return play_windows_wav_in_memory(wav_bytes);
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

#[cfg(any(target_os = "windows", test))]
fn is_valid_riff_wav(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return false;
    }
    let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    declared
        .checked_add(8)
        .is_some_and(|total| total <= bytes.len())
}

#[cfg(target_os = "windows")]
fn play_windows_wav_in_memory(wav_bytes: &[u8]) -> Result<(), PlatformError> {
    use std::ptr;

    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};

    if !is_valid_riff_wav(wav_bytes) {
        return Err(PlatformError::Io(
            "invalid RIFF/WAVE audio buffer".to_owned(),
        ));
    }

    let wav_owned = wav_bytes.to_vec();
    thread::Builder::new()
        .name("sye-audio-win32".to_owned())
        .spawn(move || {
            // SAFETY: `is_valid_riff_wav` verified the RIFF/WAVE header and that the
            // declared RIFF chunk length + 8 fits within `wav_owned.len()`, so `PlaySoundW`
            // cannot read past the buffer. `SND_MEMORY | SND_SYNC` keeps `wav_owned` live
            // on this thread until playback completes.
            #[expect(unsafe_code, reason = "Win32 PlaySoundW in-memory WAV playback")]
            let _ = unsafe {
                PlaySoundW(
                    wav_owned.as_ptr().cast(),
                    ptr::null_mut(),
                    SND_MEMORY | SND_SYNC | SND_NODEFAULT,
                )
            };
        })
        .map(|_| ())
        .map_err(|err| PlatformError::Io(err.to_string()))
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
const DEFAULT_PLAYER_SPECS: &[PlayerSpec] = &[];

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
        cmd.arg(wav_path);
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
    fn riff_wav_validation_checks_header_and_declared_length() {
        let valid = synthesize_chime_wav(ChimeKind::PreBreak, 80);
        assert!(is_valid_riff_wav(&valid));

        assert!(!is_valid_riff_wav(&[]));
        assert!(!is_valid_riff_wav(b"RIFF\x04\x00\x00\x00WAV"));
        assert!(!is_valid_riff_wav(b"XXXX\x04\x00\x00\x00WAVE"));
        assert!(!is_valid_riff_wav(b"RIFF\x04\x00\x00\x00XXXX"));
        assert!(is_valid_riff_wav(b"RIFF\x04\x00\x00\x00WAVE"));
        assert!(!is_valid_riff_wav(b"RIFF\x05\x00\x00\x00WAVE"));
        assert!(!is_valid_riff_wav(b"RIFF\xff\xff\xff\xffWAVE"));
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
