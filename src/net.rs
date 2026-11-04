//! Network play: protocol, host and guest sessions, authentication.
//!
//! Host-authoritative: guests send intents, the host validates, rolls, and
//! broadcasts the resulting events (the same RepEvents replays use).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use dicegame::engine;

use crate::board::N_SEEDS;
use crate::game::{
    diff_idx, fill_teams, Difficulty, Game, RepEvent, TeamCfg, MAX_DICE, MAX_PLAYERS, MAX_TEAMS,
};
use crate::menu::Menu;
use crate::settings::{clean_name, Settings};
use crate::theme::{color_map, resolve_colors};

const NET_PORT_DEFAULT: u16 = 7777;

pub(crate) const NET_PROTO: &str = "7"; // bumped whenever map generation or messages change

pub(crate) const NET_ROOM_CODE_LEN: usize = 6;

const NET_AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

const NET_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(75);

const NET_EVENT_QUEUE: usize = 256;

pub(crate) const NET_MAX_PENDING_EVENTS: usize = 512;

pub(crate) const NET_MESSAGES_PER_FRAME: usize = 32;

const NET_MAX_INTENTS_PER_SEC: u32 = 60;

const NET_MAX_TRACKED_IPS: usize = 4096;

const NET_AUTH_STATE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

pub(crate) fn secure_random<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    engine::random::fill_secure(&mut bytes).map_err(|e| {
        std::io::Error::other(format!("operating-system random source failed: {e}"))
    })?;
    Ok(bytes)
}

fn new_room_code() -> std::io::Result<String> {
    const CODES: u32 = 1_000_000;
    // Rejection sampling avoids making the lower room codes more likely when
    // the 32-bit random range is reduced to six decimal digits.
    const UNBIASED_RANGE: u32 = u32::MAX - u32::MAX % CODES;
    loop {
        let value = u32::from_le_bytes(secure_random::<4>()?);
        if value < UNBIASED_RANGE {
            return Ok(format!(
                "{:0width$}",
                value % CODES,
                width = NET_ROOM_CODE_LEN
            ));
        }
    }
}

