//! Кадр экрана блокировки: обои, карточка Hudbar, имя, пароль и подсказки.
//!
//! Тот же расчёт, что у экрана входа: `greeter::layout` и общие примитивы
//! `card_frame`/`canvas`/`text`. Отличие одно — вместо кнопки входа кнопка
//! «Разблокировать», а имя пользователя только показывается.

use chrono::{DateTime, Local};

use crate::card_frame;
use crate::greeter::config::Config;
use crate::greeter::image::Background;
use crate::greeter::layout;
use crate::greeter::panel;
use crate::greeter::render::{
    paint_background, paint_button, paint_card_plate, paint_clock, paint_field, paint_field_text,
};
use crate::settings::Language;
use crate::text::{Align, SCALE, TextPainter};
use crate::ui_tokens::{UiPalette, type_scale};

use super::strings;

/// Что показать в кадре. Пароль сюда не попадает — только его длина.
pub struct View<'a> {
    pub lang: Language,
    pub now: DateTime<Local>,
    pub username: &'a str,
    pub password_chars: usize,
    pub caps: bool,
    /// Ошибка проверки: текст и признак «опасное».
    pub message: Option<(&'a str, bool)>,
    /// PAM уже думает; кнопка и строка сообщения это показывают.
    pub busy: bool,
}

/// Кадр целиком. `background` загружен вызывающим: файл не читается на
/// каждом тике.
pub fn draw(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    config: &Config,
    background: Option<&Background>,
    view: &View<'_>,
) {
    let ts = type_scale();
    let size = (
        pixmap.width() as f32 / SCALE,
        pixmap.height() as f32 / SCALE,
    );
    let card = layout::card(size);
    let rows = layout::of(card);
    let t = strings::text(view.lang);

    paint_background(pixmap, ui, config, background);
    paint_card_plate(pixmap, ui, card, config.card_plate);
    card_frame::card(pixmap, card, ui);

    text.paint_boxed(
        pixmap,
        t.title,
        ts.caption,
        ui.muted,
        rows.title,
        Align::Start,
    );
    paint_clock(pixmap, text, ui, view.now, ts.display, rows.clock);
    text.paint_boxed(
        pixmap,
        &panel::date_line(view.lang, view.now),
        ts.label,
        ui.muted,
        rows.date,
        Align::Start,
    );

    text.paint_boxed(
        pixmap,
        t.user,
        ts.caption,
        ui.muted,
        rows.user_label,
        Align::Start,
    );
    paint_field(pixmap, ui, rows.user_box, false);
    paint_field_text(pixmap, text, ui, rows.user_box, view.username, None, false);

    text.paint_boxed(
        pixmap,
        t.password,
        ts.caption,
        ui.muted,
        rows.password_label,
        Align::Start,
    );
    if view.caps {
        text.paint(pixmap, t.caps, ts.caption, ui.danger, rows.caps, Align::End);
    }
    paint_field(pixmap, ui, rows.password_box, !view.busy);
    paint_field_text(
        pixmap,
        text,
        ui,
        rows.password_box,
        "",
        Some(view.password_chars),
        !view.busy,
    );

    // Строка сообщения: место занято всегда, карточка не прыгает.
    if view.busy {
        text.paint_boxed(
            pixmap,
            t.checking,
            ts.caption,
            ui.muted,
            rows.message,
            Align::Start,
        );
    } else if let Some((line, danger)) = view.message {
        text.paint_boxed(
            pixmap,
            line,
            ts.caption,
            if danger { ui.danger } else { ui.muted },
            rows.message,
            Align::Start,
        );
    }

    let button = if view.busy { t.checking } else { t.unlock };
    paint_button(pixmap, text, ui, rows.button, button);

    // Подсказки начинаются на PAD, как поля; разделитель — на всю карточку.
    card_frame::footer_inset(pixmap, text, rows.footer, ui, t.hints, None, layout::PAD);
}

/// Ширина подсказок при кегле подвала — для проверки, что они влезают.
#[cfg(test)]
fn hints_width(text: &mut TextPainter, lang: Language) -> f32 {
    text.text_width(strings::text(lang).hints, type_scale().caption)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::greeter::config::Config;
    use crate::palette::Palette;
    use crate::settings::Language;
    use crate::ui_tokens::{spacing, ui_palette};
    use chrono::TimeZone;

    fn view<'a>(username: &'a str) -> View<'a> {
        View {
            lang: Language::Ru,
            now: Local
                .with_ymd_and_hms(2026, 10, 29, 12, 30, 0)
                .single()
                .expect("дата"),
            username,
            password_chars: 4,
            caps: false,
            message: None,
            busy: false,
        }
    }

    fn frame() -> (tiny_skia::Pixmap, TextPainter) {
        let pixmap =
            tiny_skia::Pixmap::new(1920 * SCALE as u32, 1080 * SCALE as u32).expect("буфер");
        (pixmap, TextPainter::new(&crate::ui_tokens::font_name()))
    }

    /// Кадр рисуется без Wayland и без файлов.
    #[test]
    fn a_lock_frame_is_drawn_without_wayland() {
        let (mut pixmap, mut text) = frame();
        let ui = ui_palette(Palette::default());
        let v = view("mihail");
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);
        let card = layout::card((1920.0, 1080.0));
        let pixel = pixmap
            .pixel(
                ((card.x + card.w / 2.0) * SCALE) as u32,
                ((card.y + card.h / 2.0) * SCALE) as u32,
            )
            .expect("пиксель");
        assert!(pixel.alpha() > 0, "карточка нарисована");
    }

    /// Подсказки влезают в карточку при левом отступе полей и правом LG:
    /// иначе подвал заканчивается многоточием.
    #[test]
    fn hints_fit_the_card_for_both_languages() {
        let mut text = TextPainter::new(&crate::ui_tokens::font_name());
        let available = layout::CARD_W - layout::PAD - spacing::LG;
        for lang in [Language::Ru, Language::En] {
            let width = hints_width(&mut text, lang);
            assert!(width <= available, "{lang:?}: {width} > {available}");
        }
    }
}
