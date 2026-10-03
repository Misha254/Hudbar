//! Виджеты окна настроек: чистые функции рисования.
//!
//! Раньше у каждого элемента был свой `draw_*` в `SettingsApp`, и пятнадцать
//! почти одинаковых прямоугольников считалось в пятнадцати местах. Здесь
//! виджет один: принимает буфер, painter, палитру, шкалу, прямоугольник и
//! состояние. Ни окон, ни Wayland, ни знания о секциях — поэтому один и тот же
//! `Toggle` рисуется в «Панели», «Внешнем виде» и галерее.
//!
//! Все цвета берутся из `UiPalette`, где они выведены из семи базовых через
//! `mix()`, а значит следуют за matugen. Новых hex здесь нет.

use super::settings_icons;
use super::settings_ui::Rect;
use super::settings_view::{fill_round_rect, stroke_rect};
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::{TypeScale, UiPalette, radii, radius, spacing};

/// Состояние виджета. Виджет не знает, откуда оно пришло: фокус от клавиатуры,
/// наведение от мыши, выделение из настроек.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetState {
    pub focused: bool,
    pub hovered: bool,
    /// Недоступен: подпись приглушена, нажатие игнорируется.
    pub disabled: bool,
    /// Выбран в группе: тема, язык, схема, угол экрана.
    pub selected: bool,
    /// Включён: тумблер, флаг.
    pub on: bool,
}

impl WidgetState {
    /// Обычное состояние без выделения.
    pub fn plain() -> Self {
        Self::default()
    }

    pub fn with(mut self, focused: bool, hovered: bool) -> Self {
        self.focused = focused;
        self.hovered = hovered;
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Активна ли рамка фокуса.
    fn focus_ring(&self) -> bool {
        self.focused && !self.disabled
    }

    /// Подсвечен ли элемент при наведении или фокусе.
    fn lit(&self) -> bool {
        (self.hovered || self.focused) && !self.disabled
    }
}

/// Роли кнопки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// Обычная: рамка обычная, текст обычный.
    Secondary,
    /// Главная: рамка и текст акцентные.
    Primary,
}

/// Роли «таблетки».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeKind {
    Accent,
    Neutral,
}

/// Контекст рисования виджета: буфер, painter, палитра и шкала. Собран в
/// структуру, потому что у каждого виджета они одинаковые, а список аргументов
/// девять штук ни о чём не говорит.
pub struct Ctx<'a> {
    pub pixmap: &'a mut tiny_skia::Pixmap,
    pub painter: &'a mut TextPainter,
    pub palette: UiPalette,
    pub scale: TypeScale,
    pub pixel: bool,
}

impl<'a> Ctx<'a> {
    /// Рамка в 1 px. Толщина не зависит от темы: в Pixel отличие от
    /// скругления — это углы, а не толщина линии.
    fn outline(&mut self, rect: Rect, color: super::palette::Rgba) {
        stroke_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            color,
            SCALE,
        );
    }

    /// Рамка толще — для фокуса, чтобы её было видно на любом фоне.
    fn focus(&mut self, rect: Rect, color: super::palette::Rgba) {
        stroke_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            color,
            2.0 * SCALE,
        );
    }

    /// Заливка со скруглением по теме.
    fn fill(&mut self, rect: Rect, token: f32, color: super::palette::Rgba) {
        fill_round_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            radius(self.pixel, token) * SCALE,
            color,
        );
    }

    /// Подпись виджета: текст в заданной полосе. Галерея и окно рисуют
    /// через неё, чтобы выравнивание и шкала были одни и те же.
    pub fn paint(
        &mut self,
        text: &str,
        size: f32,
        color: super::palette::Rgba,
        rect: Rect,
        align: Align,
    ) {
        self.painter
            .paint(self.pixmap, text, size, color, rect, align);
    }
}

/// Внутренние отступы карточки.
pub const CARD_PAD: f32 = 16.0;
/// Зазор между заголовком карточки и содержимым.
pub const CARD_TITLE_GAP: f32 = 12.0;
/// Высота заголовка карточки.
pub const CARD_TITLE_H: f32 = 22.0;

/// Высота карточки: заголовок (если есть) плюс содержимое плюс отступы.
pub fn card_height(content: f32, has_title: bool) -> f32 {
    let title = if has_title {
        CARD_TITLE_H + CARD_TITLE_GAP
    } else {
        0.0
    };
    title + content + CARD_PAD * 2.0
}

