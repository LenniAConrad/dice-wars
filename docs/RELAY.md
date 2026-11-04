# Internet relay

Network games normally need the guests to reach the host's TCP port 7777,
which over the internet means port forwarding or a VPN. A relay removes that:
the host and every guest connect *out* to a small server that both can reach,
and the relay passes the game traffic between them. Anyone can run one; it is
the `dice-wars-relay` program in this repository.

## Security first

- **Game traffic is not encrypted.** The relay (and anyone on the network path)
  can read room codes, names and every move, and whoever runs the relay could
  tamper with the game.
- The room code is the only key to a lobby. The relay limits guessing (it bans
  an address for 10 minutes after 20 failed requests), but treat the code like a
  short-lived password: share it only with the people you invite.
- **Run your own relay**, or use one run by someone you trust. It costs very
  little: one small VPS handles hundreds of games.

## How it works

1. The host opens a control connection and registers its room code:
   `RELAY-HOST <protocol> <code> <secret>`.
2. A guest connects with `RELAY-JOIN <protocol> <code>`. The relay tells the
   host `CONN <id> <guest ip>` over the control connection.
3. The host opens a new connection, `RELAY-ATTACH <code> <id>`, and the relay
   joins it to the guest's socket byte for byte.
4. From then on host and guest speak the normal game protocol, exactly as on
   a direct connection, including reconnecting after a dropped link.

If the host's control connection drops, the relay keeps the room for 90
seconds so the host can re-register it (the secret proves it is the same
host); games already in progress are not affected. The full line protocol is
documented at the top of [`src/relay.rs`](../src/relay.rs).

## Running a relay on a VPS

Any Linux server with a public IP address works. The relay has no
dependencies beyond the Rust standard library and needs no desktop libraries.

### Build

```sh
# on the server (or build elsewhere for the same platform and copy the binary)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if Rust is missing
git clone https://github.com/LenniAConrad/dice-wars.git
cd dice-wars
cargo build --release --locked --bin dice-wars-relay
sudo install -m 755 target/release/dice-wars-relay /usr/local/bin/
```

Try it in the foreground first:

```sh
dice-wars-relay                 # listens on 0.0.0.0:7780
dice-wars-relay --help          # limits and options
```

### Open the firewall

The relay uses one TCP port, 7780 by default:

```sh
sudo ufw allow 7780/tcp                                           # Ubuntu / Debian with ufw
sudo firewall-cmd --permanent --add-port=7780/tcp && sudo firewall-cmd --reload   # Fedora / RHEL
```

Cloud providers usually have a separate firewall ("security group") in their
web console; allow inbound TCP 7780 there too.

### systemd service

`/etc/systemd/system/dice-wars-relay.service`:

```ini
[Unit]
Description=Dice Wars relay
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/dice-wars-relay --listen 0.0.0.0:7780
Restart=always
RestartSec=2
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
LimitNOFILE=4096

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now dice-wars-relay
journalctl -u dice-wars-relay -f        # one log line per room and guest
```

The log names room codes and IP addresses; pass `--quiet` to turn it off.

### Limits

| Option | Default | Meaning |
| --- | --- | --- |
| `--listen ADDR` | `0.0.0.0:7780` | address and port to listen on |
| `--max-rooms N` | 500 | rooms open at once |
| `--max-guests N` | 16 | guests per room (waiting plus connected) |
| `--max-conns N` | 1024 | open connections in total |
| `--max-conns-per-ip N` | 32 | open connections from one address |
| `--idle-minutes N` | 60 | close a guest link with no traffic either way |
| `--grace-seconds N` | 90 | keep a room while its host reconnects |

Fixed limits: request lines are at most 160 bytes and must arrive within 10
seconds; a host must attach within 10 seconds of being told about a guest; a
control connection silent for 60 seconds is closed (hosts ping every 15).
Each connected guest uses two connections (its own and the host's) and two
threads, so raise `LimitNOFILE` together with `--max-conns`.

## Playing through a relay

The host enables the relay with the relay's address (`relay.example.org` or
`relay.example.org:7780`); guests join with the same relay address and the
host's 6-digit room code. The address is saved in the settings file as
`relay <address>`. In code: `HostNet::enable_relay`, `HostNet::relay_status`,
and `spawn_guest_connect_via_relay` (or a guest address written as
`relay://relay.example.org:7780`).

## Testing

`scripts/check.sh` runs the relay's unit tests and an in-process game over a
relay on 127.0.0.1 (two guests, the lobby handshake, a reconnect and a host
re-registering). To try the binary by hand on one machine:
`dice-wars-relay --listen 127.0.0.1:7780`.
