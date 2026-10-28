//! Геометрия экрана входа: карточка, поля, кнопка, подвал.
//!
//! Ни одного нового числа «на глаз»: ширина карточки — `card_frame::CARD_W`,
//! отступы — `spacing`, высота строки — `spacing::XXL`, кегли — `type_scale`.
//! Всё, что считается здесь, — производные от них, поэтому карточка входа и
//! карточки остальных окон не разъезжаются по отступам.
//!
//! Раскладка чистая: на входе размер поверхности, на выходе прямоугольники.
//! Поэтому её проверяют тестом — без пикселей и без Wayland.

use super::card_frame;
use super::geom::Rect;
use super::ui_tokens::{spacing, type_scale};

/// Ширина карточки: та же, что у меню и окон биндов.
pub const CARD_W: f32 = card_frame::CARD_W;

/// Отступ содержимого карточки от её края.
pub const PAD: f32 = spacing::XL;

/// Высота поля ввода: строка плюс половина зазора — 36 px при токенах 32 и 4.
pub const FIELD_H: f32 = spacing::XXL + spacing::XS;

/// Высота кнопки: крупнее поля, чтобы Enter было видно как действие.
pub const BUTTON_H: f32 = spacing::XXL + spacing::MD;

/// Полоса подписи над полем.
pub const LABEL_H: f32 = spacing::LG + spacing::XS / 2.0;

/// Высота верхней полосы: тонкая, но кликабельная глазом.
pub const STRIP_H: f32 = spacing::XXL;

/// Зазор между группами.
const GROUP_GAP: f32 = spacing::MD;
/// Зазор внутри группы «подпись — поле».
const LABEL_GAP: f32 = spacing::XS;
/// Высота строки сообщения. Место под неё reserved всегда: карточка не должна
/// прыгать, когда PAM присылает ошибку.
pub const MESSAGE_H: f32 = spacing::LG + spacing::SM;

/// Полная высота карточки.
pub fn card_height() -> f32 {
    let ts = type_scale();
    PAD
        + ts.caption * 2.0            // «HUDBAR»
        + spacing::SM                 // зазор под заголовком
        + ts.display * 1.3            // часы
        + spacing::XS + ts.label * 1.6 // дата
        + GROUP_GAP
        + (LABEL_H + LABEL_GAP + FIELD_H) * 2.0 // два поля
        + spacing::SM                        // зазор под полем пароля
        + MESSAGE_H
        + spacing::XS                        // зазор до кнопки
        + BUTTON_H
        + PAD
        + card_frame::FOOTER_H
}

/// Карточка на поверхности: по центру по горизонтали и чуть выше центра по
/// вертикали — так её не перекрывает строка подсказок и она не «висит» внизу.
pub fn card(size: (f32, f32)) -> Rect {
    let height = card_height();
    let x = ((size.0 - CARD_W) / 2.0).max(0.0);
    // 0.42 свободного места сверху: чуть выше геометрического центра.
    let y = ((size.1 - height) * 0.42).max(0.0);
    Rect::new(x.round(), y.round(), CARD_W, height)
}

/// Прямоугольники содержимого карточки.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub card: Rect,
    /// Мелкий заголовок «HUDBAR».
    pub title: Rect,
    /// Крупные часы.
    pub clock: Rect,
    /// Дата строкой.
    pub date: Rect,
    pub user_label: Rect,
    pub user_box: Rect,
    pub password_label: Rect,
    /// Метка Caps Lock справа от подписи поля пароля.
    pub caps: Rect,
    pub password_box: Rect,
    /// Строка сообщения: есть всегда, рисуется не всегда.
    pub message: Rect,
    pub button: Rect,
    /// Подвал с подсказками.
    pub footer: Rect,
}

