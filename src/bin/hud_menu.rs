//! `hud-menu-rs` — меню HUDbar поверх всего, только с клавиатуры.
//!
//! Окно сделано слоем `zwlr_layer_shell`, а не `xdg_toplevel`: меню должно
//! появляться поверх панели и всех окон, не меняя раскладку рабочих областей
//! и не требуя `niri msg action focus-window`. Слой в слое `Overlay` без якорей —
//! композитор центрирует его сам, а размер равен карточке из `menu::view`.
//! Когда высота карточки меняется (поиск, число строк), слой получает новый
//! `set_size` и новый буфер.
//!
//! Клавиатура — `Exclusive`: меню открыто по бинду, и первое же нажатие должно
//! дойти до него. Плата — клавиатура не вернётся, пока слой жив, поэтому потеря
//! фокуса (`keyboard leave`) закрывает меню, иначе закрыть его без клавиатуры
//! было бы нечем. `OnDemand` тут опаснее: после чужой комбинации niri мог бы не
//! вернуть фокус без явного запроса, и меню осталось бы с мёртвой клавиатурой.
//!
//! Затемнение не рисуется: под меню должны быть видны терминал и обои. Вместо
//! него карточка делается чуть прозрачной — так размытие слоя видно на её
//! границе, а не только вокруг неё.
//!
//! Всё содержимое рисуется теми же функциями `menu::view`, что и снимки: форка
//! кода между окном и `--snapshot` нет.
//!
//! Запуск:
//!   hud-menu-rs [раздел][/строка]        — открыть меню, по возможности сразу на разделе
//!   hud-menu-rs --snapshot ...  — снимки без Wayland, см. `menu::snapshot`

use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Modifiers, RawModifiers, RepeatInfo},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShell, LayerSurface},
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

#[allow(dead_code)]
#[path = "../hud/bind_data.rs"]
mod bind_data;
#[allow(dead_code)]
#[path = "../hud/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../hud/config_io.rs"]
mod config_io;
#[allow(dead_code)]
// Меню — отдельный процесс: у него нет `Shared.sys` панели, поэтому нужны
// одноразовые читатели. Весь `data` с его потоками и DBus не подключается.
#[path = "../hud/data/oneshot.rs"]
pub mod oneshot_data;

/// Псевдоним `data::oneshot`: в `hudbar` данные лежат в `data`, а этому
/// бинарнику нужен только сам модуль чтений. Псевдоним нужен, чтобы `menu`
/// обращался к чтениям одним и тем же путём в обоих крейтах.
pub mod data {
    pub use super::oneshot_data as oneshot;
}
#[allow(dead_code)]
#[path = "../hud/anim.rs"]
mod anim;
#[allow(dead_code)]
#[path = "../hud/canvas.rs"]
mod canvas;
#[allow(dead_code)]
#[path = "../hud/geom.rs"]
mod geom;
#[allow(dead_code)]
#[path = "../hud/layer.rs"]
mod layer;
#[allow(dead_code)]
#[path = "../hud/log.rs"]
mod log;
#[allow(dead_code)]
#[path = "../hud/menu/mod.rs"]
mod menu;
#[path = "../hud/palette.rs"]
mod palette;
#[allow(dead_code)]
#[path = "../hud/power_profile.rs"]
mod power_profile;
#[path = "../hud/repeat.rs"]
mod repeat;
#[allow(dead_code)]
#[path = "../hud/schemes.rs"]
mod schemes;
#[allow(dead_code)]
#[path = "../hud/settings.rs"]
mod settings;
#[allow(dead_code)]
#[path = "../hud/settings_icons.rs"]
mod settings_icons;
#[path = "../hud/shell.rs"]
mod shell;
#[allow(dead_code)]
#[path = "../hud/text.rs"]
mod text;
#[allow(dead_code)]
#[path = "../hud/ui_tokens.rs"]
mod ui_tokens;
#[allow(dead_code)]
#[path = "../hud/wallpaper.rs"]
mod wallpaper;

use geom::Rect;
use menu::input::MenuInput;
use menu::state::Menu;
use menu::state::Outcome;
use menu::system::{CommandRunner, ProviderKey, spawn_fetch};
use menu::view::{self, MenuView};
use repeat::Repeat;
use text::{SCALE, TextPainter};

/// Пространство слоя: по нему правится размытие в `rules.kdl`.
const NAMESPACE: &str = "hudmenu";
/// Ширина окна равна ширине карточки: окно и есть карточка.
const WIDTH: u32 = view::CARD_W as u32;
/// Высота, которой заведомо хватает под любую карточку: нужна только для
/// размера пула буферов.
const MAX_H: u32 = view::HEADER_H as u32
    + (view::ROW_H_SEARCH * view::MAX_ROWS as f32) as u32
    + view::FOOTER_H as u32;
