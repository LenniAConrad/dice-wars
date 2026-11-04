//! Match state and rules: territories, battles, turns, teams and reinforcements.

use std::collections::HashMap;

use dicegame::engine::prelude::*;
use dicegame::engine::random::gen_range;

use crate::board::shuffle;
use crate::draw_game::battle_settle_time;
use crate::sound::Snd;
use crate::theme::{color_map, palette, COLOR_NAMES};

pub(crate) const MAX_PLAYERS: usize = 9;

pub(crate) const MAX_DICE: u32 = 8;

pub(crate) const HUMAN: usize = 0;

const AI_STEP: f32 = 0.35; // pause between AI actions (on top of battle time)

pub(crate) const TURN_SECS: f32 = 60.0; // multiplayer idle limit before the turn auto-ends

// battle animation timing
pub(crate) const T_SHAKE: f32 = 0.45; // dice rattle before the rolls appear

const T_BATTLE: f32 = 1.65; // total battle length

pub(crate) const SEED_MOD: u64 = 100_000_000; // seeds are 8 decimal digits

const HUMAN_LABELS: [&str; MAX_PLAYERS] = ["P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9"];

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Difficulty {
    Easy,
    Normal,
    Hard,
}

// team mode: every player keeps their own dice, but the surviving team wins
// together; the human picks a team and the bots fill up whatever is left
pub(crate) const MAX_TEAMS: usize = 4;

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct TeamCfg {
    pub(crate) on: bool,
    pub(crate) count: usize,   // number of teams (2..=MAX_TEAMS)
    pub(crate) hvb: bool,      // humans-vs-bots: overrides count/my_team
    pub(crate) my_team: usize, // the team seat 0 (the local human / host) plays on
    pub(crate) bridge: bool,   // split islands count as connected through teammate land
    pub(crate) ff: bool,       // friendly fire: teammates may attack each other
}

// every human may pick a team; bots then fill the smallest teams so the
// sides stay balanced. Deterministic, so host and guests agree.
pub(crate) fn fill_teams(
    players: usize,
    humans: usize,
    cfg: TeamCfg,
    picks: &[Option<usize>],
) -> Vec<usize> {
    if !cfg.on {
        return vec![0; players];
    }
    if cfg.hvb {
        return if humans < players {
            (0..players).map(|p| usize::from(p >= humans)).collect()
        } else {
            // an all-human lobby cannot be humans-vs-bots; alternate instead
            (0..players).map(|p| p % 2).collect()
        };
    }
    let n = cfg.count.clamp(2, MAX_TEAMS);
    let mut team = vec![0usize; players];
    let mut size = vec![0usize; n];
    for (p, slot) in team.iter_mut().enumerate().take(humans.min(players)) {
        let t = picks.get(p).copied().flatten().unwrap_or(p + cfg.my_team) % n;
        *slot = t;
        size[t] += 1;
    }
    for t in team.iter_mut().take(players).skip(humans) {
        let smallest = (0..n).min_by_key(|&k| (size[k], k)).unwrap();
        *t = smallest;
        size[smallest] += 1;
    }
    team
}

pub(crate) fn assign_teams(players: usize, humans: usize, cfg: TeamCfg) -> Vec<usize> {
    fill_teams(players, humans, cfg, &[])
}

pub(crate) fn team_letter(t: usize) -> &'static str {
    ["A", "B", "C", "D"][t % MAX_TEAMS]
}

fn team_name(t: usize) -> &'static str {
    ["Team A", "Team B", "Team C", "Team D"][t % MAX_TEAMS]
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Persona {
    Aggressive, // attacks on any non-losing odds
    Defensive,  // only attacks with a clear advantage
    Greedy,     // loves conquests that consolidate its own region
    Balancer,   // schemes against whoever is winning
    Chaotic,    // unpredictable
}

// everything needed to replay a finished game (the map itself comes from the seed)
#[derive(Clone)]
pub(crate) enum RepEvent {
    Attack {
        a: usize,
        d: usize,
        ra: u32,
        rd: u32,
        captured: bool,
    },
    Reinforce {
        player: usize,
        lands: Vec<usize>,
    },
}

pub(crate) struct Territory {
    pub(crate) owner: usize,
    pub(crate) dice: u32,
    pub(crate) neighbors: Vec<usize>,
    pub(crate) anchor: Vec2,
    pub(crate) seed: u32, // stable pseudo-random die faces
}

