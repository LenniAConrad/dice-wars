#!/usr/bin/env bash
# Install Dice Wars on macOS from a downloaded copy of this repository, on a
# Mac that has nothing set up yet: it asks macOS for Apple's Command Line Tools
# (the C compiler and linker) and installs Rust (through rustup) if they are
# missing, builds Dice Wars, installs it as an app in ~/Applications and checks
# it. Open Terminal, go to this folder and run:
#
#   bash install-macos.sh
#
# See --help for options, and docs/INSTALL.md.
# Compatible with macOS's /bin/bash 3.2; no Homebrew, sudo or shell-profile edits.
#
# Every Dice Wars installer prints the same kind of text (install-linux.sh and
# install-windows.ps1 use the same words): a title, five numbered steps,
# indented "doing", "ok", "note", "warn" and "error" lines, and a closing
# SUCCESS or FAILED block that says what happened and what to do next.

set -Eeuo pipefail
export LC_ALL=C

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
GAME_DIR=$SCRIPT_DIR
BUNDLE_ID=io.github.lenniaconrad.dicewars
APP_NAME='Dice Wars'

usage() {
    printf '%s\n' \
        'Usage: bash install-macos.sh [--app-dir PATH] [--prefix PATH] [--jobs N]' \
        '       bash install-macos.sh --uninstall [--app-dir PATH] [--prefix PATH]' \
        '' \
        'Install Dice Wars on this Mac, getting whatever is missing first: Apple'"'"'s' \
        'Command Line Tools (macOS opens an install window; click Install and' \
        'accept the licence, and this script carries on by itself) and Rust' \
        '(through the official rustup, into ~/.cargo). It then builds the game,' \
        'installs it as ~/Applications/Dice Wars.app (find it in Launchpad or' \
        'Spotlight) and links ~/.local/bin/dice-wars to it. Nothing needs installing' \
        'first, and it is safe to run again. Shell profiles are not changed.' \
        '' \
        'Options:' \
        '  --app-dir PATH   Put Dice Wars.app in PATH instead of ~/Applications' \
        '  --prefix PATH    Put the dice-wars terminal command in PATH/bin instead of' \
        '                   ~/.local/bin (spaces are fine)' \
        '  --jobs N         Build with N parallel workers (default 1, or CARGO_BUILD_JOBS)' \
        '  --uninstall      Remove what this installer put in those places' \
        '  -h, --help       Show this help' \
        '' \
        'Environment: DICE_WARS_APPLE_TOOLS_WAIT_SECONDS (default 3600) bounds how long' \
        'to wait for Apple'"'"'s installer window.'
}

PREFIX=${DICE_WARS_INSTALL_PREFIX:-${HOME:-}/.local}
APP_DIR=${HOME:-}/Applications
JOBS=${CARGO_BUILD_JOBS:-1}
UNINSTALL=0
# What to type to run this installer again, and to undo it, in the messages.
RERUN_COMMAND='bash install-macos.sh'
for argument in "$@"; do
    RERUN_COMMAND+=" $(printf '%q' "$argument")"
done
UNINSTALL_COMMAND='bash install-macos.sh --uninstall'
while (($#)); do
    case $1 in
        --prefix | --app-dir | --jobs)
            if (($# < 2)) || [[ -z $2 ]]; then
                printf 'install-macos.sh: %s needs a nonempty value (see --help)\n' "$1" >&2
                exit 2
            fi
            case $1 in
                --prefix)
                    PREFIX=$2
                    UNINSTALL_COMMAND+=" --prefix $(printf '%q' "$2")"
                    ;;
                --app-dir)
                    APP_DIR=$2
                    UNINSTALL_COMMAND+=" --app-dir $(printf '%q' "$2")"
                    ;;
                --jobs) JOBS=$2 ;;
            esac
            shift 2
            ;;
        --uninstall)
            UNINSTALL=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            printf 'install-macos.sh: unknown option: %s (see --help)\n' "$1" >&2
            exit 2
            ;;
    esac
