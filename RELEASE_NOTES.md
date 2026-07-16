# Dice Wars v1.0.0

A pastel Dice Wars–style territory conquest game written in Rust. First release!

## Highlights

- **Hex-map conquest**: seeded, deterministic maps you can bookmark and share — same seed, same map.
- **2–8 players**: play solo against AI bots with hidden personalities and three difficulty levels, or battle friends.
- **Online multiplayer**: host a lobby with a 4-digit room code (LAN/VPN out of the box); the server validates every move and rolls all dice, so nobody can cheat.
- **Battle showcase**: every attack pops up both sides' dice with running totals over a dimmed board.
- **Replays & GIF export**: rewatch any finished game or save it as a looping timelapse GIF — encoder built in, no external tools.
- **Accessibility**: colorblind-safe palette mode with per-player shape badges, win-probability hints, adjustable game speed, dark mode.
- **Procedural audio**: every sound effect is synthesized at startup — no asset files.

## Downloads

- **Linux**: `dice-wars-v1.0.0-linux-x86_64.tar.gz` — unpack and run `./dice-wars`
- **Windows**: `dice-wars-v1.0.0-windows-x86_64.zip` — unpack and run `DiceWars.exe`

Or build from source: `./run.sh` (Linux/macOS) or `scripts\build.bat` (Windows). The build scripts install the Rust toolchain automatically if it's missing.

## How to play

Click one of your territories (2+ dice), then an adjacent enemy territory to attack. Both sides roll all their dice — higher total wins, ties defend. END TURN reinforces you with one die per territory in your largest connected region. Conquer the whole map!
