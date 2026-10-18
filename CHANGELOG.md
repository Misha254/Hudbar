# Changelog

## [Unreleased]

### Changed
- Меню: в «Системе» появился раздел «О системе» — ОС и ядро, устройство из dmi,
  процессор, видеокарта, ОЗУ, аптайм и здоровье батареи; только чтение /proc и /sys,
  без внешних команд, строки пропадают, если данных нет.
- Окно биндов и окно yazi-биндов: выделение как в меню — строка залита акцентом
  целиком, ключ и описание на ней тёмным `on_primary` вместо слабой подложки и
  полоски-метки.
- Меню: вход накопителей в «Системе» переименован в «Накопители» и получил иконку диска
  вместо батареи — без повтора названия группы «Устройства».
- Панель накопителей: строка «дисков: N» и подменю каждого диска рисуются иконкой диска
  вместо батареи.
- Панель накопителей: готовый список больше не показывает некликабельную строку-счётчик
  — диски стоят первыми; статус остаётся при загрузке, ошибке и пустом списке.

## [0.12.0] - 2026-10-18

### Changed
- Меню: убраны hairline-разделители между строками — список читается подсветкой и отступами.
- Меню: выбранная строка залита акцентом целиком вместо слабой подложки с левой полоской;
  текст, иконка, шеврон, значение и полоса громкости на ней рисуются тёмным `on_primary`
  из палитры matugen (в Pixel — тёмно-синий темы). Роль `on_primary` добавлена в `UiPalette`.
- Меню: hover только у строки под курсором и только не на выбранной — подложка без смены
  цвета текста; выбранная строка остаётся акцентом даже под курсором.
- Меню: шеврон ставится только пунктам, чей Enter открывает другой экран (Submenu, Leaf,
  Picker); у Toggle, Choice, Number и Volume он исчезал из-за непустого value, теперь значение
  показывается без стрелки.
- Меню: раздел «Панель» упорядочен — OSD громкости и Срок OSD, Анимации и Скорость идут
  парами, Порядок модулей остался последним.
- Меню: «Панель» разложена на группы — Модули, Вид панели, OSD, Анимации, Порядок модулей;
  в «Системе» связь собрана в «Связь» (Wi-Fi, Bluetooth, VPN), дисплеи и накопители — в
  «Устройства». «Приложения» не тронуты.
- Панель: у батареи вместо процента — остаток в ватт-часах (`energy_now`, а без него
  `charge_now` × `voltage_now`); уровень заряда остаётся на иконке.
- Панель: клик по чипу системы открывает `btop` в kitty со своим классом окна.
- Окно приложений (`hud-apps`) приведено к тому же виду: выбранная строка — сплошной акцент,
  текст, булавка и заглушка иконки — тёмный `on_primary`, hover только не на выбранной строке,
  разделители между строками убраны. Заодно убраны мёртвые токены `selected_bg`,
  `SELECTED_MIX` и `selected_background`: сплошным акцентом пользуется всё, что в них
  нуждалось.

## [0.11.1] - 2026-10-16

### Changed
- `menu/state.rs` (2562 строк) разделён по зонам модели в подкаталог `menu/state/`:
  `levels.rs` — `Level`/`LevelKey` и пересборка уровня, `commands.rs` — очередь команд, отметки
  «выполняется» и `busy_note`, `stack.rs` — открытие уровней и разбор узлов по глубине,
  `dynamic.rs` — запросы и применение снимков динамических разделов, `secret.rs` — ввод пароля,
  `position.rs` — снимок/восстановление положения и переход к листу, `mod.rs` — `Menu`, навигация
  и `Frame`, `tests.rs` — 1402 строки тестов. Приватные поля `Menu` доступны дочерним модулям
  как раньше, внешний API (`Menu`, `Outcome`, `Status`, `Frame`) не изменился, тесты зелёные.
- `menu/view.rs` (2748 строк) разделён по задачам карточки в подкаталог `menu/view/`:
  `geometry.rs` — константы, чистая геометрия и смешение цветов, `header.rs` — крошки и поле поиска,
  `rows.rs` — список, подсветка и индикаторы строки (тумблер, пикер, полоса громкости, шеврон),
  `secret.rs` — режим ввода пароля, `footer.rs` — подвал с подсказками и резервом правой зоны,
  `mod.rs` — `MenuView`, сборка кадра и экспорт, `tests.rs` — весь тестовый модуль (1293 строки).
  Внешние пути `view::*` сохранены реэкспортом, поведение кадра без изменений: побитовые
  снимковые тесты и все 895 проверок зелёные.
