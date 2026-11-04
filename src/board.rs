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
#[cfg_attr(not(test), allow(dead_code))] // the default-map shorthand tests use
pub(crate) fn gen_map(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
) -> Game {
    gen_map_with(seed, players, humans, difficulty, teams, MapOpts::default())
}

// gen_map on a map of the given size and shape; MapOpts::default() is
// exactly gen_map
pub(crate) fn gen_map_with(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
    opts: MapOpts,
) -> Game {
    let g = gen_map_seeded_with(seed, players, humans, difficulty, teams, opts);
    // The seed only decides the map; play continues from fresh entropy.
    engine::random::srand_from_entropy();
    g
}

// gen_map_with without reseeding from entropy afterwards: the RNG is left
// where map generation stopped (the headless simulator seeds the dice itself)
pub(crate) fn gen_map_seeded_with(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
    opts: MapOpts,
) -> Game {
    engine::random::srand(seed.wrapping_add(0x9E37_79B9));
    let mut g = Game::new_with(players, humans.clamp(1, players), difficulty, teams, opts);
    // solo: picking a color means playing that dealt position — the map keeps
    // its look and you take over the territories (and dice) of that color
    g.seed = seed;
    g
}

// gen_map plus the position swap for THIS machine's current colors
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn gen_map_local(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
) -> Game {
    gen_map_local_with(seed, players, humans, difficulty, teams, MapOpts::default())
}

// gen_map_with plus the position swap for THIS machine's current colors
pub(crate) fn gen_map_local_with(
    seed: u64,
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    teams: TeamCfg,
    opts: MapOpts,
) -> Game {
    let mut g = gen_map_with(seed, players, humans, difficulty, teams, opts);
    let colors: Vec<usize> = (0..g.players).map(color_map).collect();
    apply_position_swap(&mut g, &colors);
    g
}

/// How much of the fixed grid is land, and how many territories it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MapSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// One landmass, or several islands joined by one-hex-wide land bridges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MapShape {
    #[default]
    Continent,
    Islands,
}

/// Map generation options. The default (Medium, Continent) is the classic
/// map: for a given seed it is bit-identical to maps from before options
/// existed. The grid (COLS x ROWS) and screen layout never change; only
/// which cells are land and how many territories grow on them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MapOpts {
    pub(crate) size: MapSize,
    pub(crate) shape: MapShape,
}

impl MapOpts {
    pub(crate) const ALL: [MapOpts; 6] = [
        MapOpts::new(MapSize::Small, MapShape::Continent),
        MapOpts::new(MapSize::Medium, MapShape::Continent),
        MapOpts::new(MapSize::Large, MapShape::Continent),
        MapOpts::new(MapSize::Small, MapShape::Islands),
        MapOpts::new(MapSize::Medium, MapShape::Islands),
        MapOpts::new(MapSize::Large, MapShape::Islands),
    ];

    pub(crate) const fn new(size: MapSize, shape: MapShape) -> Self {
        MapOpts { size, shape }
    }

    /// Short stable code for settings files and network messages:
    /// size s/m/l, a dash, shape c/i ("m-c" is the default).
    pub(crate) fn code(self) -> &'static str {
        match (self.size, self.shape) {
            (MapSize::Small, MapShape::Continent) => "s-c",
            (MapSize::Medium, MapShape::Continent) => "m-c",
            (MapSize::Large, MapShape::Continent) => "l-c",
            (MapSize::Small, MapShape::Islands) => "s-i",
            (MapSize::Medium, MapShape::Islands) => "m-i",
            (MapSize::Large, MapShape::Islands) => "l-i",
        }
    }

    /// Inverse of `code`; None for anything `code` never produces.
    pub(crate) fn from_code(s: &str) -> Option<Self> {
        let s = s.trim();
        Self::ALL.into_iter().find(|o| o.code() == s)
    }

    // (regions grown, minimum regions removed afterwards). Medium/Continent
    // is the classic N_SEEDS/N_HOLES. Islands remove few regions: their
    // coasts are already ragged, and every removal risks cutting a bridge.
    fn regions(self) -> (usize, usize) {
        match (self.size, self.shape) {
            (MapSize::Small, MapShape::Continent) => (36, 5),
            (MapSize::Medium, MapShape::Continent) => (N_SEEDS, N_HOLES),
            (MapSize::Large, MapShape::Continent) => (72, 5),
            // islands seed a few regions on the thin bridges; spare holes
            // absorb those slivers so island layouts rarely need a retry
            (MapSize::Small, MapShape::Islands) => (36, 4),
            (MapSize::Medium, MapShape::Islands) => (52, 6),
            (MapSize::Large, MapShape::Islands) => (68, 8),
        }
    }
}

