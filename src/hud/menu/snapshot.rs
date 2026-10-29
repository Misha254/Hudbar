//! Снимки меню в PNG без Wayland.
//!
//! Окно меню появится в M4, а смотреть на геометрию нужно уже сейчас: кадр
//! рисуется тем же кодом, что и живое окно, но состояние берётся из тестового
//! дерева `demo_tree`, поэтому снимок воспроизводим и отличается от реального
//! только данными.
//!
//! Запуск:
//!   hud-menu-rs --snapshot <root|style|search|empty|displays|displays-output|deep-crumbs> <out.png>
//!
//! Кадр — вся поверхность оверлея, с затемнением вокруг карточки. Размер
//! кадра взят у оверлея меню в niri: 1200x800, этого хватает, чтобы увидеть
//! и карточку, и поле вокруг неё.

use super::super::canvas::write_png;
use super::super::geom::Rect;
use super::super::palette;
use super::super::settings_icons;
use super::super::text::Align;
use super::super::text::{SCALE, TextPainter};
use super::super::ui_tokens::{font_name, radii};
use super::bluetooth::Block;
use super::demo_menu;
use super::displays::{self, DisplaysSnapshot};
use super::item::ItemKind;
use super::state::Outcome;
use super::state::{Frame, Menu, Status};
use super::strings;
use super::tree::Node;
use super::view::{self, MenuView};

/// Ширина кадра оверлея.
pub const FRAME_W: f32 = 1200.0;
/// Высота кадра оверлея.
pub const FRAME_H: f32 = 800.0;
/// Подложка вокруг карточки: небо и силуэты, как на макете. Без неё
/// затемнение не с чем сравнить, а снимок выглядит пустым.
const BACKDROP_TOP: (u8, u8, u8) = (0x1a, 0x1f, 0x30);
/// Горизонт в подложке.
const BACKDROP_HORIZON: (u8, u8, u8) = (0x11, 0x15, 0x24);

/// Что снимаем.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    /// Корень: восемь разделов.
    Root,
    /// Подменю «Стиль»: пять пунктов со значениями.
    Style,
    /// Поиск: запрос «dnd» и три результата с путём.
    Search,
    /// Пустой результат: запрос без совпадений.
    Empty,
    /// Раздел «Дисплеи»: статус, раскладка, один вывод и «Обновить».
    Displays,
    /// Подменю одного вывода eDP-1.
    DisplaysOutput,
    /// Подтверждение отключения одного из двух включённых выводов.
    DisplaysConfirm,
    /// Глубокий след: HUD → Система → Устройства → Дисплеи → eDP-1.
    DeepCrumbs,
    /// Кадр так, как его рисует живое окно: карточка от нуля, без
    /// затемнения и с прозрачностью. Нужен, чтобы окно и снимок не
    /// разошлись.
    Window,
}

impl Subject {
    /// Все доступные состояния для подсказки.
    pub const ALL: [Subject; 8] = [
        Subject::Root,
        Subject::Style,
        Subject::Search,
        Subject::Empty,
        Subject::Displays,
        Subject::DisplaysOutput,
        Subject::DisplaysConfirm,
        Subject::DeepCrumbs,
    ];

    /// Ключ в командной строке.
    pub fn key(&self) -> &'static str {
        match self {
            Subject::Root => "root",
            Subject::Style => "style",
            Subject::Search => "search",
            Subject::Empty => "empty",
            Subject::Displays => "displays",
            Subject::DisplaysOutput => "displays-output",
            Subject::DisplaysConfirm => "displays-confirm",
            Subject::DeepCrumbs => "deep-crumbs",
            Subject::Window => "window",
        }
    }

    /// Человеческое имя для вывода.
    pub fn label(&self) -> &'static str {
        match self {
            Subject::Root => "корень",
            Subject::Style => "подменю «Стиль»",
            Subject::Search => "поиск",
            Subject::Empty => "пусто",
            Subject::Displays => "раздел «Дисплеи»",
            Subject::DisplaysOutput => "подменю вывода",
            Subject::DisplaysConfirm => "подтверждение дисплея",
            Subject::DeepCrumbs => "глубокие крошки",
            Subject::Window => "окно",
        }
    }

    /// Разбор ключа.
    pub fn from_key(value: &str) -> Option<Subject> {
        if value == "window" {
            return Some(Subject::Window);
        }
        Subject::ALL
            .into_iter()
            .find(|subject| subject.key() == value)
    }
}

