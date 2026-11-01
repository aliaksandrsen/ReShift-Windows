//! MARK: - Feedback
//!
//! The short sound after a conversion. Notifications are
//! shown by the main thread, which owns the tray icon.

use std::f64::consts::TAU;
use std::sync::LazyLock;

use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

/// A 70 ms bell-like click as an in-memory WAV (the macOS app plays NSSound "Tink"). Static:
/// the buffer must outlive the asynchronous playback.
static TINK_SOUND: LazyLock<Vec<u8>> = LazyLock::new(create_tink_sound);

/// Plays the "tink" without waiting for it.
pub fn play_tink() {
    let sound = &*TINK_SOUND;
    // SAFETY: SND_MEMORY reads the static WAV buffer, which lives for the whole process
    let _ = unsafe {
        PlaySoundW(
            PCWSTR(sound.as_ptr().cast()),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        )
    };
}

fn create_tink_sound() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44_100;
    const SAMPLES: u32 = SAMPLE_RATE * 70 / 1000;

    let mut wav = Vec::with_capacity(44 + SAMPLES as usize * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + SAMPLES * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(SAMPLES * 2).to_le_bytes());
    for i in 0..SAMPLES {
        let t = f64::from(i) / f64::from(SAMPLE_RATE);
        let attack = (t / 0.002).min(1.0);
        let envelope = attack * (-t / 0.015).exp();
        let wave = 0.75 * (TAU * 2200.0 * t).sin() + 0.25 * (TAU * 3300.0 * t).sin();
        let sample = (wave * envelope * 0.35 * f64::from(i16::MAX)) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tink_is_a_valid_70_ms_wav() {
        let wav = create_tink_sound();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        let data_length = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        assert_eq!(data_length, 44_100 * 70 / 1000 * 2);
        assert_eq!(wav.len(), 44 + data_length);
    }
}
