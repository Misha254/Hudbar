use super::super::canvas::{fill_rect, fill_round_rect, stroke_rect};
use super::super::geom::Rect;
use super::super::item::{Item, ItemKind};
use super::super::settings::Language;
use super::super::settings_icons;
use super::super::state::Frame;
use super::super::strings;
use super::super::text::{Align, SCALE};
use super::super::ui_tokens::spacing;
use super::geometry::{
    CAPTION_GAP, CHEVRON_W, HEADER_H, ICON, MAX_ROWS, METER_H, METER_W, PAD, SCROLLBAR_INSET,
    SCROLLBAR_W, THUMB_D, TOGGLE_D, VALUE_W, VOLUME_VALUE_W, blend, center_band, highlight_rect,
    marker_rect, row_band, rows_shown, selected_background,
};
use super::{MenuView, RowsHighlight, ring};

/// Строки окна с их абсолютными индексами: ровно те, что рисуются, и не
/// больше. Больше рисовать незачем — под последней строкой оставалось пустое
/// поле, когда список прокручен до конца.
pub(crate) fn window_rows(frame: &Frame, rows: usize) -> Vec<(usize, &Item)> {
    frame
        .items
        .iter()
        .enumerate()
        .skip(frame.top)
        .take(rows)
        .collect()
}

/// Прямоугольник, в котором стоит подпись пустого списка.
///
/// У пустого списка нарисованных строк нет, поэтому высота области равна нулю
/// и подпись прижималась к шапке, оставляя под собой пустое поле во всю
/// высоту минимума. Здесь высота берётся от `MIN_ROWS`: подпись стоит там же,
/// где стояли бы строки.
pub fn empty_notice_rect(list: Rect, row_h: f32) -> Rect {
    Rect::new(list.x, list.y, list.w, rows_shown(0) as f32 * row_h)
}