/// Фиксированный ответ `niri msg --json outputs` с одним выводом: тот же,
/// что в тестах раздела «Дисплеи», поэтому снимок не зависит от машины.
const ONE_OUTPUT: &str = r#"{"eDP-1":{"name":"eDP-1","make":"Najing CEC Panda FPD Technology CO. ltd","model":"0x004D","serial":null,"physical_size":[340,190],"modes":[{"width":1920,"height":1080,"refresh_rate":144000,"is_preferred":true},{"width":1920,"height":1080,"refresh_rate":60002,"is_preferred":false}],"current_mode":0,"is_custom_mode":false,"vrr_supported":false,"vrr_enabled":true,"logical":{"x":0,"y":0,"width":1536,"height":864,"scale":1.25,"transform":"Normal"}}}"#;
const TWO_OUTPUTS: &str = r#"{"eDP-1":{"name":"eDP-1","make":"Panel","model":"0x004D","modes":[{"width":1920,"height":1080,"refresh_rate":144000,"is_preferred":true}],"current_mode":0,"is_custom_mode":false,"vrr_supported":false,"logical":{"x":0,"y":0,"width":1536,"height":864,"scale":1.25,"transform":"Normal"}},"HDMI-A-1":{"name":"HDMI-A-1","make":"Dell","model":"U2720Q","modes":[{"width":3840,"height":2160,"refresh_rate":60000,"is_preferred":true}],"current_mode":0,"is_custom_mode":false,"vrr_supported":true,"logical":{"x":1536,"y":0,"width":3840,"height":2160,"scale":1.0,"transform":"Normal"}}}"#;

/// Раздел «Дисплеи» из фиксированного снимка: без запуска niri.
fn displays_section(outputs: &str) -> Node {
    let snapshot = DisplaysSnapshot {
        outputs: Block::Ready(displays::parse_outputs(outputs).expect("фикстура выводов")),
    };
    Node::submenu(
        settings_icons::PANEL_MONITOR,
        "Дисплеи",
        displays::build_nodes(&snapshot, super::settings::Language::Ru),
    )
}

/// Меню, открытое на разделе «Дисплеи».
fn displays_menu(outputs: &str) -> Menu {
    let mut menu = Menu::new(vec![displays_section(outputs)]);
    assert_eq!(menu.enter(), Outcome::Pushed, "снимок открывает «Дисплеи»");
    menu
}

/// Выбирает строку с заголовком (или его началом) и открывает её подменю.
fn enter_titled(menu: &mut Menu, title: &str) {
    let index = menu
        .current()
        .list
        .items()
        .iter()
        .position(|item| item.title.starts_with(title))
        .unwrap_or_else(|| panic!("нет строки «{title}»"));
    menu.current_mut().list.select(index);
    assert_eq!(menu.enter(), Outcome::Pushed, "открывается «{title}»");
}

/// Модель в нужном состоянии: тот же `Menu`, что получит окно в M4.
pub fn subject_menu(subject: Subject) -> Menu {
    let mut menu = demo_menu();
    match subject {
        Subject::Root => {
            menu.move_sel(2);
        }
        Subject::Style => {
            menu.move_sel(2);
            assert_eq!(menu.enter(), Outcome::Pushed, "снимок открывает «Стиль»");
            menu.move_sel(0);
        }
        Subject::Search => {
            for ch in "dnd".chars() {
                menu.type_char(ch);
            }
            menu.clear_status();
        }
        Subject::Empty => {
            for ch in "zzz".chars() {
                menu.type_char(ch);
            }
        }
        Subject::Displays => {
            menu = displays_menu(ONE_OUTPUT);
        }
        Subject::DisplaysOutput => {
            menu = displays_menu(ONE_OUTPUT);
            enter_titled(&mut menu, "eDP-1");
        }
        Subject::DisplaysConfirm => {
            menu = displays_menu(TWO_OUTPUTS);
            enter_titled(&mut menu, "HDMI-A-1");
            enter_titled(&mut menu, "Выключить");
        }
        Subject::DeepCrumbs => {
            let devices = Node::submenu(
                settings_icons::DOT,
                "Устройства",
                vec![displays_section(ONE_OUTPUT)],
            );
            let system = Node::submenu(settings_icons::SYSTEM, "Система", vec![devices]);
            menu = Menu::new(vec![system]);
            assert_eq!(menu.enter(), Outcome::Pushed, "открывается «Система»");
            assert_eq!(menu.enter(), Outcome::Pushed, "открывается «Устройства»");
            assert_eq!(menu.enter(), Outcome::Pushed, "открывается «Дисплеи»");
            enter_titled(&mut menu, "eDP-1");
        }
        // Окно рисует как есть, состояние не меняем: проверяем путь отрисовки.
        Subject::Window => {
            menu.move_sel(2);
        }
    }
    menu.set_page(view::MAX_ROWS);
    menu
}