pub(crate) fn new_reconnect_token() -> std::io::Result<String> {
    Ok(secure_random::<16>()?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn csv_usize(v: &[usize]) -> String {
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn parse_csv_usize(s: &str) -> Vec<usize> {
    s.split(',')
        .take(MAX_PLAYERS)
        .filter_map(|x| x.parse().ok())
        .collect()
}

pub(crate) fn parse_reinforcement_lands(s: &str) -> Option<Vec<usize>> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() > N_SEEDS * MAX_DICE as usize {
        return None;
    }
    parts.into_iter().map(|x| x.parse().ok()).collect()
}

// what every human picked in the lobby, keyed by 1-based lobby seat
#[derive(Default)]
pub(crate) struct LobbyPicks {
    pub(crate) team: HashMap<usize, usize>,
    pub(crate) color: HashMap<usize, usize>,
    pub(crate) name: HashMap<usize, String>,
}

// the teams and colors the game would start with right now: host first,
// joined guests in seat order, bots fill the rest
pub(crate) fn lobby_layout(
    occupied: &[usize], // 1-based lobby seats, ascending
    players: usize,
    teams: TeamCfg,
    picks: &LobbyPicks,
    host_color: usize,
) -> (Vec<usize>, Vec<usize>) {
    let humans = occupied.len() + 1;
    let mut tpicks: Vec<Option<usize>> = vec![None; players];
    let mut cpicks: Vec<Option<usize>> = vec![None; players];
    if !tpicks.is_empty() {
        tpicks[0] = Some(teams.my_team);
        cpicks[0] = Some(host_color);
    }
    for (k, &seat) in occupied.iter().enumerate() {
        if k + 1 < players {
            tpicks[k + 1] = picks.team.get(&seat).copied();
            cpicks[k + 1] = picks.color.get(&seat).copied();
        }
    }
    (
        fill_teams(players, humans, teams, &tpicks),
        resolve_colors(players, humans, &cpicks),
    )
}

// DW_PORT overrides the port (used by the automated tests)
pub(crate) fn net_port() -> u16 {
    std::env::var("DW_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(NET_PORT_DEFAULT)
}

const NET_MAX_LINE: usize = 200; // guest -> host intents are tiny

const NET_MAX_HOST_LINE: usize = 4096; // host -> guest events can be long (REINF lists)

fn write_net_line(stream: &mut std::net::TcpStream, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
}

pub(crate) fn send_net_line(stream: &mut std::net::TcpStream, line: &str) {
    let _ = write_net_line(stream, line);
}

// Read one newline-terminated line with a hard length cap. Interrupted
// syscalls are retried, but timeouts end the read so a peer cannot occupy a
// connection slot forever by sending an authentication line one byte at a time.
fn read_net_line_inner(
    r: &mut std::io::BufReader<std::net::TcpStream>,
    cap: usize,
    deadline: Option<std::time::Instant>,
) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    loop {
        if let Some(deadline) = deadline {
            let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
            r.get_ref().set_read_timeout(Some(remaining)).ok()?;
        }
        let mut byte = [0u8; 1];
        match r.read(&mut byte) {
            Ok(0) => return None,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
            Ok(_) => {
                if byte[0] == b'\n' {
                    return String::from_utf8(buf).ok();
                }
                if buf.len() >= cap {
                    return None;
                }
                buf.push(byte[0]);
            }
        }
    }
}

fn read_net_line(r: &mut std::io::BufReader<std::net::TcpStream>, cap: usize) -> Option<String> {
    read_net_line_inner(r, cap, None)
}

fn read_net_line_before(
    r: &mut std::io::BufReader<std::net::TcpStream>,
    cap: usize,
    timeout: std::time::Duration,
) -> Option<String> {
    read_net_line_inner(r, cap, Some(std::time::Instant::now() + timeout))
}

pub(crate) enum HostMsg {
    Joined(usize, String),
    Rejoined(usize), // lobby seat that reattached mid-game
    Left(usize),
    Intent(usize, String),
}

pub(crate) struct HostShared {
    pub(crate) seats: Vec<Option<std::net::TcpStream>>, // index = lobby seat - 1
    pub(crate) gens: Vec<u64>, // bumped on every (re)attach; guards stale cleanup
    pub(crate) tokens: HashMap<String, usize>, // reconnect token -> lobby seat
    pub(crate) banned: HashSet<std::net::IpAddr>,
    pub(crate) attempts: HashMap<std::net::IpAddr, u32>,
    pub(crate) last_try: HashMap<std::net::IpAddr, std::time::Instant>,
    pub(crate) last_auth_cleanup: std::time::Instant,
    pub(crate) conns: usize,
    pub(crate) started: bool,
}

// the active lobby's routing info; the accept thread reads this so the
// port only ever needs to be bound once per process
struct HostSession {
    pub(crate) code: String,
    pub(crate) shared: std::sync::Arc<std::sync::Mutex<HostShared>>,
    pub(crate) tx: std::sync::mpsc::SyncSender<HostMsg>,
}

static HOST_SLOT: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<Option<HostSession>>>> =
    std::sync::OnceLock::new();

static LISTENING: AtomicBool = AtomicBool::new(false);

fn host_slot() -> std::sync::Arc<std::sync::Mutex<Option<HostSession>>> {
    HOST_SLOT
        .get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new(None)))
        .clone()
}

pub(crate) struct HostNet {
    pub(crate) rx: std::sync::mpsc::Receiver<HostMsg>,
    pub(crate) shared: std::sync::Arc<std::sync::Mutex<HostShared>>,
    pub(crate) code: String,
    pub(crate) remap: HashMap<usize, usize>, // lobby seat -> game seat, set at start
    pub(crate) sent: usize,                  // events broadcast so far
}

impl Drop for HostNet {
    fn drop(&mut self) {
        *host_slot().lock().unwrap() = None; // lobby gone; listener stays
    }
}

