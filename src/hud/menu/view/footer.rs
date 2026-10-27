use super::super::canvas::fill_rect;
use super::super::geom::Rect;
use super::super::settings::Language;
use super::super::state::Frame;
use super::super::strings;
use super::super::text::{Align, SCALE};
use super::super::ui_tokens::{mix, spacing};
use super::MenuView;
use super::geometry::{FOOTER_H, PAD, STATUS_DOT, center_band};

/// Подвал: подсказки слева, счётчик или статус справа.
pub(crate) fn footer(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
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
    let band = center_band(rect, view.scale.caption * 2.0);
    // Правая зона резервируется по содержимому, а не константой: подсказки не
    // должны прыгать в момент, когда вместо счётчика появляется статус.
    let right_zone = right_zone_width(view, frame.lang);
    // Между подсказками и правой частью — обязательный зазор XL. Если полные
    // подсказки не влезают, берутся средние, потом одни клавиши — но не наезд
    // на счётчик.
    let hints_left = rect.x + PAD;
    let right_edge = rect.right() - PAD - right_zone;
    let (hints, mut x) = layout_hints(view, hints_left, right_edge - spacing::XL, frame.lang);
    for hint in hints {
        let width = view.painter.text_width(hint, view.scale.caption);
        view.text_clipped(
            hint,
            view.scale.caption,
            view.ui.muted,
            Rect::new(x, band.y, width, band.h),
            Align::Start,
        );
        x += width + spacing::LG;
    }
    let right = Rect::new(right_edge, rect.y, right_zone, rect.h);
    match frame.status.as_ref() {
        Some(status) => {
            // В режиме статуса счётчика нет: точка сообщает, что сообщение
            // именно об изменении, а не о позиции в списке.
            let color = if status.is_failed() {
                mix(view.ui.accent, view.ui.muted, 0.5)
            } else {
                view.ui.accent
            };
            view.disc(
                center_band(Rect::new(right.x, rect.y, STATUS_DOT, rect.h), STATUS_DOT),
                color,
            );
            let label = if status.is_failed() {
                strings::chrome(frame.lang).failed
            } else {
                strings::chrome(frame.lang).applied
            };
            view.text_clipped(
                label,
                view.scale.caption,
                view.ui.muted,
                center_band(
                    Rect::new(
                        right.x + STATUS_DOT + spacing::XS,
                        rect.y,
                        right.w - STATUS_DOT - spacing::XS,
                        rect.h,
                    ),
                    view.scale.caption * 2.0,
                ),
                Align::Start,
            );
        }
        None => {
            // Счётчик считает выбираемые элементы, а не физические строки:
            // Header и Info не являются пунктами выбора. На пустом списке
            // «0/0», при отсутствии выбора — «—/N».
            let selectable_total = frame
                .items
                .iter()
                .filter(|item| item.kind.is_selectable())
                .count();
            let counter = match frame.selected {
                Some(index) if index < frame.items.len() => {
                    let ordinal = frame.items[..=index]
                        .iter()
                        .filter(|item| item.kind.is_selectable())
                        .count();
                    format!("{ordinal}/{selectable_total}")
                }
                _ if selectable_total == 0 => "0/0".to_string(),
                _ => format!("—/{selectable_total}"),
            };
            view.text_clipped(
                &counter,
                view.scale.caption,
                view.ui.muted,
                center_band(right, view.scale.caption * 2.0),
                Align::End,
            );
        }
    }
}

/// Ширина строки подсказок вместе с зазорами между ними.
pub(crate) fn hints_width(view: &mut MenuView<'_>, hints: &[&str]) -> f32 {
    let mut total = 0.0;
    for (index, hint) in hints.iter().enumerate() {
        total += view.painter.text_width(hint, view.scale.caption);
        if index + 1 < hints.len() {
            total += spacing::LG;
        }
    }
    total
}

/// Самая широкая строка счётчика. Список длиннее сотни строк в меню не
/// бывает, но резерв обязан быть отвязан от того, что показано сейчас, иначе
/// подсказки прыгали бы в момент появления статуса.
pub(crate) const COUNTER_SAMPLES: [&str; 2] = ["0/0", "100/100"];

/// Ключ кэша резерва подвала: язык и кегль подписи.
#[derive(Clone, Copy, PartialEq)]
struct ZoneKey {
    lang: Language,
    caption: f32,
}

thread_local! {
    /// Последний измеренный резерв. Один кадр, один язык и тема — мерить
    /// каждый кадр незачем: четыре строки `text_width` стоили 0.8 мс на карточке.
    static ZONE: std::cell::Cell<Option<(ZoneKey, f32)>> = const { std::cell::Cell::new(None) };
    /// Сколько раз резерв мерили по-настоящему. Только для теста.
    static ZONE_MISSES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Сколько раз резерв правой зоны измерялся по-настоящему.
#[cfg(test)]
pub fn zone_misses() -> u32 {
    ZONE_MISSES.with(|cell| cell.get())
}

/// Резерв правой зоны подвала.
///
/// Берётся максимум из двух вещей: самая широкая строка счётчика и самый
/// широкий статус на языке окна вместе с точкой. Считается всегда, независимо
/// от того, что нарисовано сейчас: иначе левая часть подвала сдвигалась бы
/// в момент, когда вместо счётчика появляется статус, и обратно.
///
/// Резерв зависит только от языка и кегля подписи, поэтому измеряется один
/// раз на эту пару и дальше берётся из кэша.
pub fn right_zone_width(view: &mut MenuView<'_>, lang: Language) -> f32 {
    let key = ZoneKey {
        lang,
        caption: view.scale.caption,
    };
    if let Some((cached, width)) = ZONE.with(|cell| cell.get())
        && cached == key
    {
        return width;
    }
    ZONE_MISSES.with(|cell| cell.set(cell.get() + 1));
    let chrome = strings::chrome(lang);
    let size = view.scale.caption;
    let counter = COUNTER_SAMPLES
        .iter()
        .map(|text| view.painter.text_width(text, size))
        .fold(0.0_f32, f32::max);
    let status = [chrome.applied, chrome.failed]
        .iter()
        .map(|text| view.painter.text_width(text, size))
        .fold(0.0_f32, f32::max);
    let status_block = STATUS_DOT + spacing::XS + status;
    let width = counter.max(status_block);
    ZONE.with(|cell| cell.set(Some((key, width))));
    width
}

/// Раскладка подсказок подвала: возвращает набор строк и их левую границу.
///
/// Уровней три, и берётся первый влезший: полные подписи, средние (ключ и
/// короткое слово) и наконец одни клавиши. Порядок не зависит от того, есть
/// ли сейчас статус, — ширину правой зоны считает `right_zone_width`.
pub fn layout_hints<'a>(
    view: &mut MenuView<'_>,
    left: f32,
    limit: f32,
    lang: Language,
) -> (Vec<&'a str>, f32) {
    let chrome = strings::chrome(lang);
    let levels: [[&str; 4]; 3] = [
        [chrome.select, chrome.open, chrome.back, chrome.close],
        [
            chrome.select_medium,
            chrome.open_medium,
            chrome.back_medium,
            chrome.close_medium,
        ],
        [
            chrome.select_short,
            chrome.open_short,
            chrome.back_short,
            chrome.close_short,
        ],
    ];
    for level in levels {
        if left + hints_width(view, &level) <= limit {
            return (level.to_vec(), left);
        }
    }
    (levels[2].to_vec(), left)
}