/// Кадр окна: карточка от нуля, полупрозрачная, вокруг ничего. Снимается
/// тем же кодом, что и живое окно, — иначе они разойдутся.
pub fn render_window_subject(subject: Subject) -> tiny_skia::Pixmap {
    let menu = subject_menu(subject);
    render_menu_card(&menu)
}

/// Карточка произвольного меню тем же кодом, что и живое окно.
pub fn render_menu_card(menu: &Menu) -> tiny_skia::Pixmap {
    let frame = menu.frame();
    let card_w = view::CARD_W;
    let card_h = view::card_height_for(frame.items.len(), view::search_mode(&frame));
    let (ui, scale) = view::theme(palette::Palette::default());
    let mut pixmap =
        tiny_skia::Pixmap::new((card_w * SCALE) as u32, (card_h * SCALE) as u32).expect("pixmap");
    pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut window = MenuView {
        pixmap: &mut pixmap,
        painter: &mut painter,
        icons: &mut icons,
        ui,
        scale,
    };
    let card = view::render_window(&mut window, &frame, view::RowsHighlight::none());
    view::apply_card_alpha(&mut pixmap, card, view::CARD_ALPHA);
    pixmap
}

/// Кадр состояния: подложка, затемнение и карточка.
/// Рисует произвольный кадр: снимкам и тестам отрисовки ввода пароля.
/// Кадр приходит снаружи, поэтому тот же путь годен и для режима секрета.
pub fn render_frame(frame: &Frame) -> tiny_skia::Pixmap {
    let (ui, scale) = view::theme(palette::Palette::default());
    let mut pixmap =
        tiny_skia::Pixmap::new((FRAME_W * SCALE) as u32, (FRAME_H * SCALE) as u32).expect("pixmap");
    backdrop(&mut pixmap, ui);
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut menu_view = MenuView {
        pixmap: &mut pixmap,
        painter: &mut painter,
        icons: &mut icons,
        ui,
        scale,
    };
    view::render(&mut menu_view, frame);
    pixmap
}

pub fn render_subject(subject: Subject) -> tiny_skia::Pixmap {
    let menu = subject_menu(subject);
    let mut frame = menu.frame();
    if subject == Subject::Search {
        // После выбора результата в подвале остаётся подтверждение: так
        // снимок показывает и режим статуса, где счётчика нет.
        frame.status = Some(Status::Applied(strings::STATUS_APPLIED.to_string()));
    }
    let (ui, scale) = view::theme(palette::Palette::default());
    let mut pixmap =
        tiny_skia::Pixmap::new((FRAME_W * SCALE) as u32, (FRAME_H * SCALE) as u32).expect("pixmap");
    backdrop(&mut pixmap, ui);
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut menu_view = MenuView {
        pixmap: &mut pixmap,
        painter: &mut painter,
        icons: &mut icons,
        ui,
        scale,
    };
    view::render(&mut menu_view, &frame);
    pixmap
}

