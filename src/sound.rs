//! Sound effects. All effects are synthesized at startup into in-memory
//! WAVs — no asset files.

use dicegame::engine::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};

use crate::game::T_SHAKE;

const SND_RATE: u32 = 44_100;

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let n = samples.len() as u32 * 2;
    let mut d = Vec::with_capacity(44 + samples.len() * 2);
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + n).to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes()); // PCM
    d.extend_from_slice(&1u16.to_le_bytes()); // mono
    d.extend_from_slice(&SND_RATE.to_le_bytes());
    d.extend_from_slice(&(SND_RATE * 2).to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&16u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&n.to_le_bytes());
    for &s in samples {
        d.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    d
}

pub(crate) fn secs(n: f32) -> usize {
    (n * SND_RATE as f32) as usize
}

// Every effect starts and ends at zero: a short attack ramp and a release
// fade remove the clicks a truncated decay would otherwise leave.
pub(crate) fn finish(mut v: Vec<f32>) -> Vec<f32> {
    let attack = secs(0.002).min(v.len());
    for (i, s) in v.iter_mut().take(attack).enumerate() {
        *s *= i as f32 / attack as f32;
    }
    let release = secs(0.015).min(v.len());
    let n = v.len();
    for i in 0..release {
        v[n - 1 - i] *= i as f32 / release as f32;
    }
    v
}

// A soft mallet: a fundamental with quickly fading overtones, like a
// marimba bar. Warmer than a bare sine at the same loudness.
pub(crate) fn tone(f: f32, dur: f32, amp: f32, decay: f32) -> Vec<f32> {
    let tau = std::f32::consts::TAU;
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / SND_RATE as f32;
            let body = (tau * f * t).sin()
                + 0.32 * (tau * 2.0 * f * t).sin() * (-t * decay * 1.8).exp()
                + 0.10 * (tau * 3.01 * f * t).sin() * (-t * decay * 3.0).exp();
            body * amp * (-t * decay).exp() * (t / 0.003).min(1.0)
        })
        .collect()
}

// a tone sliding from f0 to f1 with exponential decay
fn glide(f0: f32, f1: f32, dur: f32, amp: f32, decay: f32) -> Vec<f32> {
    let n = secs(dur);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SND_RATE as f32;
            let f = f0 + (f1 - f0) * (t / dur);
            phase += std::f32::consts::TAU * f / SND_RATE as f32;
            (phase.sin() + 0.2 * (2.0 * phase).sin()) * amp * (-t * decay).exp()
        })
        .collect()
}

fn add_at(base: &mut Vec<f32>, add: Vec<f32>, offset: f32) {
    let off = secs(offset);
    if base.len() < off + add.len() {
        base.resize(off + add.len(), 0.0);
    }
    for (i, s) in add.into_iter().enumerate() {
        base[off + i] += s;
    }
}

// Two quiet early reflections give musical cues a little room.
pub(crate) fn room(mut v: Vec<f32>) -> Vec<f32> {
    let dry = v.clone();
    for (delay, gain) in [(0.067, 0.20), (0.131, 0.09)] {
        let mut wet: Vec<f32> = dry.iter().map(|s| s * gain).collect();
        // darken the reflections slightly
        let mut lp = 0.0;
        for s in &mut wet {
            lp += 0.35 * (*s - lp);
            *s = lp;
        }
        add_at(&mut v, wet, delay);
    }
    v
}

struct Noise(u32);

impl Noise {
    pub(crate) fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 8_388_608.0 - 1.0
    }
}

// A die landing on the table: a band-limited tick with a short wooden body.
fn clack(seed: u32, amp: f32) -> Vec<f32> {
    let mut noise = Noise(seed);
    let (mut lp, mut hp_prev) = (0.0f32, 0.0f32);
    let mut v: Vec<f32> = (0..secs(0.05))
        .map(|i| {
            let t = i as f32 / SND_RATE as f32;
            let n = noise.next();
            lp += 0.45 * (n - lp);
            let band = lp - hp_prev;
            hp_prev += 0.08 * (lp - hp_prev);
            band * (-t * 140.0).exp() * amp
        })
        .collect();
    add_at(
        &mut v,
        tone(620.0 + (seed % 7) as f32 * 40.0, 0.045, amp * 0.25, 90.0),
        0.0,
    );
    v
}

