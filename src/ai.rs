//! Bot move selection.

use dicegame::engine::random::gen_range;

use crate::game::{Difficulty, Game, Persona, MAX_DICE, MAX_TEAMS};

// exact probability that `a` dice roll a strictly higher sum than `d` dice
fn win_chance(a: u32, d: u32) -> f32 {
    fn dist(n: u32) -> Vec<f64> {
        let mut v = vec![1.0f64];
        for _ in 0..n {
            let mut nv = vec![0.0; v.len() + 6];
            for (s, p) in v.iter().enumerate() {
                for f in 1..=6usize {
                    nv[s + f] += p / 6.0;
                }
            }
            v = nv;
        }
        v
    }
    let da = dist(a);
    let dd = dist(d);
    let mut p = 0.0;
    for (sa, pa) in da.iter().enumerate() {
        for (sd, pd) in dd.iter().enumerate() {
            if sa > sd {
                p += pa * pd;
            }
        }
    }
    p as f32
}

// exact odds, precomputed once: the AI reads these like a player reads
// the % hints
pub(crate) fn win_chance_cached(a: u32, d: u32) -> f32 {
    static T: std::sync::OnceLock<[[f32; 9]; 9]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [[0.0f32; 9]; 9];
        for (a, row) in t.iter_mut().enumerate().skip(1) {
            for (d, cell) in row.iter_mut().enumerate().skip(1) {
                *cell = win_chance(a as u32, d as u32);
            }
        }
        t
    })[a.clamp(1, 8) as usize][d.clamp(1, 8) as usize]
}

impl Game {
    // Pick the AI's next attack based on its personality and the difficulty.
    pub(crate) fn best_ai_attack(&self) -> Option<(usize, usize)> {
        let cur = self.current;
        let persona = self.personas[cur];

        // easy bots lose their nerve and end their turn early often
        if self.difficulty == Difficulty::Easy && gen_range(0.0f32, 1.0) < 0.55 {
            return None;
        }

        // a saturated board must never stall: with every stack full (or a
        // reserve overflowing) the bot attacks even at even odds
        let stuck = self.reserve[cur] >= 8
            || (0..self.terrs.len())
                .all(|t| self.terrs[t].owner != cur || self.terrs[t].dice == MAX_DICE);

        // who is currently winning (for the schemer and the shared instinct):
        // in team mode the threat is the strongest enemy TEAM, so weaker
        // teams naturally gang up on whoever is running away with the game
        let mut leader: Option<usize> = None;
        let mut lead_team: Option<usize> = None;
        let threat;
        if self.teams.on {
            let mut size = [0usize; MAX_TEAMS];
            for p in 0..self.players {
                size[self.team_of(p) % MAX_TEAMS] += self.count(p);
            }
            let my = self.team_of(cur) % MAX_TEAMS;
            lead_team = (0..MAX_TEAMS)
                .filter(|&t| t != my && size[t] > 0)
                .max_by_key(|&t| size[t]);
            threat = lead_team
                .map(|t| size[t] as f32 / self.terrs.len() as f32)
                .unwrap_or(0.0);
        } else {
            leader = (0..self.players)
                .filter(|&p| p != cur && self.count(p) > 0)
                .max_by_key(|&p| self.count(p));
            threat = leader
                .map(|p| self.count(p) as f32 / self.terrs.len() as f32)
                .unwrap_or(0.0);
        }
        // is this owner part of the leading side?
        let is_lead = |o: usize| {
            if self.teams.on {
                lead_team == Some(self.team_of(o))
            } else {
                leader == Some(o)
            }
        };
        // how strongly this bot coordinates against a runaway leader
        let coord = match self.difficulty {
            Difficulty::Easy => 0.15,
            Difficulty::Normal => 1.0,
            Difficulty::Hard => 1.6,
        };

        let mut best: Option<(f32, usize, usize)> = None;
        for a in 0..self.terrs.len() {
            let ta = &self.terrs[a];
            if ta.owner != cur || ta.dice < 2 {
                continue;
            }
            for &d in &ta.neighbors {
                let td = &self.terrs[d];
                // bots never turn on their own team, whatever the settings say
                if self.is_ally(cur, td.owner) {
                    continue;
                }
                // decide on the exact win probability, not a crude dice gap
                let p = win_chance_cached(ta.dice, td.dice);
                let mut need: f32 = match self.difficulty {
                    Difficulty::Easy => 0.65,
                    Difficulty::Normal => 0.50,
                    Difficulty::Hard => 0.42,
                };
                need += match persona {
                    Persona::Aggressive => -0.06,
                    Persona::Defensive => 0.08,
                    _ => 0.0,
                };
                if stuck {
                    need = need.min(0.42); // full stacks accept even odds
                }
                if p < need {
                    continue;
                }
                // when one player is running away with the game, stop wearing
                // each other down and turn on the leader instead
                if threat > 0.45
                    && !is_lead(td.owner)
                    && p < 0.60
                    && !stuck
                    && self.difficulty != Difficulty::Easy
                {
                    continue;
                }
                let mut score = (p - 0.5) * 120.0 + ta.dice as f32;
                if threat > 0.34 {
                    if is_lead(td.owner) {
                        score += (threat - 0.3) * 90.0 * coord;
                    } else {
                        score -= (threat - 0.3) * 60.0 * coord;
                    }
                }
                if self.difficulty != Difficulty::Easy {
                    // prefer attacking from positions with friendly backup
                    let backup = ta
                        .neighbors
                        .iter()
                        .filter(|&&n| self.terrs[n].owner == cur)
                        .count();
                    score += backup as f32 * 1.5;
                }
                if self.difficulty == Difficulty::Easy {
                    score += gen_range(0.0f32, 40.0); // sloppy target choice
                }
                if self.difficulty == Difficulty::Hard {
                    // values conquests that consolidate territory
                    let friends = td
                        .neighbors
                        .iter()
                        .filter(|&&n| self.terrs[n].owner == cur)
                        .count();
                    score += friends as f32 * 4.0;
                }
                match persona {
                    Persona::Aggressive => score += 4.0,
                    Persona::Defensive => score += (p - 0.5) * 60.0,
                    Persona::Greedy => {
                        let friends = td
                            .neighbors
                            .iter()
                            .filter(|&&n| self.terrs[n].owner == cur)
                            .count();
                        score += friends as f32 * 6.0;
                    }
                    Persona::Balancer => {
                        if is_lead(td.owner) {
                            score += 15.0;
                        }
                    }
                    Persona::Chaotic => score += gen_range(0.0f32, 18.0),
                }
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, a, d));
                }
            }
        }
        best.map(|(_, a, d)| (a, d))
    }
}