- Палитра перечитывается по mtime `colors.css`, а не на каждом проходе главного цикла: раньше полный
  `read_to_string` + разбор шёл десятки раз в секунду во время анимаций, теперь — один stat и одна
  перезарядка при изменении файла. Образец тот же, что у `settings::modified()`; matugen обновляет
  mtime при записи, поэтому подхват цветов без перезапуска сохранён.
- Часы бара и панели собираются из одного вызова `chrono::Local::now()` на проход вместо двух.

## [0.11.0] - 2026-10-16

Схема настроек вместо чтения исходников, журнал с ротацией и трассировкой, секреты мимо `argv`, CI на каждый PR. Снято легаси окна настроек вместе с мёртвым кодом, добавлен PKGBUILD и общий поиск бинарников.

### Added
- Схема настроек `docs/settings.schema.json`: ключи, диапазоны из констант, перечисления, описания и значения по умолчанию всех секций (`language`, `module_order`, `appearance`, `hudbar`, `notifications`, `osd`, `vpn`). Схема ложится рядом с настройками, установочный файл ссылается на неё ключом `$schema`, и редактор проверяет файл и подсказывает ключи. Держат схему вместе с кодом три теста: секции, ключи `hudbar` и диапазоны — с константами парсера, перечисления — с `Language`, `NotificationPosition`, `MODULE_KEYS` и `Theme`, значения по умолчанию — с установочным файлом. Установочный файл дополнен до полного набора: в нём не было `control_button`, `animations`, `animation_speed`, `osd` и `vpn`.
- Журнал: `hudbar.log` растёт до 256 КиБ, после чего прошлый файл уезжает целиком в `hudbar.log.1`. `HUDBAR_TRACE=1` пишет жизненный цикл слоёв — создание и закрытие панели, попапа, тултипа и OSD, `configure`, вход и выход поверхности на вывод, — а кадр дольше бюджета (16 мс) отмечается строкой «самый долгий кадр за секунду»: одна строка на окно в секунду, иначе поток строк вытеснил бы из журнала причину, ради которой его читают. Замер держат панель и пять окон.
- CI на GitHub Actions: `fmt`, `clippy --all-targets -D warnings` и `cargo test --locked` на PR и в `master`, релизная сборка `cargo build --release --locked` на тегах, проверка уязвимостей `cargo-deny` — информационная. Dependabot обновляет cargo, npm и actions раз в неделю.
- PKGBUILD (`packaging/`): сборка из тега, восемь бинарников, девять обёрток и скриптов, файлы по умолчанию и схема в `/usr/share/hudbar/`, `depends` по `ldd`, `optdepends` — программы, которые панель зовёт по надобности. Сообщения пакета объясняют, что делать с конфигом, и не трогают домашний каталог.
- Бинарники ищут друг друга по одному правилу: `HUDBAR_BIN_DIR`, затем `~/.local/bin`, затем `PATH`. Правило живёт в шести обёртках, в `hud-setting` и `hud-theme` и в окне обоев, которое открывает окно схем, — поэтому установка в `/usr/bin` работает без единой переменной окружения. Нет бинарника нигде — обёртка говорит, что именно она искала.
- У старого каталога настроек появился срок: `~/.config/hud-settings/` читается до 01.01.2027. Запасной путь говорит о сроке одной строкой в `stderr` — и только когда канонического файла нет, иначе строка печаталась бы на каждое нажатие в меню. Битый или недоступный старый файл больше не роняет скрипт.

### Changed
- Секреты не попадают в `argv`: пароль Wi-Fi уезжает в `nmcli --ask` по stdin, потому что `/proc/<pid>/cmdline` читают все, и пароль защищённой сети лежал там всё время, пока шло подключение. Догадки по слову `password` убраны из журнала — прячет пометка (`CmdArg::Secret`, `SecretData`), а не соседнее слово, которое молча ломалось на первом же другом ключе вроде `--psk`. `stderr` команды с секретом не пишется вовсе: nmcli возвращает в сообщении сам пароль.
- Тесты смотрят на код, а не на машину: журнал прогона пишется в `/tmp/hudbar-test-log-<pid>` вместо настоящего `~/.local/state/hudbar.log`, поддельный `/proc` ссылается на существующий файл, программы обоев приходят в тест параметром, а проверка установленных снаружи программ помечена `#[ignore]`, как соседние живые проверки.