/// Внутренняя область карточки: то, что не съел заголовок и отступы.
pub fn card_content_rect(rect: Rect, has_title: bool) -> Rect {
    let top = rect.y
        + CARD_PAD
        + if has_title {
            CARD_TITLE_H + CARD_TITLE_GAP
        } else {
            0.0
        };
    Rect::new(
        rect.x + CARD_PAD,
        top,
        rect.w - CARD_PAD * 2.0,
        (rect.bottom() - CARD_PAD - top).max(0.0),
    )
}

/// Заголовок карточки. Подпись рисуется ролью `section_title`.
pub fn card(ctx: &mut Ctx<'_>, rect: Rect, title: Option<&str>) {
    ctx.fill(rect, radii::MD, ctx.palette.surface);
    ctx.outline(rect, ctx.palette.border_subtle);
    if let Some(title) = title {
        ctx.paint(
            title,
            ctx.scale.section_title,
            ctx.palette.text,
            Rect::new(
                rect.x + CARD_PAD,
                rect.y + CARD_PAD,
                rect.w - CARD_PAD * 2.0,
                CARD_TITLE_H,
            ),
            Align::Start,
        );
    }
}

/// Тумблер: трек 36x20, ручка 16. Включён — трек акцентный и ручка тёмная,
/// выключен — трек приподнятый, ручка приглушена. Это видно с одного взгляда,
/// в отличие от пустой дорожки.
pub fn toggle(ctx: &mut Ctx<'_>, rect: Rect, state: WidgetState) {
    let track_w = 36.0;
    let track_h = 20.0;
    let knob = 16.0;
    let track = Rect::new(
        rect.right() - track_w,
        rect.y + (rect.h - track_h) / 2.0,
        track_w,
        track_h,
    );
    ctx.fill(
        track,
        radii::LG,
        if state.on {
            ctx.palette.accent
        } else {
            ctx.palette.surface_hover
        },
    );
    if state.on {
        ctx.outline(track, ctx.palette.accent);
    } else {
        ctx.outline(track, ctx.palette.border_subtle);
    }
    let knob_x = if state.on {
        track.right() - knob - 2.0
    } else {
        track.x + 2.0
    };
    let knob_rect = Rect::new(knob_x, track.y + 2.0, knob, knob);
    ctx.fill(
        knob_rect,
        radii::LG,
        if state.on {
            ctx.palette.base
        } else {
            ctx.palette.muted
        },
    );
    if state.focus_ring() {
        ctx.focus(track, ctx.palette.border_focus);
    }
}

/// Кнопка. `Secondary` — обычная рамка, `Primary` — акцентная, как CLOSE в
/// макете. Недоступная рисуется приглушённой и без рамки фокуса.
pub fn button(ctx: &mut Ctx<'_>, rect: Rect, label: &str, kind: ButtonKind, state: WidgetState) {
    let (border, text) = if state.disabled {
        (ctx.palette.border_subtle, ctx.palette.text_disabled)
    } else if state.focus_ring() {
        (
            ctx.palette.border_focus,
            match kind {
                ButtonKind::Primary => ctx.palette.accent,
                ButtonKind::Secondary => ctx.palette.text,
            },
        )
    } else if state.hovered && kind == ButtonKind::Primary {
        (ctx.palette.accent, ctx.palette.accent)
    } else {
        match kind {
            ButtonKind::Primary => (ctx.palette.accent, ctx.palette.accent),
            ButtonKind::Secondary => (ctx.palette.border, ctx.palette.text),
        }
    };
    let fill = if state.lit() && !state.disabled {
        ctx.palette.surface_hover
    } else {
        ctx.palette.surface
    };
    ctx.fill(rect, radii::SM, fill);
    ctx.outline(rect, border);
    ctx.paint(
        label,
        ctx.scale.caption,
        text,
        Rect::new(rect.x, rect.y, rect.w, rect.h),
        Align::Center,
    );
}

/// Квадратная кнопка с глифом: `↑`, `↓`, `−`, `+`. Символ всегда по центру,
/// поэтому рамка не «прыгает» между состояниями.
pub fn icon_button(ctx: &mut Ctx<'_>, rect: Rect, glyph: &str, state: WidgetState) {
    let (border, text) = if state.disabled {
        (ctx.palette.border_subtle, ctx.palette.text_disabled)
    } else if state.focus_ring() {
        (ctx.palette.border_focus, ctx.palette.accent)
    } else if state.hovered {
        (ctx.palette.accent, ctx.palette.accent)
    } else {
        (ctx.palette.border, ctx.palette.text)
    };
    let fill = if state.lit() && !state.disabled {
        ctx.palette.surface_hover
    } else {
        ctx.palette.surface
    };
    ctx.fill(rect, radii::SM, fill);
    ctx.outline(rect, border);
    ctx.paint(
        glyph,
        ctx.scale.label,
        text,
        Rect::new(rect.x, rect.y, rect.w, rect.h),
        Align::Center,
    );
}

