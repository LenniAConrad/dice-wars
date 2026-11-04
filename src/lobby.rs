//! Host, guest and join lobby screens.

use std::sync::atomic::Ordering;

use dicegame::engine::prelude::*;

use crate::board::{apply_position_swap, gen_map, random_seed};
use crate::clipboard::{copy_to_clipboard, paste_from_clipboard, spawn_clipboard_paste};
use crate::game::{
    diff_idx, idx_diff, Difficulty, Game, TeamCfg, HUMAN, MAX_PLAYERS, MAX_TEAMS, SEED_MOD,
};
use crate::lan::{LanScan, LAN_MAX_GAMES};
use crate::menu::Menu;
use crate::net::{
    lobby_layout, lobby_line, local_ip, net_port, parse_csv_usize, send_net_line,
    spawn_guest_connect, spawn_guest_connect_via_relay, GuestNet, HostMsg, HostNet, LobbyPicks,
    RelayStatus, NET_MESSAGES_PER_FRAME, NET_ROOM_CODE_LEN,
};
use crate::settings::{clean_name, clean_relay_addr, save_settings, Settings};
use crate::setup::{
    draw_color_row, draw_preview, draw_roster_row, draw_rule_rows, draw_seed_row, draw_team_block,
    draw_title, r_color, r_diff, r_mode, r_new, r_next, r_player, r_prev, r_primary, r_secondary,
    r_team_bridge, r_team_count, r_team_ff, r_team_pick, BOTTOM_Y, CTRL_X, LEFT_X, RIGHT_W,
    RIGHT_X, TOP_Y,
};
use crate::sound::Snd;
use crate::theme::{
    accent_ink, apply_colors, apply_local_colors, mix, palette, with_alpha, ACCENT, BG, CARD_EDGE,
    INK, INK_ON_FILL, REDUCED_MOTION, SRF,
};
use crate::ui::{
    card, control_fill, control_fill_t, draw_icon_bar, draw_round_frame, draw_round_rect, hover_t,
    label, mouse_virtual, primary_button, secondary_button, Ui, WIN_H, WIN_W,
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
    } else if focus == 3 {
        // the relay field takes typed text only
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

// Both lobbies use the shared setup grid (setup.rs): the same map and rule
// controls as the menu on the left, the room and its players on the right.
fn r_lobby_prev() -> Rect {
    r_prev()
}

fn r_lobby_next() -> Rect {
    r_next()
}

fn r_lobby_new() -> Rect {
    r_new()
}

fn r_lobby_player(k: usize) -> Rect {
    r_player(k)
}

fn r_lobby_diff(k: usize) -> Rect {
    r_diff(k)
}

fn r_lobby_mode(k: usize) -> Rect {
    r_mode(k)
}

fn r_room_card() -> Rect {
    Rect::new(RIGHT_X, TOP_Y, RIGHT_W, 140.0)
}

fn r_players_card() -> Rect {
    let y = r_room_card().y + r_room_card().h + 16.0;
    Rect::new(RIGHT_X, y, RIGHT_W, BOTTOM_Y - y)
}

fn team_origin() -> Vec2 {
    let c = r_players_card();
    vec2(c.x + 16.0, c.y + 46.0)
}

fn r_lobby_count(k: usize) -> Rect {
    r_team_count(team_origin(), k)
}

fn r_lobby_team(k: usize) -> Rect {
    r_team_pick(team_origin(), k)
}

fn r_guest_team(k: usize, _count: usize) -> Rect {
    r_team_pick(team_origin(), k)
}

fn r_lobby_ff() -> Rect {
    r_team_ff(team_origin())
}

fn r_lobby_bridge() -> Rect {
    r_team_bridge(team_origin())
}

fn r_lobby_copy() -> Rect {
    let c = r_room_card();
    Rect::new(c.x + c.w - 84.0, c.y + 88.0, 68.0, 28.0)
}

// baseline of roster row `row`; the team block sits above it in team games
const ROSTER_PITCH: f32 = 26.0;

fn roster_y(teams: bool, row: usize) -> f32 {
    let c = r_players_card();
    c.y + if teams { 262.0 } else { 66.0 } + row as f32 * ROSTER_PITCH
}

/// Your own roster row doubles as the name field.
fn r_name_field(teams: bool, row: usize) -> Rect {
    let c = r_players_card();
    Rect::new(c.x + 68.0, roster_y(teams, row) - 19.0, c.w - 120.0, 26.0)
}

// the internet relay row under YOUR COLOR, where the menu shows CONTINUE
fn r_host_relay() -> Rect {
    Rect::new(CTRL_X, 654.0, 240.0, 36.0)
}

fn draw_relay_row(ui: &Ui, m: Vec2, addr: &str, editing: bool, status: &RelayStatus, time: f32) {
    let r = r_host_relay();
    label(ui, "INTERNET", LEFT_X, r.y + r.h * 0.5 + 4.0);
    crate::focus::focusable(r);
    let t = hover_t(r, r.contains(m) && !editing);
    draw_round_frame(
        r.x,
        r.y,
        r.w,
        r.h,
        12.0,
        2.0,
        control_fill_t(SRF(), t),
        if editing {
            ACCENT()
        } else {
            mix(CARD_EDGE(), ACCENT(), t)
        },
    );
    let caret = if editing && (time * 2.5) as i32 % 2 == 0 {
        "|"
    } else {
        ""
    };
    if addr.is_empty() && !editing {
        ui.body(
            "Relay address (optional)",
            r.x + 12.0,
            r.y + 23.0,
            14.0,
            with_alpha(INK(), 0.4),
        );
    } else {
        ui.body_fitted(
            &format!("{addr}{caret}"),
            r.x + 12.0,
            r.y + 23.0,
            14.0,
            r.w - 24.0,
            INK(),
        );
    }
    // a status dot and one word
    let (dot, word) = match status {
        RelayStatus::Off => (with_alpha(INK(), 0.3), "off"),
        RelayStatus::Connecting => (palette(5).dark, "connecting"),
        RelayStatus::Online => (palette(3).dark, "online"),
        RelayStatus::Error(_) => (accent_ink(2), "no link"),
    };
    let (dx, dy) = (r.x + r.w + 16.0, r.y + r.h * 0.5);
    draw_circle(dx, dy, 4.5, dot);
    ui.body(word, dx + 11.0, dy + 4.5, 13.0, with_alpha(INK(), 0.75));
}

fn r_lobby_swatch(i: usize) -> Rect {
    r_color(i)
}

fn r_lobby_start() -> Rect {
    r_primary()
}

fn r_lobby_cancel() -> Rect {
    r_secondary()
}

// Join screen: name on top, LAN games in the middle, a direct address below.
const JOIN_X: f32 = WIN_W * 0.5 - 250.0;

const JOIN_W: f32 = 500.0;

const JOIN_LIST_Y: f32 = 236.0;

fn r_join_name() -> Rect {
    Rect::new(JOIN_X, 142.0, JOIN_W, 44.0)
}

fn r_join_lan_row(i: usize) -> Rect {
    Rect::new(JOIN_X, JOIN_LIST_Y + 6.0 + i as f32 * 52.0, JOIN_W, 46.0)
}

fn r_join_addr() -> Rect {
    Rect::new(JOIN_X, 500.0, 250.0, 44.0)
}

fn r_join_paste() -> Rect {
    Rect::new(JOIN_X + 258.0, 500.0, 74.0, 44.0)
}

fn r_join_code() -> Rect {
    Rect::new(JOIN_X + 340.0, 500.0, JOIN_W - 340.0, 44.0)
}

// used when the address is empty: join by room code through a relay
fn r_join_relay() -> Rect {
    Rect::new(JOIN_X, 582.0, JOIN_W, 40.0)
}

fn r_join_go() -> Rect {
    Rect::new(JOIN_X, 646.0, JOIN_W * 0.5 - 6.0, 56.0)
}

fn r_join_back() -> Rect {
    Rect::new(JOIN_X + JOIN_W * 0.5 + 6.0, 646.0, JOIN_W * 0.5 - 6.0, 56.0)
}

// the right-hand players card both lobbies share: team block, then roster
struct LobbyRoster<'a> {
    pub(crate) players: usize,
    pub(crate) humans: usize,
    pub(crate) you_row: usize,
    pub(crate) host_label: &'a str,
    pub(crate) cfg: TeamCfg,
    pub(crate) team_v: &'a [usize],
    pub(crate) color_v: &'a [usize],
    pub(crate) names: &'a [String],
    pub(crate) my_team: usize,
    pub(crate) host: bool, // the host edits the team rules; guests only pick a team
}

