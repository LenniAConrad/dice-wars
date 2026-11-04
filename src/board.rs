//! Hex grid geometry and map generation.
//!
//! Pointy-top hexes, odd-r offset layout. Directions: 0=E 1=SE 2=SW 3=W 4=NW 5=NE.

use std::collections::HashSet;

use dicegame::engine::random::gen_range;
use dicegame::engine::{self, prelude::*};

use crate::game::{
    assign_teams, Difficulty, Game, Persona, TeamCfg, TerrFx, Territory, HUMAN, MAX_DICE, SEED_MOD,
    TURN_SECS,
};
use crate::theme::{color_map, COLOR_NAMES};

pub(crate) const COLS: usize = 30;

pub(crate) const ROWS: usize = 22;

pub(crate) const HEX: f32 = 16.0;

const OFF_X: f32 = 48.0;

const OFF_Y: f32 = 96.0;

pub(crate) const SQRT3: f32 = 1.732_050_8;

pub(crate) const N_SEEDS: usize = 60; // regions grown on the grid

const N_HOLES: usize = 9; // regions removed for lakes and ragged coasts

// everyone plays the dealt position matching their color where possible;
// unpicked or out-of-range colors keep their own seat's position
pub(crate) fn apply_position_swap(g: &mut Game, colors: &[usize]) {
    let n = g.players;
    let mut pos = vec![usize::MAX; n];
    let mut used = vec![false; n];
    for (p, slot) in pos.iter_mut().enumerate() {
        if let Some(&c) = colors.get(p) {
            if c < n && !used[c] {
                *slot = c;
                used[c] = true;
            }
        }
    }
    for p in 0..n {
        if pos[p] == usize::MAX && !used[p] {
            pos[p] = p;
            used[p] = true;
        }
    }
    for slot in pos.iter_mut() {
        if *slot == usize::MAX {
            let q = (0..n).find(|&q| !used[q]).unwrap();
            *slot = q;
            used[q] = true;
        }
    }
    let mut inv = vec![0usize; n];
    for (p, &q) in pos.iter().enumerate() {
        inv[q] = p;
    }
    for t in g.terrs.iter_mut() {
        t.owner = inv[t.owner];
    }
}

pub(crate) fn cell_pos(i: usize) -> Vec2 {
    let c = (i % COLS) as f32;
    let r = i / COLS;
    let shift = if r % 2 == 1 { 0.5 } else { 0.0 };
    vec2(
        OFF_X + SQRT3 * HEX * (c + shift),
        OFF_Y + 1.5 * HEX * r as f32,
    )
}

pub(crate) fn neighbor_of(c: usize, r: usize, d: usize) -> Option<usize> {
    let odd = (r % 2) as i32;
    let (dc, dr): (i32, i32) = match d {
        0 => (1, 0),
        1 => (odd, 1),
        2 => (odd - 1, 1),
        3 => (-1, 0),
        4 => (odd - 1, -1),
        _ => (odd, -1),
    };
    let nc = c as i32 + dc;
    let nr = r as i32 + dr;
    if nc < 0 || nr < 0 || nc >= COLS as i32 || nr >= ROWS as i32 {
        None
    } else {
        Some(nr as usize * COLS + nc as usize)
    }
}

fn cell_neighbors(i: usize) -> Vec<usize> {
    let (c, r) = (i % COLS, i / COLS);
    (0..6).filter_map(|d| neighbor_of(c, r, d)).collect()
}

pub(crate) fn shuffle<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        let j = gen_range(0, i + 1);
        v.swap(i, j);
    }
}

pub(crate) fn random_seed() -> u64 {
    let t = (engine::now() * 1000.0) as u64;
    (t ^ ((engine::random::rand() as u64) << 16)) % SEED_MOD
}

// seed the global RNG, then generate: the same seed always yields the same map
pub(crate) fn gen_map(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
) -> Game {
    engine::random::srand(seed.wrapping_add(0x9E37_79B9));
    let mut g = Game::new(players, humans.clamp(1, players), difficulty, teams);
    // The seed only decides the map; play continues from fresh entropy.
    engine::random::srand_from_entropy();
    // solo: picking a color means playing that dealt position — the map keeps
    // its look and you take over the territories (and dice) of that color
    g.seed = seed;
    g
}

