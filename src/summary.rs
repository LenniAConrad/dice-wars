//! The end-of-game summary: how the map changed hands, and a few numbers.
//! Built once when a game ends by replaying its recorded events.

use dicegame::engine::prelude::*;

use crate::ai::win_chance_cached;
use crate::game::{Game, RepEvent};
use crate::replay::Replay;
use crate::theme::{palette, with_alpha, CARD_EDGE, INK};
use crate::ui::{draw_round_frame, label, Ui};

pub(crate) struct Summary {
    /// Lands per player, sampled at the start and after every turn.
    pub(crate) series: Vec<Vec<u16>>,
    pub(crate) players: usize,
    pub(crate) turns: u32,
    pub(crate) battles: u32,
    pub(crate) captures: u32,
    /// The least likely won attack: (attacker, its win chance).
    pub(crate) luckiest: Option<(usize, f32)>,
    /// The biggest stack taken in one attack: (attacker, defender's dice).
    pub(crate) biggest: Option<(usize, u32)>,
}

impl Summary {
    pub(crate) fn of(game: &Game) -> Self {
        let mut r = Replay::new(game, false);
        let lands = |g: &Game| -> Vec<u16> { (0..g.players).map(|p| g.count(p) as u16).collect() };
        let mut s = Summary {
            series: vec![lands(&r.g)],
            players: game.players,
            turns: 0,
            battles: 0,
            captures: 0,
            luckiest: None,
            biggest: None,
        };
        while r.idx < r.events.len() {
            match r.events[r.idx].clone() {
                RepEvent::Attack { a, d, captured, .. } => {
                    s.battles += 1;
                    let (ad, dd) = (r.g.terrs[a].dice, r.g.terrs[d].dice);
                    let attacker = r.g.terrs[a].owner;
                    if captured {
                        s.captures += 1;
                        let chance = win_chance_cached(ad, dd);
                        if s.luckiest.is_none_or(|(_, c)| chance < c) {
                            s.luckiest = Some((attacker, chance));
                        }
                        if s.biggest.is_none_or(|(_, n)| dd > n) {
                            s.biggest = Some((attacker, dd));
                        }
                    }
                    r.apply_instant();
                }
                RepEvent::Reinforce { .. } => {
                    s.turns += 1;
                    r.apply_instant();
                    s.series.push(lands(&r.g));
                }
            }
        }
        // the final position, if the game ended mid-turn
        let last = lands(&r.g);
        if s.series.last() != Some(&last) {
            s.series.push(last);
        }
        s
    }

    /// Full rounds played (every player's turn counts once per round).
    pub(crate) fn rounds(&self) -> u32 {
        self.turns.div_ceil(self.players.max(1) as u32)
    }
}

/// Land share over the game: one line per player, in their color.
pub(crate) fn draw_chart(s: &Summary, ui: &Ui, area: Rect, alpha: f32) {
    draw_round_frame(
        area.x,
        area.y,
        area.w,
        area.h,
        12.0,
        1.5,
        Color::new(0.0, 0.0, 0.0, 0.0),
        with_alpha(CARD_EDGE(), alpha),
    );
    let pad = 14.0;
    let (x0, y0) = (area.x + pad, area.y + pad);
    let (w, h) = (area.w - pad * 2.0, area.h - pad * 2.0 - 14.0);
    let max = s
        .series
        .iter()
        .flat_map(|row| row.iter().copied())
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    // quiet guide lines at quarter heights
    for k in 1..4 {
        let y = y0 + h * k as f32 / 4.0;
        draw_line(x0, y, x0 + w, y, 1.0, with_alpha(CARD_EDGE(), alpha));
    }
    let n = s.series.len();
    // at most ~140 points per line; long games are sampled evenly
    let step = n.div_ceil(140).max(1);
    let samples: Vec<usize> = (0..n).step_by(step).chain(std::iter::once(n - 1)).collect();
    let point = |i: usize, p: usize| {
        let fx = if n > 1 {
            i as f32 / (n - 1) as f32
        } else {
            0.0
        };
        vec2(x0 + fx * w, y0 + h - h * s.series[i][p] as f32 / max)
    };
    for p in 0..s.players {
        let col = with_alpha(palette(p).dark, alpha);
        for pair in samples.windows(2) {
            let (a, b) = (point(pair[0], p), point(pair[1], p));
            draw_line(a.x, a.y, b.x, b.y, 2.2, col);
            draw_circle(b.x, b.y, 1.1, col);
        }
    }
    label(ui, "LANDS OVER THE GAME", x0, area.y + area.h - 10.0);
}

/// Four numbers in a row of small tiles.
pub(crate) fn draw_stats(s: &Summary, game: &Game, ui: &Ui, area: Rect, alpha: f32) {
    let who = |p: usize| game.name_of(p).to_uppercase();
    let tiles: [(String, String); 4] = [
        (s.rounds().to_string(), "ROUNDS".to_string()),
        (
            s.battles.to_string(),
            format!("BATTLES · {} WON", s.captures),
        ),
        match s.luckiest {
            Some((p, c)) => (
                format!("{:.0}%", c * 100.0),
                format!("LUCKIEST · {}", who(p)),
            ),
            None => ("—".to_string(), "LUCKIEST WIN".to_string()),
        },
        match s.biggest {
            Some((p, d)) => (d.to_string(), format!("BIGGEST TAKE · {}", who(p))),
            None => ("—".to_string(), "BIGGEST TAKE".to_string()),
        },
    ];
    let gap = 10.0;
    let tw = (area.w - gap * 3.0) / 4.0;
    for (i, (value, caption)) in tiles.iter().enumerate() {
        let x = area.x + i as f32 * (tw + gap);
        draw_round_frame(
            x,
            area.y,
            tw,
            area.h,
            12.0,
            1.5,
            Color::new(0.0, 0.0, 0.0, 0.0),
            with_alpha(CARD_EDGE(), alpha),
        );
        ui.text_centered(
            value,
            x + tw * 0.5,
            area.y + 30.0,
            20.0,
            with_alpha(INK(), alpha),
        );
        let size = ui.fit_size(caption, 10.0, tw - 12.0);
        ui.text_centered(
            caption,
            x + tw * 0.5,
            area.y + area.h - 11.0,
            size,
            with_alpha(INK(), 0.62 * alpha),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::gen_map;
    use crate::game::{Difficulty, TeamCfg};
    use crate::testutil::GLOBALS;

    #[test]
    fn a_summary_follows_the_recorded_game() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let mut game = gen_map(31337, 4, 1, Difficulty::Normal, TeamCfg::default());
        // the human seat passes at once; bots play
        for _ in 0..20_000 {
            if game.current == 0 && game.battle.is_none() && game.over.is_none() {
                game.end_turn();
            }
            game.update(1.0 / 30.0);
            if (game.events.len() > 60 || game.over.is_some()) && game.battle.is_none() {
                break;
            }
        }
        assert!(game.events.len() > 10, "the bots should have played");
        let s = Summary::of(&game);
        assert_eq!(
            s.series[0].iter().map(|&n| n as usize).sum::<usize>(),
            game.terrs.len()
        );
        let last = s.series.last().unwrap();
        for p in 0..game.players {
            assert_eq!(
                last[p] as usize,
                game.count(p),
                "replayed lands of player {p}"
            );
        }
        assert!(s.battles >= s.captures && s.turns > 0);
    }
}