impl HostNet {
    pub(crate) fn start(guests: usize) -> std::io::Result<HostNet> {
        // bind the port exactly once; afterwards every lobby reuses it,
        // avoiding TIME_WAIT rebind failures when hosting again
        if !LISTENING.load(Ordering::Relaxed) {
            let listener = std::net::TcpListener::bind(("0.0.0.0", net_port()))?;
            listener.set_nonblocking(true)?;
            LISTENING.store(true, Ordering::Relaxed);
            let slot = host_slot();
            std::thread::spawn(move || loop {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let ip = peer.ip();
                        let _ = stream.set_write_timeout(Some(NET_WRITE_TIMEOUT));
                        let session = {
                            let guard = slot.lock().unwrap();
                            match guard.as_ref() {
                                Some(s) => (s.code.clone(), s.shared.clone(), s.tx.clone()),
                                None => continue, // no lobby: drop the connection
                            }
                        };
                        let (code, shared, tx) = session;
                        {
                            let mut sh = shared.lock().unwrap();
                            if sh.last_auth_cleanup.elapsed() >= std::time::Duration::from_secs(60)
                            {
                                sh.last_try.retain(|_, t| t.elapsed() < NET_AUTH_STATE_TTL);
                                let live: HashSet<std::net::IpAddr> =
                                    sh.last_try.keys().copied().collect();
                                sh.attempts.retain(|addr, _| live.contains(addr));
                                sh.banned.retain(|addr| live.contains(addr));
                                sh.last_auth_cleanup = std::time::Instant::now();
                            }
                            // bans, connection cap, 1/sec per-IP rate limit
                            let too_fast = sh
                                .last_try
                                .get(&ip)
                                .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1));
                            let tracking_full = !sh.last_try.contains_key(&ip)
                                && sh.last_try.len() >= NET_MAX_TRACKED_IPS;
                            if sh.banned.contains(&ip)
                                || sh.conns >= 10
                                || too_fast
                                || tracking_full
                            {
                                continue;
                            }
                            sh.last_try.insert(ip, std::time::Instant::now());
                            sh.conns += 1;
                        }
                        let _ = stream.set_nonblocking(false);
                        std::thread::spawn(move || {
                            host_handle_client(stream, ip, code, shared, tx)
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            });
        }
        let code = new_room_code()?;
        let (tx, rx) = std::sync::mpsc::sync_channel(NET_EVENT_QUEUE);
        let shared = std::sync::Arc::new(std::sync::Mutex::new(HostShared {
            seats: (0..guests).map(|_| None).collect(),
            gens: vec![0; guests],
            tokens: HashMap::new(),
            banned: HashSet::new(),
            attempts: HashMap::new(),
            last_try: HashMap::new(),
            last_auth_cleanup: std::time::Instant::now(),
            conns: 0,
            started: false,
        }));
        *host_slot().lock().unwrap() = Some(HostSession {
            code: code.clone(),
            shared: shared.clone(),
            tx,
        });
        Ok(HostNet {
            rx,
            shared,
            code,
            remap: HashMap::new(),
            sent: 0,
        })
    }

