//! Watching a finished match again.

use dicegame::engine::prelude::*;

use crate::board::gen_map_local;
use crate::draw_game::{draw_battle_overlay, draw_board, draw_panel_replay};
use crate::game::{split_sum, Battle, Game, RepEvent, MAX_DICE};
use crate::gif::{downsample, gif_capture_count, quantize_image, GifWriter, GIF_H, GIF_W};
use crate::settings::replay_file;
use crate::sound::Snd;
use crate::theme::{BG, BORDER, INK, SRF};
use crate::ui::{draw_icon_bar, draw_round_frame, mouse_virtual, view_camera, Ui, WIN_H, WIN_W};
use crate::{App, Screen};

pub(crate) struct Replay {
    pub(crate) g: Game,
    pub(crate) events: Vec<RepEvent>,
    pub(crate) idx: usize,
    pub(crate) timer: f32,
    pub(crate) saving: bool, // gif-capture mode: no animations, one frame per event
    pub(crate) gif: Option<GifWriter>,
    pub(crate) frame_count: usize,
    pub(crate) rt: Option<RenderTarget>,
    pub(crate) saved: Option<String>, // status message once the gif is written
}

impl Replay {
    pub(crate) fn new(src: &Game, saving: bool) -> Self {
        let mut g = gen_map_local(src.seed, src.players, src.humans, src.difficulty, src.teams);
        g.names = src.names.clone();
        // Lobby teams and the dealt board (after color swaps) come from the
        // finished game itself, never from regenerating with local colors.
        g.team = src.team.clone();
        if src.start.len() == g.terrs.len() {
            for (t, &(owner, dice)) in g.terrs.iter_mut().zip(&src.start) {
                t.owner = owner;
                t.dice = dice;
            }
        }
        g.recording = false;
        g.banner_t = 0.0;
        let (gif, saved) = if saving {
            let path = replay_file(&format!("replay-{:08}.gif", src.seed));
            match GifWriter::create(&path, GIF_W as u16, GIF_H as u16, 12) {
                Ok(gif) => (Some(gif), None),
                Err(_) => (None, Some("Could not create the GIF".to_string())),
            }
        } else {
            (None, None)
        };
        Replay {
            g,
            events: src.events.clone(),
            idx: 0,
            timer: 0.0,
            saving,
            gif,
            frame_count: 0,
            rt: if saving && saved.is_none() {
                Some(render_target(GIF_W * 2, GIF_H * 2))
            } else {
                None
            },
            saved,
        }
    }

    pub(crate) fn finished(&self) -> bool {
        self.idx >= self.events.len() && self.g.battle.is_none()
    }

    // animated playback step
    pub(crate) fn step_watch(&mut self, dt: f32) {
        if self.g.battle.is_some() || self.idx >= self.events.len() {
            return;
        }
        self.timer += dt;
        if self.timer < 0.4 {
            return;
        }
        self.timer = 0.0;
        let ev = self.events[self.idx].clone();
        self.idx += 1;
        match ev {
            RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured,
            } => {
                let ad = split_sum(ra, self.g.terrs[a].dice);
                let dd = split_sum(rd, self.g.terrs[d].dice);
                self.g.battle = Some(Battle {
                    a,
                    d,
                    ra,
                    rd,
                    ad,
                    dd,
                    captured,
                    t: 0.0,
                });
                self.g.fx[a].shake = 1.0;
                self.g.fx[d].shake = 1.0;
                let roll = self.g.roll_sound(a, d);
                self.g.snd.push(roll);
            }
            RepEvent::Reinforce { player, lands } => {
                let n = lands.len();
                for t in lands {
                    if self.g.terrs[t].dice < MAX_DICE {
                        self.g.terrs[t].dice += 1;
                        self.g.fx[t].bounce = 1.0;
                    }
                }
                let nm = self.g.name_of(player);
                self.g
                    .push_log(format!("{} reinforced with {} dice.", nm, n));
            }
        }
    }

    // instant application, used while recording the gif
    pub(crate) fn apply_instant(&mut self) {
        if self.idx >= self.events.len() {
            return;
        }
        let ev = self.events[self.idx].clone();
        self.idx += 1;
        match ev {
            RepEvent::Attack { a, d, captured, .. } => {
                if captured {
                    self.g.fx[d].from = self.g.terrs[d].owner;
                    self.g.terrs[d].owner = self.g.terrs[a].owner;
                    self.g.terrs[d].dice = self.g.terrs[a].dice - 1;
                    self.g.fx[d].flash = 1.0;
                }
                self.g.terrs[a].dice = 1;
            }
            RepEvent::Reinforce { lands, .. } => {
                for t in lands {
                    if self.g.terrs[t].dice < MAX_DICE {
                        self.g.terrs[t].dice += 1;
                    }
                }
            }
        }
    }
}

