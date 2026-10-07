//! Отрисовка окна выбора обоев. Геометрия живёт в [`super::layout`], здесь
//! только заливка и текст, поэтому тесты раскладки не зависят от пикселей.
//!
//! Стиль берётся из `ui_tokens` и `palette`: Normal мягкие скругления, Pixel —
//! квадратные. Миниатюры пока заглушки: настоящие пиксели подставит W3 через
//! загрузчик кэша превью.

use super::super::palette;
use super::super::settings::Language;
use super::super::settings_icons;
use super::super::settings_ui::Rect;
use super::super::settings_view::{self, fill_rect, fill_round_rect, stroke_rect};
use super::super::settings_widgets::{self, BadgeKind, Ctx};
use super::super::text::{Align, SCALE, TextPainter};
use super::super::ui_tokens::{self, UiPalette};
use super::layout::{self, Area, Zones};
use super::state::{Hover, State, Zone};
use super::thumb;

/// Размер кадра снимка. Живое окно возьмёт столько же.
pub const FRAME_W: f32 = 1200.0;
pub const FRAME_H: f32 = 800.0;
/// Прозрачность карточки в живом окне: сквозь неё видно размытие слоя, как у
/// меню (`CARD_ALPHA` там 0.94); фото чуть плотнее, чтобы не выцветать.
pub const CARD_ALPHA: f32 = 0.97;

/// Всё, чем рисуем кадр: два painter'а (текст и глифы), палитра и шкала.
// Пейнтеры приходят снаружи: `FontSystem::new()` внутри тяжёлый, и живое окно
// держит их весь цикл, а не пересоздаёт каждый кадр.
pub struct Canvas<'a, 'b> {
    pub pixmap: &'a mut tiny_skia::Pixmap,
    pub text: &'b mut TextPainter,
    pub icons: &'b mut TextPainter,
    pub ui: UiPalette,
    pub pixel: bool,
}

impl<'a, 'b> Canvas<'a, 'b> {
    /// Холст с готовыми пейнтерами: снимок создаёт их на кадр, живое окно
    /// держит свои весь цикл.
    pub fn new(
        pixmap: &'a mut tiny_skia::Pixmap,
        text: &'b mut TextPainter,
        icons: &'b mut TextPainter,
        pixel: bool,
        ui: UiPalette,
    ) -> Self {
        Canvas {
            pixmap,
            text,
            icons,
            ui,
            pixel,
        }
    }

    /// Заливка прямоугольника в логических координатах.
    pub fn fill(&mut self, rect: Rect, radius: f32, color: palette::Rgba) {
        let radius = if self.pixel { 0.0 } else { radius };
        fill_round_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            radius * SCALE,
            color,
        );
    }

    /// Рамка толщиной `width` логических пикселей.
    pub fn outline(&mut self, rect: Rect, color: palette::Rgba, width: f32) {
        stroke_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            color,
            width * SCALE,
        );
    }

    /// Текст с многоточием: длинные имена не должны вылезать за плитку.
    pub fn label(&mut self, text: &str, size: f32, color: palette::Rgba, rect: Rect, align: Align) {
        self.text
            .paint_boxed(self.pixmap, text, size, color, rect, align);
    }

    /// Глиф Nerd Font отдельным шрифтом: основной их не содержит.
    fn glyph(&mut self, glyph: &str, size: f32, color: palette::Rgba, rect: Rect) {
        self.icons
            .paint(self.pixmap, glyph, size, color, rect, Align::Center);
    }

    /// Виджетный контекст для готовых примитивов окна настроек.
    fn ctx(&mut self) -> Ctx<'_> {
        Ctx {
            pixmap: self.pixmap,
            painter: self.text,
            palette: self.ui,
            scale: ui_tokens::type_scale(self.pixel),
            pixel: self.pixel,
        }
    }
}

/// Параметры одного кадра, кроме холста: состояние, место карточки и стиль.
/// Собраны в структуру, потому что иначе список аргументов перестаёт читаться.
pub struct Frame<'p> {
    pub state: &'p State,
    pub card: Rect,
    pub pixel: bool,
    pub ui: UiPalette,
    pub lang: Language,
    pub provider: &'p dyn thumb::Provider,
}

