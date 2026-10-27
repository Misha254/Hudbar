//! Отрисовка меню: карточка, шапка, строки и подвал.
//!
//! Окно меню — оверлей поверх всего, поэтому кадр начинается не с карточки, а
//! с затемнения: без него подписи на тёмной панели не читались бы.
//!
//! Геометрия считается сверху вниз и держится на токенах `ui_tokens`: ширина
//! карточки, высота строки и радиусы — константы оттуда, а набор цветов —
//! `UiPalette`. Новых hex здесь нет: всё, что не приходит из палитры, берётся
//! через `mix`, иначе меню перестало бы следовать за matugen.
//!
//! Три правила, которые видно на снимках и которые поэтому вынесены в
//! функции, а не размазаны по коду:
//!
//! * выбранная строка залита акцентом целиком, а текст и иконка на ней —
//!   тёмным `on_primary`: акцентом текст читался бы на акценте;
//! * подсветка и полоса отступают от рамки карточки на `CARD_INSET`, иначе
//!   заливка съедала бы hairline по краю;
//! * по вертикали всё центрируется одним расчётом `center_band`, а базовая
//!   линия берётся в `text.rs` по измеренным пикселям глифов, а не по
//!   константе размера шрифта.
//!
//! Строки рисует тот же `TextPainter`, что и окно настроек, а глифы —
//! отдельным painter с шрифтом Nerd Font.

use super::super::canvas::{fill_round_rect, stroke_rect};
use super::super::geom::Rect;
use super::super::palette;
use super::super::text::{Align, SCALE, TextPainter};
use super::super::ui_tokens::{TypeScale, UiPalette, radii, radius, type_scale, ui_palette};
use super::state::Frame;

mod footer;
mod geometry;
mod header;
mod rows;
mod secret;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub use footer::{layout_hints, right_zone_width, zone_misses};
#[allow(unused_imports)] // бины берут только свои константы; здесь панель биндит обеих
pub use geometry::{
    CAPTION_GAP, CARD_ALPHA, CARD_INSET, CARD_W, CHEVRON_W, FOOTER_H, HEADER_H, ICON, MAX_ROWS,
    METER_H, METER_W, MIN_ROWS, PAD, ROW_H, ROW_H_SEARCH, SCRIM, SCROLLBAR_INSET, SCROLLBAR_W,
    SEARCH_H, SEARCH_W, STATUS_DOT, THUMB_D, TOGGLE_D, VALUE_W, VOLUME_VALUE_W, apply_card_alpha,
    blend, card_height, card_height_for, card_rect, center_band, highlight_rect, layer_size,
    luminance, row_at, row_band, row_height, rows_for, rows_shown, search_mode, selectable_row_at,
};
#[allow(unused_imports)]
pub use header::{crumbs_rect, search_rect};
#[cfg(test)]
pub(crate) use rows::{empty_notice_rect, scrollbar_rect};

#[cfg(test)]
pub(crate) use footer::COUNTER_SAMPLES;
pub(crate) use footer::footer;
#[cfg(test)]
pub(crate) use footer::hints_width;
pub(crate) use header::header;
pub(crate) use rows::rows_view;
#[cfg(test)]
pub(crate) use rows::{meter_fill, meter_rect, meter_track, selected_row, trailing_w, window_rows};
#[allow(unused_imports)]
#[cfg(test)]
pub(crate) use secret::SECRET_ROWS;
pub(crate) use secret::secret_view;

/// Всё, что нужно для кадра.
pub struct MenuView<'a> {
    pub pixmap: &'a mut tiny_skia::Pixmap,
    /// Painter основного шрифта темы.
    pub painter: &'a mut TextPainter,
    /// Отдельная копия painter с Nerd Font — только для глифов.
    pub icons: &'a mut TextPainter,
    pub ui: UiPalette,
    pub scale: TypeScale,
}

impl<'a> MenuView<'a> {
    /// Затемнение всего кадра: карточка должна читаться поверх любого фона.
    /// Пиксели смешиваются вручную — `fill_rect` альфу не применяет, и без
    /// смешивания получился бы непрозрачный чёрный прямоугольник.
    fn scrim(&mut self, w: f32, h: f32) {
        let dark = self.ui.base.with_a(SCRIM);
        let factor = dark.3 as u32;
        let width = (w * SCALE) as u32;
        let height = (h * SCALE) as u32;
        let pixels = self.pixmap.pixels_mut();
        for pixel in pixels.iter_mut().take((width * height) as usize) {
            let inv = 255 - factor;
            let red = (pixel.red() as u32 * inv / 255 + dark.0 as u32 * factor / 255) as u8;
            let green = (pixel.green() as u32 * inv / 255 + dark.1 as u32 * factor / 255) as u8;
            let blue = (pixel.blue() as u32 * inv / 255 + dark.2 as u32 * factor / 255) as u8;
            if let Some(next) = tiny_skia::PremultipliedColorU8::from_rgba(red, green, blue, 255) {
                *pixel = next;
            }
        }
    }