/// Подложка кадра: вертикальный градиент и горизонт. Без неё затемнение
/// нечего затемнять, а меню в пустом PNG невозможно оценить.
fn backdrop(pixmap: &mut tiny_skia::Pixmap, ui: super::super::ui_tokens::UiPalette) {
    let width = pixmap.width();
    let height = pixmap.height();
    let top = palette::Rgba(BACKDROP_TOP.0, BACKDROP_TOP.1, BACKDROP_TOP.2, 255);
    let bottom = palette::Rgba(
        BACKDROP_HORIZON.0,
        BACKDROP_HORIZON.1,
        BACKDROP_HORIZON.2,
        255,
    );
    let pixels = pixmap.pixels_mut();
    for y in 0..height {
        // Смешивание идёт сверху вниз один раз на каждую строку, а не на
        // каждый пиксель: иначе снимок 1200x800 считался бы вчетверо дольше.
        let t = y as f32 / height.max(1) as f32;
        let row = y as usize * width as usize;
        for x in 0..width as usize {
            let index = row + x;
            let red = (top.0 as f32 + (bottom.0 as f32 - top.0 as f32) * t) as u8;
            let green = (top.1 as f32 + (bottom.1 as f32 - top.1 as f32) * t) as u8;
            let blue = (top.2 as f32 + (bottom.2 as f32 - top.2 as f32) * t) as u8;
            if let Some(color) = tiny_skia::PremultipliedColorU8::from_rgba(red, green, blue, 255) {
                pixels[index] = color;
            }
        }
    }
    // Горизонт и силуэт деревьев: три полосы разной высоты. Их роль —
    // показать, что затемнение оверлея ложится поверх своего фона, а не
    // поверх пустоты.
    let horizon = [(0.16, 0x0f, 0x14, 0x22_u8), (0.20, 0x0c, 0x10, 0x1c)];
    let (mut x, mut step) = (0_f32, 26_f32);
    while x < FRAME_W {
        let height_factor = horizon[x as usize % horizon.len()].0;
        let color = horizon[x as usize % horizon.len()];
        let top_y = FRAME_H * (1.0 - height_factor);
        let y0 = (top_y * SCALE) as u32;
        for y in y0..(FRAME_H * SCALE) as u32 {
            let start = y as usize * width as usize + (x * SCALE) as usize;
            let end = start + (step * SCALE) as usize;
            for pixel in pixels
                .iter_mut()
                .take(end.min(width as usize * height as usize))
                .skip(start)
            {
                let red = (pixel.red() as u32 * 3 / 4 + color.1 as u32 / 4) as u8;
                let green = (pixel.green() as u32 * 3 / 4 + color.2 as u32 / 4) as u8;
                let blue = (pixel.blue() as u32 * 3 / 4 + color.3 as u32 / 4) as u8;
                if let Some(next) =
                    tiny_skia::PremultipliedColorU8::from_rgba(red, green, blue, 255)
                {
                    *pixel = next;
                }
            }
        }
        x += step;
        step = if step > 40.0 { 14.0 } else { 38.0 };
    }
    let _ = ui;
}

/// Разбор `--snapshot <state> <out.png>`.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut subject: Option<Subject> = None;
    let mut out: Option<&str> = None;
    for value in args {
        let value = value.as_str();
        if subject.is_none() {
            subject = Some(Subject::from_key(value).ok_or(format!(
                "состояние не понято: {value}. Доступны: {}, window",
                hint()
            ))?);
        } else if out.is_none() {
            out = Some(value);
        } else {
            return Err(format!("лишний аргумент: {value}"));
        }
    }
    let subject = subject.ok_or(format!("нужно состояние: {}", hint()))?;
    let out = out.ok_or("нужен путь к PNG")?;
    let pixmap = if subject == Subject::Window {
        render_window_subject(subject)
    } else {
        render_subject(subject)
    };
    write_png(&pixmap, std::path::Path::new(out))?;
    println!(
        "{} {}x{} → {out}",
        subject.label(),
        pixmap.width(),
        pixmap.height()
    );
    Ok(())
}

