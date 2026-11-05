//! Геометрия экрана блокировки: часы и одинокое поле пароля на обоях.
//!
//! Карточки нет: замок показывает крупные часы и одно поле, а подсказки и
//! рамка окна только забирали место. Сообщение о неверном пароле стоит под
//! полем прямо на затемнённых обоях.
//!
//! Все числа — производные `spacing`, как у greeter. Раскладка чистая: на
//! входе размер поверхности, на выходе прямоугольники.

use crate::geom::Rect;
use crate::ui_tokens::{spacing, type_scale};

/// Ширина поля: уже карточки меню, потому что в замке больше ничего нет.
pub const FIELD_W: f32 = 340.0;

/// Высота поля пароля: как поле ввода экрана входа.
pub const FIELD_H: f32 = spacing::XXL + spacing::XS;

/// Строка сообщения. Место под неё не резервируется: поле на обоях, и его
/// положение от появления ошибки не меняется.
pub const MESSAGE_H: f32 = spacing::LG + spacing::SM;

/// Зазор между блоком часов и полем.
const CLOCK_GAP: f32 = spacing::LG;

/// Кегль часов: `display` рассчитан на строку внутри карточки, а здесь часы
/// стоят на обоях и должны читаться с расстояния.
pub fn clock_size() -> f32 {
    type_scale().display * 1.5
}

/// Высота блока «часы + дата».
pub fn clock_height() -> f32 {
    clock_size() * 1.3 + spacing::XS + type_scale().label * 1.6
}

/// Высота композиции: часы, зазор и поле.
fn group_height() -> f32 {
    clock_height() + CLOCK_GAP + FIELD_H
}

/// Поле на поверхности: по центру по горизонтали, вся композиция чуть выше
/// геометрического центра экрана.
pub fn field(size: (f32, f32)) -> Rect {
    let x = ((size.0 - FIELD_W) / 2.0).max(0.0);
    let top = ((size.1 - group_height()) * 0.42).max(0.0);
    Rect::new(
        x.round(),
        (top + clock_height() + CLOCK_GAP).round(),
        FIELD_W,
        FIELD_H,
    )
}

/// Полоса крупных часов. Ширину задаёт поле, а текст центрируется в ней при
/// отрисовке: ширина строки зависит от шрифта.
pub fn clock(field: Rect) -> Rect {
    Rect::new(
        field.x,
        field.y - CLOCK_GAP - clock_height(),
        field.w,
        clock_size() * 1.3,
    )
}

/// Полоса даты под часами.
pub fn date(field: Rect) -> Rect {
    let clock = clock(field);
    Rect::new(
        field.x,
        clock.bottom() + spacing::XS,
        field.w,
        type_scale().label * 1.6,
    )
}

/// Прямоугольники поля и сообщения.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Поле пароля.
    pub field: Rect,
    /// Метка Caps Lock внутри поля, справа.
    pub caps: Rect,
    /// Строка сообщения под полем: рисуется не всегда.
    pub message: Rect,
}

/// Содержимое под часами.
pub fn of(field: Rect) -> Layout {
    let caps = Rect::new(
        field.x + field.w * 0.5,
        field.y,
        field.w * 0.5 - spacing::MD,
        field.h,
    );
    let message = Rect::new(
        field.x + spacing::MD,
        field.bottom() + spacing::SM,
        field.w - spacing::MD * 2.0,
        MESSAGE_H,
    );
    Layout {
        field,
        caps,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (f32, f32) = (1920.0, 1080.0);

    /// Поле по центру, часы над ним, а вся композиция — выше середины.
    #[test]
    fn the_clock_sits_above_the_centred_field() {
        let field = field(VIEW);
        assert!((field.x + field.w / 2.0 - VIEW.0 / 2.0).abs() < 1.0);
        let clock = clock(field);
        assert!(clock.bottom() <= field.y, "часы выше поля");
        let date = date(field);
        assert!(date.y >= clock.bottom() && date.bottom() <= field.y);
        let group_middle = (clock.y + field.bottom()) / 2.0;
        assert!(
            group_middle < VIEW.1 / 2.0,
            "композиция выше середины экрана"
        );
        assert!(clock.y >= 0.0);
        assert!(field.bottom() <= VIEW.1);
    }

    /// Маленький экран: композиция не уезжает за края.
    #[test]
    fn a_small_surface_still_contains_the_field() {
        let field = field((480.0, 400.0));
        assert!(field.x >= 0.0 && field.y >= 0.0);
        assert!(field.w <= 480.0);
        assert!(field.bottom() <= 400.0);
        assert!(clock(field).y >= 0.0);
    }

    /// Сообщение стоит под полем и не наезжает на него.
    #[test]
    fn the_message_sits_under_the_field() {
        let layout = of(field(VIEW));
        assert!(layout.message.y >= layout.field.bottom());
        assert!(layout.message.x >= layout.field.x);
        assert!(layout.message.right() <= layout.field.right());
    }

    /// Метка Caps Lock внутри поля справа и не наезжает на его центр.
    #[test]
    fn the_caps_badge_lives_in_the_field() {
        let layout = of(field(VIEW));
        assert_eq!(layout.caps.y, layout.field.y);
        assert_eq!(layout.caps.h, layout.field.h);
        assert!(layout.caps.right() <= layout.field.right());
        assert!(layout.caps.x >= layout.field.x + layout.field.w / 2.0);
    }
}