// Which cells may become land; None = the whole grid (and no RNG drawn,
// which keeps the classic generator's random sequence untouched).
fn land_mask(opts: MapOpts) -> Option<Vec<bool>> {
    let mut mask = vec![false; COLS * ROWS];
    match (opts.size, opts.shape) {
        (MapSize::Medium | MapSize::Large, MapShape::Continent) => return None,
        (MapSize::Small, MapShape::Continent) => {
            // one compact landmass near the middle, about 60% of the grid
            let c = cell_at(
                COLS / 2 + gen_range(0, 5) - 2,
                ROWS / 2 + gen_range(0, 3) - 1,
            );
            grow_blob(&mut mask, c, 400, 1.6, None);
        }
        (size, MapShape::Islands) => {
            let (k, per) = match size {
                MapSize::Small => (3, 120),
                MapSize::Medium => (4, 118),
                MapSize::Large => (6, 80),
            };
            let mut owner = vec![usize::MAX; COLS * ROWS];
            let centers = island_centers(k);
            for (i, &c) in centers.iter().enumerate() {
                grow_blob(&mut mask, c, per, 1.25, Some((&mut owner, i)));
            }
            for (a, b) in spanning_tree(&centers) {
                for c in hex_line(centers[a], centers[b]) {
                    mask[c] = true;
                }
            }
        }
    }
    Some(mask)
}

fn cell_at(c: usize, r: usize) -> usize {
    r.min(ROWS - 1) * COLS + c.min(COLS - 1)
}

// the grid cell whose center is nearest to a point
fn nearest_cell(p: Vec2) -> usize {
    let r0 = ((p.y - OFF_Y) / (1.5 * HEX)).round() as i32;
    let mut best = (f32::MAX, 0usize);
    for r in (r0 - 1)..=(r0 + 1) {
        if r < 0 || r >= ROWS as i32 {
            continue;
        }
        let shift = if r % 2 == 1 { 0.5 } else { 0.0 };
        let c0 = ((p.x - OFF_X) / (SQRT3 * HEX) - shift).round() as i32;
        for c in (c0 - 1)..=(c0 + 1) {
            if c < 0 || c >= COLS as i32 {
                continue;
            }
            let i = r as usize * COLS + c as usize;
            let d = cell_pos(i).distance(p);
            if d < best.0 {
                best = (d, i);
            }
        }
    }
    best.1
}

// Cells along the straight segment between two cell centers. Successive
// samples land in the same or an adjacent hex (hexes meeting at a corner
// all touch each other), so the result is a connected one-hex-wide path.
fn hex_line(a: usize, b: usize) -> Vec<usize> {
    let (pa, pb) = (cell_pos(a), cell_pos(b));
    let steps = (pa.distance(pb) / (HEX * 0.25)).ceil().max(1.0) as usize;
    let mut out: Vec<usize> = Vec::new();
    for s in 0..=steps {
        let c = nearest_cell(pa + (pb - pa) * (s as f32 / steps as f32));
        if out.last() != Some(&c) {
            out.push(c);
        }
    }
    out
}

