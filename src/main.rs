// Release builds on Windows start as a GUI app without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use dicegame::engine::{self, prelude::*};

use crate::board::gen_map_local;
use crate::dice::draw_die;
use crate::game::{diff_idx, split_sum, Battle, Game, RepEvent, MAX_PLAYERS, SEED_MOD};
use crate::gif::downsample;
use crate::lan::{lan_beacon_line, LanBeacon, LanScan};
use crate::lobby::JoinUi;
use crate::menu::Menu;
use crate::net::{guest_connect, net_port, GuestNet, HostNet, LobbyPicks};
use crate::play::Confirm;
use crate::replay::Replay;
use crate::settings::{load_settings, save_settings, Settings};
use crate::sound::{Snd, Sounds};
use crate::theme::{
    apply_local_colors, darken, mix, set_theme, with_alpha, Palette, BG, COLORBLIND, PLAYER_BASE,
    REDUCED_MOTION, SRF, THEMES,
};
use crate::ui::{
    close_settings, draw_settings_panel, handle_toolbar_click, mouse_virtual, settings_open,
    toolbar_shortcut, view_camera, Ui, WIN_H, WIN_W,
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
mod setup;
mod sound;
mod svg;
#[cfg(test)]
mod testutil;
mod theme;
mod ui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Menu,
    Play,
    Replay,
    HostLobby,
    JoinLobby,
}

/// Everything the frame loop keeps between frames. Each screen's per-frame
/// update lives next to that screen's drawing code (`App::run_menu` in
/// menu.rs, `run_play` in play.rs, the lobbies in lobby.rs, `run_replay` in
/// replay.rs).
pub(crate) struct App {
    pub(crate) ui: Ui,
    pub(crate) sounds: Option<Sounds>,
    pub(crate) settings: Settings,
    pub(crate) menu: Menu,
    pub(crate) screen: Screen,
    pub(crate) game: Option<Game>,
    pub(crate) replay: Option<Replay>,
    pub(crate) host: Option<HostNet>,
    pub(crate) guest: Option<GuestNet>,
    pub(crate) join: JoinUi,
    pub(crate) menu_time: f32,
    pub(crate) net_ping_t: f32,
    pub(crate) lobby_picks: LobbyPicks,
    pub(crate) lobby_sync_t: f32,
    pub(crate) lobby_name_edit: bool,
    pub(crate) awaiting: HashMap<usize, f32>, // game seat -> grace timer
    pub(crate) copied_t: f32,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) snd_queue: Vec<Snd>,
    pub(crate) autoend: bool,
    pub(crate) test_host_auto: usize,
    pub(crate) lan_scan: Option<LanScan>,
    pub(crate) lan_beacon: Option<LanBeacon>,
    pub(crate) previous_screen: Option<Screen>,
    // Screen changes fade in from the surface color instead of cutting hard.
    pub(crate) screen_fade: f32,
}