/// Строк на страницу прокрутки: сколько влезает в карточку целиком.
const PAGE: isize = view::MAX_ROWS as isize;

const USAGE: &str = "\
hud-menu-rs — меню HUDbar

  hud-menu-rs [раздел][/строка]
      Открыть меню. Раздел: apps, panel, style, notifications, capture,
      keybinds, system, power, about. Вложенный уровень — через слэш:
      hud-menu-rs system/control откроет «Панель управления» сразу.
      Неизвестное имя открывает корень.
  hud-menu-rs --action <личность строки>
      Выполнить строку меню по её личности, не открывая окно. Для биндов,
      которым нужно одно нажатие: hud-menu-rs --action system/dnd.
  hud-menu-rs --snapshot <root|style|search|empty> <out.png>
      Снимок состояния без Wayland.
  hud-menu-rs --all <каталог>
      Все состояния.
  hud-menu-rs --real <каталог>
      Настоящее дерево: корень и каждый раздел (отчёт M4).
  hud-menu-rs --icons <каталог>
      Галерея иконок.
  -h, --help
      Эта справка.";

/// Живое окно меню.
struct MenuApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    ctx: layer::Context,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    painter: TextPainter,
    icons: TextPainter,
    menu: Menu,
    /// Скользящая подсветка выбранной строки: позиция в строках экрана.
    /// Состояние живёт здесь, а не в `view`: `view` ничего не помнит.
    highlight: menu::highlight::Highlight,
    palette: palette::Palette,
    /// Пиксельная ли тема сейчас: от неё зависят шрифт и скругления.
    /// Берётся из `settings.json` при старте и после каждой смены темы.
    /// Меню открыли сразу на разделе: `Esc` с него закрывает, а не возвращает
    /// на корень, до которого пользователь не доходил.
    direct: bool,
    /// Остаток дробного скролла: тачпад присылает value120 меньше 120, и без
    /// накопления одно деление давало ноль строк.
    scroll_acc: f32,
    /// Строка под указателем мыши: индекс в `Frame::items` или `None`.
    ///
    /// Живёт здесь, а не в `view`: тот ничего не помнит. На выбор не влияет —
    /// мышь показывает, где кликнёт, но не выбирает.
    hover_row: Option<usize>,
    /// Автоповтор удержанной стрелки: композитор повторов не шлёт.
    repeat: Repeat,
    exit: Arc<AtomicBool>,
    /// Фоновые задачи: системные команды и дозапросы providers. UI-поток
    /// никогда не ждёт — готовый результат забирается `try_take` в тике.
    jobs: Vec<Job>,
}

/// Фоновая задача цикла меню.
struct Job {
    fetch: menu::system::Fetch<JobOutcome>,
}

/// Результат фоновой задачи: команда выполнена или provider отдал строки.
enum JobOutcome {
    /// Команда динамического раздела: ключ раздела, строка-владелец (по ней
    /// снимается отметка «выполняется») и результат выполнения. Флаг
    /// секретов нужен, чтобы не показывать stderr команды, которая получала
    /// пароль: там теоретически может быть и он.
    Command(
        (ProviderKey, String, bool),
        Result<menu::system::CommandOutput, String>,
    ),
    /// Provider отдал строки уровня: ключ, поколение, результат.
    Provider(ProviderKey, u64, Result<Vec<menu::tree::Node>, String>),
}

/// Worker для provider: реальный опрос системы уезжает в поток, результат —
/// пакетным `JobOutcome` для UI-тика.
fn spawn_provider_job(key: ProviderKey, generation: u64, lang: settings::Language) -> Job {
    Job {
        fetch: spawn_fetch(generation, move || match key {
            ProviderKey::AUDIO => {
                JobOutcome::Provider(key, generation, menu::audio::fetch_nodes(lang))
            }
            ProviderKey::WIFI => {
                JobOutcome::Provider(key, generation, menu::wifi::fetch_nodes(lang))
            }
            ProviderKey::BLUETOOTH => {
                JobOutcome::Provider(key, generation, menu::bluetooth::fetch_nodes(lang))
            }
            ProviderKey::OUTPUTS => {
                JobOutcome::Provider(key, generation, menu::displays::fetch_nodes(lang))
            }
            ProviderKey::STORAGE => {
                JobOutcome::Provider(key, generation, menu::storage::fetch_nodes(lang))
            }
            ProviderKey::VPN => JobOutcome::Provider(key, generation, menu::vpn::fetch_nodes(lang)),
            _ => JobOutcome::Provider(key, generation, Err("provider не реализован".to_string())),
        }),
    }
}

