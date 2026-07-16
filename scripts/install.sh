#!/usr/bin/env bash
# Build and install Dice Wars on Linux: binary in ~/.local/bin plus a
# desktop entry. Installs the Rust toolchain first if it is missing.
set -euo pipefail
cd "$(dirname "$0")/.."

./scripts/build.sh

BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"
rm -f "$BIN_DIR/dice-wars"
cp target/release/dicegame "$BIN_DIR/dice-wars"
echo "Installed binary: $BIN_DIR/dice-wars"

ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons"
mkdir -p "$ICON_DIR"
cp assets/icon.png "$ICON_DIR/dice-wars.png"

APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APP_DIR"
cat > "$APP_DIR/dice-wars.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Dice Wars
Comment=Pastel dice conquest
Exec=$BIN_DIR/dice-wars
Icon=$ICON_DIR/dice-wars.png
Terminal=false
Categories=Game;StrategyGame;
DESKTOP
update-desktop-database "$APP_DIR" 2>/dev/null || true
gtk-update-icon-cache 2>/dev/null || true
echo "Desktop entry installed. Launch 'Dice Wars' from your app menu or run: dice-wars"
