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
- Win by conquering the map, or in team mode by being on the last team alive.
  If you are knocked out of a team game, you keep watching while your team
  plays on.

## Start screen

- **Your color**: the swatches under the title choose the color, and the map
  position, you play.
- **Difficulty**: EASY bots hesitate and need a big edge; NORMAL plays by
  personality; HARD takes even fights and keeps its territory together. Bots
  roll the same dice and follow the same reinforcement rules as you.
- **Players**: 2 to 9 in total.
- **Mode**: FREE-FOR-ALL, or TEAMS with 2–4 teams (or humans against bots).
  Teammates never share dice. Two rules can be toggled: *Friendly fire*
  (teammates may attack each other) and *Islands link via allies* (an ally's
  land can connect your regions for reinforcement).
- **Seed**: the same 8-digit seed and player count always produce the same
  map. Type one, step with `<` and `>`, or press `NEW` (`N`). The star
  bookmarks the map.

Each bot has a hidden personality from the seed. When one player grows
dominant, the others stop fighting each other and turn on the leader.

## Playing together

**HOST GAME** opens a lobby with a 6-digit room code. While it is open, the
lobby is announced on your local network, and it shows up in the **JOIN GAME**
screen of every player on the same network; one click joins it. Anyone on that
network can see and join an open lobby.

To join from elsewhere (for example over a VPN such as Tailscale), enter the
host's address and room code under *Or enter an address*. Joined players take
the first seats and bots fill the rest. The host's game is authoritative and
can start whenever it is ready; **NEW MAP** after a game starts a rematch with
everyone still connected. Disconnected players are replaced by a bot after a
grace period and can reconnect.

If the game is not found or a connection fails, a firewall is the usual
cause. The game uses TCP port 7777 for play and UDP port 7778 for finding
lobbies. On Ubuntu or Mint: `sudo ufw allow 7777/tcp` and
`sudo ufw allow 7778/udp`. Guest Wi-Fi networks often block device-to-device
traffic. Game traffic is not encrypted, so play over a trusted network or VPN
rather than forwarding the port to the internet.

## Replays

The game-over card offers **REPLAY**, which plays back every battle and
reinforcement, and **SAVE GIF**, which writes a timelapse `replay-<seed>.gif`
to your data folder. **NEW MAP** (`R`) starts again and **MENU** (`Esc`)
leaves.

## Settings and accessibility

The two buttons at the top right of every screen are a quick sound toggle and
**Settings**, which holds every preference:

- **Theme**: Daylight, Midnight, Sakura, Sage, Paper, Ocean or Ember. Themes
  change the interface; the map and dice keep their colors. `F10` cycles them.
- **Sound**: full, half or off (`M`).
- **Game speed**: 1x, 2x or 4x. Fights between bots always play faster, and
  holding `Space` during bot turns fast-forwards them in local games.
- **Win chances**: show the exact odds on every attack target.
- **Color assist** (`F9`): hues chosen for protan color vision, plus an owner
  symbol on every dice badge.
- **Motion** (`F8`): *Less* removes shaking, bouncing, sliding and fades.

The keyboard also works on the map: arrow keys move the focus, `Enter` selects
and attacks.

## Your files

Settings, bookmarks and saved GIFs live in `~/.local/share/dice-wars` on
Linux, `~/Library/Application Support/dice-wars` on macOS and
`%APPDATA%\dice-wars` on Windows.
