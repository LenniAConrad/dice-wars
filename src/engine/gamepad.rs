//! Gamepads, delivered to the game as ordinary key presses.
//!
//! Each platform backend reduces every connected controller to a bit set of
//! the buttons below (sticks already turned into directions) and ORs them
//! together; `Gamepad::update` turns changes of that set into key events:
//!
//! | Pad | Keys |
//! | --- | --- |
//! | D-pad, left stick | arrow keys, repeating while held |
//! | A / Cross | Enter |
//! | B / Circle, Start / Options | Escape |
//! | X / Square | Space |
//! | Y / Triangle, RB / R1 | Tab |
//! | LB / L1 | Shift+Tab |
//!
//! Back/Select is read but not mapped.
use super::{Input, KeyCode};
use std::time::Instant;

pub(super) const UP: u16 = 1;
pub(super) const DOWN: u16 = 1 << 1;
pub(super) const LEFT: u16 = 1 << 2;
pub(super) const RIGHT: u16 = 1 << 3;
/// The bottom face button (A, Cross).
pub(super) const SOUTH: u16 = 1 << 4;
/// The right face button (B, Circle).
pub(super) const EAST: u16 = 1 << 5;
/// The left face button (X, Square).
pub(super) const WEST: u16 = 1 << 6;
/// The top face button (Y, Triangle).
pub(super) const NORTH: u16 = 1 << 7;
pub(super) const LB: u16 = 1 << 8;
pub(super) const RB: u16 = 1 << 9;
pub(super) const START: u16 = 1 << 10;
pub(super) const BACK: u16 = 1 << 11;

/// A stick deflection (each axis -1..1, y pointing down) past the deadzone
/// becomes the direction of its dominant axis, so diagonals do not press two
/// arrows at once.
pub(super) fn stick(x: f32, y: f32) -> u16 {
    const DEADZONE: f32 = 0.5;
    if x.abs().max(y.abs()) < DEADZONE {
        0
    } else if x.abs() >= y.abs() {
        if x < 0.0 {
            LEFT
        } else {
            RIGHT
        }
    } else if y < 0.0 {
        UP
    } else {
        DOWN
    }
}

const FIRST_REPEAT: f64 = 0.35;
const REPEAT: f64 = 0.12;
const ARROWS: [(u16, KeyCode); 4] = [
    (LEFT, KeyCode::Left),
    (RIGHT, KeyCode::Right),
    (UP, KeyCode::Up),
    (DOWN, KeyCode::Down),
];
// Shift comes before Tab, so Shift is already down when Tab is pressed.
const KEYS: [KeyCode; 5] = [
    KeyCode::LeftShift,
    KeyCode::Enter,
    KeyCode::Escape,
    KeyCode::Space,
    KeyCode::Tab,
];
fn keys_for(buttons: u16) -> [bool; 5] {
    let has = |b: u16| buttons & b != 0;
    [
        has(LB),
        has(SOUTH),
        has(EAST | START),
        has(WEST),
        has(NORTH | RB | LB),
    ]
}