/// Кадр целиком: подложка, карточка, зоны, содержимое и подвал.
pub fn render(
    state: &State,
    pixel: bool,
    palette: palette::Palette,
    lang: Language,
) -> tiny_skia::Pixmap {
    render_with(state, pixel, palette, lang, &thumb::Empty)
}

/// Кадр с миниатюрами: провайдер отдаёт готовое из фонового загрузчика, чего
/// нет — рисуется заглушкой. Диска здесь нет ни в каком виде: размеры — из
/// `state.ratios`, цвета схем — из `state.swatch_map`.
pub fn render_with(
    state: &State,
    pixel: bool,
    palette: palette::Palette,
    lang: Language,
    provider: &dyn thumb::Provider,
) -> tiny_skia::Pixmap {
    let ui = ui_tokens::ui_palette(pixel, palette);
    let mut pixmap =
        tiny_skia::Pixmap::new((FRAME_W * SCALE) as u32, (FRAME_H * SCALE) as u32).expect("pixmap");
    backdrop(&mut pixmap, ui);

    let card = Rect::new(
        (FRAME_W - layout::CARD_W) / 2.0,
        (FRAME_H - layout::CARD_H) / 2.0,
        layout::CARD_W,
        layout::CARD_H,
    );
    let mut text = TextPainter::new(&settings_view::font_name(pixel));
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut canvas = Canvas::new(&mut pixmap, &mut text, &mut icons, pixel, ui);
    let frame = Frame {
        state,
        card,
        pixel,
        ui,
        lang,
        provider,
    };
    draw_card(&mut canvas, &frame);
    pixmap
}

/// Кадр живого окна: та же карточка, но от нуля и без подложки — окно и есть
/// карточка, как у меню. Вызывается каждый грязный тик, поэтому тоже без
/// ввода-вывода.
pub fn render_window(
    state: &State,
    pixel: bool,
    palette: palette::Palette,
    lang: Language,
    provider: &dyn thumb::Provider,
) -> tiny_skia::Pixmap {
    let ui = ui_tokens::ui_palette(pixel, palette);
    let mut pixmap = tiny_skia::Pixmap::new(
        (layout::CARD_W * SCALE) as u32,
        (layout::CARD_H * SCALE) as u32,
    )
    .expect("pixmap");
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let card = Rect::new(0.0, 0.0, layout::CARD_W, layout::CARD_H);
    let mut text = TextPainter::new(&settings_view::font_name(pixel));
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut canvas = Canvas::new(&mut pixmap, &mut text, &mut icons, pixel, ui);
    let frame = Frame {
        state,
        card,
        pixel,
        ui,
        lang,
        provider,
    };
    draw_card(&mut canvas, &frame);
    apply_alpha(&mut pixmap, card, CARD_ALPHA);
    pixmap
}

/// Карточка в заданном месте: общая для снимка и живого окна, форка нет.
pub fn draw_card(canvas: &mut Canvas<'_, '_>, frame: &Frame<'_>) {
    let zones = layout::zones(frame.card);
    let ts = ui_tokens::type_scale(frame.pixel);

    // Карточка: в Normal со скруглением, в Pixel — прямоугольник.
    canvas.fill(frame.card, ui_tokens::radii::LG, canvas.ui.panel);
    canvas.outline(frame.card, canvas.ui.border, layout::HAIRLINE);

    header(canvas, frame.state, frame.card, zones, ts, frame.lang);
    zone_frames(canvas, frame.state, zones);
    folders(canvas, frame.state, zones, ts);
    grid(canvas, frame.state, zones, ts, frame.lang, frame.provider);
    footer(canvas, frame.state, frame.card, ts, frame.lang);
}

