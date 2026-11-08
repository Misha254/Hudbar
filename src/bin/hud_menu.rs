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

use hudbar::menu::{action, snapshot, window};

const USAGE: &str = "\
hud-menu-rs — меню HUDbar

  hud-menu-rs [раздел][/строка]
      Открыть меню. Раздел: apps, panel, style, notifications, language,
      system, power, capture, keybinds, about. Вложенный уровень — через
      слэш: hud-menu-rs system/control откроет «Конфиги» сразу.
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return;
    }
    if let Some(mode) = args.first().map(String::as_str) {
        let outcome = match mode {
            "--snapshot" => snapshot::run(&args[1..]),
            "--all" => snapshot::run_all(args.get(1).map(String::as_str)),
            "--real" => snapshot::run_real(args.get(1).map(String::as_str)),
            "--icons" => snapshot::run_icons(args.get(1).map(String::as_str)),
            "--action" => action::run_by_id(args.get(1).map(String::as_str).unwrap_or("")),
            _ => window::run(args.first().map(String::as_str).unwrap_or("")),
        };
        if let Err(error) = outcome {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = window::run("") {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
