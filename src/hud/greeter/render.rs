//! Кадр экрана входа: обои, крупные часы, два поля и кнопка.
//!
//! Карточки и верхней полосы нет: всё стоит прямо на затемнённых обоях, как
//! на экране блокировки. Здесь нет ни Wayland, ни greetd: на входе буфер,
//! палитра и [`View`] — обычные данные, поэтому тот же код рисует и живое
//! окно, и `--screenshot`.
//!
//! Все координаты логические, буфер физический (`text::SCALE`). Умножение
//! делается здесь и только здесь — `canvas` и `text` уже ждут пиксели буфера,
//! а `layout` считает в логических.

use chrono::{DateTime, Local};
use tiny_skia::Pixmap;

use super::canvas::{composite, fill_rect, fill_round_rect};
use super::config::Config;
use super::flow::{LineKind, Stage};
use super::geom::Rect;
use super::image::Background;
use super::layout;
use super::palette::Rgba;
use super::panel;
use super::settings::Language;
use super::strings;
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::{UiPalette, radii, spacing, type_scale};

/// Какое поле в фокусе. От этого зависит рамка поля.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Field {
    #[default]
    User,
    Password,
}

/// Всё, что нужно кадру. Никаких ссылок на живой автомат: окно собирает
/// снимок на каждый кадр, и отрисовка не может случайно изменить состояние
/// входа.
pub struct View<'a> {
    pub lang: Language,
    pub now: DateTime<Local>,
    pub username: &'a str,
    pub password_chars: usize,
    pub focus: Field,
    pub caps: bool,
    /// Строка под полями: текст и его вид.
    pub message: Option<(&'a str, LineKind)>,
    pub stage: Stage,
    /// Открыт режим питания (`F12`).
    pub power: bool,
    /// 0 — обычный кадр, 1 — композиция уже ушла после входа.
    pub leaving: f32,
}

/// Затемнение поверх фона. Не `ui.base`: затемнение обязано быть чёрным,
/// иначе светлая тема не приглушает картинку, а перекрашивает её.
fn dim_color(dim: f32) -> Rgba {
    let alpha = (dim.clamp(0.0, 1.0) * 255.0) as u8;
    Rgba(0, 0, 0, alpha)
}

/// Текст прямо на обоях: тень на логический пиксель вниз-вправо. Подложки у
/// экрана входа больше нет, а мелкая строка на светлом участке картинки иначе
/// пропадает.
fn paint_over_wallpaper(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    label: &str,
    size: f32,
    color: Rgba,
    rect: Rect,
    align: Align,
) {
    let shadow = Rect::new(rect.x + 1.0, rect.y + 1.0, rect.w, rect.h);
    text.paint_boxed(pixmap, label, size, Rgba(0, 0, 0, 170), shadow, align);
    text.paint_boxed(pixmap, label, size, color, rect, align);
}

/// Зазор между цифрами и краской двоеточия. Три логических пикселя: заметно
/// уже моноширинной клетки, но цифры не слипаются с точками.
const CLOCK_COLON_GAP: f32 = spacing::XS * 0.75;

/// Три колонки часов: часы, двоеточие и минуты. Клетки цифр считаются по
/// ширине строки и не зависят от конкретных цифр: обе пары — по две цифры
/// моноширинного шрифта, поэтому минуты никогда не прыгают. Двоеточие
/// ставится по своей краске и на середину пустого места справа от часов:
/// строка шире своей краски на правый подшив, и без этой половины точки
/// прижимаются к часам, а правая щель остаётся вдвое шире левой.
fn clock_columns(
    text: &mut TextPainter,
    now: DateTime<Local>,
    rect: Rect,
    size: f32,
) -> (Rect, Rect, Rect) {
    let (hour, minute) = panel::clock_parts(now);
    let hour_w = text.text_width(&hour, size);
    let minute_w = text.text_width(&minute, size);
    let hour_ink = text.ink_metrics(&hour, size).ink_width;
    let colon_w = text.ink_metrics(":", size).ink_width;
    let slack = (hour_w - hour_ink) / 2.0;
    let minute_x = rect.x + hour_w + colon_w + CLOCK_COLON_GAP * 2.0;
    (
        Rect::new(rect.x, rect.y, hour_w, rect.h),
        Rect::new(
            rect.x + hour_ink + slack + CLOCK_COLON_GAP,
            rect.y,
            colon_w,
            rect.h,
        ),
        Rect::new(minute_x, rect.y, minute_w, rect.h),
    )
}