/// Прозрачность карточки: умножает все каналы, premultiplied от этого не
/// портится — масштабирование сохраняет `rgb <= a`.
pub fn apply_alpha(pixmap: &mut tiny_skia::Pixmap, card: Rect, alpha: f32) {
    let x0 = (card.x * SCALE).round() as i32;
    let y0 = (card.y * SCALE).round() as i32;
    let x1 = ((card.x + card.w) * SCALE).round() as i32;
    let y1 = ((card.y + card.h) * SCALE).round() as i32;
    let stride = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    for py in y0.max(0)..y1.min(height) {
        for px in x0.max(0)..x1.min(stride) {
            let index = (py * stride + px) as usize;
            if let Some(pixel) = pixmap.pixels_mut().get_mut(index) {
                let scale = |channel: u8| (channel as f32 * alpha).round().clamp(0.0, 255.0) as u8;
                *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                    scale(pixel.red()),
                    scale(pixel.green()),
                    scale(pixel.blue()),
                    scale(pixel.alpha()),
                )
                .unwrap_or(*pixel);
            }
        }
    }
}

/// Подложка кадра: небо, силуэты и приглушённый низ, как в M4-снимках.
fn backdrop(pixmap: &mut tiny_skia::Pixmap, ui: UiPalette) {
    let width = pixmap.width();
    let height = pixmap.height();
    let top = palette::Rgba(0x1a, 0x1f, 0x30, 255);
    let bottom = palette::Rgba(0x11, 0x15, 0x24, 255);
    let horizon = palette::Rgba(ui.base.0, ui.base.1, ui.base.2, 255);
    let dim = palette::Rgba(ui.base.0, ui.base.1, ui.base.2, 48);
    let pixels = pixmap.pixels_mut();
    for y in 0..height {
        let t = y as f32 / (height.max(1) - 1).max(1) as f32;
        let row = mix(top, bottom, t);
        let row = if y * 2 > height {
            mix(row, horizon, 0.35)
        } else {
            row
        };
        for x in 0..width {
            if let Some(pixel) = pixels.get_mut(y as usize * width as usize + x as usize) {
                *pixel = blend(*pixel, row);
            }
        }
    }
    for y in (height / 2)..height {
        for x in 0..width {
            if let Some(pixel) = pixels.get_mut(y as usize * width as usize + x as usize) {
                *pixel = blend(*pixel, dim);
            }
        }
    }
}

fn mix(a: palette::Rgba, b: palette::Rgba, t: f32) -> palette::Rgba {
    let t = t.clamp(0.0, 1.0);
    palette::Rgba(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t).round() as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t).round() as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t).round() as u8,
        255,
    )
}