### Removed
- Окно настроек: пять модулей на 6307 строк (`settings_ui`, `settings_view`, `settings_snapshot`, `settings_widgets`, `ui_layout`) удалены после того, как живое из них переехало в `geom`, `canvas`, `ui_tokens` и `schemes`. Вместе с окном ушли трейты `View`/`DrawExt` и виджеты, включая `toggle(progress)`.
- `src/hud/dunst.rs` (650 строк): точечная замена шрифта, межстрочного интервала и места появления в `dunstrc` не звалась ни из одного бинарника — живую замену делает `scripts/hud-setting::patch_dunst`.
- `OsdDetector::active`: панель этот вопрос задавать перестала, ответ уже в `poll` и в признаке рендера `osd_alive`.

### Fixed
- Снимки окон пишутся одной `canvas::write_png` вместо трёх копий, различавшихся только текстом ошибок.
- `install.sh` не ставил `hud-wallpaper-rs`, `hud-schemes-rs` и обёртку `hud-wallpaper`: окно обоев искало бинарник схем в `~/.local/bin` и не находило его, а окно обоев было нечем запустить.

## [0.10.0] - 2026-10-15

Общие куски цикла окон в `shell.rs`, тишина анимаций на энергосбережении, колесо построчно и честные иконки лаунчера. Уведомления плагина opencode больше не висят на экране вечно.

### Added
- Общий модуль окон `src/hud/shell.rs`: шаг колеса `axis_steps` (дробные деления тачпада копятся остатком), ожидание сокета `wait_ms`/`poll_read` с учётом автоповтора удержанной клавиши и дожимание проявления `finish_fade`. Раньше инверсия колеса жила в трёх копиях, залипшее проявление — в пяти, и один баг приходилось чинить в каждом окне отдельно.
- Анимации гаснут сами на `power-saver`: `power_profile::anim_allowed()` читает профиль из `energy_performance_preference`. Панель сверяет его раз в 15 с и возвращает движение при выходе из энергосбережения без перезапуска, окна проверяют профиль при старте. Настройка `hudbar.animations` не меняется: на `balanced` и `performance` всё как было.
- Лаунчер: иконки по всем раскладкам тем. `Root` различает тему и базовый каталог (базовому нужен уровень `hicolor/`, а он не подставлялся никогда — одна причина на семь заглушек), добавлены раскладки `<size>/apps`, `apps/<size>` (breeze), `mimetypes/<size>` и `<size>/mimetypes` (Papirus), размеры 16/22/24/36/72/192, символьные — последним резервом. Кандидатов много, а не один: битый SVG отдаёт следующего, а путь в `Icon` без расширения (лаунчер War Thunder указывает на ELF) проверяет одноимённые картинки рядом. Вектор идёт перед растром по всей выдаче.
- Заглушка иконки — отдельная роль `ui.placeholder` со своим порогом 3:1 и потолком смещения к акценту 0.35 (в Pixel смешения нет): провал и ожидание рисуются по-разному, глиф «приложение» — только когда иконки уже не будет.

### Changed
- Колесо листает окна построчно, а не постранично: `Input::Move` вместо `Page`, который прыгал на 10 строк и уносил одно деление в конец списка. Знак общий для всех окон: положительное — вниз.
- Булавка у закреплённого приложения рисуется везде, включая секцию «Закреплённые»: единый признак закреплённости, и выбор строки больше ничего не меняет. Глиф заменён на наклонную канцелярскую кнопку (`f435`) вместо прямого фавикон-пина `f08d`.
- Уведомления плагина opencode получают явный `-t`: 8 с для «готово», 20 с для вопросов, разрешений и ошибок. Без него critical не гаснет — в `~/.config/dunst/dunstrc` не задан `timeout`, и такие уведомления висели на экране, пока их не закрывали руками.
- Счётчик подвала лаунчера считает приложения, а не строки: «выбрано 1 из 45». Подсказки выстроены в три уровня, берётся первый влезший, правая зона подвала зарезервирована по образцам — подсказки не прыгают при появлении статуса.

### Fixed
- Проявление окон доводится до непрозрачного кадра, даже если первый кадр рисовался дольше всей анимации (холодный кэш шрифтов и иконок): раньше на экране оставался полупрозрачный кадр до первого ввода или события.
- Направление колеса приведено к одному знаку: в трёх копиях прокрутка была инвертирована.

