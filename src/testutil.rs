//! Shared helpers for unit tests.

use dicegame::engine::prelude::*;

use crate::board::MapOpts;
use crate::game::{assign_teams, Difficulty, Game, Persona, TeamCfg, TerrFx, Territory, TURN_SECS};

// a tiny hand-built map: territory i owned by owners[i], linked by edges
pub(crate) fn mk_game(
    owners: &[usize],
    edges: &[(usize, usize)],
    players: usize,
    teams: TeamCfg,
) -> Game {
    let mut terrs: Vec<Territory> = owners
        .iter()
        .map(|&o| Territory {
            owner: o,
            dice: 2,
            neighbors: Vec::new(),
            anchor: Vec2::ZERO,
            seed: 0,
        })
        .collect();
    for &(a, b) in edges {
        terrs[a].neighbors.push(b);
        terrs[b].neighbors.push(a);
    }
    Game {
        players,
        humans: 1,
        map: MapOpts::default(),
        teams,
        team: assign_teams(players, 1, teams),
        names: Vec::new(),
        absent: vec![false; players],
        turn_timer: TURN_SECS,
        difficulty: Difficulty::Normal,
        personas: vec![Persona::Aggressive; players],
        seed: 0,
        reserve: vec![0; players],
        recording: true,
        net_guest: false,
        my_seat: 0,
        events: Vec::new(),
        start: Vec::new(),
        cell_terr: Vec::new(),
        fx: vec![TerrFx::default(); owners.len()],
        terrs,
        current: 0,
        turns_done: players as u32, // hand-built boards are past the opening round
        selected: None,
        keyboard_hover: None,
        log: Vec::new(),
        hint: String::new(),
        hint_t: 0.0,
        ai_timer: 0.0,
        over: None,
        over_t: 0.0,
        battle: None,
        floats: Vec::new(),
        spectating: false,
        banner: String::new(),
        banner_t: 0.0,
        time: 0.0,
        snd: Vec::new(),
    }
}

// gen_map and the color table are process-global: tests touching them
// must not run concurrently
pub(crate) static GLOBALS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) const TEAMS_FF: TeamCfg = TeamCfg {
    on: true,
    count: 2,
    hvb: false,
    my_team: 0,
    bridge: false,
    ff: true,
};