/// Сообщение об ошибке команды для подвала. Команда, которая получила секрет,
/// не показывает свой stderr: NetworkManager туда может вернуть что угодно, а
/// пароль в подвале — это и неудобно, и небезопасно. Остальным командам
/// stderr показывается как есть: они полезны и секретов не несут.
fn safe_failure(stderr: &str, has_secrets: bool, lang: settings::Language) -> String {
    if !has_secrets {
        return stderr.trim().to_string();
    }
    match lang {
        settings::Language::Ru => "Не удалось подключиться".to_string(),
        settings::Language::En => "Connection failed".to_string(),
    }
}

impl MenuApp {
    /// Прямоугольник карточки в окне: окно и есть карточка, поэтому она
    /// начинается в нуле. Высота — текущая высота слоя, а не конечная высота
    /// модели: во время анимации клик и hover должны попадать в то, что
    /// реально нарисовано сейчас.
    fn card(&self) -> Rect {
        Rect::new(0.0, 0.0, WIDTH as f32, self.ctx.height as f32)
    }

    /// Размер, который просит модель. Считается из карточки, поэтому поиск и
    /// пустой список дают верную высоту.
    fn wanted_size(&self) -> (u32, u32) {
        let frame = self.menu.frame();
        view::layer_size(frame.items.len(), view::search_mode(&frame))
    }

    /// Передаёт размер слою. Если высота изменилась, слой поедет к ней
    /// анимацией; повторный вызов с той же целью ничего не перезапускает.
    /// Слой без якорей не имеет права получить ноль ни по одной оси — за это
    /// композитор рвёт протокол, — а нули возвращает только расчёт, а не
    /// композитор.
    fn sync_size(&mut self, now: std::time::Instant) {
        let (width, height) = self.wanted_size();
        debug_assert!(width > 0 && height > 0, "размер слоя не может быть нулевым");
        if height != self.ctx.target_height() {
            debug_log(&format!(
                "высота {height} (строк: {}, поиск: {})",
                self.menu.frame().total,
                !self.menu.query().is_empty(),
            ));
        }
        self.ctx.request_height(width, height, now);
    }

    /// Применяет команду ввода к модели. Модель не знает про окно, поэтому
    /// «закрыть» решается здесь.
    fn apply(&mut self, input: MenuInput) {
        // Пока открыт ввод пароля, модель решает сама, что значит клавиша:
        // обычный маршрут двигал бы выбранную сеть и чистил запрос.
        if self.menu.secret_active() {
            match input {
                MenuInput::Type(ch) => self.menu.type_secret_char(ch),
                MenuInput::Erase => self.menu.secret_backspace(),
                MenuInput::ClearQuery => self.menu.secret_clear(),
                MenuInput::Activate => {
                    if self.menu.submit_secret_input() == Outcome::Changed {
                        self.rebuild_after_change();
                    }
                }
                MenuInput::Close | MenuInput::Back => {
                    self.menu.cancel_secret_input();
                }
                _ => {}
            }
            self.ctx.dirty = true;
            return;
        }
        match input {
            MenuInput::Ignore => {}
            MenuInput::Move(delta) => self.menu.move_sel(delta),
            MenuInput::Volume(delta) => {
                self.menu.adjust_volume(delta);
            }
            MenuInput::Nudge(delta) => {
                if self.menu.nudge_module(delta as i32) == Outcome::Changed {
                    let lang = self.menu.lang();
                    self.menu.retree(menu::real::tree_in(lang));
                }
            }
            MenuInput::Page(lines) => self.menu.scroll(lines * PAGE),
            MenuInput::Jump(end) => {
                if end {
                    self.menu.current_mut().list.select_last();
                } else {
                    self.menu.current_mut().list.select_first();
                }
            }
            MenuInput::Activate => self.activate(),
            MenuInput::Back => {
                // Прямой вход: Esc с открытого раздела закрывает меню, а не
                // возвращает на корневый уровень, которого не было на экране.
                if self.menu.depth() <= usize::from(self.direct && self.menu.depth() == 1) {
                    self.close();
                } else {
                    self.menu.back();
                }
            }
            MenuInput::Close => self.close(),
            MenuInput::Erase => self.menu.backspace(),
            MenuInput::ClearQuery => self.menu.clear_query(),
            MenuInput::Type(ch) => self.menu.type_char(ch),
        }
        self.ctx.dirty = true;
    }

    /// Enter: войти в подменю или применить значение. Статус после действия
    /// остаётся в подвале: это подтверждение или ошибка, а не мусор.
    /// Приложения, снимки и обои закрывают меню сразу после запуска.
    fn activate(&mut self) {
        let action = self
            .menu
            .current()
            .list
            .selected_item()
            .and_then(|item| match &item.kind {
                menu::item::ItemKind::Action(action) => Some(action.clone()),
                _ => None,
            });
        if self.menu.enter() == Outcome::Changed {
            self.rebuild_after_change();
            if action.is_some_and(|action| action.closes_menu()) {
                self.close();
            }
        }
    }

