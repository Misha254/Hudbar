#!/usr/bin/env bash
# Живая тема Zen Browser из matugen.
#
# Zen не перечитывает userChrome.css, поэтому цвета обновляет подписанное
# расширение Omafox: matugen пишет JSON, native-host отдаёт его расширению,
# а оно меняет Firefox-тему на лету. Подробности — README.md рядом.
#
#   ./install.sh [--dry-run] [--force] [--profile <путь>]
#
# Flatpak определяется по ~/.var/app/app.zen_browser.zen/.zen, обычная
# установка — по ~/.zen. Скрипт идемпотентен: повторный запуск ничего не
# ломает, пользовательский userChrome.css не перезаписывается.

set -euo pipefail

DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
HOME_DIR=${HOME:?}
CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME_DIR/.config}
DRY_RUN=0
FORCE=0
PROFILE=""

usage() {
    printf 'использование: %s [--dry-run] [--force] [--profile <путь>]\n' "${0##*/}"
}

while (($#)); do
    case "$1" in
        --dry-run) DRY_RUN=1 ;;
        --force) FORCE=1 ;;
        --profile)
            shift
            [[ $# -gt 0 ]] || { usage >&2; exit 2; }
            PROFILE=$1
            ;;
        -h | --help) usage; exit 0 ;;
        *)
            printf 'неизвестный аргумент: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

log() { printf '[zen] %s\n' "$*"; }
die() {
    printf '[zen] ошибка: %s\n' "$*" >&2
    exit 1
}

run() {
    if ((DRY_RUN)); then
        printf '+ '
        printf '%q ' "$@"
        printf '\n'
    else
        "$@"
    fi
}

# Резервная копия только при первой правке: повторный запуск не должен
# затирать рабочий откат.
backup_once() {
    local file=$1
    [[ -e $file ]] || return 0
    [[ -e $file.hudbar-backup ]] || run cp -a "$file" "$file.hudbar-backup"
}

if [[ -d "$HOME_DIR/.var/app/app.zen_browser.zen/.zen" ]]; then
    ZEN_ROOT="$HOME_DIR/.var/app/app.zen_browser.zen/.zen"
    MOZ_ROOT="$HOME_DIR/.var/app/app.zen_browser.zen/.mozilla"
    FLATPAK=1
elif [[ -d "$HOME_DIR/.zen" ]]; then
    ZEN_ROOT="$HOME_DIR/.zen"
    MOZ_ROOT="$HOME_DIR/.mozilla"
    FLATPAK=0
else
    die "Zen не найден: нет ни ~/.var/app/app.zen_browser.zen/.zen, ни ~/.zen"
fi

