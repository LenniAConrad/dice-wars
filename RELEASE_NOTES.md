# Dice Wars v1.3.5

Multiplayer lobby release. Online players must both update (older versions are told so).

## v1.3.5 fixes

- Color picks select your map position everywhere — solo AND hosted games: each player takes over the territories of the color they chose, and the map itself never re-shuffles. Protocol bumped to 5.
- The menu map preview no longer changes scale with the dealt dice heights.

## Lobby

- **Names & colors**: set your name (saved for next time, like the host address), and pick your color first-come-first-served — taken colors are crossed out. Bots take the leftovers.
- **Guests see everything live**: the map preview, seed, and the full settings panel (players, difficulty, mode, team setup) mirror the host's lobby in real time, plus the roster with everyone's name, color, and team.
- **Host controls everything in the lobby**: player count (seats resize without dropping guests), difficulty, mode, team count / humans-vs-bots, friendly fire, island links.
- **Pick your own team** — host and guests each choose their team letter; bots balance the rest.
- Redesigned segmented-control style, shared by the lobby and the start screen.

## Playing online

- **60s turn timer** in games with 2+ humans: idle turns auto-pass; any attack resets the clock (countdown shown by the turn pill).
- **Disconnects no longer end the game**: after a grace period a bot takes the seat over; reconnecting players (auto-retry plus a manual retry) take it back.
- **Rematch**: the host's NEW MAP (or R) moves everyone straight into a fresh round — names, colors, and teams intact.

## Solo

- Picking a color now means playing that map position: the previewed map keeps its look and you take over that color's territories and dice.
- AI tuning: Easy is easier, Hard is harder, and bot personalities are milder leans instead of extremes.

## Downloads

- **Linux**: `dice-wars-v1.3.5-linux-x86_64.tar.gz`
- **Windows**: `dice-wars-v1.3.5-windows-x86_64.zip`