pub(super) struct Gamepad {
    epoch: Instant,
    /// Buttons whose keys are currently held down by the pad.
    active: u16,
    /// Buttons that were held while the window had no focus; they count again
    /// once released, so returning to the window does not press anything.
    blocked: u16,
    /// When each arrow (in `ARROWS` order) repeats next.
    repeat_at: [f64; 4],
}
impl Default for Gamepad {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            active: 0,
            blocked: 0,
            repeat_at: [0.0; 4],
        }
    }
}
impl Gamepad {
    pub fn update(&mut self, held: u16, focused: bool, input: &mut Input) {
        let now = self.epoch.elapsed().as_secs_f64();
        self.step(held, focused, now, input);
    }
    /// One poll: `held` is every pad's buttons ORed, `now` is in seconds.
    pub fn step(&mut self, held: u16, focused: bool, now: f64, input: &mut Input) {
        let held = held & !BACK;
        let next = if focused {
            self.blocked &= held;
            held & !self.blocked
        } else {
            self.blocked = held;
            0
        };
        let (before, after) = (keys_for(self.active), keys_for(next));
        for (i, &key) in KEYS.iter().enumerate() {
            if before[i] && !after[i] {
                input.key(key, false);
            }
        }
        for (i, &key) in KEYS.iter().enumerate() {
            if after[i] && !before[i] {
                input.key(key, true);
            }
        }
        for (i, &(bit, key)) in ARROWS.iter().enumerate() {
            match (self.active & bit != 0, next & bit != 0) {
                (false, true) => {
                    input.key(key, true);
                    self.repeat_at[i] = now + FIRST_REPEAT;
                }
                (true, true) if now >= self.repeat_at[i] => {
                    // Arrow keys report a press on every key-down, like
                    // keyboard auto-repeat. After a stall, repeat once.
                    input.key(key, true);
                    self.repeat_at[i] = (self.repeat_at[i] + REPEAT).max(now + REPEAT * 0.5);
                }
                (true, false) => input.key(key, false),
                _ => {}
            }
        }
        self.active = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn presses(input: &Input) -> Vec<KeyCode> {
        let mut keys: Vec<KeyCode> = input.pressed.iter().copied().collect();
        keys.sort_by_key(|k| format!("{k:?}"));
        keys
    }
    /// Runs 60 Hz frames from `from` to `to` seconds and counts presses of `key`.
    fn count(
        pad: &mut Gamepad,
        input: &mut Input,
        held: u16,
        from: f64,
        to: f64,
        key: KeyCode,
    ) -> usize {
        let mut n = 0;
        let mut t = from;
        while t < to {
            input.reset();
            pad.step(held, true, t, input);
            n += input.pressed.contains(&key) as usize;
            t += 1.0 / 60.0;
        }
        n
    }
    #[test]
    fn buttons_become_key_presses_and_releases() {
        let mut pad = Gamepad::default();
        let mut input = Input::default();
        pad.step(SOUTH, true, 0.0, &mut input);
        assert_eq!(presses(&input), [KeyCode::Enter]);
        assert!(input.keys.contains(&KeyCode::Enter));
        input.reset();
        pad.step(SOUTH, true, 0.1, &mut input);
        assert!(input.pressed.is_empty(), "holding A presses Enter once");
        pad.step(0, true, 0.2, &mut input);
        assert!(input.keys.is_empty());
        for (button, key) in [
            (EAST, KeyCode::Escape),
            (START, KeyCode::Escape),
            (WEST, KeyCode::Space),
            (NORTH, KeyCode::Tab),
            (RB, KeyCode::Tab),
        ] {
            input.reset();
            pad.step(button, true, 1.0, &mut input);
            assert_eq!(presses(&input), [key]);
            pad.step(0, true, 1.1, &mut input);
            assert!(input.keys.is_empty());
        }
        input.reset();
        pad.step(BACK, true, 2.0, &mut input);
        assert!(
            input.pressed.is_empty() && input.keys.is_empty(),
            "Back is unmapped"
        );
    }
    #[test]
    fn left_bumper_is_shift_tab() {
        let mut pad = Gamepad::default();
        let mut input = Input::default();
        pad.step(LB, true, 0.0, &mut input);
        assert_eq!(presses(&input), [KeyCode::LeftShift, KeyCode::Tab]);
        assert!(input.keys.contains(&KeyCode::LeftShift));
        input.reset();
        // RB while LB is held: Tab stays down, Shift too.
        pad.step(LB | RB, true, 0.1, &mut input);
        assert!(input.pressed.is_empty());
        pad.step(RB, true, 0.2, &mut input);
        assert!(!input.keys.contains(&KeyCode::LeftShift));
        assert!(input.keys.contains(&KeyCode::Tab));
        pad.step(0, true, 0.3, &mut input);
        assert!(input.keys.is_empty());
    }
    #[test]
    fn held_directions_repeat_like_a_keyboard() {
        let mut pad = Gamepad::default();
        let mut input = Input::default();
        // first press, then nothing until 0.35 s, then every 0.12 s
        assert_eq!(
            count(&mut pad, &mut input, RIGHT, 0.0, 0.34, KeyCode::Right),
            1
        );
        assert_eq!(
            count(&mut pad, &mut input, RIGHT, 0.34, 0.9, KeyCode::Right),
            5
        );
        input.reset();
        pad.step(0, true, 1.0, &mut input);
        assert!(input.keys.is_empty() && input.pressed.is_empty());
        // a new press starts over
        assert_eq!(
            count(&mut pad, &mut input, RIGHT, 1.1, 1.4, KeyCode::Right),
            1
        );
        // a long stall repeats once, not in a burst
        input.reset();
        pad.step(RIGHT, true, 9.0, &mut input);
        assert!(input.pressed.contains(&KeyCode::Right));
        input.reset();
        pad.step(RIGHT, true, 9.01, &mut input);
        assert!(input.pressed.is_empty());
    }
    #[test]
    fn sticks_have_a_deadzone_and_a_dominant_axis() {
        assert_eq!(stick(0.3, -0.4), 0);
        assert_eq!(stick(-0.6, 0.2), LEFT);
        assert_eq!(stick(0.9, 0.7), RIGHT);
        assert_eq!(stick(0.2, -0.8), UP);
        assert_eq!(stick(-0.5, 0.55), DOWN);
    }
    #[test]
    fn nothing_reaches_an_unfocused_window() {
        let mut pad = Gamepad::default();
        let mut input = Input::default();
        pad.step(SOUTH | DOWN, false, 0.0, &mut input);
        assert!(input.pressed.is_empty() && input.keys.is_empty());
        // focus returns while the buttons are still held: no presses
        pad.step(SOUTH | DOWN, true, 0.5, &mut input);
        assert!(input.pressed.is_empty() && input.keys.is_empty());
        // released and pressed again: they count
        pad.step(0, true, 0.6, &mut input);
        pad.step(SOUTH, true, 0.7, &mut input);
        assert_eq!(presses(&input), [KeyCode::Enter]);
        // focus lost while held: the window's blur clears keys; pad keys are
        // released and do not come back on refocus
        input.blur();
        input.reset();
        pad.step(SOUTH, false, 0.8, &mut input);
        pad.step(SOUTH, true, 0.9, &mut input);
        assert!(input.pressed.is_empty() && input.keys.is_empty());
    }
}