fn draw_players_card(ui: &Ui, m: Vec2, my_name: &str, name_edit: bool, time: f32, r: &LobbyRoster) {
    let c = r_players_card();
    let open = r.players - r.humans;
    let title = if open == 0 {
        "PLAYERS".to_string()
    } else {
        format!(
            "PLAYERS · {} OPEN SEAT{}",
            open,
            if open == 1 { "" } else { "S" }
        )
    };
    card(ui, c, &title);
    if r.cfg.on {
        draw_team_block(ui, m, team_origin(), r.cfg, r.my_team, r.host, true);
    }
    let x0 = c.x + 16.0;
    for k in 0..r.players {
        let y = roster_y(r.cfg.on, k);
        let color = r.color_v.get(k).copied().unwrap_or(k);
        let team = r.cfg.on.then(|| r.team_v.get(k).copied().unwrap_or(0));
        let label = if k == r.you_row {
            String::new() // drawn as the name field below
        } else if k < r.humans {
            let n = r.names.get(k).cloned().unwrap_or_default();
            let n = if n.is_empty() {
                format!("Player {}", k + 1)
            } else {
                n
            };
            if k == 0 {
                format!("{} {}", n, r.host_label)
            } else {
                n
            }
        } else {
            "Bot · open seat".to_string()
        };
        draw_roster_row(ui, x0, y, c.w - 32.0, k, color, &label, team);
        if k >= r.humans {
            continue;
        }
        if k == r.you_row {
            let f = r_name_field(r.cfg.on, k);
            crate::focus::focusable(f);
            let hov = f.contains(m);
            draw_round_frame(
                f.x,
                f.y,
                f.w,
                f.h,
                8.0,
                1.5,
                control_fill(SRF(), hov && !name_edit),
                if name_edit || hov {
                    ACCENT()
                } else {
                    CARD_EDGE()
                },
            );
            let caret = if name_edit && (time * 2.5) as i32 % 2 == 0 {
                "|"
            } else {
                ""
            };
            let (text, col) = if my_name.is_empty() && !name_edit {
                ("Set your name".to_string(), with_alpha(INK(), 0.45))
            } else {
                (format!("{my_name}{caret}"), INK())
            };
            ui.body_fitted(&text, f.x + 10.0, f.y + 18.0, 14.0, f.w - 20.0, col);
        }
    }
}

