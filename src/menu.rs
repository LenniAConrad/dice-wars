//! The main menu screen.

use dicegame::engine;
use dicegame::engine::prelude::*;

use crate::board::{
    apply_position_swap, cell_pos, gen_map, gen_map_local, neighbor_of, random_seed, COLS, HEX,
    ROWS, SQRT3,
};
use crate::dice::{draw_die, draw_ellipse_fan, draw_star, DIE_W};
use crate::draw_game::fill_hex;
use crate::game::{
    diff_idx, idx_diff, team_letter, Difficulty, Game, TeamCfg, HUMAN, MAX_PLAYERS, MAX_TEAMS,
    SEED_MOD,
};
use crate::lobby::JoinUi;
use crate::net::{lobby_layout, HostNet, LobbyPicks};
use crate::settings::{load_bookmarks, save_bookmarks, save_settings, Settings};
use crate::sound::Snd;
use crate::theme::{
    accent_ink, apply_local_colors, base_color, color_map, colorblind_mode, dark_mode, map_fill,
    mix, on_color, palette, th, with_alpha, ACCENT, BG, BORDER, CARD_EDGE, INK, INK_ON_FILL,
    MAP_EDGE, PLAYER_BASE, PLAYER_BASE_PROTAN, SEA, SRF,
};
use crate::ui::{
    control_fill, draw_icon_bar, draw_round_frame, draw_round_rect, draw_toggle, menu_button,
    mouse_virtual, r_icon, seg_row, Ui, WIN_H, WIN_W,
};
use crate::{App, Screen};

enum MenuAction {
    None,
    Start,
    Host,
    Join,
}

pub(crate) struct Menu {
    pub(crate) players: usize,
    pub(crate) humans: usize,
    pub(crate) difficulty: Difficulty,
    pub(crate) seed: u64,
    pub(crate) preview: Game,
    pub(crate) editing: bool,
    pub(crate) seed_text: String,
    pub(crate) bookmarks: Vec<(u64, usize)>, // (seed, players)
    pub(crate) hold_t: f32,                  // how long < or > has been held
    pub(crate) hold_next: f32,               // countdown to the next auto-repeat step
    pub(crate) bm_scroll: f32,               // bookmark list scroll offset, in rows
    pub(crate) host_err: f32,                // countdown showing the hosting-failed message
}

impl Menu {
    pub(crate) fn init(settings: &Settings) -> Self {
        let players = settings.players.clamp(2, MAX_PLAYERS);
        let humans = 1;
        let difficulty = idx_diff(settings.difficulty);
        let seed = random_seed();
        Menu {
            players,
            humans,
            difficulty,
            seed,
            preview: gen_map_local(seed, players, humans, difficulty, TeamCfg::default()),
            editing: false,
            seed_text: String::new(),
            bookmarks: load_bookmarks(),
            hold_t: 0.0,
            hold_next: 0.0,
            bm_scroll: 0.0,
            host_err: 0.0,
        }
    }

    pub(crate) fn regen(&mut self) {
        // the preview only shows the map, which team settings don't affect
        self.preview = gen_map_local(
            self.seed,
            self.players,
            self.humans,
            self.difficulty,
            TeamCfg::default(),
        );
    }

    pub(crate) fn commit_seed(&mut self) {
        self.editing = false;
        if let Ok(s) = self.seed_text.parse::<u64>() {
            self.seed = s % SEED_MOD;
        }
        self.regen();
    }

    pub(crate) fn is_bookmarked(&self) -> bool {
        self.bookmarks.contains(&(self.seed, self.players))
    }
}

// menu layout: a centered main column plus a bookmarks column on the right
pub(crate) const COL_L: f32 = 300.0;

pub(crate) const COL_R: f32 = 728.0;

fn r_color(i: usize) -> Rect {
    Rect::new(WIN_W * 0.5 - 141.0 + i as f32 * 32.0, 100.0, 26.0, 14.0)
}

fn r_player(k: usize) -> Rect {
    Rect::new(COL_L - 130.0 + k as f32 * 45.0, 244.0, 45.0, 44.0)
}

fn r_diff(k: usize) -> Rect {
    Rect::new(COL_L - 130.0 + k as f32 * 119.0, 148.0, 119.0, 44.0)
}

fn r_preview() -> Rect {
    Rect::new(COL_L - 230.0, 304.0, 460.0, 216.0)
}

