//! Drawing the board, side panel, banners and battle overlay.

use std::sync::atomic::Ordering;

use dicegame::engine::{self, prelude::*};

use crate::ai::win_chance_cached;
use crate::board::{cell_pos, neighbor_of, COLS, HEX, ROWS};
use crate::dice::{draw_face_die, draw_stack, draw_symbol};
use crate::game::{
    team_letter, Battle, Game, TerrFx, Territory, HUMAN, MAX_PLAYERS, MAX_TEAMS, T_SHAKE,
};
use crate::play::{
    confirm_choices, pick_territory, r_endturn, r_over_button, r_over_card, Confirm, ConfirmChoice,
    CONFIRM_W, CONFIRM_Y,
};
use crate::settings::Settings;
use crate::summary::{draw_chart, draw_stats, Summary};
use crate::theme::{
    accent_ink, color_map, colorblind_mode, dark_mode, darken, lighten, map_fill, mix, on_color,
    palette, theme, theme_index, with_alpha, ACCENT, BG, BORDER, CARD_EDGE, INK, INK_ON_FILL,
    MAP_EDGE, REDUCED_MOTION, SEA, SRF,
};
use crate::ui::{
    control_fill_t, draw_game_menu_button, draw_icon_bar, draw_round_frame, draw_round_rect,
    ease_out, hover_t, mouse_virtual, primary_button, soft_shadow, Ui, WIN_H, WIN_W,
};

fn hex_points(p: Vec2, s: f32) -> [Vec2; 6] {
    std::array::from_fn(|k| {
        let a = (60.0 * k as f32 + 30.0).to_radians();
        p + s * vec2(a.cos(), a.sin())
    })
}

pub(crate) fn fill_hex(p: Vec2, s: f32, col: Color) {
    let pts = hex_points(p, s);
    // tiles meet same-colored neighbors: hard edges, so no seams between
    // cells; coasts and borders get smooth outlines drawn on top
    draw_convex(&pts, col, false);
}

fn board_hover(game: &Game) -> Option<usize> {
    if game.current == game.my_seat && game.over.is_none() && game.battle.is_none() {
        game.keyboard_hover
            .or_else(|| pick_territory(game, mouse_virtual()))
    } else {
        None
    }
}

pub(crate) fn draw_board(game: &Game, ui: &Ui, chances: bool) {
    use std::hash::{Hash, Hasher};
    // What changes the whole picture...
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        game.seed,
        theme_index(),
        colorblind_mode(),
        REDUCED_MOTION.load(Ordering::Relaxed),
    )
        .hash(&mut hash);
    for p in 0..MAX_PLAYERS {
        color_map(p).hash(&mut hash);
    }
    // ...and what changes one land: only its region is repainted, so
    // captures, fades, hover and selection stay cheap at any window size.
    let hover = board_hover(game);
    let parts: Vec<(u64, Rect)> = (0..game.terrs.len())
        .map(|t| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            let terr = &game.terrs[t];
            (terr.owner, terr.dice).hash(&mut h);
            land_fill(game, t, hover).bytes().hash(&mut h);
            stack_is_moving(game, t).hash(&mut h);
            (game.keyboard_hover == Some(t)).hash(&mut h);
            game.battle
                .as_ref()
                .is_some_and(|b| b.a == t || b.d == t)
                .hash(&mut h);
            chance_hint(game, t, chances).map(f32::to_bits).hash(&mut h);
            (h.finish(), land_region(game, t))
        })
        .collect();
    engine::cached_layer_parts(0, hash.finish(), &parts, || {
        draw_board_live(game, ui, chances)
    });
    draw_board_effects(game, ui);
}

// A land's fill: owner color (cross-fading after a capture), lifted when
// selected or hovered as an option, white when it is not a legal target.
fn land_fill(game: &Game, t: usize, hover: Option<usize>) -> Color {
    let terr = &game.terrs[t];
    let mut col = map_fill(terr.owner);
    if let Some(f) = capture_fade(game, t) {
        // Captured land sweeps from the loser's color into the winner's.
        col = mix(map_fill(game.fx[t].from), col, ease_out(1.0 - f));
    }
    let selectable = terr.owner == game.current && terr.dice > 1;
    let attackable = game.selected.is_some_and(|s| {
        game.can_target(game.current, terr.owner) && game.terrs[s].neighbors.contains(&t)
    });
    if game.selected == Some(t) {
        lighten(col, 0.38)
    } else if game.selected.is_some() && !attackable {
        // Selection is a map treatment, independent of the interface
        // theme. Dice and owner badges still identify these territories.
        WHITE
    } else if hover == Some(t) && (attackable || selectable) {
        lighten(col, 0.2)
    } else {
        col
    }
}

// The win chance shown on attack option `t` of the selected land, if any.
fn chance_hint(game: &Game, t: usize, chances: bool) -> Option<f32> {
    let sel = game.selected?;
    (chances
        && game.battle.is_none()
        && game.over.is_none()
        && game.current == game.my_seat
        && game.terrs[sel].neighbors.contains(&t)
        && game.can_target(game.current, game.terrs[t].owner))
    .then(|| win_chance_cached(game.terrs[sel].dice, game.terrs[t].dice))
}

