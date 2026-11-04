#!/usr/bin/env bash
# Every automated check, headless. Nothing here opens a window: DISPLAY and
# WAYLAND_DISPLAY are removed and DW_FORBID_WINDOW makes any accidental
# windowed start fail. Run from anywhere:  scripts/check.sh [--quick]
#   --quick   unit tests and cross-platform type checks only
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
unset DISPLAY WAYLAND_DISPLAY
export DW_FORBID_WINDOW=1
jobs="${CARGO_BUILD_JOBS:-1}"

step() { printf '\n== %s\n' "$*"; }

step "unit tests"
cargo test --release -j "$jobs" -- --test-threads=1

step "cross-platform type checks"
for target in x86_64-pc-windows-gnu aarch64-apple-darwin x86_64-apple-darwin; do
    if rustup target list --installed 2>/dev/null | grep -qx "$target"; then
        cargo check --release -j "$jobs" --all-targets --target "$target"
    else
        echo "skipped $target (rustup target add $target)"
    fi
done

[[ "${1:-}" == "--quick" ]] && { echo; echo "SUCCESS (quick)"; exit 0; }

step "release build"
cargo build --release -j "$jobs"

step "screenshot audit (headless)"
if python3 -c 'import PIL' 2>/dev/null; then
    python3 scripts/audit-ui.py
else
    echo "skipped: pip install pillow"
fi

step "frame-time audit (headless)"
python3 scripts/audit-runtime.py

step "audio audit (headless, temporary sink)"
if command -v pactl >/dev/null && command -v parec >/dev/null && pactl info >/dev/null 2>&1; then
    python3 scripts/audit-audio.py
else
    echo "skipped: needs a PulseAudio/PipeWire server with pactl and parec"
fi

echo
echo "SUCCESS"