fn blend(
    dst: tiny_skia::PremultipliedColorU8,
    src: palette::Rgba,
) -> tiny_skia::PremultipliedColorU8 {
    let a = src.3 as f32 / 255.0;
    let channel = |source: u8, target: u8| {
        (source as f32 * a + target as f32 * (1.0 - a))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    tiny_skia::PremultipliedColorU8::from_rgba(
        channel(src.0, dst.red()),
        channel(src.1, dst.green()),
        channel(src.2, dst.blue()),
        255,
    )
    .unwrap_or(dst)
}

/// Шапка: крошки, hairline, поле поиска с лупой и крестиком.
fn header(
    canvas: &mut Canvas<'_, '_>,
    state: &State,
    card: Rect,
    zones: Zones,
    ts: ui_tokens::TypeScale,
    lang: Language,
) {
    let section = match lang {
        Language::Ru => super::strings::WALLPAPERS,
        Language::En => super::strings::WALLPAPERS_EN,
    };
    let mut crumbs = format!(
        "{} {} {section}",
        super::strings::CRUMB_ROOT,
        super::strings::CRUMB
    );
    if !state.current_folder.is_empty() {
        crumbs = format!(
            "{crumbs} {} {}",
            super::strings::CRUMB,
            state.current_folder
        );
    }
    canvas.label(
        &crumbs,
        ts.section_title,
        canvas.ui.text,
        Rect::new(card.x + layout::PAD, card.y, card.w * 0.5, layout::HEADER_H),
        Align::Start,
    );

    // Hairline между шапкой и телом.
    canvas.outline(
        Rect::new(card.x, card.y + layout::HEADER_H, card.w, layout::HAIRLINE),
        canvas.ui.border_subtle,
        layout::HAIRLINE,
    );

    search_field(canvas, state, card, zones, ts, lang);
}

fn search_field(
    canvas: &mut Canvas<'_, '_>,
    state: &State,
    card: Rect,
    _zones: Zones,
    ts: ui_tokens::TypeScale,
    lang: Language,
) {
    let field = layout::search(card);
    canvas.fill(field, ui_tokens::radii::SM, canvas.ui.surface_hover);
    canvas.outline(field, canvas.ui.border_subtle, layout::HAIRLINE);

    // Лупа слева: глиф Nerd Font, а не символ текста.
    let icon = layout::CLEAR_BTN;
    canvas.glyph(
        settings_icons::SEARCH,
        ts.label,
        canvas.ui.muted,
        Rect::new(field.x + ui_tokens::spacing::SM, field.y, icon, field.h),
    );

    // Крестик справа — только при непустом запросе.
    let clear_w = if state.query.is_empty() {
        0.0
    } else {
        layout::CLEAR_BTN
    };
    let text_x = field.x + ui_tokens::spacing::SM + icon + ui_tokens::spacing::SM;
    let text_w = field.w - (text_x - field.x) - clear_w - ui_tokens::spacing::SM * 2.0;
    let (value, color) = if state.query.is_empty() {
        (
            match lang {
                Language::Ru => super::strings::SEARCH_PLACEHOLDER,
                Language::En => super::strings::SEARCH_PLACEHOLDER_EN,
            },
            canvas.ui.muted,
        )
    } else {
        (state.query.as_str(), canvas.ui.text)
    };
    canvas.label(
        value,
        ts.label,
        color,
        Rect::new(text_x, field.y, text_w.max(0.0), field.h),
        Align::Start,
    );
    if !state.query.is_empty() {
        let button = layout::search_clear(field);
        canvas.fill(button, ui_tokens::radii::XS, canvas.ui.surface);
        canvas.glyph(settings_icons::TIMES, ts.caption, canvas.ui.muted, button);
    }
}

/// Пункт 9: активная зона — `border_focus`, остальные — `border_subtle`.
fn zone_frames(canvas: &mut Canvas<'_, '_>, state: &State, zones: Zones) {
    for (area, rect) in [(Area::Folders, zones.folders), (Area::Grid, zones.grid)] {
        let active = state.zone == zone_of(area);
        // В Pixel различие держим толщиной и цветом, а не только углом.
        let (color, width) = if active {
            (canvas.ui.border_focus, layout::ZONE_BORDER)
        } else {
            (canvas.ui.border_subtle, layout::HAIRLINE)
        };
        canvas.outline(rect, color, width);
    }
}

fn zone_of(area: Area) -> Zone {
    match area {
        Area::Folders => Zone::Folders,
        Area::Grid => Zone::Grid,
    }
}

/// Пункт 4: колонка каталогов 200 px, имя обрезается, счётчик не теснит.
fn folders(canvas: &mut Canvas<'_, '_>, state: &State, zones: Zones, ts: ui_tokens::TypeScale) {
    let counter_w = 44.0;
    for (index, folder) in state.folders.iter().enumerate() {
        let Some(row) = layout::folder_row(zones, index) else {
            break;
        };
        let selected = index == state.folder_sel;
        if selected {
            canvas.fill(row, ui_tokens::radii::SM, canvas.ui.surface_hover);
            // Акцент-полоса слева: выбранная папка видна и без рамки зоны.
            let bar = Rect::new(row.x, row.y, 3.0, row.h);
            fill_rect(
                canvas.pixmap,
                bar.x * SCALE,
                bar.y * SCALE,
                bar.w * SCALE,
                bar.h * SCALE,
                canvas.ui.accent,
            );
        } else if state.hover == Hover::Folder(index) {
            // Hover только подсвечивает: содержимое не переключает.
            canvas.fill(row, ui_tokens::radii::SM, canvas.ui.surface);
        }
        // «all» — не папка, поэтому другой глиф.
        let icon = if folder.name == super::scan::ALL {
            settings_icons::TH_LARGE
        } else {
            settings_icons::FOLDER
        };
        let icon_rect = Rect::new(
            row.x + ui_tokens::spacing::SM,
            row.y,
            layout::CLEAR_BTN,
            row.h,
        );
        let text_color = if selected {
            canvas.ui.text
        } else {
            canvas.ui.muted
        };
        canvas.glyph(icon, ts.label, text_color, icon_rect);

        // Имя получает всю остаток до счётчика, счётчик стоит на своём месте.
        let name_x = icon_rect.right() + ui_tokens::spacing::SM;
        let name_w = row.right() - counter_w - ui_tokens::spacing::SM - name_x;
        canvas.label(
            folder.name.as_str(),
            ts.label,
            text_color,
            Rect::new(name_x, row.y, name_w.max(0.0), row.h),
            Align::Start,
        );
        canvas.label(
            &folder.count.to_string(),
            ts.caption,
            canvas.ui.muted,
            Rect::new(
                row.right() - counter_w,
                row.y,
                counter_w - ui_tokens::spacing::SM,
                row.h,
            ),
            Align::End,
        );
    }
}

/// Пункт 7 и 8: сетка миниатюр, выделение и подписи.
fn grid(
    canvas: &mut Canvas<'_, '_>,
    state: &State,
    zones: Zones,
    ts: ui_tokens::TypeScale,
    lang: Language,
    provider: &dyn thumb::Provider,
) {
    if state.filtered.is_empty() {
        let inner = Rect::new(
            zones.grid.x + layout::ZONE_INSET,
            zones.grid.y + layout::ZONE_INSET,
            zones.grid.w - layout::ZONE_INSET * 2.0,
            zones.grid.h - layout::ZONE_INSET * 2.0,
        );
        let text = match lang {
            Language::Ru => super::strings::NOTHING_FOUND,
            Language::En => super::strings::NOTHING_FOUND_EN,
        };
        canvas.label(text, ts.label, canvas.ui.muted, inner, Align::Center);
        return;
    }

    for row in 0..layout::GRID_ROWS {
        for col in 0..layout::GRID_COLS {
            let index = (state.grid.top_row + row) * layout::GRID_COLS + col;
            if index >= state.filtered.len() {
                break;
            }
            let Some(thumb) = layout::cell(zones, row, col) else {
                continue;
            };
            let path = &state.filtered[index];
            let current = state.current_wallpaper.as_deref() == Some(path.as_path());
            let selected = state.zone == Zone::Grid && index == state.grid.sel;

            let image = provider.get(path);
            match image.as_ref() {
                Some(image) => blit_thumb(canvas, thumb, image),
                None => placeholder(canvas, index, thumb, false),
            }

            // Текущие обои видны в любом случае, выделение — поверх рамки.
            if current {
                let badge = layout::active_badge(thumb);
                let mut ctx = canvas.ctx();
                settings_widgets::badge(
                    &mut ctx,
                    badge,
                    super::strings::BADGE_ACTIVE,
                    BadgeKind::Accent,
                );
            }

            if selected {
                let tick = layout::tick(thumb);
                canvas.fill(tick, layout::CLEAR_BTN / 2.0, canvas.ui.border_focus);
                canvas.glyph(settings_icons::CHECK, ts.caption, canvas.ui.text, tick);
            }
            // Рамка: у выбранной — акцентная и толще; hover — обычной, но
            // заметнее заглушечной hairline.
            if image.is_some() {
                let (color, width) = if selected {
                    (canvas.ui.border_focus, layout::ZONE_BORDER)
                } else if state.hover == Hover::Cell(index) {
                    (canvas.ui.border, layout::HAIRLINE)
                } else {
                    (canvas.ui.border_subtle, layout::HAIRLINE)
                };
                canvas.outline(thumb, color, width);
            }
            caption(canvas, state, path, thumb, ts);
        }
    }
}

/// Блитит миниатюру в плитку с масштабированием ближайшим соседом: исходник
/// уже ужатая, кадр — дешёвый. Пиксели непрозрачные, поэтому прямая запись
/// вместо `draw_pixmap` — и без новых зависимостей.
fn blit_thumb(canvas: &mut Canvas<'_, '_>, rect: Rect, thumb: &thumb::Thumb) {
    if thumb.w == 0 || thumb.h == 0 || thumb.rgba.len() < (thumb.w * thumb.h * 4) as usize {
        return;
    }
    let x0 = (rect.x * SCALE).round() as i32;
    let y0 = (rect.y * SCALE).round() as i32;
    let dw = (rect.w * SCALE).round() as i32;
    let dh = (rect.h * SCALE).round() as i32;
    if dw <= 0 || dh <= 0 {
        return;
    }
    let stride = canvas.pixmap.width() as i32;
    let height = canvas.pixmap.height() as i32;
    for dy in 0..dh {
        let src_y = (dy as u32 * thumb.h / dh as u32).min(thumb.h - 1);
        let py = y0 + dy;
        if py < 0 || py >= height {
            continue;
        }
        for dx in 0..dw {
            let src_x = (dx as u32 * thumb.w / dw as u32).min(thumb.w - 1);
            let px = x0 + dx;
            if px < 0 || px >= stride {
                continue;
            }
            let src = ((src_y * thumb.w + src_x) * 4) as usize;
            let dst = (py * stride + px) as usize;
            if let Some(pixel) = canvas.pixmap.pixels_mut().get_mut(dst) {
                *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                    thumb.rgba[src],
                    thumb.rgba[src + 1],
                    thumb.rgba[src + 2],
                    thumb.rgba[src + 3],
                )
                .unwrap_or(*pixel);
            }
        }
    }
}

