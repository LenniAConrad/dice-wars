//! The game-setup form shared by the main menu, the host lobby and the guest
//! lobby. All three screens use the same grid, so every control sits in the
//! same place on each of them:
//!
//! ```text
//!  left column (map and rules)            right column (cards)
//!  ┌ map preview ───────────────┐         ┌ saved maps / room ┐
//!  [<] [  seed  ] [>] [NEW]               │                   │
//!  PLAYERS     2 3 4 5 6 7 8 9            ├ how to play /     ┤
//!  DIFFICULTY  EASY NORMAL HARD           │ team setup /      │
//!  MODE        FREE-FOR-ALL TEAMS         │ players           │
//!  YOUR COLOR  ■ ■ ■ ■ ■ ■ ■ ■ ■          │                   │
//!  [ START GAME ] [secondary]             └───────────────────┘
//! ```

use dicegame::engine::prelude::*;

use crate::dice::draw_symbol;
use crate::game::{team_letter, Difficulty, Game, TeamCfg, MAX_PLAYERS, MAX_TEAMS};
use crate::menu::draw_map_mini;
use crate::theme::{base_color, darken, with_alpha, ACCENT, BORDER, CARD_EDGE, INK, SRF};
use crate::ui::{
    control_fill_t, draw_round_frame, draw_round_rect, draw_toggle, hover_t, label, menu_button,
    seg_group, seg_row, Ui, WIN_W,
};

// ---- the grid
pub(crate) const LEFT_X: f32 = 70.0; // row labels
pub(crate) const CTRL_X: f32 = 170.0; // controls after the label gutter
pub(crate) const COL_END: f32 = 530.0; // right edge of the left column
pub(crate) const RIGHT_X: f32 = 570.0;
pub(crate) const RIGHT_W: f32 = 320.0;
pub(crate) const TOP_Y: f32 = 112.0;
pub(crate) const BOTTOM_Y: f32 = 772.0;
const ROW_H: f32 = 40.0;
const ROW_PLAYERS: f32 = 452.0;
const ROW_DIFF: f32 = 504.0;
const ROW_MODE: f32 = 556.0;
const ROW_COLOR: f32 = 608.0;
const ACTION_Y: f32 = 700.0;

pub(crate) fn r_preview() -> Rect {
    Rect::new(LEFT_X, TOP_Y, COL_END - LEFT_X, 268.0)
}

pub(crate) fn r_prev() -> Rect {
    Rect::new(LEFT_X, 392.0, 44.0, 44.0)
}

pub(crate) fn r_seed() -> Rect {
    Rect::new(LEFT_X + 52.0, 392.0, 260.0, 44.0)
}

pub(crate) fn r_next() -> Rect {
    Rect::new(LEFT_X + 320.0, 392.0, 44.0, 44.0)
}

pub(crate) fn r_new() -> Rect {
    Rect::new(LEFT_X + 372.0, 392.0, COL_END - LEFT_X - 372.0, 44.0)
}

pub(crate) fn r_player(k: usize) -> Rect {
    Rect::new(CTRL_X + k as f32 * 45.0, ROW_PLAYERS, 45.0, ROW_H)
}

pub(crate) fn r_diff(k: usize) -> Rect {
    Rect::new(CTRL_X + k as f32 * 120.0, ROW_DIFF, 120.0, ROW_H)
}

pub(crate) fn r_mode(k: usize) -> Rect {
    Rect::new(CTRL_X + k as f32 * 180.0, ROW_MODE, 180.0, ROW_H)
}

pub(crate) fn r_color(i: usize) -> Rect {
    Rect::new(CTRL_X + i as f32 * 40.0, ROW_COLOR + 8.0, 34.0, 24.0)
}

/// The filled call to action (START GAME, or the guest's waiting slot).
pub(crate) fn r_primary() -> Rect {
    Rect::new(LEFT_X, ACTION_Y, 260.0, BOTTOM_Y - ACTION_Y)
}

/// One secondary action beside the primary one (CANCEL, LEAVE).
pub(crate) fn r_secondary() -> Rect {
    Rect::new(
        LEFT_X + 272.0,
        ACTION_Y,
        COL_END - LEFT_X - 272.0,
        BOTTOM_Y - ACTION_Y,
    )
}

/// Two stacked secondary actions (HOST GAME over JOIN GAME).
pub(crate) fn r_secondary_half(i: usize) -> Rect {
    let r = r_secondary();
    let h = (r.h - 6.0) * 0.5;
    Rect::new(r.x, r.y + i as f32 * (h + 6.0), r.w, h)
}

