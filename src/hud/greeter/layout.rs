//! Геометрия экрана входа: часы и два поля прямо на обоях.
//!
//! Карточки нет: часы, имя, пароль, кнопка и строка подсказок стоят на
//! затемнённых обоях, как на экране блокировки. Ширина полей — как у замка,
//! чтобы два экрана читались одним набором.
//!
//! Все числа — производные `spacing` и `type_scale`, как у greeter. Раскладка
//! чистая: на входе размер поверхности, на выходе прямоугольники.

use super::geom::Rect;
use super::ui_tokens::{spacing, type_scale};

/// Ширина поля и кнопки: уже прежней карточки, потому что рамки больше нет.
pub const FIELD_W: f32 = 340.0;

/// Ширина строки подсказок: шире полей, иначе «F2 СЕАНС: niri» не влезает.
/// Строка всё равно центрируется, и на обоях её край не заметен.
pub const HINT_W: f32 = 440.0;

/// Отступ содержимого от края поверхности на маленьком экране.
const MIN_MARGIN: f32 = spacing::MD;

/// Высота поля ввода: как у замка и поля карточки меню.
pub const FIELD_H: f32 = spacing::XXL + spacing::XS;

/// Высота кнопки: крупнее поля, чтобы Enter было видно как действие.
pub const BUTTON_H: f32 = spacing::XXL + spacing::MD;

/// Высота строки сообщения. Место занято всегда: композиция не должна
/// прыгать, когда PAM присылает ошибку.
pub const MESSAGE_H: f32 = spacing::LG + spacing::SM;

/// Зазор между группами.
const GROUP_GAP: f32 = spacing::MD;
/// Зазор между полями: меньше группового, поля читаются одной парой.
const FIELD_GAP: f32 = spacing::MD;
/// Зазор между блоком часов и полями.
const CLOCK_GAP: f32 = spacing::LG;

/// Кегль часов: как на экране блокировки, а не `display` из карточки меню.
pub fn clock_size() -> f32 {
    type_scale().display * 1.5
}

/// Высота блока «часы + дата».
pub fn clock_height() -> f32 {
    clock_size() * 1.3 + spacing::XS + type_scale().label * 1.6
}

/// Высота строки подсказок.
fn hint_height() -> f32 {
    type_scale().caption * 1.6
}

/// Высота всей композиции: часы, два поля, сообщение, кнопка и подсказки.
pub fn group_height() -> f32 {
    clock_height()
        + CLOCK_GAP
        + FIELD_H * 2.0
        + FIELD_GAP
        + spacing::XS
        + MESSAGE_H
        + spacing::XS
        + BUTTON_H
        + GROUP_GAP
        + hint_height()
}

/// Верхнее поле на поверхности: по центру по горизонтали, вся композиция
/// чуть выше геометрического центра.
pub fn field(size: (f32, f32)) -> Rect {
    let width = FIELD_W.min((size.0 - MIN_MARGIN * 2.0).max(0.0));
    let x = ((size.0 - width) / 2.0).max(0.0);
    let top = ((size.1 - group_height()) * 0.42).max(0.0);
    Rect::new(
        x.round(),
        (top + clock_height() + CLOCK_GAP).round(),
        width,
        FIELD_H,
    )
}

/// Полоса крупных часов над полями. Ширину задаёт поле, а текст
/// центрируется в ней при отрисовке: ширина строки зависит от шрифта.
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

/// Прямоугольники содержимого под часами.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Поле имени.
    pub field_user: Rect,
    /// Поле пароля.
    pub field_password: Rect,
    /// Метка Caps Lock внутри поля пароля, справа.
    pub caps: Rect,
    /// Строка сообщения: есть всегда, рисуется не всегда.
    pub message: Rect,
    pub button: Rect,
    /// Строка подсказок: клавиши, сеанс или команды питания.
    pub hints: Rect,
}

