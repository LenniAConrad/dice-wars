//! Internet relay for network games: players reach each other through a small
//! server anyone can run, so nobody has to forward a port.
//!
//! The relay never parses game traffic. It matches a guest to a host by room
//! code, asks the host to open a fresh connection for that guest, and then
//! copies bytes between the two sockets. From there on host and guest speak
//! the ordinary game protocol, exactly as on a direct connection.
//!
//! Every line is ASCII, space separated and newline terminated:
//!
//! ```text
//! host control   -> RELAY-HOST <proto> <code> <secret>
//!                <- RELAY-OK | RELAY-ERR <reason>
//!                <- CONN <id> <guest ip>      (a guest is waiting)
//!                -> PING   <- PONG            (keepalive)
//!                -> BYE                       (close the room now)
//! guest          -> RELAY-JOIN <proto> <code>
//!                <- RELAY-OK, then the host's bytes
//!                 | BYE <reason>              (final: unknown code, version, full)
//!                 | RELAY-WAIT <reason>       (try again shortly)
//! host attach    -> RELAY-ATTACH <code> <id>
//!                <- RELAY-OK, then the guest's bytes | BYE <reason>
//! ```
//!
//! The secret lets a host whose control connection dropped reclaim its room
//! (same code) during a grace period; anyone else gets "room code in use".
//!
//! This file is shared: the game includes it for the client helpers (and the
//! tests), and `src/bin/dice-wars-relay.rs` includes it for the server. It
//! uses only `std`, so the relay builds on a server without any desktop
//! libraries.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const RELAY_PORT_DEFAULT: u16 = 7780;

/// Longest request line the relay accepts.
pub const RELAY_MAX_LINE: usize = 160;

/// Longest line on an established host control connection.
const CONTROL_MAX_LINE: usize = 64;

const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

const SPLICE_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

const MAX_TRACKED_IPS: usize = 8192;

#[derive(Clone, Debug)]
pub struct Config {
    /// Rooms registered at once (online, or offline within the grace period).
    pub max_rooms: usize,
    /// Guests per room, waiting plus connected.
    pub max_guests_per_room: usize,
    /// Open connections in total (each connected guest also holds the
    /// host's attach connection).
    pub max_conns: usize,
    /// Open connections from one IP address.
    pub max_conns_per_ip: usize,
    /// Time a new connection gets to send its request line.
    pub header_timeout: Duration,
    /// Time a host gets to attach after being told about a guest.
    pub attach_timeout: Duration,
    /// A control connection silent this long is considered dead.
    pub control_idle: Duration,
    /// A guest connection with no traffic either way this long is closed.
    pub idle_timeout: Duration,
    /// How long a room outlives its host's control connection.
    pub grace: Duration,
    /// Failed requests (unknown codes, malformed lines) before an IP is banned.
    pub max_failures: u32,
    pub ban_time: Duration,
    pub log: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_rooms: 500,
            max_guests_per_room: 16,
            max_conns: 1024,
            max_conns_per_ip: 32,
            header_timeout: Duration::from_secs(10),
            attach_timeout: Duration::from_secs(10),
            control_idle: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60 * 60),
            grace: Duration::from_secs(90),
            max_failures: 20,
            ban_time: Duration::from_secs(10 * 60),
            log: true,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Host {
        proto: String,
        code: String,
        secret: String,
    },
    Join {
        proto: String,
        code: String,
    },
    Attach {
        code: String,
        id: String,
    },
}

fn token_ok(s: &str, min: usize, max: usize, ok: fn(u8) -> bool) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(ok)
}

pub fn valid_proto(s: &str) -> bool {
    token_ok(s, 1, 16, |b| {
        b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_'
    })
}

pub fn valid_code(s: &str) -> bool {
    token_ok(s, 1, 16, |b| b.is_ascii_alphanumeric())
}

pub fn valid_secret(s: &str) -> bool {
    token_ok(s, 16, 64, |b| b.is_ascii_hexdigit())
}

pub fn valid_id(s: &str) -> bool {
    token_ok(s, 32, 32, |b| b.is_ascii_hexdigit())
}

