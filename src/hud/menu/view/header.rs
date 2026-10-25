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

/// Роль сегмента крошек. Цвет выбирает шапка: план не знает палитры.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrumbRole {
    /// Предки и разделители.
    Muted,
    /// Последний уровень в цепочке.
    Accent,
    /// Единственный сегмент. Рисуется основным цветом, как корень до крошек:
    /// так снимки `root`, `search` и `empty` не меняются побитово.
    Solo,
}

/// Один текст плана и его ширина.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CrumbPart {
    pub text: String,
    pub width: f32,
    pub role: CrumbRole,
}

/// Готовая раскладка крошек: что рисовать, в каком порядке и с каким цветом.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CrumbPlan {
    pub parts: Vec<CrumbPart>,
    pub width: f32,
}

/// Многоточие в начале отброшенных предков и в конце обрезанного последнего.
const ELLIPSIS: &str = "…";

/// Зазор между приглушённым префиксом и акцентным последним сегментом. Тот
/// же, что был в старой шапке, поэтому `style.png` остаётся побитово прежним.
const LAST_GAP: f32 = spacing::XS;

/// Раскладывает крошки в доступной ширине без painter: измерение приходит
/// замыканием. Правила:
///
/// * между сегментами стоит `strings::CRUMBS_SEPARATOR`, предки — `Muted`;
/// * последний сегмент — `Accent` и не исчезает никогда;
/// * если места мало, самые старые сегменты слева отбрасываются и
///   заменяются одним `…` в начале;
/// * если даже `… › последний` не влезает, ведущее многоточие не тратится:
///   показывается один последний, обрезанный `…` на конце;
/// * пустой trail или отрицательная ширина — пустой план.
pub(crate) fn fit_crumbs(
    trail: &[&str],
    avail: f32,
    mut text_width: impl FnMut(&str) -> f32,
) -> CrumbPlan {
    if trail.is_empty() || avail < 0.0 {
        return CrumbPlan::default();
    }
    let last = trail[trail.len() - 1];
    if trail.len() == 1 {
        let part = fit_last(last, avail, &mut text_width, CrumbRole::Solo);
        let width = part.width;
        return CrumbPlan {
            parts: vec![part],
            width,
        };
    }

    let separator = strings::CRUMBS_SEPARATOR;
    let ancestors = &trail[..trail.len() - 1];
    let full_prefix = format!("{}{separator}", ancestors.join(separator));
    let full_width = text_width(&full_prefix) + LAST_GAP + text_width(last);
    if full_width <= avail {
        return plan_with_prefix(full_prefix, last, &mut text_width, CrumbRole::Accent);
    }

    // Самый длинный хвост предков, который ещё влезает после ведущего «…».
    for keep in (0..ancestors.len()).rev() {
        let tail = ancestors[ancestors.len() - keep..].join(separator);
        let prefix = if tail.is_empty() {
            format!("{ELLIPSIS}{separator}")
        } else {
            format!("{ELLIPSIS}{separator}{tail}{separator}")
        };
        if text_width(&prefix) + LAST_GAP + text_width(last) <= avail {
            return plan_with_prefix(prefix, last, &mut text_width, CrumbRole::Accent);
        }
    }

    // Даже «… › последний» не помещается: последний важнее многоточия.
    let part = fit_last(last, avail, &mut text_width, CrumbRole::Accent);
    let width = part.width;
    CrumbPlan {
        parts: vec![part],
        width,
    }
}

/// План из приглушённого префикса с разделителями и акцентного последнего.
fn plan_with_prefix(
    prefix: String,
    last: &str,
    text_width: &mut impl FnMut(&str) -> f32,
    role: CrumbRole,
) -> CrumbPlan {
    let prefix_width = text_width(&prefix);
    let last_width = text_width(last);
    let width = prefix_width + LAST_GAP + last_width;
    CrumbPlan {
        parts: vec![
            CrumbPart {
                text: prefix,
                width: prefix_width,
                role: CrumbRole::Muted,
            },
            CrumbPart {
                text: last.to_string(),
                width: last_width,
                role,
            },
        ],
        width,
    }
}