    // 1-based lobby seats currently connected, ascending
    pub(crate) fn occupied(&self) -> Vec<usize> {
        self.shared
            .lock()
            .unwrap()
            .seats
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|_| i + 1))
            .collect()
    }

    // grow/shrink guest seats; never drops a connected guest. Returns the
    // resulting seat count.
    pub(crate) fn resize_guests(&self, guests: usize) -> usize {
        let mut sh = self.shared.lock().unwrap();
        let min = sh
            .seats
            .iter()
            .rposition(|s| s.is_some())
            .map_or(0, |i| i + 1);
        let n = guests.max(min).max(1);
        sh.seats.resize_with(n, || None);
        sh.gens.resize(n, 0);
        n
    }

    pub(crate) fn joined(&self) -> usize {
        self.shared
            .lock()
            .unwrap()
            .seats
            .iter()
            .filter(|s| s.is_some())
            .count()
    }

    pub(crate) fn broadcast(&mut self, line: &str) {
        let mut sh = self.shared.lock().unwrap();
        for slot in sh.seats.iter_mut().flatten() {
            if write_net_line(slot, line).is_err() {
                // Wake the client handler so it can clean up the seat and
                // notify the game loop. A peer that stops reading must never
                // freeze the render/input thread indefinitely.
                let _ = slot.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    // lock in whoever is connected, compact their seats, tell each guest
    // their game seat, and return the human count (host included)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_game(
        &mut self,
        seed: u64,
        players: usize,
        diff: Difficulty,
        teams: TeamCfg,
        picks: &LobbyPicks,
        host_color: usize,
        host_name: &str,
    ) -> (usize, Vec<usize>, Vec<usize>, Vec<String>) {
        let mut sh = self.shared.lock().unwrap();
        sh.started = true;
        let occupied: Vec<usize> = sh
            .seats
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|_| i + 1))
            .collect();
        let humans = occupied.len() + 1;
        let (team_v, color_v) = lobby_layout(&occupied, players, teams, picks, host_color);
        let mut names: Vec<String> = vec![if host_name.is_empty() {
            "-".to_string()
        } else {
            host_name.to_string()
        }];
        for &seat in &occupied {
            names.push(match picks.name.get(&seat) {
                Some(n) if !n.is_empty() => n.clone(),
                _ => "-".to_string(),
            });
        }
        let line_tail = format!(
            "{} {} {} {} {} {} {} {}",
            csv_usize(&color_v),
            teams.on as u8,
            teams.bridge as u8,
            teams.ff as u8,
            teams.count.clamp(2, MAX_TEAMS),
            teams.hvb as u8,
            csv_usize(&team_v),
            names.join(",")
        );
        self.remap.clear();
        for (k, &seat) in occupied.iter().enumerate() {
            self.remap.insert(seat, k + 1);
            if let Some(s) = sh.seats[seat - 1].as_mut() {
                let line = format!(
                    "START {} {} {} {} {} {}",
                    seed,
                    players,
                    humans,
                    diff_idx(diff),
                    k + 1,
                    line_tail
                );
                if write_net_line(s, &line).is_err() {
                    let _ = s.shutdown(std::net::Shutdown::Both);
                }
            }
        }
        let names_clean = names
            .iter()
            .map(|n| if n == "-" { String::new() } else { n.clone() })
            .collect();
        (humans, team_v, color_v, names_clean)
    }

    // resend everything a reattached guest missed, then mark it synced
    pub(crate) fn send_resume(&mut self, lobby_seat: usize, g: &Game, seed: u64, diff: Difficulty) {
        let mut sh = self.shared.lock().unwrap();
        let Some(&game_seat) = self.remap.get(&lobby_seat) else {
            return;
        };
        let colors: Vec<usize> = (0..g.players).map(color_map).collect();
        let names: Vec<String> = (0..g.humans)
            .map(|p| match g.names.get(p) {
                Some(n) if !n.is_empty() => n.clone(),
                _ => "-".to_string(),
            })
            .collect();
        let absent: Vec<usize> = g.absent.iter().map(|&a| a as usize).collect();
        if let Some(s) = sh.seats[lobby_seat - 1].as_mut() {
            let header = format!(
                "RESUME {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                seed,
                g.players,
                g.humans,
                diff_idx(diff),
                game_seat,
                csv_usize(&colors),
                g.teams.on as u8,
                g.teams.bridge as u8,
                g.teams.ff as u8,
                g.teams.count.clamp(2, MAX_TEAMS),
                g.teams.hvb as u8,
                csv_usize(&g.team),
                names.join(","),
                csv_usize(&absent)
            );
            if write_net_line(s, &header).is_err() {
                let _ = s.shutdown(std::net::Shutdown::Both);
                return;
            }
            // Only events already broadcast: anything newer reaches this
            // guest through the next flush_events, exactly once.
            for ev in &g.events[..self.sent.min(g.events.len())] {
                let line = match ev {
                    RepEvent::Attack {
                        a,
                        d,
                        ra,
                        rd,
                        captured,
                    } => format!("ATK {} {} {} {} {}", a, d, ra, rd, *captured as u8),
                    RepEvent::Reinforce { player, lands } => {
                        let ls: Vec<String> = lands.iter().map(|t| t.to_string()).collect();
                        format!("REINF {} {}", player, ls.join(","))
                    }
                };
                if write_net_line(s, &line).is_err() {
                    let _ = s.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
            if write_net_line(s, "SYNCED").is_err() {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    // send any newly recorded game events to all guests
    pub(crate) fn flush_events(&mut self, g: &Game) {
        while self.sent < g.events.len() {
            let line = match &g.events[self.sent] {
                RepEvent::Attack {
                    a,
                    d,
                    ra,
                    rd,
                    captured,
                } => format!("ATK {} {} {} {} {}", a, d, ra, rd, *captured as u8),
                RepEvent::Reinforce { player, lands } => {
                    let ls: Vec<String> = lands.iter().map(|t| t.to_string()).collect();
                    format!("REINF {} {}", player, ls.join(","))
                }
            };
            self.sent += 1;
            self.broadcast(&line);
        }
    }
}

// everything a guest needs to render the lobby, broadcast by the host
pub(crate) fn lobby_line(
    h: &HostNet,
    menu: &Menu,
    settings: &Settings,
    picks: &LobbyPicks,
) -> String {
    let occ = h.occupied();
    let cfg = settings.team_cfg();
    let (team_v, color_v) = lobby_layout(&occ, menu.players, cfg, picks, settings.color);
    let mut names: Vec<String> = vec![if settings.name.is_empty() {
        "-".to_string()
    } else {
        settings.name.clone()
    }];
    for &seat in &occ {
        names.push(match picks.name.get(&seat) {
            Some(n) if !n.is_empty() => n.clone(),
            _ => "-".to_string(),
        });
    }
    format!(
        "LOBBY {} {} {} {} {} {} {} {} {} {} {} {}",
        menu.seed,
        menu.players,
        diff_idx(menu.difficulty),
        cfg.on as u8,
        cfg.hvb as u8,
        cfg.count.clamp(2, MAX_TEAMS),
        cfg.ff as u8,
        cfg.bridge as u8,
        if occ.is_empty() {
            "-".to_string()
        } else {
            csv_usize(&occ)
        },
        csv_usize(&team_v),
        csv_usize(&color_v),
        names.join(",")
    )
}

fn host_handle_client(
    stream: std::net::TcpStream,
    ip: std::net::IpAddr,
    code: String,
    shared: std::sync::Arc<std::sync::Mutex<HostShared>>,
    tx: std::sync::mpsc::SyncSender<HostMsg>,
) {
    let release = |sh: &std::sync::Arc<std::sync::Mutex<HostShared>>| {
        sh.lock().unwrap().conns -= 1;
    };
    let mut reader = match stream.try_clone() {
        Ok(s) => std::io::BufReader::new(s),
        Err(_) => return release(&shared),
    };
    let mut w = stream;
    // auth: JOIN <code> for a fresh seat, REJOIN <code> <token> to reclaim one
    let mut rejoin_seat: Option<usize> = None;
    let mut rejoin_token: Option<String> = None;
    let mut join_name = String::new();
    let ok = match read_net_line_before(&mut reader, NET_MAX_LINE, NET_AUTH_TIMEOUT) {
        Some(l) => {
            let mut it = l.split_whitespace();
            match it.next() {
                Some("JOIN") => {
                    if it.next() != Some(code.as_str()) {
                        false
                    } else if it.next() != Some(NET_PROTO) {
                        send_net_line(&mut w, "BYE version mismatch - download the latest release");
                        release(&shared);
                        return;
                    } else {
                        join_name = clean_name(it.next().unwrap_or(""));
                        true
                    }
                }
                Some("REJOIN") => {
                    if it.next() == Some(code.as_str()) {
                        if let Some(tok) = it.next() {
                            if let Ok(next_token) = new_reconnect_token() {
                                let mut sh = shared.lock().unwrap();
                                if let (Some(&slot_seat), Ok(clone)) =
                                    (sh.tokens.get(tok), w.try_clone())
                                {
                                    sh.seats[slot_seat - 1] = Some(clone);
                                    sh.gens[slot_seat - 1] += 1;
                                    sh.tokens.retain(|_, token_seat| *token_seat != slot_seat);
                                    sh.tokens.insert(next_token.clone(), slot_seat);
                                    rejoin_seat = Some(slot_seat);
                                    rejoin_token = Some(next_token);
                                }
                            }
                        }
                    }
                    rejoin_seat.is_some()
                }
                _ => false,
            }
        }
        None => false,
    };
    if !ok {
        {
            let mut sh = shared.lock().unwrap();
            let n = sh.attempts.entry(ip).or_insert(0);
            *n += 1;
            if *n >= 5 {
                sh.banned.insert(ip); // five strikes: ignored for the session
            }
            sh.conns -= 1;
        }
        send_net_line(&mut w, "BYE wrong code");
        return;
    }
    shared.lock().unwrap().attempts.remove(&ip);
    let fresh_token = if rejoin_seat.is_none() {
        match new_reconnect_token() {
            Ok(token) => Some(token),
            Err(_) => {
                send_net_line(&mut w, "BYE secure random source unavailable");
                return release(&shared);
            }
        }
    } else {
        None
    };
    let seat = if let Some(s) = rejoin_seat {
        Some(s)
    } else {
        let mut sh = shared.lock().unwrap();
        if sh.started {
            None
        } else {
            match sh.seats.iter().position(|s| s.is_none()) {
                Some(i) => match w.try_clone() {
                    Ok(clone) => {
                        sh.seats[i] = Some(clone);
                        sh.gens[i] += 1;
                        Some(i + 1)
                    }
                    Err(_) => None,
                },
                None => None,
            }
        }
    };
    let Some(seat) = seat else {
        send_net_line(&mut w, "BYE full");
        return release(&shared);
    };
    let my_gen = shared.lock().unwrap().gens[seat - 1];
    let _ = reader.get_ref().set_read_timeout(None);
    if rejoin_seat.is_some() {
        if let Some(token) = rejoin_token {
            send_net_line(&mut w, &format!("RECONNECTED {} {}", seat, token));
        }
        let _ = tx.send(HostMsg::Rejoined(seat));
    } else {
        // fresh join: issue the reconnect token
        let token = {
            let t = fresh_token.expect("fresh joins always create a token");
            let mut sh = shared.lock().unwrap();
            // A lobby seat may have been reused after a disconnect. Invalidate
            // every older token for that seat so its previous occupant cannot
            // reclaim the new player's connection.
            sh.tokens.retain(|_, token_seat| *token_seat != seat);
            sh.tokens.insert(t.clone(), seat);
            t
        };
        send_net_line(&mut w, &format!("WELCOME {} {}", seat, token));
        let _ = tx.send(HostMsg::Joined(seat, join_name));
    }
    let mut intent_window = std::time::Instant::now();
    let mut intents_in_window = 0u32;
    loop {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut dead = None;
        loop {
            let mut byte = [0u8; 1];
            match reader.read(&mut byte) {
                Ok(0) => {
                    dead = Some("EOF".to_string());
                    break;
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut => continue,
                    k => {
                        dead = Some(format!("ERR {:?} {}", k, e));
                        break;
                    }
                },
                Ok(_) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    if buf.len() >= NET_MAX_LINE {
                        dead = Some("LINE TOO LONG".to_string());
                        break;
                    }
                    buf.push(byte[0]);
                }
            }
        }
        if let Some(why) = dead {
            eprintln!("CLIENT_READ_END seat {} reason {}", seat, why);
            break;
        }
        if intent_window.elapsed() >= std::time::Duration::from_secs(1) {
            intent_window = std::time::Instant::now();
            intents_in_window = 0;
        }
        intents_in_window += 1;
        let Ok(l) = String::from_utf8(buf) else {
            break;
        };
        if intents_in_window > NET_MAX_INTENTS_PER_SEC
            || tx.try_send(HostMsg::Intent(seat, l)).is_err()
        {
            break;
        }
    }
    {
        let mut sh = shared.lock().unwrap();
        sh.conns -= 1;
        if sh.gens[seat - 1] != my_gen {
            return; // the seat was reclaimed by a reconnect; don't touch it
        }
        sh.seats[seat - 1] = None;
    }
    let _ = tx.send(HostMsg::Left(seat));
}

type NetParts = (std::net::TcpStream, std::sync::mpsc::Receiver<String>);

pub(crate) struct GuestNet {
    pub(crate) stream: std::net::TcpStream, // write half
    pub(crate) rx: std::sync::mpsc::Receiver<String>,
    pub(crate) seat: usize,
    pub(crate) pending: std::collections::VecDeque<RepEvent>,
    pub(crate) addr: String,
    pub(crate) code: String,
    pub(crate) token: String,
    pub(crate) catching_up: bool, // replaying the backlog after a reconnect
    pub(crate) rec_failed: bool,  // auto-reconnect gave up; the player may retry
    pub(crate) rec_rx: Option<std::sync::mpsc::Receiver<Result<NetParts, String>>>,
}

pub(crate) fn reject_invalid_host_data(g: &mut Game, gn: &mut GuestNet) {
    gn.pending.clear();
    gn.catching_up = false;
    let _ = gn.stream.shutdown(std::net::Shutdown::Both);
    g.battle = None;
    g.over = Some("Disconnected — host sent invalid game data".to_string());
    g.banner_t = 0.0;
}

// spawn a reader thread over a connected socket, yielding its line channel
fn spawn_reader(stream: &std::net::TcpStream) -> Result<std::sync::mpsc::Receiver<String>, String> {
    let mut reader =
        std::io::BufReader::new(stream.try_clone().map_err(|_| "Socket error".to_string())?);
    // Bound host-to-guest buffering so an untrusted host cannot grow the
    // client's memory without limit. Blocking this background reader applies
    // ordinary TCP backpressure and never stalls the render thread.
    let (tx, rx) = std::sync::mpsc::sync_channel(NET_EVENT_QUEUE);
    std::thread::spawn(move || loop {
        match read_net_line(&mut reader, NET_MAX_HOST_LINE) {
            Some(l) => {
                if tx.send(l).is_err() {
                    break;
                }
            }
            None => {
                let _ = tx.send("LOST".to_string());
                break;
            }
        }
    });
    Ok(rx)
}

// background auto-reconnect: keep trying to reclaim our seat with the token
pub(crate) fn spawn_reconnect(
    addr: String,
    code: String,
    token: String,
) -> std::sync::mpsc::Receiver<Result<NetParts, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let full = if addr.contains(':') {
            addr.clone()
        } else {
            format!("{}:{}", addr, net_port())
        };
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let Ok(sock) = full.parse::<std::net::SocketAddr>() else {
                break;
            };
            if let Ok(stream) =
                std::net::TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(3))
            {
                let _ = stream.set_write_timeout(Some(NET_WRITE_TIMEOUT));
                let mut w = match stream.try_clone() {
                    Ok(w) => w,
                    Err(_) => continue,
                };
                if write_net_line(&mut w, &format!("REJOIN {} {}", code, token)).is_err() {
                    continue;
                }
                if let Ok(lines) = spawn_reader(&stream) {
                    let _ = tx.send(Ok((w, lines)));
                    return;
                }
            }
        }
        let _ = tx.send(Err("Could not reconnect".to_string()));
    });
    rx
}