fn r_prev() -> Rect {
    Rect::new(COL_L - 200.0, 550.0, 48.0, 48.0)
}

fn r_seedbox() -> Rect {
    Rect::new(COL_L - 144.0, 550.0, 200.0, 48.0)
}

fn r_next() -> Rect {
    Rect::new(COL_L + 64.0, 550.0, 48.0, 48.0)
}

fn r_new() -> Rect {
    Rect::new(COL_L + 120.0, 550.0, 80.0, 48.0)
}

fn r_bookmark() -> Rect {
    Rect::new(COL_L + 230.0 - 54.0, 312.0, 42.0, 42.0)
}

fn r_mode(k: usize) -> Rect {
    // one segmented row: FREE-FOR-ALL | TEAMS, between the seed row and start
    Rect::new(COL_L - 130.0 + k as f32 * 178.0, 628.0, 178.0, 40.0)
}

// the team setup card replaces the how-to-play card while team mode is on
const TEAM_CARD_X: f32 = COL_R - 160.0;

const TEAM_CARD_Y: f32 = 552.0;

const TEAM_CARD_H: f32 = 240.0;

fn r_team_count(k: usize) -> Rect {
    // k = 0..2 → 2/3/4 teams; k = 3 → humans-vs-bots
    if k < 3 {
        Rect::new(
            TEAM_CARD_X + 16.0 + k as f32 * 48.0,
            TEAM_CARD_Y + 54.0,
            40.0,
            32.0,
        )
    } else {
        Rect::new(TEAM_CARD_X + 16.0 + 144.0, TEAM_CARD_Y + 54.0, 144.0, 32.0)
    }
}

fn r_team_pick(k: usize) -> Rect {
    Rect::new(
        TEAM_CARD_X + 16.0 + k as f32 * 48.0,
        TEAM_CARD_Y + 120.0,
        40.0,
        32.0,
    )
}

fn r_team_ff() -> Rect {
    Rect::new(TEAM_CARD_X + 16.0, TEAM_CARD_Y + 170.0, 288.0, 24.0)
}

fn r_team_bridge() -> Rect {
    Rect::new(TEAM_CARD_X + 16.0, TEAM_CARD_Y + 202.0, 288.0, 24.0)
}

fn r_start() -> Rect {
    Rect::new(COL_L - 234.0, 698.0, 300.0, 72.0)
}

fn r_host() -> Rect {
    Rect::new(COL_L + 78.0, 698.0, 156.0, 33.0)
}

fn r_join() -> Rect {
    Rect::new(COL_L + 78.0, 737.0, 156.0, 33.0)
}

fn r_bm_row(i: usize) -> Rect {
    Rect::new(COL_R - 160.0, 320.0 + i as f32 * 54.0, 320.0, 44.0)
}

fn r_bm_del(i: usize) -> Rect {
    let r = r_bm_row(i);
    Rect::new(r.x + r.w - 40.0, r.y + 6.0, 32.0, 32.0)
}

const BM_SHOWN: usize = 4;

