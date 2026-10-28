//! Фон экрана входа и уменьшение обоев для `--sync`.
//!
//! В каталоге данных лежит уже готовый `background.png`: экран входа читает
//! только PNG и только своим декодером из `png`-крейта. JPEG/WebP при
//! синхронизации конвертируются тем же способом, что и миниатюры окна обоев —
//! `vipsthumbnail` или `magick`, argv-вызовом без shell. Новых крейтов это не
//! требует: PNG-путь работает на своём декодере даже там, где ImageMagick не
//! установлен.
//!
//! Картинка вписывается в кадр по `cover`: пропорции сохраняются, лишнее
//! обрезается по краям, центр совпадает с центром экрана.

use std::path::Path;
use std::process::Command;

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// Предел ширины фона в каталоге данных. Больше не нужно: экран входа
/// показывает картинку затемнённой и без мелких деталей, а 1920 по ширине
/// держит файл в мегабайтах, а не в десятках.
pub const MAX_WIDTH: u32 = 1920;

/// Как вписать картинку в кадр: `(масштаб, сдвиг по x, сдвиг по y)`.
///
/// `None`, если размеры нулевые. Масштаб — максимальный из двух отношений,
/// поэтому картинка накрывает кадр целиком, а не оставляет полос по краям.
pub fn cover(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Option<(f32, f32, f32)> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return None;
    }
    let scale = (dst_w as f32 / src_w as f32).max(dst_h as f32 / src_h as f32);
    let dx = (dst_w as f32 - src_w as f32 * scale) / 2.0;
    let dy = (dst_h as f32 - src_h as f32 * scale) / 2.0;
    Some((scale, dx, dy))
}

/// Загруженный фон.
pub struct Background {
    pixmap: Pixmap,
}

impl Background {
    /// Чтение `background.png`. Нет файла или он битый — `None`: экран входа
    /// останется однотонным, но появится.
    pub fn load(path: &Path) -> Option<Self> {
        let pixmap = Pixmap::load_png(path).ok()?;
        if pixmap.width() == 0 || pixmap.height() == 0 {
            return None;
        }
        Some(Self { pixmap })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }

    /// Рисует фон на весь кадр по `cover`.
    pub fn draw_cover(&self, target: &mut Pixmap) {
        let Some((scale, dx, dy)) = cover(
            self.pixmap.width(),
            self.pixmap.height(),
            target.width(),
            target.height(),
        ) else {
            return;
        };
        // Билинейная фильтрация: фото, растянутое «ближайшим», читается как
        // мозаика, а экран входа — единственный кадр, где это видно сразу.
        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        target.draw_pixmap(
            0,
            0,
            self.pixmap.as_ref(),
            &paint,
            Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
            None,
        );
    }
}

/// PNG ли это — по сигнатуре, а не по расширению: `background` из телефона
/// может называться как угодно.
pub fn is_png(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic).is_ok() && magic == *b"\x89PNG\r\n\x1a\n"
}

/// Уменьшает PNG до `max_width` по ширине и пишет результат в `dst`.
/// Картинка уже не шире предела — копируется без пересчёта.
pub fn shrink_png(src: &Path, dst: &Path, max_width: u32) -> Result<(), String> {
    let image = Pixmap::load_png(src)
        .map_err(|error| format!("не прочитать {}: {error}", src.display()))?;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(format!("пустая картинка {}", src.display()));
    }
    if width <= max_width {
        image
            .save_png(dst)
            .map_err(|error| format!("не записать {}: {error}", dst.display()))?;
        return Ok(());
    }
    let scale = max_width as f32 / width as f32;
    let target_w = max_width.max(1);
    let target_h = ((height as f32 * scale).round() as u32).max(1);
    let mut target = Pixmap::new(target_w, target_h)
        .ok_or_else(|| format!("не выделить буфер {target_w}×{target_h}"))?;
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    target.draw_pixmap(
        0,
        0,
        image.as_ref(),
        &paint,
        Transform::from_scale(scale, scale),
        None,
    );
    target
        .save_png(dst)
        .map_err(|error| format!("не записать {}: {error}", dst.display()))
}

/// Приводит любой поддерживаемый файл к PNG не шире `max_width`.
///
/// PNG обрабатывается своим декодером, остальное — внешней программой.
/// Возвращает имя использованной программы: это попадает в вывод `--sync`,
/// чтобы было видно, чем именно конвертировали.
pub fn to_png(src: &Path, dst: &Path, max_width: u32) -> Result<&'static str, String> {
    if is_png(src) {
        shrink_png(src, dst, max_width)?;
        return Ok("png");
    }
    if let Some(tool) = converter() {
        let output = match tool {
            // Размер `1920x` — «по ширине, высота по пропорциям».
            "vipsthumbnail" => Command::new(tool)
                .arg("--size")
                .arg(format!("{max_width}x"))
                .arg("--output")
                .arg(dst)
                .arg(src)
                .output(),
            _ => Command::new(tool)
                .arg(src)
                .arg("-resize")
                .arg(format!("{max_width}x>"))
                .arg(dst)
                .output(),
        }
        .map_err(|error| format!("{tool} не запустился: {error}"))?;
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr);
            let reason = reason.lines().next().unwrap_or("без описания").trim();
            return Err(format!("{tool} не справился: {reason}"));
        }
        // Программа могла отдать картинку шире предела (у `magick` это
        // зависит от версии) — дожимаем своим декодером, если это PNG.
        if is_png(dst) {
            let path = dst.to_path_buf();
            shrink_png(&path, dst, max_width)?;
        }
        return Ok(tool);
    }
    Err(format!(
        "{} не PNG, а ni vipsthumbnail, ни magick в PATH нет",
        src.display()
    ))
}