/// Содержимое сверху вниз от одного курсора.
pub fn of(field: Rect) -> Layout {
    let field_user = field;
    let field_password = Rect::new(field.x, field_user.bottom() + FIELD_GAP, field.w, FIELD_H);
    let caps = Rect::new(
        field_password.x + field_password.w * 0.5,
        field_password.y,
        field_password.w * 0.5 - spacing::MD,
        field_password.h,
    );
    let message = Rect::new(
        field.x,
        field_password.bottom() + spacing::XS,
        field.w,
        MESSAGE_H,
    );
    let button = Rect::new(field.x, message.bottom() + spacing::XS, field.w, BUTTON_H);
    let hints = Rect::new(
        field.x + (field.w - HINT_W) / 2.0,
        button.bottom() + GROUP_GAP,
        HINT_W,
        hint_height(),
    );

    Layout {
        field_user,
        field_password,
        caps,
        message,
        button,
        hints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (f32, f32) = (1920.0, 1080.0);

    /// Поля по центру, часы над ними, а вся композиция — выше середины.
    #[test]
    fn the_clock_sits_above_the_centred_fields() {
        let field = field(VIEW);
        assert!((field.x + field.w / 2.0 - VIEW.0 / 2.0).abs() < 1.0);
        let clock = clock(field);
        assert!(clock.bottom() <= field.y, "часы выше полей");
        let date = date(field);
        assert!(date.y >= clock.bottom() && date.bottom() <= field.y);
        let group_middle = (clock.y + field.y) / 2.0;
        assert!(
            group_middle < VIEW.1 / 2.0,
            "композиция выше середины экрана"
        );
        assert!(clock.y >= 0.0);
    }

    /// Маленький экран: поля не уезжают за края и не получают отрицательных
    /// координат.
    #[test]
    fn a_small_surface_still_contains_the_fields() {
        let field = field((480.0, 400.0));
        assert!(field.x >= 0.0);
        assert!(field.y >= 0.0);
        assert!(field.w <= 480.0);
    }

    /// Строки не пересекаются, идут сверху вниз и лежат в пределах строки
    /// подсказок. Это единственная защита от «сообщение наехало на кнопку».
    #[test]
    fn the_rows_do_not_overlap() {
        let layout = of(field(VIEW));
        let rows = [
            layout.field_user,
            layout.field_password,
            layout.message,
            layout.button,
            layout.hints,
        ];
        for pair in rows.windows(2) {
            assert!(
                pair[0].bottom() <= pair[1].y + 0.01,
                "строки пересекаются: {:?} и {:?}",
                pair[0],
                pair[1]
            );
        }
        for row in rows {
            assert!(row.x >= 0.0 && row.right() <= VIEW.0);
        }
    }

    /// Место под сообщение reserved всегда: высота композиции от него не
    /// зависит.
    #[test]
    fn the_message_row_reserves_space() {
        let layout = of(field(VIEW));
        assert!(layout.message.h >= 16.0);
        assert_eq!(group_height(), layout.hints.bottom() - clock(field(VIEW)).y);
    }

    /// Метка Caps Lock внутри поля пароля справа и не наезжает на его центр.
    #[test]
    fn the_caps_badge_lives_in_the_password_field() {
        let layout = of(field(VIEW));
        assert_eq!(layout.caps.y, layout.field_password.y);
        assert_eq!(layout.caps.h, layout.field_password.h);
        assert!(layout.caps.right() <= layout.field_password.right());
        assert!(layout.caps.x >= layout.field_password.x + layout.field_password.w / 2.0);
    }

    /// Кнопка и поля одной ширины: иначе акцентная кнопка выглядит как
    /// отдельный элемент, а не действие над этими полями.
    #[test]
    fn the_button_matches_the_fields() {
        let layout = of(field(VIEW));
        assert_eq!(layout.button.x, layout.field_user.x);
        assert_eq!(layout.button.w, layout.field_user.w);
    }
}