// a pulsing dot plus one line of status under the room code
fn draw_live_status(ui: &Ui, x: f32, y: f32, text: &str, time: f32) {
    if !REDUCED_MOTION.load(Ordering::Relaxed) {
        let p = (time * 0.8).fract();
        draw_circle(
            x + 4.0,
            y - 4.0,
            4.0 + 8.0 * p,
            with_alpha(palette(3).dark, 0.3 * (1.0 - p)),
        );
    }
    draw_circle(x + 4.0, y - 4.0, 4.0, palette(3).dark);
    ui.body_fitted(
        text,
        x + 16.0,
        y,
        13.0,
        RIGHT_W - 50.0,
        with_alpha(INK(), 0.72),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_host_lobby(
    h: &HostNet,
    menu: &Menu,
    settings: &Settings,
    picks: &LobbyPicks,
    ui: &Ui,
    (name_edit, relay_edit): (bool, bool),
    time: f32,
    copied_t: f32,
) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    let m = mouse_virtual();
    draw_title(ui, "HOST GAME");
    draw_relay_row(ui, m, &settings.relay, relay_edit, &h.relay_status(), time);

    // ---- left: the map and every game setting, exactly as in the menu
    draw_preview(ui, Some(&menu.preview), "");
    draw_seed_row(ui, m, &format!("{:08}", menu.seed), false, true);
    draw_rule_rows(ui, m, true, menu.players, menu.difficulty, settings.teams);
    let occ = h.occupied();
    let cfg = settings.team_cfg();
    let (team_v, color_v) = lobby_layout(&occ, menu.players, cfg, picks, settings.color);
    let taken: Vec<usize> = picks.color.values().copied().collect();
    draw_color_row(ui, m, settings.color, &taken);
    primary_button(ui, r_lobby_start(), "START GAME", 28.0, m, true);
    secondary_button(ui, r_lobby_cancel(), "CANCEL", 16.0, m);

    // ---- right: the room, then who is here
    let rc = r_room_card();
    card(ui, rc, "ROOM CODE");
    ui.text_centered(&h.code, rc.x + rc.w * 0.5, rc.y + 76.0, 38.0, INK());
    ui.body(
        &format!("{}:{}", local_ip(), net_port()),
        rc.x + 18.0,
        rc.y + 107.0,
        14.0,
        with_alpha(INK(), 0.75),
    );
    secondary_button(
        ui,
        r_lobby_copy(),
        if copied_t > 0.0 { "COPIED" } else { "COPY" },
        11.0,
        m,
    );
    let joined = occ.len();
    draw_live_status(
        ui,
        rc.x + 18.0,
        rc.y + 130.0,
        &format!(
            "{} player{} here · on your network",
            joined + 1,
            if joined == 0 { "" } else { "s" },
        ),
        time,
    );
    let mut names: Vec<String> = vec![settings.name.clone()];
    for &seat in &occ {
        names.push(picks.name.get(&seat).cloned().unwrap_or_default());
    }
    draw_players_card(
        ui,
        m,
        settings.name.as_str(),
        name_edit,
        time,
        &LobbyRoster {
            players: menu.players,
            humans: occ.len() + 1,
            you_row: 0,
            host_label: "(host)",
            cfg,
            team_v: &team_v,
            color_v: &color_v,
            names: &names,
            my_team: cfg.my_team,
            host: true,
        },
    );
    draw_icon_bar(settings, ui, m);
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
    let m = mouse_virtual();
    draw_title(ui, "LOBBY");
    draw_preview(ui, j.preview.as_ref(), "Waiting for the host's map…");
    let dots = ".".repeat(1 + (time * 2.0) as usize % 3);
    primary_button(
        ui,
        r_lobby_start(),
        &format!("WAITING FOR HOST{dots}"),
        15.0,
        m,
        false,
    );
    secondary_button(ui, r_lobby_cancel(), "LEAVE", 16.0, m);
    let rc = r_room_card();
    card(ui, rc, "CONNECTED");
    ui.text_centered(&j.code, rc.x + rc.w * 0.5, rc.y + 76.0, 38.0, INK());
    ui.body_fitted(
        &format!("Host: {}", j.addr),
        rc.x + 18.0,
        rc.y + 107.0,
        14.0,
        rc.w - 36.0,
        with_alpha(INK(), 0.75),
    );
    draw_live_status(
        ui,
        rc.x + 18.0,
        rc.y + 130.0,
        "The host picks the map and rules.",
        time,
    );
    let Some(v) = &j.view else {
        let c = r_players_card();
        card(ui, c, "PLAYERS");
        ui.body_centered(
            &format!("Syncing with the host{dots}"),
            c.x + c.w * 0.5,
            c.y + 90.0,
            15.0,
            with_alpha(INK(), 0.72),
        );
        draw_icon_bar(settings, ui, m);
        return;
    };
    let cfg = TeamCfg {
        on: v.ton,
        count: v.tcount,
        hvb: v.thvb,
        my_team: 0,
        bridge: v.tbridge,
        ff: v.tff,
    };
    draw_seed_row(ui, m, &format!("#{:08}", j.preview_key.0), false, false);
    draw_rule_rows(ui, m, false, v.players, idx_diff(v.diff), cfg.on);
    let own_row = 1 + v.occupied.iter().position(|&x| x == my_seat).unwrap_or(0);
    let my_color = j
        .my_color
        .or_else(|| v.colors.get(own_row).copied())
        .unwrap_or(settings.color);
    let humans = v.occupied.len() + 1;
    let taken: Vec<usize> = (0..humans)
        .filter(|&row| row != own_row)
        .filter_map(|row| v.colors.get(row).copied())
        .collect();
    draw_color_row(ui, m, my_color, &taken);
    let my_team = j
        .my_team
        .or_else(|| v.teams.get(own_row).copied())
        .unwrap_or(0);
    draw_players_card(
        ui,
        m,
        settings.name.as_str(),
        name_edit,
        time,
        &LobbyRoster {
            players: v.players,
            humans,
            you_row: own_row,
            host_label: "(host)",
            cfg,
            team_v: &v.teams,
            color_v: &v.colors,
            names: &v.names,
            my_team,
            host: false,
        },
    );
    draw_icon_bar(settings, ui, m);
}

fn draw_join_lobby(j: &JoinUi, lan: Option<&LanScan>, settings: &Settings, ui: &Ui, time: f32) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    draw_title(ui, "JOIN GAME");
    let m = mouse_virtual();
    let caret = |on: bool| {
        if on && (time * 2.5) as i32 % 2 == 0 {
            "|"
        } else {
            ""
        }
    };
    let label = |text: &str, x: f32, y: f32| label(ui, text, x, y);
    let field = |r: Rect, focused: bool, text: &str, size: f32| {
        crate::focus::focusable(r);
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
        ui.body_fitted(
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
    if settings.name.is_empty() && j.focus != 2 {
        ui.body(
            "Shown to the other players",
            rn.x + 16.0,
            rn.y + 28.0,
            16.0,
            with_alpha(INK(), 0.35),
        );
    }

    // ---- games found on the local network
    label("GAMES ON YOUR NETWORK", JOIN_X, JOIN_LIST_Y - 10.0);
    let games = lan.map_or(&[][..], |l| &l.games[..]);
    let list = Rect::new(
        JOIN_X,
        JOIN_LIST_Y,
        JOIN_W,
        LAN_MAX_GAMES as f32 * 52.0 + 6.0,
    );
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
        ui.body_centered(
            hint,
            WIN_W * 0.5,
            list.y + 138.0,
            12.5,
            with_alpha(INK(), 0.6),
        );
    }
    for (i, g) in games.iter().enumerate() {
        let r = r_join_lan_row(i);
        crate::focus::focusable(r);
        let hovered = r.contains(m) && j.connect_rx.is_none();
        draw_round_frame(
            r.x + 6.0,
            r.y,
            r.w - 12.0,
            r.h,
            12.0,
            1.5,
            control_fill_t(SRF(), hover_t(r, hovered)),
            mix(CARD_EDGE(), ACCENT(), hover_t(r, hovered)),
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
        ui.body_fitted(
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
        ui.body(
            "192.168.1.20",
            ra.x + 16.0,
            ra.y + 28.0,
            16.0,
            with_alpha(INK(), 0.3),
        );
    }
    secondary_button(ui, r_join_paste(), "PASTE", 13.0, m);
    field(
        rc,
        j.focus == 1,
        &format!("{}{}", j.code, caret(j.focus == 1)),
        19.0,
    );
    // ---- internet relay: joins by room code when the address is empty
    let rr = r_join_relay();
    label(
        "INTERNET RELAY (WHEN THE ADDRESS IS EMPTY)",
        JOIN_X,
        rr.y - 10.0,
    );
    field(
        rr,
        j.focus == 3,
        &format!("{}{}", settings.relay, caret(j.focus == 3)),
        16.0,
    );
    if settings.relay.is_empty() && j.focus != 3 {
        ui.body(
            "relay.example.org — for games outside your network",
            rr.x + 16.0,
            rr.y + 26.0,
            15.0,
            with_alpha(INK(), 0.35),
        );
    }
    let connecting = j.connect_rx.is_some();
    primary_button(
        ui,
        r_join_go(),
        if connecting {
            "CONNECTING…"
        } else {
            "CONNECT"
        },
        18.0,
        m,
        !connecting,
    );
    secondary_button(ui, r_join_back(), "BACK", 16.0, m);
    if !j.status.is_empty() {
        ui.body_centered(&j.status, WIN_W * 0.5, 738.0, 15.0, with_alpha(INK(), 0.75));
    }
    draw_icon_bar(settings, ui, m);
}

impl App {
    // One frame of the host's lobby.
    pub(crate) fn run_host_lobby(&mut self, dt: f32) {
        self.menu_time += dt;
        let click2 = is_mouse_button_pressed(MouseButton::Left);
        let m2 = mouse_virtual();
        // Enter or Esc that closes the name field must not also start or cancel
        let name_edit_at_start = self.lobby_name_edit || self.lobby_relay_edit;
        let mut cancel = is_key_pressed(KeyCode::Escape) && !name_edit_at_start;
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
                || (click2 && !r_name_field(self.settings.teams, 0).contains(m2))
            {
                self.lobby_name_edit = false;
                save_settings(&self.settings);
            }
        } else if self.lobby_relay_edit {
            while let Some(c) = get_char_pressed() {
                if (c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'))
                    && self.settings.relay.len() < 64
                {
                    self.settings.relay.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.settings.relay.pop();
            }
            if is_key_pressed(KeyCode::Enter)
                || is_key_pressed(KeyCode::Escape)
                || (click2 && !r_host_relay().contains(m2))
            {
                // the new address takes effect at once
                self.lobby_relay_edit = false;
                self.settings.relay = clean_relay_addr(&self.settings.relay);
                save_settings(&self.settings);
                h.disable_relay();
                if !self.settings.relay.is_empty() {
                    let _ = h.enable_relay(&self.settings.relay);
                }
            }
        } else {
            while get_char_pressed().is_some() {}
        }
        if click2 {
            if r_host_relay().contains(m2) && !self.lobby_relay_edit {
                self.lobby_relay_edit = true;
            }
            // Global toolbar was processed before this frame was painted.
            if r_lobby_copy().contains(m2) {
                copy_to_clipboard(&format!("{}:{}", local_ip(), net_port()));
                self.copied_t = 1.5;
                self.snd_queue.push(Snd::Click);
            }
            if r_name_field(self.settings.teams, 0).contains(m2) {
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
            || (is_key_pressed(KeyCode::Enter) && !name_edit_at_start)
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
                self.settings.speed,
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
            // every seat plays the position of its color, as in the preview
            apply_position_swap(&mut ng, &cv);
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
            (self.lobby_name_edit, self.lobby_relay_edit),
            self.menu_time,
            self.copied_t,
        );
        if cancel {
            self.host = None;
            self.lobby_name_edit = false;
            self.lobby_relay_edit = false;
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
                        gn.speed = crate::net::parse_speed(it.next());
                        let g = build_guest_game(
                            seed, players, humans, di, seat, &colors, teams, team_v, names,
                        );
                        gn.seat = g.my_seat;
                        self.game = Some(g);
                        self.screen = Screen::Play;
                        self.snd_queue.push(Snd::Turn);
                        // lines after START (the first events) stay queued
                        // for the game loop
                        break;
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
            let name_rect = self
                .join
                .view
                .as_ref()
                .map_or(Rect::new(0.0, 0.0, 0.0, 0.0), |v| {
                    let row = 1 + v.occupied.iter().position(|&x| x == gn.seat).unwrap_or(0);
                    r_name_field(v.ton, row)
                });
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
                    || (click && !name_rect.contains(m))
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
                if name_rect.contains(m) {
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
                } else if self.join.focus == 3 {
                    if (c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'))
                        && self.settings.relay.len() < 64
                    {
                        self.settings.relay.push(c);
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
                    3 => {
                        self.settings.relay.pop();
                    }
                    _ => {
                        self.join.addr.pop();
                    }
                }
            }
            if is_key_pressed(KeyCode::Tab) {
                self.join.focus = (self.join.focus + 1) % 4;
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
            if click && r_join_relay().contains(m) {
                self.join.focus = 3;
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
                self.settings.relay = clean_relay_addr(&self.settings.relay);
                save_settings(&self.settings);
                if self.join.addr.trim().is_empty() && !self.settings.relay.is_empty() {
                    self.join.status = "Connecting through the relay...".to_string();
                    self.join.connect_rx = Some(spawn_guest_connect_via_relay(
                        self.settings.relay.clone(),
                        self.join.code.clone(),
                        self.settings.name.clone(),
                    ));
                } else {
                    self.join.status = "Connecting...".to_string();
                    self.join.connect_rx = Some(spawn_guest_connect(
                        self.join.addr.clone(),
                        self.join.code.clone(),
                        self.settings.name.clone(),
                    ));
                }
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
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
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
