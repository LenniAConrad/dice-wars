//! Keyboard (and gamepad) control of every screen.
//!
//! Each frame, controls register their rectangles as they are drawn. On the
//! next frame Tab / Shift+Tab step through them in drawing order, the arrow
//! keys jump to the nearest control in that direction, and Enter activates
//! the focused one with a click at its center, so every screen works without
//! code of its own. A dialog, the settings panel or the game-over card
//! registers its controls as "modal": while any exist, only they take focus.
//! Moving or clicking the mouse hides the focus ring.

use std::cell::RefCell;

use dicegame::engine::{self, prelude::*};

use crate::theme::ACCENT;
use crate::ui::{draw_round_frame, virtual_to_window};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Nav {
    /// Every registered control (menus, lobbies, replays).
    All,
    /// Like All, but Tab stays with the screen (the join screen uses it to
    /// move between its text fields).
    AllButTab,
    /// Only modal controls (play: the map keeps the arrow keys).
    ModalOnly,
    /// No keyboard navigation (typing in a text field).
    Off,
}

#[derive(Default)]
struct State {
    items: Vec<Rect>,
    modal: Vec<Rect>,
    prev_items: Vec<Rect>,
    prev_modal: Vec<Rect>,
    in_modal: u32,
    current: Option<Rect>,
    visible: bool,
    last_mouse: (f32, f32),
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Registers a control drawn this frame.
pub(crate) fn focusable(r: Rect) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.in_modal > 0 {
            s.modal.push(r);
        } else {
            s.items.push(r);
        }
    });
}

/// Controls registered between these calls belong to a modal layer.
pub(crate) fn begin_modal() {
    STATE.with(|s| s.borrow_mut().in_modal += 1);
}

pub(crate) fn end_modal() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.in_modal = s.in_modal.saturating_sub(1);
    });
}

fn center(r: Rect) -> Vec2 {
    vec2(r.x + r.w * 0.5, r.y + r.h * 0.5)
}

// the entry of `list` that is (or is closest to) the focused rectangle
fn locate(list: &[Rect], current: Option<Rect>) -> Option<usize> {
    let c = current?;
    let cc = center(c);
    list.iter()
        .enumerate()
        .min_by(|a, b| {
            center(*a.1)
                .distance(cc)
                .total_cmp(&center(*b.1).distance(cc))
        })
        .filter(|(_, r)| center(**r).distance(cc) < 40.0)
        .map(|(i, _)| i)
}

// nearest control in `dir` from `from`, favoring ones straight ahead
fn step(list: &[Rect], from: Rect, dir: Vec2) -> Option<Rect> {
    let c = center(from);
    list.iter()
        .copied()
        .filter(|r| *r != from)
        .filter_map(|r| {
            let d = center(r) - c;
            let ahead = d.x * dir.x + d.y * dir.y;
            let side = (d.x * dir.y - d.y * dir.x).abs();
            (ahead > 4.0).then_some((r, ahead + side * 2.5))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// Handles the navigation keys before the screen runs. Keys it uses are
/// swallowed so the screen underneath does not also react to them.
pub(crate) fn begin_frame(nav: Nav) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        // last frame's registrations are this frame's targets
        s.prev_items = std::mem::take(&mut s.items);
        s.prev_modal = std::mem::take(&mut s.modal);
        s.in_modal = 0;
        let mouse = mouse_position();
        if mouse != s.last_mouse || is_mouse_button_pressed(MouseButton::Left) {
            s.visible = false;
        }
        s.last_mouse = mouse;
        let list = if !s.prev_modal.is_empty() {
            s.prev_modal.clone()
        } else if matches!(nav, Nav::All | Nav::AllButTab) {
            s.prev_items.clone()
        } else {
            Vec::new()
        };
        if nav == Nav::Off || list.is_empty() {
            return;
        }
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let here = locate(&list, s.current).map(|i| list[i]);
        let mut moved = None;
        if nav != Nav::AllButTab && is_key_pressed(KeyCode::Tab) {
            engine::consume_key(KeyCode::Tab);
            let n = list.len();
            let i = locate(&list, s.current);
            let next = match (i, shift) {
                (None, false) => 0,
                (None, true) => n - 1,
                (Some(i), false) => (i + 1) % n,
                (Some(i), true) => (i + n - 1) % n,
            };
            moved = Some(list[next]);
        }
        for (key, dir) in [
            (KeyCode::Left, vec2(-1.0, 0.0)),
            (KeyCode::Right, vec2(1.0, 0.0)),
            (KeyCode::Up, vec2(0.0, -1.0)),
            (KeyCode::Down, vec2(0.0, 1.0)),
        ] {
            if is_key_pressed(key) {
                engine::consume_key(key);
                moved = Some(match here {
                    Some(from) if s.visible => step(&list, from, dir).unwrap_or(from),
                    _ => here.unwrap_or(list[0]),
                });
            }
        }
        if let Some(r) = moved {
            s.current = Some(r);
            s.visible = true;
            return;
        }
        if s.visible && is_key_pressed(KeyCode::Enter) {
            if let Some(r) = here {
                engine::consume_key(KeyCode::Enter);
                let p = virtual_to_window(center(r));
                engine::click_at(p.x, p.y);
            }
        }
    });
}

/// The focus ring, drawn after everything else.
pub(crate) fn draw_ring() {
    STATE.with(|s| {
        let s = s.borrow();
        if !s.visible {
            return;
        }
        let list = if !s.modal.is_empty() {
            &s.modal
        } else {
            &s.items
        };
        if let Some(i) = locate(list, s.current) {
            let r = list[i];
            draw_round_frame(
                r.x - 4.0,
                r.y - 4.0,
                r.w + 8.0,
                r.h + 8.0,
                14.0,
                4.0,
                Color::new(0.0, 0.0, 0.0, 0.0),
                ACCENT(),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_pick_the_nearest_control_ahead() {
        let a = Rect::new(0.0, 0.0, 40.0, 40.0);
        let b = Rect::new(60.0, 0.0, 40.0, 40.0);
        let c = Rect::new(0.0, 60.0, 40.0, 40.0);
        let d = Rect::new(200.0, 10.0, 40.0, 40.0);
        let list = [a, b, c, d];
        assert_eq!(step(&list, a, vec2(1.0, 0.0)), Some(b));
        assert_eq!(step(&list, a, vec2(0.0, 1.0)), Some(c));
        assert_eq!(step(&list, b, vec2(1.0, 0.0)), Some(d));
        assert_eq!(step(&list, a, vec2(-1.0, 0.0)), None);
        // a control that moved a little is still the focused one
        assert_eq!(
            locate(&list, Some(Rect::new(62.0, 3.0, 40.0, 40.0))),
            Some(1)
        );
    }
}