fn draw_replay_hud(r: &Replay, ui: &Ui) {
    let txt = if let Some(msg) = &r.saved {
        format!("{} — click or ESC for menu", msg)
    } else if r.saving {
        format!("Recording GIF... {}/{}", r.idx, r.events.len())
    } else if r.finished() {
        "Replay finished — click or ESC for menu".to_string()
    } else {
        format!("REPLAY  {}/{}   ESC = exit", r.idx, r.events.len())
    };
    let w = 570.0;
    draw_round_frame(30.0, 16.0, w, 44.0, 14.0, 2.5, SRF(), BORDER());
    ui.text_fitted(&txt, 48.0, 45.0, 16.0, w - 36.0, INK());
}

impl App {
    // One frame of a replay, writing GIF frames while exporting.
    pub(crate) fn run_replay(&mut self, dt: f32) {
        let r = self.replay.as_mut().unwrap();
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
                    });
                    clear_background(BG());
                    draw_board(&r.g, &self.ui, false);
                    draw_panel_replay(&r.g, &self.ui);
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
                self.snd_queue.push(Snd::Reinforce);
            }
        }
        draw_board(&r.g, &self.ui, false);
        draw_panel_replay(&r.g, &self.ui);
        draw_battle_overlay(&r.g, &self.ui);
        draw_replay_hud(r, &self.ui);
        draw_icon_bar(&self.settings, &self.ui, mouse_virtual());
        if (r.finished() || r.saved.is_some()) && is_mouse_button_pressed(MouseButton::Left) {
            exit = true;
        }
        if r.saving {
            r.g.snd.clear();
        } else {
            self.snd_queue.append(&mut r.g.snd);
        }
        if exit {
            self.replay = None;
            self.leave_network(); // a replay after a network game ends it
            self.screen = Screen::Menu;
            self.menu.regen();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::board::gen_map;
    use crate::game::{Difficulty, TeamCfg};
    use crate::gif::gif_capture_count;
    use crate::testutil::GLOBALS;

    use super::*;

    #[test]
    fn long_gifs_sample_the_whole_match_including_the_ending() {
        for total in [0, 1, 599, 600, 601, 1000, 1201, 30000] {
            let mut captured = 0;
            let mut last_capture = 0;
            for index in 1..=total {
                let desired = gif_capture_count(index, total);
                if desired > captured {
                    captured += 1;
                    last_capture = index;
                }
            }
            assert_eq!(captured, total.min(600));
            assert_eq!(last_capture, total);
        }
    }

    #[test]
    fn replay_never_schedules_new_ai_actions() {
        let _guard = GLOBALS.lock().unwrap();
        let src = gen_map(31337, 4, 1, Difficulty::Normal, TeamCfg::default());
        let mut replay = Replay::new(&src, false);
        replay.g.current = 1;
        let dice = replay
            .g
            .terrs
            .iter()
            .map(|t| (t.owner, t.dice))
            .collect::<Vec<_>>();
        replay.g.update(5.0);
        assert_eq!(replay.g.current, 1);
        assert!(replay.g.battle.is_none());
        assert_eq!(
            replay
                .g
                .terrs
                .iter()
                .map(|t| (t.owner, t.dice))
                .collect::<Vec<_>>(),
            dice
        );
    }
}