// returns true when the game should start
fn update_menu(
    menu: &mut Menu,
    settings: &mut Settings,
    snd: &mut Vec<Snd>,
    dt: f32,
) -> MenuAction {
    menu.host_err = (menu.host_err - dt).max(0.0);
    // holding < or > cycles through seeds after a short delay
    let held_dir: i64 = if is_mouse_button_down(MouseButton::Left) && !menu.editing {
        let m = mouse_virtual();
        if r_prev().contains(m) {
            -1
        } else if r_next().contains(m) {
            1
        } else {
            0
        }
    } else {
        0
    };
    if held_dir != 0 {
        menu.hold_t += dt;
        if menu.hold_t > 0.4 {
            menu.hold_next -= dt;
            if menu.hold_next <= 0.0 {
                menu.hold_next = 0.08;
                menu.seed = (menu.seed as i64 + held_dir).rem_euclid(SEED_MOD as i64) as u64;
                menu.regen();
            }
        }
    } else {
        menu.hold_t = 0.0;
        menu.hold_next = 0.0;
    }

    if menu.editing {
        while let Some(c) = get_char_pressed() {
            if c.is_ascii_digit() && menu.seed_text.len() < 8 {
                menu.seed_text.push(c);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            menu.seed_text.pop();
        }
        if is_key_pressed(KeyCode::Enter) {
            menu.commit_seed();
        }
    } else {
        while get_char_pressed().is_some() {} // drain stale chars
        if is_key_pressed(KeyCode::Enter) {
            return MenuAction::Start;
        }
        if is_key_pressed(KeyCode::N) {
            menu.seed = random_seed();
            menu.regen();
            snd.push(Snd::Click);
        }
    }
    // mouse wheel scrolls the bookmark list
    let (_, wheel) = mouse_wheel();
    if wheel != 0.0 {
        let mw = mouse_virtual();
        if mw.x > COL_R - 175.0 && mw.y > 130.0 && mw.y < 545.0 {
            let max_off = menu.bookmarks.len().saturating_sub(BM_SHOWN) as f32;
            menu.bm_scroll = (menu.bm_scroll - wheel.signum()).clamp(0.0, max_off);
        }
    }
    if !is_mouse_button_pressed(MouseButton::Left) {
        return MenuAction::None;
    }
    let m = mouse_virtual();
    if menu.editing && !r_seedbox().contains(m) {
        menu.commit_seed();
    }
    for i in 0..MAX_PLAYERS {
        if r_color(i).contains(m) {
            settings.color = i;
            apply_local_colors(i);
            menu.regen();
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    for k in 0..MAX_PLAYERS - 1 {
        if r_player(k).contains(m) {
            menu.players = k + 2;
            settings.players = menu.players;
            save_settings(settings);
            menu.regen();
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    for (k, d) in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
        .into_iter()
        .enumerate()
    {
        if r_diff(k).contains(m) {
            menu.difficulty = d;
            settings.difficulty = diff_idx(d);
            save_settings(settings);
            menu.regen();
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    for k in 0..2 {
        if r_mode(k).contains(m) {
            settings.teams = k == 1;
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    if settings.teams {
        for k in 0..4 {
            if r_team_count(k).contains(m) {
                if k < 3 {
                    settings.team_hvb = false;
                    settings.team_count = k + 2;
                    settings.team_mine = settings.team_mine.min(k + 1);
                } else {
                    settings.team_hvb = !settings.team_hvb;
                }
                save_settings(settings);
                snd.push(Snd::Click);
                return MenuAction::None;
            }
        }
        if !settings.team_hvb {
            for k in 0..settings.team_count.clamp(2, MAX_TEAMS) {
                if r_team_pick(k).contains(m) {
                    settings.team_mine = k;
                    save_settings(settings);
                    snd.push(Snd::Click);
                    return MenuAction::None;
                }
            }
        }
        if r_team_ff().contains(m) {
            settings.team_ff = !settings.team_ff;
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
        if r_team_bridge().contains(m) {
            settings.team_bridge = !settings.team_bridge;
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    if r_seedbox().contains(m) {
        if !menu.editing {
            menu.editing = true;
            menu.seed_text = menu.seed.to_string();
            snd.push(Snd::Click);
        }
        return MenuAction::None;
    }
    if r_prev().contains(m) {
        menu.seed = (menu.seed + SEED_MOD - 1) % SEED_MOD;
        menu.regen();
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if r_next().contains(m) {
        menu.seed = (menu.seed + 1) % SEED_MOD;
        menu.regen();
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if r_new().contains(m) {
        menu.seed = random_seed();
        menu.regen();
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if r_bookmark().contains(m) {
        if menu.is_bookmarked() {
            menu.bookmarks.retain(|&b| b != (menu.seed, menu.players));
        } else {
            menu.bookmarks.push((menu.seed, menu.players));
        }
        save_bookmarks(&menu.bookmarks);
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if (0..6).any(|i| r_icon(i).contains(m)) {
        return MenuAction::None;
    }
    let bm_off = menu.bm_scroll as usize;
    for i in 0..menu.bookmarks.len().saturating_sub(bm_off).min(BM_SHOWN) {
        if r_bm_del(i).contains(m) {
            menu.bookmarks.remove(i + bm_off);
            let max_off = menu.bookmarks.len().saturating_sub(BM_SHOWN) as f32;
            menu.bm_scroll = menu.bm_scroll.min(max_off);
            save_bookmarks(&menu.bookmarks);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
        if r_bm_row(i).contains(m) {
            let (s, p) = menu.bookmarks[i + bm_off];
            menu.seed = s;
            menu.players = p;
            menu.regen();
            snd.push(Snd::Select);
            return MenuAction::None;
        }
    }
    if r_start().contains(m) {
        return MenuAction::Start;
    }
    if r_host().contains(m) {
        snd.push(Snd::Click);
        return MenuAction::Host;
    }
    if r_join().contains(m) {
        snd.push(Snd::Click);
        return MenuAction::Join;
    }
    MenuAction::None
}

// small map rendering for the menu preview
pub(crate) fn draw_map_mini(game: &Game, area: Rect) {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        game.seed,
        dark_mode(),
        colorblind_mode(),
        area.x.to_bits(),
        area.y.to_bits(),
        area.w.to_bits(),
        area.h.to_bits(),
    )
        .hash(&mut hash);
    for p in 0..MAX_PLAYERS {
        color_map(p).hash(&mut hash);
    }
    for t in &game.terrs {
        (t.owner, t.dice).hash(&mut hash);
    }
    engine::cached_layer(1, hash.finish(), || draw_map_mini_live(game, area));
}

fn draw_map_mini_live(game: &Game, area: Rect) {
    // fit the frame to this map's actual content: hex extents plus each
    // territory's dice tower, so the preview sits centered with even margins
    let (mut x0, mut y0) = (f32::MAX, f32::MAX);
    let (mut x1, mut y1) = (f32::MIN, f32::MIN);
    for i in 0..COLS * ROWS {
        if game.cell_terr[i] < 0 {
            continue;
        }
        let p = cell_pos(i);
        x0 = x0.min(p.x - SQRT3 * 0.5 * HEX);
        x1 = x1.max(p.x + SQRT3 * 0.5 * HEX);
        y0 = y0.min(p.y - HEX);
        y1 = y1.max(p.y + HEX);
    }
    // constant headroom for the tallest possible stack, so the preview
    // scale never shifts with the dice actually dealt
    y0 -= DIE_W * 3.3;
    let (bx, by, bw, bh) = (x0, y0, x1 - x0, y1 - y0);
    let k = (area.w / bw).min(area.h / bh);
    let ox = area.x + (area.w - bw * k) * 0.5 - bx * k;
    let oy = area.y + (area.h - bh * k) * 0.5 - by * k;
    let tp = |p: Vec2| vec2(ox + p.x * k, oy + p.y * k);

    // water dots, like the real board
    for i in 0..COLS * ROWS {
        if game.cell_terr[i] < 0 {
            let p = tp(cell_pos(i));
            draw_circle(p.x, p.y, (2.2 * k).max(0.8), SEA());
        }
    }
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        fill_hex(
            tp(cell_pos(i)),
            HEX * k + 0.3,
            map_fill(game.terrs[t as usize].owner),
        );
    }
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let (c, r) = (i % COLS, i / COLS);
        let pos = cell_pos(i);
        for d in 0..6 {
            let same = neighbor_of(c, r, d).map_or(false, |n| game.cell_terr[n] == t);
            if !same {
                let a0 = (d as f32 * 60.0 - 30.0).to_radians();
                let a1 = (d as f32 * 60.0 + 30.0).to_radians();
                let p0 = tp(pos + HEX * vec2(a0.cos(), a0.sin()));
                let p1 = tp(pos + HEX * vec2(a1.cos(), a1.sin()));
                draw_line(p0.x, p0.y, p1.x, p1.y, 1.3, MAP_EDGE);
            }
        }
    }
    // the real dice stacks in miniature, back to front, at exact map scale
    let mut order: Vec<usize> = (0..game.terrs.len()).collect();
    order.sort_by(|&a, &b| game.terrs[a].anchor.y.total_cmp(&game.terrs[b].anchor.y));
    let w = DIE_W * k;
    let ch = w * 0.62;
    for ti in order {
        let t = &game.terrs[ti];
        let pal = palette(t.owner);
        let base = tp(t.anchor) + vec2(0.0, 2.0);
        let n = t.dice as usize;
        let (fdx, bdx) = if n > 4 {
            (-w * 0.46, w * 0.46)
        } else {
            (0.0, 0.0)
        };
        draw_ellipse_fan(
            base.x,
            base.y - w * 0.21,
            if n > 4 { w * 1.04 } else { w * 0.62 },
            w * 0.34,
            Color::new(0.15, 0.16, 0.30, 0.26),
        );
        for i in 0..n.saturating_sub(4) {
            draw_die(
                base.x + bdx,
                base.y - w * 0.26 - i as f32 * ch,
                w,
                t.seed,
                (i + 4) as u32,
                &pal,
            );
        }
        for i in 0..n.min(4) {
            draw_die(
                base.x + fdx,
                base.y - i as f32 * ch,
                w,
                t.seed,
                i as u32,
                &pal,
            );
        }
    }
}

// a dice-burst hero fills the right column, a nod to the 2001 original
// a spiky explosion: alternating outer/inner spokes with jittered lengths
fn draw_burst(cx: f32, cy: f32, r_out: f32, r_in: f32, spikes: usize, rot: f32, col: Color) {
    let tau = std::f32::consts::TAU;
    for i in 0..spikes {
        let a0 = rot + i as f32 / spikes as f32 * tau;
        let a1 = rot + (i as f32 + 0.5) / spikes as f32 * tau;
        let a2 = rot + (i as f32 + 1.0) / spikes as f32 * tau;
        let ro = r_out * (0.82 + 0.36 * (((i * 7919) % 13) as f32 / 13.0));
        draw_triangle(
            vec2(cx + r_in * a0.cos(), cy + r_in * a0.sin()),
            vec2(cx + ro * a1.cos(), cy + ro * a1.sin()),
            vec2(cx + r_in * a2.cos(), cy + r_in * a2.sin()),
            col,
        );
    }
    draw_poly(cx, cy, spikes as u8, r_in, rot.to_degrees(), col);
}

fn draw_menu_hero() {
    let cx = COL_R;
    let cy = 212.0;
    draw_burst(
        cx,
        cy,
        118.0,
        54.0,
        12,
        0.1,
        mix(base_color(0), WHITE, 0.12),
    );
    draw_burst(
        cx,
        cy,
        82.0,
        40.0,
        12,
        0.36,
        mix(base_color(0), WHITE, 0.62),
    );
    // one contact shadow per die, centered under its base diamond with the
    // same footprint formula the in-game stacks use
    let sh = Color::new(0.15, 0.16, 0.30, 0.16);
    let dice = [
        (cx + 4.0, cy - 16.0, 42.0, 5u32, 3usize),
        (cx - 46.0, cy + 54.0, 58.0, 11, 2),
        (cx + 42.0, cy + 58.0, 52.0, 23, 4),
    ];
    for &(x, y, w, _, _) in &dice {
        let qh = w * 0.26;
        draw_ellipse_fan(x, y - qh * 0.8, w * 0.62, qh * 1.3, sh);
    }
    for &(x, y, w, seed, pl) in &dice {
        draw_die(x, y, w, seed, 0, &palette(pl));
    }
}

fn draw_menu(menu: &Menu, settings: &Settings, ui: &Ui, time: f32) {
    let m = mouse_virtual();

    // Keep the large outlined title, decorative dice, and map out of hover
    // redraws. Theme and palette changes still invalidate on the input frame.
    use std::hash::{Hash, Hasher};
    let mut art_key = std::collections::hash_map::DefaultHasher::new();
    (menu.seed, menu.players, dark_mode(), colorblind_mode()).hash(&mut art_key);
    for player in 0..MAX_PLAYERS {
        color_map(player).hash(&mut art_key);
    }
    engine::cached_layer(3, art_key.finish(), || {
        clear_background(BG());
        draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());

        // chunky extruded logo
        let title = "DICE WARS";
        let tw = ui.width(title, 58.0);
        let tx = WIN_W * 0.5 - tw * 0.5;
        // many faint offset copies blend into one soft drop shadow
        let sh = with_alpha(
            th(INK_ON_FILL, BLACK),
            if dark_mode() { 0.12 } else { 0.055 },
        );
        for (dx, dy) in [
            (2.5, 3.5),
            (4.0, 4.5),
            (5.5, 5.5),
            (3.0, 5.5),
            (5.0, 3.5),
            (4.0, 6.5),
            (2.5, 5.0),
            (5.5, 4.5),
        ] {
            ui.text(title, tx + dx, 86.0 + dy, 58.0, sh);
        }
        ui.text_outlined(
            title,
            tx,
            86.0,
            58.0,
            mix(PLAYER_BASE[0], WHITE, 0.2),
            INK_ON_FILL,
        );
        draw_menu_hero();
        // map preview
        let pv = r_preview();
        draw_round_frame(pv.x, pv.y, pv.w, pv.h, 14.0, 2.0, SRF(), CARD_EDGE());
        draw_map_mini(
            &menu.preview,
            Rect::new(pv.x + 8.0, pv.y + 8.0, pv.w - 16.0, pv.h - 16.0),
        );
    });
    // the accent band doubles as the pick-your-color control
    for i in 0..MAX_PLAYERS {
        let r = r_color(i);
        let base = if colorblind_mode() {
            PLAYER_BASE_PROTAN[i]
        } else {
            PLAYER_BASE[i]
        };
        let sel = settings.color == i;
        if sel {
            draw_round_frame(
                r.x - 3.0,
                r.y - 3.0,
                r.w + 6.0,
                r.h + 6.0,
                8.0,
                2.0,
                SRF(),
                BORDER(),
            );
        }
        let hov = r.contains(m);
        draw_round_rect(r.x, r.y, r.w, r.h, 6.0, control_fill(base, hov));
    }

    let nums = ["2", "3", "4", "5", "6", "7", "8", "9"];
    let row: Vec<(Rect, &str, bool)> = (0..MAX_PLAYERS - 1)
        .map(|k| (r_player(k), nums[k], menu.players == k + 2))
        .collect();
    seg_row(ui, m, true, "PLAYERS", &row, 18.0);
    let row: Vec<(Rect, &str, bool)> = [
        ("EASY", Difficulty::Easy),
        ("NORMAL", Difficulty::Normal),
        ("HARD", Difficulty::Hard),
    ]
    .into_iter()
    .enumerate()
    .map(|(k, (l, d))| (r_diff(k), l, menu.difficulty == d))
    .collect();
    seg_row(ui, m, true, "DIFFICULTY", &row, 15.0);

    // seed row
    let (rp, rs, rn, rr) = (r_prev(), r_seedbox(), r_next(), r_new());
    menu_button(rp, SRF(), CARD_EDGE(), rp.contains(m));
    draw_poly(
        rp.x + rp.w * 0.5 - 1.5,
        rp.y + rp.h * 0.5,
        3,
        8.5,
        180.0,
        INK(),
    );
    let seed_edge = if menu.editing { ACCENT() } else { CARD_EDGE() };
    menu_button(rs, SRF(), seed_edge, rs.contains(m));
    let seed_str = if menu.editing {
        let caret = if (time * 2.5) as i32 % 2 == 0 {
            "|"
        } else {
            " "
        };
        format!("{}{}", menu.seed_text, caret)
    } else {
        format!("{:08}", menu.seed)
    };
    ui.text_centered(&seed_str, rs.x + rs.w * 0.5, rs.y + 32.0, 22.0, INK());
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
    ui.text_centered("NEW", rr.x + rr.w * 0.5, rr.y + 31.0, 18.0, INK());

    let row = vec![
        (r_mode(0), "FREE-FOR-ALL", !settings.teams),
        (r_mode(1), "TEAMS", settings.teams),
    ];
    seg_row(ui, m, true, "MODE", &row, 15.0);

    // Match the toolbar's button treatment without changing the icon's size.
    let rb = r_bookmark();
    let marked = menu.is_bookmarked();
    let hovs = rb.contains(m);
    draw_round_frame(
        rb.x,
        rb.y,
        rb.w,
        rb.h,
        8.0,
        1.5,
        control_fill(SRF(), hovs),
        if hovs { ACCENT() } else { CARD_EDGE() },
    );
    let scol = if marked {
        accent_ink(HUMAN)
    } else {
        with_alpha(INK(), 0.60)
    };
    draw_star(rb.x + rb.w * 0.5, rb.y + rb.h * 0.5, 13.0, scol);

    // the same icon toolbar as in-game (sound, chances, colorblind, speed)
    draw_icon_bar(settings, ui, m);

    if menu.host_err > 0.0 {
        ui.text_centered(
            "Could not open port 7777 — is another instance hosting?",
            COL_L,
            800.0,
            15.0,
            accent_ink(2),
        );
    }

    // start button
    let st = r_start();
    menu_button(st, palette(HUMAN).mid, BORDER(), st.contains(m));
    ui.text_centered(
        "START GAME",
        st.x + st.w * 0.5,
        st.y + 45.0,
        30.0,
        on_color(palette(HUMAN).mid),
    );
    let rh = r_host();
    menu_button(rh, SRF(), CARD_EDGE(), rh.contains(m));
    ui.text_centered("HOST GAME", rh.x + rh.w * 0.5, rh.y + 22.0, 14.0, INK());
    let rj = r_join();
    menu_button(rj, SRF(), CARD_EDGE(), rj.contains(m));
    ui.text_centered("JOIN GAME", rj.x + rj.w * 0.5, rj.y + 22.0, 14.0, INK());

    if menu.bookmarks.is_empty() {
        ui.text_centered("Save a favorite map", COL_R, 380.0, 18.0, INK());
        ui.text_centered(
            "Use the star in the preview.",
            COL_R,
            407.0,
            14.0,
            with_alpha(INK(), 0.75),
        );
    }
    // Saved-map rows
    let bm_off = menu.bm_scroll as usize;
    for (i, &(seed, players)) in menu
        .bookmarks
        .iter()
        .skip(bm_off)
        .take(BM_SHOWN)
        .enumerate()
    {
        let r = r_bm_row(i);
        let active = seed == menu.seed && players == menu.players;
        let fill = if active { palette(HUMAN).fill } else { SRF() };
        let del = r_bm_del(i);
        let dh = del.contains(m);
        menu_button(
            r,
            fill,
            if active { BORDER() } else { CARD_EDGE() },
            r.contains(m) && !dh,
        );
        ui.text(
            &format!("#{:08}", seed),
            r.x + 18.0,
            r.y + 29.0,
            19.0,
            if active { INK_ON_FILL } else { INK() },
        );
        ui.text(
            &format!("{}P", players),
            r.x + r.w - 82.0,
            r.y + 29.0,
            16.0,
            with_alpha(if active { INK_ON_FILL } else { INK() }, 0.72),
        );
        if dh {
            draw_round_frame(
                del.x,
                del.y,
                del.w,
                del.h,
                8.0,
                1.5,
                control_fill(fill, true),
                BORDER(),
            );
        }
        ui.text_centered(
            "x",
            del.x + del.w * 0.5,
            del.y + 23.0,
            20.0,
            if active {
                INK_ON_FILL
            } else {
                with_alpha(INK(), 0.72)
            },
        );
    }
    if menu.bookmarks.len() > BM_SHOWN {
        let total = menu.bookmarks.len() as f32;
        let track_y = 320.0;
        let track_h = BM_SHOWN as f32 * 54.0 - 10.0;
        draw_round_rect(COL_R + 166.0, track_y, 5.0, track_h, 2.5, CARD_EDGE());
        let th = (track_h * BM_SHOWN as f32 / total).max(24.0);
        let ty = track_y + (track_h - th) * (bm_off as f32 / (total - BM_SHOWN as f32));
        draw_round_rect(COL_R + 166.0, ty, 5.0, th, 2.5, with_alpha(INK(), 0.35));
    }

    // below the bookmark list: team setup while team mode is on,
    // otherwise a quick rules card
    if settings.teams {
        draw_team_card(settings, ui, m);
    } else {
        let hx = TEAM_CARD_X;
        let hy = TEAM_CARD_Y;
        draw_round_frame(hx, hy, 320.0, TEAM_CARD_H, 14.0, 2.0, SRF(), CARD_EDGE());
        ui.text_centered(
            "HOW TO PLAY",
            COL_R,
            hy + 28.0,
            13.0,
            with_alpha(INK(), 0.72),
        );
        for (i, line) in [
            "1. Click one of your lands (2+ dice),",
            "    then a neighbor land to attack.",
            "2. Higher dice total wins the land.",
            "    Ties go to the defender.",
            "3. END TURN gives +1 die per land",
            "    in your biggest connected area.",
            "4. Conquer the whole map!",
        ]
        .iter()
        .enumerate()
        {
            ui.text(
                line,
                hx + 18.0,
                hy + 62.0 + i as f32 * 25.0,
                14.0,
                with_alpha(INK(), 0.8),
            );
        }
    }
}

// team-mode controls: how many teams, which one is yours, and the two rules
fn draw_team_card(settings: &Settings, ui: &Ui, m: Vec2) {
    let hx = TEAM_CARD_X;
    let hy = TEAM_CARD_Y;
    draw_round_frame(hx, hy, 320.0, TEAM_CARD_H, 14.0, 2.0, SRF(), CARD_EDGE());
    ui.text_centered(
        "TEAM SETUP",
        COL_R,
        hy + 26.0,
        13.0,
        with_alpha(INK(), 0.72),
    );

    ui.text("TEAMS", hx + 16.0, hy + 48.0, 10.0, with_alpha(INK(), 0.72));
    for (k, label) in ["2", "3", "4", "HUMANS VS BOTS"].into_iter().enumerate() {
        let r = r_team_count(k);
        let sel = if k < 3 {
            !settings.team_hvb && settings.team_count == k + 2
        } else {
            settings.team_hvb
        };
        let fill = if sel { palette(HUMAN).fill } else { SRF() };
        let edge = if sel { BORDER() } else { CARD_EDGE() };
        menu_button(r, fill, edge, r.contains(m));
        ui.text_centered(
            label,
            r.x + r.w * 0.5,
            r.y + 22.0,
            if k < 3 { 16.0 } else { 12.0 },
            if sel { INK_ON_FILL } else { INK() },
        );
    }

    if settings.team_hvb {
        ui.text(
            "Every human against all the bots.",
            hx + 16.0,
            hy + 140.0,
            13.0,
            with_alpha(INK(), 0.7),
        );
    } else {
        ui.text(
            "YOUR TEAM",
            hx + 16.0,
            hy + 114.0,
            10.0,
            with_alpha(INK(), 0.72),
        );
        let count = settings.team_count.clamp(2, MAX_TEAMS);
        for k in 0..count {
            let r = r_team_pick(k);
            let sel = settings.team_mine.min(count - 1) == k;
            let fill = if sel { palette(HUMAN).fill } else { SRF() };
            let edge = if sel { BORDER() } else { CARD_EDGE() };
            menu_button(r, fill, edge, r.contains(m));
            ui.text_centered(
                team_letter(k),
                r.x + r.w * 0.5,
                r.y + 22.0,
                16.0,
                if sel { INK_ON_FILL } else { INK() },
            );
        }
    }

    for (r, label, on) in [
        (r_team_ff(), "FRIENDLY FIRE", settings.team_ff),
        (
            r_team_bridge(),
            "ISLANDS LINK VIA ALLIES",
            settings.team_bridge,
        ),
    ] {
        draw_toggle(r, label, on, ui, m);
    }
}

impl App {
    // One frame of the main menu.
    pub(crate) fn run_menu(&mut self, dt: f32) {
        self.menu_time += dt;
        match update_menu(&mut self.menu, &mut self.settings, &mut self.snd_queue, dt) {
            MenuAction::Start => {
                self.game = Some(gen_map_local(
                    self.menu.seed,
                    self.menu.players,
                    1,
                    self.menu.difficulty,
                    self.settings.team_cfg(),
                ));
                self.screen = Screen::Play;
                self.snd_queue.push(Snd::Click);
            }
            MenuAction::Host => match HostNet::start(self.menu.players - 1) {
                Ok(h) => {
                    self.host = Some(h);
                    self.lobby_picks = LobbyPicks::default();
                    self.lobby_sync_t = 0.0;
                    self.lobby_name_edit = false;
                    // preview switches to the lobby's color layout
                    let (_tv, cv) = lobby_layout(
                        &[],
                        self.menu.players,
                        self.settings.team_cfg(),
                        &self.lobby_picks,
                        self.settings.color,
                    );
                    let mut g0 = gen_map(
                        self.menu.seed,
                        self.menu.players,
                        1,
                        self.menu.difficulty,
                        TeamCfg::default(),
                    );
                    apply_position_swap(&mut g0, &cv);
                    self.menu.preview = g0;
                    self.screen = Screen::HostLobby;
                }
                Err(_) => self.menu.host_err = 4.0,
            },
            MenuAction::Join => {
                self.join = JoinUi {
                    addr: self.settings.last_addr.clone(),
                    ..Default::default()
                };
                self.lobby_name_edit = false;
                self.screen = Screen::JoinLobby;
            }
            MenuAction::None => {}
        }
        draw_menu(&self.menu, &self.settings, &self.ui, self.menu_time);
    }
}
