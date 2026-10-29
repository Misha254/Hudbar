#!/usr/bin/env bash
# Установка hud-greeter-rs: конфиг niri, каталоги данных и запись greetd.
#
# Запускать ТОЛЬКО от root и только осознанно: скрипт меняет
# /etc/greetd/config.toml, поэтому сначала делает резервную копию. Сервисы он
# не включает и не перезапускает — это остаётся человеку, чтобы сломанный
# экран входа не застал врасплох без способа откатиться.
#
#   ./install-greeter.sh <пользователь> [пользователь-гритера]
#
# <пользователь>       чей ~/.config/hudbar будет синхронизировать --sync;
#                      ему же принадлежит /var/lib/hudbar-greeter.
# [пользователь-гритера] от кого работает greetd (по умолчанию greeter);
#                      ему принадлежит /var/cache/hudbar-greeter.
#
# Скрипт идемпотентен: повторный запуск не перезаписывает резервную копию и
# приводит файлы к одному и тому же виду.

set -eu

usage() {
    printf 'использование: %s <пользователь> [пользователь-гритера]\n' "${0##*/}"
}

if [ "$(id -u)" -ne 0 ]; then
    printf 'нужен root: запусти скрипт от root, внутри sudo он не вызывает\n' >&2
    exit 1
fi

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    usage >&2
    exit 2
fi

user=$1
greeter_user=${2:-greeter}

if ! id -u "$user" >/dev/null 2>&1; then
    printf 'пользователь %s не найден\n' "$user" >&2
    exit 1
fi

if ! id -u "$greeter_user" >/dev/null 2>&1; then
    printf 'пользователь гритера %s не найден\n' "$greeter_user" >&2
    exit 1
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# Конфиг niri для композитора гритера.
install -Dm644 "$script_dir/niri-greeter.kdl" /etc/hudbar/greeter-niri.kdl

# Каталог данных: сюда --sync от обычного пользователя кладёт colors.css и
# обои, отсюда их читает гритер. Владелец — пользователь, режим 0755: гритеру
# нужно только чтение.
install -d -m 0755 /var/lib/hudbar-greeter
chown "$user" /var/lib/hudbar-greeter
chmod 0755 /var/lib/hudbar-greeter

# Состояние входа: последний пользователь и сеанс. Пишет гритер, поэтому
# владелец — пользователь гритера.
install -d -m 0755 /var/cache/hudbar-greeter
chown "$greeter_user" /var/cache/hudbar-greeter
chmod 0755 /var/cache/hudbar-greeter

# Резервная копия только при первом запуске: иначе повторный запуск затёр бы
# рабочий откат.
install -d -m 0755 /etc/greetd
if [ -f /etc/greetd/config.toml ] && [ ! -f /etc/greetd/config.toml.hudbar-backup ]; then
    cp -a /etc/greetd/config.toml /etc/greetd/config.toml.hudbar-backup
fi

# Рабочий конфиг greetd — из примера рядом со скриптом. Пример уже содержит
# команду niri с путём /etc/hudbar/greeter-niri.kdl.
install -Dm644 "$script_dir/config.toml.example" /etc/greetd/config.toml

printf 'готово: бинарь hud-greeter-rs, конфиг niri и каталоги данных на месте\n'
printf 'резервная копия greetd: /etc/greetd/config.toml.hudbar-backup\n'
printf '\n'
printf 'Если пользователь гритера не %s, проверь строку user в /etc/greetd/config.toml.\n' "$greeter_user"
printf '\n'
printf 'Дальше — вручную, скрипт сервисы не трогает:\n'
printf '  1. Проверить конфиг niri: niri validate -c /etc/hudbar/greeter-niri.kdl\n'
printf '  2. Синхронизировать тему из своей сессии (без sudo): hud-greeter-rs --sync\n'
printf '  3. Перезапустить greetd: systemctl restart greetd\n'
printf '  4. Выйти из сессии и проверить экран входа.\n'
printf '\n'
printf 'Откат на tuigreet:\n'
printf '  cp /etc/greetd/config.toml.hudbar-backup /etc/greetd/config.toml\n'
printf '  systemctl restart greetd\n'
printf '\n'
printf 'Если экран входа сломался и войти некуда:\n'
printf '  Ctrl+Alt+F2 (другой TTY), войти своим пользователем, выполнить откат\n'
printf '  из шага выше; systemctl restart greetd попросит root.\n'
