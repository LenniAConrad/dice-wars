//! The in-game screen: layout, input and the per-frame update.

use dicegame::engine::prelude::*;

use crate::board::{cell_pos, COLS, HEX, ROWS};
use crate::game::Game;
use crate::sound::Snd;
use crate::ui::{mouse_virtual, r_icon, WIN_H, WIN_W};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Confirm {
    NewMap,
    Menu,
}

pub(crate) fn r_endturn() -> Rect {
    Rect::new(WIN_W - 216.0, 640.0, 172.0, 44.0)
}

pub(crate) fn r_btn_menu() -> Rect {
    Rect::new(WIN_W - 216.0, 694.0, 78.0, 30.0)
}

pub(crate) fn r_btn_new() -> Rect {
    Rect::new(WIN_W - 122.0, 694.0, 78.0, 30.0)
}

// The game-over card and its buttons share one layout; `rise` is the entry
// animation offset, so drawing and hit-testing can never drift apart.
const OVER_W: f32 = 560.0;

const OVER_H: f32 = 214.0;

pub(crate) fn r_over_card(rise: f32) -> Rect {
    Rect::new(
        (WIN_W - OVER_W) * 0.5,
        (WIN_H - OVER_H) * 0.5 - 40.0 + rise,
        OVER_W,
        OVER_H,
    )
}

pub(crate) fn r_over_button(i: usize, rise: f32) -> Rect {
    let card = r_over_card(rise);
    let (w, gap) = (120.0, 10.0);
    let x0 = card.x + (card.w - (w * 4.0 + gap * 3.0)) * 0.5;
    Rect::new(x0 + i as f32 * (w + gap), card.y + 128.0, w, 46.0)
}

pub(crate) fn r_over_replay() -> Rect {
    r_over_button(0, 0.0)
}

pub(crate) fn r_over_gif() -> Rect {
    r_over_button(1, 0.0)
}

pub(crate) fn r_over_new() -> Rect {
    r_over_button(2, 0.0)
}

pub(crate) fn r_over_menu() -> Rect {
    r_over_button(3, 0.0)
}

pub(crate) fn r_confirm_yes() -> Rect {
    Rect::new(WIN_W * 0.5 - 158.0, 398.0, 150.0, 44.0)
}

pub(crate) fn r_confirm_no() -> Rect {
    Rect::new(WIN_W * 0.5 + 8.0, 398.0, 150.0, 44.0)
}

pub(crate) fn pick_territory(game: &Game, p: Vec2) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let dist = cell_pos(i).distance(p);
        if dist < HEX && best.map_or(true, |b| dist < b.0) {
            best = Some((dist, t as usize));
        }
    }
    best.map(|b| b.1)
}

