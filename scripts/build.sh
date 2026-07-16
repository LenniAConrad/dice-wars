#!/usr/bin/env bash
# Build Dice Wars from scratch — installs the Rust toolchain if missing.
set -euo pipefail
cd "$(dirname "$0")/.."
# prefer the rustup toolchain over any distro rust
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust toolchain not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
fi

cargo build --release
echo "Built: target/release/dicegame"
