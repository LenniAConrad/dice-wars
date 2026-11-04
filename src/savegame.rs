//! Saving an unfinished solo game and picking it up later.
//!
//! The map comes back from its seed; the file holds everything play changed:
//! owners, dice, stored dice, whose turn it is, the teams, and the event list
//! (so replays and the summary still cover the whole game).

use crate::board::{gen_map_local_with, MapOpts};
use crate::game::{diff_idx, idx_diff, Game, RepEvent, TeamCfg, MAX_TEAMS};
use crate::settings::data_file;
use crate::theme::apply_local_colors;

const SAVE_FILE: &str = "savegame.txt";
const HEADER: &str = "dicewars-save 1";

/// What the menu shows about a saved game.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SaveInfo {
    pub(crate) players: usize,
    pub(crate) difficulty: usize,
    pub(crate) round: u32,
}

/// Whether `g` is a game this machine can save: solo, live and unfinished.
pub(crate) fn saveable(g: &Game) -> bool {
    g.humans == 1 && g.recording && !g.net_guest && g.over.is_none()
}

pub(crate) fn encode(g: &Game, color: usize) -> String {
    let join = |v: &mut dyn Iterator<Item = String>| v.collect::<Vec<_>>().join(" ");
    let t = g.teams;
    let mut s = format!(
        "{HEADER}\nseed {}\nmap {}\nplayers {}\ndifficulty {}\nteams {} {} {} {} {} {}\ncolor {}\n",
        g.seed,
        g.map.code(),
        g.players,
        diff_idx(g.difficulty),
        t.on as u8,
        t.count,
        t.hvb as u8,
        t.my_team,
        t.bridge as u8,
        t.ff as u8,
        color
    );
    s += &format!("team {}\n", join(&mut g.team.iter().map(|v| v.to_string())));
    s += &format!("current {}\n", g.current);
    s += &format!(
        "reserve {}\n",
        join(&mut g.reserve.iter().map(|v| v.to_string()))
    );
    s += &format!(
        "owners {}\n",
        join(&mut g.terrs.iter().map(|t| t.owner.to_string()))
    );
    s += &format!(
        "dice {}\n",
        join(&mut g.terrs.iter().map(|t| t.dice.to_string()))
    );
    s += &format!(
        "start {}\n",
        join(&mut g.start.iter().map(|(o, d)| format!("{o}:{d}")))
    );
    for e in &g.events {
        match e {
            RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured,
            } => s += &format!("A {a} {d} {ra} {rd} {}\n", *captured as u8),
            RepEvent::Reinforce { player, lands } => {
                s += &format!(
                    "R {player} {}\n",
                    lands
                        .iter()
                        .map(|l| l.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }
    s
}

struct Parsed {
    seed: u64,
    map: MapOpts,
    players: usize,
    difficulty: usize,
    teams: TeamCfg,
    color: usize,
    team: Vec<usize>,
    current: usize,
    reserve: Vec<u32>,
    owners: Vec<usize>,
    dice: Vec<u32>,
    start: Vec<(usize, u32)>,
    events: Vec<RepEvent>,
}

fn parse(text: &str) -> Option<Parsed> {
    let mut lines = text.lines();
    if lines.next()? != HEADER {
        return None;
    }
    let nums = |rest: &str| -> Option<Vec<u64>> {
        rest.split_whitespace().map(|v| v.parse().ok()).collect()
    };
    let mut p = Parsed {
        seed: 0,
        map: MapOpts::default(),
        players: 0,
        difficulty: 1,
        teams: TeamCfg::default(),
        color: 0,
        team: Vec::new(),
        current: 0,
        reserve: Vec::new(),
        owners: Vec::new(),
        dice: Vec::new(),
        start: Vec::new(),
        events: Vec::new(),
    };
    for line in lines {
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "seed" => p.seed = rest.trim().parse().ok()?,
            "map" => p.map = MapOpts::from_code(rest)?,
            "players" => p.players = rest.trim().parse().ok()?,
            "difficulty" => p.difficulty = rest.trim().parse::<usize>().ok()?.min(2),
            "color" => p.color = rest.trim().parse().ok()?,
            "teams" => {
                let v = nums(rest)?;
                if v.len() != 6 {
                    return None;
                }
                p.teams = TeamCfg {
                    on: v[0] != 0,
                    count: (v[1] as usize).clamp(2, MAX_TEAMS),
                    hvb: v[2] != 0,
                    my_team: v[3] as usize % MAX_TEAMS,
                    bridge: v[4] != 0,
                    ff: v[5] != 0,
                };
            }
            "team" => p.team = nums(rest)?.into_iter().map(|v| v as usize).collect(),
            "current" => p.current = rest.trim().parse().ok()?,
            "reserve" => p.reserve = nums(rest)?.into_iter().map(|v| v as u32).collect(),
            "owners" => p.owners = nums(rest)?.into_iter().map(|v| v as usize).collect(),
            "dice" => p.dice = nums(rest)?.into_iter().map(|v| v as u32).collect(),
            "start" => {
                p.start = rest
                    .split_whitespace()
                    .map(|pair| {
                        let (o, d) = pair.split_once(':')?;
                        Some((o.parse().ok()?, d.parse().ok()?))
                    })
                    .collect::<Option<Vec<_>>>()?
            }
            "A" => {
                let v = nums(rest)?;
                if v.len() != 5 {
                    return None;
                }
                p.events.push(RepEvent::Attack {
                    a: v[0] as usize,
                    d: v[1] as usize,
                    ra: v[2] as u32,
                    rd: v[3] as u32,
                    captured: v[4] != 0,
                });
            }
            "R" => {
                let (player, lands) = rest.split_once(' ').unwrap_or((rest, ""));
                p.events.push(RepEvent::Reinforce {
                    player: player.trim().parse().ok()?,
                    lands: lands
                        .split(',')
                        .filter(|l| !l.is_empty())
                        .map(|l| l.parse().ok())
                        .collect::<Option<Vec<_>>>()?,
                });
            }
            _ => {}
        }
    }
    Some(p)
}

/// Rebuilds the saved game; None if the file is missing, damaged or from a
/// map this version generates differently.
pub(crate) fn decode(text: &str) -> Option<(Game, usize)> {
    let p = parse(text)?;
    if !(2..=crate::game::MAX_PLAYERS).contains(&p.players) {
        return None;
    }
    apply_local_colors(p.color);
    let mut g = gen_map_local_with(p.seed, p.players, 1, idx_diff(p.difficulty), p.teams, p.map);
    let n = g.terrs.len();
    let valid = p.owners.len() == n
        && p.dice.len() == n
        && p.reserve.len() == p.players
        && p.team.len() == p.players
        && p.current < p.players
        && p.owners.iter().all(|&o| o < p.players)
        && p.dice
            .iter()
            .all(|&d| (1..=crate::game::MAX_DICE).contains(&d))
        && (p.start.is_empty() || p.start.len() == n)
        && p.events.iter().all(|e| match e {
            RepEvent::Attack { a, d, .. } => *a < n && *d < n,
            RepEvent::Reinforce { player, lands } => {
                *player < p.players && lands.iter().all(|&l| l < n)
            }
        });
    if !valid {
        return None;
    }
    for (t, (&o, &d)) in g.terrs.iter_mut().zip(p.owners.iter().zip(&p.dice)) {
        t.owner = o;
        t.dice = d;
    }
    g.reserve = p.reserve;
    g.team = p.team;
    g.current = p.current;
    g.start = p.start;
    g.events = p.events;
    g.selected = None;
    g.ai_timer = -0.6;
    g.log.clear();
    g.push_log("Welcome back — the game continues.".to_string());
    if g.current == g.my_seat {
        g.banner = "Your turn".to_string();
        g.banner_t = 1.6;
    } else {
        g.banner_t = 0.0;
    }
    Some((g, p.color))
}

pub(crate) fn info_of(text: &str) -> Option<SaveInfo> {
    let p = parse(text)?;
    let turns = p
        .events
        .iter()
        .filter(|e| matches!(e, RepEvent::Reinforce { .. }))
        .count() as u32;
    Some(SaveInfo {
        players: p.players,
        difficulty: p.difficulty,
        round: turns / p.players.max(1) as u32 + 1,
    })
}

pub(crate) fn save(g: &Game, color: usize) {
    if saveable(g) {
        let _ = crate::settings::write_atomic(&data_file(SAVE_FILE), &encode(g, color));
    }
}

pub(crate) fn load() -> Option<(Game, usize)> {
    decode(&std::fs::read_to_string(data_file(SAVE_FILE)).ok()?)
}

pub(crate) fn info() -> Option<SaveInfo> {
    info_of(&std::fs::read_to_string(data_file(SAVE_FILE)).ok()?)
}

pub(crate) fn clear() {
    let _ = std::fs::remove_file(data_file(SAVE_FILE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::gen_map;
    use crate::game::Difficulty;
    use crate::testutil::GLOBALS;

    #[test]
    fn a_saved_game_comes_back_exactly() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let mut game = gen_map(4242, 5, 1, Difficulty::Hard, TeamCfg::default());
        for _ in 0..20_000 {
            if game.current == 0 && game.battle.is_none() && game.over.is_none() {
                game.end_turn();
            }
            game.update(1.0 / 30.0);
            if game.events.len() > 50 && game.battle.is_none() {
                break;
            }
        }
        let text = encode(&game, 0);
        let (back, color) = decode(&text).expect("decodes");
        assert_eq!(color, 0);
        assert_eq!(back.seed, game.seed);
        assert_eq!(back.current, game.current);
        assert_eq!(back.reserve, game.reserve);
        assert_eq!(back.events.len(), game.events.len());
        for (a, b) in back.terrs.iter().zip(&game.terrs) {
            assert_eq!((a.owner, a.dice), (b.owner, b.dice));
        }
        // the event list replays to the same board
        let s = crate::summary::Summary::of(&back);
        let last = s.series.last().unwrap();
        for (p, &lands) in last.iter().enumerate().take(back.players) {
            assert_eq!(lands as usize, back.count(p));
        }
        let info = info_of(&text).unwrap();
        assert_eq!((info.players, info.difficulty), (5, 2));
        crate::theme::apply_local_colors(0);
    }

    #[test]
    fn damaged_saves_are_refused() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let game = gen_map(7, 3, 1, Difficulty::Normal, TeamCfg::default());
        let text = encode(&game, 0);
        assert!(decode(&text.replace("owners ", "owners 9 ")).is_none());
        assert!(decode("something else").is_none());
        assert!(decode(&text.replace("dicewars-save 1", "dicewars-save 99")).is_none());
        crate::theme::apply_local_colors(0);
    }
}