/// Степпер `− значение +` в одной полосе. Значение крупнее кнопок: это то,
/// что человек читает, кнопки он только жмёт.
pub struct StepperParts {
    pub minus: Rect,
    pub value: Rect,
    pub plus: Rect,
}

/// Раскладка степпера внутри `rect` при размере кнопки `btn`.
pub fn stepper_layout(rect: Rect, btn: f32) -> StepperParts {
    let gap = spacing::SM;
    let value = rect.right() - btn * 2.0 - gap * 2.0;
    StepperParts {
        minus: Rect::new(value + gap, rect.y, btn, rect.h),
        value: Rect::new(rect.x, rect.y, value - gap - rect.x, rect.h),
        plus: Rect::new(rect.right() - btn, rect.y, btn, rect.h),
    }
}

/// Степпер целиком. `value` — уже готовая строка с единицей измерения.
pub fn stepper(ctx: &mut Ctx<'_>, rect: Rect, label: &str, value: &str, state: WidgetState) {
    let parts = stepper_layout(rect, rect.h.min(32.0));
    if !label.is_empty() {
        ctx.paint(
            label,
            ctx.scale.label,
            ctx.palette.text,
            Rect::new(rect.x, rect.y, parts.value.x - spacing::MD - rect.x, rect.h),
            Align::Start,
        );
    }
    ctx.paint(
        value,
        ctx.scale.value,
        ctx.palette.text,
        parts.value,
        Align::Center,
    );
    icon_button(ctx, parts.minus, settings_icons::MINUS, state);
    icon_button(ctx, parts.plus, settings_icons::PLUS, state);
}

/// Слайдер: трек 4 px, заполненная часть акцентом, ручка — круг с кольцом.
/// Подписи min/max под треком, значение справа. Та же полоса, что у
/// `draw_height_scale`, но собранная из токенов.
pub struct SliderParts {
    pub track: Rect,
    pub knob: Rect,
}

/// Раскладка слайдера. Ручка — круг диаметром `knob`, поэтому её центр ходит
/// только по треку за вычетом радиуса: на краях она не вылезает за полосу.
pub fn slider_layout(rect: Rect, knob: f32, ratio: f32) -> SliderParts {
    let track = Rect::new(rect.x, rect.y, rect.w, 4.0);
    let travel = (track.w - knob).max(0.0);
    let centre = track.x + knob / 2.0 + travel * ratio.clamp(0.0, 1.0);
    SliderParts {
        track,
        knob: Rect::new(centre - knob / 2.0, track.y + 2.0 - knob / 2.0, knob, knob),
    }
}

/// Слайдер. `ratio` — доля заполнения 0..1.
pub fn slider(ctx: &mut Ctx<'_>, rect: Rect, ratio: f32, state: WidgetState) {
    let parts = slider_layout(rect, 14.0, ratio);
    ctx.fill(parts.track, radii::XS, ctx.palette.border_subtle);
    let filled = Rect::new(
        parts.track.x,
        parts.track.y,
        (parts.knob.x + parts.knob.w / 2.0 - parts.track.x).max(0.0),
        parts.track.h,
    );
    if filled.w > 0.0 {
        ctx.fill(filled, radii::XS, ctx.palette.accent);
    }
    let knob_color = if state.lit() {
        ctx.palette.accent
    } else {
        ctx.palette.surface
    };
    ctx.fill(parts.knob, radii::LG, knob_color);
    ctx.outline(
        parts.knob,
        if state.lit() {
            ctx.palette.accent
        } else {
            ctx.palette.border
        },
    );
}