/// Заглушка миниатюры: градиент разных тонов по индексу.
fn placeholder(canvas: &mut Canvas<'_, '_>, index: usize, rect: Rect, selected: bool) {
    let base = mix(
        rgb(0x18, 0x1f, 0x30),
        rgb(0x34, 0x3e, 0x5e),
        (index % 12) as f32 / 11.0,
    );
    let color = mix(base, rgb(0x60, 0x70, 0xa0), ((index + 2) % 5) as f32 / 4.0);
    canvas.fill(rect, ui_tokens::radii::SM, color);

    // Полупрозрачная диагональ, чтобы плитка читалась как изображение.
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(rect.x * SCALE, rect.y * SCALE);
    path.line_to(rect.right() * SCALE, rect.y * SCALE);
    path.line_to(rect.right() * SCALE, (rect.y + rect.h * 0.45) * SCALE);
    path.line_to(rect.x * SCALE, rect.bottom() * SCALE);
    path.close();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(color.with_a(0.55).to_tiny());
    if let Some(path) = path.finish() {
        canvas.pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    // Рамка: у выбранной — акцентная и толще.
    let (color, width) = if selected {
        (canvas.ui.border_focus, layout::ZONE_BORDER)
    } else {
        (canvas.ui.border_subtle, layout::HAIRLINE)
    };
    canvas.outline(rect, color, width);
}

/// Пункт 8: имя слева, ratio справа, обе подписи внутри плитки. Размеры —
// из предзагруженной карты состояния: рендер диска не касается.
fn caption(
    canvas: &mut Canvas<'_, '_>,
    state: &State,
    path: &std::path::Path,
    thumb: Rect,
    ts: ui_tokens::TypeScale,
) {
    let label = layout::cell_label(thumb);
    let ratio = state.ratio_label(path);
    // Резерв под ratio фиксирован, имя обрезается с многоточием.
    let ratio_w = layout::RATIO_RESERVE;
    let name_w = (label.w - ratio_w - ui_tokens::spacing::SM).max(0.0);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    canvas.label(
        name,
        ts.caption,
        canvas.ui.text,
        Rect::new(label.x, label.y, name_w, label.h),
        Align::Start,
    );
    canvas.label(
        &ratio,
        ts.caption,
        canvas.ui.muted,
        Rect::new(label.right() - ratio_w, label.y, ratio_w, label.h),
        Align::End,
    );
}
fn footer(
    canvas: &mut Canvas<'_, '_>,
    state: &State,
    card: Rect,
    ts: ui_tokens::TypeScale,
    lang: Language,
) {
    let top = card.y + layout::CARD_H - layout::FOOTER_H;
    canvas.outline(
        Rect::new(card.x, top, card.w, layout::HAIRLINE),
        canvas.ui.border_subtle,
        layout::HAIRLINE,
    );
    let mut band = Rect::new(
        card.x + layout::PAD,
        top,
        card.w - layout::PAD * 2.0,
        layout::FOOTER_H,
    );
    for hint in super::strings::hints(matches!(lang, Language::Ru)) {
        let w = canvas.text.text_width(hint, ts.caption) + ui_tokens::spacing::XL;
        canvas.label(
            hint,
            ts.caption,
            canvas.ui.muted,
            Rect::new(band.x, band.y, w, band.h),
            Align::Start,
        );
        band.x += w;
    }
    let counter = if state.filtered.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", state.grid.sel + 1, state.filtered.len())
    };
    canvas.label(
        &counter,
        ts.caption,
        canvas.ui.text,
        Rect::new(band.right() - 80.0, band.y, 80.0, band.h),
        Align::End,
    );
}