// Everything one land can paint on the board layer: its hexes and borders,
// its dice stack (up to eight dice and a bounce) and its count badge.
fn land_region(game: &Game, t: usize) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for i in 0..COLS * ROWS {
        if game.cell_terr[i] == t as i32 {
            let p = cell_pos(i);
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
    }
    let a = game.terrs[t].anchor;
    let pad = HEX + 4.0;
    x0 = (x0 - pad).min(a.x - 34.0);
    x1 = (x1 + pad).max(a.x + 34.0);
    y0 = (y0 - pad).min(a.y - 100.0);
    y1 = (y1 + pad).max(a.y + 50.0); // badge and win-chance hint
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

fn draw_board_live(game: &Game, ui: &Ui, chances: bool) {
    // The whole window is the play surface, so letterbox bars never show a seam.
    clear_background(SRF());

    let hover = board_hover(game);

    // empty water cells become a quiet dot grid, so holes read as sea
    for i in 0..COLS * ROWS {
        if game.cell_terr[i] < 0 {
            let p = cell_pos(i);
            // smooth edges: the outer zigzag meets the page. Neighbors
            // overlap by more than the half-pixel edge ramp, so water tiles
            // still join without seams.
            draw_convex(&hex_points(p, HEX + 0.6), theme().water, true);
            draw_circle(p.x, p.y, 2.2, SEA());
        }
    }

    // tile fills
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let col = land_fill(game, t as usize, hover);
        fill_hex(cell_pos(i), HEX + 0.6, col);
    }

    // Territory borders: shared edges between lands are a soft slate hairline;
    // coastlines and the keyboard focus keep the strong outline. Coasts draw in
    // a second pass so they always win at the joints.
    let soft_edge = mix(MAP_EDGE, WHITE, 0.38);
    // coasts lead, but lighter than ink so the lands and dice carry the map
    let coast_edge = mix(MAP_EDGE, WHITE, 0.14);
    for coast_pass in [false, true] {
        for i in 0..COLS * ROWS {
            let t = game.cell_terr[i];
            if t < 0 {
                continue;
            }
            let (c, r) = (i % COLS, i / COLS);
            let pos = cell_pos(i);
            let focused = game.keyboard_hover == Some(t as usize);
            for d in 0..6 {
                let n = neighbor_of(c, r, d);
                if n.is_some_and(|n| game.cell_terr[n] == t) {
                    continue;
                }
                let coast = focused || n.is_none_or(|n| game.cell_terr[n] < 0);
                if coast != coast_pass {
                    continue;
                }
                let a0 = (d as f32 * 60.0 - 30.0).to_radians();
                let a1 = (d as f32 * 60.0 + 30.0).to_radians();
                let p0 = pos + HEX * vec2(a0.cos(), a0.sin());
                let p1 = pos + HEX * vec2(a1.cos(), a1.sin());
                let (width, col) = if focused {
                    (3.4, INK_ON_FILL)
                } else if coast {
                    (1.9, coast_edge)
                } else {
                    (1.6, soft_edge)
                };
                draw_line(p0.x, p0.y, p1.x, p1.y, width, col);
                // Round caps keep thick joints clean. Hairlines meet without
                // visible gaps, so they skip the (costly) cap discs.
                if coast {
                    draw_circle(p0.x, p0.y, width * 0.5, col);
                    draw_circle(p1.x, p1.y, width * 0.5, col);
                }
            }
        }
    }

    // During a battle both lands carry their owner's outline, so the fight is
    // readable on the map itself and not only in the status panel.
    if let Some(b) = &game.battle {
        for t in [b.a, b.d] {
            let col = palette(game.terrs[t].owner).dark;
            for i in 0..COLS * ROWS {
                if game.cell_terr[i] != t as i32 {
                    continue;
                }
                let pos = cell_pos(i);
                for d in 0..6 {
                    if neighbor_of(i % COLS, i / COLS, d)
                        .is_some_and(|n| game.cell_terr[n] == t as i32)
                    {
                        continue;
                    }
                    let a0 = (d as f32 * 60.0 - 30.0).to_radians();
                    let a1 = (d as f32 * 60.0 + 30.0).to_radians();
                    let p0 = pos + (HEX - 1.6) * vec2(a0.cos(), a0.sin());
                    let p1 = pos + (HEX - 1.6) * vec2(a1.cos(), a1.sin());
                    draw_line(p0.x, p0.y, p1.x, p1.y, 3.6, col);
                    draw_circle(p0.x, p0.y, 1.8, col);
                    draw_circle(p1.x, p1.y, 1.8, col);
                }
            }
        }
    }

    // dice stacks, back to front
    let mut order: Vec<usize> = (0..game.terrs.len()).collect();
    order.sort_by(|&a, &b| game.terrs[a].anchor.y.total_cmp(&game.terrs[b].anchor.y));
    for t in order {
        if !stack_is_moving(game, t) {
            draw_stack(&game.terrs[t], TerrFx::default(), 0.0);
        }
    }

    // readable dice-count badge at each stack's base; in colorblind mode the
    // badge takes the owner's symbol shape so color is never the only cue
    for (idx, t) in game.terrs.iter().enumerate() {
        if !stack_is_moving(game, idx) {
            draw_stack_badge(t, ui);
        }
    }

    // subtle win-chance hint on every attack option of the selected land
    for t in 0..game.terrs.len() {
        let Some(c) = chance_hint(game, t, chances) else {
            continue;
        };
        let txt = format!("{:.0}%", c * 100.0);
        let p = game.terrs[t].anchor + vec2(0.0, 32.0);
        let tw = ui.width(&txt, 12.0);
        let col = if c >= 0.6 {
            accent_ink(4) // blue: good odds (colorblind-safe pair)
        } else if c >= 0.4 {
            INK()
        } else {
            accent_ink(5) // orange: risky
        };
        draw_round_rect(
            p.x - tw * 0.5 - 6.0,
            p.y - 12.0,
            tw + 12.0,
            16.0,
            8.0,
            SRF(),
        );
        ui.text_centered(&txt, p.x, p.y + 0.5, 12.0, col);
    }
}

