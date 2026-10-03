//! Отрисовка меню: карточка, шапка, строки и подвал.
//!
//! Окно меню — оверлей поверх всего, поэтому кадр начинается не с карточки, а
//! с затемнения: без него подписи на тёмной панели не читались бы, а в Pixel
//! режим без размытия выглядел бы как чёрный прямоугольник.
//!
//! Геометрия считается сверху вниз и держится на токенах `ui_tokens`: ширина
//! карточки, высота строки и радиусы — константы оттуда, а набор цветов —
//! `UiPalette`. Новых hex здесь нет: всё, что не приходит из палитры, берётся
//! через `mix`, иначе меню перестало бы следовать за matugen.
//!
//! Строки рисует тот же `TextPainter`, что и окно настроек, а глифы —
//! отдельной копией painter с шрифтом Nerd Font: в Pixel основной шрифт —
//! Minecraft Rus, и иконок в нём нет.

use super::super::palette;
use super::super::settings_icons;
use super::super::settings_ui::Rect;
use super::super::settings_view::{fill_rect, fill_round_rect, stroke_rect};
use super::super::text::{Align, SCALE, TextPainter};
use super::super::ui_tokens::{
    TypeScale, UiPalette, mix, radii, radius, spacing, type_scale, ui_palette,
};
use super::item::Item;
use super::state::Frame;
use super::strings;

/// Ширина карточки. Фиксированная: подменю того же меню не должны менять
/// размер окна при переходе.
pub const CARD_W: f32 = 520.0;
/// Высота строки списка.
pub const ROW_H: f32 = 40.0;
/// Минимум строк в карточке. Ниже карточка прыгает при каждом нажатии.
pub const MIN_ROWS: usize = 5;
/// Максимум строк: дальше список уже нужно прокручивать.
pub const MAX_ROWS: usize = 10;
/// Высота шапки с крошками и поиском.
pub const HEADER_H: f32 = 52.0;
/// Высота подвала с подсказками.
pub const FOOTER_H: f32 = 40.0;
/// Высота поля поиска.
pub const SEARCH_H: f32 = 28.0;
/// Ширина поля поиска.
pub const SEARCH_W: f32 = 168.0;
/// Размер иконки строки.
pub const ICON: f32 = 20.0;
/// Отступ текста от края карточки.
pub const PAD: f32 = spacing::LG;
/// Толщина акцентной полосы выбранной строки.
pub const SEL_BAR: f32 = 3.0;
/// Размер кружка тумблера.
pub const TOGGLE_D: f32 = 16.0;
/// Диаметр миниатюры пикера.
pub const THUMB_D: f32 = 28.0;
/// Плотность затемнения вокруг карточки.
pub const SCRIM: f32 = 0.72;

/// Сколько строк показывает карточка: от `MIN_ROWS` до `MAX_ROWS`.
pub fn rows_shown(count: usize) -> usize {
    count.clamp(MIN_ROWS, MAX_ROWS)
}

/// Высота карточки для указанного числа строк.
pub fn card_height(rows: usize) -> f32 {
    HEADER_H + rows_shown(rows) as f32 * ROW_H + FOOTER_H
}

/// Прямоугольник карточки по центру кадра. Возвращает и он сам, и то, что
/// осталось вокруг: затемнение рисуется по всему кадру.
pub fn card_rect(frame_w: f32, frame_h: f32, rows: usize) -> Rect {
    let height = card_height(rows);
    Rect::new(
        (frame_w - CARD_W) / 2.0,
        (frame_h - height) / 2.0,
        CARD_W,
        height,
    )
}

/// Всё, что нужно для кадра.
pub struct MenuView<'a> {
    pub pixmap: &'a mut tiny_skia::Pixmap,
    /// Painter основного шрифта темы.
    pub painter: &'a mut TextPainter,
    /// Отдельная копия painter с Nerd Font — только для глифов.
    pub icons: &'a mut TextPainter,
    pub ui: UiPalette,
    pub scale: TypeScale,
    pub pixel: bool,
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
            radius(self.pixel, radii::LG) * SCALE,
            self.ui.panel,
        );
        let hairline = self.ui.border;
        stroke_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            hairline,
            SCALE,
        );
    }

    /// Текст в полосе. Обёртка нужна, чтобы цвет и выравнивание выбирались
    /// в одном месте, а `paint` вызывался с одним аргументом.
    fn text(&mut self, value: &str, size: f32, color: palette::Rgba, rect: Rect, align: Align) {
        self.painter
            .paint(self.pixmap, value, size, color, rect, align);
    }

    /// Текст с многоточием: длинные значения не должны наезжать на иконки.
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
}

