# Player guide

## The rules

- Click one of your lands with more than one die, then an adjacent enemy land.
  Selecting a land points arrows at every legal target, with its exact win
  chance when win chances are on.
- Attacker and defender roll all their dice. The higher total captures the
  land: the attacking stack moves in and leaves one die behind. Ties and losses
  drop the attacker to one die.
- **END TURN** (`Space`) gives one die per land in your largest connected
  region, placed randomly. Dice that do not fit are stored, up to 64, and come
  back on later turns; the button shows how many you will receive.
- Going first is an advantage, so in the first round only, the earliest
  players get part of their reinforcement (with 2 players the first gets 35%,
  with 6 players 95%; from 7 players everyone gets it all). It applies to bots
  and humans alike and makes every seat about equally likely to win.
- Win by conquering the map, or in team mode by being on the last team alive.
  If you are knocked out of a team game, you keep watching while your team
  plays on.

## Start screen

The left side holds the map and the rules, the right side your saved maps and
a help card. Hosting and joining use the same layout, so every control stays
in the same place.

- **Your color**: the swatches below the rules choose the color, and the map
  position, you play.
- **Difficulty**: EASY bots hesitate and need a big edge; NORMAL plays by
  personality; HARD weighs every attack by what it gains and how exposed it
  leaves its lands, and wins well over half of its games against Normal bots.
  Bots roll the same dice, see the same board and follow the same
  reinforcement rules as you.
- **Players**: 2 to 9 in total.
- **Map**: SMALL, MEDIUM or LARGE sets how many lands there are; ISLANDS
  splits the land into several islands joined by narrow bridges.
- **Mode**: FREE-FOR-ALL, or TEAMS with 2–4 teams (or humans against bots).
  Teammates never share dice. Two rules can be toggled: *Friendly fire*
  (teammates may attack each other) and *Islands link via allies* (an ally's
  land can connect your regions for reinforcement).
- **Seed**: the same 8-digit seed, player count and map option always
  produce the same map. Type one, step with `<` and `>`, or press `NEW` (`N`). The star on the
  preview saves the map to **Saved maps**.
- **How to play / Your record**: the card at the bottom right shows the rules
  until you finish a game, then your wins per difficulty (the **RULES** button
  brings the rules back). In team mode it holds the team setup.

Each bot has a hidden personality from the seed. When one player grows
dominant, the others stop fighting each other and turn on the leader.

## Playing together

**HOST GAME** opens a lobby with a 6-digit room code; your own line in the
player list is where you type your name. While it is open, the
lobby is announced on your local network, and it shows up in the **JOIN GAME**
screen of every player on the same network; one click joins it. Anyone on that
network can see and join an open lobby.

To join from elsewhere (for example over a VPN such as Tailscale), enter the
host's address and room code under *Or enter an address*. Joined players take
the first seats and bots fill the rest. The host's game is authoritative and
can start whenever it is ready, and its game speed applies to everyone; a new
map (from the game-over card or the pause menu) starts a rematch with everyone
still connected. Disconnected players are replaced by a bot after a
grace period and can reconnect.

If the game is not found or a connection fails, a firewall is the usual
cause. The game uses TCP port 7777 for play and UDP port 7778 for finding
lobbies. On Ubuntu or Mint: `sudo ufw allow 7777/tcp` and
`sudo ufw allow 7778/udp`. Guest Wi-Fi networks often block device-to-device
traffic. Game traffic is not encrypted, so play over a trusted network or VPN
rather than forwarding the port to the internet. To play over the internet
without forwarding anything, use a relay server you trust ([RELAY.md](RELAY.md)
explains how to run one): the host types its address into **INTERNET** in the
lobby (the dot turns green when the room is online), and guests type the same
address into **INTERNET RELAY** on the join screen, leave the address empty,
and enter the room code.

## Your first game

A few tips point at the next step during your first turn: pick a land, attack
a neighbor, end your turn. They stop after your first ended turn, or at once
with **SKIP TIPS**.

## Continuing later

A solo game is saved after every move. Leave it from the pause menu (or just
close the window) and the start screen offers **CONTINUE** with the round,
players and difficulty. Starting a new game or finishing this one replaces the
save.

## Pausing

**MENU** at the top left (or `Esc`) pauses with three choices: **LEAVE** to the
menu, **NEW MAP**, or **KEEP PLAYING**. `R` asks for a new map directly.

## After a game

The game-over card shows how the map changed hands (every player's lands over
the game) and four numbers: rounds played, battles fought and won, the
luckiest win (the lowest odds anyone beat) and the biggest stack taken in one
attack.

## Replays

The game-over card offers **REPLAY**, which plays back every battle and
reinforcement, and **SAVE GIF**, which writes a timelapse `replay-<seed>.gif`
to your data folder. **NEW MAP** (`R`) starts again and **MENU** (`Esc`)
leaves. While a replay runs, **PAUSE** (`Space`) stops it, `→` steps one move
while paused, **RESTART** (`R`) starts over, and 1x/2x/4x sets the speed.

## Settings and accessibility

The two buttons at the top right of every screen are a quick sound toggle and
**Settings**, which holds every preference:

- **Theme**: Daylight, Midnight, Sakura, Sage, Paper, Ocean or Ember. Themes
  change the interface and the lakes; the lands and dice keep their colors.
  Switching fades over briefly (instantly with Motion: Less). `F10` cycles
  them.
- **Sound**: full, half or off (`M`).
- **Game speed**: 1x, 2x or 4x. Fights between bots always play faster, and
  holding `Space` during bot turns fast-forwards them in local games.
- **Win chances**: show the exact odds on every attack target.
- **Color assist** (`F9`): a muted palette that stays distinct for red-green
  color vision (protanopia and deuteranopia), plus an owner symbol on every
  dice badge.
- **Motion** (`F8`): *Less* removes shaking, bouncing, sliding and fades.

Everything works without a mouse. On menus and in lobbies, `Tab` and
`Shift+Tab` move between controls, the arrow keys jump to the nearest one in
that direction, and `Enter` activates it; dialogs keep the focus to
themselves. On the map, the arrow keys move between lands and `Enter` selects
and attacks. Game controllers work too: the D-pad or left stick moves, A
activates, B goes back or pauses, X ends your turn and Y or the shoulder
buttons step through controls.

## Your files

Settings (including your record), saved maps, the saved game and GIFs live in `~/.local/share/dice-wars` on
Linux, `~/Library/Application Support/dice-wars` on macOS and
`%APPDATA%\dice-wars` on Windows.