// per-territory animation state, all timers run 1 -> 0
#[derive(Clone, Copy, Default)]
pub(crate) struct TerrFx {
    pub(crate) shake: f32,
    pub(crate) bounce: f32,
    pub(crate) flash: f32,
    pub(crate) from: usize, // previous owner while a capture cross-fades its land color
}

#[derive(Clone)]
pub(crate) struct Battle {
    pub(crate) a: usize,
    pub(crate) d: usize,
    pub(crate) ra: u32,
    pub(crate) rd: u32,
    pub(crate) ad: Vec<u8>, // attacker's individual dice
    pub(crate) dd: Vec<u8>, // defender's individual dice
    pub(crate) captured: bool,
    pub(crate) t: f32,
}

pub(crate) struct FloatText {
    pub(crate) pos: Vec2,
    pub(crate) text: String,
    pub(crate) col: Color,
    pub(crate) size: f32,
    pub(crate) t: f32,
    pub(crate) life: f32,
}

pub(crate) struct Game {
    pub(crate) players: usize,
    pub(crate) humans: usize, // players 0..humans are people (host, then guests), the rest are AI
    pub(crate) teams: TeamCfg,
    pub(crate) team: Vec<usize>, // per-player team id, from assign_teams
    pub(crate) names: Vec<String>, // lobby names for human seats; empty = P1/P2 labels
    pub(crate) absent: Vec<bool>, // human seats a bot took over after a disconnect
    pub(crate) turn_timer: f32,  // multiplayer: seconds left before the turn auto-ends
    pub(crate) difficulty: Difficulty,
    pub(crate) personas: Vec<Persona>, // one per player; index 0 (human) is unused
    pub(crate) seed: u64,
    pub(crate) reserve: Vec<u32>, // per player: reinforcements stored when all lands are full (max 64)
    pub(crate) recording: bool,   // live game: record events; replays don't
    pub(crate) net_guest: bool,   // this instance mirrors a remote host: no AI, no dice rolls
    pub(crate) my_seat: usize,    // which seat this screen controls (0 = host/local)
    pub(crate) events: Vec<RepEvent>,
    pub(crate) cell_terr: Vec<i32>, // -1 = not part of the map
    pub(crate) terrs: Vec<Territory>,
    pub(crate) fx: Vec<TerrFx>,
    pub(crate) current: usize,
    pub(crate) selected: Option<usize>,
    pub(crate) keyboard_hover: Option<usize>,
    pub(crate) log: Vec<String>, // recent events, newest last
    pub(crate) hint: String,     // why the last click was invalid
    pub(crate) hint_t: f32,
    pub(crate) ai_timer: f32,
    pub(crate) over: Option<String>,
    pub(crate) over_t: f32,
    pub(crate) battle: Option<Battle>,
    pub(crate) floats: Vec<FloatText>,
    pub(crate) banner: String,
    pub(crate) banner_t: f32,
    pub(crate) time: f32,
    pub(crate) snd: Vec<Snd>, // sound events, drained and played by the main loop
}

// partition a known dice total back into n plausible faces (display only)
pub(crate) fn split_sum(sum: u32, n: u32) -> Vec<u8> {
    let n = n.max(1);
    let mut v = vec![1u8; n as usize];
    let mut rem = sum.saturating_sub(n).min(n * 5);
    let mut i = 0usize;
    while rem > 0 {
        let idx = i % n as usize;
        let room = 6 - v[idx] as u32;
        let add = rem.min(room).min(gen_range(1u32, 6u32));
        v[idx] += add as u8;
        rem -= add;
        i += 1;
    }
    shuffle(&mut v);
    v
}

impl Game {
    pub(crate) fn count(&self, p: usize) -> usize {
        self.terrs.iter().filter(|t| t.owner == p).count()
    }

    // a human seat that lost its player is bot-controlled until they return
    pub(crate) fn is_human(&self, p: usize) -> bool {
        p < self.humans && !self.absent.get(p).copied().unwrap_or(false)
    }

    pub(crate) fn name_of(&self, p: usize) -> String {
        if p < self.humans {
            if self.humans == 1 {
                return "You".to_string();
            }
            match self.names.get(p) {
                Some(n) if !n.is_empty() => n.clone(),
                _ => HUMAN_LABELS[p % MAX_PLAYERS].to_string(),
            }
        } else {
            COLOR_NAMES[color_map(p)].to_string()
        }
    }