// Randomized compact growth from `center` up to `target` cells: each step
// draws three frontier cells and takes the one nearest the center (`aspect`
// stretches the blob horizontally). With an island id, the blob never
// touches another island's cells, so islands stay separated by water.
fn grow_blob(
    mask: &mut [bool],
    center: usize,
    target: usize,
    aspect: f32,
    mut island: Option<(&mut Vec<usize>, usize)>,
) {
    let cp = cell_pos(center);
    let dist = |i: usize| {
        let d = cell_pos(i) - cp;
        (d.x / aspect).powi(2) + d.y * d.y
    };
    let free = |c: usize, mask: &[bool], island: &Option<(&mut Vec<usize>, usize)>| {
        !mask[c]
            && island.as_ref().is_none_or(|(o, id)| {
                std::iter::once(c)
                    .chain(cell_neighbors(c))
                    .all(|n| o[n] == usize::MAX || o[n] == *id)
            })
    };
    let claim = |c: usize, mask: &mut [bool], island: &mut Option<(&mut Vec<usize>, usize)>| {
        mask[c] = true;
        if let Some((o, id)) = island {
            o[c] = *id;
        }
    };
    if !free(center, mask, &island) {
        return;
    }
    claim(center, mask, &mut island);
    let mut cand: Vec<usize> = cell_neighbors(center);
    let mut grown = 1;
    while grown < target && !cand.is_empty() {
        let mut k = gen_range(0, cand.len());
        for _ in 0..2 {
            let j = gen_range(0, cand.len());
            if dist(cand[j]) < dist(cand[k]) {
                k = j;
            }
        }
        let c = cand.swap_remove(k);
        if !free(c, mask, &island) {
            continue;
        }
        claim(c, mask, &mut island);
        grown += 1;
        cand.extend(cell_neighbors(c).into_iter().filter(|&n| !mask[n]));
    }
}

// k island centers spread over the grid (two cells clear of the edges),
// as far apart as a few random tries manage
fn island_centers(k: usize) -> Vec<usize> {
    let w = SQRT3 * HEX * COLS as f32;
    let h = 1.5 * HEX * ROWS as f32;
    let mut min_d = (w * h / k as f32).sqrt() * 0.95;
    let mut centers: Vec<usize> = Vec::new();
    let mut tries = 0;
    while centers.len() < k {
        let c = cell_at(gen_range(2, COLS - 2), gen_range(2, ROWS - 2));
        if centers
            .iter()
            .all(|&o| cell_pos(o).distance(cell_pos(c)) >= min_d)
        {
            centers.push(c);
        }
        tries += 1;
        if tries % 50 == 0 {
            min_d *= 0.9; // crowded: accept closer islands
        }
    }
    centers
}

// Prim's minimum spanning tree over the island centers: the bridges
fn spanning_tree(centers: &[usize]) -> Vec<(usize, usize)> {
    let mut in_tree = vec![false; centers.len()];
    in_tree[0] = true;
    let mut edges = Vec::new();
    for _ in 1..centers.len() {
        let mut best: Option<(f32, usize, usize)> = None;
        for a in (0..centers.len()).filter(|&a| in_tree[a]) {
            for b in (0..centers.len()).filter(|&b| !in_tree[b]) {
                let d = cell_pos(centers[a]).distance(cell_pos(centers[b]));
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, a, b));
                }
            }
        }
        let (_, a, b) = best.unwrap();
        in_tree[b] = true;
        edges.push((a, b));
    }
    edges
}

