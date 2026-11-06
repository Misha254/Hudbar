#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PREFIX=${PREFIX:-"$HOME/.local"}
CONFIG_HOME=${XDG_CONFIG_HOME:-"$HOME/.config"}
STATE_HOME=${XDG_STATE_HOME:-"$HOME/.local/state"}
BACKUP_ROOT=${HUD_BACKUP_DIR:-"$STATE_HOME/hudbar/backups/$(date +%Y%m%d-%H%M%S)"}
DRY_RUN=0
FORCE=0
ZEN=0

usage() {
    printf 'Usage: %s [--dry-run] [--force] [--zen]\n' "$0"
    printf '\nInstalls hudbar binaries and the repository-owned default config.\n'
    printf 'Existing files are backed up before replacement.\n'
    printf '\n  --zen   also install the live Zen Browser theme (see contrib/zen)\n'
}

# Цвета только для терминала: в CI и при перенаправлении вывода остаются
# обычные строки. NO_COLOR поддерживается, как в остальных CLI.
if [[ -t 1 && -z ${NO_COLOR:-} && ${TERM:-} != dumb ]]; then
    c_reset=$'\e[0m'
    c_bold=$'\e[1m'
    c_dim=$'\e[2m'
    c_green=$'\e[32m'
    c_cyan=$'\e[36m'
    c_yellow=$'\e[33m'
    c_red=$'\e[31m'
else
    c_reset="" c_bold="" c_dim="" c_green="" c_cyan="" c_yellow="" c_red=""
fi

section() {
    printf '\n%s▸ %s%s\n' "$c_cyan" "$*" "$c_reset"
}

ok() {
    printf '  %s✓%s %s\n' "$c_green" "$c_reset" "$*"
}

note() {
    printf '  %s·%s %s\n' "$c_dim" "$c_reset" "$*"
}

warn() {
    printf '  %s!%s %s\n' "$c_yellow" "$c_reset" "$*"
}

die() {
    printf '%serror:%s %s\n' "$c_red" "$c_reset" "$*" >&2
    exit 1
}

run() {
    if (( DRY_RUN )); then
        printf '%s+ ' "$c_dim"
        printf '%q ' "$@"
        printf '%s\n' "$c_reset"
    else
        "$@"
    fi
}

while (($#)); do
    case "$1" in
        --dry-run) DRY_RUN=1 ;;
        --force) FORCE=1 ;;
        --zen) ZEN=1 ;;
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
        note "skip ${destination##*/}"
        return
    fi
    if [[ -e "$destination" ]]; then
        run mkdir -p "$BACKUP_ROOT/$(dirname "$relative_path")"
        run cp -a "$destination" "$BACKUP_ROOT/$relative_path"
        note "backup ${destination##*/}"
    fi
    run mkdir -p "$(dirname "$destination")"
    run install -m "$mode" "$source" "$destination"
    ok "${destination##*/}"
}

install_default_file() {
    local source=$1
    local destination=$2
    if [[ -e "$destination" ]]; then
        note "keep ${destination##*/}"
        return
    fi
    run mkdir -p "$(dirname "$destination")"
    run install -m 0644 "$source" "$destination"
    ok "${destination##*/}"
}

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)
printf '\n%sHUDbar%s %s · installer\n' "$c_bold" "$c_reset" "$version"
printf '%s────────────────────────────────────────%s\n' "$c_dim" "$c_reset"
printf '  %sexisting files are kept; --force replaces them and makes a backup%s\n' "$c_dim" "$c_reset"

section "Building release binaries"
run cargo build --release --bins --manifest-path "$ROOT_DIR/Cargo.toml"

section "Installing binaries → $PREFIX/bin"
for binary in hudbar hud-menu-rs hud-apps-rs hud-keybinds-rs hud-yazibinds-rs hud-vpn-rs hud-wallpaper-rs hud-schemes-rs; do
    install_file "$ROOT_DIR/target/release/$binary" "$PREFIX/bin/$binary" 0755
done

section "Installing scripts → $PREFIX/bin"
for script in hud-menu hud-apps hud-keybinds hud-yazibinds hud-vpn hud-wallpaper hud-wallpapers hud-setting hud-update hud-close; do
    install_file "$ROOT_DIR/scripts/$script" "$PREFIX/bin/$script" 0755
done
install_file "$ROOT_DIR/scripts/hud-migrate-settings" "$PREFIX/bin/hud-migrate-settings" 0755

section "Installing settings → $CONFIG_HOME/hudbar"
if (( DRY_RUN )); then
    run python3 "$ROOT_DIR/scripts/hud-migrate-settings" --config-home "$CONFIG_HOME" --dry-run
else
    run "$PREFIX/bin/hud-migrate-settings" --config-home "$CONFIG_HOME"
fi

install_default_file "$ROOT_DIR/config/hudbar/settings.json" "$CONFIG_HOME/hudbar/settings.json"

# Схема лежит рядом с настройками: на неё ссылается ключ `$schema` в
# установочном файле, по ней редактор проверяет настройки.
install_file "$ROOT_DIR/docs/settings.schema.json" "$CONFIG_HOME/hudbar/settings.schema.json"

install_file "$ROOT_DIR/config/hudbar/colors.css" "$CONFIG_HOME/hudbar/colors.css"
install_file "$ROOT_DIR/config/hudbar/font" "$CONFIG_HOME/hudbar/font"

# Тема Zen — отдельный шаг: она меняет профиль браузера и flatpak-разрешения.
# Без флага установщик браузер не трогает.
if (( ZEN )); then
    section "Configuring Zen Browser theme"
    zen_args=()
    (( DRY_RUN )) && zen_args+=(--dry-run)
    (( FORCE )) && zen_args+=(--force)
    "$ROOT_DIR/contrib/zen/install.sh" "${zen_args[@]}"
fi

if (( DRY_RUN )); then
    section "Dry run complete"
    note "nothing was changed"
else
    section "Done"
    printf '  %sbinaries%s  %s\n' "$c_dim" "$c_reset" "$PREFIX/bin"
    printf '  %sconfig%s    %s\n' "$c_dim" "$c_reset" "$CONFIG_HOME/hudbar"
    if (( FORCE )); then
        printf '  %sbackups%s   %s\n' "$c_dim" "$c_reset" "$BACKUP_ROOT"
    fi
fi
