//! Снимки окна настроек без Wayland.
//!
//! Нужны, чтобы смотреть правки отрисовки без композитора: окно рисует тот же
//! кадр, что и живое, но состояние берётся из дефолтного конфига и фиктивных
//! списков. Разница со снимком живой сессии — только в данных, поэтому по
//! такому PNG можно придираться к геометрии и цветам.
//!
//! Запуск:
//!   hud-settings-rs --snapshot <section> <out.png> [--theme normal|pixel]
//!
//! Секции: overview, panel, appearance, notifications, controls, wallpaper.

use super::config::{Config, Theme};
use super::palette;
use super::settings_icons;
use super::settings_ui::{self, Focus, Hotkey, Nav, Rect, Row, Section, Wallpaper};
use super::settings_view::{DrawExt, View, font_name};
use super::settings_widgets::CARD_PAD;
use super::settings_widgets::{
    self as widgets, BadgeKind, ButtonKind, Ctx, WidgetState, badge, button, icon, icon_button,
    slider, stepper, stepper_layout, toggle,
};
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::{self, PAGE_PAD, spacing, type_scale};

/// Состояние для кадра: те же поля, что у живого окна, но без Wayland.
struct Snapshot {
    painter: TextPainter,
    config: Config,
    rows: Vec<Row>,
    nav: Nav,
    hover: Option<Focus>,
    picked: Option<bool>,
    palette: palette::Palette,
    status: String,
}

impl View for Snapshot {
    fn painter(&mut self) -> &mut TextPainter {
        &mut self.painter
    }
    fn config(&self) -> &Config {
        &self.config
    }
    fn rows(&self) -> &[Row] {
        &self.rows
    }
    fn nav(&self) -> &Nav {
        &self.nav
    }
    fn hover(&self) -> Option<Focus> {
        self.hover
    }
    fn picked(&self) -> Option<bool> {
        self.picked
    }
    fn palette(&self) -> palette::Palette {
        self.palette
    }
    fn status(&self) -> String {
        self.status.clone()
    }
    fn busy_flag(&self) -> bool {
        false
    }
}

/// Фиктивные бинды niri. Список длинный, с дублями одного действия и без пары
/// вещей, для которых бинда нет: иначе склейка и пустые слоты в разделе
/// «Управление» на снимке не проверяются.
fn fake_hotkeys() -> Vec<Hotkey> {
    [
        ("Mod+Shift+H", "Показать окно настроек"),
        ("Mod+Shift+K", "Показать бинды niri"),
        ("Mod+Shift+Y", "Показать бинды yazi"),
        ("Mod+Q", "Закрыть панель"),
        ("Mod+Comma", "Открыть меню приложений"),
        ("Mod+T", "Новый терминал"),
        ("Mod+E", "Файловый менеджер"),
        ("Mod+Shift+V", "Буфер обмена"),
        ("Mod+B", "Показать панель"),
        ("Mod+Space", "Открыть терминал"),
        ("Mod+1", "Рабочая область 1"),
        ("Mod+2", "Рабочая область 2"),
        ("Mod+3", "Рабочая область 3"),
        ("Mod+4", "Рабочая область 4"),
        ("Mod+5", "Рабочая область 5"),
        ("Mod+6", "Рабочая область 6"),
        ("Mod+Shift+Left", "Сдвинуть окно влево"),
        ("Mod+Shift+Right", "Сдвинуть окно вправо"),
        ("Mod+Shift+Up", "Сдвинуть окно вверх"),
        ("Mod+Shift+Down", "Сдвинуть окно вниз"),
        ("Mod+Alt+Left", "Перенос окон по колонкам"),
        ("Mod+Alt+Down", "Перенос окон по рядам"),
        ("Mod+F", "Развернуть окно"),
        ("Mod+Shift+Down", "Свернуть окно"),
        ("Mod+Tab", "Следующее окно"),
        ("Mod+Shift+Tab", "Предыдущее окно"),
        ("Mod+Ctrl+Space", "Переключить раскладку"),
        ("XF86AudioRaiseVolume", "Громкость: громче"),
        ("XF86AudioLowerVolume", "Громкость: тише"),
        ("XF86AudioMute", "Звук: вкл/выкл"),
        ("XF86AudioMicMute", "Микрофон: вкл/выкл"),
        ("XF86MonBrightnessUp", "Яркость: больше"),
        ("XF86MonBrightnessDown", "Яркость: меньше"),
        ("XF86AudioPlay", "Пуск/пауза"),
        ("XF86AudioNext", "Следующий трек"),
        ("XF86AudioPrev", "Предыдущий трек"),
        ("Print", "Снимок экрана"),
        ("Mod+Shift+S", "Снимок области"),
        ("KP_1", "Wi-Fi: вкл/выкл"),
        ("Mod+Ctrl+W", "Сеть: переключить"),
        ("Control+Alt+Shift+T", "Запустить таймер"),
        ("XF86PowerOff", "Питание: переключение"),
        ("XF86ScreenSaver", "Блокировка экрана"),
    ]
    .into_iter()
    .map(|(keys, desc)| Hotkey {
        keys: keys.to_string(),
        desc: desc.to_string(),
    })
    .collect()
}