    /// Пересборка дерева с возвратом на прежнее место. Положение (уровни по
    /// личностям + выбранные строки) снимается ДО `retree`: тот возвращает
    /// меню к корню, а `restore` поднимает обратно вместе с курсором.
    fn rebuild_after_change(&mut self) {
        let position = self.menu.position();
        let status = self.menu.status().cloned();
        self.sync_theme_lang();
        let lang = self.menu.lang();
        self.menu.retree(menu::real::tree_in(lang));
        self.menu.restore(&position);
        if let Some(status) = status {
            self.menu.set_status(status);
        }
    }

    /// Сверяет тему и язык с файлом после изменения. Смена темы пересоздаёт
    /// painter на шрифте новой темы — отсюда мгновенная перерисовка.
    /// Смена языка перестраивает дерево: заголовки хранятся в нём.
    fn sync_theme_lang(&mut self) {
        let current = settings::load();
        if current.language != self.menu.lang() {
            self.menu.set_lang(current.language);
            self.menu.retree(menu::real::tree_in(current.language));
        }
        self.menu.refresh();
    }

    fn close(&mut self) {
        debug_log(&format!(
            "закрытие по команде, глубина {}",
            self.menu.depth()
        ));
        self.menu.clear_status();
        self.exit.store(true, Ordering::Relaxed);
    }

    /// Клик по строке: выбрать и сразу активировать, как в списках файлов.
    fn click(&mut self, x: f32, y: f32) {
        let frame = self.menu.frame();
        let card = self.card();
        if let Some(index) = view::selectable_row_at(&frame, card, x, y) {
            self.menu.current_mut().list.select(index);
            self.activate();
        }
    }

    /// Строка под указателем. Тот же попадающий по геометрии расчёт, что и у
    /// клика, но ничего не выбирает.
    fn row_under_pointer(&self, x: f32, y: f32) -> Option<usize> {
        let frame = self.menu.frame();
        view::selectable_row_at(&frame, self.card(), x, y)
    }

    /// Разбирает клавишу и применяет команду. Вынесено, чтобы нажатие и
    /// повтор шли одним кодом.
    fn handle_key(&mut self, event: KeyEvent) {
        debug_log(&format!(
            "клавиша keysym={:#x} {:?} ctrl={} super={}",
            u32::from(event.keysym),
            event.utf8,
            self.mods.ctrl,
            self.mods.logo
        ));
        let key = menu::input::event(
            event.keysym.into(),
            event.utf8.as_deref(),
            self.mods.ctrl,
            self.mods.shift,
            self.mods.logo,
        );
        // В режиме ввода секрета своя таблица клавиш: `Esc` отменяет ввод, а
        // не возвращает на уровень, и стрелки не трогают список сетей.
        let input = if self.menu.secret_active() {
            menu::input::translate_secret(key)
        } else {
            menu::input::translate(key, self.menu.query())
        };
        debug_log(&format!("команда {input:?}"));
        self.apply(input);
    }