/// Плитка выбора: тема, язык, схема, угол экрана. Выбранная получает рамку
/// фокуса, приглушённую заливку и галочку справа — тем же способом, каким
/// выбранная тема помечена в «Внешнем виде».
pub fn choice_tile(ctx: &mut Ctx<'_>, rect: Rect, label: &str, state: WidgetState) {
    let fill = if state.selected {
        ctx.palette.accent_dim
    } else if state.lit() {
        ctx.palette.surface_hover
    } else {
        ctx.palette.surface
    };
    ctx.fill(rect, radii::SM, fill);
    ctx.outline(
        rect,
        if state.selected || state.focus_ring() {
            ctx.palette.border_focus
        } else if state.hovered {
            ctx.palette.border
        } else {
            ctx.palette.border_subtle
        },
    );
    ctx.paint(
        label,
        ctx.scale.caption,
        if state.selected {
            ctx.palette.accent
        } else {
            ctx.palette.text
        },
        Rect::new(
            rect.x + spacing::SM + spacing::XS,
            rect.y,
            (rect.w - spacing::SM * 2.0).max(0.0),
            rect.h,
        ),
        Align::Center,
    );
    if state.selected {
        ctx.paint(
            settings_icons::CHECK,
            ctx.scale.caption,
            ctx.palette.accent,
            Rect::new(rect.right() - 24.0, rect.y, 18.0, rect.h),
            Align::Center,
        );
    }
}

/// «Таблетка» с точкой и текстом капсом: статус окна и метки состояния.
pub fn badge(ctx: &mut Ctx<'_>, rect: Rect, label: &str, kind: BadgeKind) {
    let (fill, text) = match kind {
        BadgeKind::Accent => (ctx.palette.accent_dim, ctx.palette.accent),
        BadgeKind::Neutral => (ctx.palette.surface_hover, ctx.palette.muted),
    };
    ctx.fill(rect, radii::LG, fill);
    let dot = 6.0;
    let dot_rect = Rect::new(
        rect.x + spacing::SM,
        rect.y + (rect.h - dot) / 2.0,
        dot,
        dot,
    );
    ctx.fill(
        dot_rect,
        radii::LG,
        match kind {
            BadgeKind::Accent => ctx.palette.accent,
            BadgeKind::Neutral => ctx.palette.muted,
        },
    );
    ctx.paint(
        label,
        ctx.scale.caption,
        text,
        Rect::new(
            dot_rect.right() + spacing::XS,
            rect.y,
            (rect.right() - dot_rect.right() - spacing::SM - spacing::XS).max(0.0),
            rect.h,
        ),
        Align::Center,
    );
}

/// Иконка глифом Nerd Font. Шрифт задаёт вызывающий: `painter` должен быть
/// копией с `settings_icons::FONT`, иначе в Pixel будет «тофу».
pub fn icon(ctx: &mut Ctx<'_>, rect: Rect, glyph: &str) {
    ctx.paint(
        glyph,
        ctx.scale.label,
        ctx.palette.muted,
        rect,
        Align::Center,
    );
}

#[cfg(test)]
mod tests {
    use super::super::palette;
    use super::super::ui_tokens::{type_scale, ui_palette};
    use super::*;

    fn buf() -> tiny_skia::Pixmap {
        tiny_skia::Pixmap::new(400, 200).expect("pixmap")
    }

    fn rect() -> Rect {
        Rect::new(10.0, 10.0, 200.0, 40.0)
    }

    /// Каждый виджет должен рисоваться в любом состоянии и не падать: иначе
    /// опечатка в состоянии уронит окно.
    #[test]
    fn every_widget_draws_in_every_state() {
        let p = ui_palette(false, palette::Palette::default());
        let pixel_palette = ui_palette(true, palette::Palette::default());
        let states = [
            WidgetState::plain(),
            WidgetState::plain().on(true),
            WidgetState::plain().selected(true),
            WidgetState::plain().disabled(true),
            WidgetState::plain().with(true, false),
            WidgetState::plain().with(false, true),
            WidgetState::plain()
                .with(true, true)
                .on(true)
                .selected(true),
        ];
        for pixel in [false, true] {
            let palette = if pixel { pixel_palette } else { p };
            let s = type_scale(pixel);
            for state in states {
                let mut b = buf();
                let mut painter = TextPainter::new(settings_icons::FONT);
                let mut ctx = Ctx {
                    pixmap: &mut b,
                    painter: &mut painter,
                    palette,
                    scale: s,
                    pixel,
                };
                card(&mut ctx, rect(), Some("Заголовок"));
                toggle(&mut ctx, rect(), state);
                button(&mut ctx, rect(), "Кнопка", ButtonKind::Secondary, state);
                button(&mut ctx, rect(), "Кнопка", ButtonKind::Primary, state);
                icon_button(&mut ctx, rect(), settings_icons::UP, state);
                stepper(&mut ctx, rect(), "Поле", "27 px", state);
                slider(&mut ctx, rect(), 0.4, state);
                choice_tile(&mut ctx, rect(), "Плитка", state);
                badge(&mut ctx, rect(), "ГОТОВО", BadgeKind::Accent);
                badge(&mut ctx, rect(), "ТИХО", BadgeKind::Neutral);
                icon(&mut ctx, rect(), settings_icons::PANEL);
            }
        }
    }