    // multiplayer only: an idle human forfeits the turn after a minute;
    // ticks on real time (unscaled by the speed toggle), frozen in battles
    pub(crate) fn tick_turn_timer(&mut self, dt: f32) {
        if self.humans < 2
            || !self.is_human(self.current)
            || self.over.is_some()
            || self.battle.is_some()
        {
            return;
        }
        self.turn_timer -= dt;
        if self.turn_timer <= 0.0 {
            self.turn_timer = TURN_SECS;
            if !self.net_guest {
                let n = self.name_of(self.current);
                self.push_log(format!("{} ran out of time — turn passed.", n));
                self.end_turn();
            }
        }
    }

    pub(crate) fn team_of(&self, p: usize) -> usize {
        self.team.get(p).copied().unwrap_or(0)
    }

    pub(crate) fn same_team(&self, a: usize, b: usize) -> bool {
        self.teams.on && self.team_of(a) == self.team_of(b)
    }

    // own land, or a teammate's (AI never attacks allies; reinforcement
    // frontlines treat ally borders as safe)
    pub(crate) fn is_ally(&self, p: usize, owner: usize) -> bool {
        owner == p || self.same_team(p, owner)
    }

    // may player p attack a land with this owner?
    pub(crate) fn can_target(&self, p: usize, owner: usize) -> bool {
        owner != p && (self.teams.ff || !self.same_team(p, owner))
    }

    pub(crate) fn largest_region(&self, p: usize) -> u32 {
        // with ally bridging, the flood may pass through teammate land but
        // only the player's own territories count toward the region size
        let bridge = self.teams.on && self.teams.bridge;
        let n = self.terrs.len();
        let mut seen = vec![false; n];
        let mut best = 0u32;
        for start in 0..n {
            if seen[start] || self.terrs[start].owner != p {
                continue;
            }
            let mut size = 0u32;
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(t) = stack.pop() {
                if self.terrs[t].owner == p {
                    size += 1;
                }
                for &nb in &self.terrs[t].neighbors {
                    let o = self.terrs[nb].owner;
                    if !seen[nb] && (o == p || (bridge && self.same_team(p, o))) {
                        seen[nb] = true;
                        stack.push(nb);
                    }
                }
            }
            best = best.max(size);
        }
        best
    }