    /// Рисует кадр и отдаёт буфер слою.
    fn draw(&mut self) {
        let _frame = log::Frame::start("меню");
        // Степень проявления окна на текущий момент. Считается один раз на
        // кадр и уходит в общий множитель альфы.
        let now = std::time::Instant::now();
        let openness = self.ctx.openness_now(now);
        let frame_now = self.menu.frame();
        debug_log(&format!(
            "слоев: {} строк: {} выбрано: {:?} top: {} первая: {:?}",
            frame_now.items.len(),
            frame_now.total,
            frame_now.selected,
            frame_now.top,
            frame_now.items.first().map(|item| item.title.as_str())
        ));
        // Подсветка выбранной строки. `update` идёт до чтения позиции: он
        // может запустить ход, и тогда читать надо уже новую.
        self.highlight.update(
            menu::highlight::level_of(self.menu.depth(), &frame_now.query, frame_now.items.len()),
            menu::highlight::position(frame_now.selected, frame_now.top),
            now,
            self.ctx.anim,
        );
        let highlight_row = self.highlight.row(now);
        // Первый кадр рисуется сразу после нашего `set_size`: ждать configure
        // нельзя, иначе меню мигнёт пустым прямо после открытия.
        if self.ctx.width == 0 || self.ctx.height == 0 {
            return;
        }
        // Буфер ровно по размеру слоя и по тому же масштабу, что и damage:
        // иначе niri посчитает кадр обрезанным.
        let pw = (self.ctx.width as f32 * SCALE) as u32;
        let ph = (self.ctx.height as f32 * SCALE) as u32;
        debug_log(&format!("кадр {pw}x{ph}, масштаб {SCALE}"));
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        // Буфер заливается прозрачным: сквозь него видно рабочий стол, а
        // правило слоя размывает то, что под карточкой.
        pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
        let frame = self.menu.frame();
        let (ui, scale) = view::theme(self.palette);
        let mut window = MenuView {
            pixmap: &mut pixmap,
            painter: &mut self.painter,
            icons: &mut self.icons,
            ui,
            scale,
        };
        let card = view::render_window(
            &mut window,
            &frame,
            view::RowsHighlight {
                sliding: highlight_row,
                hover: self.hover_row,
            },
        );
        view::apply_card_alpha(&mut pixmap, card, view::CARD_ALPHA);
        layer::fade(&mut pixmap, openness);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.ctx
                .pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            // Буфер занят панелью: попробуем на следующем тике.
            self.ctx.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.ctx.layer.wl_surface();
        surface.set_buffer_scale(SCALE as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.ctx.layer.commit();
        shell::finish_fade(&mut self.ctx.dirty, openness);
    }
}

impl KeyboardHandler for MenuApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
    }

    /// Потеря клавиатуры закрывает меню. Иначе слой с `Exclusive` забрал бы
    /// клавиатуру себе и вернул её только после закрытия, а закрыть его без
    /// клавиатуры нечем.
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        debug_log("закрытие: клавиатура ушла с меню (keyboard leave)");
        self.close();
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat.press(&event);
        self.handle_key(event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat.release(&event);
    }

    /// Удержание стрелки. Композитор повторов не шлёт: он один раз сообщает
    /// rate и delay через `repeat_info`, а повторные нажатия клиент обязан
    /// выдавать сам. Поэтому `repeat_key` сюда почти не приходит, а движение
    /// при удержании порождает `Repeat::poll()` в цикле ниже.
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(event);
    }

    /// Композитор сообщил rate и delay удержания. Без этого меню знать не
    /// чему, с какой частотой повторять стрелку.
    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        info: RepeatInfo,
    ) {
        self.repeat.set_info(info);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = modifiers;
    }
}

impl PointerHandler for MenuApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != *self.ctx.layer.wl_surface() {
                continue;
            }
            let position = (event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                // Меню работает с клавиатуры, поэтому указатель только
                // подсвечивает строку, но никогда её не выбирает: иначе
                // случайное движение мыши сдвигало бы выбор.
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let index = self.row_under_pointer(position.0, position.1);
                    if index != self.hover_row {
                        self.hover_row = index;
                        self.ctx.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.hover_row.take().is_some() {
                        self.ctx.dirty = true;
                    }
                }
                PointerEventKind::Press { button: 0x110, .. } => self.click(position.0, position.1),
                PointerEventKind::Axis { vertical, .. } => {
                    let scroll = vertical;
                    // Колесо — построчно, а не постранично.
                    let steps = shell::axis_steps(&mut self.scroll_acc, &scroll);
                    let dir = steps.signum();
                    for _ in 0..steps.abs() {
                        self.apply(MenuInput::Move(dir));
                    }
                }
                _ => {}
            }
        }
    }
}

impl CompositorHandler for MenuApp {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for MenuApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for MenuApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl smithay_client_toolkit::shell::wlr_layer::LayerShellHandler for MenuApp {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if layer.wl_surface() == self.ctx.layer.wl_surface() {
            debug_log("закрытие: композитор прислал closed");
            self.close();
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        _: u32,
    ) {
        if layer.wl_surface() != self.ctx.layer.wl_surface() {
            return;
        }
        // Нули от композитора игнорируются: слой без якорей не должен
        // получать нулевой размер. Пока идёт анимация высоты или не
        // подтверждён последний `set_size`, размер из configure — ответ на наш
        // же запрос, и высоту он не откатывает. Первый configure запускает
        // проявление.
        let fallback = self.wanted_size();
        self.ctx.apply_configure_animated(
            (configure.new_size.0, configure.new_size.1),
            fallback,
            std::time::Instant::now(),
        );
        debug_log(&format!(
            "configure {:?} → свои {}x{}",
            (configure.new_size.0, configure.new_size.1),
            self.ctx.width,
            self.ctx.height
        ));
    }
}

impl ShmHandler for MenuApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for MenuApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(MenuApp);
smithay_client_toolkit::delegate_dispatch2!(MenuApp);

/// Отладочная строка в stderr. Отдельной зависимости ради логов заводить не
/// хочется: достаточно `RUST_LOG=debug`, который читают и нашёлки, и
/// инструменты вроде `systemctl --user`.
fn debug_log(message: &str) {
    let level = std::env::var("RUST_LOG").unwrap_or_default();
    if level.contains("debug") || level.contains("trace") {
        eprintln!("hud-menu: {message}");
    }
}

/// Флаг остановки, доступный обработчику сигнала: обработчик не может
/// захватить состояние приложения, поэтому читает общий атомарный флаг.
static SIGNAL_EXIT: AtomicBool = AtomicBool::new(false);

/// Обработчик SIGTERM: только атомарная запись. Всё остальное делает обычный
/// код выхода — так требования `signal-safety` соблюдены.
extern "C" fn on_sigterm(_signal: i32) {
    SIGNAL_EXIT.store(true, Ordering::Relaxed);
}

/// Действие по личности строки, без окна: бинд зовёт HUDbar напрямую, а
/// скрипт-обёртка ему не нужен.
mod action {
    use crate::menu::tree::{Node, NodeKind};