/// Ширина строки часов «ЧЧ:ММ» по той же формуле, что у `clock_columns`:
/// две пары цифр моноширинного шрифта и краска двоеточия с двумя зазорами.
/// Нужна, чтобы отцентрировать часы на обоях.
pub(crate) fn clock_width(text: &mut TextPainter, size: f32) -> f32 {
    text.text_width("00", size) * 2.0
        + text.ink_metrics(":", size).ink_width
        + CLOCK_COLON_GAP * 2.0
}

/// Крупные часы «ЧЧ:ММ» тремя вызовами на общей базовой линии. Двоеточие
/// ставится по своей краске, а не по клетке моноширинного шрифта, поэтому
/// «10: 17» больше не выглядит как два числа.
pub(crate) fn paint_clock(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    now: DateTime<Local>,
    size: f32,
    rect: Rect,
) {
    let (hour, minute) = panel::clock_parts(now);
    let (hour_rect, colon_rect, minute_rect) = clock_columns(text, now, rect, size);
    // Базовая линия — по цифрам, как у прежней единой строки: двоеточие
    // садится на неё же, а не центрируется отдельно и не «всплывает».
    let sample = text.ink_metrics(&hour, size);
    let baseline = text.baseline(rect, &sample);
    for (part, part_rect) in [
        (hour.as_str(), hour_rect),
        (":", colon_rect),
        (minute.as_str(), minute_rect),
    ] {
        text.paint_on_baseline(
            pixmap,
            part,
            size,
            ui.text,
            part_rect,
            Align::Start,
            baseline,
        );
    }
}

/// Фон кадра: картинка по `cover` или однотонный `base`, затем затемнение.
pub(crate) fn paint_background(
    pixmap: &mut Pixmap,
    ui: &UiPalette,
    config: &Config,
    background: Option<&Background>,
) {
    paint_background_dim(pixmap, ui, background, config.dim);
}

/// То же, но затемнение задаёт вызывающий: при уходе с экрана оно плавно
/// снимается, и обои светлеют ещё до появления рабочего стола.
pub(crate) fn paint_background_dim(
    pixmap: &mut Pixmap,
    ui: &UiPalette,
    background: Option<&Background>,
    dim: f32,
) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    match background {
        Some(image) => {
            // Картинка уже в пикселях буфера: `draw_cover` сам считает от его
            // размеров, логические координаты ему не нужны.
            image.draw_cover(pixmap);
            fill_rect(pixmap, 0.0, 0.0, w, h, dim_color(dim));
        }
        None => {
            let mut base = ui.base;
            base.3 = 255;
            fill_rect(pixmap, 0.0, 0.0, w, h, base);
        }
    }
}

/// Поле ввода: скруглённая подложка и рамка. Фокус — акцентная рамка, а не
/// отдельная строка: композиция не должна менять высоту при переходе Tab.
pub(crate) fn paint_field(pixmap: &mut Pixmap, ui: &UiPalette, rect: Rect, focused: bool) {
    let radius = radii::SM * SCALE;
    let (x, y, w, h) = (
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
    );
    // Рамка — внешний прямоугольник, подложка — на 1 px внутри: так у
    // скругления нет квадратных углов, которые дал бы `stroke_rect`.
    fill_round_rect(
        pixmap,
        x,
        y,
        w,
        h,
        radius,
        if focused { ui.accent } else { ui.border },
    );
    fill_round_rect(
        pixmap,
        x + SCALE,
        y + SCALE,
        w - SCALE * 2.0,
        h - SCALE * 2.0,
        (radius - SCALE).max(0.0),
        if focused {
            ui.surface_hover
        } else {
            ui.idle_panel
        },
    );
}

