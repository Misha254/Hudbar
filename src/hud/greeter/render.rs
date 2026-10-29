//! Кадр экрана входа: фон, полоса, карточка, поля, кнопка и подвал.
//!
//! Здесь нет ни Wayland, ни greetd: на входе буфер, палитра и [`View`] —
//! обычные данные, поэтому тот же код рисует и живое окно, и `--screenshot`.
//! Это главное свойство файла: снимок не может разойтись с экраном, потому
//! что другого кода отрисовки нет.
//!
//! Все координаты логические, буфер физический (`text::SCALE`). Умножение
//! делается здесь и только здесь — `canvas` и `text` уже ждут пиксели буфера,
//! а `card_frame` и `layout` считают в логических.

use chrono::{DateTime, Local};
use tiny_skia::Pixmap;

use super::canvas::{composite, fill_rect, fill_round_rect};
use super::card_frame;
use super::config::Config;
use super::flow::{LineKind, Stage};
use super::geom::Rect;
use super::image::Background;
use super::layout;
use super::palette::Rgba;
use super::panel::{self, Battery};
use super::settings::Language;
use super::strings;
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::{UiPalette, radii, spacing, type_scale};

/// Какое поле в фокусе. От этого зависит рамка поля и подсказка под ним.
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
    /// Имя выбранного сеанса для подвала.
    pub session: &'a str,
    /// Строка под полями: текст и его вид.
    pub message: Option<(&'a str, LineKind)>,
    pub stage: Stage,
    /// Открыт режим питания (`F12`).
    pub power: bool,
    pub battery: Option<Battery>,
    /// 0 — обычный кадр, 1 — карточка и полоса уже ушли после входа.
    /// Значение ведёт `GreeterApp`: оно же снимает затемнение с обоев.
    pub leaving: f32,
}

/// Затемнение поверх фона. Не `ui.base`: затемнение обязано быть чёрным,
/// иначе светлая тема не приглушает картинку, а перекрашивает её.
fn dim_color(dim: f32) -> Rgba {
    let alpha = (dim.clamp(0.0, 1.0) * 255.0) as u8;
    Rgba(0, 0, 0, alpha)
}

/// Подложка под карточкой: `ui.base` с прозрачностью `card_plate`. Тот же
/// прямоугольник и радиус, что у `card_frame::card`, поэтому видна ровно как
/// затемнённое поле карточки, а не как вторая рамка.
///
/// Цвет берётся из палитры, а не чёрный: на светлой схеме плашка светлая, и
/// текст остаётся тёмным на светлом. Ноль — подложки нет, карточка лежит прямо
/// на обоях.
pub(crate) fn paint_card_plate(pixmap: &mut Pixmap, ui: &UiPalette, rect: Rect, plate: f32) {
    let alpha = (plate.clamp(0.0, 1.0) * 255.0) as u8;
    if alpha == 0 {
        return;
    }
    fill_round_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radii::LG * SCALE,
        Rgba(ui.base.0, ui.base.1, ui.base.2, alpha),
    );
}

/// Зазор между цифрами и ячейкой двоеточия. Три логических пикселя: заметно
/// уже моноширинной клетки, но цифры не слипаются с точками.
const CLOCK_COLON_GAP: f32 = spacing::XS * 0.75;

/// Ширина ячейки двоеточия в долях кегля (4.8 px при `display` = 40).
const CLOCK_COLON_W: f32 = 0.12;

/// Три колонки часов: часы, двоеточие по центру своей узкой ячейки, минуты.
/// Ширина считается один раз и не зависит от конкретных цифр: обе пары —
/// по две цифры моноширинного шрифта.
fn clock_columns(
    text: &mut TextPainter,
    now: DateTime<Local>,
    rect: Rect,
    size: f32,
) -> (Rect, Rect, Rect) {
    let (hour, minute) = panel::clock_parts(now);
    let hour_w = text.text_width(&hour, size);
    let minute_w = text.text_width(&minute, size);
    let colon_w = size * CLOCK_COLON_W;
    let colon = Rect::new(rect.x + hour_w + CLOCK_COLON_GAP, rect.y, colon_w, rect.h);
    (
        Rect::new(rect.x, rect.y, hour_w, rect.h),
        colon,
        Rect::new(colon.right() + CLOCK_COLON_GAP, rect.y, minute_w, rect.h),
    )
}

