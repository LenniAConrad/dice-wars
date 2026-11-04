# Install

Download Dice Wars from GitHub (Code, then Download ZIP), unzip it, and run the
installer for your system from inside the folder. Each installer first gets
what it needs, so nothing has to be installed beforehand. Each one prints what
it is doing, ends with SUCCESS or FAILED, and is safe to run again.

| System | Run | Installs |
|---|---|---|
| Linux | `bash install-linux.sh` | `~/.local/bin/dice-wars`, icon and app-menu entry |
| macOS, Apple silicon / Intel | `bash install-macos.sh` | `~/Applications/Dice Wars.app`, plus `~/.local/bin/dice-wars` linked to it |
| Windows 10/11 x64, Windows 11 ARM64 (x64 emulation) | double-click `install-windows.cmd` | `%LOCALAPPDATA%\Programs\DiceWars\DiceWars.exe`, Start Menu shortcut, Settings > Apps entry |

What each installer gets when it is missing:

- **Linux**: a C compiler and linker, curl, and the X11 and PulseAudio client
  libraries (`libX11.so.6`, `libpulse-simple.so.0`; PipeWire desktops use the
  same library), through apt, dnf, yum, pacman or zypper with sudo. Then Rust
  through rustup. Other distributions: install those yourself and pass
  `--skip-deps`. The game uses X11, so Wayland desktops need XWayland (the
  default on GNOME and KDE).
- **macOS**: Apple's Command Line Tools (macOS opens a window; click Install and
  the script carries on by itself), then Rust through rustup. No Homebrew or
  sudo.
- **Windows**: Rust through rustup, using Rust's GNU toolchain
  (`x86_64-pc-windows-gnu`), which brings its own linker. The multi-GB Visual
  Studio Build Tools are not needed; if they are already installed, the MSVC
  toolchain is used instead. No administrator permission.

Rust goes into `~/.cargo` (`%USERPROFILE%\.cargo`) without changing your shell
profile or PATH. Builds use **one build worker** by default so the desktop stays
responsive; raise it with the jobs option or `CARGO_BUILD_JOBS`.

## Options

| Linux / macOS | Windows | Meaning |
|---|---|---|
| `--prefix PATH` | `-Prefix PATH` | Linux: install under PATH instead of `~/.local` (app-menu files go to `PATH/share`). macOS: put the `dice-wars` command link in `PATH/bin`. Windows: install folder. |
| `--app-dir PATH` (macOS) | | Put `Dice Wars.app` in PATH instead of `~/Applications`. |
| `--jobs N` | `-Jobs N` | Parallel build workers (default 1, or `CARGO_BUILD_JOBS`). |
| `--skip-deps` (Linux) | | Do not use the package manager or sudo; only check that the C tools and libraries are there. |
| `--uninstall` | `-Uninstall` | Remove what the installer added. Pass the same prefix you installed with. |
| `-h`, `--help` | `-Help` | Show the options. |

Environment: `CARGO_BUILD_JOBS` (default jobs), `CARGO_HOME` (where rustup
lives), `CARGO_TARGET_DIR` (build folder), `DICE_WARS_INSTALL_PREFIX` (Linux and
macOS default prefix), `DICE_WARS_APPLE_TOOLS_WAIT_SECONDS` (macOS, default
3600: how long to wait for Apple's installer window).

## Uninstall

- Linux: `bash install-linux.sh --uninstall`
- macOS: `bash install-macos.sh --uninstall`
- Windows: Settings > Apps > Dice Wars, or `install-windows.cmd -Uninstall`

Uninstalling keeps your settings, bookmarks and replays
(`~/.local/share/dice-wars` on Linux, `~/Library/Application Support/dice-wars`
on macOS, `%APPDATA%\dice-wars` on Windows) and leaves
Rust and the system packages in place.

## Without the installers

With Rust 1.82 or newer and a C linker already set up, `cargo run --release -j 1`
builds and starts the game from the repository root; the binary is
`target/release/dicegame` (`dicegame.exe` on Windows).