/// Текст внутри поля: значение пользователя или точки пароля с курсором.
///
/// Длина пароля приходит числом, а не строкой: сам пароль в отрисовку не
/// попадает даже в виде `x`. Иначе он оказался бы в куче лишний раз.
pub(crate) fn paint_field_text(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    rect: Rect,
    value: &str,
    dots: Option<usize>,
    focused: bool,
) {
    let ts = type_scale();
    let inner = Rect::new(
        rect.x + spacing::MD,
        rect.y,
        rect.w - spacing::MD * 2.0,
        rect.h,
    );
    let shown = match dots {
        Some(count) => {
            let mut dots = "•".repeat(count);
            if focused {
                // Курсор — обычная черта: мигание потребовало бы таймера, а
                // на экране входа он всё равно не нужен.
                dots.push('|');
            }
            dots
        }
        None => value.to_string(),
    };
    let color = if shown.is_empty() {
        ui.text_disabled
    } else {
        ui.text
    };
    text.paint_boxed(pixmap, &shown, ts.label, color, inner, Align::Start);
}

/// Заглушка пустого поля: подпись внутри рамки вместо подписи над ней.
fn paint_placeholder(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    rect: Rect,
    label: &str,
) {
    let inner = Rect::new(
        rect.x + spacing::MD,
        rect.y,
        rect.w - spacing::MD * 2.0,
        rect.h,
    );
    text.paint_boxed(
        pixmap,
        label,
        type_scale().label,
        ui.text_disabled,
        inner,
        Align::Start,
    );
}

/// Кнопка входа: сплошной акцент и текст `on_primary`.
pub(crate) fn paint_button(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    rect: Rect,
    label: &str,
) {
    let ts = type_scale();
    fill_round_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radii::SM * SCALE,
        ui.accent,
    );
    text.paint_boxed(pixmap, label, ts.label, ui.on_primary, rect, Align::Center);
}

/// Команды режима питания. В обычном кадре нижней строки нет: экран входа
/// повторяет вид блокировки, где подсказок по клавишам не показывают.
fn power_line(view: &View<'_>) -> String {
    let t = strings::text(view.lang);
    format!(
        "{} · {} · {} · {}",
        t.power_off, t.power_reboot, t.power_sleep, t.power_cancel
    )
}

/// Отступ вокруг композиции в слое ухода: рамка рисуется со сглаживанием, и
/// без запаса её крайние пиксели обрезались бы.
const LAYER_PAD: f32 = 2.0;

/// Кадр целиком. `background` уже загружен вызывающим: чтение файла не должно
/// случаться на каждом кадре.
pub fn draw(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    config: &Config,
    background: Option<&Background>,
    view: &View<'_>,
) {
    let leaving = view.leaving.clamp(0.0, 1.0);
    let size = (
        pixmap.width() as f32 / SCALE,
        pixmap.height() as f32 / SCALE,
    );
    let field = layout::field(size);

    // Затемнение снимается вместе с композицией: к моменту входа обои уже
    // светлые, и появление рабочего стола не даёт скачка яркости.
    paint_background_dim(pixmap, ui, background, config.dim * (1.0 - leaving));
    if leaving > 0.0 {
        paint_leaving(pixmap, text, ui, view, field, leaving);
    } else {
        paint_clock_block(pixmap, text, ui, view, field);
        paint_form(pixmap, text, ui, view, field);
    }
}

/// Часы и дата на обоях: крупные цифры по центру полей.
fn paint_clock_block(
    pixmap: &mut Pixmap,
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
    paint_over_wallpaper(
        pixmap,
        text,
        &panel::date_line(view.lang, view.now),
        ts.label,
        ui.muted,
        layout::date(field),
        Align::Center,
    );
}

