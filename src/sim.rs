//! Headless bot-only simulator for balance work: no window, no rendering,
//! no animation timers. Rules and bots are driven directly.
//!
//! ```sh
//! DW_SIM=2000 dicegame                       # 2000 games, 6 Normal bots
//! DW_SIM=2000 DW_SIM_PLAYERS=2,4,8 dicegame  # one batch per player count
//! DW_SIM=1000 DW_SIM_DIFF=hard,normal dicegame  # per seat; the last entry fills the rest
//! ```
//!
//! Optional: `DW_SIM_SEED` (first map seed, default 1), `DW_SIM_MAP` (map
//! option code such as `m-c` or `s-i`). Game `i` uses map seed `SEED + i`
//! and a dice stream derived from it, so every run is reproducible.

use dicegame::engine;

use crate::board::{gen_map_seeded_with, MapOpts};
use crate::game::{diff_idx, idx_diff, Difficulty, Game, TeamCfg, SEED_MOD};

/// Hard limit per game; a game that runs longer counts as a timeout.
const MAX_TURNS: u32 = 4000;

pub(crate) struct Outcome {
    pub(crate) winner: Option<usize>, // None = timeout
    pub(crate) first: usize,          // the seat that moved first
    pub(crate) turns: u32,
    pub(crate) battles: u32,
}

fn diff_name(d: Difficulty) -> &'static str {
    ["Easy", "Normal", "Hard"][diff_idx(d)]
}

fn parse_diff(s: &str) -> Option<Difficulty> {
    match s.trim().to_ascii_lowercase().as_str() {
        "easy" | "e" => Some(Difficulty::Easy),
        "normal" | "n" => Some(Difficulty::Normal),
        "hard" | "h" => Some(Difficulty::Hard),
        _ => None,
    }
}

/// One bot-only game on map `seed`; `diffs[p]` is seat p's difficulty.
pub(crate) fn play(seed: u64, players: usize, diffs: &[Difficulty], opts: MapOpts) -> Outcome {
    let mut g: Game = gen_map_seeded_with(
        seed % SEED_MOD,
        players,
        1,
        diffs[0],
        TeamCfg::default(),
        opts,
    );
    // dice and bot choices: a fixed stream per map seed
    engine::random::srand(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xD1CE);
    debug_assert_eq!(g.map, opts);
    g.humans = 0; // every seat is a bot
    g.recording = false; // no event log; the simulator decides the winner
    let first = g.current;
    let mut turns = 0u32;
    let mut battles = 0u32;
    let n = g.terrs.len();
    loop {
        let p = g.current;
        if g.count(p) == n {
            return Outcome {
                winner: Some(p),
                first,
                turns,
                battles,
            };
        }
        if turns >= MAX_TURNS {
            return Outcome {
                winner: None,
                first,
                turns,
                battles,
            };
        }
        g.difficulty = diffs[p];
        if let Some((a, d)) = g.best_ai_attack() {
            g.begin_attack(a, d);
            let b = g.battle.take().unwrap();
            g.apply_battle(b);
            battles += 1;
        } else {
            g.end_turn();
            turns += 1;
            g.floats.clear();
            g.snd.clear();
        }
        g.snd.clear();
    }
}

pub(crate) struct Tally {
    games: usize,
    timeouts: usize,
    turns: u64,
    battles: u64,
    by_seat: Vec<usize>,
    by_order: Vec<usize>,         // 0 = moved first
    by_diff: [(usize, usize); 3], // (wins, seat-games)
}

pub(crate) fn batch(
    games: usize,
    seed0: u64,
    players: usize,
    diffs: &[Difficulty],
    opts: MapOpts,
) -> Tally {
    let mut t = Tally {
        games,
        timeouts: 0,
        turns: 0,
        battles: 0,
        by_seat: vec![0; players],
        by_order: vec![0; players],
        by_diff: [(0, 0); 3],
    };
    for i in 0..games {
        let o = play(seed0 + i as u64, players, diffs, opts);
        t.turns += o.turns as u64;
        t.battles += o.battles as u64;
        for &d in diffs {
            t.by_diff[diff_idx(d)].1 += 1;
        }
        match o.winner {
            Some(w) => {
                t.by_seat[w] += 1;
                t.by_order[(w + players - o.first) % players] += 1;
                t.by_diff[diff_idx(diffs[w])].0 += 1;
            }
            None => t.timeouts += 1,
        }
    }
    t
}