/// Рисует меню целиком и возвращает прямоугольник карточки.
pub fn render(view: &mut MenuView<'_>, frame: &Frame) -> Rect {
    let surface = Rect::new(0.0, 0.0, frame_w(view), frame_h(view));
    let rows = rows_shown(frame.items.len());
    let card = card_rect(surface.w, surface.h, rows);
    view.scrim(surface.w, surface.h);
    view.card(card);
    header(view, card, frame);
    rows_view(view, card, frame, rows);
    footer(view, card, frame);
    card
}

/// Кольцо вокруг выключенного тумблера: внешний круг цветом рамки, внутренний
/// цветом карточки. Обводки окружности среди примитивов нет, а квадратный
/// «кружок» выдавал бы тумблер за чекбокс.
fn ring(view: &mut MenuView<'_>, rect: Rect, color: palette::Rgba) {
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(view.pixel, rect.w / 2.0) * SCALE,
        color,
    );
    let line = 1.0;
    fill_round_rect(
        view.pixmap,
        (rect.x + line) * SCALE,
        (rect.y + line) * SCALE,
        (rect.w - line * 2.0) * SCALE,
        (rect.h - line * 2.0) * SCALE,
        radius(view.pixel, (rect.w - line * 2.0) / 2.0) * SCALE,
        view.ui.panel,
    );
}

/// Ширина кадра: логические пиксели из буфера.
fn frame_w(view: &MenuView<'_>) -> f32 {
    view.pixmap.width() as f32 / SCALE
}

/// Высота кадра.
fn frame_h(view: &MenuView<'_>) -> f32 {
    view.pixmap.height() as f32 / SCALE
}

/// Крошки «HUD › Стиль» слева и поле поиска справа. Поле не мигает: рамка в
/// фокусе всегда видна, потому что меню работает только с клавиатуры.
fn header(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let header_rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    if !frame.trail.is_empty() {
        // Крошки рисуются одним текстом, но последний уровень ярче. Поэтому
        // он выравнивается по правому краю своей части, а предыдущие
        // набираются вручную: измерение каждого глифа дороже, а строка всегда
        // одна и та же.
        let last = frame.trail.last().expect("крошки не пусты").to_string();
        let prefix = frame
            .trail
            .iter()
            .take(frame.trail.len() - 1)
            .map(|crumb| format!("{crumb} "))
            .collect::<String>();
        let prefix_text = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}{} ", strings::CRUMBS_SEPARATOR.trim_end())
        };
        let crumbs = Rect::new(
            header_rect.x + PAD,
            header_rect.y,
            header_rect.w - PAD * 2.0 - SEARCH_W - spacing::MD,
            HEADER_H,
        );
        if prefix_text.is_empty() {
            view.text_clipped(&last, view.scale.label, view.ui.text, crumbs, Align::Start);
        } else {
            view.text_clipped(
                &prefix_text,
                view.scale.label,
                view.ui.muted,
                crumbs,
                Align::Start,
            );
            let width = view.painter.text_width(&prefix_text, view.scale.label);
            view.text_clipped(
                &last,
                view.scale.label,
                view.ui.accent,
                Rect::new(
                    crumbs.x + width + spacing::XS,
                    crumbs.y,
                    (crumbs.w - width).max(0.0),
                    crumbs.h,
                ),
                Align::Start,
            );
        }
    }
    search(view, header_rect, frame);
}