/// Последний сегмент целиком или обрезанный с `…` на конце. Даже если не
/// помещается само многоточие, возвращается хотя бы оно: последний уровень
/// не должен исчезать.
fn fit_last(
    text: &str,
    avail: f32,
    text_width: &mut impl FnMut(&str) -> f32,
    role: CrumbRole,
) -> CrumbPart {
    let full_width = text_width(text);
    if full_width <= avail {
        return CrumbPart {
            text: text.to_string(),
            width: full_width,
            role,
        };
    }
    let mut shown = ELLIPSIS.to_string();
    for (index, ch) in text.char_indices() {
        let end = index + ch.len_utf8();
        let mut candidate = text[..end].trim_end().to_string();
        candidate.push_str(ELLIPSIS);
        if text_width(&candidate) <= avail {
            shown = candidate;
        } else {
            break;
        }
    }
    let width = text_width(&shown);
    CrumbPart {
        text: shown,
        width,
        role,
    }
}

/// Каждая текстовая часть крошек получает одну и ту же базовую линию.
/// Отдельный iterator-helper используется и при отрисовке, и в тесте
/// геометрии, чтобы случайно не вернуть центрирование по строкам.
fn crumb_baselines(plan: &CrumbPlan, baseline: f32) -> impl Iterator<Item = (&CrumbPart, f32)> {
    plan.parts.iter().map(move |part| (part, baseline))
}

