use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams};
use macroquad::prelude::*;

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let rate: u32 = 44_100;
    let n = samples.len() as u32 * 2;
    let mut d = Vec::with_capacity(44 + samples.len() * 2);
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + n).to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&rate.to_le_bytes());
    d.extend_from_slice(&(rate * 2).to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&16u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&n.to_le_bytes());
    for &s in samples {
        d.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    d
}

#[macroquad::main("sndcheck")]
async fn main() {
    let samples: Vec<f32> = (0..44100)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.4)
        .collect();
    eprintln!("sndcheck: loading {} byte wav", 44 + samples.len() * 2);
    match load_sound_from_bytes(&wav_bytes(&samples)).await {
        Ok(s) => {
            eprintln!("sndcheck: LOAD OK, playing");
            play_sound(
                &s,
                PlaySoundParams {
                    looped: false,
                    volume: 0.8,
                },
            );
        }
        Err(e) => eprintln!("sndcheck: LOAD ERROR: {:?}", e),
    }
    for _ in 0..150 {
        clear_background(WHITE);
        next_frame().await;
    }
    eprintln!("sndcheck: done");
}