/// Parse a request line. Anything unexpected, including extra fields, is None.
pub fn parse_request(line: &str) -> Option<Request> {
    let fields: Vec<&str> = line.split_whitespace().take(5).collect();
    match fields.as_slice() {
        ["RELAY-HOST", proto, code, secret]
            if valid_proto(proto) && valid_code(code) && valid_secret(secret) =>
        {
            Some(Request::Host {
                proto: proto.to_string(),
                code: code.to_string(),
                secret: secret.to_string(),
            })
        }
        ["RELAY-JOIN", proto, code] if valid_proto(proto) && valid_code(code) => {
            Some(Request::Join {
                proto: proto.to_string(),
                code: code.to_string(),
            })
        }
        ["RELAY-ATTACH", code, id] if valid_code(code) && valid_id(id) => Some(Request::Attach {
            code: code.to_string(),
            id: id.to_string(),
        }),
        _ => None,
    }
}

/// `addr` with `port` appended unless it already names one. Accepts
/// `host`, `host:port`, `1.2.3.4`, `::1`, `[::1]` and `[::1]:port`.
pub fn with_default_port(addr: &str, port: u16) -> String {
    let a = addr.trim();
    if a.parse::<SocketAddr>().is_ok() {
        return a.to_string();
    }
    let bare = a.strip_prefix('[').and_then(|r| r.strip_suffix(']'));
    if let Ok(ip) = bare.unwrap_or(a).parse::<IpAddr>() {
        return SocketAddr::new(ip, port).to_string();
    }
    if a.matches(':').count() == 1 {
        a.to_string()
    } else {
        format!("{a}:{port}")
    }
}

/// Resolve `addr` (DNS names allowed) and connect to the first address that
/// answers within `timeout`.
pub fn connect_any(addr: &str, default_port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let full = with_default_port(addr, default_port);
    let mut last = std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{full} did not resolve"),
    );
    for sa in full.to_socket_addrs()? {
        match TcpStream::connect_timeout(&sa, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

pub fn write_line(mut stream: &TcpStream, line: &str) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    stream.write_all(&buf)
}

fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn invalid(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

/// Read one line a byte at a time, so nothing after the newline is consumed:
/// whatever follows belongs to the next reader of the socket. Fails at
/// `deadline`, on EOF, on non-UTF-8 and on lines longer than `cap`.
pub fn read_line_raw(
    mut stream: &TcpStream,
    cap: usize,
    deadline: Instant,
) -> std::io::Result<String> {
    let mut buf = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
        stream.set_read_timeout(Some(remaining))?;
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(_) if byte[0] == b'\n' => {
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                return String::from_utf8(buf).map_err(|_| invalid("line is not UTF-8"));
            }
            Ok(_) if buf.len() >= cap => return Err(invalid("line too long")),
            Ok(_) => buf.push(byte[0]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted || is_timeout(&e) => {}
            Err(e) => return Err(e),
        }
    }
}

/// Buffered line reader for long-lived connections polled with a short read
/// timeout: a partial line survives a timeout and completes on a later poll.
#[derive(Default)]
pub struct LineReader {
    buf: Vec<u8>,
}

impl LineReader {
    /// Ok(Some) a line, Ok(None) nothing complete before the socket's read
    /// timeout, Err on EOF, errors and lines longer than `cap`.
    pub fn poll(&mut self, mut stream: &TcpStream, cap: usize) -> std::io::Result<Option<String>> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.len() > cap {
                    return Err(invalid("line too long"));
                }
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| invalid("line is not UTF-8"));
            }
            if self.buf.len() > cap {
                return Err(invalid("line too long"));
            }
            let mut chunk = [0u8; 512];
            match stream.read(&mut chunk) {
                Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if is_timeout(&e) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }
}

