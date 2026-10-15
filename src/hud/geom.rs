//! Геометрия окна: прямоугольник и проверки попадания.
//!
//! `Rect` жил в `settings_ui` — раскладке снятого окна настроек, — поэтому его
//! тянули все окна вместе с шестью тысячами строк мёртвого кода. Здесь только
//! сам тип: ни шрифтов, ни палитры, ни Wayland.

/// Прямоугольник в логических пикселях; `x`/`y` — левый верхний угол.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}