fn draw_stack_badge(t: &Territory, ui: &Ui) {
    let pal = palette(t.owner);
    let bx = t.anchor.x + 16.0;
    let by = t.anchor.y + 13.0;
    if colorblind_mode() {
        draw_symbol(bx, by, 13.0, t.owner, SRF());
        draw_symbol(bx, by, 11.0, t.owner, pal.dark);
        ui.text_centered(&t.dice.to_string(), bx, by + 4.0, 11.0, on_color(pal.dark));
    } else {
        // small and quiet: the tower is the main signal, the badge confirms it
        draw_circle(bx, by, 9.0, SRF());
        draw_circle(bx, by, 7.8, pal.dark);
        ui.text_centered(&t.dice.to_string(), bx, by + 4.2, 12.0, on_color(pal.dark));
    }
}

// Remaining capture cross-fade (1 → 0), or None when the land shows its owner.
fn capture_fade(game: &Game, t: usize) -> Option<f32> {
    let fx = &game.fx[t];
    (fx.flash > 0.0 && fx.from != game.terrs[t].owner && !REDUCED_MOTION.load(Ordering::Relaxed))
        .then_some(fx.flash)
}

fn stack_is_moving(game: &Game, t: usize) -> bool {
    !REDUCED_MOTION.load(Ordering::Relaxed) && (game.fx[t].shake > 0.0 || game.fx[t].bounce > 0.0)
}

fn draw_board_effects(game: &Game, ui: &Ui) {
    // attack arrow during a battle
    if let Some(b) = &game.battle {
        let from = game.terrs[b.a].anchor + vec2(0.0, -18.0);
        let to = game.terrs[b.d].anchor + vec2(0.0, -18.0);
        let dir = (to - from).normalize_or_zero();
        let start = from + dir * 26.0;
        let end = to - dir * 30.0;
        let col = palette(game.terrs[b.a].owner).dark;
        let perp = vec2(-dir.y, dir.x);
        // A light halo keeps the arrow legible over stacks and both themes.
        let halo = with_alpha(WHITE, 0.9);
        draw_line(start.x, start.y, end.x, end.y, 8.0, halo);
        draw_circle(start.x, start.y, 4.0, halo);
        draw_triangle(
            end + dir * 15.0,
            end + perp * 10.0 - dir * 2.0,
            end - perp * 10.0 - dir * 2.0,
            halo,
        );
        draw_line(start.x, start.y, end.x, end.y, 4.5, col);
        draw_circle(start.x, start.y, 2.25, col);
        draw_triangle(end + dir * 11.0, end + perp * 7.0, end - perp * 7.0, col);
    }

    // with a land selected, point out every attack option
    if game.battle.is_none() && game.over.is_none() {
        if let Some(sel) = game.selected {
            let alpha = if REDUCED_MOTION.load(Ordering::Relaxed) {
                0.65
            } else {
                0.5 + 0.2 * (game.time * 5.0).sin()
            };
            let col = with_alpha(palette(game.current).dark, alpha);
            for &d in &game.terrs[sel].neighbors {
                if !game.can_target(game.current, game.terrs[d].owner) {
                    continue;
                }
                let from = game.terrs[sel].anchor;
                let to = game.terrs[d].anchor;
                let dir = (to - from).normalize_or_zero();
                let start = from + dir * 32.0;
                let end = to - dir * 36.0;
                draw_line(start.x, start.y, end.x, end.y, 2.0, col);
                let perp = vec2(-dir.y, dir.x);
                draw_triangle(end + dir * 8.0, end + perp * 5.0, end - perp * 5.0, col);
            }
        }
    }

    // A fading outline conveys captures without repainting every map tile.
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 || game.fx[t as usize].flash <= 0.0 {
            continue;
        }
        let pos = cell_pos(i);
        for d in 0..6 {
            if neighbor_of(i % COLS, i / COLS, d).is_some_and(|n| game.cell_terr[n] == t) {
                continue;
            }
            let a = (d as f32 * 60.0 - 30.0).to_radians();
            let b = (d as f32 * 60.0 + 30.0).to_radians();
            let p0 = pos + HEX * vec2(a.cos(), a.sin());
            let p1 = pos + HEX * vec2(b.cos(), b.sin());
            draw_line(
                p0.x,
                p0.y,
                p1.x,
                p1.y,
                2.0,
                with_alpha(WHITE, game.fx[t as usize].flash * 0.85),
            );
        }
    }
    let mut moving: Vec<usize> = (0..game.terrs.len())
        .filter(|&t| stack_is_moving(game, t))
        .collect();
    moving.sort_by(|&a, &b| game.terrs[a].anchor.y.total_cmp(&game.terrs[b].anchor.y));
    for &t in &moving {
        draw_stack(&game.terrs[t], game.fx[t], game.time);
    }
    for t in moving {
        draw_stack_badge(&game.terrs[t], ui);
    }
    // floating roll numbers / reinforcement popups
    for f in &game.floats {
        let p = (f.t / f.life).clamp(0.0, 1.0);
        let alpha = (1.0 - p * p).clamp(0.0, 1.0);
        let y = f.pos.y
            - if REDUCED_MOTION.load(Ordering::Relaxed) {
                0.0
            } else {
                46.0 * ease_out(p)
            };
        let w = ui.width(&f.text, f.size);
        ui.text_outlined(
            &f.text,
            f.pos.x - w * 0.5,
            y,
            f.size,
            with_alpha(f.col, alpha),
            with_alpha(SRF(), alpha),
        );
    }
}