/// Содержимое карточки сверху вниз. Единственный расчёт вертикали: все
/// прямоугольники выводятся из одного курсора, поэтому зазор между полями не
/// может разъехаться.
pub fn of(card: Rect) -> Layout {
    let ts = type_scale();
    let left = card.x + PAD;
    let width = card.w - PAD * 2.0;
    let mut y = card.y + PAD;

    let title = Rect::new(left, y, width, ts.caption * 2.0);
    y = title.bottom() + spacing::SM;

    let clock = Rect::new(left, y, width, ts.display * 1.3);
    y = clock.bottom() + spacing::XS;

    let date = Rect::new(left, y, width, ts.label * 1.6);
    y = date.bottom() + GROUP_GAP;

    let user_label = Rect::new(left, y, width, LABEL_H);
    let user_box = Rect::new(left, user_label.bottom() + LABEL_GAP, width, FIELD_H);
    y = user_box.bottom() + GROUP_GAP;

    let password_label = Rect::new(left, y, width, LABEL_H);
    // Метка Caps Lock живёт в полосе подписи: отдельная строка под полем
    // сдвигала бы кнопку и ломала бы «карточка не прыгает».
    let caps = Rect::new(
        password_label.right() - width * 0.5,
        password_label.y,
        width * 0.5,
        password_label.h,
    );
    let password_box = Rect::new(left, password_label.bottom() + LABEL_GAP, width, FIELD_H);
    y = password_box.bottom() + spacing::SM;

    let message = Rect::new(left, y, width, MESSAGE_H);
    y = message.bottom() + spacing::XS;

    let button = Rect::new(left, y, width, BUTTON_H);

    // Подвал прижат к низу карточки, а не к кнопке: он часть рамки, как в
    // меню и окнах биндов.
    let footer = Rect::new(
        card.x,
        card.bottom() - card_frame::FOOTER_H,
        card.w,
        card_frame::FOOTER_H,
    );

    Layout {
        card,
        title,
        clock,
        date,
        user_label,
        user_box,
        password_label,
        caps,
        password_box,
        message,
        button,
        footer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (f32, f32) = (1920.0, 1080.0);

    /// Карточка стоит по центру по горизонтали и выше центра по вертикали.
    #[test]
    fn the_card_is_centred_and_sits_above_the_middle() {
        let card = card(VIEW);
        assert!((card.x + card.w / 2.0 - VIEW.0 / 2.0).abs() < 1.0);
        let middle = VIEW.1 / 2.0;
        assert!(
            card.y + card.h / 2.0 < middle,
            "центр карточки выше середины экрана"
        );
        assert!(card.y >= 0.0);
        assert!(card.bottom() <= VIEW.1, "карточка влезает по высоте");
    }

    /// Маленький экран: карточка не уезжает за края и не получает
    /// отрицательных координат.
    #[test]
    fn a_small_surface_still_contains_the_card() {
        let card = card((480.0, 400.0));
        assert!(card.x >= 0.0);
        assert!(card.y >= 0.0);
        assert!(card.w <= 480.0);
    }

    /// Поля не пересекаются, идут сверху вниз в правильном порядке и лежат
    /// внутри карточки. Это единственная защита от «подпись наехала на поле».
    #[test]
    fn the_rows_do_not_overlap_and_stay_inside_the_card() {
        let card = card(VIEW);
        let layout = of(card);
        let rows = [
            layout.title,
            layout.clock,
            layout.date,
            layout.user_label,
            layout.user_box,
            layout.password_label,
            layout.password_box,
            layout.message,
            layout.button,
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
            assert!(row.x >= card.x - 0.01 && row.right() <= card.right() + 0.01);
            assert!(row.y >= card.y - 0.01 && row.bottom() <= card.bottom() + 0.01);
        }
        // Подвал ниже кнопки и прижат к низу карточки.
        assert!(layout.footer.y >= layout.button.bottom());
        assert!((layout.footer.bottom() - card.bottom()).abs() < 0.01);
    }

    /// Место под сообщение reserved всегда: размеры карточки не зависят от
    /// того, есть ли текст ошибки.
    #[test]
    fn the_message_row_reserves_space() {
        let layout = of(card(VIEW));
        assert!(layout.message.h >= 16.0);
        assert!(layout.message.bottom() <= layout.button.y + 0.01);
        assert_eq!(card_height(), of(card(VIEW)).card.h);
    }

    /// Метка Caps Lock стоит в полосе подписи пароля и не наезжает на неё.
    #[test]
    fn the_caps_badge_shares_the_password_label_band() {
        let layout = of(card(VIEW));
        assert_eq!(layout.caps.y, layout.password_label.y);
        assert_eq!(layout.caps.h, layout.password_label.h);
        assert!(layout.caps.right() <= layout.password_label.right() + 0.01);
    }
}