/// Поле поиска: лупа, набранный текст с курсором и крестик очистки.
fn search(view: &mut MenuView<'_>, header_rect: Rect, frame: &Frame) {
    let rect = Rect::new(
        header_rect.right() - PAD - SEARCH_W,
        header_rect.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    );
    let focused = true;
    let border = if focused {
        view.ui.border_focus
    } else {
        view.ui.border_subtle
    };
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(view.pixel, radii::SM) * SCALE,
        view.ui.idle_panel,
    );
    stroke_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        border,
        SCALE,
    );
    let glyph_rect = Rect::new(rect.x + spacing::SM, rect.y, ICON, rect.h);
    view.glyph(
        settings_icons::SEARCH,
        view.scale.caption,
        view.ui.muted,
        glyph_rect,
    );

    let clear = !frame.query.is_empty();
    let text_x = rect.x + spacing::SM + ICON + spacing::SM;
    let text_w = rect.w
        - (spacing::SM + ICON + spacing::SM)
        - spacing::SM
        - if clear { ICON + spacing::XS } else { 0.0 };
    let text_rect = Rect::new(text_x, rect.y, text_w.max(0.0), rect.h);
    if frame.query.is_empty() {
        view.text_clipped(
            strings::SEARCH_PLACEHOLDER,
            view.scale.caption,
            view.ui.text_disabled,
            text_rect,
            Align::Start,
        );
    } else {
        view.text_clipped(
            &frame.query,
            view.scale.label,
            view.ui.text,
            text_rect,
            Align::Start,
        );
        // Курсор: вертикальная черта сразу после набранного текста. Меню
        // только с клавиатуры, поэтому курсор всегда на виду.
        let caret = text_x + view.painter.text_width(&frame.query, view.scale.label);
        let caret = Rect::new(
            caret + 1.0,
            rect.y + spacing::SM,
            2.0,
            rect.h - spacing::SM * 2.0,
        );
        fill_rect(
            view.pixmap,
            caret.x * SCALE,
            caret.y * SCALE,
            caret.w * SCALE,
            caret.h * SCALE,
            view.ui.accent,
        );
    }
    if clear {
        let cross = Rect::new(rect.right() - spacing::SM - ICON, rect.y, ICON, rect.h);
        view.glyph(
            settings_icons::TIMES,
            view.scale.caption,
            view.ui.muted,
            cross,
        );
    }
}

/// Список строк: выбранная подсвечена и отмечена полосой, у каждой строки
/// hairline снизу.
fn rows_view(view: &mut MenuView<'_>, card: Rect, frame: &Frame, rows: usize) {
    let list = Rect::new(card.x, card.y + HEADER_H, card.w, rows as f32 * ROW_H);
    let start = frame
        .items
        .iter()
        .enumerate()
        .skip(frame.top)
        .take(rows)
        .collect::<Vec<_>>();
    let selected_row = frame.top + frame.selected;
    for (index, (position, item)) in start.iter().enumerate() {
        let rect = Rect::new(list.x, list.y + index as f32 * ROW_H, list.w, ROW_H);
        // Разделитель рисуется над строкой, а не под ней: под последней
        // строкой снизу уже подвал, а над первой лишней линии быть не
        // должно — поэтому у первой видимой строки разделителя нет.
        if index > 0 {
            let line = Rect::new(rect.x + PAD, rect.y, rect.w - PAD * 2.0, 1.0);
            fill_rect(
                view.pixmap,
                line.x * SCALE,
                line.y * SCALE,
                line.w * SCALE,
                line.h * SCALE,
                view.ui.border_subtle,
            );
        }
        row(view, rect, item, *position == selected_row);
    }
    if frame.items.is_empty() {
        let empty = Rect::new(card.x, list.y, card.w, list.h);
        view.text_clipped(
            strings::NOTHING_FOUND,
            view.scale.label,
            view.ui.muted,
            empty,
            Align::Center,
        );
    }
}

