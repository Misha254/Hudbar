//! Иконки приложений: где лежит файл, как он превращается в растр и как
//! кладётся в кадр.
//!
//! Темы иконок в системе — это SVG: в `hicolor` двадцать PNG, а остальное
//! `.svg` и `.svgz`. Поэтому рендер один — `resvg`, а PNG разбирается
//! отдельно: в расте иконок бывает прозрачность, и она должна остаться.
//!
//! Разрешение идёт по каталогам в порядке приоритета, а не по `index.theme`:
//! наследование тем здесь ничего не даёт, потому что иконки приложений
//! живут в `hicolor` и в паре явных тем, а разбирать INI-файлы тем ради
//! лаунчера — лишняя работа с тем же результатом.
//!
//! Отрисовка не ждёт диска. `Loader` держит кэш в памяти и один поток:
//! десяток видимых иконок в самом тяжёлом случае (сжатый SVG 512×512) — это
//! десятки миллисекунд, а первый кадр окна рисуется сразу, с заглушками.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Сторона растра в буфере. Иконка рисуется в 28 логических пикселей, то есть в
/// 56 пикселей буфера при `SCALE = 2`; 64 берутся с запасом под темы с
/// нестандартным размером и под будущий Pixel-режим.
pub const RASTER: u32 = 64;

/// Растр иконки: прямые RGBA8, как у миниатюр обоев. Прозрачность значащая,
/// поэтому блит считает её сам, а не кладёт пиксели поверх буфера.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Bitmap {
    /// Раст нужного размера. Меньше исходного — с ближайшим соседом: иконка
    /// всегда уменьшается, а растягивать 16×16 до 64 незачем.
    pub fn scaled_to(self, size: u32) -> Bitmap {
        if self.w == size && self.h == size {
            return self;
        }
        if size == 0 || self.w == 0 || self.h == 0 {
            return Bitmap {
                w: 1,
                h: 1,
                rgba: vec![0, 0, 0, 0],
            };
        }
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            let src_y = (y * self.h / size).min(self.h - 1);
            for x in 0..size {
                let src_x = (x * self.w / size).min(self.w - 1);
                let at = ((src_y * self.w + src_x) * 4) as usize;
                rgba.extend_from_slice(&self.rgba[at..at + 4]);
            }
        }
        Bitmap {
            w: size,
            h: size,
            rgba,
        }
    }
}

/// Каталоги с иконками в порядке приоритета: каталог пользователя перекрывает
/// системный, явные темы — `hicolor`.
pub fn roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !roots.contains(&path) {
            roots.push(path);
        }
    };
    if let Some(home) = home.as_ref() {
        push(home.join(".local/share/icons"));
        push(home.join(".icons"));
        push(home.join(".local/share/flatpak/exports/share/icons"));
    }
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        push(data_home.join("icons"));
    }
    push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    push(PathBuf::from("/usr/share/pixmaps"));
    for theme in themes() {
        push(PathBuf::from("/usr/share/icons").join(theme));
    }
    roots
}

/// Темы в порядке просмотра: те, что выбраны в GTK, затем обычные замены
/// Adwaita, и всегда последним `hicolor` — туда кладут иконки пакетов.
pub fn themes() -> Vec<String> {
    let mut themes: Vec<String> = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home.as_ref() {
        for version in ["4.0", "3.0"] {
            let settings = home.join(format!(".config/gtk-{version}/settings.ini"));
            if let Ok(content) = std::fs::read_to_string(&settings)
                && let Some(theme) = icon_theme(&content)
            {
                themes.push(theme);
            }
        }
    }
    for theme in ["Adwaita", "Papirus", "breeze", "default"] {
        themes.push(theme.to_string());
    }
    themes.push("hicolor".to_string());
    let mut seen: Vec<String> = Vec::new();
    for theme in themes {
        if !seen.contains(&theme) {
            seen.push(theme);
        }
    }
    seen
}

