//! Нативное окно биндов niri (KP_8). Замена `keybind.sh`+rofi.
//! XDG-окно на tiny-skia/cosmic-text.

use hudbar::bind_window::{self, BindsKind};

fn main() {
    bind_window::run(BindsKind::Niri);
}