/// Lobby screens put their name under the window title.
pub(crate) fn draw_title(ui: &Ui, text: &str) {
    ui.text_centered(text, WIN_W * 0.5, 82.0, 34.0, INK());
}

pub(crate) fn draw_preview(ui: &Ui, game: Option<&Game>, waiting: &str) {
    let pv = r_preview();
    draw_round_frame(pv.x, pv.y, pv.w, pv.h, 16.0, 2.0, SRF(), CARD_EDGE());
    match game {
        Some(g) => draw_map_mini(
            g,
            Rect::new(pv.x + 10.0, pv.y + 10.0, pv.w - 20.0, pv.h - 20.0),
        ),
        None => ui.body_centered(
            waiting,
            pv.x + pv.w * 0.5,
            pv.y + pv.h * 0.5,
            15.0,
            with_alpha(INK(), 0.55),
        ),
    }
}

/// `< seed > NEW`. Read-only (guests) shows the map number across the row.
pub(crate) fn draw_seed_row(ui: &Ui, m: Vec2, text: &str, editing: bool, live: bool) {
    if !live {
        let r = Rect::new(LEFT_X, r_seed().y, COL_END - LEFT_X, r_seed().h);
        draw_round_frame(r.x, r.y, r.w, r.h, 14.0, 2.0, SRF(), CARD_EDGE());
        ui.text_centered(
            &format!("MAP {text}"),
            r.x + r.w * 0.5,
            r.y + 29.0,
            18.0,
            with_alpha(INK(), 0.75),
        );
        return;
    }
    let (rp, rs, rn, rr) = (r_prev(), r_seed(), r_next(), r_new());
    menu_button(rp, SRF(), CARD_EDGE(), rp.contains(m));
    draw_poly(
        rp.x + rp.w * 0.5 - 1.5,
        rp.y + rp.h * 0.5,
        3,
        8.5,
        180.0,
        INK(),
    );
    menu_button(
        rs,
        SRF(),
        if editing { ACCENT() } else { CARD_EDGE() },
        rs.contains(m),
    );
    ui.text_centered(text, rs.x + rs.w * 0.5, rs.y + 30.0, 21.0, INK());
    menu_button(rn, SRF(), CARD_EDGE(), rn.contains(m));
    draw_poly(
        rn.x + rn.w * 0.5 + 1.5,
        rn.y + rn.h * 0.5,
        3,
        8.5,
        0.0,
        INK(),
    );
    menu_button(rr, SRF(), CARD_EDGE(), rr.contains(m));
    ui.text_centered("NEW", rr.x + rr.w * 0.5, rr.y + 28.0, 16.0, INK());
}

/// PLAYERS, DIFFICULTY and MODE.
pub(crate) fn draw_rule_rows(
    ui: &Ui,
    m: Vec2,
    live: bool,
    players: usize,
    diff: Difficulty,
    teams: bool,
) {
    let nums = ["2", "3", "4", "5", "6", "7", "8", "9"];
    let row: Vec<(Rect, &str, bool)> = (0..MAX_PLAYERS - 1)
        .map(|k| (r_player(k), nums[k], players == k + 2))
        .collect();
    seg_row(ui, m, live, "PLAYERS", &row, 18.0);
    let row: Vec<(Rect, &str, bool)> = [
        ("EASY", Difficulty::Easy),
        ("NORMAL", Difficulty::Normal),
        ("HARD", Difficulty::Hard),
    ]
    .into_iter()
    .enumerate()
    .map(|(k, (l, d))| (r_diff(k), l, diff == d))
    .collect();
    seg_row(ui, m, live, "DIFFICULTY", &row, 15.0);
    let row = vec![
        (r_mode(0), "FREE-FOR-ALL", !teams),
        (r_mode(1), "TEAMS", teams),
    ];
    seg_row(ui, m, live, "MODE", &row, 15.0);
}

