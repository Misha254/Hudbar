//! Токены окна настроек: цвета, радиусы, шрифты, отступы.
//!
//! До этого файла значения жили в вызовах `fill_round_rect` и `painter.paint`
//! числами, а палитра — в общем слое настроек. Здесь один источник правды:
//! компонент берёт токен по имени, а не вспоминает число. Токены описывают
//! текущий вид окна, поэтому их подстановка ничего не меняет на экране —
//! это чистая подготовка к дизайн-системе.
//!
//! Новые роли палитры (`surface`, `surface_hover`, `border_focus`, …) пока не
//! используются отрисовкой: они описаны здесь, чтобы следующие шаги не
//! придумывали цвета заново.

use super::palette;

// ─────────────────────────── Spacing ───────────────────────────

/// Отступы окна. Шаг 4 — базовый, всё остальное кратно ему.
pub mod spacing {
    /// 4 px: зазор внутри элемента, между глифом и рамкой.
    pub const XS: f32 = 4.0;
    /// 8 px: промежуток между строками списка.
    pub const SM: f32 = 8.0;
    /// 12 px: промежуток между секциями страницы.
    pub const MD: f32 = 12.0;
    /// 16 px: внутренний отступ карточки.
    pub const LG: f32 = 16.0;
    /// 24 px: отступ контента от края окна.
    pub const XL: f32 = 24.0;
    /// 32 px: высота строки и кнопки.
    pub const XXL: f32 = 32.0;
}

// ─────────────────────────── Radius ────────────────────────────

/// Радиусы скругления в логических пикселях. У Pixel все значения нулевые:
/// см. `radius`.
pub mod radii {
    /// 4 px: квадрат выбора темы внутри карточки.
    pub const XS: f32 = 4.0;
    /// 6 px: кнопки, строки списков, строки хоткеев.
    pub const SM: f32 = 6.0;
    /// 7 px: сайдбар и ручка тумблера в строке модуля (через `always`).
    pub const RAIL: f32 = 7.0;
    /// 8 px: сводка, степперы, ручка и подложка тумблера.
    pub const MD: f32 = 8.0;
    /// 10 px: карточка темы и трек тумблера.
    pub const LG: f32 = 10.0;
    /// 9 px: трек тумблера в строке модуля — половина его высоты.
    pub const TRACK: f32 = 9.0;
    /// 7 px: ручка того же тумблера — тоже половина высоты.
    pub const KNOB: f32 = 7.0;
}

/// Радиус под тему: у Pixel углы прямые, у обычной темы — скруглённые.
/// Наружу отдаётся в логических пикселях; умножать на `SCALE` должен caller.
pub fn radius(pixel: bool, token: f32) -> f32 {
    if pixel { 0.0 } else { token }
}

/// Радиус «таблетки»: сайдбар, трек и ручка тумблера в строке модуля.
/// Эти элементы скруглены в обеих темах — так было в 0.3.0, и токен фиксирует
/// именно это, а не меняет вид. Для всего остального — `radius`.
pub fn always(token: f32) -> f32 {
    token
}

// ───────────────────────── Typography ──────────────────────────

/// Шрифтовая шкала по ролям, а не по месту использования: одна роль может
/// стоять в шапке, в карточке и в подвале.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeScale {
    /// Заголовок окна и страницы.
    pub page_title: f32,
    /// Заголовок секции внутри страницы.
    pub section_title: f32,
    /// Основная подпись строки и значение контрола.
    pub label: f32,
    /// Второстепенный текст: подписи, подвал, «выбран».
    pub caption: f32,
    /// Крупное число: высота панели, шаг, версия.
    pub value: f32,
}

/// Размеры шрифта по темам. Обе шкалы кратны друг другу, чтобы строки
/// модуля, карточки и подвал читались одной системой.
pub fn type_scale(pixel: bool) -> TypeScale {
    if pixel {
        TypeScale {
            page_title: 19.0,
            section_title: 14.0,
            label: 13.0,
            caption: 11.5,
            value: 13.5,
        }
    } else {
        TypeScale {
            page_title: 20.0,
            section_title: 15.0,
            label: 14.0,
            caption: 12.0,
            value: 15.0,
        }
    }
}

// ─────────────────────────── Layout ────────────────────────────