// Dice tumbling on felt, as long as the on-screen shake. Each variant has its
// own bounce pattern, and bigger battles throw more dice at once.
pub(crate) fn rattle(seed: u32, big: bool) -> Vec<f32> {
    let dur = T_SHAKE * 0.6 + 0.02;
    let mut noise = Noise(seed);
    let mut v = vec![0.0f32; secs(dur)];
    let hits = if big { 11 } else { 6 };
    for k in 0..hits {
        let at = (k as f32 + 0.5 * (noise.next() + 1.0)) / hits as f32 * (dur - 0.06);
        let amp = 0.16 + 0.10 * (noise.next() + 1.0) * 0.5;
        add_at(&mut v, clack(seed.wrapping_add(k * 977), amp), at);
    }
    // a soft felt rumble underneath
    let mut lp = 0.0f32;
    let gain = if big { 0.5 } else { 0.32 };
    for (i, s) in v.iter_mut().enumerate() {
        let t = i as f32 / SND_RATE as f32;
        lp += 0.04 * (noise.next() - lp);
        *s += lp * gain * (t / 0.03).min(1.0) * ((dur - t) / 0.08).clamp(0.0, 1.0);
    }
    v
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Snd {
    Click,
    Select,
    Invalid,
    Roll { big: bool, quiet: bool },
    Land { quiet: bool },
    Capture,  // you took a land
    Repel,    // your attack failed
    Lost,     // one of your lands fell
    Defended, // your land held
    OtherCapture,
    OtherRepel,
    Reinforce,
    Turn,
    Eliminated,
    Win,
    Lose,
}

pub(crate) struct Sounds {
    pub(crate) click: Sound,
    pub(crate) select: Sound,
    pub(crate) invalid: Sound,
    pub(crate) rolls: Vec<Sound>,     // small battles
    pub(crate) rolls_big: Vec<Sound>, // big battles
    pub(crate) land: Sound,
    pub(crate) capture: Sound,
    pub(crate) repel: Sound,
    pub(crate) lost: Sound,
    pub(crate) defended: Sound,
    pub(crate) other_capture: Sound,
    pub(crate) other_repel: Sound,
    pub(crate) reinforce: Sound,
    pub(crate) turn: Sound,
    pub(crate) eliminated: Sound,
    pub(crate) win: Sound,
    pub(crate) lose: Sound,
    pub(crate) variant: std::cell::Cell<usize>,
    pub(crate) volume: std::cell::Cell<f32>,
}

impl Sounds {
    // None if the audio device is unavailable — the game then runs silent
    pub(crate) fn load() -> Option<Sounds> {
        fn snd(samples: Vec<f32>) -> Option<Sound> {
            load_sound_from_bytes(&wav_bytes(&finish(samples))).ok()
        }
        fn notes(seq: &[(f32, f32, f32, f32)], amp: f32, decay: f32) -> Vec<f32> {
            let mut v = Vec::new();
            for &(f, at, dur, gain) in seq {
                add_at(&mut v, tone(f, dur, amp * gain, decay), at);
            }
            v
        }
        // C major for good news, its relative minor for bad news.
        let (c5, e5, g5, c6) = (523.25, 659.25, 783.99, 1046.5);
        let (a4, d5, ef5) = (440.0, 587.33, 622.25);
        let capture = room(notes(
            &[
                (c5, 0.0, 0.32, 1.0),
                (e5, 0.06, 0.3, 0.9),
                (g5, 0.12, 0.34, 0.9),
            ],
            0.22,
            9.0,
        ));
        let lost = room(notes(
            &[(ef5, 0.0, 0.3, 1.0), (c5, 0.09, 0.42, 1.0)],
            0.2,
            7.0,
        ));
        let defended = room(notes(
            &[(d5, 0.0, 0.26, 1.0), (a4 * 2.0, 0.0, 0.22, 0.6)],
            0.17,
            12.0,
        ));
        let mut repel = glide(250.0, 150.0, 0.26, 0.24, 9.0);
        add_at(&mut repel, clack(91, 0.25), 0.0);
        let mut reinforce = Vec::new();
        for (k, f) in [880.0, 1108.7, 1318.5].into_iter().enumerate() {
            add_at(&mut reinforce, tone(f, 0.09, 0.16, 30.0), k as f32 * 0.055);
            add_at(
                &mut reinforce,
                clack(300 + k as u32, 0.12),
                k as f32 * 0.055,
            );
        }
        let win = room(notes(
            &[
                (c5, 0.0, 0.5, 1.0),
                (e5, 0.11, 0.5, 0.9),
                (g5, 0.22, 0.5, 0.9),
                (c6, 0.33, 0.9, 1.0),
                (e5, 0.33, 0.9, 0.5),
                (g5, 0.33, 0.9, 0.5),
            ],
            0.2,
            3.6,
        ));
        let mut lose = notes(
            &[
                (g5 * 0.5, 0.0, 0.5, 1.0),
                (ef5 * 0.5, 0.16, 0.5, 1.0),
                (c5 * 0.5, 0.32, 0.8, 1.0),
            ],
            0.24,
            4.0,
        );
        add_at(&mut lose, glide(220.0, 110.0, 0.7, 0.08, 3.0), 0.3);
        Some(Sounds {
            click: snd(tone(1760.0, 0.035, 0.22, 95.0))?,
            select: snd(notes(
                &[(1318.5, 0.0, 0.08, 1.0), (1975.5, 0.012, 0.06, 0.4)],
                0.22,
                48.0,
            ))?,
            invalid: snd(notes(
                &[(311.1, 0.0, 0.09, 1.0), (233.1, 0.08, 0.14, 1.0)],
                0.3,
                20.0,
            ))?,
            rolls: (0..3)
                .map(|k| snd(rattle(0x1234_5678 + k * 7919, false)))
                .collect::<Option<_>>()?,
            rolls_big: (0..3)
                .map(|k| snd(rattle(0x0BAD_5EED + k * 104_729, true)))
                .collect::<Option<_>>()?,
            land: snd({
                let mut v = clack(4242, 0.3);
                add_at(&mut v, clack(777, 0.22), 0.028);
                v
            })?,
            capture: snd(capture)?,
            repel: snd(repel)?,
            lost: snd(lost)?,
            defended: snd(defended)?,
            other_capture: snd(tone(392.0, 0.22, 0.2, 14.0))?,
            other_repel: snd(glide(200.0, 150.0, 0.16, 0.18, 16.0))?,
            reinforce: snd(reinforce)?,
            turn: snd(room(notes(
                &[(g5, 0.0, 0.16, 1.0), (c6, 0.09, 0.26, 1.0)],
                0.17,
                13.0,
            )))?,
            eliminated: snd(room(notes(
                &[
                    (a4, 0.0, 0.2, 1.0),
                    (370.0, 0.12, 0.2, 1.0),
                    (293.7, 0.24, 0.4, 1.0),
                ],
                0.22,
                7.0,
            )))?,
            win: snd(win)?,
            lose: snd(lose)?,
            variant: std::cell::Cell::new(0),
            volume: std::cell::Cell::new(1.0),
        })
    }

    pub(crate) fn play(&self, s: Snd) {
        // Variants rotate rather than using the game RNG, which must stay
        // reserved for dice so seeds and replays remain reproducible.
        let pick = |v: &Vec<Sound>| {
            let k = self.variant.get();
            self.variant.set(k.wrapping_add(1));
            v[k % v.len()].clone()
        };
        let (sound, volume) = match s {
            Snd::Click => (self.click.clone(), 0.45),
            Snd::Select => (self.select.clone(), 0.5),
            Snd::Invalid => (self.invalid.clone(), 0.5),
            Snd::Roll { big, quiet } => (
                pick(if big { &self.rolls_big } else { &self.rolls }),
                if quiet { 0.32 } else { 0.6 },
            ),
            Snd::Land { quiet } => (self.land.clone(), if quiet { 0.28 } else { 0.55 }),
            Snd::Capture => (self.capture.clone(), 0.7),
            Snd::Repel => (self.repel.clone(), 0.6),
            Snd::Lost => (self.lost.clone(), 0.7),
            Snd::Defended => (self.defended.clone(), 0.65),
            Snd::OtherCapture => (self.other_capture.clone(), 0.3),
            Snd::OtherRepel => (self.other_repel.clone(), 0.28),
            Snd::Reinforce => (self.reinforce.clone(), 0.55),
            Snd::Turn => (self.turn.clone(), 0.6),
            Snd::Eliminated => (self.eliminated.clone(), 0.6),
            Snd::Win => (self.win.clone(), 0.75),
            Snd::Lose => (self.lose.clone(), 0.7),
        };
        play_sound(
            &sound,
            PlaySoundParams {
                volume: volume * self.volume.get(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::game::{Battle, TeamCfg};
    use crate::testutil::mk_game;

    use super::*;

    #[test]
    fn sound_effects_start_and_end_silently_and_vary() {
        for v in [
            tone(523.25, 0.3, 0.3, 9.0),
            glide(250.0, 150.0, 0.26, 0.3, 9.0),
            rattle(1, false),
            rattle(2, true),
            room(tone(659.25, 0.2, 0.3, 9.0)),
        ] {
            let v = finish(v);
            assert!(
                v[0].abs() < 1e-3 && v[v.len() - 1].abs() < 1e-3,
                "audible click at an edge"
            );
            assert!(v.iter().all(|s| s.abs() < 1.0), "effect clips on its own");
        }
        assert_ne!(
            rattle(1, false),
            rattle(2, false),
            "roll variants are identical"
        );
        assert!(
            rattle(1, true).len() >= secs(T_SHAKE * 0.6),
            "rattle shorter than the shake"
        );
    }

    #[test]
    fn battle_sounds_follow_the_local_point_of_view() {
        let mut g = mk_game(&[0, 1, 2], &[(0, 1), (1, 2), (0, 2)], 3, TeamCfg::default());
        g.my_seat = 0;
        let fight = |a, d, captured| Battle {
            a,
            d,
            ra: 0,
            rd: 0,
            ad: vec![],
            dd: vec![],
            captured,
            t: 0.0,
        };
        assert_eq!(g.result_sound(&fight(0, 1, true)), Snd::Capture);
        assert_eq!(g.result_sound(&fight(0, 1, false)), Snd::Repel);
        assert_eq!(g.result_sound(&fight(1, 0, true)), Snd::Lost);
        assert_eq!(g.result_sound(&fight(1, 0, false)), Snd::Defended);
        assert_eq!(g.result_sound(&fight(1, 2, true)), Snd::OtherCapture);
        assert_eq!(
            g.roll_sound(1, 2),
            Snd::Roll {
                big: false,
                quiet: true
            }
        );
    }
}
