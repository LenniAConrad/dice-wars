# Agent instructions

## Tests are always headless

Never open a game window while testing, verifying, profiling or capturing
screenshots. Run every check headlessly:

```sh
scripts/check.sh          # everything: unit tests, cross-target checks, screenshot, frame-time and audio audits
scripts/check.sh --quick  # unit tests and cross-target type checks only
```

`scripts/check.sh` unsets `DISPLAY`/`WAYLAND_DISPLAY` and exports
`DW_FORBID_WINDOW=1`, which makes the engine refuse to create a window. Keep it
that way for anything you run by hand:

- Start the game only with `DW_HEADLESS=1` (plus `DW_FORBID_WINDOW=1`), a
  temporary `XDG_DATA_HOME`/`APPDATA`, and `DW_FRAME_LIMIT`, so it exits on its
  own and never touches the player's saved settings.
- Visual checks: `DW_CAPTURE=/path/out.png` (last frame) and
  `DW_CAPTURE_FRAMES=1,2,…` (numbered frames); `DW_SIZE=WxH` sets the
  viewport; `DW_INPUT="frame:click:x:y;frame:move:x:y;frame:key:Name"` scripts
  input; `DW_THEME`, `DW_SEED`, `DW_PLAYERS`, `DW_AUTOSTART`, `DW_SCENE`,
  `DW_BATTLE` and `DW_TEST_AUTOEND` (bots play every seat) set up scenes. See
  `scripts/audit-ui.py` for examples, and look at the PNGs you produce.
- Audio: `DW_HEADLESS_AUDIO=1` keeps real audio output (and 60 Hz pacing) in a
  headless run; `scripts/audit-audio.py` routes it to a temporary sink.
- Performance: `scripts/audit-runtime.py` (headless). Its `--native` mode
  opens windows and is for a person at the desk only; it refuses to run unless
  `DW_ALLOW_WINDOWS=1` is set. Agents must not set it.

Never run `./target/release/dicegame` or `cargo run` without `DW_HEADLESS=1`.

## Building

- One build worker (`-j 1` / `CARGO_BUILD_JOBS=1`) unless told otherwise; the
  owner's desktop must stay responsive.
- No Cargo dependencies. The engine (`src/engine/`) talks to X11/PulseAudio,
  Win32/winmm and Cocoa/AudioQueue directly.
- Installers: `install-linux.sh`, `install-macos.sh`, `install-windows.cmd`.

## Code notes

- `src/main.rs` holds the game, UI and networking; `src/engine/` is the
  renderer, windowing and audio.
- The global RNG is for dice and map generation only. Seeds, bookmarks and
  replays depend on its exact sequence; never draw from it for effects or sound.
- Map art keeps identical pastel colors in light and dark mode (the screenshot
  audit enforces it).
- Bump `NET_PROTO` whenever map generation or network messages change.
