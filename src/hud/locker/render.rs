//! Кадр экрана блокировки: обои, крупные часы и одно поле пароля.
//!
//! Карточки и подвала нет: на затемнённых обоях остаются часы с датой и поле,
//! под ним — сообщение о неверном пароле. Поле рисуется теми же примитивами,
//! что поля экрана входа.

use chrono::{DateTime, Local};

use crate::canvas::composite;
use crate::geom::Rect;
use crate::greeter::config::Config;
use crate::greeter::image::Background;
use crate::greeter::panel;
use crate::greeter::render::{
    clock_width, paint_background_dim, paint_clock, paint_field, paint_field_text,
};
use crate::settings::Language;
use crate::text::{Align, SCALE, TextPainter};
use crate::ui_tokens::{UiPalette, spacing, type_scale};

use super::{layout, strings};

/// Что показать в кадре. Пароль сюда не попадает — только его длина.
pub struct View<'a> {
    pub lang: Language,
    pub now: DateTime<Local>,
    pub password_chars: usize,
    pub caps: bool,
    /// Ошибка проверки: текст и признак «опасное».
    pub message: Option<(&'a str, bool)>,
    /// PAM уже думает; поле и строка сообщения это показывают.
    pub busy: bool,
    /// 0 — обычный кадр, 1 — композиция уже ушла и замок можно снимать.
    pub leaving: f32,
}

/// Отступ вокруг композиции в слое ухода: поле рисуется со сглаживанием, и
/// без запаса его крайние пиксели обрезались бы.
const LAYER_PAD: f32 = 2.0;

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
    let size = (
        pixmap.width() as f32 / SCALE,
        pixmap.height() as f32 / SCALE,
    );
    let field = layout::field(size);
    let leaving = view.leaving.clamp(0.0, 1.0);

    // Затемнение снимается вместе с композицией: к моменту разблокировки обои
    // уже светлые, и рабочий стол не бьёт по глазам сменой яркости.
    paint_background_dim(pixmap, ui, background, config.dim * (1.0 - leaving));
    if leaving > 0.0 {
        paint_leaving(pixmap, text, ui, view, field, leaving);
    } else {
        paint_clock_block(pixmap, text, ui, view, field);
        paint_field_block(pixmap, text, ui, view, field);
    }
}

/// Уход после верного пароля: часы и поле гаснут и чуть увеличиваются, а
/// слой рисуется отдельно, поэтому под ними остаются обои, а не дыра.
fn paint_leaving(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    view: &View<'_>,
    field: Rect,
    leaving: f32,
) {
    let alpha = 1.0 - leaving;
    let scale = 1.0 + crate::anim::EXIT_SCALE * leaving;
    let top = layout::clock(field).y - LAYER_PAD;
    let left = field.x - LAYER_PAD;
    let width = field.w + LAYER_PAD * 2.0;
    let height = layout::of(field).message.bottom() + LAYER_PAD - top;
    let Some(mut layer) = tiny_skia::Pixmap::new((width * SCALE) as u32, (height * SCALE) as u32)
    else {
        return;
    };
    layer.fill(tiny_skia::Color::TRANSPARENT);
    let local = shift(field, -left, -top);
    paint_clock_block(&mut layer, text, ui, view, local);
    paint_field_block(&mut layer, text, ui, view, local);
    composite(pixmap, &layer, (left * SCALE, top * SCALE), alpha, scale);
}

/// Прямоугольник со сдвигом: слой рисуется в своих координатах.
fn shift(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h)
}

/// Часы и дата на обоях: крупные цифры по центру поля.
fn paint_clock_block(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    view: &View<'_>,
    field: Rect,
) {
    let ts = type_scale();
    let size = layout::clock_size();
    let clock = layout::clock(field);
    let width = clock_width(text, size);
    let clock = Rect::new(clock.x + (clock.w - width) / 2.0, clock.y, width, clock.h);
    paint_clock(pixmap, text, ui, view.now, size, clock);
    text.paint_boxed(
        pixmap,
        &panel::date_line(view.lang, view.now),
        ts.label,
        ui.muted,
        layout::date(field),
        Align::Center,
    );
}