## [0.9.0] - 2026-10-13

Панель, меню и окна: общая основа анимаций, контраст карточек, лаунчер приложений вместо rofi. Плагин opencode переведён на API v2. Внешние команды больше не вешают главный поток панели.

### Added
- Общая основа анимаций `src/hud/anim.rs`: easing (`ease_out_cubic`, `ease_in_cubic`), `Tween` (цель меняется с текущего значения, повтор с той же целью — no-op), `AnimSet` для независимых значений по ключу, `AnimSettings` (`enabled`, `speed`) и `quantize` для Pixel-темы. Время всегда приходит параметром `Instant`, внутри модуля `Instant::now()` не вызывается — иначе тесты ничего бы не проверяли.
- Настройки `hudbar.animations` (bool, по умолчанию `true`) и `hudbar.animation_speed` (0.5..2.0, по умолчанию `1.0`) с валидацией как у соседних полей. Правятся из Control Center → «Анимации» новым подразделом: переключатель и число с шагом 0.25. Панель подхватывает файл на каждом тике, окна берут настройки при следующем запуске — это сказано в подписи строки скорости.
- Анимированы: подсветка чипов бара (120 мс), выезд панели (открытие 180 мс, закрытие 120 мс), карточка OSD (140/110 мс), полоса уровня OSD (90 мс), проявление окон меню, обоев, схем и VPN (120 мс), подсветка строки меню и VPN (90 мс), рамка ячейки обоев (100 мс), проявление миниатюр обоев (160 мс). В Pixel-теме позиции округляются до целых строк и ячеек: полупрозрачной кромки в пиксельной графике быть не должно.
- Лаунчер приложений `hud-apps` (`hud-apps-rs` + обёртка): слой `zwlr_layer_shell` в стиле меню HUDbar, список из desktop-файлов, поиск с ранжированием, закрепление на `Ctrl+P` и запуск по `Enter`. Заменяет `rofi -show hudapps` с `hud-drun.sh`. Модуль `src/hud/apps/`: `scan` (XDG-каталоги, локализованные имена, `Exec`, `TryExec`, `Categories`), `search` (ранги: префикс, начало слова, вхождение, desktop-id, `GenericName`, ключевые слова, размытое совпадение), `model` (секции «Закреплённые»/«Все приложения», выбор, прокрутка), `pins` (файл `~/.config/hudbar/apps-pinned` с одноразовым переносом из `~/.config/rofi/pinned`), `launch` (разбор `Exec` по спецификации, `Terminal=true` через kitty, `DBusActivatable=true` через `gtk-launch`), `icons` (поиск по темам иконок, SVG через resvg и PNG, кэш и поток загрузки), `keys`, `view` и `snapshot`. Снимки без Wayland: `hud-apps-rs --snapshot <default|search|empty|pinned> out.png`.
- Внешние команды панели ждут результат не дольше двух секунд: `log::output` звался с главного потока и блокировался на всё время работы процесса, из-за чего зависший `niri`, `nmcli` или `bluetoothctl` вставал колом — ни перерисовки, ни ввода — до самого SIGTERM. Окна схем и обоев подключают `data::oneshot` вместо всего `data` с его потоками и DBus.