/// Одна строка: иконка, название, значение справа и индикатор поведения.
fn row(view: &mut MenuView<'_>, rect: Rect, item: &Item, selected: bool) {
    if selected {
        fill_rect(
            view.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            view.ui.surface_hover,
        );
        let bar = Rect::new(rect.x, rect.y, SEL_BAR, rect.h);
        fill_rect(
            view.pixmap,
            bar.x * SCALE,
            bar.y * SCALE,
            bar.w * SCALE,
            bar.h * SCALE,
            view.ui.accent,
        );
    }
    let icon_color = if selected {
        view.ui.accent
    } else {
        view.ui.muted
    };
    let text_color = if selected {
        view.ui.accent
    } else {
        view.ui.text
    };
    let two_line = !item.single_line();
    // В двухстрочном режиме (поиск) подпись снизу, иначе всё по центру полосы.
    let label_h = if two_line { ROW_H / 2.0 } else { ROW_H };
    let label_rect = Rect::new(
        rect.x + PAD + ICON + spacing::MD,
        rect.y,
        rect.w - PAD * 2.0 - ICON - spacing::MD,
        label_h,
    );
    if !item.icon.is_empty() {
        let icon_rect = Rect::new(rect.x + PAD, rect.y, ICON, label_h);
        view.glyph(item.icon, view.scale.label, icon_color, icon_rect);
    }
    view.text_clipped(
        &item.title,
        view.scale.label,
        text_color,
        label_rect,
        Align::Start,
    );
    if let Some(caption) = item.caption.as_deref() {
        let caption_rect = Rect::new(
            label_rect.x,
            rect.y + label_h,
            label_rect.w,
            ROW_H - label_h,
        );
        view.text_clipped(
            caption,
            view.scale.caption,
            view.ui.muted,
            caption_rect,
            Align::Start,
        );
    }
    // Справа: круг тумблера, миниатюра пикера, значение или шеврон подменю.
    let right = rect.right() - PAD;
    if item.kind.is_toggle() {
        let on = item.value == "вкл";
        let d = TOGGLE_D;
        let dot = Rect::new(right - d, rect.y + (rect.h - d) / 2.0, d, d);
        if on {
            fill_round_rect(
                view.pixmap,
                dot.x * SCALE,
                dot.y * SCALE,
                dot.w * SCALE,
                dot.h * SCALE,
                radius(view.pixel, d / 2.0) * SCALE,
                view.ui.accent,
            );
        } else {
            ring(view, dot, mix(view.ui.base, view.ui.border, 0.6));
        }
        return;
    }
    if item.kind.is_picker() {
        // Заглушка миниатюры: настоящая картинка придёт в M4, когда будет
        // источник превью. Форма уже та же — круг 28 px, и шеврон справа от
        // него остаётся: пункт открывает выбор.
        let d = THUMB_D;
        let thumb = Rect::new(right - 16.0 - d, rect.y + (rect.h - d) / 2.0, d, d);
        let fill = mix(view.ui.accent, view.ui.base, 0.45);
        fill_round_rect(
            view.pixmap,
            thumb.x * SCALE,
            thumb.y * SCALE,
            thumb.w * SCALE,
            thumb.h * SCALE,
            radius(view.pixel, d / 2.0) * SCALE,
            fill,
        );
        stroke_rect(
            view.pixmap,
            thumb.x * SCALE,
            thumb.y * SCALE,
            thumb.w * SCALE,
            thumb.h * SCALE,
            view.ui.border_subtle,
            SCALE,
        );
        let chevron_rect = Rect::new(right - 16.0, rect.y, 16.0, rect.h);
        view.glyph(
            settings_icons::CHEVRON,
            view.scale.caption,
            view.ui.muted,
            chevron_rect,
        );
        return;
    }
    let chevron = !item.value.is_empty() || item.kind.is_submenu();
    if chevron {
        let chevron_rect = Rect::new(right - 16.0, rect.y, 16.0, rect.h);
        view.glyph(
            settings_icons::CHEVRON,
            view.scale.caption,
            view.ui.muted,
            chevron_rect,
        );
    }
    if !item.value.is_empty() {
        let value_w = 168.0;
        let value_rect = Rect::new(
            right - 16.0 - value_w - spacing::SM,
            rect.y,
            value_w,
            label_h,
        );
        view.text_clipped(
            &item.value,
            view.scale.label,
            view.ui.muted,
            value_rect,
            Align::End,
        );
    }
}

