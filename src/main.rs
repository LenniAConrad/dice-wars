// Release builds on Windows start as a GUI app without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use dicegame::engine::{self, prelude::*};

use crate::board::{apply_position_swap, gen_map, gen_map_local, random_seed};
use crate::clipboard::{copy_to_clipboard, paste_from_clipboard, spawn_clipboard_paste};
use crate::dice::draw_die;
use crate::draw_game::{
    draw_battle_overlay, draw_board, draw_confirm, draw_game, draw_panel_replay,
};
use crate::game::{
    diff_idx, split_sum, Battle, Difficulty, Game, RepEvent, TeamCfg, MAX_PLAYERS, MAX_TEAMS,
    SEED_MOD,
};
use crate::gif::{downsample, gif_capture_count, quantize_image, GifWriter};
use crate::lan::{lan_beacon_line, LanBeacon, LanScan};
use crate::lobby::{
    apply_join_paste, build_guest_game, draw_guest_lobby, draw_host_lobby, draw_join_lobby,
    parse_names_csv, parse_start_tail, r_guest_team, r_join_addr, r_join_back, r_join_code,
    r_join_go, r_join_lan_row, r_join_name, r_join_paste, r_lobby_bridge, r_lobby_cancel,
    r_lobby_copy, r_lobby_count, r_lobby_diff, r_lobby_ff, r_lobby_mode, r_lobby_name, r_lobby_new,
    r_lobby_next, r_lobby_player, r_lobby_prev, r_lobby_start, r_lobby_swatch, r_lobby_team,
    JoinUi, LobbyView,
};
use crate::menu::{draw_menu, update_menu, Menu, MenuAction};
use crate::net::{
    guest_connect, lobby_layout, lobby_line, local_ip, net_port, parse_csv_usize,
    parse_reinforcement_lands, reject_invalid_host_data, send_net_line, spawn_guest_connect,
    spawn_reconnect, GuestNet, HostMsg, HostNet, LobbyPicks, NET_MAX_PENDING_EVENTS,
    NET_MESSAGES_PER_FRAME, NET_ROOM_CODE_LEN,
};
use crate::play::{
    handle_input, r_btn_menu, r_btn_new, r_confirm_no, r_confirm_yes, r_over_gif, r_over_menu,
    r_over_new, r_over_replay, Confirm,
};
use crate::replay::{draw_replay_hud, Replay};
use crate::settings::{clean_name, load_settings, save_settings};
use crate::sound::{Snd, Sounds};
use crate::theme::{
    apply_colors, apply_local_colors, darken, mix, with_alpha, Palette, BG, BORDER, COLORBLIND,
    DARK, INK, PLAYER_BASE, REDUCED_MOTION, SRF,
};
use crate::ui::{
    draw_icon_bar, draw_round_frame, handle_icon_bar_click, mouse_virtual, r_icon, view_camera, Ui,
    WIN_H, WIN_W,
};
mod ai;
mod board;
mod clipboard;
mod dice;
mod draw_game;
mod game;
mod gif;
mod lan;
mod lobby;
mod menu;
mod net;
mod play;
mod replay;
mod settings;
mod sound;
mod svg;
#[cfg(test)]
mod testutil;
mod theme;
mod ui;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Play,
    Replay,
    HostLobby,
    JoinLobby,
}

