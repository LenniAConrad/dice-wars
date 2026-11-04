//! The in-game screen: layout, input and the per-frame update.

use std::collections::HashMap;

use dicegame::engine::prelude::*;

use crate::board::{
    apply_position_swap, cell_pos, gen_map, gen_map_local, random_seed, COLS, HEX, ROWS,
};
use crate::draw_game::{draw_confirm, draw_game};
use crate::game::{Difficulty, Game, RepEvent, MAX_PLAYERS};
use crate::lobby::{build_guest_game, parse_start_tail};
use crate::net::{
    parse_csv_usize, parse_reinforcement_lands, parse_speed, reject_invalid_host_data,
    send_net_line, spawn_reconnect, HostMsg, ReconnectError, NET_MAX_PENDING_EVENTS,
    NET_MESSAGES_PER_FRAME,
};
use crate::replay::Replay;
use crate::sound::Snd;
use crate::theme::{apply_colors, BORDER, INK, SRF};
use crate::ui::{draw_round_frame, mouse_virtual, toolbar_contains, WIN_H, WIN_W};
use crate::{App, GuestNet, HostNet, Screen};

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

fn r_over_replay() -> Rect {
    r_over_button(0, 0.0)
}

fn r_over_gif() -> Rect {
    r_over_button(1, 0.0)
}

fn r_over_new() -> Rect {
    r_over_button(2, 0.0)
}