fn draw_panel(game: &Game, ui: &Ui, settings: &Settings) {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        (game.seed, game.players),
        game.current,
        game.my_seat,
        game.humans,
        game.over.is_some(),
        game.selected,
        game.battle.is_some(),
        game.hint_t > 0.0,
        game.turn_timer.ceil() as u32,
        (
            game.teams.on,
            settings.chances,
            theme_index(),
            colorblind_mode(),
        ),
    )
        .hash(&mut hash);
    game.team.hash(&mut hash);
    game.reserve.hash(&mut hash);
    game.names.hash(&mut hash);
    game.hint.hash(&mut hash);
    game.log.last().hash(&mut hash);
    for t in &game.terrs {
        (t.owner, t.dice).hash(&mut hash);
    }
    let m = mouse_virtual();
    if game.selected.is_some() && game.battle.is_none() {
        pick_territory(game, m).hash(&mut hash);
    }
    let active = game.current == game.my_seat && game.over.is_none() && game.battle.is_none();
    ((hover_t(r_endturn(), active && r_endturn().contains(m)) * 16.0) as u8).hash(&mut hash);
    is_mouse_button_down(MouseButton::Left).hash(&mut hash);
    for p in 0..MAX_PLAYERS {
        color_map(p).hash(&mut hash);
    }
    engine::cached_layer(2, hash.finish(), || draw_panel_live(game, ui, settings));
}

