# Changelog

## [Unreleased]

## [0.7.0] - 2026-10-08

Telegram-мост: opencode с телефона — промпты, сессии, модели, права и веб-панель.

### Changed
- «Начать поиск» и «Остановить поиск» в Bluetooth объединены в одну строку «Поиск: идёт» / «Поиск: выкл», которая сама запускает и останавливает сканирование. Без строки `Discovering` в выводе остаётся справка без команды — как у питания адаптера.
- Список доступных Wi-Fi точек переехал в подменю «Доступные сети (N)», как «Сопряжённые» и «Найденные» в Bluetooth. В корне раздела остались статус, подменю, ошибка и обновление: раньше первая точка стояла вплотную к строке «Wi-Fi: вкл · сеть» и отличалась от неё только иконкой. Списка в корне больше нет — точек любое количество, обрезка на 12 убрана.
- Menu card transparency now matches the panel exactly (0.9 instead of 0.94). The two windows sit side by side on screen, and the four-point difference read as one being opaque and the other not.

### Fixed
- Меню больше не оставляет пустую строку под последним пунктом: высота карточки считается по реально нарисованным строкам, а не по всей длине списка, из-за чего при прокрутке снизу зияла пустота.
- Длинный динамический раздел (Bluetooth, Wi-Fi) больше не прокручивается при свободном месте: высота страницы пересчитывается после обновления снимка, а не остаётся от первого состояния, когда строк было меньше.

### Changed
- «Выключить Wi-Fi» и «Выключить Bluetooth» убраны: статусная строка сама переключает (`Enter`), иначе раздел повторял одно и то же двумя способами.
- Полоса громкости встала по середине строки и стала длиннее (150 px): место взято у зоны значения, которой 168 px хватало под длинные подписи, а громкости хватает короткой. Уровень и mute видны справа как «40%» и «40% выкл».

### Added
- Audio rows are now one volume slider per device: icon, device name, level bar, percentage and mute. `Shift+←`/`Shift+→` step the volume by 5%, `Enter` toggles mute. The audio section went from eleven rows to five, and the duplicated «Тише на 5%» / «Громче на 5%» / «Mute» pairs (identical for output and input, with no device name) are gone.

### Fixed
- Menu highlight jumped one row down as soon as the list scrolled: the highlighted index added `top` to an already absolute `selected`, so moving to «Режим питания» landed on «Панель управления». The scrolled list also left empty space under the last row: the list scrolled in pages of 8 while the card drew 10, so fewer rows remained than slots. Both fixed, and the page is now set from the drawn row count when a level opens.

### Added
- Мост Telegram (`hud-telegram-rs`) и два юнита в установку: `opencode-serve.service` и `hud-telegram.service`. Ставятся, но не включаются: токен и чат заполняет человек. Секреты — в `~/.config/hudbar/telegram.json` (`bot_token`, `chat_id`), рабочее состояние — в `~/.local/state/hudbar/telegram-state.json`. Команды принимаются только из чата владельца: апдейты других чатов и групп игнорируются.
- Команды бота: `/help`, `/new`, `/sessions`, `/use`, `/stop`, `/status`, `/dir`, `/menu`, `/panel`, `/models`, `/model`, `/yes`, `/always`, `/no`, плюс любой текст как промпт. У каждой есть русское слово (`/меню`, `сессии`, `стоп`, `модели`), а номера в списках совпадают с номерами в тексте — выбор не промахивается.
- Живая статус-карточка: открывается сразу после принятия промпта и правится на месте, пока агент работает (состояние с прошедшим временем, текущий инструмент и его цель). Финал правит её последний раз и снимает клавиатуру. Ответ агента уходит отдельными сообщениями в конце хода, а не в тело карточки, чтобы телефон дал push, а лента не росла как лог.
- Конец хода определяется по `session.idle`, а не по последнему чанку текста, поэтому «готово» не приходит посреди ответа. Зависшие карточки снимает вотчдог по статусам сессий, очередь промптов живёт на мосте, а не в ответе Telegram.
- Плагин opencode (`contrib/opencode/hudbar-notify.js`): уведомления сессии в dunst — `session.idle`, запросы прав, вопросы агента и ошибки. Субагенты молчат, окно с этим opencode в фокусе не спамится, троттлинг по паре (сессия, вид) и не больше трёх «готово» в минуту.
- Смена модели с телефона: `/models` и `/model`. Выбранная модель попадает в состояние и достаётся и активной сессии, и новым сессиям.
- Веб-панель Mini App (`/panel`): HTTP-сервер на localhost (порт 8787) с шестью эндпоинтами и без доступа к файлам и shell. Ссылка `t.me/бот/приложение?startapp=…` приносит токен через Telegram, вводить руками нужно один раз. Панель выключена, пока токена нет, и мост работает и без неё. Наружу её выставляет `tailscale serve` — сам мост слушает только localhost.
- Меню бота кнопками (`/menu`, или `/меню`): одно сообщение, нажатие перерисовывает его же. Корневая панель показывает активную сессию, папку и модель; оттуда — список сессий, список моделей, новая сессия, стоп и справка. В `callback_data` лежит экран и номер строки, а не текст: лимит Bot API — 64 байта, а полный id сессии или `провайдер/id` модели туда иногда не влезает.
- Menu section «Питание» (`hud-menu power`): lock, suspend, log out, reboot and power off in the panel's own style instead of the rofi power menu. `super+P` and the panel row both open it.
- Destructive rows ask first: «Перезагрузить?» and «Выключить?» push a level with «Да, …» and «Отмена»; `Esc` cancels too. The answers stay out of the global search, so nothing can be powered off from search in a single `Enter`.
- `Action::Back`: a row action that returns to the previous level. It has no command, so the runner never receives it.
- `Node::hidden_from_search()`: keeps a row visible in its section while hiding it from global search.

