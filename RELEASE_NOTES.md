# Dice Wars v1.7.0

Consistent interface and multiplayer reliability release.

- **One layout for setup**: the menu, the host lobby and the guest lobby share one grid — map and rules on the left, cards on the right — so every control sits in the same place. You type your name on your own line of the player list.
- **One visual language**: every choice is a segmented control filled with your color, one filled primary button per screen, small-caps labels, and the accent color only for hover and focus. The settings panel uses the same controls.
- **Start screen**: the decorative burst is gone; saved maps get their own card, and the help card turns into your win record (per difficulty) after your first finished game.
- **In game**: a MENU button at the top left opens a pause dialog (LEAVE / NEW MAP / KEEP PLAYING), so NEW MAP no longer sits next to END TURN. Battle results say who took what ("Mint takes your land"), END TURN is the filled action on your turn, and the game-over screen no longer shows stale messages or duplicate buttons.
- **Sharper and smoother**: the game always renders at the window's full resolution with anti-aliased edges (large and 4K windows used to be drawn smaller and scaled up), capture fades run at the full frame rate, and only the lands that change are repainted. Big windows now keep 60 fps.
- **Replays**: pause, step, restart and 1x/2x/4x speed controls.
- **Color assist**: a softer palette that also stays distinct for deuteranopia.
- **Multiplayer fixes**: the host's game speed applies to everyone, leaving a game closes its connections (a later lobby works again), guests stop reconnecting to a host that left, rematches never strand a reconnecting player, and a guest that falls behind resyncs instead of being dropped.
- **Platform fixes**: on Windows, closing the window quits, focus changes release held keys, F10 no longer opens the window menu and the icon is upright; on macOS, quitting no longer cancels logout; shortcuts ignore Ctrl/Cmd/Alt.
- **Repo**: CI also tests on Windows, macOS and the minimum Rust version; releases publish only their own notes; settings are written atomically.

---

# Dice Wars v1.6.0

Themes, settings and game-flow release.

- **Themes**: Daylight, Midnight, Sakura, Sage, Paper, Ocean and Ember. The map art keeps its colors.
- **Settings panel** replaces the icon toolbar: theme, sound, 1x/2x/4x speed, win chances, color assist and motion, all labelled.
- **Faster flow**: fights between bots play faster, holding Space fast-forwards bot turns, and the turn toast only appears for human turns.
- **Clearer panel**: plain turn labels, an END TURN explanation on hover, labelled player cards with a map-share bar.
- **Window resizing**: no stretched or flashing frames, and a minimum window size.
- **Fixes**: unpredictable online dice, reliable reconnects, host games without guests and replays match the dealt board and lobby teams, and a defeated player keeps watching while their team plays on.
- **Code**: the game is split into focused modules with no clippy warnings.

---

# Dice Wars v1.5.0

Interface, sound, LAN play and installer release.

- **Installers**: `install-linux.sh`, `install-macos.sh` and `install-windows.cmd` set up everything a fresh machine needs to build and install the game.
- **macOS**: a native Cocoa/AudioQueue backend; releases include a universal app.
- **LAN games**: open host lobbies appear in every JOIN GAME screen on the same network and join with one click.
- **Interface**: lighter outlines and soft shadows, battles outlined on the map with a clear arrow, captured land fading into the winner's color, turn toasts, screen fades, a game-over card with REPLAY / SAVE GIF / NEW MAP / MENU, and a confirm dialog whose highlighted choice keeps playing.
- **Sound**: no more clicks at the end of effects, warmer tones, varied dice rolls that land with the dice on screen, results from your point of view (captured, lost, defended, repelled), quieter fights between bots, a half-volume setting, a soft limiter, and smooth Windows output.
- **Performance**: full board redraws are about three times faster, removing most dropped frames while bots play.
- **Testing**: every check runs headlessly through `scripts/check.sh`, also in CI.

---

# Dice Wars v1.4.0

Native engine and interface refinement release.

- Replaces Macroquad and getrandom with a native software renderer, OS entropy, embedded font masks, and a bounded procedural-audio mixer. No Cargo dependencies; Linux uses X11/PulseAudio, Windows uses Win32/GDI and native audio.
- Caps animated rendering at 60 fps, reduces background polling to 15 Hz, caches map layers, and reuses unchanged menu/idle frames. Builds default to one worker.
- Compact attack/defend roll rows leave the board, controls, and player cards visible; explicit totals and tie results make battles easier to follow.
- Immediate light/dark switching updates every surface, including the outer background. Dark maps use muted fills; labels, badges, pips, and owner-colored text have contrast checks.
- Adds keyboard territory focus/activation, toolbar descriptions, reduced motion, theme/accessibility shortcuts, and consistent controls in lobbies and replays.
- Repairs offscreen image orientation, prevents live AI from inventing moves during replays, samples long GIFs through the final event, keeps saved-map seeds compatible, and adds reproducible screenshot audits.
- Hard bots now follow the same random reinforcement rule as humans and do not prioritize opponents simply because they are human.

---

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