/// Максимальная ширина контента. Пока равна фактической ширине полосы
/// (окно 1100 минус сайдбар и поля); следующий шаг сужает её до ~760 px по
/// макету, и тогда токен начнёт рулить раскладкой сам.
pub const CONTENT_MAX_W: f32 = 796.0;
/// Отступ контента от правого края окна.
pub const PAGE_PAD: f32 = 24.0;
/// Промежуток между секциями страницы.
pub const SECTION_GAP: f32 = 12.0;
/// Внутренний отступ карточки.
pub const CARD_PAD: f32 = 16.0;
/// Высота строки и кнопки.
pub const ROW_H: f32 = 32.0;
/// Ширина сайдбара.
pub const RAIL_W: f32 = 236.0;
/// Зазор между сайдбаром и контентом.
pub const RAIL_GAP: f32 = 26.0;

// ─────────────────────────── Palette ───────────────────────────

/// Фон окна непрозрачный: сквозь настройки не должен просвечивать терминал.
pub const BG_ALPHA: f32 = 1.0;

/// Смешивание двух цветов. Единственный способ получить новый цвет палитры:
/// так тема продолжает следовать за matugen, а не зашивает hex.
pub fn mix(a: palette::Rgba, b: palette::Rgba, t: f32) -> palette::Rgba {
    palette::Rgba(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
        255,
    )
}

/// Цвета окна. Первые семь — как есть, остальные выведены из них: одна
/// палитра обслуживает и обычную тему, и Pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiPalette {
    // Базовые роли.
    /// Фон окна.
    pub base: palette::Rgba,
    /// Заполненная поверхность: карточка, кнопка, выбранная строка.
    pub panel: palette::Rgba,
    /// Пустая поверхность: невыбранная строка, неактивная кнопка.
    pub idle_panel: palette::Rgba,
    /// Основная рамка.
    pub border: palette::Rgba,
    /// Акцент: фокус, выделение, активное состояние.
    pub accent: palette::Rgba,
    /// Основной текст.
    pub text: palette::Rgba,
    /// Второстепенный текст.
    pub muted: palette::Rgba,

    // Производные роли. Не используются отрисовкой, но заданы здесь, чтобы
    // компоненты дизайн-системы не изобретали цвета заново.
    /// Поверхность карточки/секции.
    pub surface: palette::Rgba,
    /// Поверхность под курсором или фокусом.
    pub surface_hover: palette::Rgba,
    /// Едва заметная рамка: разделители, границы групп.
    pub border_subtle: palette::Rgba,
    /// Рамка фокуса.
    pub border_focus: palette::Rgba,
    /// Приглушённый акцент: фон значка, неакцентная метка.
    pub accent_dim: palette::Rgba,
    /// Текст недоступного контрола.
    pub text_disabled: palette::Rgba,
}

/// Производные цвета из семи базовых. Считается одинаково для обеих тем,
/// поэтому Pixel получает те же роли из своих hex, а не из новых констант.
fn extend(base: UiPalette) -> UiPalette {
    UiPalette {
        surface: base.panel,
        surface_hover: mix(base.base, base.panel, 0.45),
        border_subtle: mix(base.base, base.border, 0.55),
        border_focus: base.accent.with_a(0.55),
        accent_dim: base.accent.with_a(0.45),
        text_disabled: base.muted.with_a(0.6),
        ..base
    }
}

/// Палитра окна: обычная тема берёт цвета из matugen, Pixel — свои.
pub fn ui_palette(pixel: bool, p: palette::Palette) -> UiPalette {
    let base = if pixel {
        UiPalette {
            base: palette::Rgba(0x0b, 0x10, 0x20, 255).with_a(BG_ALPHA),
            panel: palette::Rgba(0x11, 0x1b, 0x31, 255).with_a(BG_ALPHA),
            idle_panel: palette::Rgba(0x0e, 0x17, 0x2b, 255).with_a(BG_ALPHA),
            border: palette::Rgba(0x31, 0x5b, 0x9b, 255),
            accent: palette::Rgba(0x79, 0xa7, 0xff, 255),
            text: palette::Rgba(0xe8, 0xea, 0xff, 255),
            muted: palette::Rgba(0x8d, 0x9a, 0xbd, 255),
            surface: palette::Rgba(0, 0, 0, 0),
            surface_hover: palette::Rgba(0, 0, 0, 0),
            border_subtle: palette::Rgba(0, 0, 0, 0),
            border_focus: palette::Rgba(0, 0, 0, 0),
            accent_dim: palette::Rgba(0, 0, 0, 0),
            text_disabled: palette::Rgba(0, 0, 0, 0),
        }
    } else {
        UiPalette {
            base: p.base.with_a(BG_ALPHA),
            panel: mix(p.base, p.secondary, 0.10).with_a(BG_ALPHA),
            idle_panel: mix(p.base, p.secondary, 0.05).with_a(BG_ALPHA),
            border: mix(p.base, p.primary, 0.35),
            accent: p.primary,
            text: p.text,
            muted: p.text.with_a(0.58),
            surface: palette::Rgba(0, 0, 0, 0),
            surface_hover: palette::Rgba(0, 0, 0, 0),
            border_subtle: palette::Rgba(0, 0, 0, 0),
            border_focus: palette::Rgba(0, 0, 0, 0),
            accent_dim: palette::Rgba(0, 0, 0, 0),
            text_disabled: palette::Rgba(0, 0, 0, 0),
        }
    };
    extend(base)
}