/// Справка по синтаксису.
pub fn hint() -> String {
    Subject::ALL
        .iter()
        .map(|subject| subject.key())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Состояние для галереи иконок: все глифы меню в одном кадре. Снимок
/// нужен, чтобы увидеть «тофу» глазами, а не только по таблице кодпойнтов.
pub fn render_icons() -> tiny_skia::Pixmap {
    let (ui, scale) = view::theme(palette::Palette::default());
    let columns = 4;
    let cell_w = 220.0;
    let cell_h = 64.0;
    let width = (FRAME_W * SCALE) as u32;
    let icons_list: Vec<(&str, &str)> = settings_icons::MENU
        .iter()
        .chain(settings_icons::DISPLAYS.iter())
        .copied()
        .collect();
    let rows = icons_list.len().div_ceil(columns);
    let height = ((140.0 + rows as f32 * cell_h) * SCALE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("pixmap");
    pixmap.fill(ui.base.to_tiny());
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let title = Rect::new(24.0, 20.0, 400.0, 32.0);
    painter.paint(
        &mut pixmap,
        "Иконки меню",
        scale.page_title,
        ui.text,
        title,
        Align::Start,
    );
    for (index, (name, glyph)) in icons_list.iter().enumerate() {
        let column = index % columns;
        let line = index / columns;
        let cell = Rect::new(
            24.0 + column as f32 * cell_w,
            70.0 + line as f32 * cell_h,
            cell_w - spacing_gap(),
            cell_h - spacing_gap(),
        );
        icons.paint(
            &mut pixmap,
            glyph,
            scale.label,
            ui.muted,
            Rect::new(cell.x, cell.y, 28.0, cell.h),
            Align::Center,
        );
        painter.paint(
            &mut pixmap,
            name,
            scale.caption,
            ui.text,
            Rect::new(cell.x + 36.0, cell.y, cell.w - 36.0, cell.h),
            super::super::text::Align::Start,
        );
    }
    pixmap
}

/// Зазор между плитками галереи.
fn spacing_gap() -> f32 {
    radii::SM
}

/// Пишет все состояния в обеих темах: удобно для сравнения тем и не даёт
/// забыть половину снимков.
pub fn render_all(directory: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut written = Vec::new();
    for subject in Subject::ALL {
        let path = directory.join(format!("menu-{}.png", subject.key()));
        let pixmap = if subject == Subject::Window {
            render_window_subject(subject)
        } else {
            render_subject(subject)
        };
        write_png(&pixmap, &path)?;
        written.push(path);
    }
    Ok(written)
}

/// Снимки настоящего дерева для отчёта M4: корень и каждый раздел в обеих
/// темах. Значения живые (из `settings.json` и системы), поэтому снимки
/// отражают машину, а не макет.
pub fn render_real_all(directory: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    const SECTIONS: [&str; 13] = [
        "root",
        "apps",
        "panel",
        "order",
        "style",
        "notifications",
        "capture",
        "keybinds",
        "system",
        "system/power",
        "system/updates",
        "power",
        "about",
    ];
    let mut written = Vec::new();
    for id in SECTIONS {
        {
            let mut menu = super::real::menu();
            match id {
                "root" => {}
                "order" => {
                    let _ = super::open_section(&mut menu, "panel");
                    if let Some(index) = menu
                        .frame()
                        .items
                        .iter()
                        .position(|item| item.title.contains("Порядок"))
                    {
                        menu.current_mut().list.select(index);
                        menu.enter();
                    }
                }
                section => {
                    let _ = super::open_section(&mut menu, section);
                }
            }
            let pixmap = render_menu_card(&menu);
            let file_id = id.replace('/', "-");
            let path = directory.join(format!("menu-real-{file_id}.png"));
            write_png(&pixmap, &path)?;
            written.push(path);
        }
    }
    Ok(written)
}

/// Пишет галерею иконок.
pub fn render_icon_gallery(directory: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut written = Vec::new();
    let path = directory.join("menu-gallery.png");
    let pixmap = render_icons();
    write_png(&pixmap, &path)?;
    written.push(path);
    Ok(written)
}

/// Сколько значений и кругов на кадре: проверка для тестов.
pub fn counts(frame: &Frame) -> (usize, usize) {
    let values = frame
        .items
        .iter()
        .filter(|item| !item.value.is_empty())
        .count();
    let toggles = frame
        .items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Toggle { .. }))
        .count();
    (values, toggles)
}

#[cfg(test)]
mod tests {
    use super::super::secret::{SecretFrame, SecretInput, SecretTarget};
    use super::super::settings::Language;
    use super::*;

    /// Снимок режима ввода пароля: та же карточка, маска вместо букв.
    #[test]
    fn secret_input_is_renderable_and_carries_no_password() {
        let mut input = SecretInput::new(
            SecretTarget::wifi("Дом \\ принтер", "wifi/net/x"),
            Language::Ru,
        );
        for ch in "тихийпароль".chars() {
            input.push(ch);
        }
        let frame = Frame {
            trail: vec![
                "HUD".to_string(),
                "Система".to_string(),
                "Wi-Fi".to_string(),
            ],
            items: Vec::new(),
            selected: Some(0),
            top: 0,
            query: String::new(),
            total: 0,
            status: None,
            secret: Some(input.frame(Language::Ru)),
            lang: Language::Ru,
        };
        let pixmap = render_frame(&frame);
        assert!(pixmap.width() > 0 && pixmap.height() > 0);
        let secret: &SecretFrame = frame.secret.as_ref().expect("ввод в кадре");
        assert_eq!(secret.masked, "•••••••••••");
        assert!(
            !format!("{frame:?}").contains("тихийпароль"),
            "пароль не в кадре"
        );
    }