/// Чем конвертировать не-PNG: сначала `vipsthumbnail`, потом `magick` — тот
/// же порядок, что у миниатюр окна обоев.
fn converter() -> Option<&'static str> {
    ["vipsthumbnail", "magick"].into_iter().find(|program| {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hudbar-greeter-image-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        dir
    }

    /// `cover` накрывает кадр целиком и центрирует картинку: широкое фото на
    /// узком экране обрезается слева и справа, а не растягивается.
    #[test]
    fn cover_fills_the_frame_and_centres() {
        let (scale, dx, dy) = cover(1920, 1080, 1000, 1000).expect("геометрия");
        assert!((scale - 1000.0 / 1080.0).abs() < 1e-5, "по высоте");
        assert!(dx < 0.0, "широкое фото уходит за края по x");
        assert!(dy.abs() < 1e-5, "по высоте вписано ровно");

        let (scale, dx, dy) = cover(1080, 1920, 1000, 1000).expect("геометрия");
        assert!((scale - 1000.0 / 1080.0).abs() < 1e-5);
        assert!(dy < 0.0);
        assert!(dx.abs() < 1e-5);
    }

    #[test]
    fn cover_rejects_empty_sizes() {
        assert!(cover(0, 10, 10, 10).is_none());
        assert!(cover(10, 10, 0, 10).is_none());
    }

    /// Уменьшение: ширина ровно предел, пропорции сохранены, файл читается
    /// обратно.
    #[test]
    fn a_wide_png_is_shrunk_to_the_limit() {
        let dir = temp("shrink");
        let src = dir.join("wide.png");
        let dst = dir.join("small.png");
        let mut image = Pixmap::new(400, 200).expect("буфер");
        image.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        image.save_png(&src).expect("запись исходника");

        shrink_png(&src, &dst, 100).expect("уменьшение");

        let small = Pixmap::load_png(&dst).expect("чтение результата");
        assert_eq!((small.width(), small.height()), (100, 50));

        // Уже узкое не растягивается.
        let narrow = dir.join("narrow.png");
        shrink_png(&dst, &narrow, 1000).expect("копия");
        let kept = Pixmap::load_png(&narrow).expect("чтение");
        assert_eq!((kept.width(), kept.height()), (100, 50));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn png_is_recognised_by_signature() {
        let dir = temp("magic");
        let png = dir.join("a.dat");
        Pixmap::new(2, 2)
            .expect("буфер")
            .save_png(&png)
            .expect("png");
        assert!(is_png(&png));

        let text = dir.join("b.dat");
        std::fs::write(&text, b"not an image").expect("файл");
        assert!(!is_png(&text));
        assert!(!is_png(&dir.join("нет-такого")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Не-PNG без конвертера — понятная ошибка, а не паника и не пустой файл.
    #[test]
    fn a_non_png_without_a_converter_is_an_error() {
        let dir = temp("noconv");
        let src = dir.join("photo.jpg");
        let dst = dir.join("out.png");
        std::fs::write(&src, b"\xff\xd8\xffnot really jpeg").expect("файл");

        let result = to_png(&src, &dst, MAX_WIDTH);
        if converter().is_some() {
            // На машине с ImageMagick проверяем, что ошибка — от программы,
            // а не от нашей ветки: файл заведомо битый.
            assert!(result.is_err(), "битый jpeg не должен превратиться в PNG");
        } else {
            let error = result.expect_err("без конвертера ошибка");
            assert!(error.contains("magick"), "{error}");
        }
        assert!(!dst.exists(), "недописанный файл не остаётся");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Фон читается из PNG и рисуется на весь кадр.
    #[test]
    fn a_background_covers_the_frame() {
        let dir = temp("background");
        let path = dir.join("background.png");
        let mut source = Pixmap::new(20, 10).expect("буфер");
        source.fill(tiny_skia::Color::from_rgba8(200, 100, 50, 255));
        source.save_png(&path).expect("png");

        let background = Background::load(&path).expect("фон");
        assert_eq!(background.size(), (20, 10));

        let mut frame = Pixmap::new(40, 40).expect("кадр");
        frame.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
        background.draw_cover(&mut frame);
        // Верхний левый угол кадра закрыт картинкой: cover, а не вписать.
        let pixel = frame.pixel(0, 0).expect("пиксель");
        assert_eq!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (200, 100, 50),
            "кадр накрыт фоном"
        );

        assert!(Background::load(&dir.join("missing.png")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