/// Полоса прокрутки у правого края списка.
///
/// Показывается только когда список длиннее окна: до этого и подвал со
/// счётчиком `n/m` отвечают за это, и лишняя полоса была бы третьим
/// индикатором одного и того же. Высота и положение пропорциональны видимому
/// окну, полоса не анимируется — список и так едет под пальцем.
pub fn scrollbar_rect(list: Rect, total: usize, top: usize) -> Option<Rect> {
    let shown = total.min(MAX_ROWS);
    if total <= MAX_ROWS || shown == 0 {
        return None;
    }
    let height = list.h;
    let thumb_h = (height * shown as f32 / total as f32).max(SCROLLBAR_W * 2.0);
    let travel = height - thumb_h;
    let offset = if total > shown {
        travel * (top as f32 / (total - shown) as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(Rect::new(
        list.right() - SCROLLBAR_INSET - SCROLLBAR_W,
        list.y + offset,
        SCROLLBAR_W,
        thumb_h,
    ))
}

/// Абсолютный индекс подсвеченной строки.
///
/// `selected` — индекс во всём списке, а позиция в цикле отрисовки тоже
/// абсолютная: `enumerate` идёт до `skip`. Прибавлять тут `top` значило бы
/// подсветить строку на `top` ниже нужной, то есть перескочить через
/// выбранную, как только список прокрутится.
pub(crate) fn selected_row(frame: &Frame) -> usize {
    frame.selected
}

/// Список строк: выбранная подсвечена и отмечена полосой, между строками
/// hairline.
pub(crate) fn rows_view(
    view: &mut MenuView<'_>,
    card: Rect,
    frame: &Frame,
    rows: usize,
    row_h: f32,
    highlight: RowsHighlight,
) {
    let start = window_rows(frame, rows);
    let shown = start.len();
    let list = Rect::new(card.x, card.y + HEADER_H, card.w, shown as f32 * row_h);
    let selected_row = selected_row(frame);
    // Полоса подсветки рисуется до строк, чтобы содержимое выбранной строки
    // лежало поверх неё, как и при отрисовке внутри `row()`.
    if let Some(pos) = highlight.sliding
        && shown > 0
    {
        let slot = pos.clamp(0.0, shown as f32 - 1.0);
        highlight_band(
            view,
            Rect::new(list.x, list.y + slot * row_h, list.w, row_h),
        );
    }
    for (index, (position, item)) in start.iter().enumerate() {
        let rect = Rect::new(list.x, list.y + index as f32 * row_h, list.w, row_h);
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
        // Насколько строка близка к едущей полосе: 1 — под полосой, 0 — мимо.
        // Без полосы близость равна обычному выбору, и цвета совпадают побитово.
        let near = match highlight.sliding {
            Some(pos) => (1.0 - (pos - index as f32).abs()).clamp(0.0, 1.0),
            None => usize::from(*position == selected_row) as f32,
        };
        row(
            view,
            rect,
            item,
            frame.lang,
            RowState {
                selected: *position == selected_row,
                paint_selected: highlight.sliding.is_none(),
                near,
                hover: *position == highlight.hover.unwrap_or(usize::MAX),
            },
        );
    }
    // Полоса прокрутки рисуется после строк: заливка выбранной строки идёт
    // от `CARD_INSET` до правого края и накрыла бы её иначе.
    if let Some(track) = scrollbar_rect(list, frame.total, frame.top) {
        fill_rect(
            view.pixmap,
            track.x * SCALE,
            track.y * SCALE,
            track.w * SCALE,
            track.h * SCALE,
            view.ui.muted,
        );
    }
    if frame.items.is_empty() {
        // Список пуст, поэтому у нарисованных строк нулевая высота: подпись по
        // центру такой полосы прижималась к шапке, а под ней оставалось пустое
        // поле во всю высоту минимума. Центрируем по всей области списка.
        view.text_clipped(
            strings::chrome(frame.lang).nothing,
            view.scale.label,
            view.ui.muted,
            empty_notice_rect(list, row_h),
            Align::Center,
        );
    }
}

/// Подсветка выбранной строки целиком: заливка и акцентная полоса слева.
///
/// Отдельная функция нужна для скользящей полосы: она рисуется одна на весь
/// список, а не внутри строки, и должна совпадать с прежней побитово.
fn highlight_band(view: &mut MenuView<'_>, rect: Rect) {
    // Подсветка и полоса отступают от краёв карточки на CARD_INSET:
    // заливка от самого края ложилась бы на hairline рамки.
    let inner = highlight_rect(rect);
    fill_rect(
        view.pixmap,
        inner.x * SCALE,
        inner.y * SCALE,
        inner.w * SCALE,
        inner.h * SCALE,
        selected_background(view.ui),
    );
    let marker = marker_rect(rect);
    fill_rect(
        view.pixmap,
        marker.x * SCALE,
        marker.y * SCALE,
        marker.w * SCALE,
        marker.h * SCALE,
        view.ui.accent,
    );
}

/// Состояние строки для отрисовки: что выбрано, что под указателем и насколько
/// строка близка к едущей полосе.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowState {
    /// Строка выбрана с клавиатуры.
    pub selected: bool,
    /// Фон выбранной строки рисуется здесь, а не отдельной полосой. `false`,
    /// когда полоса едет и фон уже нарисован на весь список.
    pub paint_selected: bool,
    /// Насколько строка близка к едущей полосе: 1 — под полосой, 0 — мимо.
    pub near: f32,
    /// Строка под указателем мыши.
    pub hover: bool,
}

/// Одна строка: иконка, название, значение справа и индикатор поведения.
///
/// `state.paint_selected` снимает с строки её фон: когда подсветка едет
/// отдельной полосой, фон здесь нарисовался бы дважды и стоял бы на месте.
fn row(view: &mut MenuView<'_>, rect: Rect, item: &Item, lang: Language, state: RowState) {
    if state.selected && state.paint_selected {
        highlight_band(view, rect);
    } else if state.hover {
        // Подложка под указателем — та же заливка без акцентной полосы и без
        // смены цвета текста: мышь показывает, где кликнёт, но не выбирает.
        let inner = highlight_rect(rect);
        fill_rect(
            view.pixmap,
            inner.x * SCALE,
            inner.y * SCALE,
            inner.w * SCALE,
            inner.h * SCALE,
            view.ui.hover_bg,
        );
    }
    // Цвет текста и иконки смешивается с расстоянием до едущей полосы, а не
    // переключается сразу: строка, которую полоса сейчас проходит, проявляется
    // постепенно. При `near` ровно 0 или 1 смешивание даёт прежние цвета
    // побитово, поэтому кадр без анимации не меняется.
    let near = state.near.clamp(0.0, 1.0);
    let icon_color = blend(view.ui.muted, view.ui.accent, near);
    let text_color = blend(view.ui.text, view.ui.accent, near);
    let two_line = !item.single_line();
    let icon_x = rect.x + PAD;
    let label_x = icon_x + ICON + spacing::MD;
    let right = rect.right() - PAD;
    // Ширина зоны названия: до значения или до шеврона, чтобы длинное имя не
    // наезжало на правую часть.
    let label_w = (right - trailing_w(&item.kind) - label_x).max(0.0);
    let label_rect = Rect::new(label_x, rect.y, label_w, rect.h);
    if !item.icon.is_empty() {
        let icon_rect = center_band(Rect::new(icon_x, rect.y, ICON, rect.h), row_band(view));
        view.glyph(item.icon, view.scale.label, icon_color, icon_rect);
    }
    match item.caption.as_deref() {
        Some(caption) => {
            // Две строки: блок из названия и пути центрируется по строке, а
            // между ними ровно CAPTION_GAP.
            let block = (view.scale.label + view.scale.caption + CAPTION_GAP).max(1.0);
            let band = center_band(label_rect, block);
            view.text_clipped(
                &item.title,
                view.scale.label,
                text_color,
                Rect::new(band.x, band.y, band.w, view.scale.label + CAPTION_GAP),
                Align::Start,
            );
            view.text_clipped(
                caption,
                view.scale.caption,
                view.ui.muted,
                Rect::new(
                    band.x,
                    band.y + view.scale.label + CAPTION_GAP,
                    band.w,
                    view.scale.caption,
                ),
                Align::Start,
            );
        }
        None => {
            view.text_clipped(
                &item.title,
                view.scale.label,
                text_color,
                center_band(label_rect, view.scale.label * 2.0),
                Align::Start,
            );
        }
    }
    if let Some(level) = item.level {
        meter(view, rect, label_rect, level, item.muted, state.selected);
    }
    indicator(view, rect, item, two_line, lang);
}

/// Дорожка полосы: от края названия до места значения справа. `None`, если
/// названию не хватило места — тогда полосы просто нет.
/// Сколько места справа от названия резервирует строка: шеврон, значение,
/// миниатюра пикера или полоса громкости с процентом.
pub(crate) fn trailing_w(kind: &ItemKind) -> f32 {
    match kind {
        ItemKind::Submenu | ItemKind::Leaf => CHEVRON_W,
        ItemKind::Picker(_) => THUMB_D + CHEVRON_W + spacing::SM,
        // У громкости справа полоса с процентом, а не только число: уровень
        // должен читаться взглядом, не выбирая строку и не глядя в подпись.
        ItemKind::Volume { .. } => METER_W + VOLUME_VALUE_W + spacing::SM,
        _ => CHEVRON_W + VALUE_W + spacing::SM,
    }
}

pub(crate) fn meter_track(label_rect: Rect, row_right: f32) -> Rect {
    let right = row_right - PAD - VOLUME_VALUE_W - spacing::SM;
    if right <= label_rect.right() + spacing::MD {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    // Высота здесь — вся строка: полосу центрируют уже по ней. Дорожка в
    // 6 px уводила заливку к верхнему краю строки, а не в её середину.
    Rect::new(
        label_rect.right() + spacing::MD,
        label_rect.y,
        right - label_rect.right() - spacing::MD,
        label_rect.h,
    )
}

/// Дорожка с заливкой по уровню. `None`, когда места нет совсем.
pub(crate) fn meter_rect(band: Rect, _level: u8) -> Option<Rect> {
    if band.w <= 0.0 {
        return None;
    }
    Some(Rect::new(
        band.x,
        band.y + (band.h - METER_H) / 2.0,
        band.w,
        METER_H,
    ))
}

/// Ширина заливки по уровню в процентах.
pub(crate) fn meter_fill(track: Rect, level: u8) -> f32 {
    track.w * f32::from(level.min(100)) / 100.0
}

/// Полоса громкости: заполнение по уровню, у заглушенного устройства вместо
/// заливки — приглушённая. Сама полоса рисуется всегда: иначе строка без
/// выбора не сказала бы, где сейчас звук.
fn meter(
    view: &mut MenuView<'_>,
    rect: Rect,
    label_rect: Rect,
    level: u8,
    muted: bool,
    selected: bool,
) {
    let band = center_band(
        meter_track(label_rect, rect.right()),
        view.scale.label * 2.0,
    );
    let Some(track_rect) = meter_rect(band, level) else {
        return;
    };
    fill_round_rect(
        view.pixmap,
        track_rect.x * SCALE,
        track_rect.y * SCALE,
        track_rect.w * SCALE,
        track_rect.h * SCALE,
        METER_H * SCALE / 2.0,
        view.ui.border_subtle,
    );
    let filled = meter_fill(track_rect, level);
    if filled > 0.5 {
        // Заливка и на невыбранной строке берёт `muted`, а не `border`:
        // `border` — это линия рамки, и на 1.96:1 к карточке заливка читалась
        // как «звука нет» вместо «уровень такой». Мутит звук — цвет тот же,
        // состояние говорит подпись, чтобы уровень не терялся вместе с цветом.
        let color = if muted || selected {
            view.ui.accent
        } else {
            view.ui.muted
        };
        fill_round_rect(
            view.pixmap,
            track_rect.x * SCALE,
            track_rect.y * SCALE,
            filled * SCALE,
            track_rect.h * SCALE,
            METER_H * SCALE / 2.0,
            color,
        );
    }
}

/// Правая часть строки: круг тумблера, круглая миниатюра пикера, значение или
/// шеврон подменю. Всё центрируется по той же оси, что и название.
fn indicator(view: &mut MenuView<'_>, rect: Rect, item: &Item, two_line: bool, lang: Language) {
    let right = rect.right() - PAD;
    if item.kind.is_toggle() {
        let dot = center_band(
            Rect::new(right - TOGGLE_D, rect.y, TOGGLE_D, rect.h),
            TOGGLE_D,
        );
        if item.value == strings::chrome(lang).on {
            view.disc(dot, view.ui.accent);
        } else {
            // Кольцо выключенного тумблера — тоже носитель состояния, а не
            // отделка, и обязано отделяться от карточки. Смесь
            // `mix(base, border, 0.6)` давала 1.32–1.59:1.
            ring(view, dot, view.ui.muted);
        }
        return;
    }
    if item.kind.is_picker() {
        // Заглушка миниатюры: настоящая картинка придёт в M4, когда будет
        // источник превью. Форма уже та же — круг 28 px с hairline, и шеврон
        // справа остаётся: пункт открывает выбор.
        let thumb = center_band(
            Rect::new(right - CHEVRON_W - THUMB_D, rect.y, THUMB_D, rect.h),
            THUMB_D,
        );
        view.disc(thumb, view.ui.picker_thumb);
        stroke_rect(
            view.pixmap,
            thumb.x * SCALE,
            thumb.y * SCALE,
            thumb.w * SCALE,
            thumb.h * SCALE,
            view.ui.border_subtle,
            SCALE,
        );
        view.glyph(
            settings_icons::CHEVRON,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(right - CHEVRON_W, rect.y, CHEVRON_W, rect.h),
                row_band(view),
            ),
        );
        return;
    }
    let chevron = !item.value.is_empty() || item.kind.is_submenu();
    if chevron {
        view.glyph(
            settings_icons::CHEVRON,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(right - CHEVRON_W, rect.y, CHEVRON_W, rect.h),
                row_band(view),
            ),
        );
    }
    if !item.value.is_empty() {
        let value = Rect::new(
            right - CHEVRON_W - VALUE_W - spacing::SM,
            rect.y,
            VALUE_W,
            rect.h,
        );
        let band = if two_line {
            // В двухстрочной строке значение встаёт по центру блока «название +
            // путь», а не по центру самой строки.
            center_band(value, view.scale.label + view.scale.caption + CAPTION_GAP)
        } else {
            center_band(value, view.scale.caption * 2.0)
        };
        // Значение - вторичная роль, поэтому кеглем подписи, а не названия: иначе
        // «Обычная» и «Тема» читались одного размера, и глаз не выбирал, где
        // название, а где его состояние.
        view.text_clipped(
            &item.value,
            view.scale.caption,
            view.ui.muted,
            band,
            Align::End,
        );
    }
}