/// Цвета схемы: из файла, иначе встроенный образец (генерация — W4).
pub fn parse_hex(text: &str) -> Option<palette::Rgba> {
    let text = text.strip_prefix('#')?;
    if text.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    Some(palette::Rgba(
        (value >> 16) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
        255,
    ))
}

fn rgb(r: u8, g: u8, b: u8) -> palette::Rgba {
    palette::Rgba(r, g, b, 255)
}

#[cfg(test)]
mod tests {
    use super::super::super::settings::Language;
    use super::*;

    /// Пункт 5: глифы лупы и крестика есть в шрифте иконок, иначе будет «тофу».
    #[test]
    fn search_and_clear_glyphs_are_declared() {
        // `const`-блок: проверка на константах выполняется при сборке, а не
        // в рантайме теста. Раньше это был runtime-assert на &'static str.
        const { assert!(!settings_icons::SEARCH.is_empty()) };
        const { assert!(!settings_icons::TIMES.is_empty()) };
        // Сравнение строк в const пока нестабильно, остаётся в рантайме.
        assert_ne!(settings_icons::SEARCH, settings_icons::TIMES);
    }

    /// Пункт 4: у папки и «all» разные глифы, и оба не пустые.
    #[test]
    fn folder_glyphs_differ_and_are_not_empty() {
        const { assert!(!settings_icons::FOLDER.is_empty()) };
        const { assert!(!settings_icons::TH_LARGE.is_empty()) };
        assert_ne!(settings_icons::FOLDER, settings_icons::TH_LARGE);
    }