/// 128 unpredictable bits as hex, from std alone: SipHash keyed by the
/// per-process random keys std draws from the operating system for HashMap.
/// Only used for attach ids, which live for seconds.
pub fn random_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (0..2u8)
        .map(|half| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(n);
            h.write_u8(half);
            h.write_u128(nanos);
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// A host's control connection. Writers serialize on `write`; shutdown needs
// no lock, so a stuck writer never blocks closing the connection.
struct Control {
    stream: TcpStream,
    write: Mutex<()>,
}

impl Control {
    fn send(&self, line: &str) -> std::io::Result<()> {
        let _w = lock(&self.write);
        write_line(&self.stream, line)
    }
}

type Attached = (TcpStream, ConnGuard);

struct Room {
    proto: String,
    secret: String,
    control: Option<Arc<Control>>,
    gen: u64, // which control connection owns the room
    offline_since: Option<Instant>,
    pending: HashMap<String, mpsc::SyncSender<Attached>>,
    active: Arc<AtomicUsize>,
}

#[derive(Default)]
struct IpState {
    conns: usize,
    failures: u32,
    last_failure: Option<Instant>,
    banned_until: Option<Instant>,
}

struct State {
    cfg: Config,
    rooms: Mutex<HashMap<String, Room>>,
    ips: Mutex<HashMap<IpAddr, IpState>>,
    conns: AtomicUsize,
    next_gen: AtomicU64,
    stop: AtomicBool,
}

impl State {
    fn log(&self, msg: std::fmt::Arguments) {
        if self.cfg.log {
            eprintln!("dice-wars-relay: {msg}");
        }
    }

    fn sweep(&self, rooms: &mut HashMap<String, Room>) {
        let grace = self.cfg.grace;
        rooms.retain(|code, r| {
            let keep = r.offline_since.is_none_or(|t| t.elapsed() < grace);
            if !keep && self.cfg.log {
                eprintln!("dice-wars-relay: room {code} closed (host did not come back)");
            }
            keep
        });
    }

    fn admit(self: &Arc<Self>, ip: IpAddr) -> Result<ConnGuard, &'static str> {
        let mut ips = lock(&self.ips);
        let now = Instant::now();
        if ips.len() >= MAX_TRACKED_IPS && !ips.contains_key(&ip) {
            ips.retain(|_, s| s.conns > 0 || s.banned_until.is_some_and(|t| t > now));
            if ips.len() >= MAX_TRACKED_IPS {
                return Err("the relay is busy - try again later");
            }
        }
        let s = ips.entry(ip).or_default();
        if s.banned_until.is_some_and(|t| t > now) {
            return Err("too many failed attempts - try again later");
        }
        if self.conns.load(Ordering::Relaxed) >= self.cfg.max_conns {
            return Err("the relay is busy - try again later");
        }
        if s.conns >= self.cfg.max_conns_per_ip {
            return Err("too many connections from your address");
        }
        s.conns += 1;
        self.conns.fetch_add(1, Ordering::Relaxed);
        Ok(ConnGuard {
            state: self.clone(),
            ip,
        })
    }

    fn failure(&self, ip: IpAddr) {
        let mut ips = lock(&self.ips);
        let s = ips.entry(ip).or_default();
        let now = Instant::now();
        if s.last_failure
            .is_some_and(|t| now.duration_since(t) > self.cfg.ban_time)
        {
            s.failures = 0;
        }
        s.failures += 1;
        s.last_failure = Some(now);
        if s.failures >= self.cfg.max_failures {
            s.failures = 0;
            s.banned_until = Some(now + self.cfg.ban_time);
            drop(ips);
            self.log(format_args!("banned {ip} for repeated failed requests"));
        }
    }
}

// Counts one open connection against the global and per-IP limits.
struct ConnGuard {
    state: Arc<State>,
    ip: IpAddr,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.state.conns.fetch_sub(1, Ordering::Relaxed);
        let mut ips = lock(&self.state.ips);
        if let Some(s) = ips.get_mut(&self.ip) {
            s.conns = s.conns.saturating_sub(1);
            if s.conns == 0 && s.failures == 0 && s.banned_until.is_none() {
                ips.remove(&self.ip);
            }
        }
    }
}

struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A relay running on background threads; dropping it stops accepting.
pub struct Relay {
    state: Arc<State>,
    addr: SocketAddr,
}