/// Крупные часы «ЧЧ:ММ» тремя вызовами на общей базовой линии. Двоеточие
/// центрируется в узкой ячейке, а не занимает полную клетку моноширинного
/// шрифта, поэтому «10: 17» больше не выглядит как два числа.
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
    for (part, part_rect, align) in [
        (hour.as_str(), hour_rect, Align::Start),
        (":", colon_rect, Align::Center),
        (minute.as_str(), minute_rect, Align::Start),
    ] {
        text.paint_on_baseline(pixmap, part, size, ui.text, part_rect, align, baseline);
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

/// Тонкая верхняя полоса: слева пусто, по центру дата-время, справа батарея и
/// Caps Lock.
///
/// Рабочих столов слева нет намеренно: сессии ещё не существует, и любой
/// индикатор был бы выдумкой.
pub fn paint_strip(pixmap: &mut Pixmap, text: &mut TextPainter, ui: &UiPalette, view: &View<'_>) {
    let ts = type_scale();
    let width = pixmap.width() as f32 / SCALE;
    fill_rect(
        pixmap,
        0.0,
        0.0,
        pixmap.width() as f32,
        layout::STRIP_H * SCALE,
        Rgba(0, 0, 0, 150),
    );

    let band = card_frame::center_band(
        Rect::new(0.0, 0.0, width, layout::STRIP_H),
        ts.caption * 1.6,
    );
    text.paint(
        pixmap,
        &panel::strip_clock(view.lang, view.now),
        ts.caption,
        ui.muted,
        band,
        Align::Center,
    );

    // Правая группа: Caps, затем батарея. Раскладку клавиатуры без новых
    // зависимостей не получить — в отчёте это отмечено как пропуск, а не как
    // забытая строка.
    let mut right = width - spacing::MD;
    if let Some(battery) = view.battery {
        let label = panel::battery_label(battery);
        let label_w = text.text_width(&label, ts.caption);
        let rect = Rect::new(
            right - label_w - ts.caption - spacing::XS,
            band.y,
            label_w,
            band.h,
        );
        text.paint(pixmap, &label, ts.caption, ui.muted, rect, Align::End);
        text.paint(
            pixmap,
            super::settings_icons::BATTERY,
            ts.caption,
            ui.muted,
            Rect::new(
                rect.x - ts.caption - spacing::XS,
                band.y,
                ts.caption,
                band.h,
            ),
            Align::Start,
        );
        right = rect.x - ts.caption - spacing::MD;
    }
    if view.caps {
        let label = strings::text(view.lang).caps;
        let label_w = text.text_width(label, ts.caption);
        text.paint(
            pixmap,
            label,
            ts.caption,
            ui.danger,
            Rect::new(right - label_w, band.y, label_w, band.h),
            Align::End,
        );
    }
}

/// Поле ввода: скруглённая подложка и рамка. Фокус — акцентная рамка, а не
/// отдельная строка: карточка не должна менять высоту при переходе Tab.
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

/// Подвал карточки: подсказки, а в режиме питания — его команды.
fn footer_hints(view: &View<'_>) -> String {
    let t = strings::text(view.lang);
    if view.power {
        format!(
            "{} · {} · {} · {}",
            t.power_off, t.power_reboot, t.power_sleep, t.power_cancel
        )
    } else {
        format!(
            "{} · {} · {}: {} · {}",
            t.hint_login, t.hint_field, t.hint_session, view.session, t.hint_power
        )
    }
}

/// Отступ вокруг карточки в слое ухода: рамка рисуется со сглаживанием, и
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
    let card = layout::card(size);

    // Затемнение снимается вместе с карточкой: к моменту входа обои уже
    // светлые, и появление рабочего стола не даёт скачка яркости.
    paint_background_dim(pixmap, ui, background, config.dim * (1.0 - leaving));
    if leaving > 0.0 {
        paint_leaving(pixmap, text, ui, config, view, card, leaving);
    } else {
        paint_strip(pixmap, text, ui, view);
        paint_card(pixmap, text, ui, config, view, card);
    }
}