    /// Пункт 10 в терминах пикселей: длинное имя обрезается, ratio остаётся.
    #[test]
    fn long_name_is_truncated_and_ratio_still_fits() {
        let mut pixmap = tiny_skia::Pixmap::new(2400, 1600).expect("pixmap");
        let ui = ui_tokens::ui_palette(false, palette::Palette::default());
        let ts = ui_tokens::type_scale(false);
        let mut text = TextPainter::new(&settings_view::font_name(false));
        let mut icons = TextPainter::new(settings_icons::FONT);
        let canvas = Canvas::new(&mut pixmap, &mut text, &mut icons, false, ui);
        let card = Rect::new(
            (FRAME_W - layout::CARD_W) / 2.0,
            (FRAME_H - layout::CARD_H) / 2.0,
            layout::CARD_W,
            layout::CARD_H,
        );
        let zones = layout::zones(card);
        let thumb = layout::cell(zones, 0, 0).expect("плитка");
        let label = layout::cell_label(thumb);
        let long = "очень-очень-длинное-имя-файла-которое-не-влезает.png";
        let full = canvas.text.text_width(long, ts.caption);
        let name_w = label.w - layout::RATIO_RESERVE - ui_tokens::spacing::SM;
        assert!(
            full > name_w,
            "имя должно упереться в обрезку, а не влезать"
        );
        let ratio = "21:9";
        let ratio_w = canvas.text.text_width(ratio, ts.caption);
        assert!(
            ratio_w <= layout::RATIO_RESERVE,
            "ratio не влезает в резерв"
        );
        // Обе подписи остаются внутри плитки.
        assert!(label.x + name_w <= label.right() - layout::RATIO_RESERVE);
    }

