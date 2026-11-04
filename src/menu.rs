//! The main menu screen.

use dicegame::engine::{self, prelude::*};

use crate::board::{
    apply_position_swap, cell_pos, gen_map, gen_map_local, neighbor_of, random_seed, COLS, HEX,
    ROWS, SQRT3,
};
use crate::dice::{
    draw_die, draw_ellipse_fan, draw_star, stack_shadow, DIE_W, STACK_SHADOW, STACK_STEP,
};
use crate::draw_game::fill_hex;
use crate::game::{
    diff_idx, idx_diff, Difficulty, Game, TeamCfg, HUMAN, MAX_PLAYERS, MAX_TEAMS, SEED_MOD,
};
use crate::lobby::JoinUi;
use crate::net::{lobby_layout, net_port, HostNet, LobbyPicks};
use crate::settings::{load_bookmarks, save_bookmarks, save_settings, Settings};
use crate::setup::{
    draw_color_row, draw_preview, draw_rule_rows, draw_seed_row, draw_team_block, r_color, r_diff,
    r_mode, r_new, r_next, r_player, r_prev, r_preview, r_primary, r_secondary_half, r_seed,
    r_team_bridge, r_team_count, r_team_ff, r_team_pick, BOTTOM_Y, COL_END, LEFT_X, RIGHT_W,
    RIGHT_X, TEAM_BLOCK_H, TEAM_BLOCK_W, TOP_Y,
};
use crate::sound::Snd;
use crate::theme::{
    accent_ink, apply_local_colors, color_map, colorblind_mode, dark_mode, darken, map_fill, mix,
    palette, theme_index, with_alpha, Palette, ACCENT, BG, BORDER, CARD_EDGE, INK, INK_ON_FILL,
    MAP_EDGE, PLAYER_BASE, SEA, SRF,
};
use crate::ui::{
    card, control_fill, control_fill_t, draw_icon_bar, draw_round_frame, draw_round_rect, hover_t,
    label, menu_button, mouse_virtual, primary_button, secondary_button, toolbar_contains, Ui,
    WIN_H, WIN_W,
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
    pub(crate) show_rules: bool,             // info card shows the rules instead of your record
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
            show_rules: false,
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

// The menu uses the shared setup grid (see setup.rs): map and rules on the
// left, saved maps and a help/record or team card on the right.
fn r_bookmark() -> Rect {
    let pv = r_preview();
    Rect::new(pv.x + pv.w - 52.0, pv.y + 10.0, 42.0, 42.0)
}

// the lower right card: how to play, your record, or team setup
fn r_info_card() -> Rect {
    Rect::new(RIGHT_X, 392.0, RIGHT_W, BOTTOM_Y - 392.0)
}

fn team_origin() -> Vec2 {
    let c = r_info_card();
    vec2(c.x + 16.0, c.y + 52.0)
}

// switches the info card between your record and the rules
fn r_info_toggle() -> Rect {
    let c = r_info_card();
    Rect::new(c.x + c.w - 86.0, c.y + 12.0, 72.0, 28.0)
}

fn r_saved_card() -> Rect {
    Rect::new(RIGHT_X, TOP_Y, RIGHT_W, r_preview().h)
}

fn r_bm_row(i: usize) -> Rect {
    let c = r_saved_card();
    Rect::new(c.x + 16.0, c.y + 48.0 + i as f32 * 52.0, c.w - 40.0, 44.0)
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
        if is_key_pressed(KeyCode::N) && !modifier_down() {
            menu.seed = random_seed();
            menu.regen();
            snd.push(Snd::Click);
        }
    }
    // mouse wheel scrolls the bookmark list
    let (_, wheel) = mouse_wheel();
    if wheel != 0.0 {
        let mw = mouse_virtual();
        if r_saved_card().contains(mw) {
            let max_off = menu.bookmarks.len().saturating_sub(BM_SHOWN) as f32;
            menu.bm_scroll = (menu.bm_scroll - wheel.signum()).clamp(0.0, max_off);
        }
    }
    if !is_mouse_button_pressed(MouseButton::Left) {
        return MenuAction::None;
    }
    let m = mouse_virtual();
    if menu.editing && !r_seed().contains(m) {
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
    if !settings.teams && settings.games_played() > 0 && r_info_toggle().contains(m) {
        menu.show_rules = !menu.show_rules;
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if settings.teams {
        let o = team_origin();
        for k in 0..4 {
            if r_team_count(o, k).contains(m) {
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
                if r_team_pick(o, k).contains(m) {
                    settings.team_mine = k;
                    save_settings(settings);
                    snd.push(Snd::Click);
                    return MenuAction::None;
                }
            }
        }
        if r_team_ff(o).contains(m) {
            settings.team_ff = !settings.team_ff;
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
        if r_team_bridge(o).contains(m) {
            settings.team_bridge = !settings.team_bridge;
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    if r_seed().contains(m) {
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
    if toolbar_contains(m) {
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
    if r_primary().contains(m) {
        return MenuAction::Start;
    }
    if r_secondary_half(0).contains(m) {
        snd.push(Snd::Click);
        return MenuAction::Host;
    }
    if r_secondary_half(1).contains(m) {
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
        theme_index(),
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
    y0 -= DIE_W * 3.45;
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
            let same = neighbor_of(c, r, d).is_some_and(|n| game.cell_terr[n] == t);
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
    let ch = w * STACK_STEP;
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
        let (rx, ry) = stack_shadow(n > 4);
        draw_ellipse_fan(base.x, base.y - w * 0.21, w * rx, w * ry, STACK_SHADOW);
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

fn draw_menu(menu: &Menu, settings: &Settings, ui: &Ui, time: f32) {
    let m = mouse_virtual();

    // Keep the large outlined title and the map out of hover redraws. Theme
    // and palette changes still invalidate on the input frame.
    use std::hash::{Hash, Hasher};
    let mut art_key = std::collections::hash_map::DefaultHasher::new();
    (menu.seed, menu.players, theme_index(), colorblind_mode()).hash(&mut art_key);
    // the static parts of the right-hand cards are cached too
    (
        menu.bookmarks.is_empty(),
        settings.teams,
        settings.team_ff,
        settings.team_bridge,
        menu.show_rules,
        settings.played,
        settings.won,
    )
        .hash(&mut art_key);
    for player in 0..MAX_PLAYERS {
        color_map(player).hash(&mut art_key);
    }
    engine::cached_layer(3, art_key.finish(), || {
        clear_background(BG());
        draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());

        // the logo: the yellow die from the README next to a clean wordmark
        let title = "DICE WARS";
        let size = 54.0;
        let die_w = 44.0;
        let gap = 18.0;
        let tw = ui.width(title, size);
        let x0 = WIN_W * 0.5 - (die_w + gap + tw) * 0.5;
        let base = PLAYER_BASE[0];
        let logo = Palette {
            fill: mix(base, WHITE, 0.46),
            mid: base,
            dark: darken(base, 0.78),
            light: mix(base, WHITE, 0.62),
            pip: Color::new(0.22, 0.23, 0.38, 1.0),
        };
        let (dx, dy) = (x0 + die_w * 0.5, 88.0);
        draw_ellipse_fan(
            dx,
            dy - die_w * 0.2,
            die_w * 0.66,
            die_w * 0.3,
            with_alpha(BLACK, if dark_mode() { 0.35 } else { 0.12 }),
        );
        draw_die(dx, dy, die_w, 5, 0, &logo);
        ui.text(title, x0 + die_w + gap, 84.0, size, INK());
        draw_preview(ui, Some(&menu.preview), "");
        draw_cards_static(menu, settings, ui);
    });

    // ---- left column: map, rules, your color, actions
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
    draw_seed_row(ui, m, &seed_str, menu.editing, true);
    draw_rule_rows(ui, m, true, menu.players, menu.difficulty, settings.teams);
    draw_color_row(ui, m, settings.color, &[]);

    // the star on the preview saves the map
    let rb = r_bookmark();
    let marked = menu.is_bookmarked();
    let hovs = rb.contains(m);
    draw_round_frame(
        rb.x,
        rb.y,
        rb.w,
        rb.h,
        10.0,
        1.5,
        control_fill_t(SRF(), hover_t(rb, hovs)),
        mix(CARD_EDGE(), ACCENT(), hover_t(rb, hovs)),
    );
    let scol = if marked {
        accent_ink(HUMAN)
    } else {
        with_alpha(INK(), 0.60)
    };
    draw_star(rb.x + rb.w * 0.5, rb.y + rb.h * 0.5, 13.0, scol);

    primary_button(ui, r_primary(), "START GAME", 28.0, m, true);
    secondary_button(ui, r_secondary_half(0), "HOST GAME", 14.0, m);
    secondary_button(ui, r_secondary_half(1), "JOIN GAME", 14.0, m);
    if menu.host_err > 0.0 {
        ui.body_centered(
            &format!(
                "Could not open port {} — is another copy hosting?",
                net_port()
            ),
            (LEFT_X + COL_END) * 0.5,
            BOTTOM_Y + 26.0,
            14.0,
            accent_ink(2),
        );
    }

    // ---- right column: saved maps, then help / record / team setup
    draw_saved_rows(menu, ui, m);
    if settings.teams {
        draw_team_block(
            ui,
            m,
            team_origin(),
            settings.team_cfg(),
            settings.team_mine,
            true,
            true,
        );
    } else if settings.games_played() > 0 {
        let text = if menu.show_rules { "RECORD" } else { "RULES" };
        secondary_button(ui, r_info_toggle(), text, 11.0, m);
    }

    // the same toolbar as in-game: sound and settings
    draw_icon_bar(settings, ui, m);
}

// card frames and text that only change with settings; drawn into the cached
// menu layer so hovering never re-renders them
fn draw_cards_static(menu: &Menu, settings: &Settings, ui: &Ui) {
    let c = r_saved_card();
    card(ui, c, "SAVED MAPS");
    if menu.bookmarks.is_empty() {
        let cx = c.x + c.w * 0.5;
        draw_star(cx, c.y + 112.0, 22.0, with_alpha(INK(), 0.18));
        ui.text_centered("No saved maps yet", cx, c.y + 168.0, 17.0, INK());
        ui.body_centered(
            "Click the star on a map to keep it here.",
            cx,
            c.y + 194.0,
            13.0,
            with_alpha(INK(), 0.72),
        );
    }
    if settings.teams {
        card(ui, r_info_card(), "TEAM SETUP");
        let o = team_origin();
        let mut lines = vec!["The last team standing wins."];
        lines.push(if settings.team_ff {
            "Friendly fire: you can attack allies."
        } else {
            "No friendly fire: allies are safe."
        });
        lines.push(if settings.team_bridge {
            "Island links: allied land connects."
        } else {
            "Reinforcements count only your land."
        });
        for (i, line) in lines.iter().enumerate() {
            ui.body_fitted(
                line,
                o.x,
                o.y + TEAM_BLOCK_H + 34.0 + i as f32 * 22.0,
                13.0,
                TEAM_BLOCK_W,
                with_alpha(INK(), 0.72),
            );
        }
    } else if settings.games_played() == 0 || menu.show_rules {
        draw_rules_card(ui);
    } else {
        draw_record_card(settings, ui);
    }
}

// the saved-map rows: hoverable, so drawn live
fn draw_saved_rows(menu: &Menu, ui: &Ui, m: Vec2) {
    let c = r_saved_card();
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
        let ink = if active { INK_ON_FILL } else { INK() };
        ui.text(&format!("#{:08}", seed), r.x + 16.0, r.y + 29.0, 18.0, ink);
        ui.body(
            &format!("{} players", players),
            r.x + r.w - 128.0,
            r.y + 28.0,
            13.0,
            with_alpha(ink, 0.72),
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
                ACCENT(),
            );
        }
        // a drawn cross: crisp at every scale, unlike a letter "x"
        let (cx, cy) = (del.x + del.w * 0.5, del.y + del.h * 0.5);
        let col = with_alpha(ink, if dh { 1.0 } else { 0.6 });
        draw_line(cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 2.0, col);
        draw_line(cx - 5.0, cy + 5.0, cx + 5.0, cy - 5.0, 2.0, col);
    }
    if menu.bookmarks.len() > BM_SHOWN {
        let total = menu.bookmarks.len() as f32;
        let track_y = r_bm_row(0).y;
        let track_h = BM_SHOWN as f32 * 52.0 - 8.0;
        let x = c.x + c.w - 16.0;
        draw_round_rect(x, track_y, 5.0, track_h, 2.5, CARD_EDGE());
        let th = (track_h * BM_SHOWN as f32 / total).max(24.0);
        let ty = track_y + (track_h - th) * (bm_off as f32 / (total - BM_SHOWN as f32));
        draw_round_rect(x, ty, 5.0, th, 2.5, with_alpha(INK(), 0.35));
    }
}

const KEYS: [(&str, &str); 4] = [
    ("ENTER", "start the game"),
    ("SPACE", "end your turn"),
    ("SPACE", "hold to fast-forward bots"),
    ("ESC", "cancel, or pause the game"),
];

fn draw_keys(ui: &Ui, x: f32, y: f32) {
    label(ui, "KEYS", x, y);
    for (i, (key, what)) in KEYS.iter().enumerate() {
        let ly = y + 24.0 + i as f32 * 21.0;
        ui.text(key, x, ly, 12.0, INK());
        ui.body(what, x + 62.0, ly, 13.0, with_alpha(INK(), 0.72));
    }
}

fn draw_rules_card(ui: &Ui) {
    let c = r_info_card();
    card(ui, c, "HOW TO PLAY");
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
        ui.body(
            line,
            c.x + 18.0,
            c.y + 68.0 + i as f32 * 24.0,
            14.0,
            with_alpha(INK(), 0.85),
        );
    }
    draw_keys(ui, c.x + 18.0, c.y + 260.0);
}

fn draw_record_card(settings: &Settings, ui: &Ui) {
    let c = r_info_card();
    card(ui, c, "YOUR RECORD");
    let (played, won) = (settings.games_played(), settings.games_won());
    ui.text_centered(
        &format!(
            "{} {} of {} game{}",
            won,
            if won == 1 { "win" } else { "wins" },
            played,
            if played == 1 { "" } else { "s" }
        ),
        c.x + c.w * 0.5,
        c.y + 86.0,
        20.0,
        INK(),
    );
    for (k, name) in ["EASY", "NORMAL", "HARD"].iter().enumerate() {
        let y = c.y + 120.0 + k as f32 * 40.0;
        let (p, w) = (settings.played[k], settings.won[k]);
        label(ui, name, c.x + 18.0, y + 16.0);
        let (bx, bw) = (c.x + 100.0, c.w - 190.0);
        draw_round_rect(bx, y + 7.0, bw, 10.0, 5.0, with_alpha(INK(), 0.10));
        if p > 0 && w > 0 {
            let share = w as f32 / p as f32;
            draw_round_rect(
                bx,
                y + 7.0,
                (bw * share).max(10.0),
                10.0,
                5.0,
                palette(HUMAN).mid,
            );
        }
        ui.text(
            &if p == 0 {
                "—".to_string()
            } else {
                format!("{w} / {p}")
            },
            bx + bw + 14.0,
            y + 17.0,
            14.0,
            with_alpha(INK(), 0.85),
        );
    }
    draw_keys(ui, c.x + 18.0, c.y + 260.0);
}

impl App {
    // One frame of the main menu.
    pub(crate) fn run_menu(&mut self, dt: f32) {
        self.menu_time += dt;
        let action = update_menu(&mut self.menu, &mut self.settings, &mut self.snd_queue, dt);
        if !matches!(action, MenuAction::None) {
            self.leave_network(); // nothing from an earlier network game carries over
        }
        match action {
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