    pub(crate) fn begin_attack(&mut self, a: usize, d: usize) {
        let ad: Vec<u8> = (0..self.terrs[a].dice)
            .map(|_| gen_range(1u32, 7u32) as u8)
            .collect();
        let dd: Vec<u8> = (0..self.terrs[d].dice)
            .map(|_| gen_range(1u32, 7u32) as u8)
            .collect();
        let ra: u32 = ad.iter().map(|&x| x as u32).sum();
        let rd: u32 = dd.iter().map(|&x| x as u32).sum();
        self.battle = Some(Battle {
            a,
            d,
            ra,
            rd,
            ad,
            dd,
            captured: ra > rd,
            t: 0.0,
        });
        // recorded (and broadcast) the moment the battle starts, so remote
        // players watch the same animation at the same time
        if self.recording {
            self.events.push(RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured: ra > rd,
            });
        }
        self.fx[a].shake = 1.0;
        self.fx[d].shake = 1.0;
        self.selected = None;
        self.turn_timer = TURN_SECS; // acting resets the idle clock
        self.snd.push(self.roll_sound(a, d));
    }

    // Sounds speak from this screen's point of view; fights between other
    // players are quieter and neutral.
    pub(crate) fn involves_me(&self, a: usize, d: usize) -> bool {
        self.terrs[a].owner == self.my_seat || self.terrs[d].owner == self.my_seat
    }

    pub(crate) fn roll_sound(&self, a: usize, d: usize) -> Snd {
        Snd::Roll {
            big: self.terrs[a].dice + self.terrs[d].dice >= 10,
            quiet: !self.involves_me(a, d),
        }
    }

    pub(crate) fn result_sound(&self, b: &Battle) -> Snd {
        let me = self.my_seat;
        match (
            self.terrs[b.a].owner == me,
            self.terrs[b.d].owner == me,
            b.captured,
        ) {
            (true, _, true) => Snd::Capture,
            (true, _, false) => Snd::Repel,
            (_, true, true) => Snd::Lost,
            (_, true, false) => Snd::Defended,
            (_, _, true) => Snd::OtherCapture,
            (_, _, false) => Snd::OtherRepel,
        }
    }

    pub(crate) fn push_log(&mut self, s: String) {
        self.log.push(s);
        if self.log.len() > 6 {
            self.log.remove(0);
        }
    }

    pub(crate) fn set_hint(&mut self, s: &str) {
        self.hint = s.to_string();
        self.hint_t = 3.0;
        self.snd.push(Snd::Invalid);
    }

    pub(crate) fn apply_battle(&mut self, b: Battle) {
        let atk = self.terrs[b.a].owner;
        let def = self.terrs[b.d].owner;
        if b.captured {
            self.fx[b.d].from = def;
            self.terrs[b.d].owner = atk;
            self.terrs[b.d].dice = self.terrs[b.a].dice - 1;
            self.fx[b.d].flash = 1.0;
            self.fx[b.d].bounce = 1.0;
            self.push_log(format!(
                "{} rolled {} vs {} {} — captured!",
                self.name_of(atk),
                b.ra,
                self.name_of(def),
                b.rd
            ));
        } else {
            self.fx[b.a].bounce = 1.0;
            self.push_log(format!(
                "{} rolled {} vs {} {} — repelled!",
                self.name_of(atk),
                b.ra,
                self.name_of(def),
                b.rd
            ));
        }
        self.terrs[b.a].dice = 1;
        if b.captured && self.count(def) == 0 {
            self.push_log(format!("{} eliminated!", self.name_of(def)));
            self.snd.push(Snd::Eliminated);
        }

        if !self.recording && !self.net_guest {
            return; // pure replays never end the game themselves
        }

        self.check_game_over();
    }

    pub(crate) fn check_game_over(&mut self) {
        if self.over.is_some() {
            return;
        }
        let humans_alive = (0..self.humans).filter(|&p| self.count(p) > 0).count();
        if humans_alive == 0 {
            self.over = Some(if self.humans == 1 {
                "You have been wiped out!".to_string()
            } else {
                "The AI has conquered you all!".to_string()
            });
            self.banner_t = 0.0;
            self.snd.push(Snd::Lose);
        } else if self.teams.on {
            let mut alive = [false; MAX_TEAMS];
            for p in 0..self.players {
                if self.count(p) > 0 {
                    alive[self.team_of(p) % MAX_TEAMS] = true;
                }
            }
            if alive.iter().filter(|&&a| a).count() == 1 {
                let t = alive.iter().position(|&a| a).unwrap();
                let mine = self.team_of(self.my_seat) == t;
                self.over = Some(if self.teams.hvb {
                    if t == 0 {
                        if self.humans == 1 {
                            "You have defeated the bots!".to_string()
                        } else {
                            "The humans are victorious!".to_string()
                        }
                    } else {
                        "The bots have conquered you!".to_string()
                    }
                } else if self.humans == 1 {
                    if mine {
                        "Your team is victorious!".to_string()
                    } else {
                        "The enemy team has won!".to_string()
                    }
                } else {
                    format!("{} is victorious!", team_name(t))
                });
                self.banner_t = 0.0;
                self.snd.push(if mine { Snd::Win } else { Snd::Lose });
            }
        } else if let Some(w) = (0..self.players).find(|&p| self.count(p) == self.terrs.len()) {
            self.over = Some(if w == HUMAN && self.humans == 1 {
                "You conquered the whole map!".to_string()
            } else {
                format!("{} conquered the whole map!", self.name_of(w))
            });
            self.banner_t = 0.0;
            self.snd
                .push(if w < self.humans { Snd::Win } else { Snd::Lose });
        }
    }

    pub(crate) fn end_turn(&mut self) {
        let p = self.current;
        // stored dice from earlier turns join this turn's reinforcements
        let n = self.largest_region(p) + self.reserve[p];
        let mut reinforced: Vec<usize> = Vec::new();
        for _ in 0..n {
            // Every seat uses the same random placement rule, including Hard
            // bots: strategy never grants control over reinforcement locations.
            let cand: Vec<usize> = (0..self.terrs.len())
                .filter(|&t| self.terrs[t].owner == p && self.terrs[t].dice < MAX_DICE)
                .collect();
            if cand.is_empty() {
                break;
            }
            let t = cand[gen_range(0, cand.len())];
            self.terrs[t].dice += 1;
            self.fx[t].bounce = 1.0;
            reinforced.push(t);
        }
        // whatever could not be placed is stored off-board, capped like the original
        self.reserve[p] = (n - reinforced.len() as u32).min(64);
        if let Some(&t) = reinforced.first() {
            self.floats.push(FloatText {
                pos: self.terrs[t].anchor + vec2(0.0, -92.0),
                text: format!("+{}", reinforced.len()),
                col: palette(p).dark,
                size: 36.0,
                t: 0.0,
                life: 1.4,
            });
        }
        if self.reserve[p] > 0 {
            self.push_log(format!(
                "{} reinforced with {} dice ({} stored).",
                self.name_of(p),
                reinforced.len(),
                self.reserve[p]
            ));
        } else {
            self.push_log(format!(
                "{} reinforced with {} dice.",
                self.name_of(p),
                reinforced.len()
            ));
        }
        if self.recording {
            self.events.push(RepEvent::Reinforce {
                player: p,
                lands: reinforced.clone(),
            });
        }
        if p < self.humans && !reinforced.is_empty() {
            self.snd.push(Snd::Reinforce);
        }

        self.advance_turn();
    }

    pub(crate) fn advance_turn(&mut self) {
        for step in 1..=self.players {
            let q = (self.current + step) % self.players;
            if self.count(q) > 0 {
                self.current = q;
                break;
            }
        }
        self.selected = None;
        self.ai_timer = -0.3;
        self.turn_timer = TURN_SECS;
        if self.current < self.humans {
            self.snd.push(Snd::Turn);
        }
        self.banner = if self.current == HUMAN && self.humans == 1 {
            "Your turn".to_string()
        } else {
            format!("{}'s turn", self.name_of(self.current))
        };
        self.banner_t = 1.6;
    }

    pub(crate) fn valid_net_attack(
        &self,
        a: usize,
        d: usize,
        ra: u32,
        rd: u32,
        captured: bool,
    ) -> bool {
        let (Some(atk), Some(def)) = (self.terrs.get(a), self.terrs.get(d)) else {
            return false;
        };
        let attacker = atk.owner;
        self.over.is_none()
            && a != d
            && atk.dice > 1
            && atk.dice <= MAX_DICE
            && def.dice >= 1
            && def.dice <= MAX_DICE
            && attacker == self.current
            && self.can_target(attacker, def.owner)
            && atk.neighbors.contains(&d)
            && (atk.dice..=atk.dice * 6).contains(&ra)
            && (def.dice..=def.dice * 6).contains(&rd)
            && captured == (ra > rd)
    }

    pub(crate) fn net_apply_attack(
        &mut self,
        a: usize,
        d: usize,
        ra: u32,
        rd: u32,
        captured: bool,
    ) -> bool {
        if !self.valid_net_attack(a, d, ra, rd, captured) {
            return false;
        }
        let ad = split_sum(ra, self.terrs[a].dice);
        let dd = split_sum(rd, self.terrs[d].dice);
        if self.recording {
            self.events.push(RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured,
            });
        }
        self.battle = Some(Battle {
            a,
            d,
            ra,
            rd,
            ad,
            dd,
            captured,
            t: 0.0,
        });
        self.fx[a].shake = 1.0;
        self.fx[d].shake = 1.0;
        self.selected = None;
        self.turn_timer = TURN_SECS;
        self.snd.push(self.roll_sound(a, d));
        true
    }

    // instant attack used while replaying a reconnect backlog
    pub(crate) fn net_apply_attack_fast(
        &mut self,
        a: usize,
        d: usize,
        ra: u32,
        rd: u32,
        captured: bool,
    ) -> bool {
        if !self.valid_net_attack(a, d, ra, rd, captured) {
            return false;
        }
        if self.recording {
            self.events.push(RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured,
            });
        }
        if captured {
            self.terrs[d].owner = self.terrs[a].owner;
            self.terrs[d].dice = self.terrs[a].dice.saturating_sub(1).max(1);
        }
        self.terrs[a].dice = 1;
        let sound_count = self.snd.len();
        self.check_game_over();
        self.snd.truncate(sound_count);
        true
    }

    pub(crate) fn net_apply_reinforce(&mut self, player: usize, lands: Vec<usize>) -> bool {
        if self.over.is_some()
            || player >= self.players
            || player >= self.reserve.len()
            || player != self.current
        {
            return false;
        }
        // mirror the host's stock arithmetic so displays stay in sync
        let n = self.largest_region(player) + self.reserve[player];
        if lands.len() > n as usize {
            return false;
        }
        let mut additions: HashMap<usize, u32> = HashMap::new();
        for &t in &lands {
            let Some(terr) = self.terrs.get(t) else {
                return false;
            };
            let add = additions.entry(t).or_insert(0);
            *add += 1;
            if terr.owner != player || terr.dice.saturating_add(*add) > MAX_DICE {
                return false;
            }
        }
        let mut placed = 0u32;
        let mut placed_list: Vec<usize> = Vec::new();
        for t in lands {
            if t < self.terrs.len() && self.terrs[t].dice < MAX_DICE {
                self.terrs[t].dice += 1;
                self.fx[t].bounce = 1.0;
                placed += 1;
                placed_list.push(t);
            }
        }
        if self.recording {
            self.events.push(RepEvent::Reinforce {
                player,
                lands: placed_list,
            });
        }
        self.reserve[player] = n.saturating_sub(placed).min(64);
        self.push_log(format!(
            "{} reinforced with {} dice.",
            self.name_of(player),
            placed
        ));
        if player < self.humans && placed > 0 {
            self.snd.push(Snd::Reinforce);
        }
        self.advance_turn();
        true
    }

    pub(crate) fn update(&mut self, dt: f32) {
        self.time += dt;
        for fx in &mut self.fx {
            fx.shake = (fx.shake - dt / T_SHAKE).max(0.0);
            fx.bounce = (fx.bounce - dt / 0.55).max(0.0);
            fx.flash = (fx.flash - dt / 0.6).max(0.0);
        }
        for f in &mut self.floats {
            f.t += dt;
        }
        self.floats.retain(|f| f.t < f.life);
        self.banner_t = (self.banner_t - dt).max(0.0);
        self.hint_t = (self.hint_t - dt).max(0.0);
        if self.over.is_some() {
            self.over_t = (self.over_t + dt * 2.0).min(1.0);
        }

        // battle animation; sounds follow the picture: the dice land when the
        // rolls appear, and the result plays as soon as the totals settle
        let mut cues = Vec::new();
        if let Some(b) = &mut self.battle {
            let before = b.t;
            b.t += dt;
            let land = T_SHAKE * 0.6;
            let settle = battle_settle_time(b);
            if before < land && b.t >= land {
                cues.push(0);
            }
            if before < settle && b.t >= settle {
                cues.push(1);
            }
        }
        if let Some(b) = self.battle.clone() {
            for cue in cues {
                let s = if cue == 0 {
                    Snd::Land {
                        quiet: !self.involves_me(b.a, b.d),
                    }
                } else {
                    self.result_sound(&b)
                };
                self.snd.push(s);
            }
        }
        if self.battle.as_ref().is_some_and(|b| b.t >= T_BATTLE) {
            let b = self.battle.take().unwrap();
            self.apply_battle(b);
        }

        // Only live games schedule AI. Replays are driven solely by recorded events.
        if self.recording
            && self.over.is_none()
            && self.battle.is_none()
            && !self.is_human(self.current)
            && !self.net_guest
        {
            self.ai_timer += dt;
            if self.ai_timer >= AI_STEP {
                self.ai_timer = 0.0;
                if let Some((a, d)) = self.best_ai_attack() {
                    self.begin_attack(a, d);
                } else {
                    self.end_turn();
                }
            }
        }
    }
}

