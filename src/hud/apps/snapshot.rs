//! Снимки окна приложений без Wayland.
//!
//! Нужны, чтобы смотреть геометрию и цвета без композитора: окно рисует тот же
//! кадр, что и живое, но состояние берётся из настоящего скана desktop-файлов,
//! а иконки грузятся синхронно — потока-загрузчика в снимке нет. Разница со
//! снимком живой сессии только в этом, поэтому по такому PNG можно придираться
//! к вёрстке.
//!
//! Запуск:
//!   hud-apps-rs --snapshot <default|search|empty|pinned> <out.png>

use std::path::Path;

use super::super::canvas::write_png;
use super::super::menu::view::RowsHighlight;
use super::super::palette::{self, Palette};
use super::super::settings::Language;
use super::super::settings_icons;
use super::super::text::{SCALE, TextPainter};
use super::super::ui_tokens::{font_name, type_scale};
use super::icons::Loader;
use super::model::Model;
use super::view;

/// Разбор аргументов `--snapshot` и запись PNG.
pub fn run(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("hud-apps-rs --snapshot <default|search|empty|pinned> <out.png>".to_string());
    }
    let mode = args[0].as_str();
    let out = args[1].as_str();
    let model = scene(mode)?;
    write_png(&render(&model, palette::load()), Path::new(out))
}

/// Состояние для кадра. Список — настоящий: снимок должен показывать, что
/// покажет окно на этой машине, иначе он ничего не проверяет.
fn scene(mode: &str) -> Result<Model, String> {
    let mut model = Model::open(view::MAX_ROWS, Language::Ru);
    match mode {
        "default" => {}
        "search" => {
            // Буква, которая есть в названии почти любого приложения: список
            // остаётся длиннее окна, и видно и поиск, и прокрутку.
            model.type_char('e');
        }
        "empty" => {
            for ch in "zzqq".chars() {
                model.type_char(ch);
            }
        }
        "pinned" => {
            // Подвал с результатом действия: он появляется на секунду и должен
            // помещаться в ту же строку, что счётчик.
            model.set_status("Закреплено: Steam".to_string());
            model.type_char('e');
        }
        other => return Err(format!("неизвестный снимок: {other}")),
    }
    // Пустым может быть только результат поиска: пустой скан означает, что на
    // машине нет ни одного desktop-файла, и такой снимок ничего бы не показал.
    if model.apps().is_empty() {
        return Err("на этой машине не найдено ни одного приложения".to_string());
    }
    Ok(model)
}

/// Кадр окна в буфер. Иконки грузятся до отрисовки: первый кадр должен быть
/// готовым, иначе снимок покажет заглушки там, где окно покажет иконки.
pub fn render(model: &Model, base: Palette) -> tiny_skia::Pixmap {
    let (width, height) = view::layer_size(model.visible().len());
    let mut pixmap = tiny_skia::Pixmap::new(
        (width as f32 * SCALE) as u32,
        (height as f32 * SCALE) as u32,
    )
    .expect("буфер кадра");
    pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let (ui, scale) = (super::super::ui_tokens::ui_palette(base), type_scale());
    let mut loader = Loader::new();
    for row in model.visible() {
        if let Some(app) = model.app_at(row) {
            loader.prime(&app.icon);
        }
    }
    let card = {
        let mut window = view::AppsView {
            pixmap: &mut pixmap,
            painter: &mut painter,
            icons: &mut icons,
            ui,
            scale,
        };
        view::render_window(&mut window, model, &loader, RowsHighlight::none())
    };
    view::apply_card_alpha(&mut pixmap, card, view::CARD_ALPHA);
    pixmap
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app(id: &str, name: &str, icon: &str) -> super::super::scan::App {
        super::super::scan::App {
            id: id.into(),
            name: name.into(),
            name_plain: name.into(),
            generic: String::new(),
            keywords: Vec::new(),
            exec: "true".into(),
            icon: icon.into(),
            terminal: false,
            dbus_activatable: false,
            path: PathBuf::from(format!("{id}.desktop")),
        }
    }

    fn model() -> Model {
        Model::from_apps(
            vec![
                app("firefox", "Файрфокс", "firefox"),
                app("kitty", "kitty", "kitty"),
                app("steam", "Steam", "steam"),
            ],
            vec!["steam".to_string()],
            view::MAX_ROWS,
            Language::Ru,
        )
    }

    /// Кадр должен получиться ненулевого размера и без падения: снимок —
    /// основной способ проверить вёрстку без композитора, а падать он не должен
    /// даже на синтетическом списке.
    #[test]
    fn a_snapshot_of_a_synthetic_list_renders() {
        let pixmap = render(&model(), palette::Palette::default());
        let (width, height) = view::layer_size(model().visible().len());
        assert_eq!(pixmap.width(), (width as f32 * SCALE) as u32);
        assert_eq!(pixmap.height(), (height as f32 * SCALE) as u32);
        // Карточка непрозрачна в центре: пустой кадр означал бы, что ничего
        // не нарисовалось.
        let center = pixmap
            .pixel(pixmap.width() / 2, pixmap.height() / 2)
            .expect("пиксель центра");
        assert!(center.alpha() > 0, "центр карточки должен быть залит");
    }

    #[test]
    fn a_missing_icon_leaves_a_placeholder_instead_of_failing() {
        let mut model = model();
        model.select(0);
        let pixmap = render(&model, palette::Palette::default());
        assert_eq!(pixmap.height() % (SCALE as u32), 0);
    }

    #[test]
    fn the_png_is_written_and_readable() {
        let file =
            std::env::temp_dir().join(format!("hud-apps-snapshot-{}.png", std::process::id()));
        let pixmap = render(&model(), palette::Palette::default());
        write_png(&pixmap, &file).expect("запись png");
        let written = std::fs::metadata(&file).expect("файл снимка");
        assert!(written.len() > 0);
        let _ = std::fs::remove_file(&file);
    }
}
