# Development

Dice Wars has no Cargo dependencies. `src/engine/` draws with its own software
rasterizer and talks to the system directly: X11 and PulseAudio on Linux,
Cocoa and AudioQueue on macOS, Win32/GDI and winmm on Windows. `src/main.rs`
holds the game, interface and networking; `src/svg.rs` exports the README art.

## Checks: always headless

```sh
scripts/check.sh          # unit tests, cross-target checks, screenshot, frame-time and audio audits
scripts/check.sh --quick  # unit tests and cross-target type checks
```

Nothing in the checks opens a window. `scripts/check.sh` removes `DISPLAY`
and sets `DW_FORBID_WINDOW=1`, which makes the engine refuse to create one.
CI runs the same script on every push. Builds use one worker by default
(`CARGO_BUILD_JOBS` overrides it).

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
| `DW_HEADLESS_AUDIO=1` | with `DW_HEADLESS`: keep real audio output and 60 Hz pacing |
| `DW_FORBID_WINDOW=1` | fail instead of opening a window |
| `DW_SIZE=WxH` | headless viewport |
| `DW_FRAME_LIMIT=n` | exit after n frames |
| `DW_CAPTURE=out.png`, `DW_CAPTURE_FRAMES=1,2` | save the last frame, or numbered frames |
| `DW_INPUT=frame:click:x:y;frame:move:x:y;frame:key:Name` | scripted input |
| `DW_SEED`, `DW_PLAYERS`, `DW_TEAMS`, `DW_ACCESSIBLE` | scene setup |
| `DW_THEME` | `light`, `dark`, or a theme name such as `Sakura` |
| `DW_AUTOSTART=1`, `DW_TEST_AUTOEND=1` | start a game; bots also play your seat |
| `DW_SCENE=…`, `DW_BATTLE=1` | fixed scenes (`join`, `confirm`, `selection`, `over`, `replay`, `gif`, `battle-replay`) |
| `DW_EXPORT_SVG=dir` | on exit, write `board.svg` and `logo.svg` |
| `DW_PROFILE=1` | print frame-time statistics on exit |
| `DW_PORT=n` | game port (LAN discovery uses n + 1) |

Always pair manual runs with a temporary `XDG_DATA_HOME` (and `APPDATA`) so
your own settings stay untouched:

```sh
DW_HEADLESS=1 DW_FORBID_WINDOW=1 XDG_DATA_HOME="$(mktemp -d)" \
  DW_AUTOSTART=1 DW_FRAME_LIMIT=120 DW_CAPTURE=/tmp/dice-wars.png ./target/release/dicegame
```

## Performance

- Input is polled at up to 60 Hz; animated frames are capped at 60 fps and
  unfocused windows drop to 15 Hz. Quiet menus and idle turns reuse the last
  frame.
- Rendering is supersampled 2× per axis within a four-million-pixel budget,
  then filtered to the window size. Only changed tiles are filtered and
  uploaded.
- The map, status panel, menu art, dice and shadows are cached; moving pieces
  are drawn on top.
- Triangles rasterize only their covered span per row and copy opaque runs
  directly. This cut a full board redraw from about 17 ms to about 6 ms and
  removed most frames over 16.7 ms while bots play.
- Audio runs on one worker with at most 16 voices, a soft limiter and a
  bounded queue.

## Releases

Pushing a `v*` tag builds Linux, Windows and macOS (universal) packages in
`.github/workflows/release.yml` and attaches them to a GitHub release.