/// YOUR COLOR swatches; colors other people already took are crossed out.
pub(crate) fn draw_color_row(ui: &Ui, m: Vec2, selected: usize, taken: &[usize]) {
    label(ui, "YOUR COLOR", LEFT_X, ROW_COLOR + ROW_H * 0.5 + 4.0);
    for i in 0..MAX_PLAYERS {
        let r = r_color(i);
        let base = base_color(i);
        if selected == i {
            draw_round_frame(
                r.x - 3.0,
                r.y - 3.0,
                r.w + 6.0,
                r.h + 6.0,
                9.0,
                2.0,
                SRF(),
                BORDER(),
            );
        }
        let free = !taken.contains(&i);
        draw_round_rect(
            r.x,
            r.y,
            r.w,
            r.h,
            7.0,
            control_fill_t(base, hover_t(r, free && r.contains(m))),
        );
        if !free {
            draw_line(
                r.x + 5.0,
                r.y + r.h - 5.0,
                r.x + r.w - 5.0,
                r.y + 5.0,
                2.5,
                SRF(),
            );
        }
    }
}

// ---- team setup: the same block in the menu card and the lobby player card
pub(crate) const TEAM_BLOCK_W: f32 = 288.0;
pub(crate) const TEAM_BLOCK_H: f32 = 190.0;

/// k = 0..2 → 2/3/4 teams; k = 3 → humans vs bots.
pub(crate) fn r_team_count(o: Vec2, k: usize) -> Rect {
    let x = o.x + (k as f32 * 48.0);
    let w = if k < 3 { 48.0 } else { TEAM_BLOCK_W - 144.0 };
    Rect::new(x, o.y + 18.0, w, 34.0)
}

pub(crate) fn r_team_pick(o: Vec2, k: usize) -> Rect {
    Rect::new(o.x + k as f32 * 48.0, o.y + 84.0, 48.0, 34.0)
}

pub(crate) fn r_team_ff(o: Vec2) -> Rect {
    Rect::new(o.x, o.y + 132.0, TEAM_BLOCK_W, 24.0)
}

pub(crate) fn r_team_bridge(o: Vec2) -> Rect {
    Rect::new(o.x, o.y + 162.0, TEAM_BLOCK_W, 24.0)
}

/// `my_team` None hides the team picker (the host's own pick is in `cfg`).
/// `live_rules` makes team count and the two rules clickable, `live_pick` the
/// team picker.
pub(crate) fn draw_team_block(
    ui: &Ui,
    m: Vec2,
    o: Vec2,
    cfg: TeamCfg,
    my_team: usize,
    live_rules: bool,
    live_pick: bool,
) {
    let off = vec2(-1.0, -1.0);
    label(ui, "TEAMS", o.x, o.y + 10.0);
    let row: Vec<(Rect, &str, bool)> = ["2", "3", "4", "HUMANS VS BOTS"]
        .into_iter()
        .enumerate()
        .map(|(k, l)| {
            let sel = if k < 3 {
                !cfg.hvb && cfg.count == k + 2
            } else {
                cfg.hvb
            };
            (r_team_count(o, k), l, sel)
        })
        .collect();
    seg_group(ui, if live_rules { m } else { off }, live_rules, &row, 13.0);
    label(ui, "YOUR TEAM", o.x, o.y + 76.0);
    if cfg.hvb {
        ui.body(
            "Every human plays against all the bots.",
            o.x,
            o.y + 106.0,
            13.0,
            with_alpha(INK(), 0.72),
        );
    } else {
        let count = cfg.count.clamp(2, MAX_TEAMS);
        let row: Vec<(Rect, &str, bool)> = (0..count)
            .map(|k| {
                (
                    r_team_pick(o, k),
                    team_letter(k),
                    my_team.min(count - 1) == k,
                )
            })
            .collect();
        seg_group(ui, if live_pick { m } else { off }, live_pick, &row, 15.0);
    }
    for (r, text, on) in [
        (r_team_ff(o), "FRIENDLY FIRE", cfg.ff),
        (r_team_bridge(o), "ISLANDS LINK VIA ALLIES", cfg.bridge),
    ] {
        draw_toggle(r, text, on, ui, if live_rules { m } else { off });
    }
}

/// One roster line: color chip, symbol, name, team letter.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_roster_row(
    ui: &Ui,
    x: f32,
    y: f32,
    w: f32,
    seat: usize,
    color: usize,
    name: &str,
    team: Option<usize>,
) {
    let base = base_color(color);
    draw_round_rect(x, y - 13.0, 24.0, 18.0, 6.0, base);
    draw_symbol(x + 40.0, y - 4.0, 7.0, seat, darken(base, 0.7));
    ui.text_fitted(name, x + 58.0, y, 15.0, w - 90.0, INK());
    if let Some(t) = team {
        ui.text(
            team_letter(t),
            x + w - 14.0,
            y,
            14.0,
            with_alpha(INK(), 0.72),
        );
    }
}