#[cfg(test)]
mod tests {
    use crate::board::gen_map;
    use crate::game::{TeamCfg, HUMAN};
    use crate::testutil::{mk_game, GLOBALS};
    use crate::theme::apply_local_colors;

    use super::*;

    #[test]
    fn saturated_bot_never_stalls() {
        let _g = GLOBALS.lock().unwrap();
        let mut g = mk_game(&[1, 0], &[(0, 1)], 2, TeamCfg::default());
        g.current = 1;
        g.personas[1] = Persona::Defensive; // the most hesitant temperament
        for t in g.terrs.iter_mut() {
            t.dice = MAX_DICE;
        }
        assert_eq!(g.best_ai_attack(), Some((0, 1)));
    }

    #[test]
    fn hard_attack_choices_do_not_depend_on_human_seats() {
        let _g = GLOBALS.lock().unwrap();
        let mut g = mk_game(&[2, 1, 0], &[(0, 1), (0, 2)], 3, TeamCfg::default());
        g.current = 2;
        g.difficulty = Difficulty::Hard;
        g.personas[2] = Persona::Defensive;
        g.terrs[0].dice = 3;
        let one_human = g.best_ai_attack();
        g.humans = 2;
        assert_eq!(one_human, g.best_ai_attack());
        assert_eq!(one_human, Some((0, 1)));
    }

    // the strongest verification: a passive human who only stacks dice must
    // still be ground down — bots may never deadlock a saturated board
    #[test]
    fn bots_always_finish_the_game() {
        let _g = GLOBALS.lock().unwrap();
        apply_local_colors(0);
        for difficulty in [Difficulty::Normal, Difficulty::Hard] {
            let mut g = gen_map(31337, 4, 1, difficulty, TeamCfg::default());
            for _ in 0..30_000 {
                if g.over.is_some() {
                    break;
                }
                if g.current == HUMAN {
                    g.end_turn(); // the human never attacks, only hoards
                } else if let Some((a, d)) = g.best_ai_attack() {
                    g.begin_attack(a, d);
                    let b = g.battle.take().unwrap();
                    g.apply_battle(b);
                } else {
                    g.end_turn();
                }
            }
            assert!(g.over.is_some(), "the bots stalled out the game");
        }
    }
}
