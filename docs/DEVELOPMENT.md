# Development

Dice Wars has no Cargo dependencies. `src/engine/` draws with its own software
rasterizer and talks to the system directly: X11 and PulseAudio on Linux,
Cocoa and AudioQueue on macOS, Win32/GDI and winmm on Windows.

## Project layout

The engine is the `dicegame` library (`src/lib.rs`); the game is the binary,
one module per concern. Unit tests sit next to the code they test.

| Path | Contents |
| --- | --- |
| `src/main.rs` | entry point, `App` state and the frame loop |
| `src/game.rs` | match state and rules: territories, battles, turns, teams, reinforcements |
| `src/board.rs` | hex grid geometry and map generation |
| `src/ai.rs` | bot move selection |
| `src/theme.rs` | themes, player palettes and contrast helpers |
| `src/ui.rs` | virtual canvas scaling, text, widgets, toolbar and settings panel |
| `src/dice.rs` | dice and stack drawing |
| `src/draw_game.rs` | board, side panel, banners and battle overlay |
| `src/play.rs` | the in-game screen: layout, input and per-frame update |
| `src/menu.rs` | the main menu |
| `src/lobby.rs` | host, guest and join lobby screens |
| `src/net.rs` | network play: protocol, host and guest sessions, authentication |
| `src/lan.rs` | LAN lobby discovery (UDP beacon on the game port + 1) |
| `src/relay.rs` | internet relay: protocol, client helpers and the server, shared by the game and the relay binary |
| `src/bin/dice-wars-relay.rs` | the relay server program (`cargo build --release --bin dice-wars-relay`, see [RELAY.md](RELAY.md)) |
| `src/replay.rs` | watching a finished match again |
| `src/gif.rs` | animated GIF export of replays |
| `src/settings.rs` | settings and seed bookmarks in the per-user data directory |
| `src/sound.rs` | sound effects, synthesized at startup |
| `src/clipboard.rs` | clipboard through the platform's tools |
| `src/svg.rs` | SVG export of the board and logo for the README |
| `src/testutil.rs` | shared test helpers and the `GLOBALS` test lock |
| `src/engine/` | renderer (`raster.rs`), font, images, audio mixer, RNG, and one window/input backend per OS (`linux.rs`, `windows.rs`, `macos.rs`) |

The global RNG (`engine::random`) is for dice and map generation only. Seeds,
bookmarks and replays depend on its exact sequence; never draw from it for
effects, animation or sound.

## Checks: always headless

Never open a game window while testing, verifying, profiling or capturing
screenshots. Every test and visual check runs headlessly:

```sh
scripts/check.sh          # unit tests, cross-target checks, screenshot, frame-time and audio audits
scripts/check.sh --quick  # unit tests and cross-target type checks
```

`scripts/check.sh` unsets `DISPLAY` and `WAYLAND_DISPLAY` and sets
`DW_FORBID_WINDOW=1`, which makes the engine refuse to create a window. CI
runs the same script on Linux, plus `cargo test` on Windows and macOS and a
build on the minimum Rust version, all with `DW_FORBID_WINDOW=1`. Builds use
one worker by default (`CARGO_BUILD_JOBS` overrides it).

Start the game by hand only with `DW_HEADLESS=1 DW_FORBID_WINDOW=1`, a
`DW_FRAME_LIMIT` so it exits on its own, and a temporary `XDG_DATA_HOME`
(and `APPDATA`) so the saved settings stay untouched. Never run
`./target/release/dicegame` or `cargo run` without `DW_HEADLESS=1`.

| Script | What it verifies |
| --- | --- |
| `scripts/audit-ui.py` | 43 headless screenshot scenes across themes: layout, hover feedback, theme switching, cache refresh, GIF orientation (needs Pillow). |
| `scripts/audit-runtime.py` | Frame work per scene, including bots playing a whole turn cycle; reports mean, p95, max and frames over 16.7 ms. |
| `scripts/audit-audio.py` | Two effects separated by silence reach the audio device, routed to a temporary sink (Linux, `pactl`/`parec`). |
| `scripts/readme-art.sh` | Regenerates `assets/logo.svg` and `assets/board.svg`. |