impl Game {
    // The default options run exactly the original generator (same RNG
    // draws, same results); other options restrict growth to a land mask
    // and change how many regions are grown and removed.
    pub(crate) fn new_with(
        players: usize,
        humans: usize,
        difficulty: Difficulty,
        teams: TeamCfg,
        opts: MapOpts,
    ) -> Self {
        let requested = opts;
        let mut opts = opts;
        let mut attempts = 0u32;
        'gen: loop {
            let total = COLS * ROWS;
            // Termination guard for the optional shapes: if an island layout
            // keeps failing the checks below, fall back to one landmass.
            attempts += 1;
            if attempts > 400 && opts.shape == MapShape::Islands {
                opts.shape = MapShape::Continent;
            }
            let (n_seeds, base_holes) = opts.regions();
            let land = land_mask(opts);
            let is_land = |c: usize| land.as_ref().is_none_or(|m| m[c]);

            // Grow n_seeds organic regions with a randomized multi-source flood fill.
            let mut cell_terr = vec![-1i32; total];
            let mut picked = HashSet::new();
            let mut frontier: Vec<usize> = Vec::new();
            let mut next_id = 0i32;
            while picked.len() < n_seeds {
                let c = gen_range(0, total);
                if is_land(c) && picked.insert(c) {
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
                    .filter(|&n| cell_terr[n] == -1 && is_land(n))
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
            let mut holes_n = base_holes;
            while (n_seeds - holes_n) % players != 0 {
                holes_n += 1;
            }
            let mut region_sizes = vec![0usize; n_seeds];
            for &t in &cell_terr {
                if t >= 0 {
                    region_sizes[t as usize] += 1;
                }
            }
            if region_sizes.iter().filter(|&&size| size < 3).count() > holes_n {
                continue 'gen;
            }
            let mut ids: Vec<i32> = (0..n_seeds as i32).collect();
            shuffle(&mut ids);
            // Tiny regions cannot hold a readable dice tower. Always spend
            // the available holes on those first instead of repeatedly
            // rebuilding the entire map until a random shuffle happens to
            // remove every one of them.
            ids.sort_by_key(|&id| region_sizes[id as usize] >= 3);
            let holes: HashSet<i32> = ids[..holes_n].iter().copied().collect();
            let mut remap = vec![-1i32; n_seeds];
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
            let n_terr = n_seeds - holes_n;

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
                map: requested,
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
                turns_done: 0,
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

    #[test]
    fn map_option_codes_round_trip() {
        assert_eq!(MapOpts::default().code(), "m-c");
        assert_eq!(
            MapOpts::from_code("s-i"),
            Some(MapOpts::new(MapSize::Small, MapShape::Islands))
        );
        for o in MapOpts::ALL {
            assert_eq!(MapOpts::from_code(o.code()), Some(o));
        }
        for bad in ["", "m", "x-c", "m-x", "M-C", "m-c-i"] {
            assert_eq!(MapOpts::from_code(bad), None, "{bad:?}");
        }
    }

    // every option and player count must terminate and keep the invariants:
    // equal shares, readable territories, one connected map
    #[test]
    fn every_map_option_generates_valid_maps() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        for opts in MapOpts::ALL {
            let mut sizes = Vec::new();
            for players in 2..=MAX_PLAYERS {
                for seed in 0..12u64 {
                    let g = gen_map_with(
                        seed * 7919,
                        players,
                        1,
                        Difficulty::Normal,
                        TeamCfg::default(),
                        opts,
                    );
                    let n = g.terrs.len();
                    sizes.push(n);
                    assert_eq!(g.map, opts);
                    assert!(n >= players * 2, "{} {players}p: {n}", opts.code());
                    for p in 0..players {
                        assert_eq!(g.count(p) * players, n);
                    }
                    let mut cells = vec![0usize; n];
                    for &t in g.cell_terr.iter().filter(|&&t| t >= 0) {
                        cells[t as usize] += 1;
                    }
                    assert!(cells.iter().all(|&c| c >= 3));
                    let mut seen = vec![false; n];
                    let mut stack = vec![0usize];
                    seen[0] = true;
                    while let Some(t) = stack.pop() {
                        for &nb in &g.terrs[t].neighbors {
                            if !seen[nb] {
                                seen[nb] = true;
                                stack.push(nb);
                            }
                        }
                    }
                    assert!(seen.iter().all(|&s| s), "{} disconnected", opts.code());
                }
            }
            let avg = sizes.iter().sum::<usize>() / sizes.len();
            match opts.size {
                MapSize::Small => assert!(avg < 36, "{} avg {avg}", opts.code()),
                MapSize::Medium => assert!((40..=56).contains(&avg), "{} avg {avg}", opts.code()),
                MapSize::Large => assert!(avg > 54, "{} avg {avg}", opts.code()),
            }
        }
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
