//! Текст окна настроек: измерение настоящих границ глифов и центрирование.
//!
//! Раньше высота текстового блока бралась из заданной высоты строки
//! (`size + 8`), а базовая линия считалась как `rect.y + (rect.h - (ascent +
//! descent)) / 2 + ascent`. В `descent` попадал межстрочный интервал, поэтому
//! блок получался выше реальных пикселей и все подписи садились ниже своих
//! элементов на 15–20 px.
//!
//! Здесь высота не угадывается: `shape` возвращает границы именно тех пикселей,
//! которые будут залиты, и базовая линия подбирается так, чтобы центр этого
//! блока совпал с центром полосы. Поэтому подпись не может «уехать» ни от
//! переключателя, ни от рамки.

use super::palette;
use super::settings_ui::Rect;
use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};

/// Физических пикселей на логический. Единственное число на всё окно: раскладка
/// считается в физических пикселях, а геометрия — в логических, и перевод
/// делается только здесь.
pub const SCALE: f32 = 2.0;

/// Горизонтальное выравнивание текста внутри отведённой полосы.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// Измеренный блок текста в логических пикселях.
pub struct TextBox {
    pub width: f32,
    /// Верх залитых пикселей относительно базовой линии (обычно отрицателен).
    pub ink_top: f32,
    /// Низ залитых пикселей относительно базовой линии.
    pub ink_bottom: f32,
}

/// Владеет шрифтом и кэшем раскладки; сам измеряет и рисует текст.
pub struct TextPainter {
    font_system: FontSystem,
    swash: SwashCache,
    font: String,
}

impl TextPainter {
    pub fn new(font: &str) -> Self {
        Self {
            font_system: FontSystem::new(),
            swash: SwashCache::new(),
            font: font.to_string(),
        }
    }

    pub fn set_font(&mut self, font: &str) {
        self.font = font.to_string();
    }

    /// Раскладывает текст и измеряет реальные границы глифей относительно
    /// базовой линии. Метрики шрифта не участвуют в вычислении центра: берутся
    /// только те пиксели, которые действительно будут нарисованы.
    fn shape(&mut self, text: &str, size: f32) -> (TextBox, Vec<(CacheKey, i32, i32)>) {
        let metrics = Metrics::new(size * SCALE, size * SCALE);
        let mut buffer = Buffer::new_empty(metrics);
        let attrs = Attrs::new().family(Family::Name(&self.font));
        let mut width = 0.0f32;
        let mut glyphs = Vec::new();
        {
            let mut b = buffer.borrow_with(&mut self.font_system);
            b.set_text(text, &attrs, Shaping::Advanced, None);
            b.set_size(None, None);
            b.shape_until_scroll(false);
            for run in b.layout_runs() {
                width = width.max(run.line_w);
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, run.line_y), 1.0);
                    glyphs.push((physical.cache_key, physical.x, physical.y));
                }
            }
        }
        // Кэш глифов смотрим уже вне borrow буфера: обе структуры живут в
        // одном FontSystem.
        let mut top = f32::INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for (key, _, y) in &glyphs {
            if let Some(image) = self.swash.get_image_uncached(&mut self.font_system, *key)
                && matches!(image.content, SwashContent::Mask)
            {
                let top_edge = *y as f32 - image.placement.top as f32;
                let bottom_edge = top_edge + image.placement.height as f32;
                top = top.min(top_edge);
                bottom = bottom.max(bottom_edge);
            }
        }
        let measured = if top.is_finite() && bottom > top {
            TextBox {
                width: width / SCALE,
                ink_top: top / SCALE,
                ink_bottom: bottom / SCALE,
            }
        } else {
            // Пустая строка или шрифт без глифов: типовые пропорции заглавной.
            TextBox {
                width: width / SCALE,
                ink_top: -size * 0.72,
                ink_bottom: size * 0.2,
            }
        };
        (measured, glyphs)
    }

    pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
        self.shape(text, size).0.width
    }

    /// Базовая линия, при которой центр залитых пикселей совпадает с центром
    /// полосы. Единственная формула позиционирования во всём окне.
    pub fn baseline(&self, rect: Rect, box_: &TextBox) -> f32 {
        rect.y + rect.h / 2.0 - (box_.ink_top + box_.ink_bottom) / 2.0
    }

    /// Рисует текст в полосе: по вертикали — по центру измеренных пикселей,
    /// по горизонтали — согласно `align`.
    pub fn paint(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        rect: Rect,
        align: Align,
    ) {
        let (box_, glyphs) = self.shape(text, size);
        let baseline = self.baseline(rect, &box_);
        let x = match align {
            Align::Start => rect.x,
            Align::Center => rect.x + (rect.w - box_.width) / 2.0,
            Align::End => rect.right() - box_.width,
        };
        self.blend(pixmap, glyphs, color, x, baseline);
    }

    /// Как `paint`, но обрезает текст с многоточием, если он не влезает.
    pub fn paint_boxed(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        rect: Rect,
        align: Align,
    ) {
        let mut shown = text.to_string();
        if self.text_width(&shown, size) > rect.w {
            while !shown.is_empty() {
                let candidate = format!("{shown}…");
                if self.text_width(&candidate, size) <= rect.w {
                    shown = candidate;
                    break;
                }
                shown.pop();
            }
        }
        self.paint(pixmap, &shown, size, color, rect, align);
    }

    fn blend(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        glyphs: Vec<(CacheKey, i32, i32)>,
        color: palette::Rgba,
        x: f32,
        baseline: f32,
    ) {
        for (key, gx, gy) in glyphs {
            if let Some(image) = self.swash.get_image_uncached(&mut self.font_system, key)
                && matches!(image.content, SwashContent::Mask)
            {
                // Логические координаты умножаем на SCALE один раз, глифы уже
                // пришли в физических пикселях.
                blend_pixels(
                    pixmap,
                    (x * SCALE) as i32 + gx,
                    (baseline * SCALE) as i32 + gy - image.placement.top,
                    image.placement.width,
                    image.placement.height,
                    color,
                    &image.data,
                );
            }
        }
    }
}

/// Заливка маски глифа с альфой в пиксели.
pub fn blend_pixels(
    pixmap: &mut tiny_skia::Pixmap,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: palette::Rgba,
    mask: &[u8],
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let xx = x + col;
            let yy = y + row;
            if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                continue;
            }
            let alpha = mask[(row * w as i32 + col) as usize] as u32 * color.3 as u32 / 255;
            if alpha == 0 {
                continue;
            }
            let dst = &mut pixels[(yy * pw + xx) as usize];
            let inv = 255 - alpha;
            let nr = (color.0 as u32 * alpha / 255 + dst.red() as u32 * inv / 255).min(255) as u8;
            let ng = (color.1 as u32 * alpha / 255 + dst.green() as u32 * inv / 255).min(255) as u8;
            let nb = (color.2 as u32 * alpha / 255 + dst.blue() as u32 * inv / 255).min(255) as u8;
            let na = (alpha + dst.alpha() as u32 * inv / 255).min(255) as u8;
            if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na) {
                *dst = px;
            }
        }
    }
}
