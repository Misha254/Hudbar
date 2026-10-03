//! `hud-menu-rs` — снимки меню HUDbar без Wayland.
//!
//! В M2 у меню ещё нет окна: Wayland-слой появится в M4, вместе с клавиатурой
//! и обёрткой запуска. Пока меню можно только нарисовать, и этот бинарник
//! делает ровно одно — снимает кадр в PNG:
//!
//!   hud-menu-rs --snapshot <root|style|search|empty> <out.png> [--theme normal|pixel]
//!   hud-menu-rs --all <каталог>
//!   hud-menu-rs --icons <каталог>
//!
//! Модули подключены вручную, как в `hud-settings-rs`: бинарники в этом
//! крейте не делят библиотеку, и каждый собирает себе только нужное.

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
#[path = "../hud/dunst.rs"]
mod dunst;
#[allow(dead_code)]
#[path = "../hud/menu/mod.rs"]
mod menu;
#[allow(dead_code)]
#[path = "../hud/palette.rs"]
mod palette;
#[allow(dead_code)]
#[path = "../hud/settings.rs"]
mod settings;
#[allow(dead_code)]
#[path = "../hud/settings_icons.rs"]
mod settings_icons;
#[allow(dead_code)]
#[path = "../hud/settings_snapshot.rs"]
mod settings_snapshot;
#[allow(dead_code)]
#[path = "../hud/settings_ui.rs"]
mod settings_ui;
#[allow(dead_code)]
#[path = "../hud/settings_view.rs"]
mod settings_view;
#[allow(dead_code)]
#[path = "../hud/settings_widgets.rs"]
mod settings_widgets;
// `set_font` не нужен меню: шрифт задаётся при создании painter.
#[allow(dead_code)]
#[path = "../hud/text.rs"]
mod text;
#[allow(dead_code)]
#[path = "../hud/ui_layout.rs"]
mod ui_layout;
#[allow(dead_code)]
#[path = "../hud/ui_tokens.rs"]
mod ui_tokens;
#[allow(dead_code)]
#[path = "../hud/wallpaper.rs"]
mod wallpaper;

const USAGE: &str = "\
hud-menu-rs — снимки меню HUDbar

  --snapshot <root|style|search|empty> <out.png> [--theme normal|pixel]
      Один кадр выбранного состояния.
  --all <каталог>
      Все состояния в обеих темах: восемь PNG.
  --icons <каталог>
      Галерея иконок меню в обеих темах: два PNG.
  -h, --help
      Эта справка.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return;
    }
    let result = match args[0].as_str() {
        "--snapshot" => menu::snapshot::run(&args[1..]),
        "--all" => write_all(&args[1..]),
        "--icons" => write_icons(&args[1..]),
        other => Err(format!("неизвестный ключ: {other}\n\n{USAGE}")),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// Каталог для пакетной записи: папка создаётся, если её нет.
fn directory(args: &[String], flag: &str) -> Result<std::path::PathBuf, String> {
    let path = args
        .first()
        .ok_or(format!("{flag} требует путь к каталогу"))?;
    let path = std::path::PathBuf::from(path);
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("не создался каталог {}: {error}", path.display()))?;
    Ok(path)
}

/// Все состояния в обеих темах.
fn write_all(args: &[String]) -> Result<(), String> {
    let path = directory(args, "--all")?;
    for file in menu::snapshot::render_all(&path)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// Галерея иконок в обеих темах.
fn write_icons(args: &[String]) -> Result<(), String> {
    let path = directory(args, "--icons")?;
    for file in menu::snapshot::render_icon_gallery(&path)? {
        println!("{}", file.display());
    }
    Ok(())
}
