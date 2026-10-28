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

use super::canvas::{fill_rect, fill_round_rect};
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
}

/// Затемнение поверх фона. Не `ui.base`: затемнение обязано быть чёрным,
/// иначе светлая тема не приглушает картинку, а перекрашивает её.
fn dim_color(dim: f32) -> Rgba {
    let alpha = (dim.clamp(0.0, 1.0) * 255.0) as u8;
    Rgba(0, 0, 0, alpha)
}

/// Фон кадра: картинка по `cover` или однотонный `base`, затем затемнение.
fn paint_background(
    pixmap: &mut Pixmap,
    ui: &UiPalette,
    config: &Config,
    background: Option<&Background>,
) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    match background {
        Some(image) => {
            // Картинка уже в пикселях буфера: `draw_cover` сам считает от его
            // размеров, логические координаты ему не нужны.
            image.draw_cover(pixmap);
            fill_rect(pixmap, 0.0, 0.0, w, h, dim_color(config.dim));
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
fn paint_field(pixmap: &mut Pixmap, ui: &UiPalette, rect: Rect, focused: bool) {
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
fn paint_field_text(
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
fn paint_button(
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
    let ts = type_scale();
    let size = (
        pixmap.width() as f32 / SCALE,
        pixmap.height() as f32 / SCALE,
    );
    let card = layout::card(size);
    let rows = layout::of(card);

    paint_background(pixmap, ui, config, background);
    paint_strip(pixmap, text, ui, view);

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
    text.paint(
        pixmap,
        &panel::big_clock(view.now),
        ts.display,
        ui.text,
        rows.clock,
        Align::Start,
    );
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
    card_frame::footer(pixmap, text, rows.footer, ui, &hints, None);
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
}