    /// Подложка карточки и её рамка. Поверх затемнения она не должна быть
    /// полупрозрачной: иначе сквозь неё просвечивали бы обои.
    fn card(&mut self, rect: Rect) {
        fill_round_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            radius(radii::LG) * SCALE,
            self.ui.panel,
        );
        stroke_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            self.ui.border,
            SCALE,
        );
    }

    /// Текст с многоточием: длинные значения не должны наезжать на иконки.
    /// Базовая линия считается внутри `text.rs` по измеренным пикселям.
    fn text_clipped(
        &mut self,
        value: &str,
        size: f32,
        color: palette::Rgba,
        rect: Rect,
        align: Align,
    ) {
        self.painter
            .paint_boxed(self.pixmap, value, size, color, rect, align);
    }

    /// Глиф Nerd Font отдельным шрифтом.
    fn glyph(&mut self, value: &str, size: f32, color: palette::Rgba, rect: Rect) {
        self.icons
            .paint(self.pixmap, value, size, color, rect, Align::Center);
    }

    /// Заливка кругом по центру прямоугольника. Радиус всегда половина
    /// стороны: круг — это форма миниатюры и кружка, а не скругление угла.
    fn disc(&mut self, rect: Rect, color: palette::Rgba) {
        let r = rect.w.min(rect.h) / 2.0;
        let cx = rect.x + rect.w / 2.0;
        let cy = rect.y + rect.h / 2.0;
        fill_round_rect(
            self.pixmap,
            (cx - r) * SCALE,
            (cy - r) * SCALE,
            r * 2.0 * SCALE,
            r * 2.0 * SCALE,
            r * SCALE,
            color,
        );
    }
}

/// Рисует меню целиком и возвращает прямоугольник карточки. Кадр снимка:
/// сначала затемнение, потом карточка по центру.
pub fn render(view: &mut MenuView<'_>, frame: &Frame) -> Rect {
    let surface = Rect::new(0.0, 0.0, frame_w(view), frame_h(view));
    let search = search_mode(frame);
    let rows = rows_for(frame);
    let card = card_rect(surface.w, surface.h, rows, search);
    view.scrim(surface.w, surface.h);
    // Снимок рисуется без скользящей полосы: она свойство живого окна, и
    // снимок обязан совпадать с прежним побитово.
    paint(view, card, frame, RowsHighlight::none());
    card
}

/// Рисует меню в окне, где буфер равен самой карточке. Затемнения здесь нет:
/// под меню должны быть видны терминал и обои, размытие показывает
/// правило слоя. Прозрачность карточки задаёт вызывающий через
/// `apply_card_alpha`.
///
/// Что подсвечивается в списке.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowsHighlight {
    /// Позиция полосы выбора в строках экрана. `None` — полоса не едет, и
    /// выбранная строка подсвечивается сама внутри `row()`; `Some` рисует одну
    /// отдельную полосу на весь список.
    pub sliding: Option<f32>,
    /// Строка под указателем мыши, индекс в `Frame::items`. Это только
    /// подсветка: выбор остаётся клавиатурным, и полоса из `sliding` не
    /// двигается.
    pub hover: Option<usize>,
}

impl RowsHighlight {
    /// Без полосы и без мыши: кадр рисуется как есть.
    pub fn none() -> Self {
        Self::default()
    }
}

pub fn render_window(view: &mut MenuView<'_>, frame: &Frame, highlight: RowsHighlight) -> Rect {
    let search = search_mode(frame);
    let rows = rows_for(frame);
    let card = Rect::new(0.0, 0.0, CARD_W, card_height_for(rows, search));
    view.card(card);
    paint(view, card, frame, highlight);
    card
}

/// Содержимое карточки: шапка, строки, подвал. Общая часть для снимка и окна —
/// расхождение только в фоне вокруг.
fn paint(view: &mut MenuView<'_>, card: Rect, frame: &Frame, highlight: RowsHighlight) {
    if let Some(secret) = frame.secret.as_ref() {
        secret_view(view, card, frame, secret);
        return;
    }
    let search = search_mode(frame);
    let rows = rows_shown(frame.items.len());
    let row_h = row_height(search);
    header(view, card, frame);
    rows_view(view, card, frame, rows, row_h, highlight);
    footer(view, card, frame);
}

/// Ширина кадра: логические пиксели из буфера.
fn frame_w(view: &MenuView<'_>) -> f32 {
    view.pixmap.width() as f32 / SCALE
}

/// Высота кадра.
fn frame_h(view: &MenuView<'_>) -> f32 {
    view.pixmap.height() as f32 / SCALE
}

/// Кольцо вокруг выключенного тумблера: внешний круг цветом обводки, внутренний
/// круг — «дырка» кольца. Обводки окружности среди примитивов нет, а квадратный
/// «кружок» выдавал бы тумблер за чекбокс. Дырка задаётся отдельно: на
/// выбранной строке круг должен показывать фон самой строки, а не карточку.
pub(crate) fn ring(
    view: &mut MenuView<'_>,
    rect: Rect,
    color: palette::Rgba,
    inner: palette::Rgba,
) {
    view.disc(rect, color);
    let line = 1.0;
    view.disc(
        Rect::new(
            rect.x + line,
            rect.y + line,
            rect.w - line * 2.0,
            rect.h - line * 2.0,
        ),
        inner,
    );
}

/// Палитра и шкала по теме: тот же расчёт, что у окна настроек, чтобы меню и
/// настройки выглядели одной оболочкой.
pub fn theme(base: palette::Palette) -> (UiPalette, TypeScale) {
    (ui_palette(base), type_scale())
}
