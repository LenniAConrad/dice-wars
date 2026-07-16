#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
# prefer the rustup toolchain over any distro rust
export PATH="$HOME/.cargo/bin:$PATH"
cargo run --release
