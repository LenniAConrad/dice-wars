<p align="center">
  <img src="assets/logo.svg" width="98" height="114" alt="Dice Wars: a yellow game die">
</p>

<h1 align="center">Dice Wars</h1>

<p align="center">
  A pastel territory-conquest dice game in dependency-free Rust.<br>
  <strong><a href="docs/GUIDE.md">Read the player guide »</a></strong>
</p>

<p align="center">
  <a href="docs/DEVELOPMENT.md"><img src="https://img.shields.io/badge/Rust%20build-1.82%2B-C08472?logo=rust" alt="Rust build: 1.82 or newer"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-6a5acd" alt="Platforms: Linux, macOS, Windows">
</p>

Conquer a hex map by rolling stacks of dice against your neighbours. Play
against up to eight bots with hidden personalities, or with friends on your
local network. Maps come from an 8-digit seed, so any good map can be
bookmarked and replayed, and every finished game can be watched again or
saved as a GIF.

## Install and run

Download Dice Wars from GitHub (Code, then Download ZIP), unzip it, and run
the installer for your system from inside the folder. Each installer first
gets what it needs to build the game, so nothing has to be installed
beforehand.

| System | Run |
| --- | --- |
| Linux | `bash install-linux.sh` — [installer](install-linux.sh); gets the C build tools and audio/X11 libraries (asks for your sudo password) and Rust. |
| macOS, Apple silicon / Intel | `bash install-macos.sh` — [installer](install-macos.sh); gets Apple's command line tools (click Install in Apple's window) and Rust. |
| Windows | double-click `install-windows.cmd` — [installer](install-windows.cmd); gets Rust with its own linker, no Visual Studio needed. |

Each installer prints what it is doing and ends with SUCCESS or FAILED. Then
start **Dice Wars** from your app menu, Launchpad or Start Menu, or run
`dice-wars` in a terminal. Options and uninstalling are covered in the
[install guide](docs/INSTALL.md).

Prefer not to build? Every [release](https://github.com/LenniAConrad/dice-wars/releases)
has ready-to-run downloads for Linux (x86_64), Windows (x64) and macOS
(universal). The Mac app is not notarized: if macOS refuses to open it, go to
System Settings → Privacy & Security and click **Open Anyway**.

<p align="center">
  <img src="assets/board.svg" width="760" alt="A Dice Wars board after a few bot turns: pastel territories with stacks of dice">
</p>

## How to play

Click one of your lands with two or more dice, then a neighbouring enemy land
to attack. Both sides roll all their dice; the higher total wins, and ties go
to the defender. **END TURN** gives you one new die for every land in your
largest connected region. Conquer the whole map to win.

| Key | Action |
| --- | --- |
| Arrow keys, `Enter` | focus a land, select it or attack |
| `Space` | end your turn |
| `Esc` | cancel the selection, or pause |
| `R` | new map |
| `M` | sound: full, half, off |
| `F8` / `F9` / `F10` | less motion / colour assist / next theme |

Everything else — seven themes, game speed, win chances and accessibility
options — is in **Settings** at the top right of every screen. Hold `Space`
during bot turns to fast-forward.

## Playing together

Press **HOST GAME** and your lobby appears in every **JOIN GAME** screen on
the same network, ready to join with one click, the way LAN worlds work in
many games. Hosts can also share their address and 6-digit room code for VPN
play. Details and firewall tips are in the [player guide](docs/GUIDE.md#playing-together).

## Guides

- [Player guide](docs/GUIDE.md): rules, teams, difficulty, multiplayer, replays and accessibility.
- [Install guide](docs/INSTALL.md): installer options, manual builds and uninstalling.
- [Development](docs/DEVELOPMENT.md): [project layout](docs/DEVELOPMENT.md#project-layout), headless checks, test hooks and performance.

## License

MIT, see [LICENSE](LICENSE). The interface font is Nunito (SIL Open Font
License); see [assets/FONT-LICENSE.txt](assets/FONT-LICENSE.txt).
