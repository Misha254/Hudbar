#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PREFIX=${PREFIX:-"$HOME/.local"}
CONFIG_HOME=${XDG_CONFIG_HOME:-"$HOME/.config"}
STATE_HOME=${XDG_STATE_HOME:-"$HOME/.local/state"}
BACKUP_ROOT=${HUD_BACKUP_DIR:-"$STATE_HOME/hudbar/backups/$(date +%Y%m%d-%H%M%S)"}
DRY_RUN=0
FORCE=0

usage() {
    printf 'Usage: %s [--dry-run] [--force]\n' "$0"
    printf '\nInstalls hudbar binaries and the repository-owned default config.\n'
    printf 'Existing files are backed up before replacement.\n'
}

log() {
    printf '[hudbar] %s\n' "$*"
}

die() {
    printf '[hudbar] error: %s\n' "$*" >&2
    exit 1
}

run() {
    if (( DRY_RUN )); then
        printf '+ '
        printf '%q ' "$@"
        printf '\n'
    else
        "$@"
    fi
}

while (($#)); do
    case "$1" in
        --dry-run) DRY_RUN=1 ;;
        --force) FORCE=1 ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done

command -v cargo >/dev/null 2>&1 || die "cargo is required"
command -v install >/dev/null 2>&1 || die "install is required"

[[ -f "$ROOT_DIR/Cargo.toml" ]] || die "Cargo.toml not found in $ROOT_DIR"

install_file() {
    local source=$1
    local destination=$2
    local mode=${3:-0644}
    local relative_path=${destination#/}

    [[ -f "$source" ]] || die "managed file is missing: $source"
    if [[ -e "$destination" && $FORCE -eq 0 ]]; then
        log "skip existing $destination (use --force to replace it)"
        return
    fi
    if [[ -e "$destination" ]]; then
        run mkdir -p "$BACKUP_ROOT/$(dirname "$relative_path")"
        run cp -a "$destination" "$BACKUP_ROOT/$relative_path"
        log "backup: $destination -> $BACKUP_ROOT/$relative_path"
    fi
    run mkdir -p "$(dirname "$destination")"
    run install -m "$mode" "$source" "$destination"
}

install_default_file() {
    local source=$1
    local destination=$2
    if [[ -e "$destination" ]]; then
        log "keep user settings $destination"
        return
    fi
    run mkdir -p "$(dirname "$destination")"
    run install -m 0644 "$source" "$destination"
}

log "building release binaries"
run cargo build --release --bins --manifest-path "$ROOT_DIR/Cargo.toml"

for binary in hudbar hud-settings-rs hud-menu-rs hud-keybinds-rs hud-yazibinds-rs; do
    install_file "$ROOT_DIR/target/release/$binary" "$PREFIX/bin/$binary" 0755
done

for script in hud-settings hud-menu hud-keybinds hud-yazibinds hud-setting hud-theme; do
    install_file "$ROOT_DIR/scripts/$script" "$PREFIX/bin/$script" 0755
done
install_file "$ROOT_DIR/scripts/hud-migrate-settings" "$PREFIX/bin/hud-migrate-settings" 0755

if (( DRY_RUN )); then
    run python3 "$ROOT_DIR/scripts/hud-migrate-settings" --config-home "$CONFIG_HOME" --dry-run
else
    run "$PREFIX/bin/hud-migrate-settings" --config-home "$CONFIG_HOME"
fi

install_default_file "$ROOT_DIR/config/hudbar/settings.json" "$CONFIG_HOME/hudbar/settings.json"
install_file "$ROOT_DIR/config/hudbar/colors.css" "$CONFIG_HOME/hudbar/colors.css"
install_file "$ROOT_DIR/config/hudbar/font" "$CONFIG_HOME/hudbar/font"

if (( DRY_RUN )); then
    log "dry run complete"
else
    log "installation complete"
    log "binaries: $PREFIX/bin"
    log "config: $CONFIG_HOME/hudbar"
    if (( FORCE )); then
        log "backups: $BACKUP_ROOT"
    fi
fi