done
[[ $JOBS =~ ^[1-9][0-9]*$ ]] || {
    printf 'install-macos.sh: --jobs (or CARGO_BUILD_JOBS) must be a whole number of at least 1, not: %s\n' "$JOBS" >&2
    exit 2
}
[[ -n ${HOME:-} ]] || {
    printf 'install-macos.sh: HOME is not set, so there is nowhere to install.\n' >&2
    exit 2
}
[[ $PREFIX == /* ]] || PREFIX=$PWD/$PREFIX
[[ $APP_DIR == /* ]] || APP_DIR=$PWD/$APP_DIR
BIN_DIR=$PREFIX/bin
COMMAND_LINK=$BIN_DIR/dice-wars
APP=$APP_DIR/$APP_NAME.app
APP_BINARY=$APP/Contents/MacOS/dice-wars

# ------------------------------------------------------------- message helpers
INSTALLER_TOTAL_STEPS=5
INSTALLER_STEP=0
INSTALLER_STEP_TITLE=
INSTALLER_ERROR_SHOWN=0
INSTALLER_DONE=0
INSTALLER_INSTALLED=
INSTALLER_WORK=
INSTALLER_STARTED=$SECONDS

# The seconds since the installer began, as "42s" or "3m 07s".
installer_elapsed() {
    local total=$((SECONDS - INSTALLER_STARTED))
    if ((total >= 60)); then
        printf '%dm %02ds' $((total / 60)) $((total % 60))
    else
        printf '%ds' "$total"
    fi
}

installer_step() {
    INSTALLER_STEP=$((INSTALLER_STEP + 1))
    INSTALLER_STEP_TITLE=$1
    printf '\n[%d/%d] %s\n' "$INSTALLER_STEP" "$INSTALLER_TOTAL_STEPS" "$1"
}
installer_doing() { printf '      doing: %s\n' "$*"; }
installer_ok() { printf '      ok:    %s\n' "$*"; }
installer_note() { printf '      note:  %s\n' "$*"; }
installer_warn() { printf '      warn:  %s\n' "$*" >&2; }

# installer_die CODE MESSAGE [HINT...]: report why the installer stopped, with
# what the user can do about it, and exit. The EXIT trap prints the FAILED block.
installer_die() {
    local code=$1 hint
    shift
    printf '      error: %s\n' "$1" >&2
    shift
    for hint in "$@"; do
        printf '      hint:  %s\n' "$hint" >&2
    done
    INSTALLER_ERROR_SHOWN=1
    exit "$code"
}

# The closing block of a run that did not finish. It runs from the EXIT trap,
# so an unexpected failure under `set -e` is reported too.
installer_on_exit() {
    local code=$?
    trap - EXIT
    [[ -z $INSTALLER_WORK ]] || rm -rf -- "$INSTALLER_WORK"
    if ((INSTALLER_DONE == 1 || code == 0)); then
        return 0
    fi
    if ((INSTALLER_ERROR_SHOWN == 0)); then
        printf '      error: this step stopped unexpectedly (exit status %d); the lines above show why.\n' \
            "$code" >&2
    fi
    if ((UNINSTALL)); then
        printf '\nFAILED: Dice Wars was NOT fully removed.\n' >&2
    elif [[ -n $INSTALLER_INSTALLED ]]; then
        printf '\nFAILED: Dice Wars was copied to %s but did not pass its checks.\n' \
            "$INSTALLER_INSTALLED" >&2
    else
        printf '\nFAILED: Dice Wars was NOT installed.\n' >&2
    fi
    if ((INSTALLER_STEP > 0)); then
        printf '  It stopped at step %d of %d: %s.\n' \
            "$INSTALLER_STEP" "$INSTALLER_TOTAL_STEPS" "$INSTALLER_STEP_TITLE" >&2
    fi
    printf '  Nothing that was already set up is undone, so after fixing the problem above\n' >&2
    printf '  you can run the same command again and it continues from there:\n' >&2
    printf '    %s\n' "$RERUN_COMMAND" >&2
    printf '  Time spent: %s.\n' "$(installer_elapsed)" >&2
}

# installer_begin TITLE: print the title and arm the closing block.
installer_begin() {
    INSTALLER_WORK=$(mktemp -d "${TMPDIR:-/tmp}/dice-wars-install.XXXXXX") || {
        printf 'error: cannot create a temporary directory under %s\n' "${TMPDIR:-/tmp}" >&2
        exit 1
    }
    trap installer_on_exit EXIT
    printf '%s\n' "$1"
}

# Free megabytes on the filesystem holding a directory, or nothing when unknown.
installer_free_mb() {
    local dir=$1 available
    available=$(df -Pk "$dir" 2>/dev/null | { read -r _; read -r _ _ _ kb _; printf '%s' "${kb:-}"; })
    [[ $available =~ ^[0-9]+$ ]] || return 0
    printf '%d' $((available / 1024))
}

# installer_download URL DESTINATION: fetch over HTTPS to a file, so a failed
# or partial transfer is never executed.
installer_download() {
    curl --proto '=https' --tlsv1.2 --fail --show-error --silent --location \
        --connect-timeout 30 --max-time 300 "$1" -o "$2"
}

# ------------------------------------------------------------ system checks
# Succeeds when Apple's selected developer directory has a macOS SDK and clang;
# sets SDK and CLANG.
apple_tools_ready() {
    SDK=
    CLANG=
    if xcode-select -p >/dev/null 2>&1; then
        SDK=$(xcrun --sdk macosx --show-sdk-path 2>/dev/null || true)
        CLANG=$(xcrun --sdk macosx --find clang 2>/dev/null || true)
    fi
    [[ -d $SDK && -x $CLANG ]]
}

# The minimum Rust version Cargo.toml asks for, as "1.82".
rust_minimum() {
    sed -n 's/^rust-version *= *"\([0-9][0-9.]*\)"$/\1/p' "$GAME_DIR/Cargo.toml"
}

# The game version Cargo.toml declares, as "1.4.3".
game_version() {
    sed -n 's/^version *= *"\([0-9][0-9A-Za-z.+-]*\)"$/\1/p' "$GAME_DIR/Cargo.toml" | head -n 1
}

# Succeeds when cargo and rustc run and rustc is at least MINIMUM ("1.82").
rust_ready() {
    local minimum=$1 version major minor need_major need_minor
    command -v cargo >/dev/null 2>&1 || return 1
    version=$(rustc --version 2>/dev/null) || return 1
    cargo --version >/dev/null 2>&1 || return 1
    [[ $version =~ ^rustc\ ([0-9]+)\.([0-9]+)\. ]] || return 1
    major=${BASH_REMATCH[1]}
    minor=${BASH_REMATCH[2]}
    need_major=${minimum%%.*}
    need_minor=${minimum#*.}
    need_minor=${need_minor%%.*}
    ((major > need_major || (major == need_major && minor >= need_minor)))
}

# Succeeds when PATH is an app bundle this installer made (by its identifier).
is_our_app() {
    [[ -f $1/Contents/Info.plist ]] || return 1
    [[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$1/Contents/Info.plist" 2>/dev/null || true) == "$BUNDLE_ID" ]]
}

# Tells Launchpad and Spotlight about the app. Optional: they notice on their own.
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

# ----------------------------------------------------------------- uninstall
if ((UNINSTALL)); then
    INSTALLER_TOTAL_STEPS=1
    installer_begin 'Dice Wars uninstaller for macOS'
    installer_step 'Remove Dice Wars'
    installer_doing "removing $APP and $COMMAND_LINK"
    REMOVED=0
    if [[ -L $COMMAND_LINK ]]; then
        rm -f -- "$COMMAND_LINK" || installer_die 1 "could not remove $COMMAND_LINK."
        installer_ok "removed $COMMAND_LINK"
        REMOVED=$((REMOVED + 1))
    elif [[ -e $COMMAND_LINK ]]; then
        installer_note "left $COMMAND_LINK in place: it is not the link this installer makes"
    fi
    if [[ -d $APP ]]; then
        is_our_app "$APP" \
            || installer_die 1 "$APP is not the Dice Wars app this installer makes (its identifier is not $BUNDLE_ID), so it was left alone." \
                "Remove it in Finder if you are sure, then run the same command again."
        [[ ! -x $LSREGISTER ]] || "$LSREGISTER" -u "$APP" >/dev/null 2>&1 || true
        rm -rf -- "$APP" \
            || installer_die 1 "could not remove $APP." \
                "Quit Dice Wars if it is running, then run the same command again."
        installer_ok "removed $APP"
        REMOVED=$((REMOVED + 1))
    fi
    ((REMOVED > 0)) \
        || installer_die 1 "nothing was removed: neither $APP nor $COMMAND_LINK was found." \
            "Pass the same --app-dir and --prefix you installed with; the defaults are ~/Applications and ~/.local."
    INSTALLER_DONE=1
    printf '\nSUCCESS: Dice Wars was removed.\n'
    printf '  Your settings, bookmarks and replays in %s are kept;\n' \
        "${XDG_DATA_HOME:-$HOME/.local/share}/dice-wars"
    printf '  delete that folder too to remove them.\n'
    printf '  Apple'"'"'s Command Line Tools and the Rust that the installer added are left in place.\n'
    printf '  Finished in %s.\n' "$(installer_elapsed)"
    exit 0
fi

# ------------------------------------------------------------------- install
for needed in Cargo.toml Cargo.lock src/main.rs assets/icon.png; do
    if [[ ! -f $GAME_DIR/$needed ]]; then
        printf 'install-macos.sh: %s is missing next to this script.\n' "$needed" >&2
        printf 'Download all of Dice Wars from https://github.com/LenniAConrad/dice-wars (Code, then Download ZIP),\n' >&2
        printf 'unzip it, and run install-macos.sh from inside the unzipped folder.\n' >&2
        exit 2
    fi
done
RUST_MINIMUM=$(rust_minimum)
[[ $RUST_MINIMUM =~ ^[0-9]+\.[0-9]+ ]] || RUST_MINIMUM=1.82
VERSION=$(game_version)
[[ -n $VERSION ]] || VERSION=1.0.0

installer_begin 'Dice Wars installer for macOS'
printf 'This will check this Mac, get Apple'"'"'s Command Line Tools and Rust if they are\n'
printf 'missing, build Dice Wars, and install it as %s.\n' "$APP"
printf 'Nothing needs installing first.\n'

# ---------------------------------------------------------------- 1. computer
installer_step 'Check this computer'
[[ $(uname -s) == Darwin ]] \
    || installer_die 2 "this installer is for macOS; this computer runs $(uname -s)." \
        "On Linux run: bash install-linux.sh" \
        "On Windows double-click install-windows.cmd"
MACHINE=$(uname -m)
case $MACHINE in
    arm64) HOST=aarch64-apple-darwin ;;
    x86_64)
        [[ $(sysctl -in sysctl.proc_translated 2>/dev/null || true) != 1 ]] \
            || installer_die 2 "this Terminal is running under Rosetta, which would build an Intel game on an Apple-silicon Mac." \
                "Open a native Terminal (Get Info on Terminal.app, untick 'Open using Rosetta') and run the same command again."
        HOST=x86_64-apple-darwin
        ;;
    *) installer_die 2 "unsupported processor: $MACHINE." \
        "Dice Wars builds on Apple silicon (arm64) and Intel (x86_64) Macs." ;;
esac
MAC_VERSION=$(sw_vers -productVersion)
[[ $MAC_VERSION =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] \
    || installer_die 2 "unrecognized macOS version: $MAC_VERSION"
IFS=. read -r MAC_MAJOR MAC_MINOR _ <<< "$MAC_VERSION"
# The oldest macOS that current Rust builds for on each processor.
if [[ $MACHINE == arm64 ]]; then
    MIN_MACOS=11.0
    ((MAC_MAJOR >= 11)) \
        || installer_die 2 "Apple silicon needs macOS 11 or newer; this Mac runs macOS $MAC_VERSION." \
            "Update macOS in System Settings, then run the same command again."
else
    MIN_MACOS=10.12
    ((MAC_MAJOR > 10 || (MAC_MAJOR == 10 && MAC_MINOR >= 12))) \
        || installer_die 2 "Intel builds need macOS 10.12 or newer; this Mac runs macOS $MAC_VERSION." \
            "Update macOS in System Settings, then run the same command again."
fi
[[ $EUID -ne 0 ]] \
    || installer_die 2 "this installer was started as root (sudo), so it would install into root's home, not yours." \
        "Run it as yourself, without sudo: $RERUN_COMMAND"
[[ -d $HOME && -w $HOME ]] \
    || installer_die 2 "HOME ($HOME) is not a writable directory, so there is nowhere to install Rust."
installer_ok "macOS $MAC_VERSION, $MACHINE (build target $HOST)"
FREE_MB=$(installer_free_mb "$HOME")
if [[ -n $FREE_MB ]]; then
    if ((FREE_MB < 1024)); then
        installer_die 2 "only $FREE_MB MB are free where Rust is installed ($HOME); about 1024 MB are needed." \
            "Free some disk space, then run the same command again."
    fi
    installer_ok "free disk space: $FREE_MB MB"
fi

# ----------------------------------------------------------------- 2. tools
installer_step 'Install build tools'
if apple_tools_ready; then
    installer_ok "Apple Command Line Tools are installed"
else
    installer_note "Apple's Command Line Tools (the C compiler and linker) are not installed"
    installer_doing "asking macOS to install them"
    installer_note "a window named 'Install Command Line Developer Tools' should open: click Install and accept the licence"
    xcode-select --install > /dev/null 2>&1 || true
    WAIT_LIMIT=${DICE_WARS_APPLE_TOOLS_WAIT_SECONDS:-3600}
    POLL=${DICE_WARS_APPLE_TOOLS_POLL_SECONDS:-5}
    [[ $WAIT_LIMIT =~ ^[0-9]+$ ]] || WAIT_LIMIT=3600
    [[ $POLL =~ ^[1-9][0-9]*$ ]] || POLL=5
    WAITED_FROM=$SECONDS
    NEXT_REPORT=60
    installer_doing "waiting for Apple's installer to finish (this script carries on by itself)"
    until apple_tools_ready; do
        WAITED=$((SECONDS - WAITED_FROM))
        if ((WAITED >= WAIT_LIMIT)); then
            installer_die 3 "Apple's tools were not installed within $((WAIT_LIMIT / 60)) minutes." \
                "Finish Apple's installation window if it is still open, then rerun this same command." \
                "No window? On a Mac you reach only over SSH, install the Command Line Tools from https://developer.apple.com/download/all/ or select a full Xcode with xcode-select, then rerun this same command."
        fi
        if ((WAITED >= NEXT_REPORT)); then
            installer_note "still waiting for Apple's installer ($((WAITED / 60)) min so far)"
            NEXT_REPORT=$((NEXT_REPORT + 60))
        fi
        sleep "$POLL"
    done
    installer_ok "Apple Command Line Tools are now installed"
fi
printf 'int main(void) { return 0; }\n' > "$INSTALLER_WORK/link.c"
"$CLANG" -arch "$MACHINE" -isysroot "$SDK" "$INSTALLER_WORK/link.c" -o "$INSTALLER_WORK/link" \
    -framework Cocoa \
    || installer_die 1 "Apple's compiler, linker or SDK failed a test build." \
        "If a message above mentions the Xcode licence, run: sudo xcodebuild -license accept" \
        "Otherwise resolve the message above, then rerun this same command."
installer_ok "Apple compiler and linker work (SDK: $SDK)"
for tool in sips iconutil codesign; do
    command -v "$tool" >/dev/null 2>&1 \
        || installer_note "$tool was not found; the app is made without what it provides"
done

# ------------------------------------------------------------------ 3. rust
installer_step 'Install Rust'
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
[[ $CARGO_HOME == /* ]] || CARGO_HOME="$PWD/$CARGO_HOME"
export CARGO_HOME
# First-run PATH takes effect now; no profile edits.
PATH="$CARGO_HOME/bin:$PATH"
export PATH
if command -v rustup >/dev/null 2>&1; then
    installer_ok "rustup is already installed: $(command -v rustup)"
    if ! rust_ready "$RUST_MINIMUM"; then
        installer_doing "installing the stable Rust toolchain (a download of a few hundred MB)"
        rustup toolchain install stable --profile minimal --no-self-update \
            || installer_die 1 "could not install the stable Rust toolchain." \
                "Check your internet connection (static.rust-lang.org must be reachable), then run the same command again."
        export RUSTUP_TOOLCHAIN=stable
    fi
elif rust_ready "$RUST_MINIMUM"; then
    installer_ok "using the Rust that is already installed"
else
    if command -v cargo >/dev/null 2>&1; then
        installer_note "the installed Rust is older than $RUST_MINIMUM, which Dice Wars needs; installing rustup next to it"
    fi
    installer_doing "downloading the official Rust installer (rustup) from https://sh.rustup.rs"
    installer_download https://sh.rustup.rs "$INSTALLER_WORK/rustup-init.sh" \
        || installer_die 1 "could not download rustup from https://sh.rustup.rs." \
            "Check your internet connection (and any proxy or firewall), then run the same command again."
    installer_doing "installing rustup and stable Rust into $CARGO_HOME (your shell profile is not changed)"
    sh "$INSTALLER_WORK/rustup-init.sh" -y --no-modify-path --profile minimal \
        --default-toolchain stable --default-host "$HOST" \
        || installer_die 1 "the rustup installer failed; its messages are above." \
            "Run the same command again; it is safe to repeat."
    command -v rustup >/dev/null 2>&1 \
        || installer_die 1 "rustup installed but is not usable ($CARGO_HOME/bin/rustup is missing)."
fi
rust_ready "$RUST_MINIMUM" \
    || installer_die 1 "Rust $RUST_MINIMUM or newer is still not usable." \
        "Try: rustup update stable, then run the same command again."
RUST_HOST=$(rustc -vV | sed -n 's/^host: //p')
[[ $RUST_HOST == "$HOST" ]] \
    || installer_die 1 "this Rust builds for $RUST_HOST, not for this Mac ($HOST)." \
        "Run: rustup toolchain install stable-$HOST, then: rustup default stable-$HOST, then run the same command again."
# An inherited target would put the build somewhere this script does not look.
unset CARGO_BUILD_TARGET
installer_ok "$(rustc --version) ready: $(command -v cargo)"

# ----------------------------------------------------------------- 4. build
installer_step 'Build and install Dice Wars'
if ((JOBS == 1)); then
    installer_doing "building the game with 1 build worker, so the Mac stays responsive (usually 1 to 3 minutes)"
else
    installer_doing "building the game with $JOBS build workers"
fi
(cd -- "$GAME_DIR" && cargo build --release --locked -j "$JOBS") \
    || installer_die 1 "the build did not finish; its messages are above." \
        "Run the same command again; if it fails the same way, report it at https://github.com/LenniAConrad/dice-wars/issues with those messages."
TARGET_DIR=${CARGO_TARGET_DIR:-$GAME_DIR/target}
[[ $TARGET_DIR == /* ]] || TARGET_DIR=$GAME_DIR/$TARGET_DIR
BUILT=$TARGET_DIR/release/dicegame
[[ -x $BUILT ]] || installer_die 1 "the build finished but $BUILT does not exist."
installer_ok "built $BUILT"

# Rust builds for MIN_MACOS unless MACOSX_DEPLOYMENT_TARGET asks otherwise.
MIN_MACOS=${MACOSX_DEPLOYMENT_TARGET:-$MIN_MACOS}

# Assemble the app in the work folder, then move it into place in one go.
STAGE=$INSTALLER_WORK/$APP_NAME.app
mkdir -p "$STAGE/Contents/MacOS" "$STAGE/Contents/Resources"
cp "$BUILT" "$STAGE/Contents/MacOS/dice-wars"
chmod 755 "$STAGE/Contents/MacOS/dice-wars"
ICON_KEY=
ICONSET=$INSTALLER_WORK/DiceWars.iconset
if command -v sips >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1; then
    installer_doing "making the app icon from assets/icon.png"
    mkdir -p "$ICONSET"
    ICON_OK=1
    # name:pixels; assets/icon.png is 256x256, the largest size made from it.
    for entry in 16x16:16 16x16@2x:32 32x32:32 32x32@2x:64 128x128:128 128x128@2x:256 256x256:256; do
        sips -z "${entry#*:}" "${entry#*:}" "$GAME_DIR/assets/icon.png" \
            --out "$ICONSET/icon_${entry%%:*}.png" >/dev/null 2>&1 || ICON_OK=0
    done
    if ((ICON_OK)) && iconutil -c icns "$ICONSET" -o "$STAGE/Contents/Resources/DiceWars.icns" >/dev/null 2>&1; then
        ICON_KEY='DiceWars'
        installer_ok "app icon made"
    else
        installer_warn "could not make the app icon; the app gets the generic icon"
    fi
else
    installer_warn "sips or iconutil is missing; the app gets the generic icon"
fi
{
    printf '%s\n' \
        '<?xml version="1.0" encoding="UTF-8"?>' \
        '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
        '<plist version="1.0">' \
        '<dict>' \
        '    <key>CFBundleDevelopmentRegion</key>' \
        '    <string>en</string>' \
        '    <key>CFBundleDisplayName</key>' \
        "    <string>$APP_NAME</string>" \
        '    <key>CFBundleExecutable</key>' \
        '    <string>dice-wars</string>' \
        '    <key>CFBundleIdentifier</key>' \
        "    <string>$BUNDLE_ID</string>" \
        '    <key>CFBundleInfoDictionaryVersion</key>' \
        '    <string>6.0</string>' \
        '    <key>CFBundleName</key>' \
        "    <string>$APP_NAME</string>" \
        '    <key>CFBundlePackageType</key>' \
        '    <string>APPL</string>' \
        '    <key>CFBundleShortVersionString</key>' \
        "    <string>$VERSION</string>" \
        '    <key>CFBundleVersion</key>' \
        "    <string>$VERSION</string>" \
        '    <key>LSApplicationCategoryType</key>' \
        '    <string>public.app-category.strategy-games</string>' \
        '    <key>LSMinimumSystemVersion</key>' \
        "    <string>$MIN_MACOS</string>" \
        '    <key>NSHighResolutionCapable</key>' \
        '    <true/>' \
        '    <key>NSPrincipalClass</key>' \
        '    <string>NSApplication</string>'
    if [[ -n $ICON_KEY ]]; then
        printf '%s\n' \
            '    <key>CFBundleIconFile</key>' \
            "    <string>$ICON_KEY</string>"
    fi
    printf '%s\n' '</dict>' '</plist>'
} > "$STAGE/Contents/Info.plist"
printf 'APPL????' > "$STAGE/Contents/PkgInfo"
if command -v plutil >/dev/null 2>&1; then
    plutil -lint "$STAGE/Contents/Info.plist" >/dev/null \
        || installer_die 1 "the app's Info.plist is not valid; this is a bug in install-macos.sh."
fi
# Files copied from a downloaded ZIP may carry the quarantine flag, which
# would make macOS refuse to open the app.
xattr -cr "$STAGE" 2>/dev/null || true
# Apple silicon runs only signed code; sign the whole app ad hoc (no Apple
# account needed) so the bundle and its executable agree.
if command -v codesign >/dev/null 2>&1; then
    if codesign --force --sign - --identifier "$BUNDLE_ID" "$STAGE" >/dev/null 2>&1; then
        installer_ok "app signed for this Mac (ad hoc)"
    else
        installer_warn "could not sign the app; macOS may refuse to open it"
    fi
fi

mkdir -p -- "$APP_DIR" \
    || installer_die 1 "could not create $APP_DIR." \
        "Choose an --app-dir you can write to, then run the same command again."
if [[ -e $APP ]]; then
    is_our_app "$APP" \
        || installer_die 1 "$APP already exists and is not the Dice Wars app this installer makes, so it was left alone." \
            "Move it away, or pass a different --app-dir, then run the same command again."
    rm -rf -- "$APP" \
        || installer_die 1 "could not replace $APP." \
            "Quit Dice Wars if it is running, then run the same command again."
fi
mv -- "$STAGE" "$APP" || installer_die 1 "could not move the app to $APP."
INSTALLER_INSTALLED=$APP
installer_ok "installed $APP"
[[ ! -x $LSREGISTER ]] || "$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true

mkdir -p -- "$BIN_DIR" \
    || installer_die 1 "could not create $BIN_DIR." \
        "Choose a --prefix you can write to, then run the same command again."
if [[ -e $COMMAND_LINK && ! -L $COMMAND_LINK ]]; then
    installer_warn "$COMMAND_LINK exists and is not a link, so it was left alone; start the game from $APP"
else
    ln -sfn -- "$APP_BINARY" "$COMMAND_LINK" \
        || installer_die 1 "could not link $COMMAND_LINK to the app."
    installer_ok "linked $COMMAND_LINK -> $APP_BINARY"
fi

# ---------------------------------------------------------------- 5. verify
installer_step 'Check the installed game'
[[ -x $APP_BINARY ]] || installer_die 1 "the game file is missing after the install: $APP_BINARY"
ARCHS=$(lipo -archs "$APP_BINARY" 2>/dev/null || true)
if [[ -n $ARCHS ]]; then
    [[ " $ARCHS " == *" $MACHINE "* ]] \
        || installer_die 1 "the game was built for $ARCHS, not for this Mac ($MACHINE)."
    installer_ok "the game is a native $MACHINE program"
fi
if command -v plutil >/dev/null 2>&1; then
    plutil -lint "$APP/Contents/Info.plist" >/dev/null \
        || installer_die 1 "the installed app's Info.plist is not valid."
    installer_ok "the app's Info.plist is valid"
fi
if command -v codesign >/dev/null 2>&1; then
    if codesign --verify "$APP" >/dev/null 2>&1; then
        installer_ok "the app's signature checks out"
    else
        installer_warn "codesign could not verify $APP; if macOS refuses to open it, right-click it and choose Open"
    fi
fi

INSTALLER_DONE=1
printf '\nSUCCESS: Dice Wars is installed and passed its checks.\n'
printf '  App:          %s\n' "$APP"
printf '  Start it:     from Launchpad or Spotlight (type Dice Wars), or run: open -a "%s"\n' "$APP"
if [[ -L $COMMAND_LINK ]]; then
    printf '  Terminal:     %s\n' "$COMMAND_LINK"
    case ":${PATH:-}:" in
        *":$BIN_DIR:"*) ;;
        *)
            printf '                to type just "dice-wars", add this line to your shell profile:\n'
            printf '%s\n' "                export PATH=\"$BIN_DIR:\$PATH\""
            ;;
    esac
fi
printf '  Uninstall:    %s\n' "$UNINSTALL_COMMAND"
printf '  Finished in %s.\n' "$(installer_elapsed)"
