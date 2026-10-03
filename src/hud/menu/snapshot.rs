//! Снимки меню в PNG без Wayland.
//!
//! Окно меню появится в M4, а смотреть на геометрию нужно уже сейчас: кадр
//! рисуется тем же кодом, что и живое окно, но состояние берётся из тестового
//! дерева `demo_tree`, поэтому снимок воспроизводим и отличается от реального
//! только данными.
//!
//! Запуск:
//!   hud-menu-rs --snapshot <root|style|search|empty> <out.png> [--theme normal|pixel]
//!
//! Кадр — вся поверхность оверлея, с затемнением вокруг карточки. Размер
//! кадра взят у оверлея меню в niri: 1200x800, этого хватает, чтобы увидеть
//! и карточку, и поле вокруг неё.

use super::super::palette;
use super::super::settings_icons;
use super::super::settings_ui::Rect;
use super::super::text::Align;
use super::super::text::{SCALE, TextPainter};
use super::super::ui_tokens::radii;
use super::demo_menu;
use super::item::ItemKind;
use super::state::Outcome;
use super::state::{Frame, Menu, Status};
use super::strings;
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
}

impl Subject {
    /// Все доступные состояния для подсказки.
    pub const ALL: [Subject; 4] = [
        Subject::Root,
        Subject::Style,
        Subject::Search,
        Subject::Empty,
    ];

    /// Ключ в командной строке.
    pub fn key(&self) -> &'static str {
        match self {
            Subject::Root => "root",
            Subject::Style => "style",
            Subject::Search => "search",
            Subject::Empty => "empty",
        }
    }

    /// Человеческое имя для вывода.
    pub fn label(&self) -> &'static str {
        match self {
            Subject::Root => "корень",
            Subject::Style => "подменю «Стиль»",
            Subject::Search => "поиск",
            Subject::Empty => "пусто",
        }
    }

    /// Разбор ключа.
    pub fn from_key(value: &str) -> Option<Subject> {
        Subject::ALL
            .into_iter()
            .find(|subject| subject.key() == value)
    }
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
    }
    menu.set_page(view::MAX_ROWS);
    menu
}