    /// Пункт 7: при открытии окна выбранные обои — текущие, и они попадают на
    /// видимую страницу. Иначе бейдж ACTIVE в снимке не проверить.
    #[test]
    fn current_wallpaper_is_selected_and_visible_on_open() {
        let state = super::super::state::State::open();
        let Some(current) = state.current_wallpaper.as_ref() else {
            // Без файла ~/.config/wallpaper проверять нечего.
            return;
        };
        assert!(
            state
                .filtered
                .iter()
                .any(|path| path.as_path() == current.as_path()),
            "текущие обои есть в списке выбранной папки"
        );
        assert_eq!(
            state.grid.selected(),
            state
                .filtered
                .iter()
                .position(|path| path.as_path() == current.as_path())
                .expect("индекс текущих обоев"),
            "при открытии выбран текущий файл"
        );
        assert!(
            state.grid.visible_cells().contains(&state.grid.selected()),
            "текущие обои должны быть на видимой странице"
        );
    }

    /// Пункт 9: зоны не наезжают друг на друга и не залезают в подвал.
    #[test]
    fn zones_do_not_overlap_each_other() {
        let card = Rect::new(0.0, 0.0, layout::CARD_W, layout::CARD_H);
        let z = layout::zones(card);
        assert!(
            z.folders.right() <= z.grid.x,
            "колонка папок заходит в сетку"
        );
        assert!(
            z.grid.bottom() <= card.y + layout::CARD_H - layout::FOOTER_H,
            "сетка заходит в подвал"
        );
    }

    /// Пункт 1: содержимое зон отделено от рамки отступом MD.
    #[test]
    fn zone_content_is_inset_from_the_frame() {
        let card = Rect::new(0.0, 0.0, layout::CARD_W, layout::CARD_H);
        let z = layout::zones(card);
        assert_eq!(layout::ZONE_INSET, ui_tokens::spacing::MD);
        let row = layout::folder_row(z, 0).unwrap();
        assert!(row.y - z.folders.y >= layout::ZONE_INSET);
        let thumb = layout::cell(z, 0, 0).unwrap();
        assert!(thumb.y - z.grid.y >= layout::ZONE_INSET);
    }

    /// Снимок обеих тем рисуется без паники: значит координаты в границах.
    #[test]
    fn render_survives_both_themes_and_both_zones() {
        for pixel in [false, true] {
            for zone in [Zone::Folders, Zone::Grid] {
                let mut state = super::super::state::State::open();
                state.zone = zone;
                let pixmap = render(&state, pixel, palette::Palette::default(), Language::Ru);
                assert!(pixmap.width() > 0 && pixmap.height() > 0);
            }
        }
    }

    /// Живой кадр — ровно карточка, миниатюры провайдера ложатся без паники.
    #[test]
    fn window_frame_matches_the_card_and_uses_thumbs() {
        use std::path::Path;
        struct One(thumb::Thumb);
        impl thumb::Provider for One {
            fn get(&self, _path: &Path) -> Option<thumb::Thumb> {
                Some(self.0.clone())
            }
        }
        let state = super::super::state::State::open();
        let provider = One(thumb::Thumb::solid(16, 16, [40, 80, 160, 255]));
        for pixel in [false, true] {
            let pixmap = render_window(
                &state,
                pixel,
                palette::Palette::default(),
                Language::Ru,
                &provider,
            );
            assert_eq!(pixmap.width(), (layout::CARD_W * SCALE) as u32);
            assert_eq!(pixmap.height(), (layout::CARD_H * SCALE) as u32);
        }
    }
}
