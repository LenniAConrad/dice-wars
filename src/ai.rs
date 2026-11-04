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
    // Easy and Normal score attacks by odds and temperament; Hard evaluates
    // the board after each possible outcome (hard_attack below).
    pub(crate) fn best_ai_attack(&self) -> Option<(usize, usize)> {
        if self.difficulty == Difficulty::Hard {
            return self.hard_attack();
        }
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
        let coord = if self.difficulty == Difficulty::Easy {
            0.15
        } else {
            1.0
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
                let mut need: f32 = if self.difficulty == Difficulty::Easy {
                    0.65
                } else {
                    0.50
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

// Hard bots weigh each attack by what the board is worth to them after a
// win and after a loss, against what it is worth now. Same dice, same
// information and the same reinforcement rule as everyone else.
struct HardW {
    region: f32, // per territory in the largest region (= dice per turn)
    terr: f32,   // per territory owned
    dice: f32,   // per die on own land
    risk: f32,   // expected losses to the strongest adjacent enemy stack
    main: f32,   // extra worth of a territory inside the largest region
    enemy: f32,  // per territory an enemy's largest region shrinks by
    thresh: f32, // minimum expected gain to keep attacking
}

// tuned with the headless simulator (1 Hard vs 5 Normal, DW_SIM)
const HARD: HardW = HardW {
    region: 1.5,
    terr: 0.5,
    dice: 0.6,
    risk: 1.0,
    main: 1.0,
    enemy: 0.3,
    thresh: 0.0,
};

impl Game {
    // largest connected group of p's land on a hypothetical board (team
    // bridging as in largest_region); `main` marks its territories
    fn region_on(&self, p: usize, owner: &[usize], main: Option<&mut Vec<bool>>) -> u32 {
        let bridge = self.teams.on && self.teams.bridge;
        let n = owner.len();
        let mut comp = vec![usize::MAX; n];
        let (mut best, mut best_id) = (0u32, usize::MAX);
        for start in 0..n {
            if comp[start] != usize::MAX || owner[start] != p {
                continue;
            }
            let mut size = 0u32;
            let mut stack = vec![start];
            comp[start] = start;
            while let Some(t) = stack.pop() {
                if owner[t] == p {
                    size += 1;
                }
                for &nb in &self.terrs[t].neighbors {
                    let o = owner[nb];
                    if comp[nb] == usize::MAX && (o == p || (bridge && self.same_team(p, o))) {
                        comp[nb] = start;
                        stack.push(nb);
                    }
                }
            }
            if size > best {
                best = size;
                best_id = start;
            }
        }
        if let Some(m) = main {
            for t in 0..n {
                m[t] = comp[t] == best_id && owner[t] == p;
            }
        }
        best
    }

    // what this board is worth to `me`
    fn hard_value(&self, me: usize, owner: &[usize], dice: &[u32]) -> f32 {
        let w = &HARD;
        let mut main = vec![false; owner.len()];
        let region = self.region_on(me, owner, Some(&mut main));
        let (mut terr, mut dsum, mut risk) = (0.0f32, 0.0f32, 0.0f32);
        for t in 0..owner.len() {
            if owner[t] != me {
                continue;
            }
            terr += 1.0;
            dsum += dice[t] as f32;
            let threat = self.terrs[t]
                .neighbors
                .iter()
                .filter(|&&e| !self.is_ally(me, owner[e]) && dice[e] >= 2)
                .map(|&e| win_chance_cached(dice[e], dice[t]))
                .fold(0.0f32, f32::max);
            let worth = 1.0 + if main[t] { w.main } else { 0.0 } + 0.1 * dice[t] as f32;
            risk += threat * worth;
        }
        w.region * region as f32 + w.terr * terr + w.dice * dsum - w.risk * risk
    }

    fn hard_attack(&self) -> Option<(usize, usize)> {
        let me = self.current;
        let w = &HARD;
        let mut owner: Vec<usize> = self.terrs.iter().map(|t| t.owner).collect();
        let mut dice: Vec<u32> = self.terrs.iter().map(|t| t.dice).collect();
        // a saturated board must never stall (see best_ai_attack)
        let stuck = self.reserve[me] >= 8
            || (0..owner.len()).all(|t| owner[t] != me || dice[t] == MAX_DICE);
        let mut need = w.thresh;
        need += match self.personas[me] {
            Persona::Aggressive => -0.15,
            Persona::Defensive => 0.15,
            _ => 0.0,
        };
        let now = self.hard_value(me, &owner, &dice);
        let total = owner.len() as f32;
        let mut best: Option<(f32, usize, usize)> = None;
        for a in 0..owner.len() {
            if owner[a] != me || dice[a] < 2 {
                continue;
            }
            for &d in &self.terrs[a].neighbors {
                let def = owner[d];
                if self.is_ally(me, def) {
                    continue;
                }
                let p = win_chance_cached(dice[a], dice[d]);
                if p < 0.25 {
                    continue;
                }
                let (da, dd) = (dice[a], dice[d]);
                let def_region = self.region_on(def, &owner, None) as f32;
                // after a win
                owner[d] = me;
                dice[d] = da - 1;
                dice[a] = 1;
                let mut win = self.hard_value(me, &owner, &dice);
                let shrink = def_region - self.region_on(def, &owner, None) as f32;
                // hurting whoever is ahead matters most
                let share = self.count(def) as f32 / total;
                win += w.enemy * shrink * (0.5 + 2.0 * share);
                // after a loss
                owner[d] = def;
                dice[d] = dd;
                let lose = self.hard_value(me, &owner, &dice);
                dice[a] = da;
                let gain = p * win + (1.0 - p) * lose - now;
                let ok = gain > need || (stuck && p >= 0.42);
                if ok && best.is_none_or(|b| gain > b.0) {
                    best = Some((gain, a, d));
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
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
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
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
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
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
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