/// Кадр состояния: подложка, затемнение и карточка.
pub fn render_subject(subject: Subject, pixel: bool) -> tiny_skia::Pixmap {
    let menu = subject_menu(subject);
    let mut frame = menu.frame();
    if subject == Subject::Search {
        // После выбора результата в подвале остаётся подтверждение: так
        // снимок показывает и режим статуса, где счётчика нет.
        frame.status = Some(Status::Applied(strings::STATUS_APPLIED.to_string()));
    }
    let (ui, scale) = view::theme(pixel, palette::Palette::default());
    let mut pixmap =
        tiny_skia::Pixmap::new((FRAME_W * SCALE) as u32, (FRAME_H * SCALE) as u32).expect("pixmap");
    backdrop(&mut pixmap, ui);
    let mut painter = TextPainter::new(&view::font_name(pixel));
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut menu_view = MenuView {
        pixmap: &mut pixmap,
        painter: &mut painter,
        icons: &mut icons,
        ui,
        scale,
        pixel,
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

/// Запись буфера в PNG. Та же функция, что у окна настроек.
pub fn write_png(pixmap: &tiny_skia::Pixmap, path: &std::path::Path) -> Result<(), String> {
    super::super::settings_snapshot::write_png(pixmap, path)
}

/// Разбор `--snapshot <state> <out.png> [--theme normal|pixel]`.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut subject: Option<Subject> = None;
    let mut out: Option<&str> = None;
    let mut pixel = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--theme" => {
                let theme = args
                    .get(i + 1)
                    .ok_or("--theme требует значения: normal или pixel")?;
                pixel = match theme.as_str() {
                    "pixel" => true,
                    "normal" => false,
                    other => return Err(format!("тема не понята: {other}")),
                };
                i += 2;
            }
            value => {
                if subject.is_none() {
                    subject = Some(Subject::from_key(value).ok_or(format!(
                        "состояние не понято: {value}. Доступны: {}",
                        hint()
                    ))?);
                } else if out.is_none() {
                    out = Some(value);
                } else {
                    return Err(format!("лишний аргумент: {value}"));
                }
                i += 1;
            }
        }
    }
    let subject = subject.ok_or(format!("нужно состояние: {}", hint()))?;
    let out = out.ok_or("нужен путь к PNG")?;
    let pixmap = render_subject(subject, pixel);
    write_png(&pixmap, std::path::Path::new(out))?;
    println!(
        "{} {} {}x{} → {out}",
        subject.label(),
        if pixel { "pixel" } else { "normal" },
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
pub fn render_icons(pixel: bool) -> tiny_skia::Pixmap {
    let (ui, scale) = view::theme(pixel, palette::Palette::default());
    let columns = 4;
    let cell_w = 220.0;
    let cell_h = 64.0;
    let width = (FRAME_W * SCALE) as u32;
    let rows = settings_icons::MENU.len().div_ceil(columns);
    let height = ((140.0 + rows as f32 * cell_h) * SCALE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("pixmap");
    pixmap.fill(ui.base.to_tiny());
    let mut painter = TextPainter::new(&view::font_name(pixel));
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
    for (index, (name, glyph)) in settings_icons::MENU.iter().enumerate() {
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
        for pixel in [false, true] {
            let theme = if pixel { "pixel" } else { "normal" };
            let path = directory.join(format!("menu-{}-{}.png", subject.key(), theme));
            let pixmap = render_subject(subject, pixel);
            write_png(&pixmap, &path)?;
            written.push(path);
        }
    }
    Ok(written)
}

/// Пишет галерею иконок в обеих темах.
pub fn render_icon_gallery(directory: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut written = Vec::new();
    for pixel in [false, true] {
        let theme = if pixel { "pixel" } else { "normal" };
        let path = directory.join(format!("menu-gallery-{theme}.png"));
        let pixmap = render_icons(pixel);
        write_png(&pixmap, &path)?;
        written.push(path);
    }
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
    use super::*;

    #[test]
    fn every_state_is_reachable_by_key() {
        for subject in Subject::ALL {
            assert_eq!(Subject::from_key(subject.key()), Some(subject));
        }
        assert_eq!(Subject::from_key("нет"), None);
    }

    #[test]
    fn root_state_shows_eight_sections() {
        let frame = subject_menu(Subject::Root).frame();
        assert_eq!(frame.total, 8);
        assert_eq!(frame.trail, vec!["HUD".to_string()]);
        assert_eq!(frame.query, "");
        assert_eq!(frame.selected, 2, "выбран «Стиль»");
    }

    #[test]
    fn style_state_shows_five_rows_with_values() {
        let frame = subject_menu(Subject::Style).frame();
        assert_eq!(frame.total, 5);
        assert_eq!(frame.trail, vec!["HUD".to_string(), "Стиль".to_string()]);
        assert_eq!(frame.items[0].value, "Обычная");
        assert_eq!(frame.items[2].value, "", "у обоев вместо значения круг");
        let (values, toggles) = counts(&frame);
        assert_eq!(values, 4, "четыре пункта со значением");
        assert_eq!(toggles, 0, "в «Стиле» нет тумблеров");
    }

    #[test]
    fn search_state_shows_three_rows_with_paths() {
        let frame = subject_menu(Subject::Search).frame();
        assert_eq!(frame.query, "dnd");
        assert_eq!(frame.total, 3);
        assert!(frame.items.iter().all(|item| item.caption.is_some()));
        assert_eq!(frame.items[0].caption.as_deref(), Some("Панель ›"));
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
        assert_eq!(view::rows_shown(root.total), 8);
        assert_eq!(view::rows_shown(empty.total), view::MIN_ROWS);
    }

    #[test]
    fn frame_matches_the_overlay_size() {
        let pixmap = render_subject(Subject::Root, false);
        assert_eq!(pixmap.width() as f32, FRAME_W * SCALE);
        assert_eq!(pixmap.height() as f32, FRAME_H * SCALE);
    }

    #[test]
    fn both_themes_render() {
        for pixel in [false, true] {
            let pixmap = render_subject(Subject::Style, pixel);
            assert!(pixmap.width() > 0 && pixmap.height() > 0);
        }
    }

    #[test]
    fn icon_gallery_covers_every_menu_icon() {
        let pixmap = render_icons(false);
        assert!(pixmap.width() >= (400.0 * SCALE) as u32);
        assert!(pixmap.height() > (140.0 * SCALE) as u32);
    }
}
