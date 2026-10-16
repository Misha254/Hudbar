use super::super::canvas::{fill_rect, fill_round_rect, stroke_rect};
use super::super::geom::Rect;
use super::super::settings_icons;
use super::super::state::Frame;
use super::super::strings;
use super::super::text::{Align, SCALE};
use super::super::ui_tokens::{radii, radius, spacing};
use super::MenuView;
use super::geometry::{HEADER_H, ICON, PAD, SEARCH_H, SEARCH_W, center_band};

/// Прямоугольник крошек в шапке. Ширина фиксирована и от языка не зависит:
/// ровно столько, сколько остаётся после поля поиска и полей, поэтому длинная
/// надпись может только обрезаться многоточием, но не вытеснить поле.
pub fn crumbs_rect(header: Rect) -> Rect {
    Rect::new(
        header.x + PAD,
        header.y,
        header.w - PAD * 2.0 - SEARCH_W - spacing::MD,
        HEADER_H,
    )
}

/// Прямоугольник поля поиска: прижат к правому краю шапки, по центру по
/// вертикали.
pub fn search_rect(header: Rect) -> Rect {
    Rect::new(
        header.right() - PAD - SEARCH_W,
        header.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    )
}

/// Крошки «HUD › Стиль» слева и поле поиска справа. Поле не мигает: рамка в
/// фокусе всегда видна, потому что меню работает только с клавиатуры.
pub(crate) fn header(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let header_rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    if !frame.trail.is_empty() {
        // Крошки рисуются двумя текстами: последний уровень ярче, поэтому он
        // меряется отдельно и ставится за своим префиксом.
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
        let crumbs = crumbs_rect(header_rect);
        // Крошки — структурная метка «где я сейчас», поэтому ролью заголовка
        // секции: кеглем строки списка шапка от строки не отличалась ничем,
        // кроме цвета, а цвет при недоступной палитре не различить.
        let title = view.scale.section_title;
        if prefix_text.is_empty() {
            view.text_clipped(
                &last,
                title,
                view.ui.text,
                center_band(crumbs, title * 2.0),
                Align::Start,
            );
        } else {
            view.text_clipped(
                &prefix_text,
                title,
                view.ui.muted,
                center_band(crumbs, title * 2.0),
                Align::Start,
            );
            let width = view.painter.text_width(&prefix_text, title);
            view.text_clipped(
                &last,
                title,
                view.ui.accent,
                center_band(
                    Rect::new(
                        crumbs.x + width + spacing::XS,
                        crumbs.y,
                        (crumbs.w - width).max(0.0),
                        crumbs.h,
                    ),
                    title * 2.0,
                ),
                Align::Start,
            );
        }
    }
    search(view, header_rect, frame);
}

/// Поле поиска: лупа, набранный текст с курсором и крестик очистки.
fn search(view: &mut MenuView<'_>, header_rect: Rect, frame: &Frame) {
    let rect = search_rect(header_rect);
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
        view.ui.border_focus,
        SCALE,
    );
    view.glyph(
        settings_icons::SEARCH,
        view.scale.caption,
        view.ui.muted,
        center_band(
            Rect::new(rect.x + spacing::SM, rect.y, ICON, rect.h),
            view.scale.label * 2.0,
        ),
    );

    let clear = !frame.query.is_empty();
    let text_x = rect.x + spacing::SM + ICON + spacing::SM;
    let text_w = rect.w
        - (spacing::SM + ICON + spacing::SM)
        - spacing::SM
        - if clear { ICON + spacing::XS } else { 0.0 };
    let text_rect = center_band(
        Rect::new(text_x, rect.y, text_w.max(0.0), rect.h),
        view.scale.label * 2.0,
    );
    if frame.query.is_empty() {
        view.text_clipped(
            strings::chrome(frame.lang).search,
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
        view.glyph(
            settings_icons::TIMES,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(rect.right() - spacing::SM - ICON, rect.y, ICON, rect.h),
                view.scale.label * 2.0,
            ),
        );
    }
}
