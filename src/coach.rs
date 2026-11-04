//! First-game tips: three speech bubbles that point at the next thing to do
//! (pick a land, attack a neighbor, end the turn), shown until the player
//! has ended a turn once or skips them.

use dicegame::engine::{self, prelude::*};

use crate::ai::win_chance_cached;
use crate::game::Game;
use crate::play::r_endturn;
use crate::settings::save_settings;
use crate::theme::{with_alpha, ACCENT, INK, SRF};
use crate::ui::{draw_round_frame, mouse_virtual, soft_shadow, Ui, WIN_W};
use crate::App;

#[derive(Default)]
pub(crate) struct Coach {
    attacks: u32,       // attacks you made since the tips started
    seen_event: usize,  // events already counted
    skip: Option<Rect>, // the SKIP TIPS link drawn last frame
    tip_turn: bool,     // you have been told about END TURN this turn
}

// your land with the best attack available, if any
fn best_start(g: &Game) -> Option<usize> {
    let me = g.my_seat;
    (0..g.terrs.len())
        .filter(|&t| g.terrs[t].owner == me && g.terrs[t].dice > 1)
        .filter_map(|t| {
            let best = g.terrs[t]
                .neighbors
                .iter()
                .filter(|&&n| g.can_target(me, g.terrs[n].owner))
                .map(|&n| win_chance_cached(g.terrs[t].dice, g.terrs[n].dice))
                .fold(None, |a: Option<f32>, c| Some(a.map_or(c, |a| a.max(c))))?;
            Some((t, best))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(t, _)| t)
}

fn best_target(g: &Game, from: usize) -> Option<(usize, f32)> {
    let me = g.my_seat;
    g.terrs[from]
        .neighbors
        .iter()
        .filter(|&&n| g.can_target(me, g.terrs[n].owner))
        .map(|&n| (n, win_chance_cached(g.terrs[from].dice, g.terrs[n].dice)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

impl App {
    fn coach_active(&self) -> bool {
        !self.settings.tutorial_done
            && self.host.is_none()
            && self.guest.is_none()
            && !self.autoend
            && self.confirm.is_none()
            && self
                .game
                .as_ref()
                .is_some_and(|g| g.humans == 1 && g.over.is_none() && g.recording)
    }

    /// Before the game handles input: the SKIP link, and progress tracking.
    pub(crate) fn coach_input(&mut self) {
        if !self.coach_active() {
            return;
        }
        if is_mouse_button_pressed(MouseButton::Left)
            && self.coach.skip.is_some_and(|r| r.contains(mouse_virtual()))
        {
            engine::consume_click();
            self.finish_coach();
        }
    }

    fn finish_coach(&mut self) {
        self.settings.tutorial_done = true;
        save_settings(&self.settings);
        self.coach = Coach::default();
    }

    /// After the game is drawn: count your attacks, retire after your first
    /// ended turn, and draw the bubble for the current step.
    pub(crate) fn coach_draw(&mut self) {
        self.coach.skip = None;
        if !self.coach_active() {
            return;
        }
        let g = self.game.as_ref().unwrap();
        if let Some(b) = &g.battle {
            if g.terrs[b.a].owner == g.my_seat && g.events.len() != self.coach.seen_event {
                self.coach.seen_event = g.events.len();
                self.coach.attacks += 1;
            }
        }
        if g.current != g.my_seat {
            if self.coach.tip_turn {
                self.finish_coach(); // you ended a turn: that's the whole loop
            }
            return;
        }
        if g.battle.is_some() || g.banner_t > 0.0 {
            return;
        }
        let (target, title, body) = if let Some(sel) = g.selected {
            match best_target(g, sel) {
                Some((t, c)) => (
                    g.terrs[t].anchor + vec2(0.0, -64.0),
                    "Attack a neighbor".to_string(),
                    format!(
                        "Click a land next to yours. More dice, better odds: this one is {:.0}%.",
                        c * 100.0
                    ),
                ),
                None => (
                    g.terrs[sel].anchor + vec2(0.0, -64.0),
                    "No target here".to_string(),
                    "This land has no enemy neighbors. Click it again, then pick another."
                        .to_string(),
                ),
            }
        } else if self.coach.attacks == 0 {
            match best_start(g) {
                Some(t) => (
                    g.terrs[t].anchor + vec2(0.0, -64.0),
                    "Pick one of your lands".to_string(),
                    "Choose a land with 2 or more dice. It attacks with all of them.".to_string(),
                ),
                None => {
                    self.coach.tip_turn = true;
                    (
                        endturn_point(),
                        "Nothing to attack yet".to_string(),
                        "End your turn to get new dice for your biggest region.".to_string(),
                    )
                }
            }
        } else {
            self.coach.tip_turn = true;
            let n = g.largest_region(g.my_seat) + g.reserve[g.my_seat];
            (
                endturn_point(),
                "Attack again, or end your turn".to_string(),
                format!("END TURN adds {n} dice: one per land in your biggest connected region."),
            )
        };
        let skip = draw_bubble(&self.ui, target, &title, &body);
        self.coach.skip = Some(skip);
    }
}

fn endturn_point() -> Vec2 {
    let r = r_endturn();
    vec2(r.x + r.w * 0.5, r.y)
}

// A floating card with a pointer at `to`; returns the SKIP TIPS rectangle.
fn draw_bubble(ui: &Ui, to: Vec2, title: &str, body: &str) -> Rect {
    let w = 300.0;
    let lines = wrap(ui, body, 13.0, w - 32.0);
    let h = 50.0 + lines.len() as f32 * 18.0 + 22.0;
    // above the point when there is room, otherwise below
    let above = to.y - h - 14.0 > 60.0;
    let y = if above { to.y - h - 14.0 } else { to.y + 14.0 };
    let x = (to.x - w * 0.5).clamp(16.0, WIN_W - w - 16.0);
    soft_shadow(x, y, w, h, 14.0, 1.6);
    let tip_y = if above { y + h } else { y };
    let dir = if above { 1.0 } else { -1.0 };
    let tx = to.x.clamp(x + 20.0, x + w - 20.0);
    draw_triangle(
        vec2(tx - 9.0, tip_y - dir),
        vec2(tx + 9.0, tip_y - dir),
        vec2(tx, tip_y + dir * 11.0),
        ACCENT(),
    );
    draw_round_frame(x, y, w, h, 14.0, 2.5, SRF(), ACCENT());
    ui.text(title, x + 16.0, y + 28.0, 15.0, INK());
    for (i, line) in lines.iter().enumerate() {
        ui.body(
            line,
            x + 16.0,
            y + 50.0 + i as f32 * 18.0,
            13.0,
            with_alpha(INK(), 0.8),
        );
    }
    let skip = "SKIP TIPS";
    let sw = ui.width(skip, 10.0);
    let r = Rect::new(x + w - sw - 24.0, y + h - 24.0, sw + 12.0, 18.0);
    let hov = r.contains(mouse_virtual());
    ui.text(
        skip,
        r.x + 6.0,
        r.y + 13.0,
        10.0,
        if hov {
            ACCENT()
        } else {
            with_alpha(INK(), 0.55)
        },
    );
    r
}

// greedy word wrap for body text
fn wrap(ui: &Ui, text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if ui.body_width(&candidate, size) > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