### Fixed
- Клиент Bot API Telegram: `retry_after` разбирается и ждёт, снятие клавиатур и удаление карточек идут по правам бота вместо молчаливой ошибки. Токен уходит в `curl --config -`, а не в `argv`, поэтому в `ps` его нет.
- Мост видит только свою сессию: чужие сессии на том же сервере (тот же `opencode`, открытый в этой папке) молчат, иначе телефон гудел бы от каждой строки рабочего терминала.
- При входе в сессию и при смене сессии чистится вся лента бота, а не только последняя карточка.
- Промпт квотируется целиком: значение с пробелами терялось по дороге в `curl`.
- Смена модели идёт через `/api`, а не через текстовый вывод: HTML-заглушка opencode считается ошибкой, а не успешной сменой.
- Плагин opencode ждёт `permission.replied` перед новым запросом прав, иначе дедуп по `permissionID` глушил бы второе настоящее ожидание.
- Токен Mini App не остаётся в кнопке «назад» и в истории браузера: страница убирает его из адреса через `history.replaceState`.
- Из моста убран зашитый путь `~/code/hudbar`: он стоял и в дефолте папки сессии, и в `WorkingDirectory` юнита, из-за чего у всех, кроме автора, сервис не стартовал, а сессия уезжала в несуществующий каталог. Теперь по умолчанию домашняя папка, юнит работает из любой, а свою папку человек пишет один раз в `telegram-state.json`.
- The panel's «Питание» row opens the menu section instead of `powermenu.sh`, which is no longer referenced from the panel or the menu.

### Docs
- `docs/ROADMAP.md`: уведомления opencode больше не помечены как ненаписанные — плагин лежит в `contrib/opencode/`; мост Telegram добавлен как законченный этап.

## [0.6.0] - 2026-10-07

### Added
- Scheme picker moved out of the wallpaper window into its own layer-shell window (`hud-schemes-rs`), opened automatically right after a wallpaper applies.
- Wallpaper thumbnail grid is now 4 rows at 86 px, larger than before, since the schemes no longer share the space.
- Menu, wallpaper, schemes and VPN windows all scroll through a list while a key is held, at a readable speed.

### Fixed
- Scheme window waits for its initial `configure` before attaching a buffer — the previous check on nonzero dimensions raced with the compositor and could abort the window.
- Scheme window is a singleton: a second launch closes the first instead of stacking a second window.
- The scheme window is started through `setsid`, so it survives the wallpaper window closing after the scheme was applied.
- Empty band removed from the bottom of the scheme card: the height counted two paddings while the hints were anchored to the bottom edge, leaving twice the intended gap.
- Menu and VPN card narrowed from 520 to 440 px, since short rows such as "Панель" left a large empty area. Footer hints fall back to their short set when they no longer fit.

### Docs
- `docs/ROADMAP.md`: removed sections that described already-shipped work (Bluetooth actions, displays, lid), and corrected the three finishing items against the code. The mic OSD is done and was listed as missing; opencode notifications do not exist yet; the stdin secret path is written and tested, but `nmcli` does not read a password from stdin, so that one is a decision rather than a task.

## [0.5.0] - 2026-10-07

### Added
- Coffee mode reads a state file under `/run/user/$UID` instead of scanning the process list.

### Fixed
- The panel survives an output disappearing: layer-surface `closed` no longer ends the process, and the surface is recreated when the output comes back.