fn pct(wins: usize, n: usize) -> String {
    if n == 0 {
        return "-".to_string();
    }
    let p = wins as f64 / n as f64;
    let se = (p * (1.0 - p) / n as f64).sqrt();
    format!("{:5.1}% ±{:.1}", p * 100.0, se * 100.0)
}

fn report(players: usize, diffs: &[Difficulty], opts: MapOpts, seed0: u64, t: &Tally) {
    let names: Vec<&str> = diffs.iter().map(|&d| diff_name(d)).collect();
    println!(
        "== {} games, {} players [{}], map {}, seeds {}..{}",
        t.games,
        players,
        names.join(","),
        opts.code(),
        seed0,
        seed0 + t.games as u64 - 1
    );
    println!(
        "uniform share {:.1}%  (± is one standard error)",
        100.0 / players as f64
    );
    println!("win rate by turn order (1 = moves first):");
    for (k, &w) in t.by_order.iter().enumerate() {
        println!("  {:>2}: {}", k + 1, pct(w, t.games));
    }
    println!("win rate by seat:");
    for (s, &w) in t.by_seat.iter().enumerate() {
        println!("  seat {} {:<6}: {}", s, names[s], pct(w, t.games));
    }
    println!("win rate per seat by difficulty:");
    for (i, &(w, n)) in t.by_diff.iter().enumerate() {
        if n > 0 {
            println!(
                "  {:<6}: {} ({} seat-games)",
                diff_name(idx_diff(i)),
                pct(w, n),
                n
            );
        }
    }
    let done = t.games.max(1) as f64;
    println!(
        "avg turns {:.1} (rounds {:.1}), avg battles {:.1}, timeouts {}, draws 0",
        t.turns as f64 / done,
        t.turns as f64 / done / players as f64,
        t.battles as f64 / done,
        t.timeouts
    );
}

pub(crate) fn run(games: &str) {
    let games: usize = games.trim().parse().unwrap_or(1000).max(1);
    let seed0: u64 = std::env::var("DW_SIM_SEED")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1);
    let opts = std::env::var("DW_SIM_MAP")
        .ok()
        .and_then(|v| MapOpts::from_code(&v))
        .unwrap_or_default();
    let player_list: Vec<usize> = std::env::var("DW_SIM_PLAYERS")
        .unwrap_or_else(|_| "6".to_string())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .map(|p: usize| p.clamp(2, crate::game::MAX_PLAYERS))
        .collect();
    let diff_list: Vec<Difficulty> = std::env::var("DW_SIM_DIFF")
        .unwrap_or_else(|_| "normal".to_string())
        .split(',')
        .filter_map(parse_diff)
        .collect();
    for &players in &player_list {
        let diffs: Vec<Difficulty> = (0..players)
            .map(|p| {
                diff_list
                    .get(p)
                    .or(diff_list.last())
                    .copied()
                    .unwrap_or(Difficulty::Normal)
            })
            .collect();
        let start = std::time::Instant::now();
        let t = batch(games, seed0, players, &diffs, opts);
        report(players, &diffs, opts, seed0, &t);
        println!("({:.1}s)\n", start.elapsed().as_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::GLOBALS;

    use super::*;

    #[test]
    fn simulation_is_reproducible_and_finishes() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let diffs = [Difficulty::Hard, Difficulty::Normal, Difficulty::Easy];
        for seed in 1..6 {
            let a = play(seed, 3, &diffs, MapOpts::default());
            let b = play(seed, 3, &diffs, MapOpts::default());
            assert!(a.winner.is_some(), "seed {seed} timed out");
            assert_eq!(
                (a.winner, a.first, a.turns, a.battles),
                (b.winner, b.first, b.turns, b.battles)
            );
        }
    }
}