fn draw_panel_live(game: &Game, ui: &Ui, settings: &Settings) {
    let chances = settings.chances;
    let px = 16.0;
    let py = 632.0;
    let pw = WIN_W - 32.0;
    let ph = WIN_H - py - 14.0;

    let _ = (px, pw, ph);
    draw_panel_bar();

    let cur = game.current;
    let mp = mouse_virtual();

    // ---- who is playing, and which phase
    let turn_text = if game.over.is_some() {
        "Game over".to_string()
    } else if cur == game.my_seat && cur < game.humans {
        "Your turn".to_string()
    } else {
        format!("{}'s turn", game.name_of(cur))
    };
    let tw = ui.width(&turn_text, 21.0).min(492.0);
    if game.over.is_some() {
        draw_round_frame(
            40.0,
            py + 12.0,
            tw + 40.0,
            36.0,
            12.0,
            2.0,
            SRF(),
            CARD_EDGE(),
        );
        ui.text_fitted(&turn_text, 60.0, py + 37.0, 21.0, tw, INK());
    } else {
        draw_round_frame(
            40.0,
            py + 12.0,
            tw + 60.0,
            36.0,
            12.0,
            2.5,
            palette(cur).fill,
            BORDER(),
        );
        draw_symbol(62.0, py + 30.0, 8.5, cur, palette(cur).dark);
        ui.text_fitted(&turn_text, 80.0, py + 37.0, 21.0, tw, INK_ON_FILL);
    }
    // multiplayer idle countdown: the turn auto-passes at zero
    if game.humans > 1 && game.is_human(cur) && game.over.is_none() {
        let secs = game.turn_timer.ceil().max(0.0) as u32;
        let txt = format!("{}:{:02}", secs / 60, secs % 60);
        let col = if secs <= 10 {
            palette(2).dark
        } else {
            with_alpha(INK(), 0.72)
        };
        ui.text(&txt, 40.0 + tw + 74.0, py + 37.0, 20.0, col);
    }

    // ---- current step instruction, invalid-action hint, or attack preview
    let hover = pick_territory(game, mp);
    // nothing during battles (the rolls take the lane) or after the game
    // (the result card says it all)
    let instruction = if game.battle.is_some() || game.over.is_some() {
        String::new()
    } else if cur >= game.humans {
        if game.humans == 1 {
            "Bots are playing. Hold SPACE to fast-forward.".to_string()
        } else {
            "Bots are playing...".to_string()
        }
    } else if cur != game.my_seat {
        format!("Waiting for {}...", game.name_of(cur))
    } else if r_endturn().contains(mp) {
        let region = game.largest_region(game.my_seat);
        let stored = game.reserve[game.my_seat];
        if stored > 0 {
            format!("End your turn: +{region} dice for your largest region, plus {stored} stored.")
        } else {
            format!("End your turn: +{region} dice, one per land in your largest region.")
        }
    } else if let Some(sel) = game.selected {
        let target = hover.filter(|&h| {
            game.can_target(cur, game.terrs[h].owner) && game.terrs[sel].neighbors.contains(&h)
        });
        if let Some(tgt) = target {
            if chances {
                let c = win_chance_cached(game.terrs[sel].dice, game.terrs[tgt].dice);
                format!(
                    "You: {} dice  vs  {}: {} dice — {:.0}% win chance. Click to attack!",
                    game.terrs[sel].dice,
                    game.name_of(game.terrs[tgt].owner),
                    game.terrs[tgt].dice,
                    c * 100.0
                )
            } else {
                format!(
                    "You: {} dice  vs  {}: {} dice — click to attack!",
                    game.terrs[sel].dice,
                    game.name_of(game.terrs[tgt].owner),
                    game.terrs[tgt].dice
                )
            }
        } else {
            "Now click a neighboring enemy land. ESC cancels.".to_string()
        }
    } else {
        "Click one of your lands with 2 or more dice to attack from it.".to_string()
    };
    if game.hint_t > 0.0 && game.battle.is_none() && game.over.is_none() {
        ui.body_fitted(&game.hint, 40.0, py + 76.0, 17.0, 648.0, accent_ink(2));
    // rose = warning
    } else {
        if game.battle.is_none() {
            ui.body_fitted(&instruction, 40.0, py + 76.0, 17.0, 648.0, INK());
        }
    }

    // ---- last event, small
    if let Some(last) = game
        .log
        .last()
        .filter(|_| game.battle.is_none() && game.over.is_none())
    {
        ui.body_fitted(last, 40.0, py + 98.0, 13.5, 648.0, with_alpha(INK(), 0.72));
    }

    // ---- END TURN (right side); the pause menu sits top left
    if game.over.is_none() {
        let active = cur == game.my_seat && game.battle.is_none();
        let b = r_endturn();
        let hovered = active && b.contains(mp);
        let t = hover_t(b, hovered);
        let pressed = hovered && is_mouse_button_down(MouseButton::Left);
        let off = if pressed { 1.0 } else { 0.0 };
        let base = if active {
            palette(HUMAN).mid
        } else {
            mix(SRF(), BG(), 0.5)
        };
        let fill = control_fill_t(base, t);
        let edge = if active {
            mix(darken(base, 0.72), ACCENT(), t)
        } else {
            CARD_EDGE()
        };
        draw_round_frame(b.x, b.y + off, b.w, b.h, 22.0, 2.0, fill, edge);
        let label = if active {
            format!(
                "END TURN +{}",
                game.largest_region(game.my_seat) + game.reserve[game.my_seat]
            )
        } else {
            "END TURN".to_string()
        };
        ui.text_centered(
            &label,
            b.x + b.w * 0.5,
            b.y + off + 33.0,
            19.0,
            if active {
                on_color(fill)
            } else {
                with_alpha(INK(), 0.4)
            },
        );
        if active {
            ui.body_centered(
                "or press SPACE",
                b.x + b.w * 0.5,
                b.y + b.h + 18.0,
                11.0,
                with_alpha(INK(), 0.55),
            );
        }
    }

    draw_player_cards(game, ui, player_cards_y(game), 0.0);
}

fn player_cards_y(game: &Game) -> f32 {
    632.0 + if game.teams.on { 123.0 } else { 110.0 }
}

