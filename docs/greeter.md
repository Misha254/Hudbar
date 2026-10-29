# Экран входа hud-greeter-rs

Свой гритер для greetd на примитивах Hudbar: полноэкранный слой, карточка
меню, палитра matugen. Заменяет `tuigreet`; статическая тема
`contrib/theme/tuigreet.toml` остаётся запасным вариантом.

## Почему свой, а не tuigreet

`tuigreet` умеет ровно то, что нужно для входа, но его вид задаётся статической
темой в `/etc` и не следует за обоями. Свой гритер читает тот же `colors.css`,
что панель и окна, и тот же кадр рисует и на экране, и в `--screenshot`.
Ограничение прежнее: файл темы принадлежит root, поэтому палитру в каталог
гритера кладёт `--sync` от обычного пользователя, а не matugen напрямую.

## Устройство

| Файл | Что делает |
|---|---|
| `src/bin/hud_greeter.rs` | окно, ввод, режимы `--preview` / `--screenshot` / `--sync` |
| `src/hud/greeter/ipc.rs` | сокет `$GREETD_SOCK`, длина `u32` + JSON |
| `src/hud/greeter/flow.rs` | конечный автомат входа, пароль в затираемом буфере |
| `src/hud/greeter/sessions.rs` | `/usr/share/wayland-sessions/*.desktop`, argv без shell |
| `src/hud/greeter/state.rs` | последний пользователь и сеанс |
| `src/hud/greeter/config.rs` | каталог данных, `greeter.json`, палитра |
| `src/hud/greeter/image.rs` | фон `cover` и уменьшение обоев |
| `src/hud/greeter/layout.rs` | геометрия карточки из токенов |
| `src/hud/greeter/render.rs` | кадр целиком (окно и снимок — один код) |
| `src/hud/greeter/panel.rs` | часы, дата, батарея |
| `src/hud/greeter/sync.rs` | `--sync` |
| `src/hud/greeter/strings.rs` | подписи ru/en |

Композитор — niri с конфигом `contrib/greeter/niri-greeter.kdl`: других окон
и биндов там нет, `spawn-at-startup "hud-greeter-rs"` запускает гритер.
Полноэкранность даёт `layer::open_fullscreen` — якоря на все четыре стороны и
`set_size(0, 0)`; остальные окна продолжают пользоваться `layer::open`.

`cage` для гритера не подходит: он даёт только xdg-shell, а окно гритера —
`wlr-layer-shell`. Проверить это по документации без сети не удалось
(пакет `cage` в системе не установлен), поэтому в отчёте это отмечено как
непроверенное; выбор niri подтверждён существующими layer-окнами Hudbar и
командами `niri -c <config>` / `niri msg action quit --skip-confirmation`.

## Протокол greetd

Сообщение — 4 байта длины в native endian и JSON. Запросы:
`create_session{username}`, `post_auth_message_response{response}`,
`start_session{cmd, env}`, `cancel_session`. Ответы: `success`,
`error{error_type, description}`, `auth_message{auth_message_type,
auth_message}`.

Правила автомата:

- Enter на пустом пароле не создаёт сеанс;
- после отказа PAM обязателен `cancel_session`, иначе следующая попытка
  получит «session already exists»;
- многошаговый PAM поддержан: после пароля может прийти код или вопрос;
- пояснения `info`/`error` подтверждаются пустым ответом, иначе сеанс
  замирает на баннере;
- любая ошибка сокета или разбора — сообщение на экране, а не паника;
- после успешного `start_session` гритер выходит, и greetd запускает сессию.

Таймаут чтения и записи — 10 секунд: PAM может думать над токеном, но не
бесконечно.

## Файлы

Каталог данных по умолчанию — `/var/lib/hudbar-greeter`:

| Файл | Откуда |
|---|---|
| `colors.css` | `~/.config/hudbar/colors.css`, копия через `--sync` |
| `background.png` | текущие обои из `~/.config/wallpaper`, ≤1920 по ширине |
| `greeter.json` | язык из `settings.json`; `dim`, `card_plate` и `font` сохраняются между запусками |

Нет файла — значения по умолчанию, экран входа всё равно появится: палитра
из `palette::default()`, фон однотонный, язык русский.