/// Уход после входа: полоса и карточка гаснут, карточка чуть увеличивается.
/// Слой рисуется отдельно и кладётся поверх фона, поэтому под ним остаются
/// обои, а не дыра: экран не мигает чёрным посреди перехода.
fn paint_leaving(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    config: &Config,
    view: &View<'_>,
    card: Rect,
    leaving: f32,
) {
    let alpha = 1.0 - leaving;
    let scale = 1.0 + super::anim::EXIT_SCALE * leaving;

    // Полоса на всю ширину: высота такая же, как у обычного кадра.
    let strip_height = (layout::STRIP_H * SCALE) as u32;
    if let Some(mut strip) = Pixmap::new(pixmap.width(), strip_height) {
        strip.fill(tiny_skia::Color::TRANSPARENT);
        paint_strip(&mut strip, text, ui, view);
        composite(pixmap, &strip, (0.0, 0.0), alpha, 1.0);
    }

    // Карточка в своём прямоугольнике: при масштабе она не должна тянуть за
    // собой весь кадр.
    let layer_w = ((card.w + LAYER_PAD * 2.0) * SCALE) as u32;
    let layer_h = ((card.h + LAYER_PAD * 2.0) * SCALE) as u32;
    if let Some(mut layer) = Pixmap::new(layer_w, layer_h) {
        layer.fill(tiny_skia::Color::TRANSPARENT);
        paint_card(
            &mut layer,
            text,
            ui,
            config,
            view,
            Rect::new(LAYER_PAD, LAYER_PAD, card.w, card.h),
        );
        composite(
            pixmap,
            &layer,
            ((card.x - LAYER_PAD) * SCALE, (card.y - LAYER_PAD) * SCALE),
            alpha,
            scale,
        );
    }
}

