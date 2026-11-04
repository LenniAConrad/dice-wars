//! Persistent settings and seed bookmarks in the per-user data directory.

use crate::game::{TeamCfg, MAX_PLAYERS, MAX_TEAMS, SEED_MOD};

const BOOKMARK_FILE: &str = "bookmarks.txt";

// settings/bookmarks/replays live in the platform's per-user data directory,
// so the installed game works no matter where it is launched from
pub(crate) fn replay_file(name: &str) -> String {
    let dir = std::path::PathBuf::from(data_file("replays"));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name).to_string_lossy().into_owned()
}

fn data_file(name: &str) -> String {
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var("HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
    };
    match base {
        Some(b) => {
            let d = b.join("dice-wars");
            let _ = std::fs::create_dir_all(&d);
            d.join(name).to_string_lossy().into_owned()
        }
        None => name.to_string(),
    }
}

pub(crate) fn load_bookmarks() -> Vec<(u64, usize)> {
    std::fs::read_to_string(data_file(BOOKMARK_FILE))
        .ok()
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    let seed: u64 = it.next()?.parse().ok()?;
                    let players: usize = it.next()?.parse().ok()?;
                    Some((seed % SEED_MOD, players.clamp(2, MAX_PLAYERS)))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn save_bookmarks(b: &[(u64, usize)]) {
    let s: String = b.iter().map(|(s, p)| format!("{} {}\n", s, p)).collect();
    let _ = std::fs::write(data_file(BOOKMARK_FILE), s);
}

const SETTINGS_FILE: &str = "settings.txt";

pub(crate) struct Settings {
    pub(crate) chances: bool, // show win-chance hints on attack options
    pub(crate) muted: bool,
    pub(crate) quiet: bool,      // half volume
    pub(crate) colorblind: bool, // alternative palette + shape badges on the map
    pub(crate) color: usize,     // which palette color the human plays as
    pub(crate) speed: f32,       // game speed multiplier (1x / 2x)
    pub(crate) reduced_motion: bool,
    pub(crate) dark: bool,        // dark mode
    pub(crate) teams: bool,       // team mode on/off
    pub(crate) team_count: usize, // number of teams (2..=MAX_TEAMS)
    pub(crate) team_hvb: bool,    // humans vs bots
    pub(crate) team_mine: usize,  // which team the local player picked
    pub(crate) team_bridge: bool, // islands count as connected through teammate land
    pub(crate) team_ff: bool,     // teammates may attack each other
    pub(crate) name: String,      // display name in multiplayer lobbies
    pub(crate) last_addr: String, // last host address joined, prefilled next time
    pub(crate) players: usize,    // last chosen player count
    pub(crate) difficulty: usize, // last chosen difficulty index
}

// lobby names travel in space/comma-separated messages: keep them plain
pub(crate) fn clean_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(12)
        .collect()
}

impl Settings {
    pub(crate) fn team_cfg(&self) -> TeamCfg {
        TeamCfg {
            on: self.teams,
            count: self.team_count.clamp(2, MAX_TEAMS),
            hvb: self.team_hvb,
            my_team: self.team_mine % self.team_count.clamp(2, MAX_TEAMS),
            bridge: self.team_bridge,
            ff: self.team_ff,
        }
    }
}

pub(crate) fn load_settings() -> Settings {
    let mut s = Settings {
        chances: true,
        muted: false,
        quiet: false,
        colorblind: false,
        color: 0,
        speed: 1.0,
        dark: false,
        reduced_motion: false,
        teams: false,
        team_count: 2,
        team_hvb: false,
        team_mine: 0,
        team_bridge: false,
        team_ff: true,
        name: String::new(),
        last_addr: String::new(),
        players: 6,
        difficulty: 1,
    };
    if let Ok(txt) = std::fs::read_to_string(data_file(SETTINGS_FILE)) {
        for l in txt.lines() {
            let mut it = l.split_whitespace();
            match (it.next(), it.next()) {
                (Some("chances"), Some(v)) => s.chances = v != "0",
                (Some("muted"), Some(v)) => s.muted = v != "0",
                (Some("quiet"), Some(v)) => s.quiet = v != "0",
                (Some("colorblind"), Some(v)) => s.colorblind = v != "0",
                (Some("color"), Some(v)) => s.color = v.parse::<usize>().unwrap_or(0) % MAX_PLAYERS,
                (Some("speed"), Some(v)) => s.speed = if v == "2" { 2.0 } else { 1.0 },
                (Some("reducedmotion"), Some(v)) => s.reduced_motion = v == "1",
                (Some("dark"), Some(v)) => s.dark = v != "0",
                (Some("teams"), Some(v)) => s.teams = v != "0",
                (Some("teamcount"), Some(v)) => {
                    s.team_count = v.parse::<usize>().unwrap_or(2).clamp(2, MAX_TEAMS)
                }
                (Some("teamhvb"), Some(v)) => s.team_hvb = v != "0",
                (Some("teammine"), Some(v)) => {
                    s.team_mine = v.parse::<usize>().unwrap_or(0) % MAX_TEAMS
                }
                (Some("teambridge"), Some(v)) => s.team_bridge = v != "0",
                (Some("teamff"), Some(v)) => s.team_ff = v != "0",
                (Some("name"), Some(v)) => s.name = clean_name(v),
                (Some("lastaddr"), Some(v)) => s.last_addr = v.chars().take(40).collect(),
                (Some("players"), Some(v)) => {
                    s.players = v.parse::<usize>().unwrap_or(6).clamp(2, MAX_PLAYERS)
                }
                (Some("difficulty"), Some(v)) => {
                    s.difficulty = v.parse::<usize>().unwrap_or(1).min(2)
                }
                _ => {}
            }
        }
    }
    s
}

pub(crate) fn save_settings(s: &Settings) {
    let _ = std::fs::write(
        data_file(SETTINGS_FILE),
        format!(
            "chances {}\nmuted {}\ncolorblind {}\ncolor {}\nspeed {}\ndark {}\nteams {}\nteamcount {}\nteamhvb {}\nteammine {}\nteambridge {}\nteamff {}\nname {}\nlastaddr {}\nplayers {}\ndifficulty {}\nreducedmotion {}\nquiet {}\n",
            s.chances as u8,
            s.muted as u8,
            s.colorblind as u8,
            s.color,
            s.speed as u8,
            s.dark as u8,
            s.teams as u8,
            s.team_count,
            s.team_hvb as u8,
            s.team_mine,
            s.team_bridge as u8,
            s.team_ff as u8,
            s.name,
            s.last_addr,
            s.players,
            s.difficulty,
            s.reduced_motion as u8,
            s.quiet as u8
        ),
    );
}
