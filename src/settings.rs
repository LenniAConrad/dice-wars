//! Persistent settings and seed bookmarks in the per-user data directory.

use crate::game::{TeamCfg, MAX_PLAYERS, MAX_TEAMS, SEED_MOD};
use crate::theme::THEMES;

const BOOKMARK_FILE: &str = "bookmarks.txt";

// settings/bookmarks/replays live in the platform's per-user data directory,
// so the installed game works no matter where it is launched from
pub(crate) fn replay_file(name: &str) -> String {
    let dir = std::path::PathBuf::from(data_file("replays"));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name).to_string_lossy().into_owned()
}

pub(crate) fn data_file(name: &str) -> String {
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
    let _ = write_atomic(&data_file(BOOKMARK_FILE), &s);
}

// Write a sibling temp file and rename it over the target, so a crash or
// power loss mid-write never leaves a truncated file. The rename replaces
// the old file on every platform (MoveFileEx with REPLACE_EXISTING on Windows).
pub(crate) fn write_atomic(path: &str, contents: &str) -> std::io::Result<()> {
    let tmp = format!("{path}.{}.tmp", std::process::id());
    let result = std::fs::write(&tmp, contents).and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

const SETTINGS_FILE: &str = "settings.txt";

pub(crate) struct Settings {
    pub(crate) chances: bool, // show win-chance hints on attack options
    pub(crate) muted: bool,
    pub(crate) quiet: bool,      // half volume
    pub(crate) colorblind: bool, // alternative palette + shape badges on the map
    pub(crate) color: usize,     // which palette color the human plays as
    pub(crate) speed: f32,       // game speed multiplier (1x / 2x / 4x)
    pub(crate) reduced_motion: bool,
    pub(crate) theme: usize,        // index into theme::THEMES
    pub(crate) teams: bool,         // team mode on/off
    pub(crate) team_count: usize,   // number of teams (2..=MAX_TEAMS)
    pub(crate) team_hvb: bool,      // humans vs bots
    pub(crate) team_mine: usize,    // which team the local player picked
    pub(crate) team_bridge: bool,   // islands count as connected through teammate land
    pub(crate) team_ff: bool,       // teammates may attack each other
    pub(crate) name: String,        // display name in multiplayer lobbies
    pub(crate) last_addr: String,   // last host address joined, prefilled next time
    pub(crate) players: usize,      // last chosen player count
    pub(crate) difficulty: usize,   // last chosen difficulty index
    pub(crate) played: [u32; 3],    // finished solo games per difficulty
    pub(crate) won: [u32; 3],       // of those, the ones you won
    pub(crate) tutorial_done: bool, // the first-game tips were finished or skipped
}

// lobby names travel in space/comma-separated messages: keep them plain
pub(crate) fn clean_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(12)
        .collect()
}

impl Settings {
    pub(crate) fn games_played(&self) -> u32 {
        self.played.iter().sum()
    }

    pub(crate) fn games_won(&self) -> u32 {
        self.won.iter().sum()
    }

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
    parse_settings(&std::fs::read_to_string(data_file(SETTINGS_FILE)).unwrap_or_default())
}

fn parse_settings(txt: &str) -> Settings {
    let mut s = Settings {
        chances: true,
        muted: false,
        quiet: false,
        colorblind: false,
        color: 0,
        speed: 1.0,
        theme: 0,
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
        played: [0; 3],
        won: [0; 3],
        tutorial_done: false,
    };
    let has_theme = txt.lines().any(|l| l.starts_with("theme "));
    for l in txt.lines() {
        let mut it = l.split_whitespace();
        match (it.next(), it.next()) {
            (Some("chances"), Some(v)) => s.chances = v != "0",
            (Some("muted"), Some(v)) => s.muted = v != "0",
            (Some("quiet"), Some(v)) => s.quiet = v != "0",
            (Some("colorblind"), Some(v)) => s.colorblind = v != "0",
            (Some("color"), Some(v)) => s.color = v.parse::<usize>().unwrap_or(0) % MAX_PLAYERS,
            (Some("speed"), Some(v)) => {
                s.speed = match v {
                    "2" => 2.0,
                    "4" => 4.0,
                    _ => 1.0,
                }
            }
            (Some("reducedmotion"), Some(v)) => s.reduced_motion = v == "1",
            // older settings files only know light/dark
            (Some("dark"), Some(v)) if !has_theme => s.theme = (v != "0") as usize,
            (Some("theme"), Some(v)) => s.theme = v.parse::<usize>().unwrap_or(0) % THEMES.len(),
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
            (Some("difficulty"), Some(v)) => s.difficulty = v.parse::<usize>().unwrap_or(1).min(2),
            (Some("tutorial"), Some(v)) => s.tutorial_done = v != "0",
            // record p_easy w_easy p_normal w_normal p_hard w_hard
            (Some("record"), Some(first)) => {
                let n: Vec<u32> = std::iter::once(first)
                    .chain(it)
                    .map(|v| v.parse().unwrap_or(0))
                    .collect();
                for k in 0..3 {
                    s.played[k] = n.get(k * 2).copied().unwrap_or(0);
                    s.won[k] = n.get(k * 2 + 1).copied().unwrap_or(0).min(s.played[k]);
                }
            }
            _ => {}
        }
    }
    // players from before the tips existed have finished games: no tips
    if !txt.lines().any(|l| l.starts_with("tutorial ")) {
        s.tutorial_done = s.games_played() > 0;
    }
    s
}