    /// Высота карточки складывается из заголовка, содержимого и отступов, а
    /// внутренняя область равна именно тому, что осталось.
    #[test]
    fn card_height_is_title_plus_content_plus_padding() {
        let content = 74.0;
        assert!((card_height(content, false) - (content + CARD_PAD * 2.0)).abs() < 0.001);
        assert!(
            (card_height(content, true)
                - (content + CARD_TITLE_H + CARD_TITLE_GAP + CARD_PAD * 2.0))
                .abs()
                < 0.001
        );

        let outer = Rect::new(0.0, 0.0, 300.0, card_height(content, true));
        let inner = card_content_rect(outer, true);
        assert!(
            (inner.h - content).abs() < 0.001,
            "высота содержимого {inner:?}"
        );
        assert!((inner.x - CARD_PAD).abs() < 0.001, "нет левого отступа");
        assert!(
            (inner.right() - (outer.right() - CARD_PAD)).abs() < 0.001,
            "нет правого"
        );
        assert!(inner.y >= outer.y + CARD_PAD, "нет верхнего отступа");
        assert!(
            (outer.bottom() - inner.bottom() - CARD_PAD).abs() < 0.001,
            "нет нижнего"
        );
    }

    /// Карточка без заголовка отдаёт содержимому всю высоту.
    #[test]
    fn card_without_title_gives_all_height_to_content() {
        let outer = Rect::new(0.0, 0.0, 200.0, card_height(40.0, false));
        let inner = card_content_rect(outer, false);
        assert!((inner.h - 40.0).abs() < 0.001);
        assert!((inner.y - outer.y - CARD_PAD).abs() < 0.001);
    }

    /// Степпер: две кнопки по краям, значение между ними, ничего не наезжает.
    #[test]
    fn stepper_layout_fits_the_row() {
        let row = Rect::new(100.0, 50.0, 400.0, 32.0);
        let parts = stepper_layout(row, 32.0);
        assert_eq!(
            parts.minus.x + 32.0 + spacing::SM,
            parts.plus.x,
            "между кнопками зазор"
        );
        assert!(
            (parts.plus.right() - row.right()).abs() < 0.001,
            "плюс не у края"
        );
        assert!(
            parts.value.right() <= parts.minus.x,
            "значение налезает на кнопку"
        );
        assert!(parts.value.x >= row.x, "значение вылезло за строку");
    }

    /// Слайдер: трек на всю ширину, ручка внутри и по краям не выходит.
    #[test]
    fn slider_keeps_the_knob_inside_the_track() {
        for ratio in [0.0, 0.5, 1.0] {
            let row = Rect::new(50.0, 100.0, 300.0, 4.0);
            let parts = slider_layout(row, 14.0, ratio);
            assert!((parts.track.w - row.w).abs() < 0.001);
            assert!(parts.knob.x >= row.x - 0.001, "ручка уехала влево: {ratio}");
            assert!(
                parts.knob.right() <= row.right() + 0.001,
                "ручка уехала вправо: {ratio}"
            );
            // Центр ручки на краях стоит на треке, а не за ним.
            let centre = parts.knob.x + parts.knob.w / 2.0;
            assert!(
                centre >= row.x && centre <= row.right(),
                "центр ручки вне трека"
            );
        }
    }

    /// У фокуса есть рамка, у недоступного — нет, даже если он в фокусе.
    #[test]
    fn focus_ring_is_skipped_for_disabled_widgets() {
        let state = WidgetState::plain().with(true, false).disabled(true);
        assert!(!state.focus_ring());
        assert!(!state.lit());
        assert!(WidgetState::plain().with(true, false).focus_ring());
    }

    /// В Pixel скругление нулевое у всех виджетов.
    #[test]
    fn pixel_theme_has_square_widgets() {
        for token in [radii::XS, radii::SM, radii::MD, radii::LG] {
            assert_eq!(radius(true, token), 0.0);
        }
    }
}
