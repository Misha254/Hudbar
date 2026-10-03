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
use super::settings_ui::{self, Focus, Hotkey, Nav, Row, Section, Wallpaper};
use super::settings_view::{DrawExt, View, font_name};
use super::text::{SCALE, TextPainter};

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

/// Разбор `--snapshot <section> <out.png> [--theme normal|pixel]`.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut section: Option<Section> = None;
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
                if section.is_none() {
                    section = Some(Section::from_key(value).ok_or(format!(
                        "секция не понята: {value}. Доступны: overview, panel, appearance, \
                         notifications, controls, wallpaper"
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
    let section = section.ok_or("нужна секция")?;
    let out = out.ok_or("нужен путь к PNG")?;
    let pixmap = render_section(section, pixel);
    write_png(&pixmap, std::path::Path::new(out))?;
    let theme = if pixel { "pixel" } else { "normal" };
    println!(
        "{} {} {}x{} → {out}",
        section.label(),
        theme,
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
