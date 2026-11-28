use std::f64::consts::TAU;

pub const CHIME_SAMPLE_RATE: u32 = 22_050;

pub const CHIME_CHANNELS: u16 = 1;

pub const CHIME_BITS_PER_SAMPLE: u16 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChimeKind {
    PreBreak,
    PostBreak,
}

const BYTES_PER_SAMPLE: u16 = CHIME_BITS_PER_SAMPLE / 8;
const BLOCK_ALIGN: u16 = CHIME_CHANNELS * BYTES_PER_SAMPLE;
const BYTE_RATE: u32 = CHIME_SAMPLE_RATE * (BLOCK_ALIGN as u32);
const WAV_HEADER_BYTES: usize = 44;

const fn note_samples(dur_ms: u32) -> u32 {
    ((CHIME_SAMPLE_RATE as u64 * dur_ms as u64) / 1000) as u32
}

#[must_use]
pub fn synthesize_chime_wav(kind: ChimeKind, volume: u8) -> Vec<u8> {
    let notes: &[(f64, u32)] = match kind {
        ChimeKind::PreBreak => &[(523.25, 180), (659.25, 180)],
        ChimeKind::PostBreak => &[(783.99, 160), (523.25, 240)],
    };

    let total_samples: u32 = notes.iter().map(|&(_, dur_ms)| note_samples(dur_ms)).sum();
    let data_bytes = total_samples.saturating_mul(u32::from(BLOCK_ALIGN));
    let riff_chunk_size = 36u32.saturating_add(data_bytes);
    let data_len_usize = usize::try_from(data_bytes).unwrap_or(0);

    let mut wav = Vec::with_capacity(WAV_HEADER_BYTES + data_len_usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&CHIME_CHANNELS.to_le_bytes());
    wav.extend_from_slice(&CHIME_SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&BYTE_RATE.to_le_bytes());
    wav.extend_from_slice(&BLOCK_ALIGN.to_le_bytes());
    wav.extend_from_slice(&CHIME_BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());

    if volume == 0 {
        wav.resize(WAV_HEADER_BYTES + data_len_usize, 0);
        return wav;
    }

    let gain = f64::from(volume.min(100)) / 100.0;
    let max_amplitude = 0.75 * f64::from(i16::MAX) * gain;
    let attack_samples = f64::from(note_samples(12).max(1));
    let sample_rate_f64 = f64::from(CHIME_SAMPLE_RATE);

    for &(freq_hz, dur_ms) in notes {
        let count = note_samples(dur_ms);
        let note_len_f64 = f64::from(count.max(1));
        for idx in 0..count {
            let idx_f64 = f64::from(idx);
            let t = idx_f64 / sample_rate_f64;
            let t_norm = idx_f64 / note_len_f64;
            let attack = (idx_f64 / attack_samples).min(1.0);
            let decay = (-6.0 * t_norm).exp();
            // Smooth fade-out on the last 10 ms to guarantee zero end-click.
            let tail = ((note_len_f64 - idx_f64) / attack_samples).min(1.0);
            let envelope = attack * decay * tail;

            let wave = 0.2f64.mul_add(
                (TAU * (2.0 * freq_hz) * t).sin(),
                0.8 * (TAU * freq_hz * t).sin(),
            );
            let scaled =
                (wave * envelope * max_amplitude).clamp(f64::from(i16::MIN), f64::from(i16::MAX));
            let sample = scaled.round() as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }

    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, Box<dyn std::error::Error>> {
        let slice: [u8; 2] = bytes[offset..offset + 2].try_into()?;
        Ok(u16::from_le_bytes(slice))
    }

    fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn std::error::Error>> {
        let slice: [u8; 4] = bytes[offset..offset + 4].try_into()?;
        Ok(u32::from_le_bytes(slice))
    }

    #[test]
    fn wav_header_and_byte_length_invariants() -> Result<(), Box<dyn std::error::Error>> {
        for kind in [ChimeKind::PreBreak, ChimeKind::PostBreak] {
            let wav = synthesize_chime_wav(kind, 100);
            assert!(wav.len() > 44);
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
            assert_eq!(&wav[12..16], b"fmt ");
            assert_eq!(read_u32_le(&wav, 16)?, 16);
            assert_eq!(read_u16_le(&wav, 20)?, 1);
            assert_eq!(read_u16_le(&wav, 22)?, CHIME_CHANNELS);
            assert_eq!(read_u32_le(&wav, 24)?, CHIME_SAMPLE_RATE);
            assert_eq!(read_u32_le(&wav, 28)?, CHIME_SAMPLE_RATE * 2);
            assert_eq!(read_u16_le(&wav, 32)?, 2);
            assert_eq!(read_u16_le(&wav, 34)?, CHIME_BITS_PER_SAMPLE);
            assert_eq!(&wav[36..40], b"data");

            let riff_size = read_u32_le(&wav, 4)? as usize;
            let data_size = read_u32_le(&wav, 40)? as usize;
            assert_eq!(riff_size + 8, wav.len());
            assert_eq!(44 + data_size, wav.len());
            assert_eq!(data_size % 2, 0);
        }
        Ok(())
    }

    #[test]
    fn wav_volume_zero_is_silent_valid_wav() {
        let wav = synthesize_chime_wav(ChimeKind::PreBreak, 0);
        assert!(wav.len() > 44);
        assert!(wav[44..].iter().all(|&b| b == 0));
    }

    #[test]
    fn wav_volume_over_100_clamps_without_clipping() {
        let max_wav = synthesize_chime_wav(ChimeKind::PreBreak, 100);
        let over_wav = synthesize_chime_wav(ChimeKind::PreBreak, 255);
        assert_eq!(over_wav, max_wav);

        let mut peak: i32 = 0;
        for &chunk in max_wav[44..].as_chunks::<2>().0 {
            let sample = i16::from_le_bytes(chunk);
            peak = peak.max(i32::from(sample).abs());
        }
        assert!(peak > 1000, "expected audible chime signal");
        assert!(
            peak < i32::from(i16::MAX),
            "expected headroom below clipping"
        );
    }

    #[test]
    fn wav_pre_and_post_chimes_differ() {
        let pre = synthesize_chime_wav(ChimeKind::PreBreak, 80);
        let post = synthesize_chime_wav(ChimeKind::PostBreak, 80);
        assert_ne!(pre, post);
    }
}