### Changed
- Подсветка выбранной строки меню скользит между строками, а рамка выбранной ячейки обоев и подсветка схемы едут между позициями. Вход в подменю, выход, ввод запроса и подгрузка строк провайдером оставляют подсветку на месте: набор пересобран, и «продолжать путь» неоткуда.
- Слой OSD больше не создаётся заново на каждое нажатие: окно живёт, пока оверлей активен, смена громкости, mute и вида обновляет содержимое. Раньше создание и уничтожение layer-shell на каждый шаг давало мигание.
- Цвет текста и иконки выбранной строки меню считается смешением по расстоянию до едущей полосы, а не переключается сразу. Без анимации смешение на концах даёт прежние цвета побитово.
- `settings_widgets::toggle` стала чистой функцией от `(on, progress)`: ручка едет между положениями, цвет трека смешивается по ходу. Снапшоты рисуются на концах хода и не изменились.
- Плагин opencode (`contrib/opencode`) переведён на API v2: `export default { id, setup(ctx) }`, подписка через `ctx.event.subscribe()`, чтение сессии через `ctx.session.get({ sessionID })`. На v1 он не запускался вовсе — загрузчик требует `default` с `id` и `setup`/`effect`, и плагин висел в списке как `failed`. `client.app.log` в v2 нет, журнал ведёт `lib/debug.js`.
- События переписаны под v2: `session.execution.started/succeeded/interrupted/failed` вместо `session.status`/`session.idle`, `permission.asked`/`replied` с `data.id`/`data.requestID`, полезная нагрузка в `event.data`. Старые `session.status`/`session.idle`/`session.error` тоже эмитятся и обрабатываются, помечка «работает» снимается первым же событием, поэтому дубли не проходят.
- Вопрос агента больше не определяется одним `form.created`: в v2 формой называется и выбор провайдера веб-поиска (`kind: "websearch.provider"`, заголовок «Web Search»), и MCP-элицитация (`kind: "mcp-elicitation"`). Плагин писал «Агент задал вопрос / Web Search» на каждую первую веб-поисковую сессию. Теперь уведомляет только `kind: "question"`, а тело берёт из `fields[0].description` (сам вопрос) — заголовок формы у вопросов служебный, «Questions». Повторное событие по той же форме гасится дедупом по `form.id` до `form.replied`/`form.cancelled`.
- Резерв правой зоны подвала меряется один раз на язык, тему и кегль подписи: четыре вызова `text_width` на кадр карточки стоили 0.8 мс при открытом меню. Тест считает промахи, чтобы кэш не стал источником устаревших значений.
- Смена профиля питания показывает уведомление сама операция (`Native::PowerProfileCycle`), а не вызывающий: профиль переключают и строкой меню, и биндом через `--action`, и во втором случае статус показывать некуда. Сбой тоже уведомляется — молча переключившийся профиль хуже явной ошибки.
- Панель переживает закрытие слоя композитором: `closed()` разбирает совпавшую поверхность и пересоздаёт панель после отключения выхода, popup и grab закрывают всю связку, OSD — только себя. Правило вынесено в чистую функцию и проверяется тестом без композитора.

### Fixed
- Рамка карточки и приглушённый текст проходят WCAG с запасом: на 0.65 к акценту рамка читалась второй подсветкой, на 0.35 не отделялась от карточки вовсе, альфа `muted` 0.61 давала 4.54:1 в худшей схеме. Стало 0.55 и 0.63 — 3.28:1 и 4.81:1, у теста появился запас `MUTED_MARGIN`.
- Проявления миниатюр обоев вытесняются под потолок в порядке старта, а не в порядке обхода `HashMap`: под потолок попадал случайный набор, в том числе ещё проявляющийся, и он мигал в 1.0 посреди перехода. Под потолок уходят завершённые, идущее — только когда завершённых не осталось.

### Removed
- `layer::Context::present`, `present_alpha`, сдвиг карточки при появлении окон и кривая `ease_in_out_cubic`: ни у одного вызова в бою не было, а окна открываются слоем ровно по размеру карточки, так что сдвигать было некуда.
- Смена цели в `Tween::retarget` и `AnimSet::set_target` больше не сдвигает отсчёт, если цель не изменилась. Раньше вызывающий, дёргавший цель каждый кадр, бесконечно сбрасывал движение: окно не снималось бы никогда.

## [0.8.0] - 2026-10-10

Меню и панель: громкость ползунком, нативные переключатели вместо скриптов, панель управления
конфигами подменю. Telegram-мост удалён.

## [0.7.0] - 2026-10-08

Telegram-мост: opencode с телефона — промпты, сессии, модели, права и веб-панель.

### Removed
- Мост Telegram удалён: `hud-telegram-rs`, модуль `src/hud/telegram/` (около 9800 строк), systemd-юниты `hud-telegram` и `opencode-serve`, Mini App и раздел про мост из README. Код остался в ветке `telegram-bridge-backup`. Токен бота в `~/.config/hudbar/telegram.json` оставлен на будущее.

### Changed
- `hud-menu-rs` понимает вложенный уровень через слэш: `hud-menu-rs system/control` открывает «Панель управления» сразу. Бинд `super+Escape` переведён с `control.sh` на этот вызов, сам скрипт удалён.
- «Панель управления» больше не запускает `control.sh` с лаунчером rofi: это подменю в меню со списком конфигов, а строки открывают их в `kitty -e nvim`. Из списка ушли noctalia и noctalia-config, добавились waybar, dunst, kitty, `niri/binds.kdl` и сам проект hudbar. Пути, которых нет на диске, строкой не показываются.
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