pub(crate) fn guest_connect(addr: &str, code: &str, name: &str) -> Result<GuestNet, String> {
    let addr_owned = addr.to_string();
    let code_owned = code.to_string();
    let full = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{}:{}", addr, net_port())
    };
    let sock: std::net::SocketAddr = full.parse().map_err(|_| "Bad address".to_string())?;
    let stream = std::net::TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(5))
        .map_err(|_| {
            "Could not connect — wrong address, or firewall blocking port 7777".to_string()
        })?;
    let _ = stream.set_write_timeout(Some(NET_WRITE_TIMEOUT));
    let mut w = stream.try_clone().map_err(|_| "Socket error".to_string())?;
    write_net_line(
        &mut w,
        &format!("JOIN {} {} {}", code, NET_PROTO, clean_name(name)),
    )
    .map_err(|_| "Could not send join request".to_string())?;
    let mut reader =
        std::io::BufReader::new(stream.try_clone().map_err(|_| "Socket error".to_string())?);
    let hello = read_net_line_before(
        &mut reader,
        NET_MAX_HOST_LINE,
        std::time::Duration::from_secs(8),
    )
    .ok_or_else(|| {
        "Host unreachable — check the code, and the host's firewall (port 7777)".to_string()
    })?;
    let mut it = hello.split_whitespace();
    match it.next() {
        Some("WELCOME") => {}
        Some("BYE") => {
            return Err(format!("Rejected: {}", it.collect::<Vec<_>>().join(" ")));
        }
        _ => return Err("Bad reply".to_string()),
    }
    let seat: usize = it
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "Bad reply".to_string())?;
    let token = it.next().unwrap_or("");
    if seat == 0
        || seat >= MAX_PLAYERS
        || token.len() != 32
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
        || it.next().is_some()
    {
        return Err("Bad reply".to_string());
    }
    let token = token.to_string();
    let _ = stream.set_read_timeout(None);
    drop(reader);
    let rx = spawn_reader(&stream)?;
    Ok(GuestNet {
        stream: w,
        rx,
        seat,
        pending: std::collections::VecDeque::new(),
        addr: addr_owned,
        code: code_owned,
        token,
        catching_up: false,
        rec_failed: false,
        rec_rx: None,
    })
}

