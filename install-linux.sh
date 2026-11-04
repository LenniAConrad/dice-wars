#!/usr/bin/env bash
# Install Dice Wars on Linux from a downloaded copy of this repository, on a
# computer that has nothing set up yet: it installs the C build tools and the
# game's X11 and sound libraries (through the system package manager) and Rust
# (through rustup) if they are missing, builds Dice Wars, installs it with an
# app-menu entry and checks it. Run it from this folder:
#
#   bash install-linux.sh
#
# See --help for options, and docs/INSTALL.md.
#
# Every Dice Wars installer prints the same kind of text (install-macos.sh and
# install-windows.ps1 use the same words): a title, five numbered steps,
# indented "doing", "ok", "note", "warn" and "error" lines, and a closing
# SUCCESS or FAILED block that says what happened and what to do next.

# `sh install-linux.sh` starts dash, which lacks what this script uses.
if [ -z "${BASH_VERSION:-}" ]; then
    command -v bash >/dev/null 2>&1 || {
        echo "install-linux.sh needs bash, which is not installed; install bash and run: bash install-linux.sh" >&2
        exit 2
    }
    exec bash "$0" "$@"
fi

set -Eeuo pipefail
export LC_ALL=C

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
GAME_DIR=$SCRIPT_DIR

usage() {
    printf '%s\n' \
        'Usage: bash install-linux.sh [--prefix PATH] [--jobs N] [--skip-deps]' \
        '       bash install-linux.sh --uninstall [--prefix PATH]' \
        '' \
        'Install Dice Wars on this Linux computer, getting whatever is missing first:' \
        'a C compiler and linker, curl, and the X11 and PulseAudio client libraries' \
        '(through apt, dnf, yum, pacman or zypper, which asks for your sudo password)' \
        'and Rust (through the official rustup, into ~/.cargo). It then builds the' \
        'game and installs it to ~/.local/bin/dice-wars with an icon and an app-menu' \
        'entry. Nothing else needs installing first, and it is safe to run again.' \
        '' \
        'Options:' \
        '  --prefix PATH    Install under PATH instead of ~/.local' \
        '  --jobs N         Build with N parallel workers (default 1, or CARGO_BUILD_JOBS)' \
        '  --skip-deps      Do not use the package manager or sudo; only check that the' \
        '                   C tools and libraries are already there' \
        '  --uninstall      Remove what this installer put under the prefix' \
        '  -h, --help       Show this help' \
        '' \
        'One build worker keeps the desktop responsive while the game compiles; pass' \
        '--jobs 4 (or set CARGO_BUILD_JOBS=4) for a faster build.'
}

PREFIX=${DICE_WARS_INSTALL_PREFIX:-}
PREFIX_GIVEN=0
[[ -z $PREFIX ]] || PREFIX_GIVEN=1
JOBS=${CARGO_BUILD_JOBS:-1}
SKIP_DEPS=0
UNINSTALL=0
# What to type to run this installer again, and to undo it, in the messages.
RERUN_COMMAND='bash install-linux.sh'
for argument in "$@"; do
    RERUN_COMMAND+=" $(printf '%q' "$argument")"
