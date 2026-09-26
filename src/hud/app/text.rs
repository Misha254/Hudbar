use super::{App, ui::*};

use cosmic_text::{Attrs, Buffer, CacheKey, Family, Metrics, Shaping, SwashContent};

use crate::hud::palette;

pub trait TextRenderer {
    fn layout(&mut self, text: &str, size: f32, line_h: f32) -> (f32, Vec<(CacheKey, i32, i32)>);
    fn paint(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        glyphs: &[(CacheKey, i32, i32)],
        x: f32,
        y: f32,
        color: palette::Rgba,
    );
    fn text_width(&mut self, text: &str, size: f32) -> f32;
    fn icon_gap(&self) -> &'static str;
    fn theme(&self) -> Theme;
    fn pixel_mode(&self) -> bool;
    fn bar_palette(&self) -> palette::Palette;
    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        x: f32,
        y: f32,
        line_h: f32,
    ) -> f32;
    #[allow(clippy::too_many_arguments)]
    fn draw_text_boxed(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        max_w: f32,
        x: f32,
        y: f32,
        line_h: f32,
    );
}

impl TextRenderer for App {
    fn layout(&mut self, text: &str, size: f32, line_h: f32) -> (f32, Vec<(CacheKey, i32, i32)>) {
        let mut buffer = Buffer::new_empty(Metrics::new(size * SCALE, line_h * SCALE));
        let attrs = Attrs::new().family(Family::Name(&self.font));
        let mut width = 0f32;
        let mut glyphs = Vec::new();
        {
            let mut b = buffer.borrow_with(&mut self.font_system);
            b.set_text(text, &attrs, Shaping::Advanced, None);
            b.set_size(None, None);
            b.shape_until_scroll(false);
            for run in b.layout_runs() {
                width = width.max(run.line_w);
                for g in run.glyphs {
                    let pg = g.physical((0., run.line_y), 1.0);
                    glyphs.push((pg.cache_key, pg.x, pg.y));
                }
            }
        }
        (width, glyphs)
    }

    fn paint(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        glyphs: &[(CacheKey, i32, i32)],
        x: f32,
        y: f32,
        color: palette::Rgba,
    ) {
        for (key, gx, gy) in glyphs {
            if let Some(img) = self.swash.get_image_uncached(&mut self.font_system, *key)
                && matches!(img.content, SwashContent::Mask)
            {
                blend(
                    pixmap,
                    x as i32 + gx + img.placement.left,
                    y as i32 + gy - img.placement.top,
                    img.placement.width,
                    img.placement.height,
                    color,
                    &img.data,
                    self.pixel_mode(),
                );
            }
        }
    }

    fn text_width(&mut self, text: &str, size: f32) -> f32 {
        let h = self.height as f32;
        self.layout(text, size, h).0
    }

    fn icon_gap(&self) -> &'static str {
        self.theme().icon_gap
    }

    fn theme(&self) -> Theme {
        if self.pixel_mode() {
            Theme::pixel()
        } else {
            Theme::normal()
        }
    }

    fn pixel_mode(&self) -> bool {
        self.font.contains("Minecraft Rus")
    }

    fn bar_palette(&self) -> palette::Palette {
        if self.pixel_mode() {
            palette::Palette {
                base: PIXEL_BASE,
                text: PIXEL_TEXT,
                primary: PIXEL_HIGHLIGHT,
                on_primary: PIXEL_ACTIVE_TEXT,
                secondary: PIXEL_MUTED,
                error: PIXEL_WARNING,
            }
        } else {
            self.palette
        }
    }

    fn draw_text(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        x: f32,
        y: f32,
        line_h: f32,
    ) -> f32 {
        let (w, glyphs) = self.layout(text, size, line_h);
        self.paint(pixmap, &glyphs, x, y, color);
        w
    }

    fn draw_text_boxed(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        max_w: f32,
        x: f32,
        y: f32,
        line_h: f32,
    ) {
        let (w, glyphs) = self.layout(text, size, line_h);
        if w <= max_w {
            self.paint(pixmap, &glyphs, x, y, color);
            return;
        }
        let mut cut = text.to_string();
        while !cut.is_empty() {
            cut.pop();
            let (cw, _) = self.layout(&cut, size, line_h);
            if cw <= max_w - self.text_width("\u{2026}", size) {
                break;
            }
        }
        let shown = format!("{cut}\u{2026}");
        let (_, glyphs) = self.layout(&shown, size, line_h);
        self.paint(pixmap, &glyphs, x, y, color);
    }
}
