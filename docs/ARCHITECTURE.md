# Архитектура HUDbar

Как устроен кадр, где что лежит и что трогать при типичных правках.

Установка, настройка и скриншоты — в [README](../README.md).

## Как устроен один кадр

1. Фоновые потоки пишут в `Shared.sys` (это `Mutex<Sys>`) и зовут `Shared::mark()`: ставит
   `dirty` и пишет байт в `wake_tx`.
2. Главный цикл в `src/main.rs` спит в `poll` сразу на двух fd: wayland-сокет и `wake_rx`.
   Проснувшись, забирает события и, если `dirty`, вызывает `App::frame()`.
3. `frame()` рисует бар и открытые поверхности (панель, тултип, grab) в shm-слоты и коммитит их.

Пока идёт анимация (открытие панели или подсветка ховера), `poll` ограничен 16 мс, иначе 500 мс.
Палитра перечитывается при изменении mtime `colors.css` — цвета от matugen подхватываются без
перезапуска, а сверка stat'ом вместо полного разбора не дёргает диск на каждом проходе цикла.
Часы собираются из одного вызова `Local::now()` на проход.

## Компоненты и служебные бинарники

`src/bin/sni_test.rs` — отдельный бинарник, не часть панели. Это стенд для отладки протокола
StatusNotifierItem: поднимает фейковый элемент трея, чтобы проверять `tray.rs`.

`src/bin/hud_apps.rs` — лаунчер приложений на `Super+D`, замена прежнего
`rofi -show hudapps` с `hud-drun.sh`: тот же список из desktop-файлов и те же
закреплённые, но окно рисуется тем же кодом, что панель и меню, и не требует X11.
Иконки — SVG через `resvg` и PNG, с кэшем и фоновой загрузкой; первый кадр рисуется
сразу, с заглушками. Закреплённые лежат в `~/.config/hudbar/apps-pinned` и один раз
переносятся из `~/.config/rofi/pinned`. Снимки без композитора:
`hud-apps-rs --snapshot <default|search|empty|pinned> out.png`. Логика окна —
в `src/hud/apps/`.

`src/bin/hud_keybinds.rs` и `src/bin/hud_yazibinds.rs` — нативные окна биндов niri (KP_8)
и yazi (KP_9), замена прежних Rofi-скриптов. Общая логика парсинга — в
`src/hud/bind_data.rs`. Канонические toggle-обёртки находятся в `scripts/` и устанавливаются в
`~/.local/bin/hud-keybinds` и `~/.local/bin/hud-yazibinds`. Полная карта компонентов — в
[`COMPONENTS.md`](COMPONENTS.md).

## Карта файлов

```
src/main.rs            точка входа: создаёт поверхности, владеет циклом событий
src/hud/mod.rs         корень модулей, реэкспортит App и ui
src/hud/data/mod.rs    состояние Sys, Shared и координация фоновых циклов
src/hud/data/system.rs батарея, cpu, память, камера, рекордер, DND
src/hud/data/network.rs wifi, bluetooth и сетевые действия
src/hud/data/audio.rs  PipeWire/PulseAudio, громкость и EasyEffects
src/hud/data/backlight.rs яркость из sysfs: uevent ядра вместо опроса
src/hud/data/weather.rs погода и её обновление
src/hud/data/workspaces.rs воркспейсы и EventStream niri
src/hud/actions.rs     действия пользователя: фокус, звук, микрофон, bluetooth
src/hud/tray.rs        протокол StatusNotifierWatcher / StatusNotifierItem
src/hud/palette.rs     цвета из colors.css
src/hud/settings.rs    что показывать, из settings.json
src/hud/shell.rs       общие куски цикла окон: шаг колеса, ожидание сокета, дожимание проявления
src/hud/app/mod.rs     связывает ui, render и handlers
src/hud/app/ui.rs      константы, иконки, типы интерфейса, примитивы рисования
src/hud/app/render/mod.rs     App и общие импорты отрисовки
src/hud/app/render/bar.rs     бар, чипы и раскладка
src/hud/app/render/panels.rs  панели и содержимое виджетов
src/hud/app/render/surfaces.rs поверхности, popup, tooltip и grab
src/hud/app/render/input.rs   обработка кликов, hover и переключателей
src/hud/app/text.rs    работа с cosmic-text и отрисовка глифов
src/hud/app/calendar.rs календарь и обработка навигации по месяцам
src/hud/app/handlers.rs реализации трейтов smithay-client-toolkit
```

### `src/main.rs`

Только запуск. Биндит `wl_compositor`, `wlr-layer-shell`, `wl_shm`, создаёт layer surface на
`Layer::Top` с якорем `TOP | LEFT | RIGHT` и exclusive zone высотой бара. Собирает `App` и крутит
цикл. Никакой логики панели здесь нет — если что-то про поведение, это не этот файл.