#[cfg(test)]
mod tests {
    use super::super::settings_ui;
    use super::*;

    /// Токены обязаны совпадать с геометрией окна, иначе появится второй
    /// источник правды и раскладка разъедется.
    #[test]
    fn layout_tokens_match_the_window_geometry() {
        assert_eq!(
            CONTENT_MAX_W,
            settings_ui::WIDTH - settings_ui::PAD_X - settings_ui::PAD_R
        );
        assert_eq!(PAGE_PAD, settings_ui::PAD_R);
        assert_eq!(ROW_H, settings_ui::ROW_H);
        assert_eq!(CARD_PAD, settings_ui::CARD_PAD);
        assert_eq!(SECTION_GAP, settings_ui::ZONE_GAP);
        assert_eq!(RAIL_W, settings_ui::RAIL_W);
        assert_eq!(
            RAIL_GAP,
            settings_ui::PAD_X - settings_ui::RAIL_X - settings_ui::RAIL_W
        );
    }

    /// Отступы кратны базовым четырём: иначе сетка разъездится.
    #[test]
    fn spacing_is_a_four_pixel_grid() {
        for token in [
            spacing::XS,
            spacing::SM,
            spacing::MD,
            spacing::LG,
            spacing::XL,
            spacing::XXL,
        ] {
            assert_eq!(token % spacing::XS, 0.0, "отступ {token} не кратен 4");
        }
    }

    /// «Таблетки» остаются скруглёнными в обеих темах: так выглядел HUDbar
    /// 0.3.0, и токен не должен был это менять.
    #[test]
    fn pill_shapes_stay_rounded_in_both_themes() {
        for token in [radii::RAIL, radii::TRACK, radii::KNOB] {
            assert_eq!(always(token), token);
            assert_eq!(radius(true, token), 0.0, "таблетка должна быть круглой");
        }
    }

    /// Pixel обязан быть полностью прямым: любой радиус в нём — ноль.
    #[test]
    fn pixel_theme_has_no_rounding() {
        for token in [radii::XS, radii::SM, radii::RAIL, radii::MD, radii::LG] {
            assert!(token > 0.0, "радиус {token} должен быть осмысленным");
            assert_eq!(radius(true, token), 0.0, "у Pixel углы прямые");
            assert_eq!(radius(false, token), token);
        }
    }

    /// Обе темы используют одну шкалу ролей, и в обычной теме текст крупнее.
    #[test]
    fn both_themes_share_the_same_type_roles() {
        let normal = type_scale(false);
        let pixel = type_scale(true);
        assert!(normal.page_title > pixel.page_title);
        assert!(normal.section_title > pixel.section_title);
        assert!(normal.label > pixel.label);
        assert!(normal.caption > pixel.caption);
        assert!(normal.value > pixel.value);
        assert!(normal.page_title > normal.section_title);
        assert!(normal.section_title > normal.label);
        assert!(normal.value > normal.caption);
    }

    /// Производные цвета считаются из базовых и остаются непрозрачными там,
    /// где базовые непрозрачны: иначе проявится терминал под окном.
    #[test]
    fn derived_palette_colors_come_from_the_base_ones() {
        for pixel in [false, true] {
            let p = ui_palette(pixel, palette::Palette::default());
            assert_eq!(p.surface, p.panel, "surface должен быть panel");
            assert_ne!(
                p.surface_hover, p.surface,
                "surface_hover обязан отличаться"
            );
            assert_ne!(p.border_subtle, p.border, "border_subtle обязан отличаться");
            assert_eq!(
                p.border_focus.0, p.accent.0,
                "border_focus берёт цвет акцента"
            );
            assert_eq!(p.accent_dim.0, p.accent.0);
            assert!(p.accent_dim.3 < p.accent.3, "accent_dim приглушён по альфе");
            assert_eq!(p.text_disabled.0, p.muted.0);
            assert!(
                p.text_disabled.3 < p.muted.3,
                "text_disabled приглушён по альфе"
            );
            // Окно непрозрачное: базовые цвета обязаны быть плотными.
            assert_eq!(p.base.3, 255);
            assert_eq!(p.panel.3, 255);
        }
    }
}