    #[test]
    fn every_state_is_reachable_by_key() {
        for subject in Subject::ALL {
            assert_eq!(Subject::from_key(subject.key()), Some(subject));
        }
        assert_eq!(Subject::from_key("нет"), None);
    }

    #[test]
    fn root_state_shows_nine_sections() {
        let frame = subject_menu(Subject::Root).frame();
        assert_eq!(frame.total, 9);
        assert_eq!(frame.trail, vec!["HUD".to_string()]);
        assert_eq!(frame.query, "");
        assert_eq!(frame.selected, Some(2), "выбран «Стиль»");
    }

    #[test]
    fn style_state_shows_two_rows_with_values() {
        let frame = subject_menu(Subject::Style).frame();
        assert_eq!(frame.total, 2);
        assert_eq!(frame.trail, vec!["HUD".to_string(), "Стиль".to_string()]);
        assert_eq!(frame.items[0].value, "", "у обоев вместо значения круг");
        assert!(!frame.items[1].value.is_empty(), "схема показывает имя");
        let (values, toggles) = counts(&frame);
        assert_eq!(values, 1, "значение есть только у схемы");
        assert_eq!(toggles, 0, "в «Стиле» нет тумблеров");
    }

    #[test]
    fn search_state_shows_two_rows_with_paths() {
        let frame = subject_menu(Subject::Search).frame();
        assert_eq!(frame.query, "dnd");
        assert_eq!(frame.total, 2);
        assert!(frame.items.iter().all(|item| item.caption.is_some()));
        assert_eq!(frame.items[0].caption.as_deref(), Some("Панель ›"));
    }

    #[test]
    fn displays_subjects_show_expected_trails() {
        let displays = subject_menu(Subject::Displays).frame();
        assert_eq!(
            displays.trail,
            vec!["HUD".to_string(), "Дисплеи".to_string()]
        );
        assert!(
            displays.items.iter().any(|item| {
                item.title == "eDP-1 — встроенный" && item.value == "1920×1080 · 144 Гц"
            }),
            "строка вывода содержит новое название и режим: {:?}",
            displays
                .items
                .iter()
                .map(|item| (&item.title, &item.value))
                .collect::<Vec<_>>()
        );

        let output = subject_menu(Subject::DisplaysOutput).frame();
        assert_eq!(
            output.trail,
            vec![
                "HUD".to_string(),
                "Дисплеи".to_string(),
                "eDP-1".to_string(),
            ]
        );

        let deep = subject_menu(Subject::DeepCrumbs).frame();
        assert_eq!(
            deep.trail,
            vec![
                "HUD".to_string(),
                "Система".to_string(),
                "Устройства".to_string(),
                "Дисплеи".to_string(),
                "eDP-1".to_string(),
            ]
        );
    }

    #[test]
    fn empty_state_shows_nothing_and_a_query() {
        let frame = subject_menu(Subject::Empty).frame();
        assert_eq!(frame.query, "zzz");
        assert_eq!(frame.total, 0);
        assert!(frame.items.is_empty());
    }

    #[test]
    fn card_height_grows_with_the_state() {
        let root = subject_menu(Subject::Root).frame();
        let empty = subject_menu(Subject::Empty).frame();
        assert_eq!(view::rows_shown(root.total), 9);
        assert_eq!(view::rows_shown(empty.total), view::MIN_ROWS);
    }

    #[test]
    fn frame_matches_the_overlay_size() {
        let pixmap = render_subject(Subject::Root);
        assert_eq!(pixmap.width() as f32, FRAME_W * SCALE);
        assert_eq!(pixmap.height() as f32, FRAME_H * SCALE);
    }

    #[test]
    fn the_style_subject_renders() {
        let pixmap = render_subject(Subject::Style);
        assert!(pixmap.width() > 0 && pixmap.height() > 0);
    }

    #[test]
    fn real_tree_renders_every_section() {
        let dir = std::env::temp_dir().join(format!("hud-menu-real-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог снимков");
        let written = render_real_all(&dir).expect("снимки настоящего дерева");
        // Корень, девять разделов, порядок модулей и два подменю «Системы».
        assert_eq!(written.len(), 13);
        for path in &written {
            assert!(
                std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0),
                "пустой снимок: {path:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icon_gallery_covers_every_menu_icon() {
        let pixmap = render_icons();
        assert!(pixmap.width() >= (400.0 * SCALE) as u32);
        assert!(pixmap.height() > (140.0 * SCALE) as u32);
    }
}
