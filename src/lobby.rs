//! Host, guest and join lobby screens.

use std::sync::atomic::Ordering;

use dicegame::engine::prelude::*;

use crate::board::{apply_position_swap, gen_map, random_seed};
use crate::clipboard::{copy_to_clipboard, paste_from_clipboard, spawn_clipboard_paste};
use crate::dice::draw_symbol;
use crate::game::{
    diff_idx, idx_diff, team_letter, Difficulty, Game, TeamCfg, HUMAN, MAX_PLAYERS, MAX_TEAMS,
    SEED_MOD,
};
use crate::lan::{LanScan, LAN_MAX_GAMES};
use crate::menu::{draw_map_mini, Menu, COL_L, COL_R};
use crate::net::{
    lobby_layout, lobby_line, local_ip, net_port, parse_csv_usize, send_net_line,
    spawn_guest_connect, GuestNet, HostMsg, HostNet, LobbyPicks, NET_MESSAGES_PER_FRAME,
    NET_ROOM_CODE_LEN,
};
use crate::settings::{clean_name, save_settings, Settings};
use crate::sound::Snd;
use crate::theme::{
    apply_colors, apply_local_colors, base_color, darken, mix, on_color, palette, with_alpha,
    ACCENT, BG, BORDER, CARD_EDGE, INK, INK_ON_FILL, REDUCED_MOTION, SRF,
};
use crate::ui::{
    control_fill, draw_icon_bar, draw_round_frame, draw_round_rect, draw_toggle, menu_button,
    mouse_virtual, seg_row, Ui, WIN_H, WIN_W,
};
use crate::{App, Screen};

// the host's lobby snapshot, as far as this guest has seen it
pub(crate) struct LobbyView {
    pub(crate) players: usize,
    pub(crate) diff: usize,
    pub(crate) ton: bool,
    pub(crate) thvb: bool,
    pub(crate) tcount: usize,
    pub(crate) tff: bool,
    pub(crate) tbridge: bool,
    pub(crate) occupied: Vec<usize>,
    pub(crate) teams: Vec<usize>,
    pub(crate) colors: Vec<usize>,
    pub(crate) names: Vec<String>,
}

#[derive(Default)]
pub(crate) struct JoinUi {
    pub(crate) addr: String,
    pub(crate) code: String,
    pub(crate) focus: usize, // 0 = address, 1 = code, 2 = name
    pub(crate) status: String,
    pub(crate) view: Option<LobbyView>,
    pub(crate) my_team: Option<usize>,
    pub(crate) my_color: Option<usize>,
    pub(crate) preview: Option<Game>,
    pub(crate) preview_key: (u64, usize),
    pub(crate) preview_colors: Vec<usize>,
    pub(crate) connect_rx: Option<std::sync::mpsc::Receiver<Result<GuestNet, String>>>,
    pub(crate) paste_rx: Option<std::sync::mpsc::Receiver<Option<String>>>,
    pub(crate) paste_focus: usize,
}

fn apply_join_paste(join: &mut JoinUi, focus: usize, text: &str) {
    if focus == 1 {
        join.code = text
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(NET_ROOM_CODE_LEN)
            .collect();
    } else if focus == 0 {
        join.addr = text
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == ':' || *c == '-')
            .take(40)
            .collect();
    }
}

fn parse_names_csv(s: &str) -> Vec<String> {
    s.split(',')
        .take(MAX_PLAYERS)
        .map(|n| {
            if n == "-" {
                String::new()
            } else {
                clean_name(n)
            }
        })
        .collect()
}