fn r_over_menu() -> Rect {
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
        if dist < HEX && best.is_none_or(|b| dist < b.0) {
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

fn handle_input(game: &mut Game, mut net_out: Option<&mut Vec<String>>) {
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
    if toolbar_contains(mp) {
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
                    if let Some(out) = net_out {
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

impl App {
    // One frame of a match: network traffic, input, drawing.
    pub(crate) fn run_play(&mut self, dt: f32) {
        let g = self.game.as_mut().unwrap();
        // Fights between bots run faster, and holding SPACE fast-forwards bot
        // turns. Only local games: a host's clock also paces its guests.
        let local = self.host.is_none() && self.guest.is_none();
        let pace = if local && g.bots_only() {
            if is_key_down(KeyCode::Space) {
                6.0
            } else {
                2.5
            }
        } else {
            1.0
        };
        // A network game runs at the host's speed on every machine.
        let speed = self
            .guest
            .as_ref()
            .map_or(self.settings.speed, |gn| gn.speed);
        g.update(dt * speed * pace);
        g.tick_turn_timer(dt);
        if let Some(gn) = self.guest.as_mut() {
            if gn.rec_failed && is_mouse_button_pressed(MouseButton::Left) {
                gn.rec_failed = false;
                gn.rec_rx = Some(spawn_reconnect(
                    gn.addr.clone(),
                    gn.code.clone(),
                    gn.token.clone(),
                ));
                g.push_log("Reconnecting...".to_string());
            }
        }
        let mut go_menu = false;
        let click = is_mouse_button_pressed(MouseButton::Left);

        if self.autoend && g.current == 0 && g.battle.is_none() && g.over.is_none() {
            g.end_turn(); // test hook: the host seat passes instantly
        }
        // ---- heartbeats: keep idle connections alive through NATs
        self.net_ping_t += dt;
        let do_ping = self.net_ping_t >= 2.0;
        if do_ping {
            self.net_ping_t = 0.0;
        }
        // ---- hosting: apply guest intents, broadcast new events
        if let Some(h) = self.host.as_mut() {
            if self.settings.speed != h.speed {
                // the host changed the game speed mid-match: everyone follows
                h.speed = self.settings.speed;
                h.broadcast(&format!("SPEED {}", h.speed));
            }
            host_tick(
                h,
                g,
                &mut self.awaiting,
                self.menu.seed,
                self.menu.difficulty,
                dt,
                do_ping,
            );
        }

        // ---- joined: apply the host's events, queue our own intents
        if let Some(gn) = self.guest.as_mut() {
            guest_tick(gn, g, do_ping);
        }
        if let Some(kind) = self.confirm {
            // modal: destructive actions need explicit confirmation
            if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) {
                self.confirm = None;
            } else if click {
                let m = mouse_virtual();
                if r_confirm_yes().contains(m) {
                    self.confirm = None;
                    match kind {
                        Confirm::NewMap => {
                            self.menu.seed = random_seed();
                            if let Some(h) = self.host.as_mut() {
                                // rematch: everyone still connected moves
                                // into the fresh round
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
                                self.awaiting.clear();
                                *g = ng;
                            } else {
                                *g = gen_map_local(
                                    self.menu.seed,
                                    self.menu.players,
                                    1,
                                    self.menu.difficulty,
                                    self.settings.team_cfg(),
                                );
                            }
                        }
                        Confirm::Menu => go_menu = true,
                    }
                } else if r_confirm_no().contains(m) {
                    self.confirm = None;
                }
            }
        } else if click && g.over.is_none() && r_btn_menu().contains(mouse_virtual()) {
            self.confirm = Some(Confirm::Menu);
        } else if click
            && g.over.is_none()
            && self.guest.is_none()
            && r_btn_new().contains(mouse_virtual())
        {
            self.confirm = Some(Confirm::NewMap);
        } else if is_key_pressed(KeyCode::Escape) {
            if g.selected.is_some() {
                g.selected = None; // ESC first cancels the selection
            } else if g.over.is_some() {
                go_menu = true;
            } else {
                self.confirm = Some(Confirm::Menu);
            }
        } else if is_key_pressed(KeyCode::R) && self.guest.is_none() && !modifier_down() {
            if g.over.is_some() && self.host.is_none() {
                self.menu.seed = random_seed();
                *g = gen_map_local(
                    self.menu.seed,
                    self.menu.players,
                    1,
                    self.menu.difficulty,
                    self.settings.team_cfg(),
                );
            } else {
                self.confirm = Some(Confirm::NewMap);
            }
        } else if g.over.is_some() {
            if g.over_t >= 0.6 && click {
                let m = mouse_virtual();
                if r_over_replay().contains(m) {
                    self.replay = Some(Replay::new(g, false));
                    self.screen = Screen::Replay;
                } else if r_over_gif().contains(m) {
                    self.replay = Some(Replay::new(g, true));
                    self.screen = Screen::Replay;
                } else if r_over_new().contains(m) && self.guest.is_none() {
                    if self.host.is_none() {
                        self.menu.seed = random_seed();
                        *g = gen_map_local(
                            self.menu.seed,
                            self.menu.players,
                            1,
                            self.menu.difficulty,
                            self.settings.team_cfg(),
                        );
                    } else {
                        self.confirm = Some(Confirm::NewMap);
                    }
                } else if r_over_menu().contains(m) {
                    go_menu = true;
                }
            }
        } else if g.battle.is_none() && g.over.is_none() {
            let my_turn = if let Some(gn) = &self.guest {
                g.current == gn.seat && gn.rec_rx.is_none() && !gn.catching_up
            } else if self.host.is_some() {
                g.current == 0
            } else {
                g.current < g.humans
            };
            if my_turn {
                if self.guest.is_some() {
                    let mut outbox: Vec<String> = Vec::new();
                    handle_input(g, Some(&mut outbox));
                    if let Some(gn) = self.guest.as_mut() {
                        for l in outbox {
                            send_net_line(&mut gn.stream, &l);
                        }
                    }
                } else {
                    handle_input(g, None);
                }
            }
        }
        draw_game(g, &self.ui, &self.settings);
        if let Some(gn) = &self.guest {
            if (gn.rec_rx.is_some() || gn.rec_failed) && g.over.is_none() {
                let txt = if gn.rec_failed {
                    "Could not reconnect — click anywhere to retry"
                } else {
                    "Connection lost — reconnecting..."
                };
                let w = self.ui.width(txt, 22.0) + 56.0;
                draw_round_frame(
                    (WIN_W - w) * 0.5,
                    270.0,
                    w,
                    56.0,
                    16.0,
                    2.5,
                    SRF(),
                    BORDER(),
                );
                self.ui.text_centered(txt, WIN_W * 0.5, 306.0, 22.0, INK());
            }
        }
        if let Some(kind) = self.confirm {
            draw_confirm(kind, &self.ui);
        }
        self.snd_queue.append(&mut g.snd);
        if go_menu {
            self.confirm = None;
            self.leave_network();
            self.screen = Screen::Menu;
            self.menu.regen();
        }
    }
}

// Host side of a match: apply the guests' intents, hold seats of guests who
// dropped out (a bot takes over when the grace timer runs out) and broadcast
// the resulting events.
fn host_tick(
    h: &mut HostNet,
    g: &mut Game,
    awaiting: &mut HashMap<usize, f32>,
    seed: u64,
    difficulty: Difficulty,
    dt: f32,
    do_ping: bool,
) {
    if do_ping {
        h.broadcast("PING");
    }
    for _ in 0..NET_MESSAGES_PER_FRAME {
        let Ok(msg) = h.rx.try_recv() else { break };
        match msg {
            HostMsg::Intent(lobby_seat, line) => {
                let Some(&seat) = h.remap.get(&lobby_seat) else {
                    continue;
                };
                if g.battle.is_some() || g.over.is_some() || g.current != seat {
                    continue;
                }
                let mut it = line.split_whitespace();
                match it.next() {
                    Some("ATTACK") => {
                        let a: Option<usize> = it.next().and_then(|v| v.parse().ok());
                        let d: Option<usize> = it.next().and_then(|v| v.parse().ok());
                        if let (Some(a), Some(d)) = (a, d) {
                            if a < g.terrs.len()
                                && d < g.terrs.len()
                                && g.terrs[a].owner == seat
                                && g.terrs[a].dice > 1
                                && g.can_target(seat, g.terrs[d].owner)
                                && g.terrs[a].neighbors.contains(&d)
                            {
                                g.begin_attack(a, d);
                            }
                        }
                    }
                    Some("END") => g.end_turn(),
                    _ => {}
                }
            }
            HostMsg::Left(lobby_seat) => {
                let Some(&seat) = h.remap.get(&lobby_seat) else {
                    continue;
                };
                if g.over.is_none() {
                    // hold the seat and give them time to come back
                    awaiting.insert(seat, 45.0);
                    let n = g.name_of(seat);
                    g.push_log(format!(
                        "{} lost connection — waiting for them to reconnect...",
                        n
                    ));
                }
            }
            HostMsg::Rejoined(lobby_seat) => {
                if let Some(&seat) = h.remap.get(&lobby_seat).copied().as_ref() {
                    if seat < g.absent.len() {
                        g.absent[seat] = false;
                    }
                    awaiting.remove(&seat);
                    let n = g.name_of(seat);
                    g.push_log(format!("{} reconnected!", n));
                    h.broadcast(&format!("ABSENT {} 0", seat));
                }
                h.send_resume(lobby_seat, g, seed, difficulty);
            }
            HostMsg::Joined(..) => {}
        }
    }
    h.flush_events(g);
    // grace timers: give up on seats that never came back
    let mut expired: Vec<usize> = Vec::new();
    for (seat, t) in awaiting.iter_mut() {
        *t -= dt;
        if *t <= 0.0 {
            expired.push(*seat);
        }
    }
    for seat in expired {
        awaiting.remove(&seat);
        if g.over.is_none() && seat < g.absent.len() {
            g.absent[seat] = true;
            let n = g.name_of(seat);
            g.push_log(format!("{} left — a bot takes over.", n));
            h.broadcast(&format!("ABSENT {} 1", seat));
        }
    }
}

// The guest fell so far behind that its event queue is full: drop the
// backlog and reconnect, so the host's RESUME brings the board up to date.
fn resync_with_host(g: &mut Game, gn: &mut GuestNet) {
    if gn.awaiting_resume {
        return;
    }
    gn.pending.clear();
    gn.awaiting_resume = true;
    // the reader thread sees the closed socket and reports LOST, which
    // starts the reconnect
    let _ = gn.stream.shutdown(std::net::Shutdown::Both);
    g.push_log("Fell behind the host — resyncing...".to_string());
}

// Guest side of a match: reconnect handling and the host's events, applied
// one at a time (animated) or all at once while catching up.
fn guest_tick(gn: &mut GuestNet, g: &mut Game, do_ping: bool) {
    if do_ping && gn.rec_rx.is_none() {
        send_net_line(&mut gn.stream, "PING");
    }
    // reconnect attempt finished?
    if let Some(rr) = &gn.rec_rx {
        if let Ok(result) = rr.try_recv() {
            match result {
                Ok((w, rx)) => {
                    gn.stream = w;
                    gn.rx = rx;
                    gn.rec_rx = None;
                    gn.catching_up = true;
                    gn.awaiting_resume = true;
                    gn.pending.clear();
                }
                Err(ReconnectError::Unreachable) => {
                    gn.rec_rx = None;
                    gn.rec_failed = true;
                }
                Err(ReconnectError::Rejected(why)) => {
                    // the host is gone or moved on: retrying cannot help
                    gn.rec_rx = None;
                    if g.over.is_none() {
                        g.over = Some(if why.is_empty() || why == "wrong code" {
                            "Could not rejoin — the host closed your seat".to_string()
                        } else {
                            why
                        });
                        g.banner_t = 0.0;
                    }
                }
            }
        }
    }
    for _ in 0..NET_MESSAGES_PER_FRAME {
        let Ok(l) = gn.rx.try_recv() else { break };
        let mut it = l.split_whitespace();
        match it.next() {
            Some("ATK") => {
                let v: Result<Vec<u32>, _> = it.map(str::parse).collect();
                if let Ok(v) = v {
                    if v.len() != 5 || v[4] > 1 {
                        reject_invalid_host_data(g, gn);
                        break;
                    }
                    if gn.awaiting_resume {
                        continue; // RESUME resends it
                    }
                    if gn.pending.len() >= NET_MAX_PENDING_EVENTS {
                        resync_with_host(g, gn);
                        continue;
                    }
                    gn.pending.push_back(RepEvent::Attack {
                        a: v[0] as usize,
                        d: v[1] as usize,
                        ra: v[2],
                        rd: v[3],
                        captured: v[4] == 1,
                    });
                } else {
                    reject_invalid_host_data(g, gn);
                    break;
                }
            }
            Some("REINF") => {
                let player = it.next().and_then(|v| v.parse::<usize>().ok());
                let lands = match it.next() {
                    Some(csv) => parse_reinforcement_lands(csv),
                    None => Some(Vec::new()),
                };
                if let (Some(player), Some(lands), None) = (player, lands, it.next()) {
                    if gn.awaiting_resume {
                        continue; // RESUME resends it
                    }
                    if gn.pending.len() >= NET_MAX_PENDING_EVENTS {
                        resync_with_host(g, gn);
                        continue;
                    }
                    gn.pending.push_back(RepEvent::Reinforce { player, lands });
                } else {
                    reject_invalid_host_data(g, gn);
                    break;
                }
            }
            Some("RECONNECTED") => {
                let lobby_seat = it.next().and_then(|v| v.parse::<usize>().ok());
                let token = it.next();
                if let (Some(_), Some(token), None) = (
                    lobby_seat.filter(|&seat| seat > 0 && seat < MAX_PLAYERS),
                    token.filter(|token| {
                        token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())
                    }),
                    it.next(),
                ) {
                    gn.token = token.to_string();
                } else {
                    reject_invalid_host_data(g, gn);
                    break;
                }
            }
            Some("PING") => {}
            Some("SPEED") => gn.speed = parse_speed(it.next()),
            Some("RESUME") => {
                // rebuild the match and fast-forward the backlog
                let seed: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let players: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                let humans: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                let di: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                let seat: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                let (colors, teams, team_v, names) = parse_start_tail(&mut it);
                let absent: Vec<usize> = it.next().map(parse_csv_usize).unwrap_or_default();
                gn.speed = parse_speed(it.next());
                gn.awaiting_resume = false;
                gn.pending.clear();
                let mut ng = build_guest_game(
                    seed, players, humans, di, seat, &colors, teams, team_v, names,
                );
                gn.seat = ng.my_seat;
                if absent.len() == ng.players {
                    ng.absent = absent.iter().map(|&a| a == 1).collect();
                }
                *g = ng;
                gn.catching_up = true;
            }
            Some("SYNCED") => {
                gn.catching_up = false;
                g.push_log("Reconnected!".to_string());
            }
            Some("START") => {
                // the host started a fresh round with everyone in it
                let seed: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let players: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                let humans: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                let di: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                let seat: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                let (colors, teams, team_v, names) = parse_start_tail(&mut it);
                gn.speed = parse_speed(it.next());
                gn.pending.clear();
                gn.catching_up = false;
                gn.awaiting_resume = false;
                let ng = build_guest_game(
                    seed, players, humans, di, seat, &colors, teams, team_v, names,
                );
                gn.seat = ng.my_seat;
                *g = ng;
                g.push_log("The host started a new round!".to_string());
                g.snd.push(Snd::Turn);
            }
            Some("ABSENT") => {
                let ps: Option<usize> = it.next().and_then(|v| v.parse().ok());
                let gone = it.next() == Some("1");
                if let Some(ps) = ps {
                    if ps < g.absent.len() {
                        g.absent[ps] = gone;
                        let n = g.name_of(ps);
                        g.push_log(if gone {
                            format!("{} left — a bot takes over.", n)
                        } else {
                            format!("{} is back!", n)
                        });
                    }
                }
            }
            Some("LOST") => {
                // connection dropped: hold the game and retry quietly
                if g.over.is_none() && gn.rec_rx.is_none() {
                    gn.rec_rx = Some(spawn_reconnect(
                        gn.addr.clone(),
                        gn.code.clone(),
                        gn.token.clone(),
                    ));
                    g.push_log("Connection lost — reconnecting...".to_string());
                }
            }
            Some("OVER") | Some("BYE") if g.over.is_none() => {
                let rest: Vec<&str> = it.collect();
                g.over = Some(if rest.is_empty() {
                    "Connection lost".to_string()
                } else {
                    rest.join(" ")
                });
                g.banner_t = 0.0;
            }
            _ => {}
        }
        // while catching up, apply events instantly
        if gn.catching_up {
            while let Some(ev) = gn.pending.pop_front() {
                let valid = match ev {
                    RepEvent::Attack {
                        a,
                        d,
                        ra,
                        rd,
                        captured,
                    } => g.net_apply_attack_fast(a, d, ra, rd, captured),
                    RepEvent::Reinforce { player, lands } => g.net_apply_reinforce(player, lands),
                };
                if !valid {
                    reject_invalid_host_data(g, gn);
                    break;
                }
            }
        }
    }
    if g.battle.is_none() && g.over.is_none() {
        if let Some(ev) = gn.pending.pop_front() {
            let valid = match ev {
                RepEvent::Attack {
                    a,
                    d,
                    ra,
                    rd,
                    captured,
                } => g.net_apply_attack(a, d, ra, rd, captured),
                RepEvent::Reinforce { player, lands } => g.net_apply_reinforce(player, lands),
            };
            if !valid {
                reject_invalid_host_data(g, gn);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::game::TeamCfg;
    use crate::testutil::mk_game;

    use super::*;

    // a guest session whose host lines come from the returned sender; the
    // returned socket is the host's end of the connection
    fn test_guest() -> (
        GuestNet,
        std::sync::mpsc::SyncSender<String>,
        std::net::TcpStream,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (host_end, _) = listener.accept().unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        let gn = GuestNet {
            stream,
            rx,
            seat: 1,
            pending: std::collections::VecDeque::new(),
            addr: String::new(),
            code: String::new(),
            token: String::new(),
            catching_up: false,
            rec_failed: false,
            rec_rx: None,
            awaiting_resume: false,
            speed: 1.0,
        };
        (gn, tx, host_end)
    }

    #[test]
    fn guest_ignores_events_between_reconnect_and_resume() {
        let mut g = mk_game(&[0, 1], &[(0, 1)], 2, TeamCfg::default());
        g.net_guest = true;
        let (mut gn, tx, _host) = test_guest();
        gn.awaiting_resume = true;
        tx.send("ATK 0 1 7 6 1".to_string()).unwrap();
        tx.send("REINF 0 0".to_string()).unwrap();
        tx.send("SPEED 4".to_string()).unwrap();
        guest_tick(&mut gn, &mut g, false);
        assert!(gn.pending.is_empty());
        assert!(g.battle.is_none());
        assert_eq!(g.terrs[1].owner, 1);
        assert_eq!(gn.speed, 4.0);
    }

    #[test]
    fn a_guest_far_behind_resyncs_instead_of_quitting() {
        use std::io::Read;
        let mut g = mk_game(&[0, 1], &[(0, 1)], 2, TeamCfg::default());
        g.net_guest = true;
        g.battle = None;
        let (mut gn, tx, mut host) = test_guest();
        for _ in 0..NET_MAX_PENDING_EVENTS {
            gn.pending.push_back(RepEvent::Reinforce {
                player: 0,
                lands: vec![0],
            });
        }
        g.over = Some("hold".to_string()); // keep the queue from draining
        tx.send("ATK 0 1 7 6 1".to_string()).unwrap();
        guest_tick(&mut gn, &mut g, false);
        assert_eq!(g.over.as_deref(), Some("hold"));
        assert!(gn.pending.is_empty());
        assert!(gn.awaiting_resume);
        // the connection is closed so the reconnect (and RESUME) follow
        host.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        assert_eq!(host.read(&mut [0u8; 8]).unwrap(), 0);
    }

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
