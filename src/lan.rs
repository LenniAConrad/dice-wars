//! LAN game discovery.
//!
//! An open host lobby announces itself once a second with a UDP broadcast on
//! the game port + 1 (7778 by default). The join screen lists every lobby it
//! hears, so players on the same network can join with one click. The beacon
//! carries the room code: anyone on the local network may join an open lobby.

use crate::game::MAX_PLAYERS;
use crate::net::{net_port, NET_PROTO, NET_ROOM_CODE_LEN};
use crate::settings::clean_name;

const LAN_MAGIC: &str = "DICEWARS-LAN";

const LAN_TTL: f32 = 3.5; // seconds without a beacon before a game disappears

pub(crate) const LAN_MAX_GAMES: usize = 4;

fn lan_port() -> u16 {
    net_port().wrapping_add(1)
}

pub(crate) fn lan_beacon_line(
    port: u16,
    code: &str,
    humans: usize,
    players: usize,
    name: &str,
) -> String {
    format!(
        "{LAN_MAGIC} {NET_PROTO} {port} {code} {humans} {players} {}",
        clean_name(name)
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LanGame {
    pub(crate) addr: String,
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) humans: usize,
    pub(crate) players: usize,
    pub(crate) age: f32,
}

// Strict parser: anything malformed, oversized, or from another protocol
// version is ignored rather than shown.
fn parse_lan_beacon(line: &str, from: std::net::SocketAddr) -> Option<LanGame> {
    if line.len() > 96 {
        return None;
    }
    let mut it = line.split(' ');
    if it.next()? != LAN_MAGIC || it.next()? != NET_PROTO {
        return None;
    }
    let port: u16 = it.next()?.parse().ok().filter(|&p| p != 0)?;
    let code = it.next()?;
    if code.len() != NET_ROOM_CODE_LEN || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let humans: usize = it.next()?.parse().ok()?;
    let players: usize = it.next()?.parse().ok()?;
    if !(1..=MAX_PLAYERS).contains(&players) || humans == 0 || humans > players {
        return None;
    }
    let name = clean_name(it.next().unwrap_or(""));
    if it.next().is_some() {
        return None;
    }
    let ip = match from.ip() {
        std::net::IpAddr::V4(v4) => v4,
        std::net::IpAddr::V6(_) => return None,
    };
    Some(LanGame {
        addr: format!("{ip}:{port}"),
        code: code.to_string(),
        name: if name.is_empty() {
            "Host".to_string()
        } else {
            name
        },
        humans,
        players,
        age: 0.0,
    })
}

pub(crate) struct LanBeacon {
    pub(crate) socket: Option<std::net::UdpSocket>,
    pub(crate) t: f32,
}

impl LanBeacon {
    pub(crate) fn new() -> LanBeacon {
        let socket = std::net::UdpSocket::bind(("0.0.0.0", 0))
            .ok()
            .filter(|s| s.set_broadcast(true).is_ok() && s.set_nonblocking(true).is_ok());
        LanBeacon { socket, t: 1.0 }
    }
    pub(crate) fn tick(&mut self, dt: f32, line: impl FnOnce() -> String) {
        self.t += dt;
        if self.t < 1.0 {
            return;
        }
        self.t = 0.0;
        if let Some(s) = &self.socket {
            let line = line();
            // Broadcast to the network, and to this machine for a second copy
            // of the game running locally.
            let _ = s.send_to(line.as_bytes(), (std::net::Ipv4Addr::BROADCAST, lan_port()));
            let _ = s.send_to(line.as_bytes(), (std::net::Ipv4Addr::LOCALHOST, lan_port()));
        }
    }
}

pub(crate) struct LanScan {
    pub(crate) socket: Option<std::net::UdpSocket>,
    pub(crate) games: Vec<LanGame>,
}

impl LanScan {
    pub(crate) fn new() -> LanScan {
        let socket = std::net::UdpSocket::bind(("0.0.0.0", lan_port()))
            .ok()
            .filter(|s| s.set_nonblocking(true).is_ok());
        LanScan {
            socket,
            games: Vec::new(),
        }
    }
    pub(crate) fn listening(&self) -> bool {
        self.socket.is_some()
    }
    pub(crate) fn tick(&mut self, dt: f32) {
        for g in &mut self.games {
            g.age += dt;
        }
        self.games.retain(|g| g.age < LAN_TTL);
        let Some(socket) = &self.socket else { return };
        let mut buf = [0u8; 128];
        // Bounded per frame so a flood cannot stall rendering.
        for _ in 0..32 {
            let Ok((n, from)) = socket.recv_from(&mut buf) else {
                break;
            };
            let Some(game) = std::str::from_utf8(&buf[..n])
                .ok()
                .and_then(|line| parse_lan_beacon(line.trim_end(), from))
            else {
                continue;
            };
            if let Some(known) = self.games.iter_mut().find(|g| g.addr == game.addr) {
                *known = game;
            } else if self.games.len() < LAN_MAX_GAMES {
                self.games.push(game);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_beacons_round_trip_and_reject_malformed_input() {
        let from: std::net::SocketAddr = "192.168.1.20:50000".parse().unwrap();
        let line = lan_beacon_line(7777, "123456", 2, 6, "Lenni");
        let game = parse_lan_beacon(&line, from).expect("valid beacon");
        assert_eq!(game.addr, "192.168.1.20:7777");
        assert_eq!(
            (game.code.as_str(), game.humans, game.players),
            ("123456", 2, 6)
        );
        assert_eq!(game.name, "Lenni");
        for bad in [
            "",
            "DICEWARS-LAN",
            &line.replace("123456", "12345"),
            &line.replace("123456", "12345x"),
            &line.replace(" 2 6 ", " 7 6 "),
            &line.replace(" 2 6 ", " 2 99 "),
            &line.replace(" 7777 ", " 0 "),
            &format!("{line} extra"),
            &line.replace(&format!(" {NET_PROTO} "), " 0 "),
            &"x".repeat(200),
        ] {
            assert!(parse_lan_beacon(bad, from).is_none(), "accepted {bad:?}");
        }
        let v6: std::net::SocketAddr = "[::1]:50000".parse().unwrap();
        assert!(parse_lan_beacon(&line, v6).is_none());
    }
}
