//! Нативное окно биндов yazi (KP_9). Замена `yazibind.sh`+rofi.
//! XDG-окно на tiny-skia/cosmic-text.

use hudbar::bind_window::{self, BindsKind};

fn main() {
    bind_window::run(BindsKind::Yazi);
}
