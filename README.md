# hudbar

Верхняя панель для [niri](https://github.com/YaLTeR/niri) на Wayland. Пишет сама в `wl_shm` через
[tiny-skia](https://github.com/RazrFalcon/tiny-skia), без GTK и без виджетов: каждый кадр — это
пиксельный буфер, который отдаётся композитору через `wlr-layer-shell`.

Один бинарник, один процесс. Данные собираются в фоновых потоках, главный поток только рисует и
обрабатывает ввод.

## Сборка и запуск

```bash
cargo build --release
install -m 755 target/release/hudbar ~/.local/bin/hudbar
pkill -x hudbar; hudbar &
```

Зависимости системы, без которых часть функций молча не работает: `nmcli` (wifi), `bluetoothctl`
(bluetooth), `wpctl` (громкость, PipeWire), `niri` (сокет `$NIRI_SOCKET` для воркспейсов).

`src/bin/sni_test.rs` — отдельный бинарник, не часть панели. Это стенд для отладки протокола
StatusNotifierItem: поднимает фейковый элемент трея, чтобы проверять `tray.rs`.

## Как устроен один кадр

1. Фоновые потоки пишут в `Shared.sys` (это `Mutex<Sys>`) и зовут `Shared::mark()`: ставит
   `dirty` и пишет байт в `wake_tx`.
2. Главный цикл в `src/main.rs` спит в `poll` сразу на двух fd: wayland-сокет и `wake_rx`.
   Проснувшись, забирает события и, если `dirty`, вызывает `App::frame()`.
3. `frame()` рисует бар и открытые поверхности (панель, тултип, grab) в shm-слоты и коммитит их.

Пока идёт анимация (открытие панели или подсветка ховера), `poll` ограничен 16 мс, иначе 500 мс.
Палитра перечитывается с диска каждый проход цикла — так цвета от matugen подхватываются без
перезапуска.

## Карта файлов

```
src/main.rs            точка входа: создаёт поверхности, владеет циклом событий
src/hud/mod.rs         корень модулей, реэкспортит App и ui
src/hud/data/mod.rs    состояние Sys, Shared и координация фоновых циклов
src/hud/data/system.rs батарея, cpu, память, камера, рекордер, DND
src/hud/data/network.rs wifi, bluetooth и сетевые действия
src/hud/data/audio.rs  PipeWire/PulseAudio, громкость и EasyEffects
src/hud/data/weather.rs погода и её обновление
src/hud/data/workspaces.rs воркспейсы и EventStream niri
src/hud/actions.rs     действия пользователя: фокус, звук, микрофон, bluetooth
src/hud/tray.rs        протокол StatusNotifierWatcher / StatusNotifierItem
src/hud/palette.rs     цвета из colors.css
src/hud/settings.rs    что показывать, из settings.json
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

`data/mod.rs` хранит общий снимок `Sys`, `Shared` и поднимает три потока плюс трей:

- `sys_loop` — опрос системы. Батарея раз в 30 тиков, cpu/память раз в 12, громкость и wifi каждый
  тик. Пишет в `Sys` только то, что реально изменилось, и только тогда будит отрисовку.
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

- Константы размеров (`SIZE`, `SCALE`, `PANEL_W`, `PANEL_ROW`, ...) и пиксельная палитра
  `PIXEL_*`, которая используется, когда включён pixel-режим.
- `Theme` — все числа, которыми темы отличаются друг от друга: отступ чипов, зазор между ними,
  размер и отступы иконок трея, пробелы перед процентами звука (`vol_pad`), промежуток между
  иконкой и текстом (`icon_gap`), ширина подсветки wifi (`wifi_extra`) и флаг `full_hover`.
  Два конструктора: `Theme::pixel()` и `Theme::normal()`. Цветов здесь нет — у обычной темы они
  приходят из `colors.css`, у пиксельной остаются константами `PIXEL_*`.
- Иконки `I_*` — глифы шрифта иконок.
- `Hit` — что можно нажать на баре. `PanelKind` — какая панель открыта. `PanelView` — состояние
  панели: список, ввод пароля wifi, календарь.
- `Cell` и `Group` — единица раскладки бара. `pad_l`/`pad_r` входят в ширину ячейки и в область
  подсветки, `gap_r` — промежуток после ячейки, в подсветку не входит. Это важно: зазор между
  wifi и bluetooth задан через `gap_r`, а не через `pad_r`, иначе подсветка шире иконки.
- Примитивы: `fill_rect`, `fill_round_rect`, `draw_pixel_frame`, `draw_pixel_outline`, `blit_argb`.

### `src/hud/app/render/` и `src/hud/app/text.rs`

`App` и основная отрисовка находятся в `render/`. Работа с cosmic-text, шрифтами и отрисовкой
глифов вынесена в `text.rs` через небольшой трейт `TextRenderer`. Внутри рендер разбит по задачам:

- текст: `layout`, `paint`, `draw_text`, `text_width` — cosmic-text, шрифт берётся из
  `~/.config/hudbar/font`, иначе `"Minecraft Rus"`;
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
| `colors.css` | `palette.rs` | каждый проход главного цикла |
| `font` | `ui::configured_font` | один раз на старте |

Ошибки внешних зависимостей и ненулевые коды выхода пишутся в
`~/.local/state/hudbar.log` или `$XDG_STATE_HOME/hudbar.log`. Пароли в аргументах команд
заменяются на `<redacted>`.

В панелях Wi-Fi, Bluetooth, аудио и погоды ошибка теперь отображается отдельно от пустого
списка устройств или отсутствия данных.

## Проверка

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Что трогать при типичных правках

- Сдвинуть или переставить чип на баре — `App::render` в `render/bar.rs`, блок сборки `right`/`left`.
  Ширину иконки относительно подсветки задают `pad_l`/`pad_r`, промежуток между иконками внутри
  одного чипа — `gap_r`.
- Добавить новый чип — завести вариант в `Hit`, поле в `Settings`, данные в `Sys`, и нарисовать
  `Cell` в `render`.
- Новый пункт в выпадающей панели — `panel_rows` и `on_panel_click`.
- Новые данные из системы — функция чтения и запись в `Sys` внутри `sys_loop`, затем `mark()`.
- Цвет или отступ, общий для всех — константы в `ui.rs`.
- Отступ, зазор или ширина подсветки только для одной темы — поля `Theme` в `ui.rs`. Менять
  `Theme::pixel()` и `Theme::normal()` по отдельности, тогда правка одной темы не заденет другую.
  Тема выбирается по шрифту: `Minecraft Rus` даёт пиксельную, любой другой — обычную.
