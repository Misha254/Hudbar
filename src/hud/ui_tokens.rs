//! Токены интерфейса: цвета, радиусы, шрифты, отступы.
//!
//! До этого файла значения жили в вызовах `fill_round_rect` и `painter.paint`
//! числами, а палитра — в общем слое настроек. Здесь один источник правды:
//! компонент берёт токен по имени, а не вспоминает число. Токены описывают
//! текущий вид окон, поэтому их подстановка ничего не меняет на экране —
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

/// Шрифт темы: у Pixel и обычной он разный. Им же пользуются снапшоты, чтобы
/// PNG и живое окно рисовались одним шрифтом.
pub fn font_name(pixel: bool) -> String {
    if pixel {
        "Minecraft Rus".to_string()
    } else {
        "JetBrainsMono Nerd Font Propo".to_string()
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
    /// Текст недоступного контрола и заглушка ввода.
    pub text_disabled: palette::Rgba,
    /// Заглушка миниатюры выбора: круг до появления настоящей картинки.
    pub picker_thumb: palette::Rgba,
    /// Подложка строки под указателем мыши.
    pub hover_bg: palette::Rgba,
    /// Фон выбранной строки.
    pub selected_bg: palette::Rgba,
    /// Плитка на месте иконки, которой нет. Отдельная роль: у неё свой порог
    /// (3:1 — это графика) и своя альфа. Приглушённый текст на карточке даёт
    /// 4.5:1 и на строке читался бы как значок, которого не существует.
    pub placeholder: palette::Rgba,
}

/// Доля акцента в фоне строки под указателем: вчетверо слабее выбранной.
pub const HOVER_MIX: f32 = 0.07;

/// Доля акцента в фоне выбранной строки. Малая: фон должен наметиться, а не
/// стать второй подсветкой. Значение снято с макета — там оно 0.15–0.17.
pub const SELECTED_MIX: f32 = 0.14;

/// Альфа плитки-заглушки на месте отсутствующей иконки.
///
/// Ровно столько, сколько нужно, чтобы плитка проходила 3:1 к карточке как
/// графика: мельче — и её не видно вовсе, крупнее — и она читается как значок,
/// которого нет. Проверка на всех модельных схемах живёт в тестах этого файла.
pub const PLACEHOLDER_ALPHA: f32 = 0.52;

/// Производные цвета из семи базовых. Считается одинаково для обеих тем,
/// поэтому Pixel получает те же роли из своих hex, а не из новых констант.
///
/// Каждый коэффициент и каждая альфа подобраны так, чтобы роль проходила
/// контраст WCAG на модельных палитрах схем matugen: текст — 4.5:1, графика,
/// иконки и маркеры — 3:1. Проверка живёт в тестах этого файла.
fn extend(base: UiPalette) -> UiPalette {
    UiPalette {
        surface: base.panel,
        surface_hover: mix(base.base, base.panel, 0.45),
        border_subtle: mix(base.base, base.border, 0.55),
        border_focus: base.accent.with_a(0.75),
        accent_dim: base.accent.with_a(0.45),
        // Заглушка приглушается смещением к фону поля, а не прозрачностью:
        // `with_a` поверх `muted` давал альфу 0.37 и контраст 3.08:1, то
        // есть ровно столько, сколько нужно, чтобы ничего не читалось.
        text_disabled: mix(base.idle_panel, base.text, 0.70),
        // Заглушка миниатюры выбора. Смещение к `base` на 0.45 давало 2.58:1
        // на светлых схемах — круг сливался с карточкой.
        picker_thumb: mix(base.accent, base.base, 0.35),
        // Подложка строки под указателем: та же смесь, что у выбранной строки,
        // но вчетверо слабее. Отдельная роль, чтобы её нельзя было спутать с
        // выделением выбором.
        hover_bg: mix(base.panel, base.accent, HOVER_MIX),
        selected_bg: mix(base.panel, base.accent, SELECTED_MIX),
        placeholder: base.text.with_a(PLACEHOLDER_ALPHA),
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
            // Рамка карточки — единственное, что отделяет карточку от
            // размытого фона, а не украшение. Старые 0x315b9b давали 2.53:1
            // к своей же карточке; 0x4d6aa5 даёт 3.20:1 и остаётся холодным.
            border: palette::Rgba(0x4d, 0x6a, 0xa5, 255),
            accent: palette::Rgba(0x79, 0xa7, 0xff, 255),
            text: palette::Rgba(0xe8, 0xea, 0xff, 255),
            muted: palette::Rgba(0x8d, 0x9a, 0xbd, 255),
            surface: palette::Rgba(0, 0, 0, 0),
            surface_hover: palette::Rgba(0, 0, 0, 0),
            border_subtle: palette::Rgba(0, 0, 0, 0),
            border_focus: palette::Rgba(0, 0, 0, 0),
            accent_dim: palette::Rgba(0, 0, 0, 0),
            text_disabled: palette::Rgba(0, 0, 0, 0),
            picker_thumb: palette::Rgba(0, 0, 0, 0),
            hover_bg: palette::Rgba(0, 0, 0, 0),
            selected_bg: palette::Rgba(0, 0, 0, 0),
            // В Pixel заглушка идёт ступенями и без полутонов, но альфа та же,
            // что и в обычной теме: проверка контраста одна на обе темы.
            placeholder: palette::Rgba(0xe8, 0xea, 0xff, 255).with_a(PLACEHOLDER_ALPHA),
        }
    } else {
        UiPalette {
            base: p.base.with_a(BG_ALPHA),
            panel: mix(p.base, p.secondary, 0.10).with_a(BG_ALPHA),
            idle_panel: mix(p.base, p.secondary, 0.05).with_a(BG_ALPHA),
            // Рамка карточки — не украшение, а граница слоя: у слоя ровно размер
            // карточки, тени нет, и отделяет её только эта линия. На 0.35 к
            // акценту она давала 1.98:1 и не отделяла совсем; 0.45 давало
            // 2.52:1, тоже ниже порога графики. 0.55 — 3.28:1 в худшей схеме,
            // и рамка при этом не читается второй подсветкой, как на 0.65.
            border: mix(p.base, p.primary, 0.55),
            accent: p.primary,
            text: p.text,
            // 0.63 — минимальная альфа, при которой `muted` проходит 4.5:1 с запасом:
            // худший случай по десяти схемам в обоих режимах — 4.81:1. На
            // 0.61 было 4.54, на 0.58 было 4.09.
            muted: p.text.with_a(0.63),
            surface: palette::Rgba(0, 0, 0, 0),
            surface_hover: palette::Rgba(0, 0, 0, 0),
            border_subtle: palette::Rgba(0, 0, 0, 0),
            border_focus: palette::Rgba(0, 0, 0, 0),
            accent_dim: palette::Rgba(0, 0, 0, 0),
            text_disabled: palette::Rgba(0, 0, 0, 0),
            picker_thumb: palette::Rgba(0, 0, 0, 0),
            hover_bg: palette::Rgba(0, 0, 0, 0),
            selected_bg: palette::Rgba(0, 0, 0, 0),
            placeholder: palette::Rgba(0, 0, 0, 0),
        }
    };
    extend(base)
}
/// Относительная яркость по WCAG 2.1.
fn relative_luminance(color: palette::Rgba) -> f32 {
    let channel = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

/// Полупрозрачный цвет на фоне: то, что глаз видит, а не то, что записано в
/// палитре.
fn flatten(front: palette::Rgba, back: palette::Rgba) -> palette::Rgba {
    let alpha = front.3 as f32 / 255.0;
    palette::Rgba(
        (front.0 as f32 * alpha + back.0 as f32 * (1.0 - alpha)) as u8,
        (front.1 as f32 * alpha + back.1 as f32 * (1.0 - alpha)) as u8,
        (front.2 as f32 * alpha + back.2 as f32 * (1.0 - alpha)) as u8,
        255,
    )
}

/// Контраст пары «что на чём нарисовано» по WCAG 2.1.
///
/// Функция публичная, потому что контраст считают не только тесты палитры: окну
/// лаунчера нужно проверить, что заглушка и подвал держат свои пороги на тех же
/// модельных схемах, а дублировать формулу WCAG в компоненте нельзя — через
/// год формулы разъедутся, и проверка станет проверкой копии.
pub fn contrast(front: palette::Rgba, back: palette::Rgba) -> f32 {
    let front = relative_luminance(flatten(front, back));
    let back = relative_luminance(back);
    let (high, low) = if front > back {
        (front, back)
    } else {
        (back, front)
    };
    (high + 0.05) / (low + 0.05)
}

/// Запас поверх порога: вторая роль, на которую смотрит тест.
const MUTED_MARGIN: f32 = 4.8;

#[cfg(test)]
mod tests {
    use super::*;

    /// Модельные палитры схем matugen: тона Material 3 по семействам схем.
    ///
    /// Реальный вывод matugen на машине не хранится в репозитории, а генерировать
    /// его здесь нельзя — команда переписывает `colors.css`. Поэтому палитры
    /// собраны из публичных тонов M3: тёмная тема берёт акцент тона 80 и
    /// поверхность тона 6–10, светлая — акцент тона 40 и поверхность тона 98.
    /// Схемы различаются chroma и тоном акцента, а производные роли считаются
    /// из них той же формулой, что и в живом окне.
    const MODEL_SCHEMES: [&str; 10] = [
        "tonal-spot",
        "expressive",
        "fidelity",
        "fruit-salad",
        "monochrome",
        "neutral",
        "rainbow",
        "content",
        "vibrant",
        "smart",
    ];

    struct ModelPalette {
        base: palette::Rgba,
        text: palette::Rgba,
        primary: palette::Rgba,
        secondary: palette::Rgba,
    }

    /// Палитра схемы в виде, из которого делается `UiPalette`. Отдельная
    /// функция: новые проверки ролей не должны копировать сборку.
    fn model_palette(scheme: &str, light: bool) -> palette::Palette {
        let model = model(scheme, light);
        palette::Palette {
            base: model.base,
            text: model.text,
            primary: model.primary,
            secondary: model.secondary,
            on_primary: palette::Rgba(0, 0, 0, 255),
            error: palette::Rgba(255, 0, 0, 255),
        }
    }

    /// Собрать палитру схемы: акцент и его chroma, поверхность и текст режима.
    fn model(scheme: &str, light: bool) -> ModelPalette {
        // Акцент схемы: (тон 80 или 40, chroma, поверхность).
        let (accent, chroma, surface): ([u8; 3], f32, [u8; 3]) = match scheme {
            "tonal-spot" => ([0x9d, 0xcb, 0xfb], 1.00, [0x10, 0x14, 0x18]),
            "expressive" => ([0xb6, 0xc8, 0xff], 1.00, [0x12, 0x12, 0x16]),
            "fidelity" => ([0xaf, 0xc6, 0xff], 0.95, [0x11, 0x13, 0x17]),
            "fruit-salad" => ([0xf5, 0xc0, 0x6b], 1.00, [0x14, 0x12, 0x0e]),
            "monochrome" => ([0xc8, 0xc9, 0xcc], 0.05, [0x11, 0x11, 0x12]),
            "neutral" => ([0xbd, 0xc2, 0xc9], 0.15, [0x11, 0x12, 0x13]),
            "rainbow" => ([0x8f, 0xd8, 0xc0], 1.00, [0x10, 0x16, 0x16]),
            "content" => ([0xa9, 0xc7, 0xf0], 0.70, [0x10, 0x13, 0x17]),
            "vibrant" => ([0xe3, 0xb6, 0xf2], 1.00, [0x14, 0x10, 0x18]),
            "smart" => ([0xa8, 0xc7, 0xfa], 0.85, [0x10, 0x14, 0x18]),
            other => unreachable!("схема {other} не описана"),
        };
        let (accent, surface): ([u8; 3], [u8; 3]) = if light {
            ([0x00, 0x31, 0x52], [0xfa, 0xfa, 0xff])
        } else {
            (accent, surface)
        };
        let rgb = |value: [u8; 3]| palette::Rgba(value[0], value[1], value[2], 255);
        let secondary = [
            (surface[0] as f32 + (accent[0] as f32 - surface[0] as f32) * 0.22 * chroma) as u8,
            (surface[1] as f32 + (accent[1] as f32 - surface[1] as f32) * 0.22 * chroma) as u8,
            (surface[2] as f32 + (accent[2] as f32 - surface[2] as f32) * 0.22 * chroma) as u8,
        ];
        ModelPalette {
            base: rgb(surface),
            text: rgb(if light {
                [0x1a, 0x1b, 0x20]
            } else {
                [0xe3, 0xe2, 0xe9]
            }),
            primary: rgb(accent),
            secondary: rgb(secondary),
        }
    }

    /// Пары «что на чём нарисовано», которые обязан читать человек.
    ///
    /// Пороги: 4.5:1 на мелкий текст, 3:1 на графику, иконки и маркеры. Список
    /// один на обе темы и на все палитры — иначе проверка была бы списком
    /// отдельных утверждений по каждой роли.
    ///
    /// Две строки в таблице не имеют порога, и это не пропуск:
    ///
    /// * **подложка hover** — намеренно слабая смесь (0.07 от акцента): её
    ///   задаёт сам курсор, и она обязана быть слабее выбранной строки, иначе
    ///   мышь выглядела бы как выбор. Отличие от карточки проверяет
    ///   `derived_palette_colors_come_from_the_base_ones`.
    /// * **дорожка полосы** — «пустая» часть одного графического элемента.
    ///   WCAG 1.4.11 требует различия графики *целиком* с фоном, то есть
    ///   заливки, и это отдельная строка таблицы. Требовать ещё 3:1 и от
    ///   дорожки нельзя: заливка — это `muted`, а он дальше всех от карточки в
    ///   худшей схеме (4.51:1), и на отрезке в 4.51:1 нельзя поставить точку
    ///   в 3:1 от обоих концов — нужно минимум 9:1. Проверка дорожки на
    ///   контраст поэтому идёт по заливке: «заливка к дорожке» ниже.
    fn pairs(ui: &UiPalette) -> Vec<(&'static str, palette::Rgba, palette::Rgba, f32)> {
        let selected = ui.selected_bg;
        vec![
            ("название строки", ui.text, ui.panel, 4.5),
            ("иконка строки", ui.muted, ui.panel, 3.0),
            ("выбранная строка: текст", ui.accent, selected, 4.5),
            ("выбранная строка: иконка", ui.accent, selected, 3.0),
            ("значение справа", ui.muted, ui.panel, 4.5),
            ("подпись пути", ui.muted, ui.panel, 4.5),
            ("подвал", ui.muted, ui.panel, 4.5),
            ("заглушка поиска", ui.text_disabled, ui.idle_panel, 4.5),
            ("запрос в поиске", ui.text, ui.idle_panel, 4.5),
            ("подложка hover", ui.hover_bg, ui.panel, 0.0),
            ("рамка поля поиска", ui.border_focus, ui.idle_panel, 3.0),
            ("крошка: префикс", ui.muted, ui.panel, 4.5),
            ("крошка: раздел", ui.accent, ui.panel, 4.5),
            ("маркер выбранной строки", ui.accent, ui.panel, 3.0),
            ("рамка карточки", ui.border, ui.panel, 3.0),
            ("полоса: дорожка", ui.border_subtle, ui.panel, 0.0),
            ("полоса: заливка к карточке", ui.muted, ui.panel, 3.0),
            ("полоса: заливка к дорожке", ui.muted, ui.border_subtle, 3.0),
            ("тумблер: кольцо", ui.muted, ui.panel, 3.0),
            ("тумблер: заливка", ui.accent, ui.panel, 3.0),
            ("миниатюра выбора", ui.picker_thumb, ui.panel, 3.0),
            ("заглушка иконки", ui.placeholder, ui.panel, 3.0),
        ]
    }

    /// Контраст ролей обычной темы на всех схемах в обоих режимах.
    ///
    /// Двадцать палитр — десять схем, тёмная и светлая. Медленный, но дешёвый:
    /// считает яркость по разу на пару.
    #[test]
    fn every_pair_passes_its_wcag_threshold_on_all_schemes() {
        let mut failures = Vec::new();
        for light in [false, true] {
            for scheme in MODEL_SCHEMES {
                let model = model(scheme, light);
                let ui = ui_palette(
                    false,
                    palette::Palette {
                        base: model.base,
                        text: model.text,
                        primary: model.primary,
                        secondary: model.secondary,
                        on_primary: model.primary,
                        error: model.text,
                    },
                );
                for (name, front, back, need) in pairs(&ui) {
                    let ratio = contrast(front, back);
                    if ratio < need {
                        failures.push(format!(
                            "{name}: {ratio:.2}:1 при пороге {need} ({scheme}, {})",
                            if light {
                                "светлый"
                            } else {
                                "тёмный"
                            }
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "пары ниже порога:\n{}",
            failures.join("\n")
        );
    }

    /// Те же пары в теме Pixel: у неё свои hex, и правила общие.
    #[test]
    fn every_pair_passes_its_wcag_threshold_in_the_pixel_theme() {
        let ui = ui_palette(true, palette::Palette::default());
        let mut failures = Vec::new();
        for (name, front, back, need) in pairs(&ui) {
            let ratio = contrast(front, back);
            if ratio < need {
                failures.push(format!("{name}: {ratio:.2}:1 при пороге {need}"));
            }
        }
        assert!(
            failures.is_empty(),
            "пары ниже порога в Pixel:\n{}",
            failures.join("\n")
        );
    }

    /// `muted` держит запас над порогом мелкого текста.
    ///
    /// WCAG требует 4.5:1, но `muted` — роль с самым большим числом
    /// употреблений в меню: значение справа, подпись пути, подвал, префикс
    /// крошек, иконка, заливка полосы, кольцо тумблера, полоса прокрутки. На
    /// ровно 4.5:1 любая правка палитры матем или рост числа схем роняет роль
    /// в провал молча, поэтому порог поднят на 0.3.
    #[test]
    fn muted_keeps_a_margin_over_the_text_threshold() {
        let mut worst = f32::MAX;
        let mut where_ = "";
        for light in [false, true] {
            for scheme in MODEL_SCHEMES {
                let model = model(scheme, light);
                let ui = ui_palette(
                    false,
                    palette::Palette {
                        base: model.base,
                        text: model.text,
                        primary: model.primary,
                        secondary: model.secondary,
                        on_primary: model.primary,
                        error: model.text,
                    },
                );
                let value = contrast(ui.muted, ui.panel);
                if value < worst {
                    worst = value;
                    where_ = scheme;
                }
            }
        }
        assert!(
            worst >= MUTED_MARGIN,
            "худший случай {worst:.2}:1 у схемы {where_}, требуется {MUTED_MARGIN}:1"
        );
    }

    /// Пороги не должны разъезжаться: они объявлены один раз и проверяются
    /// таблицей, а не набором отдельных утверждений.
    #[test]
    fn the_thresholds_are_the_ones_the_audit_asked_for() {
        assert_eq!(
            pairs(&ui_palette(false, palette::Palette::default())).len(),
            22
        );
        for (_, _, _, need) in pairs(&ui_palette(true, palette::Palette::default())) {
            assert!(need == 0.0 || need == 3.0 || need == 4.5, "порог {need}");
        }
    }

    /// Заглушка на месте отсутствующей иконки — графика, поэтому порог у неё
    /// 3:1, а не 4.5:1 как у текста. Проверяется отдельно и с запасом: плитка,
    /// до которой не дотягивается глаз, не сообщает ни о чём, а та, что
    /// проходит 4.5:1, читается как значок, которого нет.
    #[test]
    fn the_placeholder_tile_passes_the_graphic_threshold_on_every_scheme() {
        let mut worst = f32::INFINITY;
        for light in [false, true] {
            for scheme in MODEL_SCHEMES {
                let ui = ui_palette(false, model_palette(scheme, light));
                let ratio = contrast(ui.placeholder, ui.panel);
                worst = worst.min(ratio);
                assert!(
                    ratio >= 3.0,
                    "заглушка {scheme} {}: {ratio}",
                    if light {
                        "светлая"
                    } else {
                        "тёмная"
                    }
                );
            }
        }
        assert!(
            worst >= 3.0,
            "худшая пара схем должна держать порог, а не средняя: {worst}"
        );
    }

    /// Подсказки подвала рисуются `muted` на карточке — это тот же порог, что у
    /// остального мелкого текста, и проверяется той же строкой таблицы.
    #[test]
    fn the_footer_hints_use_the_text_role_that_passes_four_and_a_half() {
        let mut worst = f32::INFINITY;
        for light in [false, true] {
            for scheme in MODEL_SCHEMES {
                let ui = ui_palette(false, model_palette(scheme, light));
                worst = worst.min(contrast(ui.muted, ui.panel));
            }
        }
        assert!(worst >= 4.5, "подвальные подсказки и счётчик: {worst}");
        assert!(worst >= MUTED_MARGIN, "и с запасом, как `muted`: {worst}");
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
            // Заглушка приглушена смещением к фону поля, а не прозрачностью:
            // `with_a` поверх `muted` давал альфу 0.37 и контраст 3.08:1.
            assert_eq!(p.text_disabled.3, 255, "заглушка непрозрачна");
            assert_ne!(p.text_disabled, p.text, "заглушка не совпадает с текстом");
            assert_ne!(p.text_disabled, p.muted, "заглушка не совпадает с muted");
            assert_ne!(p.picker_thumb, p.panel, "миниатюра отделяется от карточки");
            assert_ne!(p.hover_bg, p.panel, "подложка hover отделяется от карточки");
            assert_ne!(
                p.hover_bg, p.surface,
                "hover отличается от выбранной строки"
            );
            // Окно непрозрачное: базовые цвета обязаны быть плотными.
            assert_eq!(p.base.3, 255);
            assert_eq!(p.panel.3, 255);
        }
    }
}
