# Dice Wars v1.0.1

Multiplayer stability release.

## Fixed

- **Guests no longer lose the connection mid-game.** The network protocol capped
  messages at 200 bytes as an anti-flood measure, but late-game reinforcement
  broadcasts legitimately exceed that — the guest treated the long line as a dead
  socket and dropped ("Connection lost"). Host-to-guest messages now allow 4 KB,
  while the strict cap remains on untrusted guest input.
- **Hosting again after a match always works.** The lobby port is now opened once
  and reused for every lobby, so re-hosting can no longer fail silently on a port
  stuck in TIME_WAIT. If the port is genuinely unavailable, the menu says so.
- Wrong-code bans now trigger after five attempts instead of three, and the join
  screen explains likely firewall causes when a connection fails.
- Installing over a running game no longer fails with "Text file busy."
- The window/taskbar now shows the die icon on Linux (engine updated to
  macroquad 0.4.15), instead of the generic placeholder.

## Downloads

- **Linux**: `dice-wars-v1.0.1-linux-x86_64.tar.gz` — unpack and run `./dice-wars`
- **Windows**: `dice-wars-v1.0.1-windows-x86_64.zip` — unpack and run `DiceWars.exe`