impl Relay {
    pub fn start(listener: TcpListener, cfg: Config) -> std::io::Result<Relay> {
        let addr = listener.local_addr()?;
        let state = new_state(cfg);
        let st = state.clone();
        std::thread::spawn(move || accept_loop(listener, st));
        Ok(Relay { state, addr })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Rooms currently registered, including those within the grace period.
    pub fn rooms(&self) -> usize {
        lock(&self.state.rooms).len()
    }

    /// Cut every host control connection (rooms stay, as after a network
    /// blip), to exercise the hosts' reconnect path.
    pub fn drop_controls(&self) {
        for room in lock(&self.state.rooms).values() {
            if let Some(c) = &room.control {
                let _ = c.stream.shutdown(Shutdown::Both);
            }
        }
    }

    pub fn stop(&self) {
        if self.state.stop.swap(true, Ordering::Relaxed) {
            return;
        }
        self.drop_controls();
        // wake the blocking accept
        let mut wake = self.addr;
        if wake.ip().is_unspecified() {
            wake.set_ip(match wake {
                SocketAddr::V4(_) => IpAddr::from([127, 0, 0, 1]),
                SocketAddr::V6(_) => IpAddr::from([0u16, 0, 0, 0, 0, 0, 0, 1]),
            });
        }
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Run the relay on the calling thread until the process ends.
pub fn serve(listener: TcpListener, cfg: Config) {
    accept_loop(listener, new_state(cfg));
}

fn new_state(cfg: Config) -> Arc<State> {
    Arc::new(State {
        cfg,
        rooms: Mutex::new(HashMap::new()),
        ips: Mutex::new(HashMap::new()),
        conns: AtomicUsize::new(0),
        next_gen: AtomicU64::new(1),
        stop: AtomicBool::new(false),
    })
}

fn accept_loop(listener: TcpListener, state: Arc<State>) {
    let _ = listener.set_nonblocking(false);
    loop {
        let accepted = listener.accept();
        if state.stop.load(Ordering::Relaxed) {
            break;
        }
        match accepted {
            Ok((stream, peer)) => {
                let ip = peer.ip();
                let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                match state.admit(ip) {
                    Ok(guard) => {
                        let st = state.clone();
                        let spawned = std::thread::Builder::new()
                            .spawn(move || handle_conn(stream, ip, guard, st));
                        if spawned.is_err() {
                            state.log(format_args!("could not start a thread for {ip}"));
                        }
                    }
                    Err(why) => refuse(stream, why),
                }
            }
            // out of file descriptors and the like: back off briefly
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

// Tell a refused client why, then close gracefully. Closing a socket whose
// request is still unread makes Windows (and sometimes others) send a reset
// that destroys the BYE line before the client reads it, so the request is
// drained first, briefly, on a short-lived thread.
fn refuse(stream: TcpStream, why: &str) {
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = write_line(&stream, &format!("BYE {why}"));
    let _ = stream.shutdown(Shutdown::Write);
    let _ = std::thread::Builder::new().spawn(move || {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
        let mut buf = [0u8; 512];
        let mut total = 0;
        while total < 4096 {
            match std::io::Read::read(&mut &stream, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n,
            }
        }
    });
}

fn handle_conn(stream: TcpStream, ip: IpAddr, guard: ConnGuard, state: Arc<State>) {
    let _ = stream.set_nodelay(true);
    let deadline = Instant::now() + state.cfg.header_timeout;
    // silence or a dropped connection is not worth a strike; the header
    // timeout and the per-IP cap bound what it costs
    let Ok(line) = read_line_raw(&stream, RELAY_MAX_LINE, deadline) else {
        return;
    };
    match parse_request(&line) {
        Some(Request::Host {
            proto,
            code,
            secret,
        }) => host_session(stream, ip, proto, code, secret, &state),
        Some(Request::Join { proto, code }) => {
            guest_session(stream, ip, guard, proto, code, &state)
        }
        Some(Request::Attach { code, id }) => attach(stream, ip, guard, code, id, &state),
        None => {
            state.failure(ip);
            let _ = write_line(&stream, "BYE bad request");
        }
    }
}

fn host_session(
    stream: TcpStream,
    ip: IpAddr,
    proto: String,
    code: String,
    secret: String,
    state: &Arc<State>,
) {
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let control = Arc::new(Control {
        stream: clone,
        write: Mutex::new(()),
    });
    let gen = state.next_gen.fetch_add(1, Ordering::Relaxed);
    let (refused, replaced) = {
        let mut rooms = lock(&state.rooms);
        state.sweep(&mut rooms);
        let full = rooms.len() >= state.cfg.max_rooms;
        match rooms.get_mut(&code) {
            Some(room) if room.secret != secret => {
                (Some("room code already in use on this relay"), None)
            }
            Some(room) => {
                room.proto = proto;
                room.gen = gen;
                room.offline_since = None;
                (None, room.control.replace(control.clone()))
            }
            None if full => (Some("the relay is full - try again later"), None),
            None => {
                rooms.insert(
                    code.clone(),
                    Room {
                        proto,
                        secret,
                        control: Some(control.clone()),
                        gen,
                        offline_since: None,
                        pending: HashMap::new(),
                        active: Arc::new(AtomicUsize::new(0)),
                    },
                );
                (None, None)
            }
        }
    };
    if let Some(why) = refused {
        state.log(format_args!("room {code}: refused host {ip}: {why}"));
        let _ = write_line(&stream, &format!("RELAY-ERR {why}"));
        return;
    }
    if let Some(old) = &replaced {
        let _ = old.stream.shutdown(Shutdown::Both);
    }
    let what = if replaced.is_some() {
        "reclaimed"
    } else {
        "opened"
    };
    state.log(format_args!("room {code}: {what} by host {ip}"));

    let mut bye = false;
    if control.send("RELAY-OK").is_ok()
        && stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .is_ok()
    {
        let mut reader = LineReader::default();
        let mut last = Instant::now();
        while !state.stop.load(Ordering::Relaxed) {
            match reader.poll(&stream, CONTROL_MAX_LINE) {
                Ok(Some(l)) => {
                    last = Instant::now();
                    match l.trim() {
                        "PING" => {
                            if control.send("PONG").is_err() {
                                break;
                            }
                        }
                        "BYE" => {
                            bye = true;
                            break;
                        }
                        _ => {}
                    }
                }
                Ok(None) if last.elapsed() < state.cfg.control_idle => {}
                Ok(None) | Err(_) => break,
            }
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    let mut rooms = lock(&state.rooms);
    if let Some(room) = rooms.get_mut(&code).filter(|r| r.gen == gen) {
        if bye {
            rooms.remove(&code);
            drop(rooms);
            state.log(format_args!("room {code}: closed by the host"));
        } else {
            room.control = None;
            room.offline_since = Some(Instant::now());
            drop(rooms);
            state.log(format_args!(
                "room {code}: host connection lost, holding the room"
            ));
        }
    }
}

fn guest_session(
    stream: TcpStream,
    ip: IpAddr,
    guard: ConnGuard,
    proto: String,
    code: String,
    state: &Arc<State>,
) {
    let id = random_id();
    let (tx, rx) = mpsc::sync_channel::<Attached>(1);
    let reply = {
        let mut rooms = lock(&state.rooms);
        state.sweep(&mut rooms);
        match rooms.get_mut(&code) {
            None => Err("BYE no game with that code on this relay"),
            Some(room) if room.proto != proto => {
                Err("BYE version mismatch - download the latest release")
            }
            Some(room) => match room.control.clone() {
                None => Err("RELAY-WAIT the host is reconnecting to the relay"),
                Some(_)
                    if room.pending.len() + room.active.load(Ordering::Relaxed)
                        >= state.cfg.max_guests_per_room =>
                {
                    Err("BYE the room is full")
                }
                Some(control) => {
                    room.pending.insert(id.clone(), tx);
                    Ok((control, room.active.clone()))
                }
            },
        }
    };
    let (control, active) = match reply {
        Ok(found) => found,
        Err(line) => {
            if line.starts_with("BYE no game") {
                state.failure(ip);
            }
            state.log(format_args!("room {code}: guest {ip} turned away: {line}"));
            let _ = write_line(&stream, line);
            return;
        }
    };
    let answer = if control.send(&format!("CONN {id} {ip}")).is_ok() {
        rx.recv_timeout(state.cfg.attach_timeout)
    } else {
        Err(mpsc::RecvTimeoutError::Timeout)
    };
    if let Some(room) = lock(&state.rooms).get_mut(&code) {
        room.pending.remove(&id);
    }
    let (host, host_guard) = match answer {
        Ok(attached) => attached,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = write_line(&stream, "BYE The host left the game");
            return;
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = write_line(&stream, "RELAY-WAIT the host did not answer");
            return;
        }
    };
    active.fetch_add(1, Ordering::Relaxed);
    let _active = ActiveGuard(active);
    if write_line(&host, "RELAY-OK").is_err() || write_line(&stream, "RELAY-OK").is_err() {
        return;
    }
    state.log(format_args!("room {code}: guest {ip} connected"));
    splice(stream, host, state.cfg.idle_timeout);
    state.log(format_args!("room {code}: guest {ip} disconnected"));
    drop(host_guard);
    drop(guard);
}

fn attach(
    stream: TcpStream,
    ip: IpAddr,
    guard: ConnGuard,
    code: String,
    id: String,
    state: &Arc<State>,
) {
    let waiting = lock(&state.rooms)
        .get_mut(&code)
        .and_then(|r| r.pending.remove(&id));
    match waiting {
        // the guest thread answers both sides once it has the socket; if it
        // already gave up, dropping the socket closes it
        Some(tx) => drop(tx.try_send((stream, guard))),
        None => {
            state.failure(ip);
            let _ = write_line(&stream, "BYE unknown or expired connection");
        }
    }
}

// Copy bytes both ways until either side closes or nothing moves for `idle`.
fn splice(a: TcpStream, b: TcpStream, idle: Duration) {
    let (Ok(a2), Ok(b2)) = (a.try_clone(), b.try_clone()) else {
        return;
    };
    let base = Instant::now();
    let last = Arc::new(AtomicU64::new(0));
    for s in [&a, &b] {
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = s.set_write_timeout(Some(SPLICE_WRITE_TIMEOUT));
        let _ = s.set_nodelay(true);
    }
    let last2 = last.clone();
    let other = std::thread::Builder::new().spawn(move || pump(&b2, &a2, base, &last2, idle));
    pump(&a, &b, base, &last, idle);
    if let Ok(t) = other {
        let _ = t.join();
    }
}

fn pump(mut from: &TcpStream, mut to: &TcpStream, base: Instant, last: &AtomicU64, idle: Duration) {
    let now_ms = || base.elapsed().as_millis() as u64;
    let mut buf = [0u8; 8192];
    loop {
        match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
                last.store(now_ms(), Ordering::Relaxed);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if is_timeout(&e) => {
                if now_ms().saturating_sub(last.load(Ordering::Relaxed)) > idle.as_millis() as u64 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    // closing both ends also ends the pump running the other way
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    fn relay(cfg: Config) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        Relay::start(listener, Config { log: false, ..cfg }).unwrap()
    }

    fn connect(r: &Relay) -> TcpStream {
        TcpStream::connect(r.local_addr()).unwrap()
    }

    fn ask(r: &Relay, line: &str) -> (TcpStream, String) {
        let s = connect(r);
        write_line(&s, line).unwrap();
        let reply = read_line_raw(&s, 200, Instant::now() + Duration::from_secs(5))
            .unwrap_or_else(|e| format!("<{e}>"));
        (s, reply)
    }

    fn read(s: &TcpStream) -> String {
        read_line_raw(s, 200, Instant::now() + Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn requests_parse_strictly() {
        assert_eq!(
            parse_request(&format!("RELAY-HOST 8 123456 {SECRET}")),
            Some(Request::Host {
                proto: "8".into(),
                code: "123456".into(),
                secret: SECRET.into()
            })
        );
        assert_eq!(
            parse_request("RELAY-JOIN 8 123456"),
            Some(Request::Join {
                proto: "8".into(),
                code: "123456".into()
            })
        );
        assert_eq!(
            parse_request(&format!("RELAY-ATTACH 123456 {SECRET}")),
            Some(Request::Attach {
                code: "123456".into(),
                id: SECRET.into()
            })
        );
        for bad in [
            "",
            "RELAY-JOIN 8",
            "RELAY-JOIN 8 123456 extra",
            "RELAY-JOIN 8 12/456",
            "RELAY-JOIN 8 12345678901234567",
            "RELAY-HOST 8 123456 short",
            "RELAY-HOST 8 123456 zz23456789abcdef0123456789abcdef",
            "RELAY-ATTACH 123456 0123",
            "relay-join 8 123456",
            "JOIN 123456 8 name",
        ] {
            assert_eq!(parse_request(bad), None, "{bad:?}");
        }
        assert!(!valid_proto("8 9"));
        assert!(valid_id(&random_id()));
        assert_ne!(random_id(), random_id());
    }

    #[test]
    fn addresses_get_the_default_port() {
        assert_eq!(with_default_port("example.org", 7780), "example.org:7780");
        assert_eq!(with_default_port(" example.org:9 ", 7780), "example.org:9");
        assert_eq!(with_default_port("1.2.3.4", 7780), "1.2.3.4:7780");
        assert_eq!(with_default_port("::1", 7780), "[::1]:7780");
        assert_eq!(with_default_port("[::1]", 7780), "[::1]:7780");
        assert_eq!(with_default_port("[::1]:9", 7780), "[::1]:9");
    }

    #[test]
    fn line_readers_respect_caps_deadlines_and_partial_lines() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (s, _) = listener.accept().unwrap();

        // the raw reader leaves the bytes after the newline in the socket
        peer.write_all(b"RELAY-OK\r\nWELCOME").unwrap();
        assert_eq!(read(&s), "RELAY-OK");
        let mut rest = [0u8; 7];
        (&s).read_exact(&mut rest).unwrap();
        assert_eq!(&rest, b"WELCOME");

        let started = Instant::now();
        let r = read_line_raw(&s, 10, Instant::now() + Duration::from_millis(50));
        assert!(r.is_err() && started.elapsed() < Duration::from_secs(1));
        peer.write_all(b"0123456789ABC\n").unwrap();
        assert!(read_line_raw(&s, 10, Instant::now() + Duration::from_secs(1)).is_err());

        let (s, peer) = {
            let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (s, _) = listener.accept().unwrap();
            peer.write_all(b"PI").unwrap();
            (s, peer)
        };
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut lr = LineReader::default();
        assert_eq!(lr.poll(&s, 8).unwrap(), None);
        (&peer).write_all(b"NG\nPONG\n").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(lr.poll(&s, 8).unwrap().as_deref(), Some("PING"));
        assert_eq!(lr.poll(&s, 8).unwrap().as_deref(), Some("PONG"));
        (&peer).write_all(b"0123456789\n").unwrap();
        assert!(lr.poll(&s, 8).is_err());
    }

    #[test]
    fn guests_are_spliced_to_the_host_byte_for_byte() {
        let r = relay(Config::default());
        let (control, ok) = ask(&r, &format!("RELAY-HOST 8 424242 {SECRET}"));
        assert_eq!(ok, "RELAY-OK");

        let guest = connect(&r);
        write_line(&guest, "RELAY-JOIN 8 424242").unwrap();
        let conn = read(&control);
        let mut f = conn.split_whitespace();
        assert_eq!(f.next(), Some("CONN"));
        let id = f.next().unwrap().to_string();
        assert_eq!(f.next(), Some("127.0.0.1"));

        let (host, ok) = ask(&r, &format!("RELAY-ATTACH 424242 {id}"));
        assert_eq!(ok, "RELAY-OK");
        assert_eq!(read(&guest), "RELAY-OK");
        write_line(&guest, "JOIN 424242 8 Ada").unwrap();
        assert_eq!(read(&host), "JOIN 424242 8 Ada");
        write_line(&host, "WELCOME 1 token").unwrap();
        assert_eq!(read(&guest), "WELCOME 1 token");

        // the attach id is single use
        let (_s, bye) = ask(&r, &format!("RELAY-ATTACH 424242 {id}"));
        assert_eq!(bye, "BYE unknown or expired connection");

        // closing one side closes the other
        host.shutdown(Shutdown::Both).unwrap();
        assert!(read_line_raw(&guest, 10, Instant::now() + Duration::from_secs(5)).is_err());

        // keepalive, then a clean close removes the room
        write_line(&control, "PING").unwrap();
        assert_eq!(read(&control), "PONG");
        write_line(&control, "BYE").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while r.rooms() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(r.rooms(), 0);
    }

    #[test]
    fn unknown_rooms_versions_collisions_and_full_rooms_are_refused() {
        let r = relay(Config {
            max_rooms: 2,
            max_guests_per_room: 1,
            attach_timeout: Duration::from_millis(300),
            ..Config::default()
        });
        let (_c1, ok) = ask(&r, &format!("RELAY-HOST 8 111111 {SECRET}"));
        assert_eq!(ok, "RELAY-OK");
        let other = "ffffffffffffffffffffffffffffffff";
        let (_c, err) = ask(&r, &format!("RELAY-HOST 8 111111 {other}"));
        assert_eq!(err, "RELAY-ERR room code already in use on this relay");
        let (_c2, ok) = ask(&r, &format!("RELAY-HOST 8 222222 {SECRET}"));
        assert_eq!(ok, "RELAY-OK");
        let (_c, err) = ask(&r, &format!("RELAY-HOST 8 333333 {SECRET}"));
        assert_eq!(err, "RELAY-ERR the relay is full - try again later");

        let (_g, bye) = ask(&r, "RELAY-JOIN 8 999999");
        assert_eq!(bye, "BYE no game with that code on this relay");
        let (_g, bye) = ask(&r, "RELAY-JOIN 7 111111");
        assert_eq!(bye, "BYE version mismatch - download the latest release");
        let (_g, bye) = ask(&r, "HELLO");
        assert_eq!(bye, "BYE bad request");

        // one guest waits for an attach that never comes; a second is turned away
        let waiting = connect(&r);
        write_line(&waiting, "RELAY-JOIN 8 111111").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let (_g, bye) = ask(&r, "RELAY-JOIN 8 111111");
        assert_eq!(bye, "BYE the room is full");
        assert_eq!(read(&waiting), "RELAY-WAIT the host did not answer");
    }

    #[test]
    fn a_dropped_host_can_reclaim_its_room_within_the_grace_period() {
        let r = relay(Config {
            grace: Duration::from_millis(400),
            ..Config::default()
        });
        let (c, ok) = ask(&r, &format!("RELAY-HOST 8 555555 {SECRET}"));
        assert_eq!(ok, "RELAY-OK");
        drop(c);
        std::thread::sleep(Duration::from_millis(100));
        let (_g, wait) = ask(&r, "RELAY-JOIN 8 555555");
        assert_eq!(wait, "RELAY-WAIT the host is reconnecting to the relay");
        let (_c, ok) = ask(&r, &format!("RELAY-HOST 8 555555 {SECRET}"));
        assert_eq!(ok, "RELAY-OK");

        // a host that never comes back loses the room
        drop(_c);
        std::thread::sleep(Duration::from_millis(600));
        let (_g, bye) = ask(&r, "RELAY-JOIN 8 555555");
        assert_eq!(bye, "BYE no game with that code on this relay");
    }

    #[test]
    fn connection_caps_and_bans_apply_per_address() {
        let r = relay(Config {
            max_conns_per_ip: 2,
            max_failures: 3,
            header_timeout: Duration::from_millis(300),
            ..Config::default()
        });
        let a = connect(&r);
        let b = connect(&r);
        std::thread::sleep(Duration::from_millis(50));
        let (_c, bye) = ask(&r, "RELAY-JOIN 8 123456");
        assert_eq!(bye, "BYE too many connections from your address");
        // a silent connection is closed at the header deadline
        assert!(read_line_raw(&a, 10, Instant::now() + Duration::from_secs(5)).is_err());
        drop((a, b));
        std::thread::sleep(Duration::from_millis(50));

        for _ in 0..3 {
            let (_g, bye) = ask(&r, "RELAY-JOIN 8 000000");
            assert_eq!(bye, "BYE no game with that code on this relay");
        }
        let (_g, bye) = ask(&r, "RELAY-JOIN 8 000000");
        assert_eq!(bye, "BYE too many failed attempts - try again later");
    }
}