/// Крошки «HUD › Стиль» слева и поле поиска справа. Поле не мигает: рамка в
/// фокусе всегда видна, потому что меню работает только с клавиатуры.
pub(crate) fn header(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let header_rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    if !frame.trail.is_empty() {
        let crumbs = crumbs_rect(header_rect);
        // Крошки — структурная метка «где я сейчас», поэтому ролью заголовка
        // секции: кеглем строки списка шапка от строки не отличалась ничем,
        // кроме цвета, а цвет при недоступной палитре не различить.
        let title = view.scale.section_title;
        let trail = frame.trail.iter().map(String::as_str).collect::<Vec<_>>();
        let plan = fit_crumbs(&trail, crumbs.w, |text| {
            view.painter.text_width(text, title)
        });
        let band = center_band(crumbs, title * 2.0);
        // Общая линейка письма учитывает кириллические и латинские верхние и
        // нижние выносы; все части крошек используют один baseline.
        let baseline = view.painter.shared_baseline(band, title);
        let mut x = crumbs.x;
        for (index, (part, baseline)) in crumb_baselines(&plan, baseline).enumerate() {
            let color = match part.role {
                CrumbRole::Muted => view.ui.muted,
                CrumbRole::Accent => view.ui.accent,
                CrumbRole::Solo => view.ui.text,
            };
            view.painter.paint_on_baseline(
                view.pixmap,
                &part.text,
                title,
                color,
                Rect::new(x, band.y, part.width, band.h),
                Align::Start,
                baseline,
            );
            x += part.width;
            if index + 1 < plan.parts.len() {
                x += LAST_GAP;
            }
        }
        debug_assert!(
            (x - crumbs.x - plan.width).abs() < 0.01,
            "ширина плана и сумма частей разошлись"
        );
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
        radius(radii::SM) * SCALE,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Детерминированная линейка: один символ — одна единица. Настоящий
    /// painter здесь не нужен, `fit_crumbs` не знает про пиксели.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32
    }

    #[test]
    fn all_crumbs_fit_with_separator_and_roles() {
        let plan = fit_crumbs(&["HUD", "Стиль"], 100.0, width);
        assert_eq!(plan.parts.len(), 2);
        assert_eq!(
            plan.parts[0].text,
            format!("HUD{}", strings::CRUMBS_SEPARATOR)
        );
        assert_eq!(plan.parts[0].role, CrumbRole::Muted);
        assert_eq!(plan.parts[1].text, "Стиль");
        assert_eq!(plan.parts[1].role, CrumbRole::Accent);
        assert!(plan.width <= 100.0);
    }

    #[test]
    fn oldest_crumbs_are_replaced_by_one_ellipsis() {
        let trail = ["HUD", "Система", "Устройства", "Дисплеи", "eDP-1"];
        let plan = fit_crumbs(&trail, 24.0, width);
        assert_eq!(plan.parts.len(), 2);
        assert_eq!(plan.parts[0].text, "… › Дисплеи › ");
        assert!(!plan.parts[0].text.contains("HUD"));
        assert!(!plan.parts[0].text.contains("Система"));
        assert_eq!(plan.parts[1].text, "eDP-1");
        assert!(plan.width <= 24.0);
    }

    #[test]
    fn last_crumb_never_disappears() {
        let trail = ["HUD", "Очень длинное имя вывода"];
        let plan = fit_crumbs(&trail, 10.0, width);
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].role, CrumbRole::Accent);
        assert!(plan.parts[0].text.starts_with("Очень"));
        assert!(plan.parts[0].text.ends_with(ELLIPSIS));
    }

    #[test]
    fn single_crumb_keeps_the_root_colour() {
        let plan = fit_crumbs(&["HUD"], 100.0, width);
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].text, "HUD");
        assert_eq!(plan.parts[0].role, CrumbRole::Solo);
        assert_eq!(plan.width, 3.0);
    }

    #[test]
    fn empty_trail_and_negative_width_make_an_empty_plan() {
        let empty = fit_crumbs(&[], 100.0, width);
        assert!(empty.parts.is_empty());
        assert_eq!(empty.width, 0.0);
        let negative = fit_crumbs(&["HUD", "Дисплеи"], -1.0, width);
        assert!(negative.parts.is_empty());
        assert_eq!(negative.width, 0.0);
    }

    #[test]
    fn very_narrow_width_leaves_at_least_an_ellipsis() {
        let plan = fit_crumbs(&["HUD", "Дисплеи"], 0.0, width);
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].text, ELLIPSIS);
        assert_eq!(plan.parts[0].role, CrumbRole::Accent);
    }

    #[test]
    fn leading_ellipsis_is_not_spent_when_only_last_fits() {
        let plan = fit_crumbs(&["HUD", "abcdefghij"], 12.0, width);
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].text, "abcdefghij");
        assert!(!plan.parts[0].text.starts_with(ELLIPSIS));
    }

    #[test]
    fn separator_does_not_depend_on_language() {
        let ru = fit_crumbs(&["HUD", "Стиль"], 100.0, width);
        let en = fit_crumbs(&["HUD", "Style"], 100.0, width);
        assert_eq!(ru.parts.len(), en.parts.len());
        assert_eq!(ru.parts[0].text, en.parts[0].text);
        assert_eq!(
            ru.parts[0].text,
            format!("HUD{}", strings::CRUMBS_SEPARATOR)
        );
        assert_eq!(ru.parts[0].role, en.parts[0].role);
        assert_eq!(ru.parts[1].role, en.parts[1].role);
    }

    #[test]
    fn separators_are_identical_and_have_no_extra_spaces_on_every_trail() {
        for trail in [
            vec!["HUD", "Стиль"],
            vec!["HUD", "Система", "Устройства", "Дисплеи", "eDP-1"],
        ] {
            let plan = fit_crumbs(&trail, 1_000.0, width);
            let prefix = &plan.parts[0].text;
            let expected = format!(
                "{}{}",
                trail[..trail.len() - 1].join(strings::CRUMBS_SEPARATOR),
                strings::CRUMBS_SEPARATOR
            );
            assert_eq!(prefix, &expected);
            assert!(!prefix.contains("  "), "лишний пробел в {prefix:?}");
            assert_eq!(
                prefix.matches(strings::CRUMBS_SEPARATOR).count(),
                trail.len() - 1,
                "разделитель должен стоять на каждом стыке"
            );
        }
    }

    #[test]
    fn all_crumb_segments_share_one_baseline() {
        use super::super::super::text::TextPainter;
        use super::super::super::ui_tokens::font_name;

        let trail = ["HUD", "Дисплеи", "eDP-1", "Стиль", "Система"];
        let plan = fit_crumbs(&trail, 1_000.0, width);
        let mut painter = TextPainter::new(&font_name());
        let band = Rect::new(0.0, 0.0, 228.0, 52.0);
        let baseline = painter.shared_baseline(band, 15.0);
        let assigned = crumb_baselines(&plan, baseline)
            .map(|(_, y)| y)
            .collect::<Vec<_>>();
        assert_eq!(assigned.len(), plan.parts.len());
        assert!(assigned.iter().all(|y| *y == baseline));

        // Каждый из встречающихся в меню алфавитных сегментов также получает
        // то же значение Y при раскладке как последний сегмент.
        for segment in ["HUD", "Дисплеи", "eDP-1", "Стиль", "Система"] {
            let single = fit_crumbs(&["HUD", segment], 1_000.0, width);
            let ys = crumb_baselines(&single, baseline)
                .map(|(_, y)| y)
                .collect::<Vec<_>>();
            assert!(ys.iter().all(|y| *y == baseline), "segment: {segment}");
        }
    }
}