fn main() {
    if let Err(error) = engine::init("Dice Wars", WIN_W as usize, WIN_H as usize) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    engine::random::srand(engine::now() as u64);
    let ui = Ui {
        font: Font::embedded(),
    };
    let sounds = Sounds::load();
    let mut settings = load_settings();
    if let Ok(theme) = std::env::var("DW_THEME") {
        settings.dark = theme == "dark";
    }
    if std::env::var_os("DW_ACCESSIBLE").is_some() {
        settings.colorblind = true;
    }
    if std::env::var_os("DW_TEAMS").is_some() {
        settings.teams = true;
    }
    if let Ok(players) = std::env::var("DW_PLAYERS") {
        settings.players = players.parse::<usize>().unwrap_or(4).clamp(2, MAX_PLAYERS);
    }

    COLORBLIND.store(settings.colorblind, Ordering::Relaxed);
    apply_local_colors(settings.color);
    DARK.store(settings.dark, Ordering::Relaxed);
    REDUCED_MOTION.store(settings.reduced_motion, Ordering::Relaxed);
    if std::env::var("DW_EXPORT_ICON").is_ok() {
        let rt = render_target(1024, 1024);
        set_camera(&Camera2D {
            target: vec2(128.0, 128.0),
            zoom: vec2(2.0 / 256.0, 2.0 / 256.0),
            render_target: Some(rt.clone()),
            ..Default::default()
        });
        clear_background(Color::new(0.0, 0.0, 0.0, 0.0));
        let base = PLAYER_BASE[0]; // always buttercup, whatever the user picked
        let pal = Palette {
            fill: mix(base, WHITE, 0.46),
            mid: base,
            dark: darken(base, 0.78),
            light: mix(base, WHITE, 0.62),
            pip: Color::new(0.22, 0.23, 0.38, 1.0),
        };
        draw_die(128.0, 232.0, 176.0, 11, 0, &pal);
        set_default_camera();
        let img = downsample(&rt.texture.get_texture_data(), 4);
        img.export_png("assets/icon.png")
            .expect("could not export icon");
        std::process::exit(0);
    }

    let mut menu = Menu::init(&settings);
    if let Ok(seed) = std::env::var("DW_SEED") {
        menu.seed = seed.parse::<u64>().unwrap_or(31337) % SEED_MOD;
        menu.regen();
    }

    let mut screen = Screen::Menu;
    let mut game: Option<Game> = None;
    if std::env::var("DW_AUTOSTART").is_ok() {
        game = Some(gen_map_local(
            menu.seed,
            menu.players,
            1,
            menu.difficulty,
            settings.team_cfg(),
        ));
        screen = Screen::Play;
    }
    if std::env::var_os("DW_BATTLE").is_some() {
        if let Some(g) = &mut game {
            if let Some((a, d)) = g.terrs.iter().enumerate().find_map(|(a, t)| {
                t.neighbors
                    .iter()
                    .find(|&&d| g.terrs[d].owner != t.owner)
                    .map(|&d| (a, d))
            }) {
                g.terrs[a].dice = 8;
                g.terrs[d].dice = 8;
                g.current = g.terrs[a].owner;
                g.begin_attack(a, d);
                g.battle.as_mut().unwrap().t = 1.1;
                g.banner_t = 0.0;
            }
        }
    }
    let mut replay: Option<Replay> = None;
    let mut host: Option<HostNet> = None;
    let mut guest: Option<GuestNet> = None;
    let mut join = JoinUi::default();
    let mut menu_time = 0.0f32;
    let mut net_ping_t = 0.0f32;
    let mut lobby_picks = LobbyPicks::default();
    let mut lobby_sync_t = 0.0f32;
    let mut lobby_name_edit = false;
    let mut awaiting: HashMap<usize, f32> = HashMap::new(); // game seat -> grace timer
    let mut copied_t = 0.0f32;
    let mut confirm: Option<Confirm> = None;
    let mut snd_queue: Vec<Snd> = Vec::new();
    let autoend = std::env::var("DW_TEST_AUTOEND").is_ok();
    let test_host_auto: usize = std::env::var("DW_TEST_HOST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // Deterministic audit scenes; screenshot scripts use a temporary data directory.
    match std::env::var("DW_SCENE").as_deref() {
        Ok("battle-replay") => {
            if let Some(g) = &game {
                let mut r = Replay::new(g, false);
                if let Some(b) = &g.battle {
                    r.g.current = g.current;
                    r.g.terrs[b.a].dice = 8;
                    r.g.terrs[b.d].dice = 8;
                    r.g.battle = Some(Battle {
                        a: b.a,
                        d: b.d,
                        ra: 21,
                        rd: 12,
                        ad: split_sum(21, 8),
                        dd: split_sum(12, 8),
                        captured: true,
                        t: b.t,
                    });
                }
                replay = Some(r);
                screen = Screen::Replay;
            }
        }
        Ok("join") => screen = Screen::JoinLobby,
        Ok("confirm") => confirm = Some(Confirm::Menu),
        Ok("selection") => {
            if let Some(g) = &mut game {
                if let Some(a) = g.terrs.iter().position(|t| {
                    t.owner == g.my_seat
                        && t.neighbors
                            .iter()
                            .any(|&d| g.can_target(g.my_seat, g.terrs[d].owner))
                }) {
                    g.current = g.my_seat;
                    g.terrs[a].dice = g.terrs[a].dice.max(2);
                    g.selected = Some(a);
                    g.banner_t = 0.0;
                }
            }
        }
        Ok("over") => {
            if let Some(g) = &mut game {
                g.over = Some("Buttercup wins!".to_string());
                g.over_t = 1.0;
                g.banner_t = 0.0;
            }
        }
        Ok("replay") | Ok("gif") => {
            if let Some(g) = &mut game {
                let t = g.terrs.iter().position(|t| t.owner == 0).unwrap_or(0);
                g.events.push(RepEvent::Reinforce {
                    player: 0,
                    lands: vec![t],
                });
                replay = Some(Replay::new(
                    g,
                    std::env::var("DW_SCENE").as_deref() == Ok("gif"),
                ));
                screen = Screen::Replay;
            }
        }
        _ => {}
    }

    // automation hooks for testing the network path
    if std::env::var("DW_TEST_HOST").is_ok() {
        match HostNet::start(menu.players - 1) {
            Ok(h) => {
                eprintln!("HOSTCODE {}", h.code);
                host = Some(h);
                screen = Screen::HostLobby;
            }
            Err(e) => eprintln!("HOSTFAIL {}", e),
        }
    }
    if let Ok(spec) = std::env::var("DW_TEST_JOIN") {
        let mut it = spec.split_whitespace();
        let addr = it.next().unwrap_or("").to_string();
        let code = it.next().unwrap_or("").to_string();
        join.addr = addr;
        join.code = code;
        match guest_connect(&join.addr, &join.code, &settings.name) {
            Ok(g) => {
                eprintln!("JOINED seat {}", g.seat);
                guest = Some(g);
                screen = Screen::JoinLobby;
            }
            Err(e) => eprintln!("JOIN FAILED: {}", e),
        }
    }

    let mut lan_scan: Option<LanScan> = None;
    let mut lan_beacon: Option<LanBeacon> = None;
    let mut previous_screen = None;
    // Screen changes fade in from the surface color instead of cutting hard.
    let mut screen_fade = 0.0f32;
    loop {
        let dt = get_frame_time().min(0.05);
        if previous_screen.is_some_and(|p| p != screen) && !REDUCED_MOTION.load(Ordering::Relaxed) {
            screen_fade = 1.0;
        }
        let text_editing = (matches!(screen, Screen::Menu) && menu.editing)
            || lobby_name_edit
            || (matches!(screen, Screen::JoinLobby) && guest.is_none());
        let shortcut = if is_key_pressed(KeyCode::F10) {
            Some(4)
        } else if is_key_pressed(KeyCode::F9) {
            Some(2)
        } else if is_key_pressed(KeyCode::F8) {
            Some(5)
        } else if !text_editing && is_key_pressed(KeyCode::M) {
            Some(0)
        } else {
            None
        };
        if let Some(i) = shortcut {
            let r = r_icon(i);
            handle_icon_bar_click(
                &mut settings,
                &mut snd_queue,
                vec2(r.x + r.w * 0.5, r.y + r.h * 0.5),
            );
        } else if is_mouse_button_pressed(MouseButton::Left) && confirm.is_none() {
            handle_icon_bar_click(&mut settings, &mut snd_queue, mouse_virtual());
        }

        // Reuse a quiet frame while still polling input at 60 Hz. No redraw or blit is
        // needed for an unchanged menu or a single-player human waiting to act.
        let quiet_menu = matches!(screen, Screen::Menu)
            && previous_screen == Some(screen)
            && !menu.editing
            && menu.host_err <= 0.0
            && menu.hold_t == 0.0;
        let quiet_play = matches!(screen, Screen::Play)
            && previous_screen == Some(screen)
            && host.is_none()
            && guest.is_none()
            && confirm.is_none()
            && !autoend
            && game.as_ref().is_some_and(|g| {
                g.humans == 1
                    && g.current == g.my_seat
                    && g.over.is_none()
                    && g.battle.is_none()
                    && g.selected.is_none()
                    && g.floats.is_empty()
                    && g.banner_t <= 0.0
                    && g.hint_t <= 0.0
                    && g.fx
                        .iter()
                        .all(|f| f.shake <= 0.0 && f.bounce <= 0.0 && f.flash <= 0.0)
            });
        if (quiet_menu || quiet_play) && screen_fade <= 0.0 && !engine::input_changed() {
            if quiet_menu {
                menu_time += dt;
            }
            if quiet_play {
                if let Some(g) = &mut game {
                    g.update(dt * settings.speed);
                }
            }
            if !engine::next_frame() {
                break;
            }
            continue;
        }
        previous_screen = Some(screen);

        if matches!(screen, Screen::JoinLobby) && guest.is_none() {
            lan_scan.get_or_insert_with(LanScan::new).tick(dt);
        } else {
            lan_scan = None;
        }
        if matches!(screen, Screen::HostLobby) && host.is_some() {
            let beacon = lan_beacon.get_or_insert_with(LanBeacon::new);
            if let Some(h) = &host {
                let (code, humans) = (h.code.clone(), h.joined() + 1);
                let (players, name) = (menu.players, settings.name.clone());
                beacon.tick(dt, || {
                    lan_beacon_line(net_port(), &code, humans, players, &name)
                });
            }
        } else {
            lan_beacon = None;
        }

        set_default_camera();
        clear_background(BG());
        set_camera(&view_camera());

        match screen {
            Screen::Menu => {
                menu_time += dt;
                match update_menu(&mut menu, &mut settings, &mut snd_queue, dt) {
                    MenuAction::Start => {
                        game = Some(gen_map_local(
                            menu.seed,
                            menu.players,
                            1,
                            menu.difficulty,
                            settings.team_cfg(),
                        ));
                        screen = Screen::Play;
                        snd_queue.push(Snd::Click);
                    }
                    MenuAction::Host => match HostNet::start(menu.players - 1) {
                        Ok(h) => {
                            host = Some(h);
                            lobby_picks = LobbyPicks::default();
                            lobby_sync_t = 0.0;
                            lobby_name_edit = false;
                            // preview switches to the lobby's color layout
                            let (_tv, cv) = lobby_layout(
                                &[],
                                menu.players,
                                settings.team_cfg(),
                                &lobby_picks,
                                settings.color,
                            );
                            let mut g0 = gen_map(
                                menu.seed,
                                menu.players,
                                1,
                                menu.difficulty,
                                TeamCfg::default(),
                            );
                            apply_position_swap(&mut g0, &cv);
                            menu.preview = g0;
                            screen = Screen::HostLobby;
                        }
                        Err(_) => menu.host_err = 4.0,
                    },
                    MenuAction::Join => {
                        join = JoinUi {
                            addr: settings.last_addr.clone(),
                            ..Default::default()
                        };
                        lobby_name_edit = false;
                        screen = Screen::JoinLobby;
                    }
                    MenuAction::None => {}
                }
                draw_menu(&menu, &settings, &ui, menu_time);
            }
            Screen::Play => {
                let g = game.as_mut().unwrap();
                g.update(dt * settings.speed);
                g.tick_turn_timer(dt);
                if let Some(gn) = guest.as_mut() {
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

                if autoend && g.current == 0 && g.battle.is_none() && g.over.is_none() {
                    g.end_turn(); // test hook: the host seat passes instantly
                }
                // ---- heartbeats: keep idle connections alive through NATs
                net_ping_t += dt;
                let do_ping = net_ping_t >= 2.0;
                if do_ping {
                    net_ping_t = 0.0;
                }
                // ---- hosting: apply guest intents, broadcast new events
                if let Some(h) = host.as_mut() {
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
                                        let a: Option<usize> =
                                            it.next().and_then(|v| v.parse().ok());
                                        let d: Option<usize> =
                                            it.next().and_then(|v| v.parse().ok());
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
                                h.send_resume(lobby_seat, g, menu.seed, menu.difficulty);
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

                // ---- joined: apply the host's events, queue our own intents
                if let Some(gn) = guest.as_mut() {
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
                                    gn.pending.clear();
                                }
                                Err(_) => {
                                    gn.rec_rx = None;
                                    gn.rec_failed = true;
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
                                    if gn.pending.len() >= NET_MAX_PENDING_EVENTS {
                                        reject_invalid_host_data(g, gn);
                                        break;
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
                                if let (Some(player), Some(lands), None) =
                                    (player, lands, it.next())
                                {
                                    if gn.pending.len() >= NET_MAX_PENDING_EVENTS {
                                        reject_invalid_host_data(g, gn);
                                        break;
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
                                        token.len() == 32
                                            && token.bytes().all(|b| b.is_ascii_hexdigit())
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
                            Some("RESUME") => {
                                // rebuild the match and fast-forward the backlog
                                let seed: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                                let players: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let humans: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let di: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                                let seat: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                                let (colors, teams, team_v, names) = parse_start_tail(&mut it);
                                let absent: Vec<usize> =
                                    it.next().map(parse_csv_usize).unwrap_or_default();
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
                                let players: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let humans: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let di: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                                let seat: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                                let (colors, teams, team_v, names) = parse_start_tail(&mut it);
                                gn.pending.clear();
                                gn.catching_up = false;
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
                            Some("OVER") | Some("BYE") => {
                                if g.over.is_none() {
                                    let rest: Vec<&str> = it.collect();
                                    g.over = Some(if rest.is_empty() {
                                        "Connection lost".to_string()
                                    } else {
                                        rest.join(" ")
                                    });
                                    g.banner_t = 0.0;
                                }
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
                                    RepEvent::Reinforce { player, lands } => {
                                        g.net_apply_reinforce(player, lands)
                                    }
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
                                RepEvent::Reinforce { player, lands } => {
                                    g.net_apply_reinforce(player, lands)
                                }
                            };
                            if !valid {
                                reject_invalid_host_data(g, gn);
                            }
                        }
                    }
                }
                if let Some(kind) = confirm {
                    // modal: destructive actions need explicit confirmation
                    if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) {
                        confirm = None;
                    } else if click {
                        let m = mouse_virtual();
                        if r_confirm_yes().contains(m) {
                            confirm = None;
                            match kind {
                                Confirm::NewMap => {
                                    menu.seed = random_seed();
                                    if let Some(h) = host.as_mut() {
                                        // rematch: everyone still connected moves
                                        // into the fresh round
                                        let cfg = settings.team_cfg();
                                        let (humans, tv, cv, nm) = h.start_game(
                                            menu.seed,
                                            menu.players,
                                            menu.difficulty,
                                            cfg,
                                            &lobby_picks,
                                            settings.color,
                                            &settings.name,
                                        );
                                        let mut ng = gen_map(
                                            menu.seed,
                                            menu.players,
                                            humans,
                                            menu.difficulty,
                                            cfg,
                                        );
                                        ng.team = tv;
                                        ng.names = nm;
                                        apply_colors(&cv);
                                        if ng.humans > 1 {
                                            apply_position_swap(&mut ng, &cv);
                                        }
                                        h.sent = 0;
                                        awaiting.clear();
                                        *g = ng;
                                    } else {
                                        *g = gen_map_local(
                                            menu.seed,
                                            menu.players,
                                            1,
                                            menu.difficulty,
                                            settings.team_cfg(),
                                        );
                                    }
                                }
                                Confirm::Menu => go_menu = true,
                            }
                        } else if r_confirm_no().contains(m) {
                            confirm = None;
                        }
                    }
                } else if click && g.over.is_none() && r_btn_menu().contains(mouse_virtual()) {
                    confirm = Some(Confirm::Menu);
                } else if click
                    && g.over.is_none()
                    && guest.is_none()
                    && r_btn_new().contains(mouse_virtual())
                {
                    confirm = Some(Confirm::NewMap);
                } else if is_key_pressed(KeyCode::Escape) {
                    if g.selected.is_some() {
                        g.selected = None; // ESC first cancels the selection
                    } else if g.over.is_some() {
                        go_menu = true;
                    } else {
                        confirm = Some(Confirm::Menu);
                    }
                } else if is_key_pressed(KeyCode::R) && guest.is_none() {
                    if g.over.is_some() && host.is_none() {
                        menu.seed = random_seed();
                        *g = gen_map_local(
                            menu.seed,
                            menu.players,
                            1,
                            menu.difficulty,
                            settings.team_cfg(),
                        );
                    } else {
                        confirm = Some(Confirm::NewMap);
                    }
                } else if g.over.is_some() {
                    if g.over_t >= 0.6 && click {
                        let m = mouse_virtual();
                        if r_over_replay().contains(m) {
                            replay = Some(Replay::new(g, false));
                            screen = Screen::Replay;
                        } else if r_over_gif().contains(m) {
                            replay = Some(Replay::new(g, true));
                            screen = Screen::Replay;
                        } else if r_over_new().contains(m) && guest.is_none() {
                            if host.is_none() {
                                menu.seed = random_seed();
                                *g = gen_map_local(
                                    menu.seed,
                                    menu.players,
                                    1,
                                    menu.difficulty,
                                    settings.team_cfg(),
                                );
                            } else {
                                confirm = Some(Confirm::NewMap);
                            }
                        } else if r_over_menu().contains(m) {
                            go_menu = true;
                        }
                    }
                } else if g.battle.is_none() && g.over.is_none() {
                    let my_turn = if let Some(gn) = &guest {
                        g.current == gn.seat && gn.rec_rx.is_none() && !gn.catching_up
                    } else if host.is_some() {
                        g.current == 0
                    } else {
                        g.current < g.humans
                    };
                    if my_turn {
                        if guest.is_some() {
                            let mut outbox: Vec<String> = Vec::new();
                            handle_input(g, Some(&mut outbox));
                            if let Some(gn) = guest.as_mut() {
                                for l in outbox {
                                    send_net_line(&mut gn.stream, &l);
                                }
                            }
                        } else {
                            handle_input(g, None);
                        }
                    }
                }
                draw_game(g, &ui, &settings);
                if let Some(gn) = &guest {
                    if (gn.rec_rx.is_some() || gn.rec_failed) && g.over.is_none() {
                        let txt = if gn.rec_failed {
                            "Could not reconnect — click anywhere to retry"
                        } else {
                            "Connection lost — reconnecting..."
                        };
                        let w = ui.width(txt, 22.0) + 56.0;
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
                        ui.text_centered(txt, WIN_W * 0.5, 306.0, 22.0, INK());
                    }
                }
                if let Some(kind) = confirm {
                    draw_confirm(kind, &ui);
                }
                snd_queue.append(&mut g.snd);
                if go_menu {
                    confirm = None;
                    host = None;
                    guest = None;
                    awaiting.clear();
                    apply_local_colors(settings.color);
                    screen = Screen::Menu;
                    menu.regen();
                }
            }
            Screen::HostLobby => {
                menu_time += dt;
                let click2 = is_mouse_button_pressed(MouseButton::Left);
                let m2 = mouse_virtual();
                let mut cancel = is_key_pressed(KeyCode::Escape) && !lobby_name_edit;
                if click2 && r_lobby_cancel().contains(m2) {
                    cancel = true;
                }
                let h = host.as_mut().unwrap();
                let mut dirty = false;
                for _ in 0..NET_MESSAGES_PER_FRAME {
                    let Ok(msg) = h.rx.try_recv() else { break };
                    match msg {
                        HostMsg::Joined(seat, name) => {
                            if !name.is_empty() {
                                lobby_picks.name.insert(seat, name);
                            }
                            // hand the newcomer the first color still free
                            let mut used = vec![settings.color];
                            used.extend(lobby_picks.color.values().copied());
                            if let Some(c) = (0..MAX_PLAYERS).find(|c| !used.contains(c)) {
                                lobby_picks.color.entry(seat).or_insert(c);
                            }
                            dirty = true;
                        }
                        HostMsg::Left(seat) => {
                            lobby_picks.team.remove(&seat);
                            lobby_picks.color.remove(&seat);
                            lobby_picks.name.remove(&seat);
                            dirty = true;
                        }
                        HostMsg::Intent(seat, line) => {
                            let mut it = line.split_whitespace();
                            match it.next() {
                                Some("TEAM") => {
                                    if let Some(t) = it.next().and_then(|v| v.parse::<usize>().ok())
                                    {
                                        let n = settings.team_count.clamp(2, MAX_TEAMS);
                                        lobby_picks.team.insert(seat, t % n);
                                        dirty = true;
                                    }
                                }
                                Some("COLOR") => {
                                    if let Some(c) = it.next().and_then(|v| v.parse::<usize>().ok())
                                    {
                                        let taken = c == settings.color
                                            || lobby_picks
                                                .color
                                                .iter()
                                                .any(|(&s2, &c2)| s2 != seat && c2 == c);
                                        if c < MAX_PLAYERS && !taken {
                                            lobby_picks.color.insert(seat, c);
                                            dirty = true;
                                        }
                                    }
                                }
                                Some("NAME") => {
                                    let n = clean_name(it.next().unwrap_or(""));
                                    if n.is_empty() {
                                        lobby_picks.name.remove(&seat);
                                    } else {
                                        lobby_picks.name.insert(seat, n);
                                    }
                                    dirty = true;
                                }
                                _ => {}
                            }
                        }
                        HostMsg::Rejoined(_) => {}
                    }
                }
                copied_t = (copied_t - dt).max(0.0);
                if lobby_name_edit {
                    while let Some(c) = get_char_pressed() {
                        if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                            && settings.name.len() < 12
                        {
                            settings.name.push(c);
                            dirty = true;
                        }
                    }
                    if is_key_pressed(KeyCode::Backspace) {
                        settings.name.pop();
                        dirty = true;
                    }
                    if is_key_pressed(KeyCode::Enter)
                        || is_key_pressed(KeyCode::Escape)
                        || (click2 && !r_lobby_name().contains(m2))
                    {
                        lobby_name_edit = false;
                        save_settings(&settings);
                    }
                } else {
                    while get_char_pressed().is_some() {}
                }
                if click2 {
                    // Global toolbar was processed before this frame was painted.
                    if r_lobby_copy().contains(m2) {
                        copy_to_clipboard(&format!("{}:{}", local_ip(), net_port()));
                        copied_t = 1.5;
                        snd_queue.push(Snd::Click);
                    }
                    if r_lobby_name().contains(m2) {
                        lobby_name_edit = true;
                    }
                    if r_lobby_prev().contains(m2) {
                        menu.seed = (menu.seed + SEED_MOD - 1) % SEED_MOD;
                        dirty = true;
                        snd_queue.push(Snd::Click);
                    }
                    if r_lobby_next().contains(m2) {
                        menu.seed = (menu.seed + 1) % SEED_MOD;
                        dirty = true;
                        snd_queue.push(Snd::Click);
                    }
                    if r_lobby_new().contains(m2) {
                        menu.seed = random_seed();
                        dirty = true;
                        snd_queue.push(Snd::Click);
                    }
                    for k in 0..MAX_PLAYERS - 1 {
                        if r_lobby_player(k).contains(m2) {
                            let n = h.resize_guests(k + 1);
                            menu.players = n + 1;
                            settings.players = menu.players;
                            save_settings(&settings);
                            dirty = true;
                            snd_queue.push(Snd::Click);
                        }
                    }
                    for (k, d) in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
                        .into_iter()
                        .enumerate()
                    {
                        if r_lobby_diff(k).contains(m2) {
                            menu.difficulty = d;
                            settings.difficulty = diff_idx(d);
                            save_settings(&settings);
                            dirty = true;
                            snd_queue.push(Snd::Click);
                        }
                    }
                    for k in 0..2 {
                        if r_lobby_mode(k).contains(m2) {
                            settings.teams = k == 1;
                            save_settings(&settings);
                            dirty = true;
                            snd_queue.push(Snd::Click);
                        }
                    }
                    if settings.teams {
                        for k in 0..4 {
                            if r_lobby_count(k).contains(m2) {
                                if k < 3 {
                                    settings.team_hvb = false;
                                    settings.team_count = k + 2;
                                    settings.team_mine = settings.team_mine.min(k + 1);
                                } else {
                                    settings.team_hvb = !settings.team_hvb;
                                }
                                save_settings(&settings);
                                dirty = true;
                                snd_queue.push(Snd::Click);
                            }
                        }
                        if !settings.team_hvb {
                            let count = settings.team_count.clamp(2, MAX_TEAMS);
                            for k in 0..count {
                                if r_lobby_team(k, count).contains(m2) {
                                    settings.team_mine = k;
                                    save_settings(&settings);
                                    dirty = true;
                                    snd_queue.push(Snd::Click);
                                }
                            }
                        }
                        if r_lobby_ff().contains(m2) {
                            settings.team_ff = !settings.team_ff;
                            save_settings(&settings);
                            dirty = true;
                            snd_queue.push(Snd::Click);
                        }
                        if r_lobby_bridge().contains(m2) {
                            settings.team_bridge = !settings.team_bridge;
                            save_settings(&settings);
                            dirty = true;
                            snd_queue.push(Snd::Click);
                        }
                    }
                    for i in 0..MAX_PLAYERS {
                        if r_lobby_swatch(i).contains(m2) {
                            let taken = lobby_picks.color.values().any(|&c| c == i);
                            if !taken {
                                settings.color = i;
                                apply_local_colors(i);
                                save_settings(&settings);
                                dirty = true;
                                snd_queue.push(Snd::Click);
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
                    menu.hold_t += dt;
                    if menu.hold_t > 0.4 {
                        menu.hold_next -= dt;
                        if menu.hold_next <= 0.0 {
                            menu.hold_next = 0.08;
                            menu.seed =
                                (menu.seed as i64 + held_dir).rem_euclid(SEED_MOD as i64) as u64;
                            dirty = true;
                        }
                    }
                } else {
                    menu.hold_t = 0.0;
                    menu.hold_next = 0.0;
                }
                // keep every guest's lobby view fresh
                lobby_sync_t += dt;
                if dirty {
                    // the preview shows who plays where under the current colors
                    let occ = h.occupied();
                    let (_tv, cv) = lobby_layout(
                        &occ,
                        menu.players,
                        settings.team_cfg(),
                        &lobby_picks,
                        settings.color,
                    );
                    let mut g = gen_map(
                        menu.seed,
                        menu.players,
                        1,
                        menu.difficulty,
                        TeamCfg::default(),
                    );
                    apply_position_swap(&mut g, &cv);
                    menu.preview = g;
                }
                if dirty || lobby_sync_t >= 1.5 {
                    lobby_sync_t = 0.0;
                    let line = lobby_line(h, &menu, &settings, &lobby_picks);
                    h.broadcast(&line);
                }
                let want_start = (click2 && r_lobby_start().contains(m2))
                    || (is_key_pressed(KeyCode::Enter) && !lobby_name_edit)
                    || (test_host_auto > 0 && h.joined() >= test_host_auto);
                if want_start {
                    let cfg = settings.team_cfg();
                    let (humans, tv, cv, nm) = h.start_game(
                        menu.seed,
                        menu.players,
                        menu.difficulty,
                        cfg,
                        &lobby_picks,
                        settings.color,
                        &settings.name,
                    );
                    let mut ng = gen_map(menu.seed, menu.players, humans, menu.difficulty, cfg);
                    ng.team = tv;
                    ng.names = nm;
                    apply_colors(&cv);
                    if ng.humans > 1 {
                        apply_position_swap(&mut ng, &cv);
                    }
                    h.sent = 0;
                    game = Some(ng);
                    screen = Screen::Play;
                    snd_queue.push(Snd::Turn);
                }
                draw_host_lobby(
                    h,
                    &menu,
                    &settings,
                    &lobby_picks,
                    &ui,
                    lobby_name_edit,
                    menu_time,
                    copied_t,
                );
                if cancel {
                    host = None;
                    lobby_name_edit = false;
                    menu.regen(); // back to the solo color layout
                    screen = Screen::Menu;
                }
            }
            Screen::JoinLobby => {
                menu_time += dt;
                let mut back = is_key_pressed(KeyCode::Escape) && !lobby_name_edit;
                let mut drop_guest = false;
                let click = is_mouse_button_pressed(MouseButton::Left);
                let m = mouse_virtual();
                let paste_done = join.paste_rx.as_ref().and_then(|rx| match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
                });
                if let Some(text) = paste_done {
                    join.paste_rx = None;
                    if let Some(text) = text {
                        let focus = join.paste_focus;
                        apply_join_paste(&mut join, focus, &text);
                    } else {
                        join.status = "Could not read the clipboard".to_string();
                    }
                }
                let connect_done = join.connect_rx.as_ref().and_then(|rx| match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Some(Err("Connection attempt failed".to_string()))
                    }
                });
                if let Some(result) = connect_done {
                    join.connect_rx = None;
                    match result {
                        Ok(mut g) => {
                            send_net_line(&mut g.stream, &format!("COLOR {}", settings.color));
                            send_net_line(&mut g.stream, &format!("TEAM {}", settings.team_mine));
                            guest = Some(g);
                            settings.last_addr = join.addr.clone();
                            save_settings(&settings);
                            join.status =
                                "Connected — waiting for the host to start...".to_string();
                        }
                        Err(e) => join.status = e,
                    }
                }
                if let Some(gn) = guest.as_mut() {
                    for _ in 0..NET_MESSAGES_PER_FRAME {
                        let Ok(l) = gn.rx.try_recv() else { break };
                        let mut it = l.split_whitespace();
                        match it.next() {
                            Some("START") => {
                                let seed: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                                let players: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let humans: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let di: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                                let seat =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                                let (colors, teams, team_v, names) = parse_start_tail(&mut it);
                                let g = build_guest_game(
                                    seed, players, humans, di, seat, &colors, teams, team_v, names,
                                );
                                gn.seat = g.my_seat;
                                game = Some(g);
                                screen = Screen::Play;
                                snd_queue.push(Snd::Turn);
                            }
                            Some("LOBBY") => {
                                let seed =
                                    it.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
                                        % SEED_MOD;
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
                                if join.preview_key != (seed, players)
                                    || join.preview_colors != colors
                                {
                                    let mut pg = gen_map(
                                        seed,
                                        players,
                                        1,
                                        Difficulty::Normal,
                                        TeamCfg::default(),
                                    );
                                    apply_position_swap(&mut pg, &colors);
                                    join.preview = Some(pg);
                                    join.preview_key = (seed, players);
                                    join.preview_colors = colors.clone();
                                }
                                join.view = Some(LobbyView {
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
                                join.status = "Disconnected by host".to_string();
                                drop_guest = true;
                            }
                            Some("LOST") => {
                                join.status = "Connection lost".to_string();
                                drop_guest = true;
                            }
                            _ => {}
                        }
                    }
                    if lobby_name_edit {
                        while let Some(c) = get_char_pressed() {
                            if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                                && settings.name.len() < 12
                            {
                                settings.name.push(c);
                            }
                        }
                        if is_key_pressed(KeyCode::Backspace) {
                            settings.name.pop();
                        }
                        if is_key_pressed(KeyCode::Enter)
                            || is_key_pressed(KeyCode::Escape)
                            || (click && !r_lobby_name().contains(m))
                        {
                            lobby_name_edit = false;
                            save_settings(&settings);
                            let n = if settings.name.is_empty() {
                                "-".to_string()
                            } else {
                                settings.name.clone()
                            };
                            send_net_line(&mut gn.stream, &format!("NAME {}", n));
                        }
                    } else {
                        while get_char_pressed().is_some() {}
                    }
                    if click {
                        if r_lobby_name().contains(m) {
                            lobby_name_edit = true;
                        }
                        if let Some(v) = &join.view {
                            let own_row =
                                1 + v.occupied.iter().position(|&x| x == gn.seat).unwrap_or(0);
                            let humans = v.occupied.len() + 1;
                            for i in 0..MAX_PLAYERS {
                                if r_lobby_swatch(i).contains(m) {
                                    let taken = (0..humans)
                                        .any(|r2| r2 != own_row && v.colors.get(r2) == Some(&i));
                                    if !taken {
                                        join.my_color = Some(i);
                                        settings.color = i;
                                        save_settings(&settings);
                                        send_net_line(&mut gn.stream, &format!("COLOR {}", i));
                                        snd_queue.push(Snd::Click);
                                    }
                                }
                            }
                            if v.ton && !v.thvb {
                                let count = v.tcount.clamp(2, MAX_TEAMS);
                                for k in 0..count {
                                    if r_guest_team(k, count).contains(m) {
                                        join.my_team = Some(k);
                                        settings.team_mine = k;
                                        save_settings(&settings);
                                        send_net_line(&mut gn.stream, &format!("TEAM {}", k));
                                        snd_queue.push(Snd::Click);
                                    }
                                }
                            }
                        }
                        if r_lobby_cancel().contains(m) {
                            back = true;
                        }
                    }
                } else {
                    let ctrl =
                        is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
                    let mut paste = ctrl && is_key_pressed(KeyCode::V);
                    if click && r_join_paste().contains(m) {
                        paste = true;
                    }
                    if paste {
                        if let Some(t) = paste_from_clipboard() {
                            let focus = join.focus;
                            apply_join_paste(&mut join, focus, &t);
                        } else if join.paste_rx.is_none() {
                            join.paste_focus = join.focus;
                            join.paste_rx = Some(spawn_clipboard_paste());
                            if cfg!(target_os = "linux") {
                                join.status = "Reading clipboard...".to_string();
                            }
                        }
                    }
                    while let Some(c) = get_char_pressed() {
                        if ctrl {
                            continue; // don't type the 'v' from ctrl+v
                        }
                        if join.focus == 1 {
                            if c.is_ascii_digit() && join.code.len() < NET_ROOM_CODE_LEN {
                                join.code.push(c);
                            }
                        } else if join.focus == 2 {
                            if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                                && settings.name.len() < 12
                            {
                                settings.name.push(c);
                            }
                        } else if (c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-')
                            && join.addr.len() < 40
                        {
                            join.addr.push(c);
                        }
                    }
                    if is_key_pressed(KeyCode::Backspace) {
                        match join.focus {
                            1 => {
                                join.code.pop();
                            }
                            2 => {
                                settings.name.pop();
                            }
                            _ => {
                                join.addr.pop();
                            }
                        }
                    }
                    if is_key_pressed(KeyCode::Tab) {
                        join.focus = (join.focus + 1) % 3;
                    }
                    if click && r_join_addr().contains(m) {
                        join.focus = 0;
                    }
                    if click && r_join_code().contains(m) {
                        join.focus = 1;
                    }
                    if click && r_join_name().contains(m) {
                        join.focus = 2;
                    }
                    // One click on a LAN game fills in its address and code and connects.
                    let picked = lan_scan.as_ref().and_then(|l| {
                        l.games
                            .iter()
                            .enumerate()
                            .find(|(i, _)| click && r_join_lan_row(*i).contains(m))
                            .map(|(_, g)| g.clone())
                    });
                    if let Some(g) = picked.filter(|_| join.connect_rx.is_none()) {
                        join.addr = g.addr;
                        join.code = g.code;
                        join.status = format!("Joining {}'s game...", g.name);
                        join.connect_rx = Some(spawn_guest_connect(
                            join.addr.clone(),
                            join.code.clone(),
                            settings.name.clone(),
                        ));
                    } else if join.connect_rx.is_none()
                        && ((click && r_join_go().contains(m)) || is_key_pressed(KeyCode::Enter))
                    {
                        join.status = "Connecting...".to_string();
                        join.connect_rx = Some(spawn_guest_connect(
                            join.addr.clone(),
                            join.code.clone(),
                            settings.name.clone(),
                        ));
                    }
                    if click && r_join_back().contains(m) {
                        back = true;
                    }
                }
                if drop_guest {
                    guest = None;
                    join.view = None;
                }
                if let Some(gn) = &guest {
                    draw_guest_lobby(&join, gn.seat, &settings, &ui, lobby_name_edit, menu_time);
                } else {
                    draw_join_lobby(&join, lan_scan.as_ref(), &settings, &ui, menu_time);
                }
                if back {
                    guest = None;
                    join.view = None;
                    join.connect_rx = None;
                    lobby_name_edit = false;
                    screen = Screen::Menu;
                }
            }
            Screen::Replay => {
                let r = replay.as_mut().unwrap();
                let speed = if r.saving { 1.0 } else { 2.2 };
                r.g.update(dt * speed);
                let mut exit = is_key_pressed(KeyCode::Escape);
                if r.saving {
                    if r.saved.is_none() && r.idx < r.events.len() {
                        r.apply_instant();
                    }
                } else {
                    r.step_watch(dt * speed);
                }
                if r.saving && r.saved.is_none() {
                    if r.frame_count < gif_capture_count(r.idx, r.events.len()) {
                        if let Some(rt) = r.rt.clone() {
                            // Render the frame offscreen and read it back;
                            // capturing the live screen mid-frame corrupts
                            // the live framebuffer.
                            set_camera(&Camera2D {
                                target: vec2(WIN_W * 0.5, WIN_H * 0.5),
                                zoom: vec2(2.0 / WIN_W, 2.0 / WIN_H),
                                render_target: Some(rt.clone()),
                                ..Default::default()
                            });
                            clear_background(BG());
                            draw_board(&r.g, &ui, false);
                            draw_panel_replay(&r.g, &ui);
                            set_camera(&view_camera());
                            let ss = downsample(&rt.texture.get_texture_data(), 2);
                            let frame = quantize_image(&ss);
                            match r.gif.as_mut().map(|gif| gif.write_frame(&frame)) {
                                Some(Ok(())) => r.frame_count += 1,
                                _ => {
                                    r.gif.take();
                                    r.saved = Some("Could not save the GIF".to_string());
                                }
                            }
                        } else {
                            r.gif.take();
                            r.saved = Some("Could not render the GIF".to_string());
                        }
                    } else if r.idx >= r.events.len() {
                        r.saved = Some(match r.gif.take().map(GifWriter::finish) {
                            Some(Ok(path)) => format!("Saved {}", path),
                            _ => "Could not save the GIF".to_string(),
                        });
                        snd_queue.push(Snd::Reinforce);
                    }
                }
                draw_board(&r.g, &ui, false);
                draw_panel_replay(&r.g, &ui);
                draw_battle_overlay(&r.g, &ui);
                draw_replay_hud(r, &ui);
                draw_icon_bar(&settings, &ui, mouse_virtual());
                if (r.finished() || r.saved.is_some()) && is_mouse_button_pressed(MouseButton::Left)
                {
                    exit = true;
                }
                if r.saving {
                    r.g.snd.clear();
                } else {
                    snd_queue.append(&mut r.g.snd);
                }
                if exit {
                    replay = None;
                    screen = Screen::Menu;
                    menu.regen();
                }
            }
        }

        if screen_fade > 0.0 {
            set_default_camera();
            let veil = SRF();
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                with_alpha(veil, 0.85 * screen_fade * screen_fade),
            );
            screen_fade = (screen_fade - dt / 0.22).max(0.0);
        }

        if let Some(s) = &sounds {
            s.volume.set(if settings.quiet { 0.5 } else { 1.0 });
            if settings.muted {
                snd_queue.clear();
            }
            for e in snd_queue.drain(..) {
                s.play(e);
            }
        } else {
            snd_queue.clear();
        }

        set_default_camera();
        if !engine::next_frame() {
            break;
        }
    }
    // Documentation art: `DW_EXPORT_SVG=dir` writes the final board and the
    // logo as SVG (headless runs only need DW_AUTOSTART and a frame limit).
    if let Some(dir) = std::env::var_os("DW_EXPORT_SVG") {
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        if let Some(g) = &game {
            if let Err(e) = std::fs::write(dir.join("board.svg"), svg::board(g)) {
                eprintln!("SVG export failed: {e}");
            }
        }
        if let Err(e) = std::fs::write(dir.join("logo.svg"), svg::logo()) {
            eprintln!("SVG export failed: {e}");
        }
    }
    engine::shutdown();
}