pub(crate) fn spawn_guest_connect(
    addr: String,
    code: String,
    name: String,
) -> std::sync::mpsc::Receiver<Result<GuestNet, String>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(guest_connect(&addr, &code, &name));
    });
    rx
}

// figure out the LAN address to show in the host lobby; cached because it
// is drawn every frame and the socket probe can be slow on some networks
pub(crate) fn local_ip() -> String {
    static IP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    IP.get_or_init(|| {
        std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| {
                s.connect("8.8.8.8:80")?;
                s.local_addr()
            })
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|_| "?".to_string())
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use crate::testutil::{mk_game, GLOBALS};

    use super::*;

    #[test]
    fn guest_rejects_invalid_host_events_without_panicking() {
        let _g = GLOBALS.lock().unwrap();
        let game = || mk_game(&[0, 1], &[(0, 1)], 2, TeamCfg::default());

        let mut g = game();
        assert!(!g.net_apply_attack(usize::MAX, 1, 7, 6, true));
        assert!(g.battle.is_none());

        let mut g = game();
        assert!(!g.net_apply_attack(0, 1, 1, 6, false));
        assert!(!g.net_apply_attack(0, 1, 7, 6, false));
        assert!(g.net_apply_attack(0, 1, 7, 6, true));

        let mut g = game();
        assert!(!g.net_apply_reinforce(usize::MAX, vec![0]));
        assert!(!g.net_apply_reinforce(0, vec![1]));
        assert!(g.net_apply_reinforce(0, vec![0]));
        assert_eq!(g.terrs[0].dice, 3);

        let mut g = game();
        g.over = Some("finished".to_string());
        assert!(!g.net_apply_attack(0, 1, 7, 6, true));
        assert!(!g.net_apply_reinforce(0, vec![0]));
    }

    #[test]
    fn reconnect_backlog_detects_the_final_win() {
        let mut g = mk_game(&[0, 1], &[(0, 1)], 2, TeamCfg::default());
        g.net_guest = true;
        assert!(g.net_apply_attack_fast(0, 1, 7, 6, true));
        assert!(g.over.is_some());
    }

    #[test]
    fn credentials_use_fixed_length_os_random_values() {
        let room = new_room_code().unwrap();
        assert_eq!(room.len(), NET_ROOM_CODE_LEN);
        assert!(room.bytes().all(|b| b.is_ascii_digit()));

        let token = new_reconnect_token().unwrap();
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn guest_rejects_malformed_welcome_credentials() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let fake_host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            assert!(read_net_line_before(
                &mut reader,
                NET_MAX_LINE,
                std::time::Duration::from_secs(1),
            )
            .is_some());
            write_net_line(
                &mut stream,
                "WELCOME 999 00000000000000000000000000000000 trailing",
            )
            .unwrap();
        });
        let result = guest_connect(&addr.to_string(), "123456", "Player");
        assert!(matches!(result, Err(message) if message == "Bad reply"));
        fake_host.join().unwrap();
    }

    #[test]
    fn host_and_guest_complete_secure_handshake() {
        let _g = GLOBALS.lock().unwrap();
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        std::env::set_var("DW_PORT", port.to_string());

        let host = HostNet::start(1).unwrap();
        let guest = guest_connect("127.0.0.1", &host.code, "Test_Player").unwrap();
        assert_eq!(guest.seat, 1);
        assert_eq!(guest.token.len(), 32);
        assert!(guest.token.bytes().all(|b| b.is_ascii_hexdigit()));
        match host.rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(HostMsg::Joined(1, name)) => assert_eq!(name, "Test_Player"),
            _ => panic!("host did not receive the authenticated join"),
        }

        let old_token = guest.token.clone();
        let _ = guest.stream.shutdown(std::net::Shutdown::Both);
        drop(guest);

        // A reconnect rotates the bearer token. Replaying the prior token must
        // no longer be able to take the seat away from its current occupant.
        host.shared
            .lock()
            .unwrap()
            .last_try
            .remove(&"127.0.0.1".parse().unwrap());
        let mut rejoined = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write_net_line(
            &mut rejoined,
            &format!("REJOIN {} {}", host.code, old_token),
        )
        .unwrap();
        let mut reply = std::io::BufReader::new(rejoined.try_clone().unwrap());
        let line = read_net_line_before(
            &mut reply,
            NET_MAX_HOST_LINE,
            std::time::Duration::from_secs(2),
        )
        .unwrap();
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("RECONNECTED"));
        assert_eq!(fields.next(), Some("1"));
        let new_token = fields.next().unwrap();
        assert_ne!(new_token, old_token);

        host.shared
            .lock()
            .unwrap()
            .last_try
            .remove(&"127.0.0.1".parse().unwrap());
        let mut replay = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write_net_line(&mut replay, &format!("REJOIN {} {}", host.code, old_token)).unwrap();
        let mut rejected = std::io::BufReader::new(replay.try_clone().unwrap());
        let line = read_net_line_before(
            &mut rejected,
            NET_MAX_HOST_LINE,
            std::time::Duration::from_secs(2),
        )
        .unwrap();
        assert!(line.starts_with("BYE "));

        let _ = rejoined.shutdown(std::net::Shutdown::Both);
        let _ = replay.shutdown(std::net::Shutdown::Both);
        drop(host);
        std::env::remove_var("DW_PORT");
    }

    #[test]
    fn reinforcement_parser_is_strict_and_bounded() {
        assert_eq!(parse_reinforcement_lands("1,2,3"), Some(vec![1, 2, 3]));
        assert_eq!(parse_reinforcement_lands("1,nope,3"), None);
        let oversized = vec!["0"; N_SEEDS * MAX_DICE as usize + 1].join(",");
        assert_eq!(parse_reinforcement_lands(&oversized), None);
    }

    #[test]
    fn authentication_line_has_an_absolute_deadline() {
        use std::io::Write;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream.write_all(b"J").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(250));
        });
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let started = std::time::Instant::now();
        assert_eq!(
            read_net_line_before(
                &mut reader,
                NET_MAX_LINE,
                std::time::Duration::from_millis(40),
            ),
            None
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
        peer.join().unwrap();
    }
}