/// Фиктивные обои: вложенные папки и разные расширения, как в настоящем каталоге.
fn fake_wallpapers() -> Vec<String> {
    [
        "/home/mihail/wallpapers/anime/103.png",
        "/home/mihail/wallpapers/anime/157.jpg",
        "/home/mihail/wallpapers/anime/26.jpg",
        "/home/mihail/wallpapers/anime/5m5kLI9.png",
        "/home/mihail/wallpapers/nature/forest-01.jpg",
        "/home/mihail/wallpapers/nature/forest-02.jpeg",
        "/home/mihail/wallpapers/pixelart/light/city.png",
        "/home/mihail/wallpapers/pixelart/dark/cave.webp",
        "/home/mihail/wallpapers/renders/01.jpg",
        "/home/mihail/wallpapers/renders/02.jpg",
        "/home/mihail/wallpapers/mix/space.webp",
        "/home/mihail/wallpapers/README.md",
    ]
    .iter()
    .map(|path| (*path).to_string())
    .collect()
}

/// Кадр секции в натуральную величину: те же 1100x740 и тот же SCALE, что у
/// живого окна, иначе снимок врал бы масштабом.
pub fn render_section(section: Section, pixel: bool) -> tiny_skia::Pixmap {
    let config = Config {
        theme: Some(if pixel { Theme::Pixel } else { Theme::Normal }),
        ..Config::default()
    };
    let wallpaper = Wallpaper {
        files: fake_wallpapers(),
        selected: Some(1),
        scroll: 0,
        scheme: 2,
    };
    let rows = settings_ui::rows_for_state(section, &config, &fake_hotkeys(), 0, &wallpaper);
    let mut nav = Nav::new(section);
    // Фокус на первом элементе содержимого: на снимке видно, как выглядит
    // активная строка, а не только пустое состояние.
    if let Some(focus) = nav.first_content(&rows) {
        nav.focus = focus;
    }
    let mut snapshot = Snapshot {
        painter: TextPainter::new(&font_name(pixel)),
        config,
        rows,
        nav,
        hover: None,
        picked: None,
        palette: palette::Palette::default(),
        status: "Готово".to_string(),
    };
    let width = (settings_ui::WIDTH * SCALE) as u32;
    let height = (settings_ui::HEIGHT * SCALE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("pixmap");
    snapshot.render(&mut pixmap);
    pixmap
}

/// PNG из буфера окна. Буфер непрозрачный, поэтому альфа идёт как есть.
pub fn write_png(pixmap: &tiny_skia::Pixmap, path: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::create(path)
        .map_err(|error| format!("не создался {}: {error}", path.display()))?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, pixmap.width(), pixmap.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png = encoder
        .write_header()
        .map_err(|error| format!("PNG: {error}"))?;
    png.write_image_data(pixmap.data())
        .map_err(|error| format!("PNG: {error}"))
}

/// Галерея виджетов: все виджеты во всех состояниях плюс все иконки. Это
/// визуальный тест — снимок сразу показывает, сломался ли виджет, поэтому
/// здесь не нужен ни Wayland, ни реальный конфиг.
pub fn render_gallery(pixel: bool) -> tiny_skia::Pixmap {
    let p = ui_tokens::ui_palette(pixel, palette::Palette::default());
    let s = type_scale(pixel);
    let width = (settings_ui::WIDTH * SCALE) as u32;
    let height = (settings_ui::HEIGHT * SCALE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("pixmap");
    pixmap.fill(p.base.to_tiny());
    let mut painter = TextPainter::new(&font_name(pixel));
    // Иконки рисуются отдельной копией painter: в Pixel основной шрифт
    // Minecraft Rus, а Nerd Font глифов в нём нет.
    let mut icon_painter = TextPainter::new(settings_icons::FONT);

    let area = Rect::new(PAGE_PAD, PAGE_PAD, settings_ui::WIDTH - PAGE_PAD * 2.0, 0.0);
    let columns = 4;
    let column_w = (area.w - CARD_PAD * (columns + 1) as f32) / columns as f32;
    let column_x = |index: usize| area.x + CARD_PAD + index as f32 * (column_w + CARD_PAD);
    let row_h = 64.0;

    let mut ctx = Ctx {
        pixmap: &mut pixmap,
        painter: &mut painter,
        palette: p,
        scale: s,
        pixel,
    };

    ctx.paint(
        "Виджеты",
        s.page_title,
        p.text,
        Rect::new(area.x, area.y, 300.0, 30.0),
        Align::Start,
    );
    badge(
        &mut ctx,
        Rect::new(area.right() - 230.0, area.y + 2.0, 100.0, 24.0),
        "ACCENT",
        BadgeKind::Accent,
    );
    badge(
        &mut ctx,
        Rect::new(area.right() - 120.0, area.y + 2.0, 120.0, 24.0),
        "NEUTRAL",
        BadgeKind::Neutral,
    );

    let mut y = area.y + 46.0;
    for (index, title) in [
        "Карточка и плитки",
        "Кнопки",
        "Степпер и слайдер",
        "Тумблер",
    ]
    .iter()
    .enumerate()
    {
        ctx.paint(
            title,
            s.caption,
            p.muted,
            Rect::new(column_x(index), y, column_w, 20.0),
            Align::Start,
        );
    }
    y += 26.0;
    let cell = |column: usize, row: usize| {
        Rect::new(
            column_x(column),
            y + row as f32 * (row_h + spacing::SM),
            column_w,
            row_h,
        )
    };

    // Состояния плитки: обычный, наведение, фокус, выбран, неактивный.
    let states = [
        ("обычный", WidgetState::plain()),
        ("наведение", WidgetState::plain().with(false, true)),
        ("фокус", WidgetState::plain().with(true, false)),
        ("выбран", WidgetState::plain().selected(true)),
        ("неактивен", WidgetState::plain().disabled(true)),
    ];
    widgets::card(&mut ctx, cell(0, 0), Some("Карточка"));
    for (index, (name, state)) in states.iter().enumerate() {
        widgets::choice_tile(&mut ctx, cell(0, 1 + index), name, *state);
    }

    // Кнопки обоих ролей во всех состояниях.
    for (index, (kind, label)) in [
        (ButtonKind::Secondary, "Secondary"),
        (ButtonKind::Primary, "Primary"),
    ]
    .iter()
    .enumerate()
    {
        let base = cell(1, index * 3);
        for (offset, (_, state)) in states.iter().enumerate() {
            button(
                &mut ctx,
                Rect::new(base.x, base.y + offset as f32 * 38.0, base.w, 32.0),
                label,
                *kind,
                *state,
            );
        }
    }

    // Степперы: обычный и неактивный.
    stepper(
        &mut ctx,
        Rect::new(cell(2, 0).x, cell(2, 0).y + 8.0, cell(2, 0).w, 32.0),
        "Панель",
        "27 px",
        WidgetState::plain(),
    );
    stepper(
        &mut ctx,
        Rect::new(cell(2, 1).x, cell(2, 1).y + 8.0, cell(2, 1).w, 32.0),
        "Недоступно",
        "\u{2014}",
        WidgetState::plain().disabled(true),
    );

    // Слайдер с подписями и степпером: трек, min/max, значение и кнопки.
    let slider_cell = cell(2, 2);
    let track_w = slider_cell.w - 104.0;
    slider(
        &mut ctx,
        Rect::new(slider_cell.x, slider_cell.y + 14.0, track_w, 4.0),
        0.32,
        WidgetState::plain(),
    );
    ctx.paint(
        "24",
        s.caption,
        p.muted,
        Rect::new(slider_cell.x, slider_cell.y + 26.0, 40.0, 16.0),
        Align::Start,
    );
    ctx.paint(
        "48",
        s.caption,
        p.muted,
        Rect::new(
            slider_cell.x + track_w - 40.0,
            slider_cell.y + 26.0,
            40.0,
            16.0,
        ),
        Align::End,
    );
    let parts = stepper_layout(
        Rect::new(slider_cell.right() - 88.0, slider_cell.y, 88.0, 32.0),
        32.0,
    );
    icon_button(
        &mut ctx,
        parts.minus,
        settings_icons::MINUS,
        WidgetState::plain(),
    );
    ctx.paint("27 px", s.value, p.text, parts.value, Align::Center);
    icon_button(
        &mut ctx,
        parts.plus,
        settings_icons::PLUS,
        WidgetState::plain(),
    );

    // Тот же слайдер в фокусе.
    let focus_cell = cell(2, 3);
    slider(
        &mut ctx,
        Rect::new(focus_cell.x, focus_cell.y + 14.0, focus_cell.w - 8.0, 4.0),
        0.68,
        WidgetState::plain().with(true, false),
    );

    // Тумблер: включён, выключен, в фокусе, неактивен.
    for (index, state) in [
        WidgetState::plain().on(true),
        WidgetState::plain().on(false),
        WidgetState::plain().on(true).with(true, false),
        WidgetState::plain().on(false).disabled(true),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = cell(3, index);
        let name = match (state.on, state.disabled, state.focused) {
            (true, false, false) => "вкл",
            (false, false, false) => "выкл",
            (true, false, true) => "вкл, фокус",
            _ => "выкл, неактивен",
        };
        ctx.paint(
            name,
            s.caption,
            p.muted,
            Rect::new(rect.x, rect.y + 4.0, rect.w - 44.0, 20.0),
            Align::Start,
        );
        toggle(
            &mut ctx,
            Rect::new(rect.right() - 36.0, rect.y + 4.0, 36.0, 20.0),
            state,
        );
    }
    // IconButton: стрелки перестановки в трёх состояниях.
    for (index, state) in [
        WidgetState::plain(),
        WidgetState::plain().with(false, true),
        WidgetState::plain().with(true, false),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::new(
            cell(3, 4).x + index as f32 * 72.0,
            cell(3, 4).y + 10.0,
            32.0,
            32.0,
        );
        icon_button(&mut ctx, rect, settings_icons::UP, state);
        icon_button(
            &mut ctx,
            Rect::new(rect.x + 36.0, rect.y, 32.0, 32.0),
            settings_icons::DOWN,
            state,
        );
    }

    // Иконки: сетка 8 колонок с подписями под каждой.
    let icons_y = y + 6.0 * (row_h + spacing::SM) + 16.0;
    ctx.paint(
        "Иконки",
        s.section_title,
        p.text,
        Rect::new(area.x, icons_y, 300.0, 24.0),
        Align::Start,
    );
    let icon_w = (area.w - spacing::SM * 7.0) / 8.0;
    for (index, (name, glyph)) in settings_icons::ALL.into_iter().enumerate() {
        let column = index % 8;
        let row = index / 8;
        let x = area.x + column as f32 * (icon_w + spacing::SM);
        let cell_y = icons_y + 30.0 + row as f32 * 52.0;
        ctx.paint(
            name,
            s.caption,
            p.muted,
            Rect::new(x, cell_y + 26.0, icon_w, 16.0),
            Align::Center,
        );
        // Иконка рисуется последней и отдельным painter: в Pixel основной
        // шрифт — Minecraft Rus, и Nerd Font глифов в нём нет.
        let mut icon_ctx = Ctx {
            pixmap: ctx.pixmap,
            painter: &mut icon_painter,
            palette: p,
            scale: s,
            pixel,
        };
        icon(&mut icon_ctx, Rect::new(x, cell_y, icon_w, 26.0), glyph);
    }
    pixmap
}

/// Что рисовать: секция окна или галерея виджетов.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Section(settings_ui::Section),
    Gallery,
}

/// Разбор `--snapshot <section|gallery> <out.png> [--theme normal|pixel]`.
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
                    subject = Some(if value == "gallery" {
                        Subject::Gallery
                    } else {
                        Subject::Section(Section::from_key(value).ok_or(format!(
                            "секция не понята: {value}. Доступны: overview, panel, \
                                 appearance, notifications, controls, wallpaper, gallery"
                        ))?)
                    });
                } else if out.is_none() {
                    out = Some(value);
                } else {
                    return Err(format!("лишний аргумент: {value}"));
                }
                i += 1;
            }
        }
    }
    let subject = subject.ok_or("нужна секция или gallery")?;
    let out = out.ok_or("нужен путь к PNG")?;
    let pixmap = match subject {
        Subject::Section(section) => render_section(section, pixel),
        Subject::Gallery => render_gallery(pixel),
    };
    write_png(&pixmap, std::path::Path::new(out))?;
    let theme = if pixel { "pixel" } else { "normal" };
    let name = match subject {
        Subject::Section(section) => section.label(),
        Subject::Gallery => "галерея",
    };
    println!(
        "{name} {theme} {}x{} → {out}",
        pixmap.width(),
        pixmap.height()
    );
    Ok(())
}

/// Справка по синтаксису: печатается вместе с ошибкой разбора.
pub fn hint() -> String {
    format!(
        "--snapshot <{}> <out.png> [--theme normal|pixel]",
        Section::ALL
            .iter()
            .map(|section| section.key())
            .collect::<Vec<_>>()
            .join("|")
    )
}