pub(crate) fn diff_idx(d: Difficulty) -> usize {
    match d {
        Difficulty::Easy => 0,
        Difficulty::Normal => 1,
        Difficulty::Hard => 2,
    }
}

pub(crate) fn idx_diff(i: usize) -> Difficulty {
    match i {
        0 => Difficulty::Easy,
        2 => Difficulty::Hard,
        _ => Difficulty::Normal,
    }
}

#[cfg(test)]
mod tests {
    use dicegame::engine;

    use crate::testutil::{mk_game, GLOBALS, TEAMS_FF};

    use super::*;

    #[test]
    fn teams_assign_choice_and_fill() {
        // the human picks team B (1); bots keep the sides balanced
        let cfg = TeamCfg {
            my_team: 1,
            ..TEAMS_FF
        };
        assert_eq!(assign_teams(5, 1, cfg), vec![1, 0, 0, 1, 0]);
        // three teams, human on C (2): bots fill A and B first
        let cfg = TeamCfg {
            count: 3,
            my_team: 2,
            ..TEAMS_FF
        };
        assert_eq!(assign_teams(6, 1, cfg), vec![2, 0, 1, 0, 1, 2]);
        // humans vs bots: both humans together, every bot on the other side
        let cfg = TeamCfg {
            hvb: true,
            ..TEAMS_FF
        };
        assert_eq!(assign_teams(5, 2, cfg), vec![0, 0, 1, 1, 1]);
        // all-human lobby cannot be humans-vs-bots; falls back to the cycle
        assert_eq!(assign_teams(4, 4, cfg), vec![0, 1, 0, 1]);
        // explicit picks win: two humans both on team A, bots balance
        let picks = [Some(0), Some(0)];
        assert_eq!(fill_teams(4, 2, TEAMS_FF, &picks), vec![0, 0, 1, 1]);
    }