fn keyboard_neighbor(game: &Game, from: usize, direction: Vec2) -> usize {
    let origin = game.terrs[from].anchor;
    game.terrs
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let delta = t.anchor - origin;
            let forward = delta.x * direction.x + delta.y * direction.y;
            let sideways = (delta.x * direction.y - delta.y * direction.x).abs();
            (forward > 1.0).then_some((i, delta.length() + sideways * 2.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .unwrap_or(from)
}

pub(crate) fn handle_input(game: &mut Game, mut net_out: Option<&mut Vec<String>>) {
    let initial = || {
        game.terrs
            .iter()
            .position(|t| t.owner == game.current && t.dice > 1)
            .unwrap_or(0)
    };
    for (key, direction) in [
        (KeyCode::Left, vec2(-1.0, 0.0)),
        (KeyCode::Right, vec2(1.0, 0.0)),
        (KeyCode::Up, vec2(0.0, -1.0)),
        (KeyCode::Down, vec2(0.0, 1.0)),
    ] {
        if is_key_pressed(key) {
            game.keyboard_hover = Some(match game.keyboard_hover {
                Some(from) => keyboard_neighbor(game, from, direction),
                None => game.selected.unwrap_or_else(initial),
            });
            return;
        }
    }
    let keyboard_activate = is_key_pressed(KeyCode::Enter);
    if keyboard_activate && game.keyboard_hover.is_none() {
        game.keyboard_hover = Some(initial());
    }

    if is_key_pressed(KeyCode::Space) {
        if let Some(out) = net_out.as_deref_mut() {
            out.push("END".to_string());
        } else {
            game.end_turn();
        }
        return;
    }
    if !is_mouse_button_pressed(MouseButton::Left) && !keyboard_activate {
        return;
    }
    if !keyboard_activate {
        game.keyboard_hover = None;
    }
    let mp = if keyboard_activate {
        game.terrs[game.keyboard_hover.unwrap()].anchor
    } else {
        mouse_virtual()
    };
    if r_endturn().contains(mp) {
        game.snd.push(Snd::Click);
        if let Some(out) = net_out.as_deref_mut() {
            out.push("END".to_string());
        } else {
            game.end_turn();
        }
        return;
    }
    if r_btn_menu().contains(mp) || r_btn_new().contains(mp) {
        return; // handled in the main loop (also works during AI turns)
    }
    if (0..6).any(|i| r_icon(i).contains(mp)) {
        return; // toolbar clicks are handled in the main loop
    }
    let Some(t) = (if keyboard_activate {
        game.keyboard_hover
    } else {
        pick_territory(game, mp)
    }) else {
        game.selected = None;
        return;
    };
    let owner = game.terrs[t].owner;
    let dice = game.terrs[t].dice;
    match game.selected {
        Some(sel) if sel == t => {
            game.selected = None;
            game.snd.push(Snd::Click);
        }
        Some(sel) => {
            if owner != game.current {
                if !game.can_target(game.current, owner) {
                    game.set_hint("That land belongs to a teammate — friendly fire is off.");
                } else if game.terrs[sel].neighbors.contains(&t) {
                    if let Some(out) = net_out.as_deref_mut() {
                        out.push(format!("ATTACK {} {}", sel, t));
                        game.selected = None;
                    } else {
                        game.begin_attack(sel, t);
                    }
                } else {
                    game.set_hint("Too far away — you can only attack direct neighbors.");
                }
            } else if dice > 1 {
                game.selected = Some(t);
                game.snd.push(Snd::Select);
            } else {
                game.set_hint("That land has only 1 die — it cannot attack.");
            }
        }
        None => {
            if owner != game.current {
                if game.teams.on && game.same_team(game.current, owner) {
                    game.set_hint(
                        "That land belongs to a teammate — select one of your own first.",
                    );
                } else {
                    game.set_hint("That is an enemy land — select one of your own first.");
                }
            } else if dice > 1 {
                game.selected = Some(t);
                game.snd.push(Snd::Select);
            } else {
                game.set_hint("That land has only 1 die — it cannot attack.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::game::TeamCfg;
    use crate::testutil::mk_game;

    use super::*;

    #[test]
    fn keyboard_selection_follows_visible_directions() {
        let mut g = mk_game(&[0, 0, 1, 1], &[(0, 1), (1, 2)], 2, TeamCfg::default());
        for (t, anchor) in g.terrs.iter_mut().zip([
            vec2(100.0, 100.0),
            vec2(140.0, 100.0),
            vec2(100.0, 150.0),
            vec2(80.0, 110.0),
        ]) {
            t.anchor = anchor;
        }
        assert_eq!(keyboard_neighbor(&g, 0, vec2(1.0, 0.0)), 1);
        assert_eq!(keyboard_neighbor(&g, 0, vec2(0.0, 1.0)), 2);
        assert_eq!(keyboard_neighbor(&g, 0, vec2(-1.0, 0.0)), 3);
        assert_eq!(keyboard_neighbor(&g, 0, vec2(0.0, -1.0)), 0);
    }
}
