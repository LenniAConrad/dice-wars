#!/usr/bin/env bash
# Regenerate the README art (assets/logo.svg, assets/board.svg) headlessly:
# a fixed seed, six bots playing for 40 simulated seconds, exported as SVG.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -j "${CARGO_BUILD_JOBS:-1}"
data="$(mktemp -d)"
trap 'rm -rf "$data"' EXIT
env -u DISPLAY -u WAYLAND_DISPLAY DW_FORBID_WINDOW=1 DW_HEADLESS=1 \
    XDG_DATA_HOME="$data" APPDATA="$data" \
    DW_SEED=48213097 DW_PLAYERS=6 DW_AUTOSTART=1 DW_TEST_AUTOEND=1 \
    DW_FRAME_LIMIT=2400 DW_EXPORT_SVG=assets \
    ./target/release/dicegame
echo "Wrote assets/logo.svg and assets/board.svg"