/// Поля, сообщение, кнопка и подсказки.
fn paint_form(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    view: &View<'_>,
    field: Rect,
) {
    let rows = layout::of(field);
    let t = strings::text(view.lang);
    let caps_w = if view.caps {
        text.text_width(t.caps, type_scale().caption) + spacing::SM
    } else {
        0.0
    };
    let password_input = Rect::new(
        rows.field_password.x,
        rows.field_password.y,
        (rows.field_password.w - caps_w).max(0.0),
        rows.field_password.h,
    );

    paint_field(pixmap, ui, rows.field_user, view.focus == Field::User);
    if view.username.is_empty() {
        paint_placeholder(pixmap, text, ui, rows.field_user, t.user);
    } else {
        paint_field_text(
            pixmap,
            text,
            ui,
            rows.field_user,
            view.username,
            None,
            view.focus == Field::User,
        );
    }

    paint_field(
        pixmap,
        ui,
        rows.field_password,
        view.focus == Field::Password,
    );
    if view.password_chars == 0 {
        paint_placeholder(pixmap, text, ui, password_input, t.password);
    } else {
        // `password_chars` — длина, а не текст: сам пароль в отрисовку не
        // попадает.
        paint_field_text(
            pixmap,
            text,
            ui,
            password_input,
            "",
            Some(view.password_chars),
            view.focus == Field::Password,
        );
    }
    if view.caps {
        text.paint_boxed(
            pixmap,
            t.caps,
            type_scale().caption,
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

    // Строка сообщения: место под неё занято всегда, поэтому композиция не
    // прыгает при появлении ошибки.
    let message = match (&view.message, view.stage) {
        (Some((line, kind)), _) => Some((*line, *kind)),
        (None, Stage::Authenticating) => Some((t.authenticating, LineKind::Info)),
        (None, Stage::Starting) => Some((t.starting, LineKind::Info)),
        (None, _) => None,
    };
    if let Some((line, kind)) = message {
        let color = match kind {
            LineKind::Error => ui.danger,
            LineKind::Info => ui.muted,
        };
        paint_over_wallpaper(
            pixmap,
            text,
            line,
            type_scale().caption,
            color,
            rows.message,
            Align::Start,
        );
    }

    let button_label = match view.stage {
        Stage::Authenticating => t.authenticating,
        Stage::Starting => t.starting,
        _ => t.login,
    };
    paint_button(pixmap, text, ui, rows.button, button_label);

    // Нижняя строка — только команды питания: в обычном кадре подсказок нет.
    if view.power {
        paint_over_wallpaper(
            pixmap,
            text,
            &power_line(view),
            type_scale().caption,
            ui.text,
            rows.hints,
            Align::Center,
        );
    }
}

/// Уход после входа: часы и форма гаснут и чуть увеличиваются. Слой рисуется
/// отдельно и кладётся поверх фона, поэтому под ним остаются обои, а не дыра.
fn paint_leaving(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    view: &View<'_>,
    field: Rect,
    leaving: f32,
) {
    let alpha = 1.0 - leaving;
    let scale = 1.0 + super::anim::EXIT_SCALE * leaving;

    let top = layout::clock(field).y - LAYER_PAD;
    let left = layout::of(field).hints.x - LAYER_PAD;
    let width = layout::HINT_W + LAYER_PAD * 2.0;
    let height = layout::of(field).hints.bottom() + LAYER_PAD - top;
    let Some(mut layer) = Pixmap::new((width * SCALE) as u32, (height * SCALE) as u32) else {
        return;
    };
    layer.fill(tiny_skia::Color::TRANSPARENT);
    let local = Rect::new(field.x - left, field.y - top, field.w, field.h);
    paint_clock_block(&mut layer, text, ui, view, local);
    paint_form(&mut layer, text, ui, view, local);
    composite(pixmap, &layer, (left * SCALE, top * SCALE), alpha, scale);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn view(username: &str) -> View<'_> {
        View {
            lang: Language::Ru,
            now: Local
                .with_ymd_and_hms(2026, 10, 28, 20, 31, 0)
                .single()
                .expect("дата"),
            username,
            password_chars: 0,
            focus: Field::Password,
            caps: false,
            message: None,
            stage: Stage::Idle,
            power: false,
            leaving: 0.0,
        }
    }

    fn frame() -> (Pixmap, TextPainter) {
        let pixmap = Pixmap::new(1920 * SCALE as u32, 1080 * SCALE as u32).expect("буфер");
        (
            pixmap,
            TextPainter::new(&super::super::ui_tokens::font_name()),
        )
    }

    fn centre(pixmap: &Pixmap, rect: Rect) -> tiny_skia::ColorU8 {
        pixmap
            .pixel(
                ((rect.x + rect.w / 2.0) * SCALE) as u32,
                ((rect.y + rect.h / 2.0) * SCALE) as u32,
            )
            .expect("пиксель")
            .demultiply()
    }

    /// Кадр рисуется без Wayland и без файлов: это и есть `--screenshot`.
    #[test]
    fn a_frame_is_drawn_without_wayland() {
        let (mut pixmap, mut text) = frame();
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let v = view("mihail");
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);
        let field = layout::field((1920.0, 1080.0));
        let pixel = centre(&pixmap, field);
        assert_ne!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (ui.base.0, ui.base.1, ui.base.2),
            "поле нарисовано"
        );
    }

    /// К концу ухода формы и часов нет: вход не заканчивается кадром с ещё
    /// видимой кнопкой «Войти».
    #[test]
    fn the_form_is_gone_at_the_end_of_the_exit() {
        let (mut pixmap, mut text) = frame();
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let mut v = view("mihail");
        v.leaving = 1.0;
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);

        let pixel = centre(&pixmap, layout::field((1920.0, 1080.0)));
        assert_eq!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (ui.base.0, ui.base.1, ui.base.2),
            "в центре поля должен быть только фон"
        );
    }

    /// На середине ухода кадр отличается от финального: форма гаснет
    /// постепенно, а не исчезает одним переключением.
    #[test]
    fn the_form_is_still_visible_halfway_through_the_exit() {
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let (mut halfway, mut halfway_text) = frame();
        let mut v = view("mihail");
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
            "в середине ухода форма ещё должна быть видна"
        );
    }

    /// Строка сообщения резервируется всегда: с ошибкой и без неё кнопка
    /// остаётся на месте.
    #[test]
    fn a_message_does_not_move_the_button() {
        let field = layout::field((1920.0, 1080.0));
        let rows = layout::of(field);
        assert!(rows.message.bottom() <= rows.button.y);
        assert_eq!(rows.button, layout::of(field).button);
    }

    /// В обычном кадре нижней строки нет, а в режиме питания она появляется.
    #[test]
    fn the_hints_appear_only_in_power_mode() {
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let rows = layout::of(layout::field((1920.0, 1080.0)));
        let ink = |v: &View<'_>| {
            let (mut pixmap, mut text) = frame();
            draw(&mut pixmap, &mut text, &ui, &Config::default(), None, v);
            let (x0, y0) = ((rows.hints.x * SCALE) as u32, (rows.hints.y * SCALE) as u32);
            let (x1, y1) = (
                (rows.hints.right() * SCALE) as u32,
                (rows.hints.bottom() * SCALE) as u32,
            );
            let mut count = 0;
            for x in x0..x1 {
                for y in y0..y1 {
                    if let Some(pixel) = pixmap.pixel(x, y) {
                        let pixel = pixel.demultiply();
                        if (pixel.red(), pixel.green(), pixel.blue())
                            != (ui.base.0, ui.base.1, ui.base.2)
                        {
                            count += 1;
                        }
                    }
                }
            }
            count
        };
        assert_eq!(ink(&view("mihail")), 0, "в обычном кадре подсказок нет");

        let mut power = view("mihail");
        power.power = true;
        assert!(ink(&power) > 0, "команды питания должны быть видны");
    }

    /// Строка питания перечисляет все команды режима.
    #[test]
    fn the_power_line_lists_the_commands() {
        let mut v = view("mihail");
        v.power = true;
        let line = power_line(&v);
        for label in [
            strings::RU.power_off,
            strings::RU.power_reboot,
            strings::RU.power_sleep,
            strings::RU.power_cancel,
        ] {
            assert!(line.contains(label), "{line}");
        }
    }

    /// Английская строка длиннее русской; она обязана влезать в полосу
    /// целиком, иначе заканчивается многоточием.
    #[test]
    fn the_power_line_fits_its_band() {
        let mut text = TextPainter::new(&super::super::ui_tokens::font_name());
        let mut v = view("mihail");
        v.power = true;
        for lang in [Language::Ru, Language::En] {
            v.lang = lang;
            let line = power_line(&v);
            let width = text.text_width(&line, type_scale().caption);
            assert!(width <= layout::HINT_W, "{lang:?}: {width}");
        }
    }

    /// Английский кадр не должен содержать кириллицы ни в одной подписи.
    #[test]
    fn the_english_frame_has_no_russian_labels() {
        let mut v = view("mihail");
        v.lang = Language::En;
        v.power = true;
        fn is_cyrillic(text: &str) -> bool {
            text.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё'))
        }
        let line = power_line(&v);
        assert!(!is_cyrillic(&line), "{line}");
        assert!(!is_cyrillic(strings::EN.authenticating));
    }

    /// Часы влезают в полосу над полями: иначе крупная строка вылезла бы за
    /// края и не отцентрировалась.
    #[test]
    fn the_clock_fits_the_band_above_the_fields() {
        let mut text = TextPainter::new(&super::super::ui_tokens::font_name());
        let width = clock_width(&mut text, layout::clock_size());
        assert!(width <= layout::FIELD_W);
    }

    fn at(hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 28, hour, minute, 0)
            .single()
            .expect("дата")
    }

    /// Щели вокруг двоеточия, посчитанные по нарисованным пикселям: строки
    /// краски идут подряд, между ними — провалы. Возвращает две щели вокруг
    /// двоеточия — слева и справа.
    fn colon_gaps(now: DateTime<Local>, size: f32) -> (f32, f32) {
        let mut text = TextPainter::new(&super::super::ui_tokens::font_name());
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let rect = Rect::new(0.0, 0.0, 400.0, size * 1.3);
        let (w, h) = ((rect.w * SCALE) as u32, (rect.h * SCALE) as u32);
        let mut pixmap = Pixmap::new(w, h).expect("буфер");
        pixmap.fill(tiny_skia::Color::BLACK);
        paint_clock(&mut pixmap, &mut text, &ui, now, size, rect);

        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut start = None;
        for x in 0..w {
            let ink = (0..h).any(|y| {
                pixmap
                    .pixel(x, y)
                    .is_some_and(|pixel| pixel.alpha() > 0 && pixel.green() > 32)
            });
            match (ink, start) {
                (true, None) => start = Some(x),
                (false, Some(from)) => {
                    runs.push((from, x - 1));
                    start = None;
                }
                _ => {}
            }
        }
        assert_eq!(runs.len(), 5, "две пары цифр и двоеточие: {runs:?}");
        let left = (runs[2].0 - runs[1].1 - 1) as f32 / SCALE;
        let right = (runs[3].0 - runs[2].1 - 1) as f32 / SCALE;
        (left, right)
    }

    /// Точки двоеточия стоят ровно между цифрами при любом времени. Раньше
    /// двоеточие центрировалось по ширине символа, а его краска уже ширины и
    /// рисуется от пера, — точки уезжали к часам и наезжали на последнюю цифру.
    #[test]
    fn the_colon_sits_evenly_between_the_digits() {
        let ts = type_scale();
        for now in [at(10, 17), at(11, 11), at(0, 0), at(23, 59)] {
            let (left, right) = colon_gaps(now, ts.display);
            assert!(
                (left - right).abs() <= 1.0 / SCALE,
                "{now}: щели вокруг двоеточия разные — {left} и {right}"
            );
            assert!(left > 0.0, "{now}: двоеточие слиплось с часами");
        }
    }

    /// Ширина часов не зависит от цифр: обе пары — по две цифры
    /// моноширинного шрифта, а зазоры и клетка двоеточия фиксированы.
    #[test]
    fn the_clock_width_does_not_jump_between_times() {
        let mut text = TextPainter::new(&super::super::ui_tokens::font_name());
        let ts = type_scale();
        let rect = Rect::new(0.0, 0.0, 400.0, ts.display * 1.3);
        let mut width = |now| {
            let (hour_rect, _, minute_rect) = clock_columns(&mut text, now, rect, ts.display);
            minute_rect.right() - hour_rect.x
        };
        let zero = width(at(0, 0));
        let late = width(at(23, 59));
        let ones = width(at(11, 11));
        assert!((zero - late).abs() < 0.01, "{zero} и {late}");
        assert!((zero - ones).abs() < 0.01, "{zero} и {ones}");
        // И это уже не моноширинная клетка двоеточия: строка плотнее.
        assert!(
            zero < text.text_width("00:00", ts.display),
            "{zero} не уже клетки"
        );
    }
}