    #[test]
    fn hard_reinforcements_follow_the_same_rules_as_humans() {
        let _g = GLOBALS.lock().unwrap();
        let game = |humans| {
            let mut g = mk_game(
                &[1, 1, 1, 0],
                &[(0, 1), (1, 2), (2, 3)],
                2,
                TeamCfg::default(),
            );
            g.current = 1;
            g.humans = humans;
            g.difficulty = Difficulty::Hard;
            g.reserve[1] = 2;
            for (t, dice) in g.terrs.iter_mut().zip([1, 3, 7, 8]) {
                t.dice = dice;
            }
            g
        };
        for seed in 0..32 {
            let mut human = game(2);
            let mut bot = game(1);
            engine::random::srand(seed);
            human.end_turn();
            engine::random::srand(seed);
            bot.end_turn();
            assert_eq!(
                human.terrs.iter().map(|t| t.dice).collect::<Vec<_>>(),
                bot.terrs.iter().map(|t| t.dice).collect::<Vec<_>>(),
                "reinforcement placement depends on who controls the seat (seed {seed})"
            );
            assert_eq!(human.reserve, bot.reserve);
        }
    }

    #[test]
    fn bridge_connects_islands_through_allies() {
        // p0's two islands separated by teammate p2's land in the middle
        let mut g = mk_game(&[0, 2, 0], &[(0, 1), (1, 2)], 4, TEAMS_FF);
        assert_eq!(g.largest_region(0), 1);
        g.teams.bridge = true;
        assert_eq!(g.largest_region(0), 2);
        // an enemy in the middle never bridges
        g.terrs[1].owner = 1;
        assert_eq!(g.largest_region(0), 1);
    }