`audit-runtime.py --native` measures real window presentation and therefore
opens windows; it requires `DW_ALLOW_WINDOWS=1` and is for a person at the
desk, never for automated runs.

## Test hooks

All hooks are environment variables read at startup.

| Variable | Effect |
| --- | --- |
| `DW_HEADLESS=1` | render offscreen, no window, no pacing, silent |
| `DW_FORBID_WINDOW=1` | fail instead of opening a window |
| `DW_HEADLESS_AUDIO=1` | with `DW_HEADLESS`: keep real audio output and 60 Hz pacing |
| `DW_CAPTURE=out.png`, `DW_CAPTURE_FRAMES=1,2` | save the last frame, or numbered frames |
| `DW_INPUT=frame:click:x:y;frame:move:x:y;frame:key:Name` | scripted input |
| `DW_SIZE=WxH` | headless viewport |
| `DW_FRAME_LIMIT=n` | exit after n frames |
| `DW_SEED`, `DW_PLAYERS`, `DW_TEAMS`, `DW_ACCESSIBLE` | scene setup |
| `DW_THEME` | `light`, `dark`, or a theme name such as `Sakura` |
| `DW_AUTOSTART=1`, `DW_TEST_AUTOEND=1` | start a game; bots also play your seat |
| `DW_SCENE=…`, `DW_BATTLE=1` | fixed scenes (`join`, `confirm`, `selection`, `over`, `replay`, `gif`, `battle-replay`) |
| `DW_EXPORT_SVG=dir` | on exit, write `board.svg` and `logo.svg` |
| `DW_PROFILE=1` | print frame-time statistics on exit |
| `DW_PORT=n` | game port (LAN discovery uses n + 1) |
| `DW_DICE_SEED=n` | pin the dice and bot choices (headless runs only) |
| `DW_FULL_LAYERS=1` | repaint cached layers in full instead of by region |
| `DW_TEST_HOST`, `DW_TEST_JOIN="addr code"` | open a host lobby or join one at startup (network tests); `addr` may be `relay://host:port` |

See `scripts/audit-ui.py` for examples, and look at the PNGs you produce:

```sh
DW_HEADLESS=1 DW_FORBID_WINDOW=1 XDG_DATA_HOME="$(mktemp -d)" \
  DW_AUTOSTART=1 DW_FRAME_LIMIT=120 DW_CAPTURE=/tmp/dice-wars.png ./target/release/dicegame
```

## Performance

- Input is polled at up to 60 Hz; animated frames are capped at 60 fps and
  unfocused windows drop to 15 Hz. Quiet menus and idle turns reuse the last
  frame.
- The raster is never below the window's resolution. Edges are anti-aliased
  analytically: triangles get per-edge coverage (shapes built from several
  triangles keep their inner edges hard, so they tile without seams), and
  circles, rounded rectangles and glyphs use coverage masks. Windows up to
  about a megapixel additionally render at 2× per axis and are averaged down.
- Only changed 64×32 tiles are copied to the presented frame; on Linux only
  those tiles are uploaded to the X server, and the server round trip
  overlaps the next frame's rendering. Windows and macOS present the whole
  frame.
- The map, status panel, menu art, dice and shadows are cached. The map is
  one cached layer made of per-land parts: a capture fade, a lifted stack,
  hover or selection repaints only the affected lands' regions (clipped),
  which `scripts/audit-ui.py` checks is pixel-identical to a full repaint.
- At 3840×2160 a bot turn averages about 6 ms per frame and the menu about
  7 ms (headless, one thread, before the upload to the display).
- Triangles rasterize only their covered span per row and copy opaque runs
  directly. This cut a full board redraw from about 17 ms to about 6 ms and
  removed most frames over 16.7 ms while bots play.
- Audio runs on one worker with at most 16 voices, a soft limiter and a
  bounded queue.

## Releases

Pushing a `v*` tag builds Linux, Windows and macOS (universal) packages in
`.github/workflows/release.yml` and attaches them to a GitHub release. The
tag must match the `version` in `Cargo.toml`, and the release text is the
`# Dice Wars vX.Y.Z` section of `RELEASE_NOTES.md`.
