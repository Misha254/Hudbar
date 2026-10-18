//! Рисование по пиксельной карте: примитивы, контекст виджета и запись холста.
//!
//! Всё это лежало в `settings_view` и `settings_widgets` — остатках снятого
//! окна настроек, — хотя пользовались ими живые окна: меню, обои, приложения.
//! Здесь только то, что действительно вызывается: четыре примитива, контекст
//! рисования, «таблетка» статуса и запись снимка в PNG.

use super::geom::Rect;
use super::palette;
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::{TypeScale, UiPalette, radii, radius, spacing};

/// Заливка прямоугольника без сглаживания: координаты в пикселях буфера.
pub fn fill_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: palette::Rgba,
) {
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

/// Заливка прямоугольника со скруглением; радиус обрезается половиной стороны.
pub fn fill_round_rect(
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

/// Круг без сглаживания: окружность строится построчно, каждый ряд — целый
/// отрезок.
///
/// `fill_round_rect` всегда рисует со сглаживанием, а в теме Pixel полутонов
/// быть не должно: восемнадцать уникальных цветов в тумблере 16×16 означали
/// ровно то, чего в этой теме быть не может. Форма остаётся кругом — квадрат
/// выдавал бы переключатель за чекбокс, — но край идёт ступенькой.
///
/// Координаты и радиус — в пикселях буфера, как у `fill_rect`.
pub fn fill_disc_hard(
    pixmap: &mut tiny_skia::Pixmap,
    cx: f32,
    cy: f32,
    r: f32,
    color: palette::Rgba,
) {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let cx = cx.round() as i32;
    let cy = cy.round() as i32;
    let radius = r.max(1.0);
    for dy in -(radius as i32)..=(radius as i32) {
        let y = cy + dy;
        if y < 0 || y >= height {
            continue;
        }
        let squared = radius * radius - (dy * dy) as f32;
        if squared <= 0.0 {
            continue;
        }
        let half = squared.sqrt().floor() as i32;
        let x0 = (cx - half).clamp(0, width);
        let x1 = (cx + half).clamp(0, width);
        if x1 > x0 {
            fill_rect(pixmap, x0 as f32, y as f32, (x1 - x0) as f32, 1.0, color);
        }
    }
}

/// Рамка в 1 px и толще — четырьмя полосами, без сглаживания.
pub fn stroke_rect(
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

/// Запись холста в PNG — одна на все снимки: меню, приложения и окно обоев.
///
/// Копий было три, и различались они только текстом ошибок; теперь «не создался
/// файл» читается одинаково, где бы снимок ни писался. Буфер непрозрачный,
/// поэтому альфа идёт как есть.
pub fn write_png(pixmap: &tiny_skia::Pixmap, path: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::create(path)
        .map_err(|error| format!("не создался {}: {error}", path.display()))?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, pixmap.width(), pixmap.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png = encoder
        .write_header()
        .map_err(|error| format!("PNG: {error}"))?;
    png.write_image_data(pixmap.data())
        .map_err(|error| format!("PNG: {error}"))
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
}

impl<'a> Ctx<'a> {
    /// Заливка со скруглением по теме.
    fn fill(&mut self, rect: Rect, token: f32, color: palette::Rgba) {
        fill_round_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            radius(token) * SCALE,
            color,
        );
    }

    /// Подпись виджета: текст в заданной полосе.
    pub fn paint(&mut self, text: &str, size: f32, color: palette::Rgba, rect: Rect, align: Align) {
        self.painter
            .paint(self.pixmap, text, size, color, rect, align);
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

#[cfg(test)]
mod tests {
    use super::super::palette::{Palette, Rgba};
    use super::super::ui_tokens::{font_name, type_scale, ui_palette};
    use super::*;

    /// Кадр с одной «таблеткой» на чёрном фоне.
    fn draw(kind: BadgeKind) -> tiny_skia::Pixmap {
        let mut pixmap = tiny_skia::Pixmap::new(120, 40).expect("буфер");
        pixmap.fill(Rgba(0, 0, 0, 255).to_tiny());
        let mut painter = TextPainter::new(&font_name());
        let mut ctx = Ctx {
            pixmap: &mut pixmap,
            painter: &mut painter,
            palette: ui_palette(Palette::default()),
            scale: type_scale(),
        };
        badge(&mut ctx, Rect::new(4.0, 4.0, 100.0, 28.0), "ГОТОВО", kind);
        pixmap
    }

    /// «Таблетка» — живой виджет: её рисует окно обоев, поэтому она должна
    /// работать в обеих темах и различать акцентный вид от нейтрального.
    #[test]
    fn badge_paints_in_both_themes_and_kinds() {
        for pixel in [false, true] {
            let accent = draw(BadgeKind::Accent);
            let neutral = draw(BadgeKind::Neutral);
            assert!(
                accent.data().iter().any(|byte| *byte != 0),
                "таблетка нарисована: pixel={pixel}"
            );
            assert_ne!(
                accent.data(),
                neutral.data(),
                "виды различаются: pixel={pixel}"
            );
        }
    }

    /// Снимок читается обратно: запись кладёт RGBA как есть, и тем же форматом
    /// живут миниатюры обоев.
    #[test]
    fn a_written_png_reads_back_as_the_same_pixels() {
        let dir = std::env::temp_dir().join(format!("hudbar-canvas-png-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог теста");
        let path = dir.join("frame.png");

        let mut pixmap = tiny_skia::Pixmap::new(3, 2).expect("буфер");
        pixmap.fill(Rgba(0x12, 0x34, 0x56, 255).to_tiny());
        write_png(&pixmap, &path).expect("запись png");

        let bytes = std::fs::read(&path).expect("файл снимка");
        let reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .expect("заголовок png");
        let mut reader = reader;
        let mut buf = vec![0; reader.output_buffer_size().expect("размер кадра")];
        let info = reader.next_frame(&mut buf).expect("кадр png");

        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(buf.as_slice(), pixmap.data(), "пиксели те же");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