# Профиль по умолчанию: сначала Default из [Install*], затем первый
# [Profile*]. Этого достаточно и для flatpak, и для обычной установки.
default_profile() {
    local ini=$ZEN_ROOT/profiles.ini path="" first="" section="" dir=""
    if [[ ! -f $ini ]]; then
        for dir in "$ZEN_ROOT"/*; do
            if [[ -d $dir ]]; then
                printf '%s\n' "$dir"
                return
            fi
        done
        return
    fi
    while IFS= read -r line; do
        case "$line" in
            \[Install*) section=install ;;
            \[Profile*) section=profile ;;
            \[*) section=other ;;
            Default=*)
                if [[ $section == install && -z $path ]]; then
                    path=${line#Default=}
                fi
                ;;
            Path=*)
                if [[ $section == profile && -z $first ]]; then
                    first=${line#Path=}
                fi
                ;;
        esac
    done <"$ini"
    printf '%s\n' "${path:-$first}"
}

if [[ -n $PROFILE ]]; then
    [[ -d $PROFILE ]] || PROFILE="$ZEN_ROOT/$PROFILE"
else
    name=$(default_profile)
    [[ -n $name ]] || die "не нашёл профиль в $ZEN_ROOT/profiles.ini"
    PROFILE="$ZEN_ROOT/$name"
fi
[[ -d $PROFILE ]] || die "профиль не найден: $PROFILE"

log "профиль: $PROFILE"

# Мост --lwt-* -> --zen-*. Обновляем всегда: это часть Hudbar, а не файл
# пользователя; прежняя версия остаётся в .hudbar-backup.
chrome=$PROFILE/chrome
run mkdir -p "$chrome"
backup_once "$chrome/omafox.css"
run install -m0644 "$DIR/omafox.css" "$chrome/omafox.css"

# userChrome.css принадлежит пользователю: только дописываем импорт, если его
# ещё нет, и никогда не заменяем файл целиком.
import='@import url("omafox.css");'
if [[ -f $chrome/userChrome.css ]]; then
    if ! grep -qF 'omafox.css' "$chrome/userChrome.css"; then
        backup_once "$chrome/userChrome.css"
        if ((DRY_RUN)); then
            log "дописал бы $import в $chrome/userChrome.css"
        else
            printf '\n%s\n' "$import" >>"$chrome/userChrome.css"
        fi
    fi
elif ((DRY_RUN)); then
    log "создал бы $chrome/userChrome.css с $import"
else
    printf '%s\n' "$import" >"$chrome/userChrome.css"
fi

# Подписанное расширение с AMO. Если оно уже стоит, не трогаем: обновления
# приходят из about:addons.
run mkdir -p "$PROFILE/extensions"
if [[ -e "$PROFILE/extensions/@omafox.xpi" && $FORCE -eq 0 ]]; then
    log "расширение уже стоит: $PROFILE/extensions/@omafox.xpi"
else
    run install -m0644 "$DIR/omafox.xpi" "$PROFILE/extensions/@omafox.xpi"
fi

# Префы, без которых Zen отключает lightweight-тему и side-loaded расширение.
prefs=(
    'user_pref("zen.theme.disable-lightweight", false);'
    'user_pref("extensions.autoDisableScopes", 0);'
    'user_pref("extensions.enabledScopes", 15);'
)
user_js=$PROFILE/user.js
[[ -e $user_js ]] || : >"$user_js"
for pref in "${prefs[@]}"; do
    if grep -qF "$pref" "$user_js"; then
        continue
    fi
    backup_once "$user_js"
    if ((DRY_RUN)); then
        log "дописал бы в user.js: $pref"
    else
        printf '%s\n' "$pref" >>"$user_js"
    fi
done

# Native-host собирается из исходника: бинарник платформозависим, а компилятор
# на Linux уже нужен самому Hudbar.
command -v cc >/dev/null 2>&1 || die "нужен cc (gcc) для сборки native-host"
host_dir=$MOZ_ROOT/native-messaging-hosts
run mkdir -p "$host_dir"
run cc -O2 -o "$host_dir/omafox-native-host" "$DIR/omafox-native-host.c"
run chmod 0755 "$host_dir/omafox-native-host"

# Путь внутри песочницы flatpak тот же: /home/<user>/.mozilla, потому что
# каталог .mozilla сделан постоянным через --persist.
manifest=$host_dir/org.omarchy.omafox.json
if ((DRY_RUN)); then
    log "записал бы $manifest"
else
    cat >"$manifest" <<EOF
{
  "name": "org.omarchy.omafox",
  "description": "matugen theme bridge for Zen Browser",
  "path": "$HOME_DIR/.mozilla/native-messaging-hosts/omafox-native-host",
  "type": "stdio",
  "allowed_extensions": ["@omafox"]
}
EOF
fi

if ((FLATPAK)); then
    if command -v flatpak >/dev/null 2>&1 &&
        flatpak info app.zen_browser.zen >/dev/null 2>&1; then
        run flatpak override --user --persist=.mozilla app.zen_browser.zen
    else
        log "flatpak-приложение не найдено: пропускаю --persist=.mozilla"
    fi
fi

# Шаблон matugen: JSON, который читает native-host.
templates=$CONFIG_HOME/matugen/templates
state=$ZEN_ROOT/matugen-theme.json
run mkdir -p "$templates"
backup_once "$templates/zen-theme.json"
run install -m0644 "$DIR/zen-theme.json" "$templates/zen-theme.json"

# Запись [templates.zen]. TOML правится скриптом, а не переписывается целиком:
# в config.toml у пользователя десятки других шаблонов.
config=$CONFIG_HOME/matugen/config.toml
if command -v python3 >/dev/null 2>&1; then
    backup_once "$config"
    if ((DRY_RUN)); then
        log "обновил бы [templates.zen] в $config"
    else
        python3 - "$config" "$templates/zen-theme.json" "$state" <<'PY'
import pathlib
import re
import sys

config, template, output = sys.argv[1:4]
path = pathlib.Path(config)
text = path.read_text() if path.exists() else ""
block = (
    "[templates.zen]\n"
    "# Живая тема Zen: JSON читает omafox-native-host и отдаёт расширению\n"
    "# Omafox, а оно меняет Firefox-тему без перезапуска браузера.\n"
    f"input_path = '{template}'\n"
    f"output_path = '{output}'\n"
)
pattern = re.compile(r"(?ms)^\[templates\.zen\][^\[]*")
if pattern.search(text):
    text = pattern.sub(block, text, count=1)
else:
    if text and not text.endswith("\n"):
        text += "\n"
    text += "\n" + block
path.write_text(text)
PY
    fi
else
    log "python3 не найден: добавь [templates.zen] в $config вручную"
    log "  input_path = '$templates/zen-theme.json'"
    log "  output_path = '$state'"
fi

# Сразу покрасить Zen текущей палитрой, не дожидаясь смены обоев. Отдельный
# временный конфиг: полный прогон matugen перерисовал бы всю систему.
wallpaper=$HOME_DIR/.config/wallpaper
if command -v matugen >/dev/null 2>&1 && [[ -f $wallpaper ]]; then
    scheme=$(cat "$CONFIG_HOME/hudbar/scheme" 2>/dev/null || printf 'scheme-tonal-spot')
    tmp=$(mktemp)
    trap 'rm -f "$tmp"' EXIT
    {
        printf '[config]\ntheme = "dark"\n\n[templates.zen]\n'
        printf "input_path = '%s'\n" "$templates/zen-theme.json"
        printf "output_path = '%s'\n" "$state"
    } >"$tmp"
    if ((DRY_RUN)); then
        log "запустил бы matugen: текущая палитра -> $state"
    elif run matugen image "$(cat "$wallpaper")" --source-color-index 0 \
        -t "$scheme" -c "$tmp"; then
        log "текущая палитра записана в $state"
    else
        log "matugen не смог посчитать палитру: Zen покрасится после смены обоев"
    fi
else
    log "нет matugen или ~/.config/wallpaper: Zen покрасится после смены обоев"
fi

if ((DRY_RUN)); then
    log "dry run завершён"
    exit 0
fi

log "готово"
printf '\n'
printf 'Перезапусти Zen один раз: расширение и userChrome.css читаются при старте.\n'
printf 'Цвета дальше меняются на лету, без перезапуска.\n'
printf 'Отключить тему: удалить расширение Omafox в about:addons.\n'
