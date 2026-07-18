# Dice Wars v1.3.10

Accessibility, team readability, and multiplayer hardening release. Online players must both update (older versions are told so).

## v1.3.10 fixes

- Color picks select your map position everywhere — solo AND hosted games: each player takes over the territories of the color they chose, and the map itself never re-shuffles. Protocol bumped to 7.
- The menu map preview no longer changes scale with the dealt dice heights.
- Map generation no longer stalls for seconds on seeds with tiny regions, and win-chance hints reuse precomputed odds.
- Connect attempts and Linux clipboard fallbacks run off the UI thread; replay GIFs stream to disk instead of retaining up to roughly 1 GB of frame data.
- Colorblind mode is now tuned specifically for protan color vision, with nine colors separated by blue/yellow position and luminance, high-contrast pips, owner-symbol badges, and a full-protanopia simulation test.
- Team games group player cards inside shared, labeled team outlines. Protected allied lands remain visibly colored but dimmed while selecting a target, without adding more map icons or forcing team hues.
- Multiplayer hardening adds OS-random 6-digit room codes and rotating reconnect tokens, absolute authentication deadlines, bounded queues/rate limits, strict host-event validation, and non-blocking socket failure handling.
- The release workflow no longer exposes checkout credentials to builds, interpolates tag names into shell source, deletes unrelated draft releases, or force-pushes debug output.

## Lobby

- Up to **9 players** (new Cocoa color, star symbol). All preferences persist now — name, color, team, player count, difficulty — and guests automatically bring their saved color and team into any lobby they join.

- **Names & colors**: set your name (saved for next time, like the host address), and pick your color first-come-first-served — taken colors are crossed out. Bots take the leftovers.
- **Guests see everything live**: the map preview, seed, and the full settings panel (players, difficulty, mode, team setup) mirror the host's lobby in real time, plus the roster with everyone's name, color, and team.
- **Host controls everything in the lobby**: player count (seats resize without dropping guests), difficulty, mode, team count / humans-vs-bots, friendly fire, island links.
- The host lobby now includes the same sound, win-chance, protan accessibility, speed, and dark-mode toolbar as the start screen and game.
- **Pick your own team** — host and guests each choose their team letter; bots balance the rest.
- Redesigned segmented-control style, shared by the lobby and the start screen.

## Playing online

- **Real odds-based AI**: bots now decide attacks from the exact win probabilities (like the % hints) with per-difficulty thresholds — Easy needs 65%, Normal 50%, Hard attacks from 42% — and saturated boards accept even odds, so full-stack games never stall. Verified by a full-game simulation test.

- **60s turn timer** in games with 2+ humans: idle turns auto-pass; any attack resets the clock (countdown shown by the turn pill).
- **Disconnects no longer end the game**: after a grace period a bot takes the seat over; reconnecting players (auto-retry plus a manual retry) take it back.
- **Rematch**: the host's NEW MAP (or R) moves everyone straight into a fresh round — names, colors, and teams intact.

## Solo

- Picking a color now means playing that map position: the previewed map keeps its look and you take over that color's territories and dice.
- AI tuning: Easy is easier, Hard is harder, personalities are milder leans — and in team mode bots track the strongest enemy team, so trailing teams gang up on the leader together.

## Downloads

- **Linux**: `dice-wars-v1.3.10-linux-x86_64.tar.gz`
- **Windows**: `dice-wars-v1.3.10-windows-x86_64.zip`