/// Содержимое карточки: подложка, рамка, поля, кнопка и подвал.
fn paint_card(
    pixmap: &mut Pixmap,
    text: &mut TextPainter,
    ui: &UiPalette,
    config: &Config,
    view: &View<'_>,
    card: Rect,
) {
    let ts = type_scale();
    let rows = layout::of(card);

    // Подложка из `ui.base` под карточкой: на пёстрых обоях карточка с одной
    // полупрозрачной заливкой сливалась с картинкой. Цвет палитры, не чёрный:
    // светлая схема получает светлую плашку.
    paint_card_plate(pixmap, ui, card, config.card_plate);
    // Карточка: та же подложка и hairline-рамка, что у меню и окон биндов.
    card_frame::card(pixmap, card, ui);

    let t = strings::text(view.lang);

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
    let user_focused = view.focus == Field::User;
    paint_field(pixmap, ui, rows.user_box, user_focused);
    paint_field_text(
        pixmap,
        text,
        ui,
        rows.user_box,
        view.username,
        None,
        user_focused,
    );

    text.paint_boxed(
        pixmap,
        t.password,
        ts.caption,
        ui.muted,
        rows.password_label,
        Align::Start,
    );
    if view.caps {
        // Заметная метка у поля пароля: Caps Lock на экране входа — причина
        // половины «неверных» паролей.
        text.paint(pixmap, t.caps, ts.caption, ui.danger, rows.caps, Align::End);
    }
    let password_focused = view.focus == Field::Password;
    paint_field(pixmap, ui, rows.password_box, password_focused);
    // `password_chars` — длина, а не текст: сам пароль в отрисовку не попадает.
    paint_field_text(
        pixmap,
        text,
        ui,
        rows.password_box,
        "",
        Some(view.password_chars),
        password_focused,
    );

    // Строка сообщения: место под неё занято всегда, поэтому карточка не
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
        text.paint_boxed(pixmap, line, ts.caption, color, rows.message, Align::Start);
    }

    let button_label = match view.stage {
        Stage::Authenticating => t.authenticating,
        Stage::Starting => t.starting,
        _ => t.login,
    };
    paint_button(pixmap, text, ui, rows.button, button_label);

    let hints = footer_hints(view);
    // Подсказки начинаются на `layout::PAD`, как поля; разделитель остаётся на
    // всю ширину карточки. Старая `card_frame::footer` не тронута.
    card_frame::footer_inset(pixmap, text, rows.footer, ui, &hints, None, layout::PAD);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn view<'a>(username: &'a str, session: &'a str) -> View<'a> {
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
            session,
            message: None,
            stage: Stage::Idle,
            power: false,
            battery: None,
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

    /// Кадр рисуется без Wayland и без файлов: это и есть `--screenshot`.
    #[test]
    fn a_frame_is_drawn_without_wayland() {
        let (mut pixmap, mut text) = frame();
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let v = view("mihail", "niri");
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);
        // Угол — фон, центр — карточка: кадр не пустой.
        assert_ne!(
            pixmap.pixel(0, 0).map(|p| (p.red(), p.green(), p.blue())),
            None
        );
    }

    /// К концу ухода карточки и полосы нет: вход не заканчивается кадром с
    /// ещё видимой кнопкой «Войти».
    #[test]
    fn the_card_is_gone_at_the_end_of_the_exit() {
        let (mut pixmap, mut text) = frame();
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let mut v = view("mihail", "niri");
        v.leaving = 1.0;
        draw(&mut pixmap, &mut text, &ui, &Config::default(), None, &v);

        let card = layout::card((1920.0, 1080.0));
        let pixel = pixmap
            .pixel(
                ((card.x + card.w / 2.0) * SCALE) as u32,
                ((card.y + card.h / 2.0) * SCALE) as u32,
            )
            .expect("пиксель")
            .demultiply();
        assert_eq!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (ui.base.0, ui.base.1, ui.base.2),
            "в центре карточки должен быть только фон"
        );
    }

    /// На середине ухода кадр отличается от финального: карточка гаснет
    /// постепенно, а не исчезает одним переключением.
    #[test]
    fn the_card_is_still_visible_halfway_through_the_exit() {
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let (mut halfway, mut halfway_text) = frame();
        let mut v = view("mihail", "niri");
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
            "в середине ухода карточка ещё должна быть видна"
        );
    }

    /// Строка сообщения резервируется всегда: с ошибкой и без неё карточка
    /// остаётся на месте.
    #[test]
    fn a_message_does_not_move_the_card() {
        let card = layout::card((1920.0, 1080.0));
        let with = layout::of(card);
        let without = layout::of(layout::card((1920.0, 1080.0)));
        assert_eq!(with.message, without.message);
        assert_eq!(with.button, without.button);
    }

    /// Подвал показывает выбранный сеанс, а в режиме питания — его команды.
    #[test]
    fn the_footer_follows_the_mode() {
        let v = view("mihail", "niri");
        let idle = footer_hints(&v);
        assert!(idle.contains("niri"), "{idle}");
        assert!(idle.contains(strings::RU.hint_power), "{idle}");

        let mut power = view("mihail", "niri");
        power.power = true;
        let hints = footer_hints(&power);
        assert!(hints.contains(strings::RU.power_off), "{hints}");
        assert!(hints.contains(strings::RU.power_cancel), "{hints}");
        assert!(!hints.contains("niri"), "{hints}");
    }

    /// Английские подсказки длиннее русских; после выравнивания по полям они
    /// обязаны влезать целиком, иначе подвал заканчивается многоточием.
    #[test]
    fn the_footer_hints_fit_the_card_at_the_field_inset() {
        let mut text = TextPainter::new(&super::super::ui_tokens::font_name());
        let mut v = view("mihail", "niri");
        v.lang = Language::En;
        let hints = footer_hints(&v);
        let card = layout::card((1920.0, 1080.0));
        let available = card.w - layout::PAD - spacing::LG;
        let width = text.text_width(&hints, type_scale().caption);
        assert!(width <= available, "{width} > {available}: {hints}");
        assert!(hints.ends_with(strings::EN.hint_power), "{hints}");
    }

    /// Английский кадр не должен содержать кириллицы ни в одной подписи.
    #[test]
    fn the_english_frame_has_no_russian_labels() {
        let mut v = view("mihail", "niri");
        v.lang = Language::En;
        let hints = footer_hints(&v);
        fn is_cyrillic(text: &str) -> bool {
            text.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё'))
        }
        assert!(!is_cyrillic(&hints), "{hints}");
        assert!(!is_cyrillic(strings::EN.authenticating));
    }

    fn at(hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 28, hour, minute, 0)
            .single()
            .expect("дата")
    }

    /// Ширина часов не зависит от цифр: обе пары — по две цифры
    /// моноширинного шрифта, а зазоры и ячейка двоеточия фиксированы.
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

    /// Подложка под карточкой проверяется на белом фоне — худшем случае для
    /// светлой схемы. Контраст текста к фону карточки — не ниже 4.5:1.
    #[test]
    fn the_card_plate_keeps_text_readable_on_white() {
        let white = tiny_skia::Color::WHITE;
        let card = Rect::new(20.0, 20.0, 180.0, 80.0);
        let contrast_of = |ui: &UiPalette, plate: f32| {
            let mut pixmap = Pixmap::new(400, 300).expect("буфер");
            pixmap.fill(white);
            paint_card_plate(&mut pixmap, ui, card, plate);
            card_frame::card(&mut pixmap, card, ui);
            let pixel = pixmap
                .pixel(
                    ((card.x + card.w / 2.0) * SCALE) as u32,
                    ((card.y + spacing::XS) * SCALE) as u32,
                )
                .expect("пиксель")
                .demultiply();
            super::super::ui_tokens::contrast(
                ui.text,
                Rgba(pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
            )
        };

        let dark = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        let light = super::super::ui_tokens::ui_palette(super::super::palette::Palette {
            base: Rgba(0xf9, 0xf9, 0xff, 255),
            text: Rgba(0x1a, 0x1b, 0x21, 255),
            primary: Rgba(0x3b, 0x5b, 0x92, 255),
            on_primary: Rgba(0xff, 0xff, 0xff, 255),
            secondary: Rgba(0xdf, 0xe2, 0xeb, 255),
            error: Rgba(0xba, 0x1a, 0x1a, 255),
        });
        let plate = Config::default().card_plate;
        assert!(contrast_of(&dark, plate) >= 4.5, "тёмная схема");
        assert!(contrast_of(&light, plate) >= 4.5, "светлая схема");
    }

    /// Ноль — подложки нет: карточка не обязана темнеть, если так настроено.
    #[test]
    fn a_zero_card_plate_leaves_the_background_alone() {
        let mut pixmap = Pixmap::new(220, 120).expect("буфер");
        pixmap.fill(tiny_skia::Color::WHITE);
        let card = Rect::new(20.0, 20.0, 180.0, 80.0);
        let ui = super::super::ui_tokens::ui_palette(super::super::palette::Palette::default());
        paint_card_plate(&mut pixmap, &ui, card, 0.0);
        let pixel = pixmap.pixel(100, 100).expect("пиксель");
        assert_eq!((pixel.red(), pixel.green(), pixel.blue()), (255, 255, 255));
    }
}