    pub fn run_by_id(id: &str) -> Result<(), String> {
        if id.is_empty() {
            return Err("--action требует личность строки".to_string());
        }
        let root = crate::menu::real::tree();
        let Some(kind) = find(&root, id) else {
            return Err(format!("нет строки {id}"));
        };
        match kind {
            NodeKind::Action(action) => action.perform(),
            NodeKind::Toggle { get, set } => {
                set(!get());
                Ok(())
            }
            _ => Err(format!("строка {id} не действие")),
        }
        .map_err(|error| format!("{id}: {error}"))
    }

    /// Поиск по всему дереву: строка может лежать во вложенном разделе, как
    /// `system/dnd` в «Системе».
    pub(super) fn find<'a>(nodes: &'a [Node], id: &str) -> Option<&'a NodeKind> {
        for node in nodes {
            if node.identity() == id {
                return Some(&node.kind);
            }
            if let NodeKind::Submenu(children) = &node.kind
                && let Some(found) = find(children, id)
            {
                return Some(found);
            }
        }
        None
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return;
    }
    if let Some(mode) = args.first().map(String::as_str) {
        let outcome = match mode {
            "--snapshot" => menu::snapshot::run(&args[1..]),
            "--all" => render_all(args.get(1).map(String::as_str)),
            "--real" => render_real(args.get(1).map(String::as_str)),
            "--icons" => render_icons(args.get(1).map(String::as_str)),
            "--action" => action::run_by_id(args.get(1).map(String::as_str).unwrap_or("")),
            _ => run(args.first().map(String::as_str).unwrap_or("")),
        };
        if let Err(error) = outcome {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run("") {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// `--all <каталог>`: все состояния в обеих темах.
fn render_all(dir: Option<&str>) -> Result<(), String> {
    let dir = dir.ok_or("--all требует путь к каталогу")?;
    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path).map_err(|error| format!("не создался каталог {dir}: {error}"))?;
    for file in menu::snapshot::render_all(path)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// `--real <каталог>`: настоящее дерево, корень и разделы в обеих темах.
fn render_real(dir: Option<&str>) -> Result<(), String> {
    let dir = dir.ok_or("--real требует путь к каталогу")?;
    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path).map_err(|error| format!("не создался каталог {dir}: {error}"))?;
    for file in menu::snapshot::render_real_all(path)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// `--icons <каталог>`: галерея иконок в обеих темах.
fn render_icons(dir: Option<&str>) -> Result<(), String> {
    let dir = dir.ok_or("--icons требует путь к каталогу")?;
    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path).map_err(|error| format!("не создался каталог {dir}: {error}"))?;
    for file in menu::snapshot::render_icon_gallery(path)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// Живой цикл меню.
fn run(section: &str) -> Result<(), String> {
    // Один экземпляр: слой не виден в `niri msg windows`, поэтому единственный
    // способ узнать, открыто ли меню, — pid-файл.
    let pid_file = menu::instance::pid_path();
    if let menu::instance::Startup::CloseRunning(pid) =
        menu::instance::startup(std::path::Path::new("/proc"))
    {
        debug_log(&format!("меню уже открыто: pid {pid}"));
        // Повторный вызов закрывает: сигнал, а не второй слой.
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        )
        .map_err(|error| format!("не закрыть прошлый pid {pid}: {error}"))?;
        return Ok(());
    }
    let pid = std::process::id() as i32;
    if let Err(error) = menu::instance::store(&pid_file, pid) {
        return Err(format!("не записать pid-файл: {error}"));
    }
    debug_log(&format!("pid {pid}, файл {}", pid_file.display()));
    // Идентификатор раздела проверяется здесь: неизвестный открывает корень.
    let known = menu::has_section(section);
    let result = event_loop(section, &pid_file);
    // pid-файл снимается на любом выходе, включая падение: иначе следующий
    // запуск увидит «живой» pid и не откроет меню.
    menu::instance::clear(&pid_file, pid);
    debug_log("pid-файл снят");
    let _ = known;
    result
}

/// Основной цикл: слой, ввод, перерисовка.
fn event_loop(section: &str, pid_file: &std::path::Path) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "нет zwlr_layer_shell".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;

    // Модель заводится до слоя: её размер нужен уже первому commit. Слой без
    // якорей, получивший `0` по ширине или высоте, — ошибка протокола, и
    // композитор убивает клиента прямо на старте. Дерево настоящее, язык и
    // тема — из `settings.json`.
    let mut menu = menu::real::menu();
    let user_anim = settings::load().animations;
    // На энергосбережении анимации гаснут сами, настройку не трогаем.
    let anim_on = user_anim && power_profile::anim_allowed();
    if user_anim && !anim_on {
        debug_log("энергосбережение: анимации выключены на время меню");
    }
    let anim = anim::AnimSettings::new(anim_on, settings::load().animation_speed);
    let started = std::time::Instant::now();
    let mut direct = false;
    if menu::open_section(&mut menu, section) {
        direct = true;
    }
    let start = menu.frame();
    let (start_w, start_h) = view::layer_size(start.items.len(), view::search_mode(&start));

    // Модель + слой заводятся через общий layer::open: поверхность, слой,
    // set_size до первого commit и пул буферов под максимальную карточку —
    // поведение то же, что было здесь по месту, но реализация одна.
    let ctx = layer::open(
        &compositor,
        &layer_shell,
        &shm,
        &qh,
        layer::Shape {
            namespace: Some(NAMESPACE),
            width: start_w,
            height: start_h,
            max_height: MAX_H,
            scale: SCALE as u32,
        },
        started,
        anim,
    )?;
    debug_log(&format!(
        "слой {NAMESPACE}: set_size {start_w}x{start_h} до commit"
    ));

    let mut app = MenuApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        ctx,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        painter: TextPainter::new(&ui_tokens::font_name()),
        icons: TextPainter::new(settings_icons::FONT),
        menu,
        highlight: menu::highlight::Highlight::new(started),
        palette: palette::load(),
        scroll_acc: 0.0,
        hover_row: None,
        repeat: Repeat::default(),
        direct,
        exit: Arc::new(AtomicBool::new(false)),
        jobs: Vec::new(),
    };
    // Размер уже задан до первого commit; sync_size только досылает его, если
    // карточка успела измениться.
    app.sync_size(std::time::Instant::now());
    let _ = pid_file;

    unsafe {
        let _ = nix::sys::signal::signal(
            nix::sys::signal::Signal::SIGTERM,
            nix::sys::signal::SigHandler::Handler(on_sigterm),
        );
    }

    while !app.exit.load(Ordering::Relaxed) && !SIGNAL_EXIT.load(Ordering::Relaxed) {
        if event_queue.flush().is_err() {
            break;
        }
        let _ = event_queue.dispatch_pending(&mut app);
        // Один момент на итерацию: проявление и высота считаются по нему,
        // иначе разные значения читались бы в разные instants и кадр дёргался.
        let now = std::time::Instant::now();
        // Модель могла сменить уровень или запрос во время dispatch: цель
        // высоты сверяется после ввода, а тик отправляет текущий кадр
        // анимации. Повтор с той же целью твин не перезапускает.
        app.sync_size(now);
        app.ctx.tick_resize(now);
        // Команды и дозапросы providers из модели уезжают в worker-ы, готовые
        // результаты возвращаются в слоты. UI-поток ничего не ждёт.
        for job in app.menu.take_pending_commands() {
            let (refresh, commands, generation, row) =
                (job.refresh, job.commands, job.generation, job.row);
            let has_secrets = commands.iter().any(|command| command.has_secrets());
            // Пресет — это несколько команд подряд, и каждой нужно её
            // собственное ожидание целиком, а не общее на всех: медленная
            // первая команда не должна съедать время последней.
            let timeout = commands
                .iter()
                .map(|command| command.timeout())
                .max()
                .unwrap_or_else(|| std::time::Duration::from_secs(1));
            app.jobs.push(Job {
                fetch: spawn_fetch(generation, move || {
                    let runner = menu::system::SystemCommandRunner::with_timeout(timeout);
                    let mut failure = None;
                    for command in &commands {
                        let result = runner.run(command);
                        let done = match &result {
                            Ok(output) => output.success(),
                            Err(_) => false,
                        };
                        if !done {
                            failure = Some(result.map_err(|error| error.to_string()));
                            break;
                        }
                    }
                    JobOutcome::Command(
                        (refresh, row, has_secrets),
                        failure.unwrap_or_else(|| Ok(menu::system::CommandOutput::empty_success())),
                    )
                }),
            });
        }
        for (key, generation) in app.menu.take_dynamic_requests() {
            app.jobs
                .push(spawn_provider_job(key, generation, app.menu.lang()));
        }
        let mut index = 0;
        while index < app.jobs.len() {
            match app.jobs[index].fetch.try_take() {
                Some(JobOutcome::Command((refresh, row, has_secrets), result)) => {
                    app.menu.clear_busy_row(&row);
                    match result {
                        Ok(output) if output.success() => {
                            // Успех: перезапрашиваем snapshot того же раздела.
                            // Запрос в очереди модели разрядится ниже.
                            app.menu.refresh_dynamic(refresh);
                        }
                        Ok(output) => {
                            // Пересканирование NetworkManager может быть
                            // отклонено по rate limit: это не сбой раздела,
                            // список всё равно показываем.
                            if menu::wifi::is_rate_limited(&output.stderr) {
                                app.menu.refresh_dynamic(refresh);
                            } else {
                                app.menu
                                    .set_status(menu::state::Status::Failed(safe_failure(
                                        &output.stderr,
                                        has_secrets,
                                        app.menu.lang(),
                                    )));
                            }
                        }
                        Err(error) => {
                            app.menu
                                .set_status(menu::state::Status::Failed(safe_failure(
                                    &error,
                                    has_secrets,
                                    app.menu.lang(),
                                )))
                        }
                    }
                    app.jobs.remove(index);
                    app.ctx.dirty = true;
                }
                Some(JobOutcome::Provider(key, generation, result)) => {
                    app.menu.apply_dynamic(key, generation, result);
                    app.jobs.remove(index);
                    app.ctx.dirty = true;
                }
                None => index += 1,
            }
        }
        // Запросы, выставленные refresh из успешных команд — сразу в worker-ы.
        for (key, generation) in app.menu.take_dynamic_requests() {
            app.jobs
                .push(spawn_provider_job(key, generation, app.menu.lang()));
        }
        // Пока идёт проявление или ход подсветки, кадр нужен каждый тик: иначе
        // окно проявлялось бы ступенями по чужим событиям ввода, а подсветка
        // доезжала бы только к следующему нажатию.
        if app.ctx.animating(now) || app.highlight.animating(now) {
            app.ctx.dirty = true;
        }
        // Рисовать можно только после первого configure: буфер, приложенный
        // раньше ack, композитор рвёт протоколом. Размер слоя при этом уже
        // задан `set_size` до первого commit — это другое требование.
        if app.ctx.configured && app.ctx.dirty {
            app.ctx.dirty = false;
            app.draw();
        }
        // Повтор удержанной стрелки: композитор его не шлёт, поэтому крутим сами.
        // Пока клавиша нажата, циклу нельзя спать дольше интервала повтора.
        let mut repeat_fired = false;
        while let Some(event) = app.repeat.poll() {
            app.handle_key(event);
            repeat_fired = true;
        }
        if repeat_fired && app.ctx.configured {
            app.ctx.dirty = false;
            app.draw();
        }
        if app.exit.load(Ordering::Relaxed) || SIGNAL_EXIT.load(Ordering::Relaxed) {
            break;
        }
        // События ждутся с таймаутом, а не вечно: SIGTERM должен закрывать
        // меню сразу, а не после следующего сообщения композитора. Пока есть
        // фоновые задачи (providers/команды), тик короче — результат worker-а
        // подхватывается быстро, без ожидания ввода.
        // В простое тик длинный: от его частоты не зависит ничего — ни
        // курсор (в карточке его нет), ни таймеры (их в окне нет вовсе).
        // Пока есть фоновые задачи, тик короткий: результат worker-а
        // подхватывается без ожидания ввода. С удержанной стрелкой тик
        // ещё короче — иначе повтор шёл бы рывками.
        // Проявление и ход подсветки идут кадрами: ждать дольше 16 мс
        // значило бы проявлять окно и гнать подсветку ступенями.
        // Грязный флаг тоже будит короткий тик: недожатое проявление
        // должно дойти до непрозрачного кадра сразу, а не через простой.
        let moving = app.ctx.animating(now) || app.highlight.animating(now) || app.ctx.dirty;
        let idle = if !moving && app.jobs.is_empty() {
            250u16
        } else {
            16u16
        };
        let wait = shell::wait_ms(app.repeat.wait_hint(), idle);
        shell::poll_read(&mut event_queue, conn.as_fd(), wait);
    }
    Ok(())
}

#[cfg(test)]
mod action_tests {
    #[test]
    fn the_tree_has_rows_a_bind_can_call_by_id() {
        // Бинды зовут `--action <id>`, поэтому у каждой такой строки должна
        // быть личность, иначе нажатие молча ничего не сделает.
        for id in ["system/dnd", "system/coffee", "system/power-profile"] {
            let root = crate::menu::real::tree();
            assert!(
                super::action::find(&root, id).is_some(),
                "нет строки {id} для бинда"
            );
        }
    }

    #[test]
    fn an_unknown_id_is_an_error_rather_than_a_silent_no_op() {
        assert!(super::action::find(&crate::menu::real::tree(), "system/нет").is_none());
    }
}