/// `gtk-icon-theme-name` из файла настроек GTK. Разбор строковый, а не INI:
/// в файле есть секции с точкой с запятой и кавычками, ради одного значения
/// тянуть зависимость незачем.
fn icon_theme(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("gtk-icon-theme-name") else {
            continue;
        };
        let value = value.trim_start().strip_prefix('=')?.trim();
        let value = value.trim_matches('"').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Каталоги внутри темы, от больших к маленьким. `scalable` первый: вектор
/// отдаёт одинаковую картинку на любом размере, а у разных тем он ещё и
/// единственный.
pub const SIZES: [&str; 8] = [
    "scalable", "512x512", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32",
];

/// Расширения по тем же каталогам: сначала вектор, потом растр.
const EXTENSIONS: [&str; 4] = ["svg", "svgz", "png", "xpm"];

/// Ищет файл иконки. `icon` — либо имя из темы, либо абсолютный путь: у
/// AppImage и игровых лаунчеров в `Icon` лежит именно путь.
pub fn locate(icon: &str) -> Option<PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    if icon.contains('/') {
        let path = PathBuf::from(icon);
        return path.is_file().then_some(path);
    }
    locate_in(&roots(), icon)
}

/// Поиск по готовому списку каталогов: так же ищет иконка в тестах.
pub fn locate_in(roots: &[PathBuf], icon: &str) -> Option<PathBuf> {
    for root in roots {
        for size in SIZES {
            for extension in EXTENSIONS {
                let candidate = root
                    .join(size)
                    .join("apps")
                    .join(format!("{icon}.{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Растеризует файл иконки в квадрат `size`. Формат выбирается по расширению,
/// а не по содержимому: зонд по magic number для файлов из `hicolor` — лишняя
/// работа, а неизвестное расширение всё равно нечего рисовать.
pub fn rasterize(path: &Path, size: u32) -> Option<Bitmap> {
    let bytes = std::fs::read(path).ok()?;
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "svg" | "svgz" => rasterize_svg(&bytes, size),
        "png" => decode_png(&bytes).map(|bitmap| bitmap.scaled_to(size)),
        _ => None,
    }
}

/// SVG и SVGZ через resvg. Размер задаётся по меньшей стороне: иконка не
/// обязана быть квадратной, а поле в списке — квадратное, поэтому лишнее по
/// большей стороне отрезается центрированием.
pub fn rasterize_svg(bytes: &[u8], size: u32) -> Option<Bitmap> {
    if size == 0 {
        return None;
    }
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let source = tree.size();
    if source.width() <= 0.0 || source.height() <= 0.0 {
        return None;
    }
    let scale = size as f32 / source.width().max(source.height());
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(Bitmap {
        w: pixmap.width(),
        h: pixmap.height(),
        rgba: unpremultiply(pixmap.data()),
    })
}

/// tiny-skia отдаёт премультиплицированные пиксели, а растр иконки хранится
/// прямыми: блит считает альфу сам, и премультипликация там посчиталась бы
/// дважды.
fn unpremultiply(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for pixel in data.as_chunks::<4>().0 {
        let alpha = pixel[3];
        if alpha == 0 || alpha == 255 {
            out.extend_from_slice(pixel);
            continue;
        }
        let scale = 255.0 / alpha as f32;
        out.push((pixel[0] as f32 * scale).min(255.0) as u8);
        out.push((pixel[1] as f32 * scale).min(255.0) as u8);
        out.push((pixel[2] as f32 * scale).min(255.0) as u8);
        out.push(alpha);
    }
    out
}

/// Разбор PNG. Только четыре типа цвета из спецификации PNG, остальные
/// (палитра, 16 бит) иконками не бывают, но место не должно падать из-за
/// одной нестандартной картинки.
fn decode_png(bytes: &[u8]) -> Option<Bitmap> {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut reader = reader;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.width == 0 || info.height == 0 {
        return None;
    }
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::Grayscale => buffer
            .iter()
            .flat_map(|gray| [*gray, *gray, *gray, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        _ => return None,
    };
    Some(Bitmap {
        w: info.width,
        h: info.height,
        rgba,
    })
}

/// Кладёт иконку в кадр: билинейно по площади, с наложением по альфе.
///
/// `x`, `y` и `size` — логические пиксели, как и у остальной отрисовки:
/// масштабом буфера занимается вызывающий.
pub fn blit(pixmap: &mut tiny_skia::Pixmap, icon: &Bitmap, x: f32, y: f32, size: f32) {
    let target = (size * crate::text::SCALE) as i32;
    if target <= 0 || icon.w == 0 || icon.h == 0 {
        return;
    }
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let x0 = (x * crate::text::SCALE).round() as i32;
    let y0 = (y * crate::text::SCALE).round() as i32;
    let source_w = icon.w as i32;
    let source_h = icon.h as i32;
    let pixels = pixmap.pixels_mut();
    for dy in 0..target {
        let fy = (dy as f32 + 0.5) * source_h as f32 / target as f32 - 0.5;
        let y1 = fy.floor() as i32;
        let ty = (fy - y1 as f32).clamp(0.0, 1.0);
        for dx in 0..target {
            let fx = (dx as f32 + 0.5) * source_w as f32 / target as f32 - 0.5;
            let x1 = fx.floor() as i32;
            let tx = (fx - x1 as f32).clamp(0.0, 1.0);
            let mut acc = [0f32; 4];
            for oy in 0..2 {
                for ox in 0..2 {
                    let sx = (x1 + ox).clamp(0, source_w - 1) as usize;
                    let sy = (y1 + oy).clamp(0, source_h - 1) as usize;
                    let at = (sy * icon.w as usize + sx) * 4;
                    let weight = (if ox == 0 { 1.0 - tx } else { tx })
                        * (if oy == 0 { 1.0 - ty } else { ty });
                    for (channel, sum) in acc.iter_mut().enumerate() {
                        *sum += icon.rgba[at + channel] as f32 * weight;
                    }
                }
            }
            let alpha = (acc[3] / 255.0).clamp(0.0, 1.0);
            if alpha <= 0.004 {
                continue;
            }
            let px = x0 + dx;
            let py = y0 + dy;
            if px < 0 || py < 0 || px >= width || py >= height {
                continue;
            }
            let dst = &mut pixels[(py * width + px) as usize];
            let inv = 1.0 - alpha;
            let mixed = [
                acc[0] * alpha + dst.red() as f32 * inv,
                acc[1] * alpha + dst.green() as f32 * inv,
                acc[2] * alpha + dst.blue() as f32 * inv,
                acc[3] + dst.alpha() as f32 * inv,
            ];
            if let Some(next) = tiny_skia::PremultipliedColorU8::from_rgba(
                mixed[0].min(255.0) as u8,
                mixed[1].min(255.0) as u8,
                mixed[2].min(255.0) as u8,
                mixed[3].min(255.0) as u8,
            ) {
                *dst = next;
            }
        }
    }
}

/// Загрузчик иконок: кэш, очередь и один поток.
///
/// Кэш хранит и отрицательный ответ (`None`): отсутствующая иконка не должна
/// искаться заново на каждом кадре, а очередь без этого росла бы на прокрутке.
pub struct Loader {
    cache: HashMap<String, Option<Arc<Bitmap>>>,
    queued: HashSet<String>,
    tx: Sender<String>,
    rx: Receiver<(String, Option<Arc<Bitmap>>)>,
}

impl Loader {
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<String>();
        let (result_tx, result_rx) = channel::<(String, Option<Arc<Bitmap>>)>();
        // Один поток: иконок десяток, а разбирать их по очереди дешевле, чем
        // держать пул: первый видимый кадр всё равно рисуется с заглушками.
        // Ответ приходит всегда, в том числе отрицательный — иначе UI будет
        // вечно считать отсутствующую иконку «в пути».
        let _ = std::thread::Builder::new()
            .name("hud-apps-icons".to_string())
            .spawn(move || {
                for spec in request_rx {
                    let loaded = locate(&spec)
                        .and_then(|path| rasterize(&path, RASTER))
                        .map(Arc::new);
                    if result_tx.send((spec, loaded)).is_err() {
                        break;
                    }
                }
            });
        Loader {
            cache: HashMap::new(),
            queued: HashSet::new(),
            tx: request_tx,
            rx: result_rx,
        }
    }

    /// Что уже известно про иконку: `Some` — готова, `None` — её надо запросить.
    pub fn get(&self, spec: &str) -> Option<Arc<Bitmap>> {
        self.cache.get(spec).cloned().flatten()
    }

    /// Ставит иконку в очередь, если её ещё нет. Повторный запрос во время
    /// ожидания игнорируется: иначе десять строк списка забили бы очередь
    /// десятью одинаковыми заданиями.
    pub fn request(&mut self, spec: &str) {
        let spec = spec.trim();
        if spec.is_empty() || self.cache.contains_key(spec) || !self.queued.insert(spec.to_string())
        {
            return;
        }
        let _ = self.tx.send(spec.to_string());
    }

    /// Синхронная загрузка одной иконки. Нужна снимкам окна: там нет живого
    /// цикла, ждать поток некому, а первый кадр должен быть с настоящими
    /// иконками, а не с заглушками.
    pub fn prime(&mut self, spec: &str) {
        let spec = spec.trim();
        if spec.is_empty() || self.cache.contains_key(spec) {
            return;
        }
        let icon = locate(spec)
            .and_then(|path| rasterize(&path, RASTER))
            .map(Arc::new);
        self.cache.insert(spec.to_string(), icon);
    }

    /// Забирает готовые ответы. `true` — кардр надо перерисовать.
    pub fn take(&mut self) -> bool {
        let mut changed = false;
        // Канал пустеет — ждать больше нечего; обрыв значит то же: поток
        // закончился, и оставшиеся иконки уже не придут.
        while let Ok((spec, icon)) = self.rx.try_recv() {
            self.queued.remove(&spec);
            self.cache.insert(spec, icon);
            changed = true;
        }
        changed
    }

    /// Сколько ответов ещё в пути: нужно для отладки, а не для отрисовки.
    pub fn pending(&self) -> usize {
        self.queued.len()
    }
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Кодирует PNG в память: иконки читаются из файлов, но для теста растра
    /// файл не нужен — нужен сам байтовый формат.
    fn png_bytes(w: u32, h: u32, pixel: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("заголовок");
            writer
                .write_image_data(&pixel.repeat((w * h) as usize))
                .expect("данные");
        }
        out
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hud-apps-icons-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scalable/apps")).expect("каталог");
        std::fs::create_dir_all(dir.join("48x48/apps")).expect("каталог");
        dir
    }

    #[test]
    fn a_theme_icon_is_found_in_the_first_matching_size() {
        let root = temp_root("locate");
        std::fs::write(root.join("scalable/apps/app.svg"), b"<svg/>").expect("запись");
        std::fs::write(root.join("48x48/apps/app.png"), b"png").expect("запись");

        let found = locate_in(std::slice::from_ref(&root), "app").expect("иконка найдена");
        assert!(
            found.ends_with("scalable/apps/app.svg"),
            "вектор приоритетнее"
        );

        std::fs::remove_file(root.join("scalable/apps/app.svg")).expect("удаление");
        let found = locate_in(&[root], "app").expect("иконка найдена");
        assert!(found.ends_with("48x48/apps/app.png"));
    }

    #[test]
    fn roots_are_searched_in_order() {
        let first = temp_root("roots-first");
        let second = temp_root("roots-second");
        std::fs::write(second.join("scalable/apps/app.svg"), b"<svg/>").expect("запись");

        let found = locate_in(&[first.clone(), second.clone()], "app").expect("найдена");
        assert!(
            found.starts_with(&second),
            "каталог пользователя должен перекрывать системный"
        );
    }

    #[test]
    fn an_absent_icon_is_simply_absent() {
        let root = temp_root("locate-missing");
        assert!(locate_in(&[root], "definitely-not-here").is_none());
        assert!(locate("").is_none());
        assert!(locate("   ").is_none());
    }

    #[test]
    fn an_icon_given_as_a_path_is_taken_as_is() {
        let root = temp_root("path-icon");
        let file = root.join("scalable/apps/app.svg");
        std::fs::write(&file, b"<svg/>").expect("запись");
        assert_eq!(locate(file.to_str().expect("utf-8")), Some(file));
        assert!(locate("/nonexistent/icon.png").is_none());
    }

    #[test]
    fn the_icon_theme_is_read_from_gtk_settings() {
        let content =
            "[Settings]\ngtk-theme-name=Bibata-Modern-Ice\ngtk-icon-theme-name=Papirus-Dark\n";
        assert_eq!(icon_theme(content).as_deref(), Some("Papirus-Dark"));
        assert_eq!(
            icon_theme("[Settings]\ngtk-icon-theme-name = \"Adwaita\" \n").as_deref(),
            Some("Adwaita")
        );
        assert!(icon_theme("[Settings]\ngtk-theme-name=Bibata\n").is_none());
    }

    #[test]
    fn hicolor_is_always_the_last_theme() {
        let themes = themes();
        assert_eq!(themes.last().map(String::as_str), Some("hicolor"));
        assert!(
            themes.contains(&"Adwaita".to_string()),
            "тема-замена Adwaita должна просматриваться"
        );
    }

    #[test]
    fn png_icons_are_decoded_and_scaled() {
        let bytes = png_bytes(2, 2, [10, 20, 30, 255]);
        let bitmap = decode_png(&bytes).expect("разбор PNG");
        assert_eq!((bitmap.w, bitmap.h), (2, 2));
        assert_eq!(&bitmap.rgba[0..4], &[10, 20, 30, 255]);

        let scaled = bitmap.scaled_to(4);
        assert_eq!((scaled.w, scaled.h), (4, 4));
        assert_eq!(
            &scaled.rgba[0..4],
            &[10, 20, 30, 255],
            "цвет не должен меняться"
        );
    }

    #[test]
    fn a_transparent_png_keeps_its_alpha() {
        let bytes = png_bytes(1, 1, [1, 2, 3, 0]);
        let bitmap = decode_png(&bytes).expect("разбор PNG");
        assert_eq!(bitmap.rgba[3], 0, "прозрачность обязана выжить");
    }

    #[test]
    fn garbage_is_not_an_icon() {
        assert!(decode_png("не png".as_bytes()).is_none());
        assert!(rasterize_svg("не svg".as_bytes(), 32).is_none());
    }

    #[test]
    fn an_svg_is_rastered_to_the_requested_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <rect x="0" y="0" width="10" height="10" fill="#ff0000"/></svg>"##;
        let bitmap = rasterize_svg(svg, RASTER).expect("растр");
        assert_eq!((bitmap.w, bitmap.h), (RASTER, RASTER));
        // Пиксель из середины красного прямоугольника: цвет должен быть
        // красным, а альфа непрозрачной.
        let at = ((RASTER / 2 * RASTER + RASTER / 2) * 4) as usize;
        assert!(bitmap.rgba[at] > 200, "красный канал: {}", bitmap.rgba[at]);
        assert!(bitmap.rgba[at + 3] > 200, "альфа: {}", bitmap.rgba[at + 3]);
        assert!(bitmap.rgba[at + 1] < 40, "зелёный канал должен быть пуст");
    }

    /// Растр хранится прямыми пикселями: премультипликация от resvg испортила
    /// бы цвета на полупрозрачных краях иконки.
    #[test]
    fn the_svg_raster_is_straight_alpha() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <rect x="0" y="0" width="5" height="10" fill="#ffffff" fill-opacity="0.5"/></svg>"##;
        let bitmap = rasterize_svg(svg, 8).expect("растр");
        // Точка внутри прямоугольника: в восьми пикселях он занимает колонки
        // 0..4, и правая граница уже прозрачна.
        let middle = (4 * 8 + 2) * 4;
        assert!(
            (128..=132).contains(&bitmap.rgba[middle + 3]),
            "полупрозрачный пиксель: {}",
            bitmap.rgba[middle + 3]
        );
        assert!(
            bitmap.rgba[middle] > 240,
            "цвет должен остаться белым, а не стать серым: {}",
            bitmap.rgba[middle]
        );
    }

    #[test]
    fn a_raster_file_is_read_by_extension() {
        let root = temp_root("raster-file");
        let png = root.join("48x48/apps/app.png");
        std::fs::write(&png, png_bytes(48, 48, [1, 2, 3, 255])).expect("запись");
        let bitmap = rasterize(&png, RASTER).expect("растр");
        assert_eq!((bitmap.w, bitmap.h), (RASTER, RASTER));

        let unknown = root.join("scalable/apps/app.xpm");
        std::fs::write(&unknown, b"xpm").expect("запись");
        assert!(rasterize(&unknown, RASTER).is_none(), "xpm мы не рисуем");
    }

    #[test]
    fn scaling_to_zero_does_not_panic() {
        let bitmap = Bitmap {
            w: 4,
            h: 4,
            rgba: vec![0; 64],
        };
        assert_eq!(bitmap.scaled_to(0).w, 1);
    }

    #[test]
    fn blit_draws_an_opaque_icon_over_the_background() {
        let icon = Bitmap {
            w: 2,
            h: 2,
            rgba: [255u8, 0, 0, 255].repeat(4),
        };
        let mut pixmap = black(4);
        blit(&mut pixmap, &icon, 0.0, 0.0, 2.0);

        let red = pixmap.pixel(1, 1).expect("пиксель");
        assert!(
            red.red() > 200 && red.green() < 60,
            "иконка должна лечь поверх"
        );
    }

    /// Прозрачные пиксели иконки не затирают фон: иначе значок приложения был бы
    /// белым квадратом на тёмной карточке.
    #[test]
    fn blit_keeps_transparent_pixels_transparent() {
        let icon = Bitmap {
            w: 2,
            h: 2,
            rgba: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0],
        };
        let mut pixmap = black(4);
        blit(&mut pixmap, &icon, 0.0, 0.0, 2.0);

        // Ни один пиксель не стал белым: прозрачность прошла насквозь.
        for pixel in pixmap.pixels() {
            assert!(
                pixel.red() < 120,
                "полупрозрачный красный не должен долечь до 255"
            );
        }
    }

    /// Кадр размером `size` × `size` логических пикселей, залитый чёрным.
    fn black(size: u32) -> tiny_skia::Pixmap {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("буфер");
        for pixel in pixmap.pixels_mut() {
            *pixel = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 255).expect("цвет");
        }
        pixmap
    }

    #[test]
    fn blit_outside_the_buffer_is_dropped() {
        let icon = Bitmap {
            w: 2,
            h: 2,
            rgba: vec![255; 16],
        };
        let mut pixmap = tiny_skia::Pixmap::new(4, 4).expect("буфер");
        // Иконка целиком за левым и верхним краем: ни одного обращения к памяти
        // за пределами буфера, а кадр не должен измениться.
        blit(&mut pixmap, &icon, -20.0, -20.0, 2.0);
        blit(&mut pixmap, &icon, 0.0, 0.0, 0.0);
        for pixel in pixmap.pixels() {
            assert_eq!(pixel.alpha(), 0);
        }
    }

    #[test]
    fn the_loader_caches_missing_icons_too() {
        let mut loader = Loader::new();
        let spec = format!("definitely-not-here-{}", std::process::id());
        assert!(loader.get(&spec).is_none());
        loader.request(&spec);
        loader.request(&spec);
        // Второй запрос во время ожидания должен быть проигнорирован: иначе
        // очередь росла бы на каждое перерисовывание кадра.
        assert_eq!(loader.pending(), 1);
        for _ in 0..400 {
            if loader.take() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            loader.get(&spec).is_none(),
            "иконки нет, и это уже известно"
        );
        assert!(
            loader.cache.contains_key(&spec),
            "отрицательный ответ кэшируется"
        );
    }
}