### `src/hud/data/`

Единый снимок состояния — `Sys`. Туда складывается всё, что панель показывает: батарея, cpu,
память, громкость, микрофон, wifi, bluetooth, списки устройств, погода, флаг вебкамеры, рекордер,
DND, воркспейсы, элементы трея.

`data/mod.rs` хранит общий снимок `Sys`, `Shared` и поднимает фоновые циклы и трей:

- `sys_loop` — опрос системы. Батарея раз в 30 тиков, cpu/память раз в 12, громкость раз в 6,
  wifi каждый тик. Пишет в `Sys` только то, что реально изменилось, и только тогда будит отрисовку.
- `audio_watch` — подписка на `pactl subscribe`, перечитывает громкость и микрофон по событиям звука.
- `backlight` — яркость из `/sys/class/backlight`: подписка на netlink-группу uevent, ядро
  шлёт `KOBJ_CHANGE` на каждую запись в `brightness`. Значение читается из sysfs по событию,
  а опрос раз в 100 мс остаётся запасным путём, если сокет недоступен.
- `niri_loop` — подписка на `EventStream` сокета niri, обновляет воркспейсы.
- `weather_loop` — погода, повтор каждые 5400 с при успехе и 60 с при ошибке.

Обновления состояния находятся здесь, а действия пользователя вынесены в `actions.rs`.
Рендер только вызывает их. Команды уходят в `nmcli`, `bluetoothctl`, `wpctl`, `pactl` и `niri`.

`Shared::mark()` — единственный правильный способ сказать главному потоку «перерисуй». Прямая
запись в `dirty` из фонового потока без байта в пайп оставит кадр до следующего события.

### `src/hud/tray.rs`

Реализует сторону хоста для StatusNotifier: регистрируется как `StatusNotifierWatcher`, следит за
именами на сессионной шине, забирает иконки и тултипы, парсит меню `com.canonical.dbusmenu`.
Наружу торчат `spawn`, `fetch_menu_entries`, `activate_item`, `send_menu_event`. Результат кладёт
в `Sys.tray` и `Sys.tray_menu`.

### `src/hud/palette.rs`

`Palette` из шести цветов: `base`, `text`, `primary`, `on_primary`, `secondary`, `error`. Читает
`~/.config/hudbar/colors.css`, формат `@define-color name #rrggbb;`. Нет файла или нет строки —
берётся значение по умолчанию, остальные цвета не сбрасываются.

### `src/hud/settings.rs`

`Settings` читается из `~/.config/hudbar/settings.json`. Булевы поля включают
чипы: `tray`, `weather`, `webcam`, `clock`, `recorder`, `battery`, `system`, `audio`, `network`,
`dnd`. `height` зажимается в диапазон 24..48. Изменения файла подхватываются автоматически;
при изменении `height` панель обновляет размер и exclusive zone.

### `src/hud/app/ui.rs`

Всё, от чего зависят и отрисовка, и ввод, но что само ничего не решает.

- Константы размеров (`SIZE`, `SCALE`, `PANEL_W`, `PANEL_ROW`, ...).
- `Theme` — числа отрисовки бара: отступ чипов, зазор между ними, размер и отступы иконок
  трея, пробелы перед процентами звука (`vol_pad`), промежуток между иконкой и текстом
  (`icon_gap`). Конструктор один — `Theme::normal()`; цвета приходят из `colors.css`.
- Иконки `I_*` — глифы шрифта иконок.
- `Hit` — что можно нажать на баре. `PanelKind` — какая панель открыта. `PanelView` — состояние
  панели: список, ввод пароля wifi, календарь.
- `Cell` и `Group` — единица раскладки бара. `pad_l`/`pad_r` входят в ширину ячейки и в область
  подсветки, `gap_r` — промежуток после ячейки, в подсветку не входит. Это важно: зазор между
  wifi и bluetooth задан через `gap_r`, а не через `pad_r`, иначе подсветка шире иконки.
- Примитивы: `fill_rect`, `fill_round_rect`, `blit_argb`.

### `src/hud/app/render/` и `src/hud/app/text.rs`

`App` и основная отрисовка находятся в `render/`. Работа с cosmic-text, шрифтами и отрисовкой
глифов вынесена в `text.rs` через небольшой трейт `TextRenderer`. Внутри рендер разбит по задачам:

- текст: `layout`, `paint`, `draw_text`, `text_width` — cosmic-text, шрифт берётся из
  `~/.config/hudbar/font`, иначе `"JetBrainsMono Nerd Font Propo"`;