// the shared tail of START/RESUME: colors, team cfg, team list, names
pub(crate) fn parse_start_tail(
    it: &mut std::str::SplitWhitespace,
) -> (Vec<usize>, TeamCfg, Vec<usize>, Vec<String>) {
    let colors = it.next().map(parse_csv_usize).unwrap_or_default();
    let teams = TeamCfg {
        on: it.next() == Some("1"),
        bridge: it.next() == Some("1"),
        ff: it.next() == Some("1"),
        count: it.next().and_then(|v| v.parse().ok()).unwrap_or(2),
        hvb: it.next() == Some("1"),
        my_team: 0,
    };
    let team_v = it.next().map(parse_csv_usize).unwrap_or_default();
    let names = it.next().map(parse_names_csv).unwrap_or_default();
    (colors, teams, team_v, names)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_guest_game(
    seed: u64,
    players: usize,
    humans: usize,
    di: usize,
    seat: usize,
    colors: &[usize],
    teams: TeamCfg,
    team_v: Vec<usize>,
    names: Vec<String>,
) -> Game {
    let players = players.clamp(2, MAX_PLAYERS);
    let humans = humans.clamp(1, players);
    let mut teams = teams;
    teams.count = teams.count.clamp(2, MAX_TEAMS);
    let mut g = gen_map(seed % SEED_MOD, players, humans, idx_diff(di), teams);
    if team_v.len() == g.players {
        g.team = team_v.into_iter().map(|t| t % teams.count).collect();
    }
    g.names = names
        .into_iter()
        .take(humans)
        .map(|n| clean_name(&n))
        .collect();
    apply_colors(colors);
    if g.humans > 1 {
        apply_position_swap(&mut g, colors);
    }
    g.net_guest = true;
    g.my_seat = seat.min(players - 1);
    g
}

fn r_lobby_prev() -> Rect {
    Rect::new(COL_L - 177.0, 286.0, 44.0, 40.0)
}

fn r_lobby_seed() -> Rect {
    Rect::new(COL_L - 125.0, 286.0, 172.0, 40.0)
}

fn r_lobby_next() -> Rect {
    Rect::new(COL_L + 55.0, 286.0, 44.0, 40.0)
}

fn r_lobby_new() -> Rect {
    Rect::new(COL_L + 107.0, 286.0, 70.0, 40.0)
}

fn r_lobby_player(k: usize) -> Rect {
    Rect::new(COL_L - 130.0 + k as f32 * 45.0, 356.0, 45.0, 38.0)
}

fn r_lobby_diff(k: usize) -> Rect {
    Rect::new(COL_L - 130.0 + k as f32 * 119.0, 404.0, 119.0, 38.0)
}

fn r_lobby_mode(k: usize) -> Rect {
    Rect::new(COL_L - 130.0 + k as f32 * 178.0, 452.0, 178.0, 38.0)
}

fn r_lobby_count(k: usize) -> Rect {
    if k < 3 {
        Rect::new(COL_L - 130.0 + k as f32 * 60.0, 500.0, 60.0, 38.0)
    } else {
        Rect::new(COL_L + 50.0, 500.0, 177.0, 38.0)
    }
}

fn r_lobby_team(k: usize) -> Rect {
    Rect::new(COL_L - 130.0 + k as f32 * 64.0, 548.0, 64.0, 38.0)
}

fn r_lobby_ff() -> Rect {
    Rect::new(COL_L - 130.0, 598.0, 170.0, 24.0)
}

fn r_lobby_bridge() -> Rect {
    Rect::new(COL_L + 40.0, 598.0, 200.0, 24.0)
}

fn r_lobby_copy() -> Rect {
    Rect::new(COL_R + 104.0, 152.0, 72.0, 26.0)
}

fn r_lobby_name() -> Rect {
    Rect::new(COL_R - 160.0, 212.0, 320.0, 40.0)
}

fn r_lobby_swatch(i: usize) -> Rect {
    Rect::new(COL_R - 150.0 + i as f32 * 34.0, 288.0, 28.0, 22.0)
}

fn r_guest_team(k: usize, count: usize) -> Rect {
    let w = count as f32 * 48.0 - 8.0;
    Rect::new(COL_R - w * 0.5 + k as f32 * 48.0, 348.0, 40.0, 30.0)
}

fn r_lobby_start() -> Rect {
    Rect::new(WIN_W * 0.5 - 160.0, 700.0, 320.0, 62.0)
}

fn r_lobby_cancel() -> Rect {
    Rect::new(WIN_W * 0.5 - 70.0, 772.0, 140.0, 36.0)
}

// Join screen: name on top, LAN games in the middle, a direct address below.
const JOIN_X: f32 = WIN_W * 0.5 - 250.0;

const JOIN_W: f32 = 500.0;

fn r_join_name() -> Rect {
    Rect::new(WIN_W * 0.5 - 150.0, 212.0, 300.0, 44.0)
}

fn r_join_lan_row(i: usize) -> Rect {
    Rect::new(JOIN_X, 312.0 + i as f32 * 52.0, JOIN_W, 46.0)
}

fn r_join_addr() -> Rect {
    Rect::new(JOIN_X, 560.0, 250.0, 44.0)
}

fn r_join_paste() -> Rect {
    Rect::new(JOIN_X + 258.0, 560.0, 74.0, 44.0)
}

fn r_join_code() -> Rect {
    Rect::new(JOIN_X + 340.0, 560.0, JOIN_W - 340.0, 44.0)
}

fn r_join_go() -> Rect {
    Rect::new(JOIN_X, 626.0, JOIN_W * 0.5 - 6.0, 48.0)
}

fn r_join_back() -> Rect {
    Rect::new(JOIN_X + JOIN_W * 0.5 + 6.0, 626.0, JOIN_W * 0.5 - 6.0, 48.0)
}

// the game-settings rows both lobbies show: the host clicks them, guests
// see the exact same picture read-only
fn draw_lobby_settings(
    players: usize,
    diff: Difficulty,
    cfg: TeamCfg,
    my_team: Option<usize>,
    ui: &Ui,
    m: Vec2,
    live: bool,
) {
    let nums = ["2", "3", "4", "5", "6", "7", "8", "9"];
    let row: Vec<(Rect, &str, bool)> = (0..MAX_PLAYERS - 1)
        .map(|k| (r_lobby_player(k), nums[k], players == k + 2))
        .collect();
    seg_row(ui, m, live, "PLAYERS", &row, 18.0);
    let row: Vec<(Rect, &str, bool)> = [
        ("EASY", Difficulty::Easy),
        ("NORMAL", Difficulty::Normal),
        ("HARD", Difficulty::Hard),
    ]
    .into_iter()
    .enumerate()
    .map(|(k, (l, d))| (r_lobby_diff(k), l, diff == d))
    .collect();
    seg_row(ui, m, live, "DIFFICULTY", &row, 15.0);
    let row = vec![
        (r_lobby_mode(0), "FREE-FOR-ALL", !cfg.on),
        (r_lobby_mode(1), "TEAMS", cfg.on),
    ];
    seg_row(ui, m, live, "MODE", &row, 15.0);
    if cfg.on {
        let row: Vec<(Rect, &str, bool)> = ["2", "3", "4", "HUMANS VS BOTS"]
            .into_iter()
            .enumerate()
            .map(|(k, l)| {
                let sel = if k < 3 {
                    !cfg.hvb && cfg.count == k + 2
                } else {
                    cfg.hvb
                };
                (r_lobby_count(k), l, sel)
            })
            .collect();
        seg_row(ui, m, live, "TEAMS", &row, 13.0);
        if let Some(mine) = my_team {
            if !cfg.hvb {
                let count = cfg.count.clamp(2, MAX_TEAMS);
                let row: Vec<(Rect, &str, bool)> = (0..count)
                    .map(|k| (r_lobby_team(k), team_letter(k), mine.min(count - 1) == k))
                    .collect();
                seg_row(ui, m, live, "YOUR TEAM", &row, 15.0);
            }
        }
        for (r, label, on) in [
            (r_lobby_ff(), "FRIENDLY FIRE", cfg.ff),
            (r_lobby_bridge(), "ISLAND LINKS", cfg.bridge),
        ] {
            draw_toggle(r, label, on, ui, if live { m } else { vec2(-1.0, -1.0) });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_host_lobby(
    h: &HostNet,
    menu: &Menu,
    settings: &Settings,
    picks: &LobbyPicks,
    ui: &Ui,
    name_edit: bool,
    time: f32,
    copied_t: f32,
) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    let m = mouse_virtual();
    ui.text_centered("HOSTING", WIN_W * 0.5, 52.0, 32.0, INK());
    draw_icon_bar(settings, ui, m);

    // ---- left: the map and every game setting
    let pv = Rect::new(COL_L - 230.0, 74.0, 460.0, 190.0);
    draw_round_frame(pv.x, pv.y, pv.w, pv.h, 14.0, 2.0, SRF(), CARD_EDGE());
    draw_map_mini(
        &menu.preview,
        Rect::new(pv.x + 8.0, pv.y + 8.0, pv.w - 16.0, pv.h - 16.0),
    );
    let (rp, rs, rn, rr) = (
        r_lobby_prev(),
        r_lobby_seed(),
        r_lobby_next(),
        r_lobby_new(),
    );
    menu_button(rp, SRF(), CARD_EDGE(), rp.contains(m));
    draw_poly(
        rp.x + rp.w * 0.5 - 1.5,
        rp.y + rp.h * 0.5,
        3,
        8.0,
        180.0,
        INK(),
    );
    menu_button(rs, SRF(), CARD_EDGE(), false);
    ui.text_centered(
        &format!("{:08}", menu.seed),
        rs.x + rs.w * 0.5,
        rs.y + 27.0,
        19.0,
        INK(),
    );
    menu_button(rn, SRF(), CARD_EDGE(), rn.contains(m));
    draw_poly(
        rn.x + rn.w * 0.5 + 1.5,
        rn.y + rn.h * 0.5,
        3,
        8.0,
        0.0,
        INK(),
    );
    menu_button(rr, SRF(), CARD_EDGE(), rr.contains(m));
    ui.text_centered("NEW", rr.x + rr.w * 0.5, rr.y + 26.0, 15.0, INK());

    draw_lobby_settings(
        menu.players,
        menu.difficulty,
        settings.team_cfg(),
        Some(settings.team_mine),
        ui,
        m,
        true,
    );

    // ---- right: room info, identity, and who is here
    ui.text_centered("ROOM CODE", COL_R, 92.0, 13.0, with_alpha(INK(), 0.72));
    ui.text_centered(&h.code, COL_R, 140.0, 44.0, INK());
    ui.text(
        &format!("{}:{}", local_ip(), net_port()),
        COL_R - 160.0,
        172.0,
        15.0,
        with_alpha(INK(), 0.7),
    );
    let rc2 = r_lobby_copy();
    menu_button(rc2, SRF(), CARD_EDGE(), rc2.contains(m));
    ui.text_centered(
        if copied_t > 0.0 { "COPIED!" } else { "COPY" },
        rc2.x + rc2.w * 0.5,
        rc2.y + 18.0,
        12.0,
        INK(),
    );
    let occ = h.occupied();
    let cfg = settings.team_cfg();
    let (team_v, color_v) = lobby_layout(&occ, menu.players, cfg, picks, settings.color);
    let mut names: Vec<String> = vec![settings.name.clone()];
    for &seat in &occ {
        names.push(picks.name.get(&seat).cloned().unwrap_or_default());
    }
    draw_lobby_shared(
        ui,
        m,
        settings.name.as_str(),
        name_edit,
        settings.color,
        time,
        &LobbyRoster {
            players: menu.players,
            humans: occ.len() + 1,
            you_row: 0,
            host_label: "(you)",
            cfg,
            team_v: &team_v,
            color_v: &color_v,
            names: &names,
            my_team_sel: if cfg.on && !cfg.hvb {
                Some(cfg.my_team)
            } else {
                None
            },
            show_team_picker: false,
        },
    );

    let joined = occ.len();
    // The lobby is announced on the local network; a live dot says so.
    let status = format!(
        "{} player{} here  ·  open to your local network",
        joined + 1,
        if joined == 0 { "" } else { "s" },
    );
    let sw = ui.width(&status, 15.0);
    let (dx, dy) = (WIN_W * 0.5 - sw * 0.5 - 14.0, 683.0);
    if !REDUCED_MOTION.load(Ordering::Relaxed) {
        let p = (time * 0.8).fract();
        draw_circle(
            dx,
            dy,
            4.0 + 9.0 * p,
            with_alpha(palette(3).dark, 0.3 * (1.0 - p)),
        );
    }
    draw_circle(dx, dy, 4.0, palette(3).dark);
    ui.text_centered(
        &status,
        WIN_W * 0.5 + 7.0,
        688.0,
        15.0,
        with_alpha(INK(), 0.72),
    );
    let rst = r_lobby_start();
    menu_button(rst, palette(HUMAN).mid, BORDER(), rst.contains(m));
    ui.text_centered(
        "START GAME",
        rst.x + rst.w * 0.5,
        rst.y + 40.0,
        25.0,
        on_color(palette(HUMAN).mid),
    );
    let rc = r_lobby_cancel();
    menu_button(rc, SRF(), CARD_EDGE(), rc.contains(m));
    ui.text_centered("CANCEL", rc.x + rc.w * 0.5, rc.y + 24.0, 15.0, INK());
}

// the right-hand column both lobbies share: name box, color band, roster
struct LobbyRoster<'a> {
    pub(crate) players: usize,
    pub(crate) humans: usize,
    pub(crate) you_row: usize,
    pub(crate) host_label: &'a str,
    pub(crate) cfg: TeamCfg,
    pub(crate) team_v: &'a [usize],
    pub(crate) color_v: &'a [usize],
    pub(crate) names: &'a [String],
    pub(crate) my_team_sel: Option<usize>,
    pub(crate) show_team_picker: bool,
}

fn draw_lobby_shared(
    ui: &Ui,
    m: Vec2,
    my_name: &str,
    name_edit: bool,
    my_color: usize,
    time: f32,
    r: &LobbyRoster,
) {
    ui.text_centered("YOUR NAME", COL_R, 204.0, 12.0, with_alpha(INK(), 0.72));
    let rn = r_lobby_name();
    menu_button(
        rn,
        SRF(),
        if name_edit { ACCENT() } else { CARD_EDGE() },
        rn.contains(m),
    );
    let caret = if name_edit && (time * 2.5) as i32 % 2 == 0 {
        "|"
    } else {
        ""
    };
    let shown = if my_name.is_empty() && !name_edit {
        "click to set a name".to_string()
    } else {
        format!("{}{}", my_name, caret)
    };
    ui.text_centered(
        &shown,
        rn.x + rn.w * 0.5,
        rn.y + 26.0,
        17.0,
        if my_name.is_empty() && !name_edit {
            with_alpha(INK(), 0.4)
        } else {
            INK()
        },
    );

    ui.text_centered("YOUR COLOR", COL_R, 280.0, 12.0, with_alpha(INK(), 0.72));
    // colors already claimed by other humans are crossed out
    let taken: Vec<usize> = (0..r.humans)
        .filter(|&row| row != r.you_row)
        .filter_map(|row| r.color_v.get(row).copied())
        .collect();
    for i in 0..MAX_PLAYERS {
        let rc = r_lobby_swatch(i);
        let base = base_color(i);
        let sel = my_color == i;
        if sel {
            draw_round_frame(
                rc.x - 3.0,
                rc.y - 3.0,
                rc.w + 6.0,
                rc.h + 6.0,
                8.0,
                2.0,
                SRF(),
                BORDER(),
            );
        }
        let hov = rc.contains(m) && !taken.contains(&i);
        draw_round_rect(rc.x, rc.y, rc.w, rc.h, 6.0, control_fill(base, hov));
        if taken.contains(&i) {
            draw_line(
                rc.x + 4.0,
                rc.y + rc.h - 4.0,
                rc.x + rc.w - 4.0,
                rc.y + 4.0,
                2.5,
                SRF(),
            );
        }
    }

    if r.show_team_picker && r.cfg.on && !r.cfg.hvb {
        ui.text_centered("YOUR TEAM", COL_R, 340.0, 12.0, with_alpha(INK(), 0.72));
        let count = r.cfg.count.clamp(2, MAX_TEAMS);
        for k in 0..count {
            let rt = r_guest_team(k, count);
            let sel = r.my_team_sel == Some(k);
            let fill = if sel { palette(HUMAN).fill } else { SRF() };
            menu_button(
                rt,
                fill,
                if sel { BORDER() } else { CARD_EDGE() },
                rt.contains(m),
            );
            ui.text_centered(
                team_letter(k),
                rt.x + rt.w * 0.5,
                rt.y + 21.0,
                15.0,
                if sel { INK_ON_FILL } else { INK() },
            );
        }
    }

    let head = if r.cfg.on {
        if r.cfg.hvb {
            "PLAYERS — HUMANS VS BOTS"
        } else {
            "PLAYERS"
        }
    } else {
        "PLAYERS"
    };
    let ry0 = if r.show_team_picker { 412.0 } else { 344.0 };
    ui.text_centered(head, COL_R, ry0, 12.0, with_alpha(INK(), 0.72));
    let x0 = COL_R - 160.0;
    for k in 0..r.players {
        let ry = ry0 + 24.0 + k as f32 * 31.0;
        let slot = r.color_v.get(k).copied().unwrap_or(k);
        let base = base_color(slot);
        draw_round_rect(x0, ry - 13.0, 24.0, 18.0, 6.0, base);
        draw_symbol(x0 + 40.0, ry - 4.0, 7.0, k, darken(base, 0.7));
        let label = if k == 0 {
            let n = r.names.first().cloned().unwrap_or_default();
            let n = if n.is_empty() { "P1".to_string() } else { n };
            format!("{} {}", n, r.host_label)
        } else if k < r.humans {
            let n = r.names.get(k).cloned().unwrap_or_default();
            let n = if n.is_empty() {
                format!("P{}", k + 1)
            } else {
                n
            };
            if k == r.you_row {
                format!("{} (you)", n)
            } else {
                n
            }
        } else {
            "AI".to_string()
        };
        ui.text(&label, x0 + 58.0, ry, 15.0, INK());
        if r.cfg.on {
            ui.text(
                team_letter(r.team_v.get(k).copied().unwrap_or(0)),
                x0 + 296.0,
                ry,
                14.0,
                with_alpha(INK(), 0.72),
            );
        }
    }
}

fn draw_guest_lobby(
    j: &JoinUi,
    my_seat: usize,
    settings: &Settings,
    ui: &Ui,
    name_edit: bool,
    time: f32,
) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    draw_icon_bar(settings, ui, mouse_virtual());
    let m = mouse_virtual();
    ui.text_centered("LOBBY", WIN_W * 0.5, 52.0, 32.0, INK());
    let pv = Rect::new(COL_L - 230.0, 74.0, 460.0, 190.0);
    draw_round_frame(pv.x, pv.y, pv.w, pv.h, 14.0, 2.0, SRF(), CARD_EDGE());
    if let Some(g) = &j.preview {
        draw_map_mini(
            g,
            Rect::new(pv.x + 8.0, pv.y + 8.0, pv.w - 16.0, pv.h - 16.0),
        );
    } else {
        ui.text_centered(
            "Waiting for the host's map...",
            COL_L,
            pv.y + 100.0,
            15.0,
            with_alpha(INK(), 0.5),
        );
    }
    if let Some(v) = &j.view {
        ui.text_centered(
            &format!("MAP #{:08}", j.preview_key.0),
            COL_L,
            314.0,
            17.0,
            with_alpha(INK(), 0.7),
        );
        let cfg = TeamCfg {
            on: v.ton,
            count: v.tcount,
            hvb: v.thvb,
            my_team: 0,
            bridge: v.tbridge,
            ff: v.tff,
        };
        draw_lobby_settings(v.players, idx_diff(v.diff), cfg, None, ui, m, false);
        let own_row = 1 + v.occupied.iter().position(|&x| x == my_seat).unwrap_or(0);
        let my_color = j
            .my_color
            .or_else(|| v.colors.get(own_row).copied())
            .unwrap_or(settings.color);
        let my_team = j.my_team.or_else(|| v.teams.get(own_row).copied());
        draw_lobby_shared(
            ui,
            m,
            settings.name.as_str(),
            name_edit,
            my_color,
            time,
            &LobbyRoster {
                players: v.players,
                humans: v.occupied.len() + 1,
                you_row: own_row,
                host_label: "(host)",
                cfg: TeamCfg {
                    on: v.ton,
                    count: v.tcount,
                    hvb: v.thvb,
                    my_team: 0,
                    bridge: v.tbridge,
                    ff: v.tff,
                },
                team_v: &v.teams,
                color_v: &v.colors,
                names: &v.names,
                my_team_sel: my_team,
                show_team_picker: true,
            },
        );
    } else {
        ui.text_centered(
            "Connected — syncing with the host...",
            COL_R,
            300.0,
            15.0,
            with_alpha(INK(), 0.72),
        );
    }
    let dots = ".".repeat(1 + (time * 2.0) as usize % 3);
    ui.text_centered(
        &format!("Waiting for the host to start{}", dots),
        WIN_W * 0.5,
        738.0,
        16.0,
        with_alpha(INK(), 0.72),
    );
    let rc = r_lobby_cancel();
    menu_button(rc, SRF(), CARD_EDGE(), rc.contains(m));
    ui.text_centered("LEAVE", rc.x + rc.w * 0.5, rc.y + 24.0, 15.0, INK());
}

fn draw_join_lobby(j: &JoinUi, lan: Option<&LanScan>, settings: &Settings, ui: &Ui, time: f32) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    ui.text_centered("JOIN GAME", WIN_W * 0.5, 150.0, 40.0, INK());
    draw_icon_bar(settings, ui, mouse_virtual());
    let m = mouse_virtual();
    let caret = |on: bool| {
        if on && (time * 2.5) as i32 % 2 == 0 {
            "|"
        } else {
            ""
        }
    };
    let label = |text: &str, x: f32, y: f32| ui.text(text, x, y, 12.0, with_alpha(INK(), 0.62));
    let field = |r: Rect, focused: bool, text: &str, size: f32| {
        let hovered = r.contains(m);
        draw_round_frame(
            r.x,
            r.y,
            r.w,
            r.h,
            12.0,
            2.0,
            control_fill(SRF(), hovered && !focused),
            if focused { ACCENT() } else { CARD_EDGE() },
        );
        ui.text_fitted(
            text,
            r.x + 16.0,
            r.y + r.h * 0.5 + size * 0.36,
            size,
            r.w - 28.0,
            INK(),
        );
    };

    // ---- who you are
    let rn = r_join_name();
    label("YOUR NAME", rn.x, rn.y - 10.0);
    field(
        rn,
        j.focus == 2,
        &format!("{}{}", settings.name, caret(j.focus == 2)),
        18.0,
    );

    // ---- games found on the local network
    label("GAMES ON YOUR NETWORK", JOIN_X, 298.0);
    let games = lan.map_or(&[][..], |l| &l.games[..]);
    let list = Rect::new(JOIN_X, 306.0, JOIN_W, LAN_MAX_GAMES as f32 * 52.0 + 6.0);
    draw_round_frame(
        list.x,
        list.y,
        list.w,
        list.h,
        16.0,
        1.5,
        mix(SRF(), BG(), 0.6),
        CARD_EDGE(),
    );
    if games.is_empty() {
        let dots = ".".repeat(1 + (time * 2.0) as usize % 3);
        let (title, hint) = if lan.is_some_and(LanScan::listening) {
            (
                format!("Looking for games{dots}"),
                "Games appear here when someone on your network hosts one.",
            )
        } else {
            (
                "LAN search unavailable".to_string(),
                "Another copy is already listening. Enter an address below.",
            )
        };
        // A slow pulse ring signals the search is live.
        let (cx, cy) = (WIN_W * 0.5, list.y + 62.0);
        if !REDUCED_MOTION.load(Ordering::Relaxed) {
            let p = (time * 0.8).fract();
            draw_circle(
                cx,
                cy,
                8.0 + 18.0 * p,
                with_alpha(ACCENT(), 0.22 * (1.0 - p)),
            );
        }
        draw_circle(cx, cy, 6.0, with_alpha(ACCENT(), 0.85));
        ui.text_centered(&title, WIN_W * 0.5, list.y + 112.0, 17.0, INK());
        ui.text_centered(
            hint,
            WIN_W * 0.5,
            list.y + 138.0,
            12.5,
            with_alpha(INK(), 0.6),
        );
    }
    for (i, g) in games.iter().enumerate() {
        let r = r_join_lan_row(i);
        let hovered = r.contains(m) && j.connect_rx.is_none();
        draw_round_frame(
            r.x + 6.0,
            r.y,
            r.w - 12.0,
            r.h,
            12.0,
            1.5,
            control_fill(SRF(), hovered),
            if hovered { ACCENT() } else { CARD_EDGE() },
        );
        draw_circle(r.x + 30.0, r.y + r.h * 0.5, 5.0, palette(3).dark);
        ui.text_fitted(
            &format!("{}'s game", g.name),
            r.x + 46.0,
            r.y + 21.0,
            16.0,
            330.0,
            INK(),
        );
        ui.text_fitted(
            &format!("{} of {} seats taken  ·  {}", g.humans, g.players, g.addr),
            r.x + 46.0,
            r.y + 38.0,
            11.5,
            330.0,
            with_alpha(INK(), 0.62),
        );
        let jb = Rect::new(r.x + r.w - 84.0, r.y + 8.0, 66.0, 30.0);
        draw_round_rect(
            jb.x,
            jb.y,
            jb.w,
            jb.h,
            10.0,
            control_fill(palette(HUMAN).fill, hovered),
        );
        ui.text_centered("JOIN", jb.x + jb.w * 0.5, jb.y + 20.0, 13.0, INK_ON_FILL);
    }

    // ---- direct connection
    let ra = r_join_addr();
    label("OR ENTER AN ADDRESS", JOIN_X, ra.y - 10.0);
    let rc = r_join_code();
    label("ROOM CODE", rc.x, rc.y - 10.0);
    let addr_text = if j.addr.is_empty() && j.focus != 0 {
        String::new()
    } else {
        format!("{}{}", j.addr, caret(j.focus == 0))
    };
    field(ra, j.focus == 0, &addr_text, 17.0);
    if j.addr.is_empty() && j.focus != 0 {
        ui.text(
            "192.168.1.20",
            ra.x + 16.0,
            ra.y + 28.0,
            16.0,
            with_alpha(INK(), 0.3),
        );
    }
    let rpst = r_join_paste();
    menu_button(rpst, SRF(), CARD_EDGE(), rpst.contains(m));
    ui.text_centered("PASTE", rpst.x + rpst.w * 0.5, rpst.y + 27.0, 13.0, INK());
    field(
        rc,
        j.focus == 1,
        &format!("{}{}", j.code, caret(j.focus == 1)),
        19.0,
    );
    let rg = r_join_go();
    menu_button(
        rg,
        palette(HUMAN).fill,
        darken(palette(HUMAN).fill, 0.8),
        rg.contains(m),
    );
    ui.text_centered("CONNECT", rg.x + rg.w * 0.5, rg.y + 31.0, 17.0, INK_ON_FILL);
    let rb = r_join_back();
    menu_button(rb, SRF(), CARD_EDGE(), rb.contains(m));
    ui.text_centered("BACK", rb.x + rb.w * 0.5, rb.y + 31.0, 17.0, INK());
    if !j.status.is_empty() {
        ui.text_centered(&j.status, WIN_W * 0.5, 712.0, 15.0, with_alpha(INK(), 0.75));
    }
}