// gen_map plus the position swap for THIS machine's current colors
pub(crate) fn gen_map_local(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
) -> Game {
    let mut g = gen_map(seed, players, humans, difficulty, teams);
    let colors: Vec<usize> = (0..g.players).map(color_map).collect();
    apply_position_swap(&mut g, &colors);
    g
}

impl Game {
    pub(crate) fn new(
        players: usize,
        humans: usize,
        difficulty: Difficulty,
        teams: TeamCfg,
    ) -> Self {
        'gen: loop {
            let total = COLS * ROWS;

            // Grow N_SEEDS organic regions with a randomized multi-source flood fill.
            let mut cell_terr = vec![-1i32; total];
            let mut picked = HashSet::new();
            let mut frontier: Vec<usize> = Vec::new();
            let mut next_id = 0i32;
            while picked.len() < N_SEEDS {
                let c = gen_range(0, total);
                if picked.insert(c) {
                    cell_terr[c] = next_id;
                    next_id += 1;
                    frontier.push(c);
                }
            }
            while !frontier.is_empty() {
                let k = gen_range(0, frontier.len());
                let cell = frontier[k];
                let free: Vec<usize> = cell_neighbors(cell)
                    .into_iter()
                    .filter(|&n| cell_terr[n] == -1)
                    .collect();
                if free.is_empty() {
                    frontier.swap_remove(k);
                    continue;
                }
                let n = free[gen_range(0, free.len())];
                cell_terr[n] = cell_terr[cell];
                frontier.push(n);
            }

            // Delete a few regions so the map gets an irregular outline and
            // holes — removing extras until every player gets the same count
            let mut holes_n = N_HOLES;
            while (N_SEEDS - holes_n) % players != 0 {
                holes_n += 1;
            }
            let mut region_sizes = [0usize; N_SEEDS];
            for &t in &cell_terr {
                region_sizes[t as usize] += 1;
            }
            if region_sizes.iter().filter(|&&size| size < 3).count() > holes_n {
                continue 'gen;
            }
            let mut ids: Vec<i32> = (0..N_SEEDS as i32).collect();
            shuffle(&mut ids);
            // Tiny regions cannot hold a readable dice tower. Always spend
            // the available holes on those first instead of repeatedly
            // rebuilding the entire map until a random shuffle happens to
            // remove every one of them.
            ids.sort_by_key(|&id| region_sizes[id as usize] >= 3);
            let holes: HashSet<i32> = ids[..holes_n].iter().copied().collect();
            let mut remap = vec![-1i32; N_SEEDS];
            for (new_id, &old) in ids[holes_n..].iter().enumerate() {
                remap[old as usize] = new_id as i32;
            }
            for t in cell_terr.iter_mut() {
                *t = if *t >= 0 && !holes.contains(t) {
                    remap[*t as usize]
                } else {
                    -1
                };
            }
            let n_terr = N_SEEDS - holes_n;

            let mut cells: Vec<Vec<usize>> = vec![Vec::new(); n_terr];
            for (i, &t) in cell_terr.iter().enumerate() {
                if t >= 0 {
                    cells[t as usize].push(i);
                }
            }
            if cells.iter().any(|c| c.len() < 3) {
                continue 'gen;
            }

            // Territory adjacency from cell adjacency.
            let mut neigh: Vec<HashSet<usize>> = vec![HashSet::new(); n_terr];
            for (i, &t) in cell_terr.iter().enumerate() {
                if t < 0 {
                    continue;
                }
                for n in cell_neighbors(i) {
                    let u = cell_terr[n];
                    if u >= 0 && u != t {
                        neigh[t as usize].insert(u as usize);
                    }
                }
            }

            // The whole map must be connected, otherwise it can be unwinnable.
            let mut seen = vec![false; n_terr];
            let mut stack = vec![0usize];
            seen[0] = true;
            let mut reached = 1;
            while let Some(t) = stack.pop() {
                for &n in &neigh[t] {
                    if !seen[n] {
                        seen[n] = true;
                        reached += 1;
                        stack.push(n);
                    }
                }
            }
            if reached != n_terr {
                continue 'gen;
            }

            let mut terrs: Vec<Territory> = (0..n_terr)
                .map(|t| {
                    let centroid = cells[t]
                        .iter()
                        .fold(Vec2::ZERO, |acc, &c| acc + cell_pos(c))
                        / cells[t].len() as f32;
                    // Dice towers rise above their anchor, so aim a little
                    // below the centroid: the tower then sits visually
                    // centered on its land and rarely pokes past the top coast.
                    let target = centroid + vec2(0.0, HEX * 0.8);
                    let anchor_cell = *cells[t]
                        .iter()
                        .min_by(|&&a, &&b| {
                            cell_pos(a)
                                .distance(target)
                                .total_cmp(&cell_pos(b).distance(target))
                        })
                        .unwrap();
                    Territory {
                        owner: 0,
                        dice: 1,
                        // sorted: hash-set order differs per process, and
                        // bots break ties by neighbor order
                        neighbors: {
                            let mut v: Vec<usize> = neigh[t].iter().copied().collect();
                            v.sort_unstable();
                            v
                        },
                        anchor: cell_pos(anchor_cell),
                        seed: gen_range(0u32, 1_000_000u32),
                    }
                })
                .collect();

            // Deal territories so every player's biggest starting cluster is
            // comparable — the deal lottery used to decide games at generation
            let mut order: Vec<usize> = (0..n_terr).collect();
            let mut best_assign: Vec<usize> = Vec::new();
            let mut best_spread = usize::MAX;
            for _ in 0..300 {
                shuffle(&mut order);
                let mut owner_of = vec![0usize; n_terr];
                for (i, &t) in order.iter().enumerate() {
                    owner_of[t] = i % players;
                }
                let (mut mn, mut mx) = (usize::MAX, 0usize);
                for p in 0..players {
                    let mut seen = vec![false; n_terr];
                    let mut best = 0usize;
                    for s in 0..n_terr {
                        if owner_of[s] != p || seen[s] {
                            continue;
                        }
                        let mut stack = vec![s];
                        seen[s] = true;
                        let mut size = 0usize;
                        while let Some(t) = stack.pop() {
                            size += 1;
                            for &nb in &neigh[t] {
                                if !seen[nb] && owner_of[nb] == p {
                                    seen[nb] = true;
                                    stack.push(nb);
                                }
                            }
                        }
                        best = best.max(size);
                    }
                    mn = mn.min(best);
                    mx = mx.max(best);
                }
                let spread = mx - mn;
                if spread < best_spread {
                    best_spread = spread;
                    best_assign = owner_of.clone();
                    if spread <= 1 {
                        break;
                    }
                }
            }
            for (t, owner) in best_assign.iter().enumerate() {
                terrs[t].owner = *owner;
            }
            for p in 0..players {
                let own: Vec<usize> = (0..n_terr).filter(|&t| terrs[t].owner == p).collect();
                for _ in 0..own.len() as u32 * 2 {
                    let cand: Vec<usize> = own
                        .iter()
                        .copied()
                        .filter(|&t| terrs[t].dice < MAX_DICE)
                        .collect();
                    if cand.is_empty() {
                        break;
                    }
                    terrs[cand[gen_range(0, cand.len())]].dice += 1;
                }
            }

            // seeded like the map, so a bookmarked seed replays the same cast
            let personas: Vec<Persona> = (0..players)
                .map(|_| match gen_range(0, 5) {
                    0 => Persona::Aggressive,
                    1 => Persona::Defensive,
                    2 => Persona::Greedy,
                    3 => Persona::Balancer,
                    _ => Persona::Chaotic,
                })
                .collect();

            // rotate who moves first: it's decided by the seed, not the host
            let first = gen_range(0, players);

            return Game {
                players,
                humans,
                teams,
                team: assign_teams(players, humans, teams),
                names: Vec::new(),
                absent: vec![false; players],
                turn_timer: TURN_SECS,
                difficulty,
                personas,
                seed: 0,
                reserve: vec![0; players],
                recording: true,
                net_guest: false,
                my_seat: 0,
                events: Vec::new(),
                start: Vec::new(),
                cell_terr,
                fx: vec![TerrFx::default(); terrs.len()],
                terrs,
                current: first,
                selected: None,
                keyboard_hover: None,
                log: vec!["A new war begins!".to_string()],
                hint: String::new(),
                hint_t: 0.0,
                ai_timer: 0.0,
                over: None,
                over_t: 0.0,
                battle: None,
                floats: Vec::new(),
                spectating: false,
                banner: if first == HUMAN && humans == 1 {
                    "Your turn".to_string()
                } else {
                    format!("{}'s turn", COLOR_NAMES[color_map(first)])
                },
                banner_t: if first < humans { 1.6 } else { 0.0 },
                time: 0.0,
                snd: Vec::new(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::game::MAX_PLAYERS;
    use crate::testutil::GLOBALS;
    use crate::theme::apply_local_colors;

    use super::*;

    #[test]
    fn color_pick_swaps_position_not_the_map() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        apply_local_colors(0);
        let base = gen_map_local(777, 6, 1, Difficulty::Normal, TeamCfg::default());
        apply_local_colors(4); // pick a middle slot (sky)
        let g = gen_map_local(777, 6, 1, Difficulty::Normal, TeamCfg::default());
        for i in 0..base.terrs.len() {
            // every territory keeps its on-screen color...
            assert_eq!(color_map(g.terrs[i].owner), base.terrs[i].owner);
            // ...and you own exactly what the sky player had
            assert_eq!(g.terrs[i].owner == 0, base.terrs[i].owner == 4);
        }
        apply_local_colors(0);
    }

    #[test]
    fn generated_maps_keep_core_invariants() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        apply_local_colors(0);
        for players in 2..=MAX_PLAYERS {
            for seed in [0, 1, 777, SEED_MOD - 1] {
                let g = gen_map(seed, players, 1, Difficulty::Normal, TeamCfg::default());
                let expected = g.terrs.len() / players;
                assert!(!g.terrs.is_empty());
                assert_eq!(expected * players, g.terrs.len());
                for player in 0..players {
                    assert_eq!(g.count(player), expected);
                }
                let mut cells_per_territory = vec![0usize; g.terrs.len()];
                for &territory in &g.cell_terr {
                    if territory >= 0 {
                        cells_per_territory[territory as usize] += 1;
                    }
                }
                assert!(cells_per_territory.iter().all(|&count| count >= 3));

                let mut seen = vec![false; g.terrs.len()];
                let mut stack = vec![0usize];
                seen[0] = true;
                while let Some(t) = stack.pop() {
                    for &neighbor in &g.terrs[t].neighbors {
                        if !seen[neighbor] {
                            seen[neighbor] = true;
                            stack.push(neighbor);
                        }
                    }
                }
                assert!(seen.into_iter().all(|reached| reached));
            }
        }
    }

    // FNV-1a over everything a seed decides: cell layout, owners, dice,
    // adjacency, die-face seeds, anchors, personas and the first player
    pub(crate) fn map_hash(g: &Game) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: u64| {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for &c in &g.cell_terr {
            eat(c as u64);
        }
        for t in &g.terrs {
            eat(t.owner as u64);
            eat(t.dice as u64);
            eat(t.seed as u64);
            eat(t.anchor.x.to_bits() as u64);
            eat(t.anchor.y.to_bits() as u64);
            for &n in &t.neighbors {
                eat(n as u64);
            }
            eat(u64::MAX);
        }
        for &p in &g.personas {
            eat(p as u64);
        }
        eat(g.current as u64);
        h
    }

    // Seeds are shared, bookmarked and sent to network peers: the default
    // map for a seed must never change. These hashes were taken before any
    // map option existed; if this fails, generation drifted.
    #[test]
    fn default_maps_are_pinned() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        apply_local_colors(0);
        for (seed, players, n_terr, hash) in [
            (0u64, 2usize, 50usize, 0x1b3b_b95b_80e5_bfffu64),
            (1, 4, 48, 0x9eab_a732_2dca_cd31),
            (777, 6, 48, 0x1c25_ae6c_4c97_13b7),
            (31337, 8, 48, 0x7d46_90c2_d838_8126),
            (SEED_MOD - 1, 9, 45, 0x2116_cedc_5f15_6364),
            (12345678, 3, 51, 0xee10_a713_3654_6ff4),
            (42, 5, 50, 0x697e_d9ae_4f56_4d9e),
            (99, 7, 49, 0x3f94_6a5d_49c5_7053),
        ] {
            for humans in [1, 2] {
                let g = gen_map(seed, players, humans, Difficulty::Hard, TeamCfg::default());
                assert_eq!(g.terrs.len(), n_terr, "seed {seed}, {players} players");
                assert_eq!(map_hash(&g), hash, "seed {seed}, {players} players");
            }
        }
    }
}