/// Подвал: подсказки слева, счётчик или статус справа.
fn footer(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let rect = Rect::new(card.x, card.bottom() - FOOTER_H, card.w, FOOTER_H);
    let line = Rect::new(card.x, rect.y, card.w, 1.0);
    fill_rect(
        view.pixmap,
        line.x * SCALE,
        line.y * SCALE,
        line.w * SCALE,
        line.h * SCALE,
        view.ui.border_subtle,
    );
    // Подсказки раскладываются по колонкам с зазором из токена, а не
    // склеиваются пробелами: в пиксельном шрифте пробел уже, и слипшиеся
    // подсказки не читаются.
    let hints = [
        strings::HINT_SELECT,
        strings::HINT_OPEN,
        strings::HINT_BACK,
        strings::HINT_CLOSE,
    ];
    let counter_w = 96.0;
    let hints_w = rect.w - PAD * 2.0 - counter_w;
    let mut x = rect.x + PAD;
    let limit = rect.x + PAD + hints_w;
    for hint in hints {
        if x >= limit {
            break;
        }
        let width = view.painter.text_width(hint, view.scale.caption);
        view.text_clipped(
            hint,
            view.scale.caption,
            view.ui.muted,
            Rect::new(x, rect.y, width, rect.h),
            Align::Start,
        );
        x += width + spacing::XL;
    }
    let right = Rect::new(rect.right() - PAD - counter_w, rect.y, counter_w, rect.h);
    match frame.status.as_ref() {
        Some(status) => {
            // В режиме статуса счётчика нет: точка сообщает, что сообщение
            // именно об изменении, а не о позиции в списке.
            // Успех — чистый акцент, ошибка — приглушённый: два состояния
            // должны отличаться, иначе «ошибка» выглядит как подтверждение.
            let dot_color = if status.is_failed() {
                mix(view.ui.accent, view.ui.muted, 0.5)
            } else {
                view.ui.accent
            };
            let dot = Rect::new(right.x, right.y + (right.h - 8.0) / 2.0, 8.0, 8.0);
            fill_round_rect(
                view.pixmap,
                dot.x * SCALE,
                dot.y * SCALE,
                dot.w * SCALE,
                dot.h * SCALE,
                radius(view.pixel, 4.0) * SCALE,
                dot_color,
            );
            let label = if status.is_failed() {
                strings::STATUS_FAILED
            } else {
                strings::STATUS_APPLIED
            };
            let label_rect = Rect::new(
                right.x + 8.0 + spacing::XS,
                right.y,
                right.w - 8.0 - spacing::XS,
                right.h,
            );
            view.text_clipped(
                label,
                view.scale.caption,
                view.ui.muted,
                label_rect,
                Align::Start,
            );
        }
        None => {
            // На пустом списке счётчик был бы «1/0»: читается как ошибка.
            // Там ноль из нуля.
            let index = if frame.total == 0 {
                0
            } else {
                frame.selected + 1
            };
            let counter = format!("{index}/{}", frame.total);
            view.text_clipped(
                &counter,
                view.scale.caption,
                view.ui.muted,
                right,
                Align::End,
            );
        }
    }
}

/// Палитра и шкала по теме: тот же расчёт, что у окна настроек, чтобы меню и
/// настройки выглядели одной оболочкой.
pub fn theme(pixel: bool, base: palette::Palette) -> (UiPalette, TypeScale) {
    (ui_palette(pixel, base), type_scale(pixel))
}

/// Шрифт основного текста по теме.
pub fn font_name(pixel: bool) -> String {
    if pixel {
        "Minecraft Rus".to_string()
    } else {
        "JetBrainsMono Nerd Font Propo".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_is_at_least_five_rows_tall() {
        assert_eq!(card_height(0), HEADER_H + 5.0 * ROW_H + FOOTER_H);
        assert_eq!(card_height(3), card_height(5), "минимум держит карточку");
    }

    #[test]
    fn card_stops_growing_at_ten_rows() {
        assert_eq!(card_height(10), HEADER_H + 10.0 * ROW_H + FOOTER_H);
        assert_eq!(card_height(50), card_height(10), "дальше список листается");
    }

    #[test]
    fn rows_shown_is_clamped() {
        assert_eq!(rows_shown(1), MIN_ROWS);
        assert_eq!(rows_shown(7), 7);
        assert_eq!(rows_shown(99), MAX_ROWS);
    }

    #[test]
    fn card_sits_in_the_middle_of_the_frame() {
        let rect = card_rect(1000.0, 700.0, 6);
        assert_eq!(rect.x, (1000.0 - CARD_W) / 2.0);
        assert_eq!(rect.w, CARD_W);
        assert_eq!(
            rect.y,
            (700.0 - rect.h) / 2.0,
            "карточка центрируется по вертикали"
        );
    }

    #[test]
    fn dimensions_stay_on_the_four_pixel_grid() {
        for value in [CARD_W, ROW_H, HEADER_H, FOOTER_H, SEARCH_H, PAD] {
            assert_eq!(
                value % spacing::XS,
                0.0,
                "{value} не кратно четырём: сетка разъедется"
            );
        }
        // Полоса выделения — единственное исключение: 3 px берутся из
        // макета, и на сетку они не влияют, а вот 4 px читались бы как
        // рамка строки.
        assert_eq!(SEL_BAR, 3.0);
    }
}
