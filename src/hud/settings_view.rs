//! Отрисовка окна настроек отдельно от Wayland.
//!
//! Раньше рисование жило методами `SettingsApp` вперемешку с обработчиками
//! ввода, из-за чего снять скриншот без композитора было невозможно. Здесь
//! отрисовка описана двумя трейтами: `View` задаёт доступ к состоянию,
//! `DrawExt` — саму отрисовку. И живое окно, и снапшот реализуют `View`,
//! поэтому PNG и реальное окно рисуются одним кодом и разойтись не могут.
//!
//! Геометрия не меняется: она по-прежнему в `settings_ui`.

use super::config;
use super::config::Config;
use super::palette;
use super::settings;
use super::settings_ui::{
    self, Control, Focus, Module, Nav, NotificationPosition, Rect, Row, Section,
};
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::radii as Radius;
use super::ui_tokens::{TypeScale, UiPalette, always, radius, type_scale, ui_palette};

pub const TITLE: &str = "HUDbar  /  Control Center";

/// Контуры и центральные линии отладочной отрисовки раскладки.
const DEBUG_OUTLINE: palette::Rgba = palette::Rgba(0xff, 0x5c, 0x5c, 255);

/// Имя файла без пути: в списке обоев полный путь занимает всю строку.
pub fn notification_position_label(position: NotificationPosition) -> &'static str {
    match position {
        NotificationPosition::TopLeft => "Сверху слева",
        NotificationPosition::TopRight => "Сверху справа",
        NotificationPosition::BottomLeft => "Снизу слева",
        NotificationPosition::BottomRight => "Снизу справа",
    }
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Подпись обоев: имя файла и папка-категория вместо полного пути.
pub fn wallpaper_dir_label(path: &str) -> String {
    let name = file_name(path);
    let mut parts = path.rsplit('/');
    parts.next();
    let dir = parts.next().unwrap_or("");
    if dir.is_empty() || dir == "wallpapers" {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn fill_rect(pixmap: &mut tiny_skia::Pixmap, x: f32, y: f32, w: f32, h: f32, color: palette::Rgba) {
    let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
        return;
    };
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
        anti_alias: false,
        ..Default::default()
    };
    pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
}

fn fill_round_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: palette::Rgba,
) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(x + w - r, y);
    path.quad_to(x + w, y, x + w, y + r);
    path.line_to(x + w, y + h - r);
    path.quad_to(x + w, y + h, x + w - r, y + h);
    path.line_to(x + r, y + h);
    path.quad_to(x, y + h, x, y + h - r);
    path.line_to(x, y + r);
    path.quad_to(x, y, x + r, y);
    path.close();
    if let Some(path) = path.finish() {
        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
            anti_alias: true,
            ..Default::default()
        };
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
}

fn stroke_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: palette::Rgba,
    width: f32,
) {
    fill_rect(pixmap, x, y, w, width, color);
    fill_rect(pixmap, x, y + h - width, w, width, color);
    fill_rect(pixmap, x, y, width, h, color);
    fill_rect(pixmap, x + w - width, y, width, h, color);
}

/// Источник состояния для отрисовки: и живое окно, и снапшот.
pub trait View {
    fn painter(&mut self) -> &mut TextPainter;
    fn config(&self) -> &Config;
    fn rows(&self) -> &[Row];
    fn nav(&self) -> &Nav;
    fn hover(&self) -> Option<Focus>;
    /// Тема, выбранная пользователем, но ещё не применённая.
    fn picked(&self) -> Option<bool>;
    fn palette(&self) -> palette::Palette;
    fn status(&self) -> String;
    /// Идёт ли фоновая работа: на это время кнопки подсвечены.
    fn busy_flag(&self) -> bool;
}

/// Шрифт темы: у Pixel и обычного он разный, и он же попадает в конфиг.
pub fn font_name(pixel: bool) -> String {
    if pixel {
        "Minecraft Rus".to_string()
    } else {
        "JetBrainsMono Nerd Font Propo".to_string()
    }
}

/// Вся отрисовка окна: те же методы, что были у `SettingsApp`, но состояние
/// берётся через `View`, поэтому их можно звать и без Wayland.
pub trait DrawExt: View {
    fn pixel_mode(&self) -> bool {
        self.picked()
            .unwrap_or(self.config().theme == Some(config::Theme::Pixel))
    }

    fn font_for(&self, pixel: bool) -> String {
        if pixel {
            "Minecraft Rus".to_string()
        } else {
            "JetBrainsMono Nerd Font Propo".to_string()
        }
    }