fn main() {
    if let Err(error) = engine::init("Dice Wars", WIN_W as usize, WIN_H as usize) {
        engine::fatal_error(&error);
        std::process::exit(1);
    }
    engine::random::srand(engine::now() as u64);
    let mut app = App::new();
    while app.frame() {}
    // Documentation art: `DW_EXPORT_SVG=dir` writes the final board and the
    // logo as SVG (headless runs only need DW_AUTOSTART and a frame limit).
    if let Some(dir) = std::env::var_os("DW_EXPORT_SVG") {
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        if let Some(g) = &app.game {
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

// `DW_EXPORT_ICON=1` renders the application icon to assets/icon.png and exits.
fn export_icon() -> ! {
    let rt = render_target(1024, 1024);
    set_camera(&Camera2D {
        target: vec2(128.0, 128.0),
        zoom: vec2(2.0 / 256.0, 2.0 / 256.0),
        render_target: Some(rt.clone()),
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

impl App {
    // Startup state: saved settings, environment overrides and the automation
    // scenes used by the headless audits and network tests.
    fn new() -> App {
        let ui = Ui {
            font: Font::embedded(),
            body: Font::embedded_body(),
        };
        let sounds = Sounds::load();
        let mut settings = load_settings();
        if let Ok(theme) = std::env::var("DW_THEME") {
            settings.theme = match theme.as_str() {
                "light" => 0,
                "dark" => 1,
                name => THEMES
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(name))
                    .unwrap_or(0),
            };
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
        set_theme(settings.theme);
        REDUCED_MOTION.store(settings.reduced_motion, Ordering::Relaxed);
        if std::env::var("DW_EXPORT_ICON").is_ok() {
            export_icon();
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
        let menu_time = 0.0f32;
        let net_ping_t = 0.0f32;
        let lobby_picks = LobbyPicks::default();
        let lobby_sync_t = 0.0f32;
        let lobby_name_edit = false;
        let awaiting: HashMap<usize, f32> = HashMap::new(); // game seat -> grace timer
        let copied_t = 0.0f32;
        let mut confirm: Option<Confirm> = None;
        let snd_queue: Vec<Snd> = Vec::new();
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
                    // the longest result text, so the audit shows it is never cut off
                    g.over = Some("Buttercup conquered the whole map!".to_string());
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

        let lan_scan: Option<LanScan> = None;
        let lan_beacon: Option<LanBeacon> = None;
        let previous_screen = None;
        // Screen changes fade in from the surface color instead of cutting hard.
        let screen_fade = 0.0f32;
        App {
            ui,
            sounds,
            settings,
            menu,
            screen,
            game,
            replay,
            host,
            guest,
            join,
            menu_time,
            net_ping_t,
            lobby_picks,
            lobby_sync_t,
            lobby_name_edit,
            awaiting,
            copied_t,
            confirm,
            snd_queue,
            autoend,
            test_host_auto,
            lan_scan,
            lan_beacon,
            previous_screen,
            screen_fade,
        }
    }

    // End any network session: guests are told the host left, and the local
    // colors come back. Used whenever play ends up back at the menu.
    /// Counts a finished solo game in the menu's record, once, on the frame
    /// it ends. Network games and bot-only test runs are not counted.
    fn record_result(&mut self) {
        let Some(g) = self.game.as_ref() else { return };
        if g.over.is_none()
            || self.autoend
            || self.host.is_some()
            || self.guest.is_some()
            || g.humans != 1
        {
            return;
        }
        let d = diff_idx(g.difficulty).min(2);
        self.settings.played[d] += 1;
        if g.my_side_won() {
            self.settings.won[d] += 1;
        }
        save_settings(&self.settings);
    }

    pub(crate) fn leave_network(&mut self) {
        self.host = None;
        self.guest = None;
        self.awaiting.clear();
        apply_local_colors(self.settings.color);
    }

    // One frame; false once the window is closed or the frame limit is hit.
    fn frame(&mut self) -> bool {
        let dt = get_frame_time().min(0.05);
        if self.previous_screen.is_some_and(|p| p != self.screen)
            && !REDUCED_MOTION.load(Ordering::Relaxed)
        {
            self.screen_fade = 1.0;
        }
        let text_editing = (matches!(self.screen, Screen::Menu) && self.menu.editing)
            || self.lobby_name_edit
            || (matches!(self.screen, Screen::JoinLobby) && self.guest.is_none());
        toolbar_shortcut(&mut self.settings, &mut self.snd_queue, text_editing);
        // The toolbar and its settings panel sit above every screen: clicks
        // and Esc they use never reach the screen underneath.
        if is_mouse_button_pressed(MouseButton::Left)
            && self.confirm.is_none()
            && handle_toolbar_click(&mut self.settings, &mut self.snd_queue, mouse_virtual())
        {
            engine::consume_click();
        }
        if settings_open() && is_key_pressed(KeyCode::Escape) {
            close_settings();
            engine::consume_key(KeyCode::Escape);
        }

        // Reuse a quiet frame while still polling input at 60 Hz. No redraw or blit is
        // needed for an unchanged menu or a single-player human waiting to act.
        let quiet_menu = matches!(self.screen, Screen::Menu)
            && self.previous_screen == Some(self.screen)
            && !self.menu.editing
            && self.menu.host_err <= 0.0
            && self.menu.hold_t == 0.0;
        let quiet_play = matches!(self.screen, Screen::Play)
            && self.previous_screen == Some(self.screen)
            && self.host.is_none()
            && self.guest.is_none()
            && self.confirm.is_none()
            && !self.autoend
            && self.game.as_ref().is_some_and(|g| {
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
        if (quiet_menu || quiet_play)
            && self.screen_fade <= 0.0
            && !engine::input_changed()
            && !crate::ui::ui_animating()
        {
            if quiet_menu {
                self.menu_time += dt;
            }
            if quiet_play {
                if let Some(g) = &mut self.game {
                    g.update(dt * self.settings.speed);
                }
            }
            return engine::next_frame();
        }
        self.previous_screen = Some(self.screen);

        if matches!(self.screen, Screen::JoinLobby) && self.guest.is_none() {
            self.lan_scan.get_or_insert_with(LanScan::new).tick(dt);
        } else {
            self.lan_scan = None;
        }
        if matches!(self.screen, Screen::HostLobby) && self.host.is_some() {
            let beacon = self.lan_beacon.get_or_insert_with(LanBeacon::new);
            if let Some(h) = &self.host {
                let (code, humans) = (h.code.clone(), h.joined() + 1);
                let (players, name) = (self.menu.players, self.settings.name.clone());
                beacon.tick(dt, || {
                    lan_beacon_line(net_port(), &code, humans, players, &name)
                });
            }
        } else {
            self.lan_beacon = None;
        }

        // in a joined game the host's speed applies; the panel shows it
        crate::ui::set_host_speed(
            self.guest
                .as_ref()
                .filter(|_| matches!(self.screen, Screen::Play))
                .map(|gn| gn.speed),
        );
        set_default_camera();
        clear_background(BG());
        set_camera(&view_camera());
        match self.screen {
            Screen::Menu => self.run_menu(dt),
            Screen::Play => {
                let was_over = self.game.as_ref().is_some_and(|g| g.over.is_some());
                self.run_play(dt);
                if !was_over {
                    self.record_result();
                }
            }
            Screen::HostLobby => self.run_host_lobby(dt),
            Screen::JoinLobby => self.run_join_lobby(dt),
            Screen::Replay => self.run_replay(dt),
        }
        set_camera(&view_camera());
        draw_settings_panel(&self.settings, &self.ui, mouse_virtual());

        if self.screen_fade > 0.0 {
            set_default_camera();
            let veil = SRF();
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                with_alpha(veil, 0.85 * self.screen_fade * self.screen_fade),
            );
            self.screen_fade = (self.screen_fade - dt / 0.22).max(0.0);
        }

        if let Some(s) = &self.sounds {
            s.volume.set(if self.settings.quiet { 0.5 } else { 1.0 });
            if self.settings.muted {
                self.snd_queue.clear();
            }
            for e in self.snd_queue.drain(..) {
                s.play(e);
            }
        } else {
            self.snd_queue.clear();
        }

        set_default_camera();
        engine::next_frame()
    }
}