- бар: `render` собирает левую и правую группы чипов, `draw_group` рисует чип и его подсветку;
- панели: `render_panel`, `render_av`, `render_clock`, `render_weather`, `panel_rows`;
- поверхности: `draw_bar`, `draw_popup`, `draw_tooltip`, `draw_grab`, `open_panel`, `close_panel`;
- ввод: `on_bar_click`, `on_bar_right_click`, `on_panel_click`, `on_clock_click`, `set_bar_hover`.

`regions: Vec<(Hit, Rect)>` заполняется заново каждый кадр в `draw_group`. Попадание клика
считается по нему, отдельных виджетов нет.

### `src/hud/app/handlers.rs`

Реализации трейтов `smithay-client-toolkit`: `CompositorHandler`, `OutputHandler`, `SeatHandler`,
`PointerHandler`, `KeyboardHandler`, `LayerShellHandler`, `ShmHandler`, `ProvidesRegistryState`.
Они только переводят события Wayland в вызовы методов `App`. `PointerHandler` различает три
поверхности — бар, попап и grab — и зовёт `on_bar_click` / `on_panel_click` / `close_panel`.
`KeyboardHandler` обрабатывает ввод только в режиме пароля wifi, иначе реагирует лишь на Escape.

## Конфиги

Все в `~/.config/hudbar/`:

| Файл | Кто читает | Когда |
| --- | --- | --- |
| `settings.json` | `settings.rs` | при старте и изменении файла |
| `settings.schema.json` | редактор по ключу `$schema` | при открытии настроек |
| `colors.css` | `palette.rs` | каждый проход главного цикла |
| `font` | `ui::configured_font` | один раз на старте |

`settings.json` содержит секции `hudbar`, `notifications`, `osd`, `idle` и `vpn`; описание
ключей, диапазонов и значений по умолчанию — в `docs/settings.schema.json`. Схема ложится рядом с
настройками, а в установочном файле стоит ссылка `"$schema": "./settings.schema.json"`: редактор по
ней проверяет файл и подсказывает ключи. Соответствие схемы коду держат тесты
(`the_settings_schema_matches_the_parser`), а установочного файла — `the_installed_settings_match_the_schema_defaults`.
Установщик перед обновлением объединяет старые настройки и сохраняет backup; исходный файл
`~/.config/hud-settings/settings.json` оставляется без изменений: его читают только легаси-утилиты
`hud-setting` и `hud-migrate-settings` до 01.01.2027, а меню и панель работают исключительно с
`~/.config/hudbar/settings.json` (см. ROADMAP, F).

Ошибки внешних зависимостей и ненулевые коды выхода пишутся в
`~/.local/state/hudbar.log` или `$XDG_STATE_HOME/hudbar.log`. Секреты прячутся по
типу, а не по слову: пароль Wi-Fi уходит в `nmcli` по stdin (`--ask`), в `argv`
его нет вовсе, а строка команды в журнале несёт `<stdin redacted>`; `stderr`
команды с секретом не пишется — `nmcli` умеет вернуть в сообщении сам пароль.
Журнал растёт до 256 КиБ, после чего прошлый уезжает в `hudbar.log.1`: читают его
за последние минуты, а не за месяцы.

Туда же уходит и трассировка: с `HUDBAR_TRACE=1` пишутся создание и закрытие слоёв
(панель, попап, тултип, OSD), `configure` и события вывода. По этим строкам видно, что
было с панелью перед сбоем — например, что композитор закрыл слой при отключении вывода.
Кадр дольше бюджета (16 мс) отмечается строкой «самый долгий кадр за секунду» — одна
строка на окно в секунду, а не на каждый медленный кадр.

В панелях Wi-Fi, Bluetooth, аудио и погоды ошибка теперь отображается отдельно от пустого
списка устройств или отсутствия данных.


## Что трогать при типичных правках

- Сдвинуть или переставить чип на баре — `App::render` в `render/bar.rs`, блок сборки `right`/`left`.
  Ширину иконки относительно подсветки задают `pad_l`/`pad_r`, промежуток между иконками внутри
  одного чипа — `gap_r`.
- Добавить новый чип — завести вариант в `Hit`, поле в `Settings`, данные в `Sys`, и нарисовать
  `Cell` в `render`.
- Новый пункт в выпадающей панели — `panel_rows` и `on_panel_click`.
- Новые данные из системы — функция чтения и запись в `Sys` внутри `sys_loop`, затем `mark()`.
- Цвет или отступ, общий для всех — константы в `ui.rs`.
- Отступ, зазор или ширина подсветки — поля `Theme` в `ui.rs` («тема» тут одна, `normal`).

## История: мост Telegram

Мост удалён 12 октября 2026: код, systemd-юниты и Mini App вынесены из сборки. Код остался в
ветке `telegram-bridge-backup`, если понадобится снова. Плагин opencode для dunst лежит
отдельно и к мосту отношения не имеет: `contrib/opencode/`.