impl App {
    // One frame of the host's lobby.
    pub(crate) fn run_host_lobby(&mut self, dt: f32) {
        self.menu_time += dt;
        let click2 = is_mouse_button_pressed(MouseButton::Left);
        let m2 = mouse_virtual();
        let mut cancel = is_key_pressed(KeyCode::Escape) && !self.lobby_name_edit;
        if click2 && r_lobby_cancel().contains(m2) {
            cancel = true;
        }
        let h = self.host.as_mut().unwrap();
        let mut dirty = false;
        for _ in 0..NET_MESSAGES_PER_FRAME {
            let Ok(msg) = h.rx.try_recv() else { break };
            match msg {
                HostMsg::Joined(seat, name) => {
                    if !name.is_empty() {
                        self.lobby_picks.name.insert(seat, name);
                    }
                    // hand the newcomer the first color still free
                    let mut used = vec![self.settings.color];
                    used.extend(self.lobby_picks.color.values().copied());
                    if let Some(c) = (0..MAX_PLAYERS).find(|c| !used.contains(c)) {
                        self.lobby_picks.color.entry(seat).or_insert(c);
                    }
                    dirty = true;
                }
                HostMsg::Left(seat) => {
                    self.lobby_picks.team.remove(&seat);
                    self.lobby_picks.color.remove(&seat);
                    self.lobby_picks.name.remove(&seat);
                    dirty = true;
                }
                HostMsg::Intent(seat, line) => {
                    let mut it = line.split_whitespace();
                    match it.next() {
                        Some("TEAM") => {
                            if let Some(t) = it.next().and_then(|v| v.parse::<usize>().ok()) {
                                let n = self.settings.team_count.clamp(2, MAX_TEAMS);
                                self.lobby_picks.team.insert(seat, t % n);
                                dirty = true;
                            }
                        }
                        Some("COLOR") => {
                            if let Some(c) = it.next().and_then(|v| v.parse::<usize>().ok()) {
                                let taken = c == self.settings.color
                                    || self
                                        .lobby_picks
                                        .color
                                        .iter()
                                        .any(|(&s2, &c2)| s2 != seat && c2 == c);
                                if c < MAX_PLAYERS && !taken {
                                    self.lobby_picks.color.insert(seat, c);
                                    dirty = true;
                                }
                            }
                        }
                        Some("NAME") => {
                            let n = clean_name(it.next().unwrap_or(""));
                            if n.is_empty() {
                                self.lobby_picks.name.remove(&seat);
                            } else {
                                self.lobby_picks.name.insert(seat, n);
                            }
                            dirty = true;
                        }
                        _ => {}
                    }
                }
                HostMsg::Rejoined(_) => {}
            }
        }
        self.copied_t = (self.copied_t - dt).max(0.0);
        if self.lobby_name_edit {
            while let Some(c) = get_char_pressed() {
                if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && self.settings.name.len() < 12
                {
                    self.settings.name.push(c);
                    dirty = true;
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.settings.name.pop();
                dirty = true;
            }
            if is_key_pressed(KeyCode::Enter)
                || is_key_pressed(KeyCode::Escape)
                || (click2 && !r_lobby_name().contains(m2))
            {
                self.lobby_name_edit = false;
                save_settings(&self.settings);
            }
        } else {
            while get_char_pressed().is_some() {}
        }
        if click2 {
            // Global toolbar was processed before this frame was painted.
            if r_lobby_copy().contains(m2) {
                copy_to_clipboard(&format!("{}:{}", local_ip(), net_port()));
                self.copied_t = 1.5;
                self.snd_queue.push(Snd::Click);
            }
            if r_lobby_name().contains(m2) {
                self.lobby_name_edit = true;
            }
            if r_lobby_prev().contains(m2) {
                self.menu.seed = (self.menu.seed + SEED_MOD - 1) % SEED_MOD;
                dirty = true;
                self.snd_queue.push(Snd::Click);
            }
            if r_lobby_next().contains(m2) {
                self.menu.seed = (self.menu.seed + 1) % SEED_MOD;
                dirty = true;
                self.snd_queue.push(Snd::Click);
            }
            if r_lobby_new().contains(m2) {
                self.menu.seed = random_seed();
                dirty = true;
                self.snd_queue.push(Snd::Click);
            }
            for k in 0..MAX_PLAYERS - 1 {
                if r_lobby_player(k).contains(m2) {
                    let n = h.resize_guests(k + 1);
                    self.menu.players = n + 1;
                    self.settings.players = self.menu.players;
                    save_settings(&self.settings);
                    dirty = true;
                    self.snd_queue.push(Snd::Click);
                }
            }
            for (k, d) in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
                .into_iter()
                .enumerate()
            {
                if r_lobby_diff(k).contains(m2) {
                    self.menu.difficulty = d;
                    self.settings.difficulty = diff_idx(d);
                    save_settings(&self.settings);
                    dirty = true;
                    self.snd_queue.push(Snd::Click);
                }
            }
            for k in 0..2 {
                if r_lobby_mode(k).contains(m2) {
                    self.settings.teams = k == 1;
                    save_settings(&self.settings);
                    dirty = true;
                    self.snd_queue.push(Snd::Click);
                }
            }
            if self.settings.teams {
                for k in 0..4 {
                    if r_lobby_count(k).contains(m2) {
                        if k < 3 {
                            self.settings.team_hvb = false;
                            self.settings.team_count = k + 2;
                            self.settings.team_mine = self.settings.team_mine.min(k + 1);
                        } else {
                            self.settings.team_hvb = !self.settings.team_hvb;
                        }
                        save_settings(&self.settings);
                        dirty = true;
                        self.snd_queue.push(Snd::Click);
                    }
                }
                if !self.settings.team_hvb {
                    let count = self.settings.team_count.clamp(2, MAX_TEAMS);
                    for k in 0..count {
                        if r_lobby_team(k).contains(m2) {
                            self.settings.team_mine = k;
                            save_settings(&self.settings);
                            dirty = true;
                            self.snd_queue.push(Snd::Click);
                        }
                    }
                }
                if r_lobby_ff().contains(m2) {
                    self.settings.team_ff = !self.settings.team_ff;
                    save_settings(&self.settings);
                    dirty = true;
                    self.snd_queue.push(Snd::Click);
                }
                if r_lobby_bridge().contains(m2) {
                    self.settings.team_bridge = !self.settings.team_bridge;
                    save_settings(&self.settings);
                    dirty = true;
                    self.snd_queue.push(Snd::Click);
                }
            }
            for i in 0..MAX_PLAYERS {
                if r_lobby_swatch(i).contains(m2) {
                    let taken = self.lobby_picks.color.values().any(|&c| c == i);
                    if !taken {
                        self.settings.color = i;
                        apply_local_colors(i);
                        save_settings(&self.settings);
                        dirty = true;
                        self.snd_queue.push(Snd::Click);
                    }
                }
            }
        }
        // hold-to-repeat on the seed arrows
        let held_dir: i64 = if is_mouse_button_down(MouseButton::Left) {
            if r_lobby_prev().contains(m2) {
                -1
            } else if r_lobby_next().contains(m2) {
                1
            } else {
                0
            }
        } else {
            0
        };
        if held_dir != 0 {
            self.menu.hold_t += dt;
            if self.menu.hold_t > 0.4 {
                self.menu.hold_next -= dt;
                if self.menu.hold_next <= 0.0 {
                    self.menu.hold_next = 0.08;
                    self.menu.seed =
                        (self.menu.seed as i64 + held_dir).rem_euclid(SEED_MOD as i64) as u64;
                    dirty = true;
                }
            }
        } else {
            self.menu.hold_t = 0.0;
            self.menu.hold_next = 0.0;
        }
        // keep every guest's lobby view fresh
        self.lobby_sync_t += dt;
        if dirty {
            // the preview shows who plays where under the current colors
            let occ = h.occupied();
            let (_tv, cv) = lobby_layout(
                &occ,
                self.menu.players,
                self.settings.team_cfg(),
                &self.lobby_picks,
                self.settings.color,
            );
            let mut g = gen_map(
                self.menu.seed,
                self.menu.players,
                1,
                self.menu.difficulty,
                TeamCfg::default(),
            );
            apply_position_swap(&mut g, &cv);
            self.menu.preview = g;
        }
        if dirty || self.lobby_sync_t >= 1.5 {
            self.lobby_sync_t = 0.0;
            let line = lobby_line(h, &self.menu, &self.settings, &self.lobby_picks);
            h.broadcast(&line);
        }
        let want_start = (click2 && r_lobby_start().contains(m2))
            || (is_key_pressed(KeyCode::Enter) && !self.lobby_name_edit)
            || (self.test_host_auto > 0 && h.joined() >= self.test_host_auto);
        if want_start {
            let cfg = self.settings.team_cfg();
            let (humans, tv, cv, nm) = h.start_game(
                self.menu.seed,
                self.menu.players,
                self.menu.difficulty,
                cfg,
                &self.lobby_picks,
                self.settings.color,
                &self.settings.name,
            );
            let mut ng = gen_map(
                self.menu.seed,
                self.menu.players,
                humans,
                self.menu.difficulty,
                cfg,
            );
            ng.team = tv;
            ng.names = nm;
            apply_colors(&cv);
            if ng.humans > 1 {
                apply_position_swap(&mut ng, &cv);
            }
            h.sent = 0;
            self.game = Some(ng);
            self.screen = Screen::Play;
            self.snd_queue.push(Snd::Turn);
        }
        draw_host_lobby(
            h,
            &self.menu,
            &self.settings,
            &self.lobby_picks,
            &self.ui,
            self.lobby_name_edit,
            self.menu_time,
            self.copied_t,
        );
        if cancel {
            self.host = None;
            self.lobby_name_edit = false;
            self.menu.regen(); // back to the solo color layout
            self.screen = Screen::Menu;
        }
    }

    // One frame of the join screen and, once connected, the guest's lobby.
    pub(crate) fn run_join_lobby(&mut self, dt: f32) {
        self.menu_time += dt;
        let mut back = is_key_pressed(KeyCode::Escape) && !self.lobby_name_edit;
        let mut drop_guest = false;
        let click = is_mouse_button_pressed(MouseButton::Left);
        let m = mouse_virtual();
        let paste_done = self
            .join
            .paste_rx
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
            });
        if let Some(text) = paste_done {
            self.join.paste_rx = None;
            if let Some(text) = text {
                let focus = self.join.paste_focus;
                apply_join_paste(&mut self.join, focus, &text);
            } else {
                self.join.status = "Could not read the clipboard".to_string();
            }
        }
        let connect_done = self
            .join
            .connect_rx
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Connection attempt failed".to_string()))
                }
            });
        if let Some(result) = connect_done {
            self.join.connect_rx = None;
            match result {
                Ok(mut g) => {
                    send_net_line(&mut g.stream, &format!("COLOR {}", self.settings.color));
                    send_net_line(&mut g.stream, &format!("TEAM {}", self.settings.team_mine));
                    self.guest = Some(g);
                    self.settings.last_addr = self.join.addr.clone();
                    save_settings(&self.settings);
                    self.join.status = "Connected — waiting for the host to start...".to_string();
                }
                Err(e) => self.join.status = e,
            }
        }
        if let Some(gn) = self.guest.as_mut() {
            for _ in 0..NET_MESSAGES_PER_FRAME {
                let Ok(l) = gn.rx.try_recv() else { break };
                let mut it = l.split_whitespace();
                match it.next() {
                    Some("START") => {
                        let seed: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                        let players: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                        let humans: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                        let di: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                        let seat = it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                        let (colors, teams, team_v, names) = parse_start_tail(&mut it);
                        let g = build_guest_game(
                            seed, players, humans, di, seat, &colors, teams, team_v, names,
                        );
                        gn.seat = g.my_seat;
                        self.game = Some(g);
                        self.screen = Screen::Play;
                        self.snd_queue.push(Snd::Turn);
                    }
                    Some("LOBBY") => {
                        let seed =
                            it.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) % SEED_MOD;
                        let players = it
                            .next()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(2)
                            .clamp(2, MAX_PLAYERS);
                        let diff = it
                            .next()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                            .min(2);
                        let ton = it.next() == Some("1");
                        let thvb = it.next() == Some("1");
                        let tcount = it
                            .next()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(2)
                            .clamp(2, MAX_TEAMS);
                        let tff = it.next() == Some("1");
                        let tbridge = it.next() == Some("1");
                        let mut occupied = it
                            .next()
                            .map(|x| {
                                if x == "-" {
                                    Vec::new()
                                } else {
                                    parse_csv_usize(x)
                                }
                            })
                            .unwrap_or_default();
                        occupied.retain(|&seat| seat > 0 && seat < players);
                        occupied.sort_unstable();
                        occupied.dedup();
                        let teams: Vec<usize> = it
                            .next()
                            .map(parse_csv_usize)
                            .unwrap_or_default()
                            .into_iter()
                            .take(players)
                            .map(|team| team % tcount)
                            .collect();
                        let colors: Vec<usize> = it
                            .next()
                            .map(parse_csv_usize)
                            .unwrap_or_default()
                            .into_iter()
                            .take(players)
                            .map(|color| color % MAX_PLAYERS)
                            .collect();
                        let names = it.next().map(parse_names_csv).unwrap_or_default();
                        if self.join.preview_key != (seed, players)
                            || self.join.preview_colors != colors
                        {
                            let mut pg =
                                gen_map(seed, players, 1, Difficulty::Normal, TeamCfg::default());
                            apply_position_swap(&mut pg, &colors);
                            self.join.preview = Some(pg);
                            self.join.preview_key = (seed, players);
                            self.join.preview_colors = colors.clone();
                        }
                        self.join.view = Some(LobbyView {
                            players,
                            diff,
                            ton,
                            thvb,
                            tcount,
                            tff,
                            tbridge,
                            occupied,
                            teams,
                            colors,
                            names,
                        });
                    }
                    Some("BYE") => {
                        self.join.status = "Disconnected by host".to_string();
                        drop_guest = true;
                    }
                    Some("LOST") => {
                        self.join.status = "Connection lost".to_string();
                        drop_guest = true;
                    }
                    _ => {}
                }
            }
            if self.lobby_name_edit {
                while let Some(c) = get_char_pressed() {
                    if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                        && self.settings.name.len() < 12
                    {
                        self.settings.name.push(c);
                    }
                }
                if is_key_pressed(KeyCode::Backspace) {
                    self.settings.name.pop();
                }
                if is_key_pressed(KeyCode::Enter)
                    || is_key_pressed(KeyCode::Escape)
                    || (click && !r_lobby_name().contains(m))
                {
                    self.lobby_name_edit = false;
                    save_settings(&self.settings);
                    let n = if self.settings.name.is_empty() {
                        "-".to_string()
                    } else {
                        self.settings.name.clone()
                    };
                    send_net_line(&mut gn.stream, &format!("NAME {}", n));
                }
            } else {
                while get_char_pressed().is_some() {}
            }
            if click {
                if r_lobby_name().contains(m) {
                    self.lobby_name_edit = true;
                }
                if let Some(v) = &self.join.view {
                    let own_row = 1 + v.occupied.iter().position(|&x| x == gn.seat).unwrap_or(0);
                    let humans = v.occupied.len() + 1;
                    for i in 0..MAX_PLAYERS {
                        if r_lobby_swatch(i).contains(m) {
                            let taken =
                                (0..humans).any(|r2| r2 != own_row && v.colors.get(r2) == Some(&i));
                            if !taken {
                                self.join.my_color = Some(i);
                                self.settings.color = i;
                                save_settings(&self.settings);
                                send_net_line(&mut gn.stream, &format!("COLOR {}", i));
                                self.snd_queue.push(Snd::Click);
                            }
                        }
                    }
                    if v.ton && !v.thvb {
                        let count = v.tcount.clamp(2, MAX_TEAMS);
                        for k in 0..count {
                            if r_guest_team(k, count).contains(m) {
                                self.join.my_team = Some(k);
                                self.settings.team_mine = k;
                                save_settings(&self.settings);
                                send_net_line(&mut gn.stream, &format!("TEAM {}", k));
                                self.snd_queue.push(Snd::Click);
                            }
                        }
                    }
                }
                if r_lobby_cancel().contains(m) {
                    back = true;
                }
            }
        } else {
            let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
            let mut paste = ctrl && is_key_pressed(KeyCode::V);
            if click && r_join_paste().contains(m) {
                paste = true;
            }
            if paste {
                if let Some(t) = paste_from_clipboard() {
                    let focus = self.join.focus;
                    apply_join_paste(&mut self.join, focus, &t);
                } else if self.join.paste_rx.is_none() {
                    self.join.paste_focus = self.join.focus;
                    self.join.paste_rx = Some(spawn_clipboard_paste());
                    if cfg!(target_os = "linux") {
                        self.join.status = "Reading clipboard...".to_string();
                    }
                }
            }
            while let Some(c) = get_char_pressed() {
                if ctrl {
                    continue; // don't type the 'v' from ctrl+v
                }
                if self.join.focus == 1 {
                    if c.is_ascii_digit() && self.join.code.len() < NET_ROOM_CODE_LEN {
                        self.join.code.push(c);
                    }
                } else if self.join.focus == 2 {
                    if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                        && self.settings.name.len() < 12
                    {
                        self.settings.name.push(c);
                    }
                } else if (c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-')
                    && self.join.addr.len() < 40
                {
                    self.join.addr.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                match self.join.focus {
                    1 => {
                        self.join.code.pop();
                    }
                    2 => {
                        self.settings.name.pop();
                    }
                    _ => {
                        self.join.addr.pop();
                    }
                }
            }
            if is_key_pressed(KeyCode::Tab) {
                self.join.focus = (self.join.focus + 1) % 3;
            }
            if click && r_join_addr().contains(m) {
                self.join.focus = 0;
            }
            if click && r_join_code().contains(m) {
                self.join.focus = 1;
            }
            if click && r_join_name().contains(m) {
                self.join.focus = 2;
            }
            // One click on a LAN game fills in its address and code and connects.
            let picked = self.lan_scan.as_ref().and_then(|l| {
                l.games
                    .iter()
                    .enumerate()
                    .find(|(i, _)| click && r_join_lan_row(*i).contains(m))
                    .map(|(_, g)| g.clone())
            });
            if let Some(g) = picked.filter(|_| self.join.connect_rx.is_none()) {
                self.join.addr = g.addr;
                self.join.code = g.code;
                self.join.status = format!("Joining {}'s game...", g.name);
                self.join.connect_rx = Some(spawn_guest_connect(
                    self.join.addr.clone(),
                    self.join.code.clone(),
                    self.settings.name.clone(),
                ));
            } else if self.join.connect_rx.is_none()
                && ((click && r_join_go().contains(m)) || is_key_pressed(KeyCode::Enter))
            {
                self.join.status = "Connecting...".to_string();
                self.join.connect_rx = Some(spawn_guest_connect(
                    self.join.addr.clone(),
                    self.join.code.clone(),
                    self.settings.name.clone(),
                ));
            }
            if click && r_join_back().contains(m) {
                back = true;
            }
        }
        if drop_guest {
            self.guest = None;
            self.join.view = None;
        }
        if let Some(gn) = &self.guest {
            draw_guest_lobby(
                &self.join,
                gn.seat,
                &self.settings,
                &self.ui,
                self.lobby_name_edit,
                self.menu_time,
            );
        } else {
            draw_join_lobby(
                &self.join,
                self.lan_scan.as_ref(),
                &self.settings,
                &self.ui,
                self.menu_time,
            );
        }
        if back {
            self.guest = None;
            self.join.view = None;
            self.join.connect_rx = None;
            self.lobby_name_edit = false;
            self.screen = Screen::Menu;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::GLOBALS;

    use super::*;

    #[test]
    fn hostile_guest_setup_values_are_normalized() {
        let _g = GLOBALS.lock().unwrap();
        let teams = TeamCfg {
            on: true,
            count: usize::MAX,
            ..TeamCfg::default()
        };
        let g = build_guest_game(
            u64::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            &[usize::MAX; MAX_PLAYERS],
            teams,
            vec![usize::MAX; MAX_PLAYERS],
            vec!["name-with-invalid-characters!".to_string(); MAX_PLAYERS + 4],
        );
        assert_eq!(g.players, MAX_PLAYERS);
        assert_eq!(g.humans, MAX_PLAYERS);
        assert_eq!(g.my_seat, MAX_PLAYERS - 1);
        assert_eq!(g.names.len(), MAX_PLAYERS);
        assert!(g.team.iter().all(|&team| team < MAX_TEAMS));
        assert!(g.names.iter().all(|name| name.len() <= 12));
    }

    #[test]
    fn pasted_connection_fields_are_sanitized_and_bounded() {
        let mut join = JoinUi::default();
        apply_join_paste(&mut join, 1, "room: 12-34-56-789");
        assert_eq!(join.code, "123456");
        apply_join_paste(
            &mut join,
            0,
            " example.invalid:7777/ignored-and-way-too-long-for-the-field ",
        );
        assert_eq!(join.addr, "example.invalid:7777ignored-and-way-too-");
        assert!(join.addr.len() <= 40);
    }
}