done
UNINSTALL_COMMAND='bash install-linux.sh --uninstall'
while (($#)); do
    case $1 in
        --prefix | --jobs)
            if (($# < 2)) || [[ -z $2 ]]; then
                printf 'install-linux.sh: %s needs a value (see --help)\n' "$1" >&2
                exit 2
            fi
            if [[ $1 == --prefix ]]; then
                PREFIX=$2
                PREFIX_GIVEN=1
                UNINSTALL_COMMAND+=" --prefix $(printf '%q' "$2")"
            else
                JOBS=$2
            fi
            shift 2
            ;;
        --skip-deps)
            SKIP_DEPS=1
            shift
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
            printf 'install-linux.sh: unknown option: %s (see --help)\n' "$1" >&2
            exit 2
            ;;
    esac
done
[[ $JOBS =~ ^[1-9][0-9]*$ ]] || {
    printf 'install-linux.sh: --jobs (or CARGO_BUILD_JOBS) must be a whole number of at least 1, not: %s\n' "$JOBS" >&2
    exit 2
}

[[ -n ${HOME:-} ]] || {
    printf 'install-linux.sh: HOME is not set, so there is nowhere to install.\n' >&2
    exit 2
}
[[ -n $PREFIX ]] || PREFIX=$HOME/.local
[[ $PREFIX == /* ]] || PREFIX=$PWD/$PREFIX
BIN_DIR=$PREFIX/bin
# Desktop files go where the desktop looks for them: the XDG data directory for
# the default prefix, PREFIX/share for a chosen one.
if ((PREFIX_GIVEN)); then
    DATA_DIR=$PREFIX/share
else
    DATA_DIR=${XDG_DATA_HOME:-$HOME/.local/share}
fi
BINARY=$BIN_DIR/dice-wars
ICON=$DATA_DIR/icons/hicolor/256x256/apps/dice-wars.png
DESKTOP_FILE=$DATA_DIR/applications/dice-wars.desktop
# Where scripts/install.sh (before these installers) put the icon.
LEGACY_ICON=$DATA_DIR/icons/dice-wars.png

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
# or partial transfer is never executed. Returns 127 when no downloader exists.
installer_download() {
    local url=$1 destination=$2
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 --fail --show-error --silent --location \
            --connect-timeout 30 --max-time 300 "$url" -o "$destination"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --quiet --tries=3 --timeout=30 -O "$destination" "$url"
    else
        return 127
    fi
}

# ------------------------------------------------------------ system checks
# Runs a command as root: directly when this is root, else through sudo.
run_privileged() {
    if [[ $EUID -eq 0 ]]; then
        "$@"
    else
        sudo "$@"
    fi
}

# Succeeds when a program can be compiled and linked with the system C
# compiler, which is how Rust links. Extra arguments are added to the link,
# e.g. -l:libX11.so.6 to check a library the game links against.
c_toolchain_links() {
    command -v cc >/dev/null 2>&1 || return 1
    printf 'int main(void) { return 0; }\n' > "$INSTALLER_WORK/link.c"
    cc "$INSTALLER_WORK/link.c" -o "$INSTALLER_WORK/link" "$@" > /dev/null 2>&1
}

# has_library FILE: succeeds when the shared library FILE (e.g. libX11.so.6) can
# be linked against; without a working compiler, asks the dynamic linker cache.
has_library() {
    local file=$1 ldconfig
    if c_toolchain_links; then
        c_toolchain_links "-l:$file"
        return
    fi
    ldconfig=$(command -v ldconfig || true)
    [[ -n $ldconfig ]] || { [[ -x /sbin/ldconfig ]] && ldconfig=/sbin/ldconfig; } || return 1
    "$ldconfig" -p 2>/dev/null | grep -qF " $file "
}

has_downloader() {
    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1
}

# Succeeds when HTTPS has a bundle of certificate authorities to check against.
has_ca_bundle() {
    local file
    for file in /etc/ssl/certs/ca-certificates.crt /etc/pki/tls/certs/ca-bundle.crt \
        /etc/ssl/ca-bundle.pem /etc/ssl/cert.pem "${SSL_CERT_FILE:-}" "${CURL_CA_BUNDLE:-}"; do
        [[ -n $file && -s $file ]] && return 0
    done
    return 1
}

# The package manager this system uses, or nothing.
detect_package_manager() {
    local manager
    for manager in apt-get dnf yum pacman zypper; do
        if command -v "$manager" >/dev/null 2>&1; then
            printf '%s' "$manager"
            return 0
        fi
    done
    return 0
}

# package_name MANAGER WHAT: the package that provides WHAT (compiler, x11,
# pulse, curl or certificates) on this package manager.
package_name() {
    case $1:$2 in
        apt-get:compiler) printf 'build-essential' ;;
        *:compiler) printf 'gcc' ;;
        apt-get:x11) printf 'libx11-6' ;;
        dnf:x11 | yum:x11) printf 'libX11' ;;
        pacman:x11) printf 'libx11' ;;
        zypper:x11) printf 'libX11-6' ;;
        apt-get:pulse | zypper:pulse) printf 'libpulse0' ;;
        dnf:pulse | yum:pulse) printf 'pulseaudio-libs' ;;
        pacman:pulse) printf 'libpulse' ;;
        *:curl) printf 'curl' ;;
        *:certificates) printf 'ca-certificates' ;;
    esac
}

# What this computer is called, for the messages.
describe_system() {
    local name=Linux
    if [[ -r /etc/os-release ]]; then
        # shellcheck disable=SC1091  # the distribution's own file
        name=$( . /etc/os-release && printf '%s' "${PRETTY_NAME:-Linux}")
    fi
    printf '%s' "$name"
}

# The minimum Rust version Cargo.toml asks for, as "1.82".
rust_minimum() {
    sed -n 's/^rust-version *= *"\([0-9][0-9.]*\)"$/\1/p' "$GAME_DIR/Cargo.toml"
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

# Removes FILE if it exists and counts it; fails loudly if it cannot.
REMOVED=0
remove_file() {
    local file=$1
    if [[ -e $file || -L $file ]]; then
        rm -f -- "$file" \
            || installer_die 1 "could not remove $file." \
                "Check that you own it (try: ls -l '$file'), then run the same command again."
        installer_ok "removed $file"
        REMOVED=$((REMOVED + 1))
    fi
}

# Tells the desktop that the app-menu entry and icon changed. Optional: menus
# also notice on their own, a little later.
refresh_desktop_caches() {
    if command -v update-desktop-database >/dev/null 2>&1 && [[ -d $DATA_DIR/applications ]]; then
        update-desktop-database -q "$DATA_DIR/applications" >/dev/null 2>&1 || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1 && [[ -d $DATA_DIR/icons/hicolor ]]; then
        gtk-update-icon-cache -q -t -f "$DATA_DIR/icons/hicolor" >/dev/null 2>&1 || true
    fi
}

# ----------------------------------------------------------------- uninstall
if ((UNINSTALL)); then
    INSTALLER_TOTAL_STEPS=1
    installer_begin 'Dice Wars uninstaller for Linux'
    installer_step 'Remove Dice Wars'
    installer_doing "removing what the installer put under $PREFIX"
    remove_file "$BINARY"
    remove_file "$DESKTOP_FILE"
    remove_file "$ICON"
    remove_file "$LEGACY_ICON"
    ((REMOVED > 0)) \
        || installer_die 1 "nothing was removed: no Dice Wars files were found under $PREFIX." \
            "Pass the same --prefix you installed with; with the default prefix it is ~/.local."
    refresh_desktop_caches
    INSTALLER_DONE=1
    printf '\nSUCCESS: Dice Wars was removed from %s.\n' "$PREFIX"
    printf '  Your settings, bookmarks and replays in %s are kept;\n' \
        "${XDG_DATA_HOME:-$HOME/.local/share}/dice-wars"
    printf '  delete that folder too to remove them.\n'
    printf '  The C tools, libraries and Rust that the installer added are left in place.\n'
    printf '  Finished in %s.\n' "$(installer_elapsed)"
    exit 0
fi

# ------------------------------------------------------------------- install
for needed in Cargo.toml Cargo.lock src/main.rs assets/icon.png; do
    if [[ ! -f $GAME_DIR/$needed ]]; then
        printf 'install-linux.sh: %s is missing next to this script.\n' "$needed" >&2
        printf 'Download all of Dice Wars from https://github.com/LenniAConrad/dice-wars (Code, then Download ZIP),\n' >&2
        printf 'unzip it, and run install-linux.sh from inside the unzipped folder.\n' >&2
        exit 2
    fi
done
RUST_MINIMUM=$(rust_minimum)
[[ $RUST_MINIMUM =~ ^[0-9]+\.[0-9]+ ]] || RUST_MINIMUM=1.82

installer_begin 'Dice Wars installer for Linux'
printf 'This will check this computer, install the C build tools, the X11 and sound\n'
printf 'libraries and Rust if they are missing, build Dice Wars, and install it to\n'
printf '%s with an app-menu entry. Nothing needs installing first.\n' "$BINARY"

# ---------------------------------------------------------------- 1. computer
installer_step 'Check this computer'
[[ $(uname -s) == Linux ]] \
    || installer_die 2 "this installer is for Linux; this computer runs $(uname -s)." \
        "On a Mac run: bash install-macos.sh" \
        "On Windows double-click install-windows.cmd"
MACHINE=$(uname -m)
[[ ! -f /etc/alpine-release ]] \
    || installer_die 2 "Alpine Linux (musl) is not supported by this installer." \
        "Use a glibc Linux such as Ubuntu, Debian, Mint, Fedora, Arch or openSUSE."
if [[ $EUID -eq 0 && -n ${SUDO_USER:-} ]]; then
    installer_die 2 "this installer was started with sudo, so it would install into root's home, not yours." \
        "Run it as yourself: $RERUN_COMMAND" \
        "It asks for your sudo password itself, only to install system packages."
fi
[[ -d $HOME && -w $HOME ]] \
    || installer_die 2 "HOME ($HOME) is not a writable directory, so there is nowhere to install Rust."
case $BINARY$ICON in
    *[\"\`\$\\%]*)
        installer_die 2 "the install path $PREFIX contains one of \" \` \$ \\ %, which an app-menu entry cannot point to." \
            "Choose another --prefix."
        ;;
esac
PACKAGE_MANAGER=$(detect_package_manager)
installer_ok "$(describe_system), $MACHINE"
if ((SKIP_DEPS)); then
    installer_note "--skip-deps: the package manager and sudo will not be used"
elif [[ -n $PACKAGE_MANAGER ]]; then
    installer_ok "package manager: $PACKAGE_MANAGER"
else
    installer_note "no supported package manager (apt, dnf, yum, pacman, zypper) was found"
fi
FREE_MB=$(installer_free_mb "$HOME")
if [[ -n $FREE_MB ]]; then
    if ((FREE_MB < 1024)); then
        installer_die 2 "only $FREE_MB MB are free where Rust is installed ($HOME); about 1024 MB are needed." \
            "Free some disk space, then run the same command again."
    fi
    installer_ok "free disk space: $FREE_MB MB"
fi
if [[ -z ${DISPLAY:-} && -z ${WAYLAND_DISPLAY:-} ]]; then
    installer_note "no desktop session was detected here; the game is installed anyway and runs from a desktop"
fi

# ----------------------------------------------------------------- 2. tools
installer_step 'Install build tools and libraries'
NEED=()
c_toolchain_links || NEED+=(compiler)
has_library libX11.so.6 || NEED+=(x11)
has_library libpulse-simple.so.0 || NEED+=(pulse)
has_downloader || NEED+=(curl)
has_ca_bundle || NEED+=(certificates)
report_tools() {
    installer_ok "C compiler and linker: $(command -v cc)"
    installer_ok "X11 client library: libX11.so.6"
    installer_ok "PulseAudio client library (also used with PipeWire): libpulse-simple.so.0"
    if has_downloader; then
        installer_ok "download tool: $(command -v curl || command -v wget)"
    fi
}
if ((${#NEED[@]} == 0)); then
    report_tools
else
    for what in "${NEED[@]}"; do
        case $what in
            compiler) installer_note "no working C compiler and linker (cc) was found" ;;
            x11) installer_note "the X11 client library (libX11.so.6) was not found" ;;
            pulse) installer_note "the PulseAudio client library (libpulse-simple.so.0) was not found" ;;
            curl) installer_note "neither curl nor wget was found" ;;
            certificates) installer_note "no certificate authority bundle was found, which HTTPS needs" ;;
        esac
    done
    if ((SKIP_DEPS)); then
        installer_die 2 "--skip-deps was given, so the missing pieces above were not installed." \
            "Install them yourself (on Debian, Ubuntu or Mint: sudo apt-get install build-essential libx11-6 libpulse0 curl ca-certificates)," \
            "or run without --skip-deps to let the installer do it."
    fi
    [[ -n $PACKAGE_MANAGER ]] \
        || installer_die 2 "the missing pieces cannot be installed automatically on this system (no apt, dnf, yum, pacman or zypper)." \
            "Install a C compiler and linker (gcc with cc), the X11 client library (libX11.so.6)," \
            "the PulseAudio client library (libpulse-simple.so.0), curl and ca-certificates yourself," \
            "then run: bash install-linux.sh --skip-deps"
    PACKAGES=()
    for what in "${NEED[@]}"; do
        PACKAGES+=("$(package_name "$PACKAGE_MANAGER" "$what")")
    done
    if [[ $EUID -ne 0 ]] && ! command -v sudo >/dev/null 2>&1; then
        installer_die 2 "installing ${PACKAGES[*]} needs administrator rights, and sudo is not available." \
            "Ask an administrator to install: ${PACKAGES[*]}" \
            "Then run: bash install-linux.sh --skip-deps"
    fi
    if [[ $EUID -ne 0 ]]; then
        installer_note "sudo will ask for your password to install: ${PACKAGES[*]}"
    fi
    case $PACKAGE_MANAGER in
        apt-get)
            installer_doing "apt-get update"
            run_privileged apt-get update \
                || installer_die 1 "apt-get update failed; its messages are above." \
                    "Check your internet connection and that you can use sudo, then run the same command again."
            installer_doing "apt-get install ${PACKAGES[*]}"
            run_privileged env DEBIAN_FRONTEND=noninteractive apt-get install -y \
                --no-install-recommends "${PACKAGES[@]}" \
                || installer_die 1 "apt-get could not install ${PACKAGES[*]}; its messages are above."
            ;;
        dnf | yum)
            installer_doing "$PACKAGE_MANAGER install ${PACKAGES[*]}"
            run_privileged "$PACKAGE_MANAGER" install -y "${PACKAGES[@]}" \
                || installer_die 1 "$PACKAGE_MANAGER could not install ${PACKAGES[*]}; its messages are above."
            ;;
        pacman)
            # No -y: syncing the package list without upgrading the system is
            # a partial upgrade, which Arch does not support. A full -Syu is
            # the user's call, so a stale package list is reported instead.
            installer_doing "pacman -S --needed ${PACKAGES[*]}"
            run_privileged pacman -S --noconfirm --needed "${PACKAGES[@]}" \
                || installer_die 1 "pacman could not install ${PACKAGES[*]}; its messages are above." \
                    "If pacman reported \"failed retrieving file\" or \"404\", its package list is out of date: update the whole system with sudo pacman -Syu, then run the same command again."
            ;;
        zypper)
            installer_doing "zypper install ${PACKAGES[*]}"
            run_privileged zypper --non-interactive install "${PACKAGES[@]}" \
                || installer_die 1 "zypper could not install ${PACKAGES[*]}; its messages are above."
            ;;
    esac
    c_toolchain_links \
        || installer_die 1 "the C compiler still cannot compile and link a program after installing ${PACKAGES[*]}." \
            "Make sure 'cc' works (try: cc --version), then run the same command again."
    has_library libX11.so.6 \
        || installer_die 1 "libX11.so.6 still cannot be linked against after installing ${PACKAGES[*]}."
    has_library libpulse-simple.so.0 \
        || installer_die 1 "libpulse-simple.so.0 still cannot be linked against after installing ${PACKAGES[*]}."
    report_tools
fi

# ------------------------------------------------------------------ 3. rust
installer_step 'Install Rust'
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
[[ $CARGO_HOME == /* ]] || CARGO_HOME="$PWD/$CARGO_HOME"
export CARGO_HOME
# Prefer rustup's Rust over an older distribution one; no profile edits.
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
        --default-toolchain stable \
        || installer_die 1 "the rustup installer failed; its messages are above." \
            "Run the same command again; it is safe to repeat."
    command -v rustup >/dev/null 2>&1 \
        || installer_die 1 "rustup installed but is not usable ($CARGO_HOME/bin/rustup is missing)."
fi
rust_ready "$RUST_MINIMUM" \
    || installer_die 1 "Rust $RUST_MINIMUM or newer is still not usable." \
        "Try: rustup update stable, then run the same command again."
# An inherited target would put the build somewhere this script does not look.
unset CARGO_BUILD_TARGET
installer_ok "$(rustc --version) ready: $(command -v cargo)"

# ----------------------------------------------------------------- 4. build
installer_step 'Build and install Dice Wars'
if ((JOBS == 1)); then
    installer_doing "building the game with 1 build worker, so the desktop stays responsive (usually 1 to 3 minutes)"
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

mkdir -p -- "$BIN_DIR" "$(dirname -- "$ICON")" "$(dirname -- "$DESKTOP_FILE")" \
    || installer_die 1 "could not create the install folders under $PREFIX." \
        "Choose a --prefix you can write to, then run the same command again."
# Copy then rename, so a running game is never overwritten mid-file.
{ install -m 755 -- "$BUILT" "$BINARY.new" && mv -f -- "$BINARY.new" "$BINARY"; } \
    || installer_die 1 "could not copy the game to $BINARY."
INSTALLER_INSTALLED=$BINARY
installer_ok "installed $BINARY"
install -m 644 -- "$GAME_DIR/assets/icon.png" "$ICON" \
    || installer_die 1 "could not copy the icon to $ICON."
installer_ok "installed $ICON"
if [[ -f $LEGACY_ICON ]]; then
    rm -f -- "$LEGACY_ICON" && installer_note "removed the icon an older install put at $LEGACY_ICON"
fi
cat > "$DESKTOP_FILE.new" << DESKTOP
[Desktop Entry]
Type=Application
Name=Dice Wars
GenericName=Strategy Game
Comment=Pastel dice conquest
Exec="$BINARY"
TryExec=$BINARY
Icon=$ICON
Terminal=false
Categories=Game;StrategyGame;
Keywords=dice;strategy;conquest;
DESKTOP
mv -f -- "$DESKTOP_FILE.new" "$DESKTOP_FILE" \
    || installer_die 1 "could not write the app-menu entry $DESKTOP_FILE."
installer_ok "installed the app-menu entry $DESKTOP_FILE"
refresh_desktop_caches

# ---------------------------------------------------------------- 5. verify
installer_step 'Check the installed game'
[[ -x $BINARY ]] || installer_die 1 "the game file is missing after the install: $BINARY"
if command -v ldd >/dev/null 2>&1; then
    MISSING=$(ldd "$BINARY" 2>/dev/null | sed -n 's/^[[:space:]]*\([^ ]*\) => not found.*/\1/p' | tr '\n' ' ')
    [[ -z $MISSING ]] \
        || installer_die 1 "the game cannot find these libraries: $MISSING" \
            "Run the installer without --skip-deps, or install them with your package manager."
    installer_ok "every library the game needs is found (checked with ldd)"
fi
if command -v desktop-file-validate >/dev/null 2>&1; then
    if desktop-file-validate "$DESKTOP_FILE" >/dev/null 2>&1; then
        installer_ok "the app-menu entry is valid"
    else
        installer_warn "desktop-file-validate reports problems with $DESKTOP_FILE; the game still runs from the terminal"
    fi
fi
if command -v pactl >/dev/null 2>&1 && ! pactl info >/dev/null 2>&1; then
    installer_note "no PulseAudio or PipeWire sound server answered; the game plays silently until one runs"
fi

INSTALLER_DONE=1
printf '\nSUCCESS: Dice Wars is installed and passed its checks.\n'
printf '  Game file:    %s\n' "$BINARY"
printf '  Start it:     choose Dice Wars in your app menu, or run: %s\n' "$BINARY"
case ":${PATH:-}:" in
    *":$BIN_DIR:"*) ;;
    *)
        printf '  Terminal:     to type just "dice-wars", add this line to your shell profile:\n'
        printf '%s\n' "                export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac
printf '  Uninstall:    %s\n' "$UNINSTALL_COMMAND"
printf '  Finished in %s.\n' "$(installer_elapsed)"