pub(crate) fn save_settings(s: &Settings) {
    let _ = write_atomic(
        &data_file(SETTINGS_FILE),
        &format!(
            "chances {}\nmuted {}\ncolorblind {}\ncolor {}\nspeed {}\ndark {}\nteams {}\nteamcount {}\nteamhvb {}\nteammine {}\nteambridge {}\nteamff {}\nname {}\nlastaddr {}\nplayers {}\ndifficulty {}\nreducedmotion {}\nquiet {}\ntheme {}\nrecord {} {} {} {} {} {}\ntutorial {}\n",
            s.chances as u8,
            s.muted as u8,
            s.colorblind as u8,
            s.color,
            s.speed as u8,
            THEMES[s.theme % THEMES.len()].dark as u8,
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
            s.quiet as u8,
            s.theme,
            s.played[0],
            s.won[0],
            s.played[1],
            s.won[1],
            s.played[2],
            s.won[2],
            s.tutorial_done as u8
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::GLOBALS;

    #[test]
    fn legacy_dark_flag_becomes_a_theme() {
        assert_eq!(parse_settings("dark 0\n").theme, 0);
        assert_eq!(parse_settings("dark 1\n").theme, 1);
        // a newer file keeps `dark` for older versions; `theme` wins in any order
        assert_eq!(parse_settings("dark 1\ntheme 3\n").theme, 3);
        assert_eq!(parse_settings("theme 3\ndark 1\n").theme, 3);
        assert_eq!(parse_settings("theme 999\n").theme, 999 % THEMES.len());
        assert_eq!(parse_settings("").theme, 0);
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        // data_file reads APPDATA on Windows, HOME on macOS, XDG_DATA_HOME elsewhere
        let var = if cfg!(windows) {
            "APPDATA"
        } else if cfg!(target_os = "macos") {
            "HOME"
        } else {
            "XDG_DATA_HOME"
        };
        let dir = std::env::temp_dir().join(format!("dice-wars-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = std::env::var_os(var);
        std::env::set_var(var, &dir);

        let mut s = parse_settings("");
        s.chances = false;
        s.muted = true;
        s.quiet = true;
        s.colorblind = true;
        s.color = 4;
        s.speed = 4.0;
        s.reduced_motion = true;
        s.theme = 2 % THEMES.len();
        s.teams = true;
        s.team_count = 3.min(MAX_TEAMS);
        s.team_hvb = true;
        s.team_mine = 1;
        s.team_bridge = true;
        s.team_ff = false;
        s.name = "Ada_9".into();
        s.last_addr = "192.168.1.20:7777".into();
        s.players = 5;
        s.difficulty = 2;
        s.played = [3, 7, 1];
        s.won = [2, 4, 0];
        save_settings(&s);
        save_settings(&s); // replacing an existing file works too
        let l = load_settings();
        let data = if cfg!(target_os = "macos") {
            dir.join("Library/Application Support/dice-wars")
        } else {
            dir.join("dice-wars")
        };
        let leftovers: Vec<_> = std::fs::read_dir(&data)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        save_bookmarks(&[(12345678, 4)]);
        let bookmarks = load_bookmarks();

        match old {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        assert_eq!(bookmarks, vec![(12345678, 4)]);
        assert_eq!(
            (
                l.chances,
                l.muted,
                l.quiet,
                l.colorblind,
                l.color,
                l.speed,
                l.reduced_motion
            ),
            (false, true, true, true, 4, 4.0, true)
        );
        assert_eq!(l.theme, s.theme);
        assert_eq!((l.played, l.won), ([3, 7, 1], [2, 4, 0]));
        assert_eq!(
            (
                l.teams,
                l.team_count,
                l.team_hvb,
                l.team_mine,
                l.team_bridge,
                l.team_ff
            ),
            (true, s.team_count, true, 1, true, false)
        );
        assert_eq!(
            (l.name.as_str(), l.last_addr.as_str()),
            ("Ada_9", "192.168.1.20:7777")
        );
        assert_eq!((l.players, l.difficulty), (5, 2));
    }
}
