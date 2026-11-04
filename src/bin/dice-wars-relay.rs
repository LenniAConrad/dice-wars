//! dice-wars-relay: lets friends play Dice Wars over the internet without
//! port forwarding. Run it on any machine both players can reach (a small
//! VPS is plenty); see docs/RELAY.md.
//!
//! Standard library only and independent of the game's window/audio engine,
//! so it builds and runs on a headless server.

// The relay module also carries the game's client helpers, unused here.
#[allow(dead_code)]
#[path = "../relay.rs"]
mod relay;

use std::time::Duration;

const USAGE: &str = "\
dice-wars-relay - internet relay for Dice Wars network games

USAGE: dice-wars-relay [OPTIONS]

OPTIONS:
  --listen ADDR          address and port to listen on   [default: 0.0.0.0:7780]
                         (use [::]:7780 for IPv6 as well, where the OS allows)
  --max-rooms N          rooms open at once               [default: 500]
  --max-guests N         guests per room                  [default: 16]
  --max-conns N          open connections in total        [default: 1024]
  --max-conns-per-ip N   open connections per address     [default: 32]
  --idle-minutes N       close guest links silent this long [default: 60]
  --grace-seconds N      keep a room while its host reconnects [default: 90]
  --quiet                no log lines on stderr
  -h, --help             this text
  -V, --version          print the version

Game traffic is relayed unencrypted; the relay sees room codes and moves.";

fn main() {
    let mut cfg = relay::Config::default();
    let mut listen = format!("0.0.0.0:{}", relay::RELAY_PORT_DEFAULT);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next()
                .unwrap_or_else(|| fail(&format!("{name} needs a value")))
        };
        let number = |name: &str, v: String| -> u64 {
            match v.parse::<u64>() {
                Ok(n) if n > 0 => n,
                _ => fail(&format!("{name} wants a positive whole number, not {v:?}")),
            }
        };
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "-V" | "--version" => {
                println!("dice-wars-relay {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--quiet" => cfg.log = false,
            "--listen" => listen = value("--listen"),
            "--max-rooms" => cfg.max_rooms = number(&arg, value(&arg)) as usize,
            "--max-guests" => cfg.max_guests_per_room = number(&arg, value(&arg)) as usize,
            "--max-conns" => cfg.max_conns = number(&arg, value(&arg)) as usize,
            "--max-conns-per-ip" => cfg.max_conns_per_ip = number(&arg, value(&arg)) as usize,
            "--idle-minutes" => {
                cfg.idle_timeout = Duration::from_secs(number(&arg, value(&arg)) * 60)
            }
            "--grace-seconds" => cfg.grace = Duration::from_secs(number(&arg, value(&arg))),
            _ => fail(&format!("unknown option {arg:?} (see --help)")),
        }
    }
    let listener = match std::net::TcpListener::bind(&listen) {
        Ok(l) => l,
        Err(e) => fail(&format!("cannot listen on {listen}: {e}")),
    };
    if cfg.log {
        let at = listener
            .local_addr()
            .map_or(listen.clone(), |a| a.to_string());
        eprintln!(
            "dice-wars-relay {}: listening on {at}",
            env!("CARGO_PKG_VERSION")
        );
    }
    relay::serve(listener, cfg);
}

fn fail(msg: &str) -> ! {
    eprintln!("dice-wars-relay: {msg}");
    std::process::exit(2);
}