// Player cards are grouped spatially in team games. The shared neutral frame
// and explicit team heading carry alliance information without inventing a
// second set of team hues.
fn draw_player_cards(game: &Game, ui: &Ui, cy: f32, right_margin: f32) {
    let chip_w = 88.0;
    let chip_h = 48.0;
    let card_gap = if game.teams.on { 6.0 } else { 8.0 };
    let group_gap = 18.0;
    let groups: Vec<(Option<usize>, Vec<usize>)> = if game.teams.on {
        (0..MAX_TEAMS)
            .filter_map(|team| {
                let players: Vec<usize> = (0..game.players)
                    .filter(|&p| game.team_of(p) % MAX_TEAMS == team)
                    .collect();
                (!players.is_empty()).then_some((Some(team), players))
            })
            .collect()
    } else {
        vec![(None, (0..game.players).collect())]
    };
    let group_width = |members: &[usize]| {
        members.len() as f32 * chip_w + members.len().saturating_sub(1) as f32 * card_gap
    };
    let total: f32 = groups
        .iter()
        .map(|(_, members)| group_width(members))
        .sum::<f32>()
        + groups.len().saturating_sub(1) as f32 * group_gap;
    let center_nudge = 0.0;
    let mut gx = ((WIN_W - right_margin - total) * 0.5 + center_nudge).max(20.0);

    for (team, members) in groups {
        let gw = group_width(&members);
        if let Some(team) = team {
            draw_round_frame(
                gx - 5.0,
                cy - 13.0,
                gw + 10.0,
                chip_h + 18.0,
                16.0,
                1.5,
                SRF(),
                with_alpha(BORDER(), 0.62),
            );
            let label = format!("TEAM {}", team_letter(team));
            let lw = ui.width(&label, 10.0) + 12.0;
            draw_round_rect(gx + 8.0, cy - 19.0, lw, 14.0, 7.0, SRF());
            ui.text_centered(&label, gx + 8.0 + lw * 0.5, cy - 8.0, 10.0, INK());
        }

        for (slot, p) in members.into_iter().enumerate() {
            let cx = gx + slot as f32 * (chip_w + card_gap);
            let alive = game.count(p) > 0;
            let is_cur = p == game.current && game.over.is_none();
            if is_cur {
                soft_shadow(cx, cy, chip_w, chip_h, 14.0, 1.4);
                draw_round_frame(
                    cx,
                    cy,
                    chip_w,
                    chip_h,
                    14.0,
                    3.0,
                    palette(p).fill,
                    darken(palette(p).fill, 0.7),
                );
            } else {
                let mine = p == game.my_seat && game.recording;
                let edge = if mine { ACCENT() } else { CARD_EDGE() };
                draw_round_frame(cx, cy, chip_w, chip_h, 14.0, 2.0, SRF(), edge);
            }
            let a = if alive { 1.0 } else { 0.25 };
            draw_symbol(
                cx + 20.0,
                cy + 23.0,
                8.5,
                p,
                with_alpha(
                    if is_cur {
                        palette(p).dark
                    } else {
                        accent_ink(p)
                    },
                    a,
                ),
            );
            let mine = p == game.my_seat && game.recording;
            if alive {
                let ink = if is_cur { INK_ON_FILL } else { INK() };
                ui.text(&game.count(p).to_string(), cx + 36.0, cy + 27.0, 21.0, ink);
                ui.text(
                    if mine { "you" } else { "lands" },
                    cx + 37.0,
                    cy + 40.0,
                    10.0,
                    with_alpha(ink, if mine { 0.95 } else { 0.6 }),
                );
                // share of the map, a quiet progress line along the card base
                let share = game.count(p) as f32 / game.terrs.len().max(1) as f32;
                let (bx, bw) = (cx + 10.0, chip_w - 20.0);
                draw_round_rect(bx, cy + chip_h - 6.0, bw, 3.0, 1.5, with_alpha(ink, 0.12));
                draw_round_rect(
                    bx,
                    cy + chip_h - 6.0,
                    (bw * share).max(3.0),
                    3.0,
                    1.5,
                    palette(p).dark,
                );
                if game.reserve[p] > 0 {
                    // Keep corner badges inside the shared team frame.
                    let bt = format!("+{}", game.reserve[p]);
                    let bw_ = ui.width(&bt, 11.0) + 10.0;
                    let (bx, by) = if game.teams.on {
                        (cx + chip_w - bw_ - 4.0, cy + chip_h - 12.0)
                    } else {
                        (cx + chip_w - bw_ + 6.0, cy - 8.0)
                    };
                    draw_round_rect(bx, by, bw_, 17.0, 8.5, palette(p).dark);
                    ui.text_centered(
                        &bt,
                        bx + bw_ * 0.5,
                        by + 13.0,
                        11.0,
                        on_color(palette(p).dark),
                    );
                }
            }
        }
        gx += gw + group_gap;
    }
}

// One quiet surface under the board holds the whole status area: turn,
// instructions, rolls, END TURN and the player cards.
fn draw_panel_bar() {
    draw_round_frame(
        16.0,
        626.0,
        WIN_W - 32.0,
        WIN_H - 626.0 - 8.0,
        20.0,
        1.5,
        mix(SRF(), BG(), if dark_mode() { 0.55 } else { 0.7 }),
        CARD_EDGE(),
    );
}

// replays show only the stats: no turn pill, instructions, or buttons
pub(crate) fn draw_panel_replay(game: &Game, ui: &Ui) {
    let py = 632.0;
    draw_panel_bar();
    // below the replay controls, in the lane the battle rolls use
    if let Some(last) = game.log.last().filter(|_| game.battle.is_none()) {
        ui.body_fitted(
            last,
            40.0,
            py + 82.0,
            15.0,
            WIN_W - 80.0,
            with_alpha(INK(), 0.72),
        );
    }
    draw_player_cards(game, ui, player_cards_y(game), 0.0);
}

