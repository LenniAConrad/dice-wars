# Dice Wars v1.0.2

Multiplayer resilience release. Both players should update.

## New

- **Automatic reconnection.** If a player's connection drops mid-game, the host
  holds their seat for 45 seconds ("P2 lost connection — waiting...") while their
  game quietly reconnects, re-authenticates with a per-player token, replays the
  missed moves, and resumes — usually without anyone else noticing. Stolen seats
  are impossible: reconnecting requires the secret token issued at join.

## Fixed

- **Idle games no longer drop.** Both sides now exchange heartbeats every two
  seconds, so a match where nobody has acted yet can't be reaped by routers,
  hotspots, or NAT tables that kill silent connections (the "P2 disconnected
  after 5 seconds" bug on mixed Linux/Windows games).
- Transient socket errors (common on Windows) are retried instead of being
  treated as a disconnect.
- Connection problems on the host side now log a diagnostic reason.

## Downloads

- **Linux**: `dice-wars-v1.0.2-linux-x86_64.tar.gz` — unpack and run `./dice-wars`
- **Windows**: `dice-wars-v1.0.2-windows-x86_64.zip` — unpack and run `DiceWars.exe`