### Performance
- Release profile uses thin LTO, a single codegen unit and stripping.
- Palette lookups no longer clone a subtree; glyph checks moved to a const block.
- Panel rendering and JPEG parsing survive poisoned locks instead of unwrapping.

## [0.4.2] - 2026-10-07

### Fixed
- Menu: a snapshot arriving while a nested submenu of a dynamic section was open no longer left stale rows — the dynamic level and everything stacked above it are rebuilt together.
- Menu: when the node a nested level was opened through disappears from the new snapshot, the menu returns to the last level that still exists instead of showing an empty screen.
- Menu: the "working…" marker on a busy row survives a background snapshot refresh of its section.

## [0.4.1] - 2026-10-07

### Fixed
- Bluetooth: a failed `devices Paired`/`devices Connected` read no longer turns into `false` for every device — state is now `Option<bool>` and stays `unknown`.
- Bluetooth: with unknown paired/connected state the device submenu offers no Pair/Connect/Remove/Trust action, only an explicit "state unknown" row.
- Bluetooth: `Powered: None` no longer implies "off" — the power button is hidden when adapter power state is unknown.
- Bluetooth: ordinary refresh is now exactly four `bluetoothctl` calls; the per-device `info <MAC>` N+1 is gone, so refresh no longer scales with device count.
- Wi-Fi: profile SSID is unescaped with the same `split_nmcli_terse` as the network list, so `My\:Network` and `Дом \\ принтер` match their visible networks.
- Wi-Fi: a failed `nmcli radio` read shows "unknown" instead of "off".
- Secret: `SecretInput` and `Menu` no longer implement `Clone`, so a plaintext password cannot be duplicated through ordinary UI state cloning.
- Docs: corrected the claims that the password disappears after submit and that it travels via stdin — it now passes through `CommandSpec`/`CommandJob` and remains visible in the child process `argv`.

## [0.4.0] - 2026-10-07

### Added
- Live HUD menu window (layer-shell overlay) with keyboard navigation, global search and per-section providers.
- Menu providers: audio devices and volume, Bluetooth, Wi-Fi networks, displays with layouts and presets, storage with safe unmount.
- Wallpaper picker window: thumbnail grid, folders, ten matugen schemes, mouse and keyboard control.
- Volume OSD overlay with PipeWire watcher (disabled by default while a compositor freeze is investigated).
- VPN window (`hud-vpn-rs` on numpad 7): three Mihomo modes over `MODE-RU`/`MODE-REST` selectors plus a filtered server list (Nordics, USA, Japan).
- Cyclic arrow-key navigation in menus, wallpaper grid, folders and scheme chips.
- Single-instance toggle for every overlay window via pid-file, with stale and recycled pid protection.

### Fixed
- VPN mode and server switches re-read the controller state, so partial failures are shown honestly.
- HTTP client rejects truncated bodies, oversized responses and non-loopback controller addresses; bearer secret never reaches logs or argv.

## [0.3.0] - 2026-10-03

### Added
- Wallpaper section: recursive scan of `~/wallpapers` (jpg, jpeg, png, webp), per-page scrolling, ten matugen schemes and one explicit apply button.
- `control_button` setting: the gear slot can be hidden from the panel.
- Panel section with per-module switches, reordering and panel height.
- Notifications section for dunst font size, line height and screen corner.
- Controls section: niri keybinds merged per action, clipped to width, read-only.
- Audit of control center actions without a niri bind.

### Changed
- Module zones are laid out from the real content width instead of a fixed constant.
- Overview shows only a summary of theme, enabled modules, height and language.
- Footer hints are per section; the language switcher became two buttons, `РУС` and `EN`.
- Settings are written through single-field patches that keep unknown keys and key order.
- Wallpaper file labels show `folder/name` instead of the full path.

### Fixed
- Row rebuilds no longer drop the wallpaper list, and focus stays on the row you acted on.
- Applying wallpapers reports the script's stderr instead of a bare exit code.
- The placeholder label for unfinished sections is gone.

## [0.2.0] - 2026-10-02

### Added
- Native Rust windows for HUD settings, niri keybinds, and yazi keybinds.
- Built-in control center for network, audio, notifications, wallpapers, power, and lock actions.
- Safe installer, settings migration, and component documentation.
- RAM icon in the status bar.

### Changed
- Store appearance, HUD modules, and notification preferences in one settings file.
- Use native keybind viewer routes instead of the previous Rofi scripts.

### Fixed
- Keep the normal battery chip background while highlighting critically low battery text.
- Refresh battery estimates after power state changes settle.