// Turn changes appear as a short toast that drops in at the top of the board.
fn draw_banner(game: &Game, ui: &Ui) {
    if game.banner_t <= 0.0 || game.over.is_some() {
        return;
    }
    // 1.6 s life: 0.2 s in, hold, 0.35 s out.
    let t_in = ((1.6 - game.banner_t) / 0.2).clamp(0.0, 1.0);
    let t_out = (game.banner_t / 0.35).clamp(0.0, 1.0);
    let alpha = ease_out(t_in) * ease_out(t_out);
    if alpha <= 0.01 {
        return;
    }
    let size = 20.0;
    let text_w = ui.width(&game.banner, size).min(420.0);
    let w = text_w + 72.0;
    let h = 42.0;
    let x = (WIN_W - w) * 0.5 - 60.0;
    let drop = if REDUCED_MOTION.load(Ordering::Relaxed) {
        0.0
    } else {
        -14.0 * (1.0 - ease_out(t_in))
    };
    let y = 14.0 + drop;
    let pal = palette(game.current);
    soft_shadow(x, y, w, h, 21.0, 1.6 * alpha);
    draw_round_frame(
        x,
        y,
        w,
        h,
        21.0,
        2.0,
        with_alpha(pal.fill, alpha),
        with_alpha(darken(pal.fill, 0.78), alpha),
    );
    draw_symbol(
        x + 26.0,
        y + h * 0.5,
        8.0,
        game.current,
        with_alpha(pal.dark, alpha),
    );
    ui.text_fitted(
        &game.banner,
        x + 44.0,
        y + 28.5,
        size,
        text_w,
        with_alpha(INK_ON_FILL, alpha),
    );
}

fn draw_over(game: &Game, ui: &Ui, summary: Option<&Summary>) {
    let Some(msg) = &game.over else { return };
    let a = ease_out(game.over_t);
    // A light scrim keeps the final map readable behind the result card.
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, with_alpha(SRF(), 0.55 * a));
    let rise = if REDUCED_MOTION.load(Ordering::Relaxed) {
        0.0
    } else {
        24.0 * (1.0 - a)
    };
    let card = r_over_card(rise);
    soft_shadow(card.x, card.y, card.w, card.h, 24.0, 2.2 * a);
    draw_round_frame(
        card.x,
        card.y,
        card.w,
        card.h,
        24.0,
        2.0,
        with_alpha(SRF(), a),
        with_alpha(CARD_EDGE(), a),
    );
    // A thin accent bar in the winner's color carries the result at a glance.
    let winner = (0..game.players).find(|&p| game.count(p) > 0).unwrap_or(0);
    draw_round_rect(
        card.x + card.w * 0.5 - 36.0,
        card.y + 18.0,
        72.0,
        6.0,
        3.0,
        with_alpha(palette(winner).mid, a),
    );
    // the whole result always shows: long ones get a smaller size
    let size = ui.fit_size(msg, 34.0, card.w - 64.0);
    ui.text_centered(
        msg,
        card.x + card.w * 0.5,
        card.y + 72.0,
        size,
        with_alpha(INK(), a),
    );
    ui.body_centered(
        "R  new map    ·    ESC  menu",
        WIN_W * 0.5,
        card.y + 100.0,
        13.0,
        with_alpha(INK(), 0.55 * a),
    );
    if let Some(s) = summary {
        let inner = Rect::new(card.x + 32.0, card.y + 120.0, card.w - 64.0, 170.0);
        draw_chart(s, ui, inner, a);
        draw_stats(
            s,
            game,
            ui,
            Rect::new(inner.x, inner.y + inner.h + 12.0, inner.w, 62.0),
            a,
        );
    }
    let m = mouse_virtual();
    for (i, label) in ["REPLAY", "SAVE GIF", "NEW MAP", "MENU"]
        .into_iter()
        .enumerate()
    {
        let r = r_over_button(i, rise);
        let enabled = i != 2 || !game.net_guest;
        let hovered = enabled && r_over_button(i, 0.0).contains(m);
        let primary = i == 2 && enabled;
        let base = if primary { palette(HUMAN).mid } else { SRF() };
        let fill = control_fill_t(base, hover_t(r_over_button(i, 0.0), hovered));
        draw_round_frame(
            r.x,
            r.y,
            r.w,
            r.h,
            14.0,
            2.0,
            with_alpha(fill, a),
            with_alpha(
                if hovered {
                    ACCENT()
                } else if primary {
                    BORDER()
                } else {
                    CARD_EDGE()
                },
                a,
            ),
        );
        let ink = if primary { on_color(fill) } else { INK() };
        ui.text_centered(
            label,
            r.x + r.w * 0.5,
            r.y + 29.0,
            15.0,
            with_alpha(ink, if enabled { a } else { 0.35 * a }),
        );
    }
}