/// Поле пароля: заглушка или точки, курсор, Caps Lock и строка сообщения.
fn paint_field_block(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    view: &View<'_>,
    field: Rect,
) {
    let ts = type_scale();
    let rows = layout::of(field);
    let focused = !view.busy;
    let t = strings::text(view.lang);
    paint_field(pixmap, ui, field, focused);

    let caps_w = if view.caps {
        text.text_width(t.caps, ts.caption) + spacing::SM
    } else {
        0.0
    };
    let input = Rect::new(field.x, field.y, (field.w - caps_w).max(0.0), field.h);
    if view.password_chars == 0 {
        if focused {
            text.paint_boxed(
                pixmap,
                t.password,
                ts.label,
                ui.text_disabled,
                Rect::new(
                    input.x + spacing::MD,
                    input.y,
                    (input.w - spacing::MD * 2.0).max(0.0),
                    input.h,
                ),
                Align::Start,
            );
        }
    } else {
        paint_field_text(
            pixmap,
            text,
            ui,
            input,
            "",
            Some(view.password_chars),
            focused,
        );
    }
    if view.caps {
        text.paint_boxed(
            pixmap,
            t.caps,
            ts.caption,
            ui.danger,
            Rect::new(
                rows.caps.x,
                rows.caps.y,
                rows.caps.w - spacing::MD,
                rows.caps.h,
            ),
            Align::End,
        );
    }

    let line = if view.busy {
        Some((t.checking, false))
    } else {
        view.message
    };
    if let Some((line, danger)) = line {
        text.paint_boxed(
            pixmap,
            line,
            ts.caption,
            if danger { ui.danger } else { ui.muted },
            rows.message,
            Align::Start,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::greeter::config::Config;
    use crate::palette::Palette;
    use crate::ui_tokens::ui_palette;
    use chrono::TimeZone;

    fn view() -> View<'static> {
        View {
            lang: Language::Ru,
            now: Local
                .with_ymd_and_hms(2026, 10, 29, 12, 30, 0)
                .single()
                .expect("дата"),
            password_chars: 4,
            caps: false,
            message: None,
            busy: false,
            leaving: 0.0,
        }
    }

    fn frame() -> (tiny_skia::Pixmap, TextPainter) {
        let pixmap =
            tiny_skia::Pixmap::new(1920 * SCALE as u32, 1080 * SCALE as u32).expect("буфер");
        (pixmap, TextPainter::new(&crate::ui_tokens::font_name()))
    }

    fn centre(pixmap: &tiny_skia::Pixmap, rect: Rect) -> tiny_skia::ColorU8 {
        pixmap
            .pixel(
                ((rect.x + rect.w / 2.0) * SCALE) as u32,
                ((rect.y + rect.h / 2.0) * SCALE) as u32,
            )
            .expect("пиксель")
            .demultiply()
    }

    /// Кадр рисуется без Wayland и без файлов: в центре поля не `base`.
    #[test]
    fn a_lock_frame_is_drawn_without_wayland() {
        let (mut pixmap, mut text) = frame();
        let ui = ui_palette(Palette::default());
        let v = view();
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);
        let pixel = centre(&pixmap, layout::field((1920.0, 1080.0)));
        assert_ne!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (ui.base.0, ui.base.1, ui.base.2),
            "поле нарисовано"
        );
    }

    /// К концу ухода композиции нет: в центре поля остаётся фон. Если бы слой
    /// не гас, замок снимался бы с видимым полем на экране.
    #[test]
    fn the_field_is_gone_at_the_end_of_the_exit() {
        let (mut pixmap, mut text) = frame();
        let ui = ui_palette(Palette::default());
        let mut v = view();
        v.leaving = 1.0;
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);

        let pixel = centre(&pixmap, layout::field((1920.0, 1080.0)));
        assert_eq!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (ui.base.0, ui.base.1, ui.base.2),
            "в центре поля должен быть только фон"
        );
    }

    /// На середине ухода кадр отличается от финального: композиция гаснет
    /// постепенно, а не исчезает одним переключением.
    #[test]
    fn the_field_is_still_visible_halfway_through_the_exit() {
        let ui = ui_palette(Palette::default());
        let (mut halfway, mut halfway_text) = frame();
        let mut v = view();
        v.leaving = 0.5;
        draw(
            &mut halfway,
            &mut halfway_text,
            &ui,
            &Config::default(),
            None,
            &v,
        );

        let (mut finished, mut finished_text) = frame();
        v.leaving = 1.0;
        draw(
            &mut finished,
            &mut finished_text,
            &ui,
            &Config::default(),
            None,
            &v,
        );

        assert_ne!(
            halfway.data(),
            finished.data(),
            "в середине ухода поле ещё должно быть видно"
        );
    }

    /// Часы влезают в полосу над полем: иначе крупная строка вылезла бы за
    /// края и не отцентрировалась.
    #[test]
    fn the_clock_fits_the_band_above_the_field() {
        let mut text = TextPainter::new(&crate::ui_tokens::font_name());
        let width = clock_width(&mut text, layout::clock_size());
        assert!(width <= layout::FIELD_W);
    }
}