    fn busy(&self) -> bool {
        self.busy_flag()
    }

    /// Координаты указателя уже логические: `wl_pointer` сообщает их в
    /// surface-local пикселях, поэтому масштаб буфера не применяется.
    fn hit(&self, x: f32, y: f32) -> Option<Focus> {
        settings_ui::hit(self.rows(), x, y)
    }

    fn text_width(&mut self, text: &str, size: f32) -> f32 {
        self.painter().text_width(text, size)
    }

    /// Сфокусирован ли контрол правой колонки.
    fn focused(&self, control: Control) -> bool {
        self.nav().focus == Focus::Content(control)
    }

    /// Наведён ли мышью контрол правой колонки.
    fn hovered(&self, control: Control) -> bool {
        self.hover() == Some(Focus::Content(control))
    }

    /// Контуры всех полос и линия их центра — включается `HUD_DEBUG_LAYOUT=1`.
    fn draw_debug_overlay(&mut self, pixmap: &mut tiny_skia::Pixmap) {
        let k = SCALE;
        for rect in settings_ui::debug_rects() {
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                DEBUG_OUTLINE,
                k,
            );
            // горизонтальная линия центра полосы
            fill_rect(
                pixmap,
                rect.x * k,
                (rect.y + rect.h / 2.0) * k,
                rect.w * k,
                k,
                DEBUG_OUTLINE.with_a(0.8),
            );
        }
    }

    /// Переключатель «Кнопка Control Center»: слева подпись, справа тумблер
    /// вкл/выкл — тот же вид, что у строки модуля.
    fn draw_control_button(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        s: TypeScale,
        pixel: bool,
    ) {
        let k = SCALE;
        let control = Control::ControlButton;
        let on = self.config().control_button;
        let active = self.hovered(control) || self.focused(control);
        if active {
            fill_round_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                radius(self.pixel_mode(), Radius::SM) * k,
                p.idle_panel,
            );
        }
        self.painter().paint(
            pixmap,
            "Кнопка Control Center",
            s.label,
            if on { p.text } else { p.muted },
            Rect::new(rect.x + 10.0, rect.y, rect.w - 60.0, rect.h),
            Align::Start,
        );
        let track = Rect::new(
            rect.right() - 10.0 - 46.0,
            rect.y + (rect.h - 18.0) / 2.0,
            46.0,
            18.0,
        );
        fill_round_rect(
            pixmap,
            track.x * k,
            track.y * k,
            track.w * k,
            track.h * k,
            always(Radius::TRACK) * k,
            if on { p.accent } else { p.border },
        );
        let knob = 14.0;
        let knob_x = if on {
            track.right() - knob - 2.0
        } else {
            track.x + 2.0
        };
        fill_round_rect(
            pixmap,
            knob_x * k,
            (track.y + 2.0) * k,
            knob * k,
            knob * k,
            always(Radius::RAIL) * k,
            if on { p.base } else { p.text.with_a(0.7) },
        );
        let _ = pixel;
    }

    /// Файл обоев в списке: выбранный подсвечивается акцентной рамкой, имя
    /// обрезается по ширине полосы.
    #[allow(clippy::too_many_arguments)]
    fn draw_wallpaper_file(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        name: &str,
        selected: bool,
        active: bool,
        s: TypeScale,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(self.pixel_mode(), Radius::SM) * k,
            if selected || active {
                p.panel
            } else {
                p.idle_panel
            },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if selected || active {
                p.accent
            } else {
                p.border
            },
            if selected { 2.0 * k } else { k },
        );
        self.painter().paint_boxed(
            pixmap,
            name,
            s.label,
            if selected { p.text } else { p.muted },
            Rect::new(rect.x + 12.0, rect.y, rect.w - 60.0, rect.h),
            Align::Start,
        );
        if selected {
            self.painter().paint(
                pixmap,
                "выбран",
                s.caption,
                p.accent,
                Rect::new(rect.right() - 56.0, rect.y, 46.0, rect.h),
                Align::End,
            );
        }
    }

    /// Кнопка языка: та же подсветка с галочкой, что у схем и темы.
    #[allow(clippy::too_many_arguments)]
    fn draw_language(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        language: settings::Language,
        rect: Rect,
        selected: bool,
        s: TypeScale,
        pixel: bool,
    ) {
        let control = Control::Language(language);
        self.draw_button(
            pixmap,
            p,
            rect,
            language.short(),
            selected || self.hovered(control) || self.focused(control),
            s.caption,
            pixel,
        );
        if selected {
            self.painter().paint(
                pixmap,
                "✓",
                s.label,
                p.accent,
                Rect::new(rect.right() - 26.0, rect.y, 20.0, rect.h),
                Align::Center,
            );
        }
    }

    /// Схема matugen: та же подсветка выбранного, что и у темы.
    #[allow(clippy::too_many_arguments)]
    fn draw_scheme(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        name: &str,
        index: usize,
        selected: bool,
        s: TypeScale,
        pixel: bool,
    ) {
        let control = Control::Scheme(index);
        self.draw_button(
            pixmap,
            p,
            rect,
            name,
            selected || self.hovered(control) || self.focused(control),
            s.caption,
            pixel,
        );
        if selected {
            let k = SCALE;
            self.painter().paint(
                pixmap,
                "✓",
                s.label,
                p.accent,
                Rect::new(rect.right() - 26.0, rect.y, 20.0, rect.h),
                Align::Center,
            );
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                p.accent,
                2.0 * k,
            );
        }
    }

    /// Действие меню без бинда: пометка «нет клавиши» вместо сочетания.
    fn draw_missing_bind(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        desc: &str,
        s: TypeScale,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(self.pixel_mode(), Radius::SM) * k,
            p.idle_panel,
        );
        self.painter().paint_boxed(
            pixmap,
            "нет клавиши",
            s.caption,
            p.accent,
            Rect::new(rect.x + 10.0, rect.y, rect.w * 0.42, rect.h),
            Align::Start,
        );
        let keys_x = rect.x + 10.0 + rect.w * 0.42;
        self.painter().paint_boxed(
            pixmap,
            desc,
            s.label,
            p.muted,
            Rect::new(keys_x + 10.0, rect.y, rect.right() - keys_x - 20.0, rect.h),
            Align::Start,
        );
    }

    /// Строка сводки: подпись слева, значение справа. Кликается как ссылка в
    /// раздел, к которому относится значение, — дублировать контролы не нужно.
    #[allow(clippy::too_many_arguments)]
    fn draw_summary(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        value: &str,
        section: Section,
        s: TypeScale,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(self.pixel_mode(), Radius::MD) * k,
            p.idle_panel,
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            p.border,
            k,
        );
        self.painter().paint(
            pixmap,
            label,
            s.label,
            p.muted,
            Rect::new(rect.x + 12.0, rect.y, rect.w * 0.5, rect.h),
            Align::Start,
        );
        let control = Control::Goto(section);
        let active = self.hovered(control) || self.focused(control);
        if active {
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                p.accent,
                k,
            );
        }
        self.painter().paint_boxed(
            pixmap,
            value,
            s.label,
            p.text,
            Rect::new(rect.x + rect.w * 0.5, rect.y, rect.w * 0.5 - 34.0, rect.h),
            Align::End,
        );
        self.painter().paint(
            pixmap,
            "→",
            s.label,
            if active { p.accent } else { p.muted },
            Rect::new(rect.right() - 28.0, rect.y, 20.0, rect.h),
            Align::Center,
        );
    }

    /// Строка модуля в разделе «Панель»: название, переключатель вкл/выкл и
    /// кнопки ▲▼ строго слева направо, без наложений и обрезки.
    fn draw_module_row(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        module: Module,
        line: usize,
        rect: Rect,
        s: TypeScale,
    ) {
        let k = SCALE;
        let control = Control::Switch(module);
        let on = self.config().module_enabled(module);
        let row_radius = radius(self.pixel_mode(), Radius::SM) * k;
        if self.hovered(control) || self.focused(control) {
            fill_round_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                row_radius,
                p.idle_panel,
            );
        }

        // 1. Название. Длинные вроде «Не беспокоить» умещаются целиком:
        //    ширина полосы задана от содержимого колонки, а не наоборот.
        let name = module.label();
        self.painter().paint_boxed(
            pixmap,
            name,
            s.caption,
            if on { p.text } else { p.muted },
            settings_ui::module_name_rect(module, line),
            Align::Start,
        );

        // 2. Переключатель вкл/выкл.
        let switch = settings_ui::module_switch_rect(module, line);
        fill_round_rect(
            pixmap,
            switch.x * k,
            switch.y * k,
            switch.w * k,
            switch.h * k,
            always(Radius::TRACK) * k,
            if on { p.accent } else { p.border },
        );
        let knob = switch.h - 4.0;
        let knob_x = if on {
            switch.right() - knob - 2.0
        } else {
            switch.x + 2.0
        };
        fill_round_rect(
            pixmap,
            knob_x * k,
            (switch.y + 2.0) * k,
            knob * k,
            knob * k,
            always(Radius::KNOB) * k,
            if on { p.base } else { p.text.with_a(0.7) },
        );

        if self.focused(control) {
            fill_rect(
                pixmap,
                rect.x * k,
                (rect.y + 5.0) * k,
                2.0 * k,
                (rect.h - 10.0) * k,
                p.accent,
            );
        }
        let _ = radius;
    }

    /// Горячая клавиша из niri: сочетание слева, расшифровка справа. Строка
    /// informational — кликать тут нечего.
    fn draw_hotkey(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        keys: &str,
        desc: &str,
        s: TypeScale,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(self.pixel_mode(), Radius::SM) * k,
            p.idle_panel,
        );
        // Обе части обрезаются по измеренной ширине: длинное сочетание или
        // расшифровка не должны вылезать за границу колонки.
        // Полоса клавиш фиксированной доли строки: длинное сочетание и длинная
        // расшифровка обе обрезаются по измеренной ширине и не выходят за колонку.
        let keys_w = rect.w * 0.42;
        let keys_x = rect.x + 10.0;
        self.painter().paint_boxed(
            pixmap,
            keys,
            s.caption,
            p.accent,
            Rect::new(keys_x, rect.y, keys_w, rect.h),
            Align::Start,
        );
        let desc_x = keys_x + keys_w + 10.0;
        self.painter().paint_boxed(
            pixmap,
            desc,
            s.label,
            p.text,
            Rect::new(desc_x, rect.y, rect.right() - desc_x - 10.0, rect.h),
            Align::Start,
        );
    }

    /// Кнопка шапки: подпись центрируется по обеим осям внутри рамки.
    #[allow(clippy::too_many_arguments)]
    fn draw_button(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        active: bool,
        size: f32,
        pixel: bool,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(pixel, Radius::SM) * k,
            if active { p.panel } else { p.idle_panel },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if active { p.accent } else { p.border },
            k,
        );
        self.painter()
            .paint(pixmap, label, size, p.text, rect, Align::Center);
    }

    /// Кнопка, которая сейчас не работает: подпись приглушена, чтобы её
    /// нельзя было спутать с доступной.
    #[allow(clippy::too_many_arguments)]
    fn draw_button_disabled(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        active: bool,
        size: f32,
        pixel: bool,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(pixel, Radius::SM) * k,
            if active { p.panel } else { p.idle_panel },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if active { p.accent } else { p.border },
            k,
        );
        self.painter()
            .paint(pixmap, label, size, p.muted, rect, Align::Center);
    }

    fn draw_navigation(&mut self, pixmap: &mut tiny_skia::Pixmap, p: UiPalette, s: TypeScale) {
        let k = SCALE;
        let focus = self.nav().focus;
        let hover = self.hover();
        let section_now = self.nav().section;
        let rail = settings_ui::rail_rect();
        fill_round_rect(
            pixmap,
            rail.x * k,
            rail.y * k,
            rail.w * k,
            rail.h * k,
            always(Radius::RAIL) * k,
            p.idle_panel,
        );
        stroke_rect(
            pixmap,
            rail.x * k,
            rail.y * k,
            rail.w * k,
            rail.h * k,
            p.border,
            k,
        );
        self.painter().paint(
            pixmap,
            "РАЗДЕЛЫ",
            s.caption,
            p.muted,
            Rect::new(rail.x + 18.0, rail.y + 14.0, rail.w - 36.0, 20.0),
            Align::Start,
        );

        // Три состояния пункта: активный раздел — заливка и полоса слева,
        // фокус — контур, наведение — слабая подсветка.
        for (index, section) in Section::ALL.iter().enumerate() {
            let rect = settings_ui::nav_rect(index);
            let selected = *section == section_now;
            let focused = focus == Focus::Nav(index);
            let hovered = hover == Some(Focus::Nav(index));
            if selected {
                fill_round_rect(
                    pixmap,
                    rect.x * k,
                    rect.y * k,
                    rect.w * k,
                    rect.h * k,
                    5.0 * k,
                    p.panel,
                );
                fill_rect(
                    pixmap,
                    rect.x * k,
                    (rect.y + 8.0) * k,
                    3.0 * k,
                    (rect.h - 16.0) * k,
                    p.accent,
                );
            } else if hovered {
                fill_round_rect(
                    pixmap,
                    rect.x * k,
                    rect.y * k,
                    rect.w * k,
                    rect.h * k,
                    5.0 * k,
                    p.idle_panel,
                );
            }
            if focused && !selected {
                stroke_rect(
                    pixmap,
                    rect.x * k,
                    rect.y * k,
                    rect.w * k,
                    rect.h * k,
                    p.accent,
                    k,
                );
            }
            self.painter().paint(
                pixmap,
                section.label(),
                s.label,
                if selected { p.text } else { p.muted },
                Rect::new(rect.x + 20.0, rect.y, rect.w - 32.0, rect.h),
                Align::Start,
            );
        }

        let rail_w = rail.w - 36.0;
        fill_rect(
            pixmap,
            (rail.x + 18.0) * k,
            settings_ui::RAIL_RULE_Y * k,
            rail_w * k,
            k,
            p.border,
        );
        let theme = if self.pixel_mode() {
            "ТЕМА: PIXEL"
        } else {
            "ТЕМА: NORMAL"
        };
        let language = if self.config().language == settings::Language::Ru {
            "ЯЗЫК: РУС"
        } else {
            "ЯЗЫК: ENG"
        };
        let lines = [
            ("СОСТОЯНИЕ", p.muted),
            ("● HUDbar активен", p.accent),
            (concat!("v", env!("CARGO_PKG_VERSION")), p.muted),
            (theme, p.muted),
            (language, p.muted),
        ];
        for (index, (text, color)) in lines.into_iter().enumerate() {
            let y = settings_ui::RAIL_STATUS_Y + index as f32 * 30.0;
            self.painter().paint(
                pixmap,
                text,
                s.caption,
                color,
                Rect::new(rail.x + 18.0, y, rail_w, 22.0),
                Align::Start,
            );
        }
    }

    /// Карточка темы: чекбокс, название и метка «применено» стоят в одной полосе,
    /// описание и шрифт — в двух следующих. Всё центрируется по вертикали.
    fn draw_theme_card(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        pixel: bool,
        rect: Rect,
        s: TypeScale,
    ) {
        let k = SCALE;
        let control = Control::Theme(pixel);
        let applied = self.config().theme
            == Some(if pixel {
                config::Theme::Pixel
            } else {
                config::Theme::Normal
            });
        let selected = self.pixel_mode() == pixel;
        let hovered = self.hovered(control);
        let focused = self.focused(control);
        let (title, description, font) = settings_ui::theme_card(pixel);
        let border = if selected || hovered || focused {
            p.accent
        } else {
            p.border
        };
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(self.pixel_mode(), Radius::LG) * k,
            if selected { p.panel } else { p.idle_panel },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            border,
            if selected || hovered || focused {
                2.0 * k
            } else {
                k
            },
        );

        // Полоса заголовка: чекбокс, имя темы и метка применения — одна строка.
        let title_band = settings_ui::card_title_rect(pixel);
        let box_size = 18.0;
        let box_x = rect.x + 16.0;
        let box_y = title_band.y + (title_band.h - box_size) / 2.0;
        fill_round_rect(
            pixmap,
            box_x * k,
            box_y * k,
            box_size * k,
            box_size * k,
            radius(self.pixel_mode(), Radius::XS) * k,
            if selected { p.accent } else { p.base },
        );
        stroke_rect(
            pixmap,
            box_x * k,
            box_y * k,
            box_size * k,
            box_size * k,
            p.accent,
            k,
        );
        self.painter().paint(
            pixmap,
            title,
            s.section_title + 3.0,
            p.text,
            Rect::new(
                box_x + box_size + 12.0,
                title_band.y,
                title_band.w - box_size - 96.0,
                title_band.h,
            ),
            Align::Start,
        );
        if applied {
            self.painter().paint(
                pixmap,
                "применено",
                s.caption,
                p.accent,
                Rect::new(rect.right() - 104.0, title_band.y, 88.0, title_band.h),
                Align::End,
            );
        }
        self.painter().paint_boxed(
            pixmap,
            description,
            s.caption,
            p.muted,
            settings_ui::card_desc_rect(pixel),
            Align::Start,
        );
        self.painter().paint_boxed(
            pixmap,
            font,
            s.caption,
            p.accent,
            settings_ui::card_font_rect(pixel),
            Align::Start,
        );
    }

    /// Строка модуля: подпись слева, переключатель справа.
    #[allow(clippy::too_many_arguments)]
    fn draw_toggle(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        on: bool,
        hovered: bool,
        focused: bool,
        s: TypeScale,
    ) {
        let k = SCALE;
        if hovered || focused {
            fill_round_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                radius(self.pixel_mode(), Radius::MD) * k,
                p.idle_panel,
            );
        }
        if focused {
            fill_rect(
                pixmap,
                rect.x * k,
                (rect.y + 6.0) * k,
                2.0 * k,
                (rect.h - 12.0) * k,
                p.accent,
            );
        }
        // Переключатель, подпись и «вкл» живут в одной полосе строки, поэтому
        // их центры совпадают по вертикали при любом размере шрифта.
        let switch_w = 38.0;
        let switch_h = 20.0;
        let switch_rect = Rect::new(rect.right() - 12.0 - switch_w, rect.y, switch_w, rect.h);
        let track_y = switch_rect.y + (switch_rect.h - switch_h) / 2.0;
        fill_round_rect(
            pixmap,
            switch_rect.x * k,
            track_y * k,
            switch_w * k,
            switch_h * k,
            radius(self.pixel_mode(), Radius::LG) * k,
            if on { p.accent } else { p.border },
        );
        let knob = 16.0;
        let knob_x = if on {
            switch_rect.right() - knob - 2.0
        } else {
            switch_rect.x + 2.0
        };
        fill_round_rect(
            pixmap,
            knob_x * k,
            (track_y + 2.0) * k,
            knob * k,
            knob * k,
            radius(self.pixel_mode(), Radius::MD) * k,
            if on { p.base } else { p.text.with_a(0.7) },
        );
        self.painter().paint(
            pixmap,
            label,
            s.label,
            if on { p.text } else { p.muted },
            Rect::new(rect.x + 12.0, rect.y, 240.0, rect.h),
            Align::Start,
        );
        self.painter().paint(
            pixmap,
            if on { "вкл" } else { "выкл" },
            s.caption,
            p.muted,
            Rect::new(switch_rect.x - 60.0, rect.y, 48.0, rect.h),
            Align::End,
        );
    }

    /// Строка высоты: дорожка, значение, подпись диапазона и кнопки шага стоят
    /// в одной полосе, поэтому всё лежит на одной линии.
    fn draw_height_scale(&mut self, pixmap: &mut tiny_skia::Pixmap, p: UiPalette, s: TypeScale) {
        let k = SCALE;
        let track = settings_ui::height_track_rect();
        fill_round_rect(
            pixmap,
            track.x * k,
            track.y * k,
            track.w * k,
            track.h * k,
            3.0 * k,
            p.idle_panel,
        );
        let span = settings_ui::HEIGHT_MAX - settings_ui::HEIGHT_MIN;
        let share = (self.config().height - settings_ui::HEIGHT_MIN) as f32 / span as f32;
        fill_round_rect(
            pixmap,
            track.x * k,
            track.y * k,
            (track.w * share).max(6.0) * k,
            track.h * k,
            3.0 * k,
            p.accent,
        );
        let value = self.config().height.to_string();
        self.painter().paint(
            pixmap,
            &value,
            s.value,
            p.text,
            settings_ui::height_value_rect(),
            Align::Center,
        );
        let hint = format!("{}–{} px", settings_ui::HEIGHT_MIN, settings_ui::HEIGHT_MAX);
        self.painter().paint(
            pixmap,
            &hint,
            s.caption,
            p.muted,
            settings_ui::height_hint_rect(),
            Align::Start,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_step(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        hovered: bool,
        focused: bool,
        value_size: f32,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            radius(self.pixel_mode(), Radius::MD) * k,
            if hovered || focused {
                p.panel
            } else {
                p.idle_panel
            },
        );
        if focused {
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                p.accent,
                k,
            );
        }
        self.painter()
            .paint(pixmap, label, value_size, p.text, rect, Align::Center);
    }

    /// Подсказки внизу: промежутки одинаковые, блок центрируется по ширине полосы.
    fn draw_footer_hints(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        y: f32,
        size: f32,
        hints: &[&str],
    ) {
        let band = settings_ui::footer_rect();
        let widths: Vec<f32> = hints
            .iter()
            .map(|hint| self.text_width(hint, size))
            .collect();
        let offsets = settings_ui::spread(band.w, &widths);
        for (hint, offset) in hints.iter().zip(offsets) {
            self.painter().paint(
                pixmap,
                hint,
                size,
                p.muted,
                Rect::new(band.x + offset, y, 400.0, band.h),
                Align::Start,
            );
        }
    }

    /// Кадр целиком: заливка, шапка, сайдбар, строки раздела, статус, подвал.
    /// Буфер отдаётся наружу: живое окно кладёт его в shm, снапшот — в PNG.
    fn render(&mut self, pixmap: &mut tiny_skia::Pixmap) {
        let pw = pixmap.width() as f32;
        let ph = pixmap.height() as f32;
        let k = SCALE;
        let p = ui_palette(self.pixel_mode(), self.palette());
        let pixel = self.pixel_mode();
        let s = type_scale(pixel);
        pixmap.fill(p.base.to_tiny());
        fill_rect(pixmap, 0.0, 0.0, pw, k, p.accent);
        fill_rect(pixmap, 0.0, 0.0, k, ph, p.accent.with_a(0.3));

        self.draw_navigation(pixmap, p, s);

        self.painter().paint(
            pixmap,
            TITLE,
            s.page_title,
            p.text,
            settings_ui::title_rect(),
            Align::Start,
        );

        let status = self.status();
        // строки копируем: рисование берёт &mut self, а обход идёт по self.rows()
        let rows = self.rows().to_vec();
        for row in &rows {
            match row.clone() {
                Row::Header { text, y } => self.painter().paint(
                    pixmap,
                    text,
                    s.section_title,
                    p.text,
                    settings_ui::header_rect(y),
                    Align::Start,
                ),
                Row::Rule { y } => {
                    let w = settings_ui::WIDTH - settings_ui::PAD_X - settings_ui::PAD_R;
                    fill_rect(pixmap, settings_ui::PAD_X * k, y * k, w * k, k, p.border);
                }
                Row::Nav { .. } => {}
                Row::Stub { title, note, y } => {
                    self.painter().paint(
                        pixmap,
                        title,
                        s.section_title + 4.0,
                        p.text,
                        settings_ui::stub_rect(y),
                        Align::Start,
                    );
                    self.painter().paint(
                        pixmap,
                        note,
                        s.label,
                        p.muted,
                        settings_ui::stub_rect(y + 34.0),
                        Align::Start,
                    );
                }
                Row::Theme { pixel, rect } => self.draw_theme_card(pixmap, p, pixel, rect, s),
                Row::Toggle { module, rect } => {
                    let on = self.config().module_enabled(module);
                    let control = Control::Toggle(module);
                    self.draw_toggle(
                        pixmap,
                        p,
                        rect,
                        module.label(),
                        on,
                        self.hovered(control),
                        self.focused(control),
                        s,
                    );
                }
                Row::ZoneLabel {
                    title, hint, rect, ..
                } => {
                    self.painter().paint(
                        pixmap,
                        title,
                        s.label,
                        p.text,
                        Rect::new(rect.x, rect.y, rect.w, 18.0),
                        Align::Start,
                    );
                    self.painter().paint(
                        pixmap,
                        hint,
                        s.caption,
                        p.muted,
                        Rect::new(rect.x, rect.y + 18.0, rect.w, 16.0),
                        Align::Start,
                    );
                }
                Row::ModuleRow { module, line, rect } => {
                    self.draw_module_row(pixmap, p, module, line, rect, s)
                }
                Row::Move {
                    module, dir, rect, ..
                } => {
                    let control = Control::Move(module, dir);
                    self.draw_step(
                        pixmap,
                        p,
                        rect,
                        if dir < 0 { "▲" } else { "▼" },
                        self.hovered(control),
                        self.focused(control),
                        s.caption,
                    );
                }
                Row::HeightLabel { .. } => self.draw_height_scale(pixmap, p, s),
                Row::NotificationFont { dir, rect } => {
                    let label = format!("Размер шрифта: {}", self.config().font_size);
                    self.painter().paint(
                        pixmap,
                        &label,
                        s.label,
                        p.text,
                        Rect::new(settings_ui::PAD_X, rect.y, 260.0, rect.h),
                        Align::Start,
                    );
                    self.draw_step(
                        pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hovered(Control::NotificationFont(dir))
                            || self.focused(Control::NotificationFont(dir)),
                        self.focused(Control::NotificationFont(dir)),
                        s.value,
                    );
                }
                Row::NotificationLineHeight { dir, rect } => {
                    let label = format!("Высота строки: {}", self.config().line_height);
                    self.painter().paint(
                        pixmap,
                        &label,
                        s.label,
                        p.text,
                        Rect::new(settings_ui::PAD_X, rect.y, 260.0, rect.h),
                        Align::Start,
                    );
                    self.draw_step(
                        pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hovered(Control::NotificationLineHeight(dir))
                            || self.focused(Control::NotificationLineHeight(dir)),
                        self.focused(Control::NotificationLineHeight(dir)),
                        s.value,
                    );
                }
                Row::NotificationPosition { position, rect } => {
                    // Выбранный угол подсвечивается так же, как выбранная тема
                    // в «Внешнем виде»: заливка, акцентная рамка и метка.
                    let control = Control::NotificationPosition(position);
                    let selected = self.config().position == position;
                    let active = selected || self.hovered(control) || self.focused(control);
                    self.draw_button(
                        pixmap,
                        p,
                        rect,
                        notification_position_label(position),
                        active,
                        s.caption,
                        pixel,
                    );
                    if selected {
                        let k = SCALE;
                        let mark = Rect::new(rect.right() - 26.0, rect.y, 20.0, rect.h);
                        self.painter()
                            .paint(pixmap, "✓", s.label, p.accent, mark, Align::Center);
                        stroke_rect(
                            pixmap,
                            rect.x * k,
                            rect.y * k,
                            rect.w * k,
                            rect.h * k,
                            p.accent,
                            2.0 * k,
                        );
                    }
                }
                Row::Height { dir, rect } => {
                    let control = Control::Height(dir);
                    self.draw_step(
                        pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hovered(control),
                        self.focused(control),
                        s.value,
                    );
                }
                Row::Status { y } => {
                    let busy = self.busy();
                    self.painter().paint(
                        pixmap,
                        &status,
                        s.value,
                        if busy { p.accent } else { p.muted },
                        settings_ui::header_rect(y),
                        Align::Start,
                    );
                }
                Row::Footer { y } => {
                    let hints = settings_ui::hints(self.nav().section);
                    let hints: Vec<&str> =
                        hints.into_iter().filter(|hint| !hint.is_empty()).collect();
                    self.draw_footer_hints(pixmap, p, y, s.caption, &hints);
                }
                Row::Language {
                    language,
                    rect,
                    selected,
                } => self.draw_language(pixmap, p, language, rect, selected, s, pixel),
                Row::Summary {
                    label,
                    value,
                    rect,
                    section,
                } => self.draw_summary(pixmap, p, rect, label, &value, section, s),
                Row::Hotkey { keys, desc, rect } => {
                    self.draw_hotkey(pixmap, p, rect, &keys, &desc, s);
                }
                Row::MissingBind { desc, rect } => {
                    self.draw_missing_bind(pixmap, p, rect, &desc, s);
                }
                Row::ControlButton { rect } => {
                    self.draw_control_button(pixmap, p, rect, s, pixel);
                }
                Row::WallpaperFile {
                    name,
                    index,
                    rect,
                    selected,
                } => {
                    let label = wallpaper_dir_label(&name);
                    let control = Control::WallpaperFile(index);
                    // Фокус виден всегда: иначе непонятно, к какой строке
                    // сейчас относится Space.
                    let active = self.hovered(control) || self.focused(control);
                    self.draw_wallpaper_file(pixmap, p, rect, &label, selected, active, s);
                }
                Row::Scheme {
                    name,
                    index,
                    rect,
                    selected,
                } => self.draw_scheme(pixmap, p, rect, name, index, selected, s, pixel),
                Row::WallpaperApply { rect, enabled } => {
                    let control = Control::WallpaperApply;
                    let label = if enabled {
                        "Применить обои"
                    } else {
                        "Сначала выберите файл"
                    };
                    let active =
                        enabled && (self.hovered(control) || self.focused(control) || self.busy());
                    self.draw_button_disabled(pixmap, p, rect, label, active, s.label, pixel);
                }
                Row::Close { rect } => {
                    self.draw_button(
                        pixmap,
                        p,
                        rect,
                        "Закрыть",
                        self.hovered(Control::Close) || self.focused(Control::Close),
                        s.caption,
                        pixel,
                    );
                }
            }
        }

        if settings_ui::debug_layout() {
            self.draw_debug_overlay(pixmap);
        }
    }
}

impl<T: View> DrawExt for T {}