`card_plate` (0..=1, по умолчанию 0.6) — прозрачность подложки под карточкой.
Это `ui.base` из палитры, а не чёрный: на светлой схеме плашка светлая и текст
остаётся тёмным. Ноль убирает подложку совсем, карточка ложится прямо на
обои; единица делает её сплошной. Подложка рисуется тем же прямоугольником и
радиусом, что и карточка, поэтому на экране это одно затемнённое поле, а не
вторая рамка. Ставится в `greeter.json` вручную или переживает `--sync`:
синхронизация сохраняет настроенное значение, как `dim` и `font`.

Состояние входа — `/var/cache/hudbar-greeter/state.json` (последний
пользователь и сеанс). Путь переопределяется `--state` и
`HUDBAR_GREETER_STATE`; битый файл даёт значения по умолчанию. Ошибка записи
не фатальна.

## Управление

| Клавиша | Действие |
|---|---|
| `Tab` / `Shift+Tab` | переключить поле |
| `Enter` | из имени — к паролю; в пароле — вход |
| `Backspace` | стереть символ |
| `Ctrl+U` | очистить активное поле |
| `F2` | следующий сеанс по кругу |
| `F12` | режим питания |
| `1` / `2` / `3` | выключить / перезагрузить / сон (только в режиме питания) |
| `Esc` | выйти из режима питания или отменить сеанс |

Раскладка клавиатуры в верхней полосе не показывается: получить её имя без
новых зависимостей не удалось. Caps Lock показывается и в полосе, и у поля
пароля.

Пароль не попадает ни в лог, ни в argv, ни в файлы. Он живёт в
`flow::Secret`, который затирает буфер нулями при очистке и при уничтожении.

## Режимы запуска

```sh
hud-greeter-rs                 # из greetd, сокет $GREETD_SOCK
hud-greeter-rs --preview       # из живой сессии, пароль test
hud-greeter-rs --screenshot out.png --variant error --lang en
hud-greeter-rs --sync          # тема из ~/.config/hudbar в каталог гритера
```

`--preview` не завершает niri: после успешного входа окно просто закрывается.
В обычном режиме после `start_session` гритер зовёт
`niri msg action quit --skip-confirmation` argv-вызовом и выходит — иначе два
niri подрались бы за DRM.

`--screenshot` рисует кадр 1920×1080 без Wayland. Виды: `known`, `unknown`,
`error`, `caps`, `power`; язык — `--lang ru|en`; каталог данных — `--data`.

`--sync` копирует `colors.css`, уменьшает текущие обои в `background.png`
(атомарная запись: временный файл и `rename`) и переносит язык из
`settings.json`. Каталог по умолчанию — `/var/lib/hudbar-greeter`, другой
задаётся `--dir`. Без прав записи — понятная ошибка и ненулевой код, `sudo`
не предлагается. Автоматического вызова из смены обоев нет; хук вручную:

```sh
# ~/.local/bin/wall.sh, после успешного matugen:
hud-greeter-rs --sync
```

## Установка

```sh
# от root, один раз; <user> — чей ~/.config/hudbar синхронизировать
contrib/greeter/install-greeter.sh <user>
```

Скрипт кладёт `niri-greeter.kdl` в `/etc/hudbar/greeter-niri.kdl`, создаёт
`/var/lib/hudbar-greeter` (владелец — `<user>`) и
`/var/cache/hudbar-greeter` (владелец — пользователь гритера, по умолчанию
`greeter`), делает резервную копию `/etc/greetd/config.toml` в
`config.toml.hudbar-backup` и ставит конфиг из `config.toml.example`.
Сервисы он не включает и не перезапускает.

Проверка и откат:

```sh
niri validate -c /etc/hudbar/greeter-niri.kdl
systemctl restart greetd          # вручную, когда готовы проверить

cp /etc/greetd/config.toml.hudbar-backup /etc/greetd/config.toml
systemctl restart greetd
```

Если экран входа сломался: `Ctrl+Alt+F2`, вход своим пользователем, откат из
команд выше. `tuigreet` и `contrib/theme/tuigreet.toml` остаются рабочим
запасным вариантом.

## Границы

- Нет выбора раскладки в полосе и нет экранной клавиатуры.
- Нет аватара пользователя и списка пользователей: имя вводится строкой, как
  в tuigreet.
- `systemctl poweroff/reboot/suspend` вызывается только по цифре в режиме
  питания; гритер не проверяет, разрешено ли это пользователю.
- Пароль проверяет PAM через greetd; гритер не читает `/etc/shadow`.