    #[test]
    fn friendly_fire_toggle_gates_targets() {
        let mut g = mk_game(&[0, 2, 1], &[(0, 1), (1, 2)], 4, TEAMS_FF);
        assert!(g.can_target(0, 2)); // teammate, friendly fire on
        assert!(g.can_target(0, 1)); // enemy
        assert!(!g.can_target(0, 0)); // own land
        g.teams.ff = false;
        assert!(!g.can_target(0, 2)); // teammate protected
        assert!(g.can_target(0, 1));
    }

    #[test]
    fn last_team_standing_wins() {
        // team A (p0 + p2) vs team B (p1, one land left)
        let mut g = mk_game(&[0, 2, 1], &[(0, 1), (1, 2)], 3, TEAMS_FF);
        g.apply_battle(Battle {
            a: 1,
            d: 2,
            ra: 9,
            rd: 3,
            ad: Vec::new(),
            dd: Vec::new(),
            captured: true,
            t: 0.0,
        });
        assert!(g.over.as_deref().unwrap_or("").contains("victorious"));
    }

    #[test]
    fn ffa_conquest_still_ends_game() {
        let mut g = mk_game(&[0, 0, 1], &[(0, 1), (1, 2)], 2, TeamCfg::default());
        g.apply_battle(Battle {
            a: 1,
            d: 2,
            ra: 9,
            rd: 3,
            ad: Vec::new(),
            dd: Vec::new(),
            captured: true,
            t: 0.0,
        });
        assert!(g.over.is_some());
    }
}