pub(crate) fn draw_confirm(kind: Confirm, ui: &Ui, can_new: bool, resumable: bool) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, with_alpha(SRF(), 0.6));
    let (w, h) = (CONFIRM_W, 214.0);
    let x = (WIN_W - w) * 0.5;
    let y = CONFIRM_Y;
    soft_shadow(x, y, w, h, 22.0, 2.4);
    draw_round_frame(x, y, w, h, 22.0, 2.0, SRF(), CARD_EDGE());
    let (title, body) = match kind {
        Confirm::NewMap => ("Start a new map?", "The current game will be lost."),
        Confirm::Menu if resumable => (
            "Paused",
            "Leave and continue later from the menu, or start a new map.",
        ),
        Confirm::Menu if can_new => ("Paused", "Leaving or starting a new map ends this game."),
        Confirm::Menu => ("Paused", "Leaving ends this game for you."),
    };
    ui.text_centered(title, WIN_W * 0.5, y + 56.0, 28.0, INK());
    ui.body_centered(body, WIN_W * 0.5, y + 90.0, 16.0, with_alpha(INK(), 0.72));
    let m = mouse_virtual();
    for (r, choice) in confirm_choices(kind, can_new) {
        match choice {
            // ending the game is the quiet, rose-outlined choice; keeping
            // the game is the filled default, matching Enter and ESC
            ConfirmChoice::Leave | ConfirmChoice::NewMap => {
                let warn = accent_ink(2);
                let hov = r.contains(m);
                draw_round_frame(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    14.0,
                    2.0,
                    control_fill_t(SRF(), hover_t(r, hov)),
                    if hov { ACCENT() } else { warn },
                );
                let text = if choice == ConfirmChoice::Leave {
                    "LEAVE"
                } else {
                    "NEW MAP"
                };
                ui.text_centered(text, r.x + r.w * 0.5, r.y + 29.0, 16.0, warn);
            }
            ConfirmChoice::Keep => primary_button(ui, r, "KEEP PLAYING", 16.0, m, true),
        }
    }
    ui.body_centered(
        "Enter or ESC: keep playing",
        WIN_W * 0.5,
        y + h - 22.0,
        12.0,
        with_alpha(INK(), 0.6),
    );
}

pub(crate) fn draw_game(game: &Game, ui: &Ui, settings: &Settings, summary: Option<&Summary>) {
    draw_board(game, ui, settings.chances);
    draw_icon_bar(settings, ui, mouse_virtual());
    if game.over.is_none() {
        draw_game_menu_button(ui, mouse_virtual());
    }
    draw_panel(game, ui, settings);
    // Rolls share the status lane; controls and player cards stay visible.
    draw_battle_overlay(game, ui);
    draw_banner(game, ui);
    draw_over(game, ui, summary);
}

// Both rolls live in the existing status lane, clear of the map and controls.
// When the last die of a battle has appeared and the totals are final.
pub(crate) fn battle_settle_time(b: &Battle) -> f32 {
    T_SHAKE * 0.6 + (b.ad.len().max(b.dd.len()) as f32 - 1.0).max(0.0) * 0.06 + 0.14
}

// "Mint takes Rose's land", "You hold", "Tie — Rose holds"
fn battle_result(game: &Game, b: &Battle) -> String {
    let atk = game.terrs[b.a].owner;
    let def = game.terrs[b.d].owner;
    let me = |p: usize| p == game.my_seat && p < game.humans;
    let name = |p: usize| game.name_of(p);
    if b.captured {
        match (me(atk), me(def)) {
            (true, _) => format!("You take {}'s land", name(def)),
            (_, true) => format!("{} takes your land", name(atk)),
            _ => format!("{} takes {}'s land", name(atk), name(def)),
        }
    } else {
        let holds = if me(def) {
            "You hold".to_string()
        } else {
            format!("{} holds", name(def))
        };
        if b.ra == b.rd {
            format!("Tie — {holds}")
        } else {
            holds
        }
    }
}

pub(crate) fn draw_battle_overlay(game: &Game, ui: &Ui) {
    let Some(b) = &game.battle else { return };
    let reveal = b.t - T_SHAKE * 0.6;
    let settled = b.t >= battle_settle_time(b);
    for (row, (player, dice, role)) in [
        (game.terrs[b.a].owner, &b.ad, "ATTACK"),
        (game.terrs[b.d].owner, &b.dd, "DEFEND"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 691.0 + row as f32 * 27.0;
        let pal = palette(player);
        draw_symbol(48.0, y + 10.0, 6.0, player, accent_ink(player));
        ui.text(role, 62.0, y + 14.0, 11.0, INK());
        let mut shown = 0u32;
        for (i, &value) in dice.iter().enumerate() {
            let td = reveal - i as f32 * 0.06;
            let val = if td < 0.14 && !REDUCED_MOTION.load(Ordering::Relaxed) {
                1 + ((b.t * 22.0) as usize + i * 3 + row) % 6
            } else {
                shown += value as u32;
                value as usize
            };
            draw_face_die(126.0 + i as f32 * 25.0, y, 20.0, val, &pal);
        }
        let won = settled && (b.captured == (row == 0));
        let total = if settled {
            shown.to_string()
        } else {
            "…".to_string()
        };
        ui.text_centered(
            &total,
            350.0,
            y + 16.0,
            19.0,
            if won { accent_ink(player) } else { INK() },
        );
        if won {
            ui.text("WIN", 377.0, y + 14.0, 11.0, accent_ink(player));
        }
    }
    if settled {
        ui.body_fitted(
            &battle_result(game, b),
            435.0,
            721.0,
            14.0,
            WIN_W - 216.0 - 435.0 - 12.0,
            INK(),
        );
    } else {
        ui.body("Rolling…", 435.0, 721.0, 14.0, with_alpha(INK(), 0.72));
    }
}
