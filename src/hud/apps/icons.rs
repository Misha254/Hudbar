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

/// Каталог с иконками: либо сама тема, либо базовый каталог, внутри которого
/// темы лежат одна под другой.
///
/// Разница не в теории, а в том, что показал L0: иконка `zcode` лежит в
/// `~/.local/share/icons/hicolor/512x512/apps/zcode.png`, а
/// `moe.launcher.an-anime-game-launcher` — в
/// `/var/lib/flatpak/exports/share/icons/hicolor/512x512/apps/…`. Базовые
/// каталоги пользователя и экспортов flatpak отдавались поиску как темы, и
/// иконка не находилась никогда: не хватало одного уровня `hicolor/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub path: PathBuf,
    /// Спускаться ли в подкаталоги: `true` у базовых каталогов, где внутри
    /// лежат темы, `false` у `/usr/share/icons/<тема>`.
    pub themes: bool,
}

impl Root {
    /// Каталог темы: иконка лежит прямо в нём.
    pub fn theme(path: PathBuf) -> Self {
        Root {
            path,
            themes: false,
        }
    }

    /// Базовый каталог: иконка лежит в одной из тем внутри него.
    pub fn base(path: PathBuf) -> Self {
        Root { path, themes: true }
    }

    /// Каталоги, в которых ищется иконка. Для темы это сам каталог, для базового
    /// — он сам и все его подкаталоги по алфавиту: порядок тем в базовом
    /// каталоге правилами не задан, а `hicolor` там обычно единственный.
    pub fn dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.path.clone()];
        if !self.themes {
            return dirs;
        }
        let Ok(entries) = std::fs::read_dir(&self.path) else {
            return dirs;
        };
        let mut nested: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        nested.sort();
        dirs.extend(nested);
        dirs
    }
}

/// Каталоги с иконками в порядке приоритета: каталог пользователя перекрывает
/// системный, явные темы — `hicolor`.
pub fn roots() -> Vec<Root> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut roots: Vec<Root> = Vec::new();
    let mut push = |root: Root| {
        if !roots.iter().any(|item| item.path == root.path) {
            roots.push(root);
        }
    };
    if let Some(home) = home.as_ref() {
        // Базовые каталоги: внутри них темы (L0, п. «нет файла», 7 из 9).
        push(Root::base(home.join(".local/share/icons")));
        push(Root::base(home.join(".icons")));
        push(Root::base(
            home.join(".local/share/flatpak/exports/share/icons"),
        ));
    }
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        push(Root::base(data_home.join("icons")));
    }
    push(Root::base(PathBuf::from(
        "/var/lib/flatpak/exports/share/icons",
    )));
    // `pixmaps` — каталог без тем: иконки лежат прямо в нём.
    push(Root::theme(PathBuf::from("/usr/share/pixmaps")));
    for theme in themes() {
        push(Root::theme(PathBuf::from("/usr/share/icons").join(theme)));
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
/// единственный. Размеров больше, чем было: иконки `x-office-calendar` лежат в
/// `AdwaitaLegacy/16x16/mimetypes`, а `16x16` в списке не было.
pub const SIZES: [&str; 14] = [
    "scalable", "512x512", "256x256", "192x192", "128x128", "96x96", "72x72", "64x64", "48x48",
    "36x36", "32x32", "24x24", "22x22", "16x16",
];

/// Те же размеры без `x`: в раскладках `apps/<size>` и `mimetypes/<size>`
/// (breeze, breeze-plus) каталог называется голым числом.
pub const BARE: [&str; 13] = [
    "512", "256", "192", "128", "96", "72", "64", "48", "36", "32", "24", "22", "16",
];

/// Расширения по тем же каталогам: сначала вектор, потом растр.
const EXTENSIONS: [&str; 4] = ["svg", "svgz", "png", "xpm"];

/// Расширения для символьных иконок: `-symbolic` рисуют тем же, чем остальное,
/// но искать их стоит только в самом конце.
const SYMBOLIC: [&str; 2] = ["svg", "png"];

/// Ищет файл иконки: первый кандидат в порядке приоритета.
///
/// `icon` — либо имя из темы, либо абсолютный путь: у AppImage и игровых
/// лаунчеров в `Icon` лежит именно путь. Путь без расширения (лаунчер War
/// Thunder отдаёт в `Icon` сам исполняемый файл) подставляет рядом одноимённые
/// картинки — L0, «файл найден, но это не картинка».
pub fn locate(icon: &str) -> Option<PathBuf> {
    candidates(icon).into_iter().next()
}

/// Все кандидаты по имени иконки, от лучшего к худшему.
///
/// Их больше одного не зря: SVG из темы может не разобраться (L0 п. 3), и
/// тогда берётся следующий — растр того же имени или иконка темы по
/// умолчанию. Загрузчик идёт по списку и берёт первый, который растеризовался.
pub fn candidates(icon: &str) -> Vec<PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return Vec::new();
    }
    if icon.contains('/') {
        let path = PathBuf::from(icon);
        let mut out: Vec<PathBuf> = Vec::new();
        if is_image(&path) && path.is_file() {
            out.push(path.clone());
        }
        // Рядом с файлом нередко лежит одноимённая картинка: сам лаунчер —
        // исполняемый файл, а значок — `launcher.png` рядом с ним.
        for extension in ["png", "svg", "svgz"] {
            let sibling = path.with_extension(extension);
            if sibling.is_file() && !out.contains(&sibling) {
                out.push(sibling);
            }
        }
        return out;
    }
    candidates_in(&roots(), icon)
}

/// Поиск по готовому списку каталогов: так же ищет иконка в тестах. В живое
/// окно не входит — загрузчик берёт [`candidates`], потому что ему нужен весь
/// список, а не только первый файл.
#[cfg(test)]
pub fn locate_in(roots: &[Root], icon: &str) -> Option<PathBuf> {
    candidates_in(roots, icon).into_iter().next()
}

/// Кандидаты по готовому списку каталогов, от лучшего к худшему.
///
/// Вектор идёт перед раСТром по всей выдаче, а не по каталогам: в
/// `~/.local/share/icons/hicolor` лежат PNG от 16 до 64 пикселей, и такой
/// значок перебивал бы вектор из Papirus, хотя рисуется в 26 логических
/// пикселей и на масштабе 2 был бы вдвое меньше нужного. Внутри своего
/// формата порядок каталогов обычный: пользовательский перекрывает системный.
pub fn candidates_in(roots: &[Root], icon: &str) -> Vec<PathBuf> {
    // Три корзины: вектор, растр и символьные. Порядок корзин жёсткий, внутри
    // корзины — порядок каталогов.
    let mut buckets: [Vec<PathBuf>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for root in roots {
        for dir in root.dirs() {
            for candidate in search_dir(&dir, icon) {
                let bucket = if is_symbolic(&candidate) {
                    2
                } else if is_vector(&candidate) {
                    0
                } else {
                    1
                };
                if !buckets[bucket].contains(&candidate) {
                    buckets[bucket].push(candidate);
                }
            }
        }
    }
    buckets.into_iter().flatten().collect()
}

/// Символьная ли иконка: одноимённая картинка с хвостом `-symbolic`. Такие
/// лежат в отдельных каталогах и рисуются монохромными, поэтому идут после
/// всех обычных.
fn is_symbolic(path: &Path) -> bool {
    path.file_stem()
        .is_some_and(|stem| stem.to_string_lossy().ends_with("-symbolic"))
}

/// Вектор ли это по расширению.
fn is_vector(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy().to_lowercase();
        extension == "svg" || extension == "svgz"
    })
}

/// Раскладки внутри одного каталога темы, от лучшей к худшей.
///
/// Три порядка каталогов встречаются в живых темах, и L0 нашёл все три:
/// `<size>/apps` (Papirus, hicolor), `apps/<size>` (breeze, breeze-plus — там
/// нашлась `org.vinegarhq.Sober`), `mimetypes/<size>` и `<size>/mimetypes`
/// (`x-office-calendar` у khal). Символьные иконки идут последними и только
/// если ничего другого не нашлось.
fn search_dir(dir: &Path, icon: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |path: PathBuf| {
        if path.is_file() {
            out.push(path);
        }
    };
    for size in SIZES {
        for extension in EXTENSIONS {
            add(dir
                .join(size)
                .join("apps")
                .join(format!("{icon}.{extension}")));
        }
    }
    for size in BARE {
        for extension in EXTENSIONS {
            add(dir
                .join("apps")
                .join(size)
                .join(format!("{icon}.{extension}")));
        }
    }
    for size in BARE {
        for extension in EXTENSIONS {
            add(dir
                .join("mimetypes")
                .join(size)
                .join(format!("{icon}.{extension}")));
        }
    }
    for size in SIZES {
        for extension in EXTENSIONS {
            add(dir
                .join(size)
                .join("mimetypes")
                .join(format!("{icon}.{extension}")));
        }
    }
    for context in ["symbolic/apps", "apps/symbolic", "scalable/apps-symbolic"] {
        for extension in SYMBOLIC {
            add(dir
                .join(context)
                .join(format!("{icon}-symbolic.{extension}")));
        }
    }
    out
}

/// Картинка ли это по расширению: у `Icon` может лежать исполняемый файл, и
/// растеризатор такие не берёт.
fn is_image(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy().to_lowercase();
        EXTENSIONS.contains(&extension.as_str())
    })
}

/// Растеризует первый кандидат, который разобрался.
///
/// Запасной вариант нужен ровно для L0 п. 3: файл найден, а `rasterize` вернул
/// `None`. Тогда берётся следующий — растр того же имени из другого размера
/// или темы по умолчанию.
pub fn rasterize_first(candidates: &[PathBuf], size: u32) -> Option<Bitmap> {
    candidates.iter().find_map(|path| rasterize(path, size))
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
    /// Спеки, у которых загрузка завершилась неудачей: иконки нет ни в одной
    /// раскладке, либо ни один файл не растеризовался. Отличать от «ещё едет»
    /// нужно: заглушка «загрузка идёт» и заглушка «иконки не будет никогда» —
    /// разные картинки, а по одному `None` в кэше их не различить.
    failed: HashSet<String>,
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
            failed: HashSet::new(),
            queued: HashSet::new(),
            tx: request_tx,
            rx: result_rx,
        }
    }

    /// Готовая иконка; `None` — её надо запросить либо она не найдётся.
    pub fn get(&self, spec: &str) -> Option<Arc<Bitmap>> {
        self.cache.get(spec).cloned().flatten()
    }

    /// Загрузка завершилась неудачей: иконки нет ни в одной раскладке. От
    /// «ещё едет» отличается тем, что ждать больше нечего.
    pub fn has_failed(&self, spec: &str) -> bool {
        self.failed.contains(spec.trim())
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
        let icon = rasterize_first(&candidates(spec), RASTER).map(Arc::new);
        if icon.is_none() {
            self.failed.insert(spec.to_string());
        }
        self.cache.insert(spec.to_string(), icon);
    }

    /// Забирает готовые ответы. `true` — кардр надо перерисовать.
    pub fn take(&mut self) -> bool {
        let mut changed = false;
        // Канал пустеет — ждать больше нечего; обрыв значит то же: поток
        // закончился, и оставшиеся иконки уже не придут.
        while let Ok((spec, icon)) = self.rx.try_recv() {
            self.queued.remove(&spec);
            if icon.is_none() {
                self.failed.insert(spec.clone());
            }
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

    /// Каталог темы во временном дереве: `путь/подкаталог/подкаталог/имя.файл`.
    fn fixture(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hud-apps-icons-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        dir
    }

    /// Кладёт файл иконки по относительному пути внутри каталога.
    fn place(root: &Path, relative: &str, content: &[u8]) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("родитель")).expect("каталоги");
        std::fs::write(&path, content).expect("запись");
        path
    }

    /// Раст в байтах, который всегда растеризуется: тесты поиска интересует
    /// раскладка, а не умение разбирать SVG.
    fn png() -> Vec<u8> {
        png_bytes(8, 8, [200, 30, 40, 255])
    }

    // ───────────────────────── раскладки из отчёта L0 ─────────────────────────

    /// L0: иконка `zcode` лежит в `~/.local/share/icons/hicolor/512x512/apps`.
    /// Базовый каталог отдаётся поиску как тема, и файл не находится никогда —
    /// не хватает уровня `hicolor/`.
    #[test]
    fn a_base_root_descends_into_its_themes() {
        let base = fixture("base-theme");
        let icon = place(&base, "hicolor/512x512/apps/zcode.png", &png());
        let found = locate_in(&[Root::base(base)], "zcode").expect("иконка найдена");
        assert_eq!(found, icon);
    }

    /// L0: экспорт flatpak — тоже базовый каталог, и его тема `hicolor`.
    #[test]
    fn a_flatpak_export_is_a_base_root_too() {
        let export = fixture("flatpak-export");
        let icon = place(
            &export,
            "hicolor/scalable/apps/moe.launcher.an-anime-game-launcher.png",
            &png(),
        );
        let found = locate_in(&[Root::base(export)], "moe.launcher.an-anime-game-launcher")
            .expect("иконка найдена");
        assert_eq!(found, icon);
    }

    /// L0: `org.vinegarhq.Sober` нашлась в `breeze-plus/apps/48/` — размер
    /// стоит после `apps`, а не до него.
    #[test]
    fn the_apps_size_layout_is_searched() {
        let theme = fixture("apps-size");
        let icon = place(&theme, "apps/48/org.vinegarhq.Sober.svg", b"<svg/>");
        let found = locate_in(&[Root::theme(theme)], "org.vinegarhq.Sober").expect("найдена");
        assert_eq!(found, icon);
    }

    /// L0: `x-office-calendar` лежит в `mimetypes` двумя способами — в breeze
    /// это `mimetypes/64/`, в Papirus `64x64/mimetypes/`. Проверяем оба.
    #[test]
    fn the_mimetypes_layout_is_searched_in_both_orders() {
        let breeze = fixture("mimetypes-bare");
        let bare = place(&breeze, "mimetypes/64/x-office-calendar.svg", b"<svg/>");
        assert_eq!(
            locate_in(&[Root::theme(breeze)], "x-office-calendar").expect("breeze"),
            bare
        );

        let papirus = fixture("mimetypes-sized");
        let sized = place(&papirus, "64x64/mimetypes/x-office-calendar.svg", b"<svg/>");
        assert_eq!(
            locate_in(&[Root::theme(papirus)], "x-office-calendar").expect("papirus"),
            sized
        );
    }

    /// L0: темы полны иконок в размерах, которых в списке не было — 16, 22, 24,
    /// 36, 72, 192. Без них `AdwaitaLegacy/16x16/mimetypes` не читалась.
    #[test]
    fn the_small_and_unusual_sizes_are_in_the_list() {
        for size in ["16x16", "22x22", "24x24", "36x36", "72x72", "192x192"] {
            assert!(
                SIZES.contains(&size),
                "{size} должен быть в списке размеров"
            );
            let bare = size.split('x').next().expect("число");
            assert!(
                BARE.contains(&bare),
                "{bare} должен быть и в списке голых размеров"
            );
        }

        let theme = fixture("small-size");
        let icon = place(&theme, "16x16/mimetypes/x-office-calendar.png", &png());
        assert_eq!(
            locate_in(&[Root::theme(theme)], "x-office-calendar").expect("найдена"),
            icon,
            "иконка в 16x16 обязана находиться"
        );
    }

    /// Символьные иконки — последний резерв: нашлись, только когда обычных
    /// кандидатов нет.
    #[test]
    fn symbolic_icons_are_the_last_resort() {
        let theme = fixture("symbolic");
        place(&theme, "symbolic/apps/terminal-symbolic.svg", b"<svg/>");
        let found = locate_in(&[Root::theme(theme.clone())], "terminal").expect("найдена");
        assert!(found.to_string_lossy().ends_with("terminal-symbolic.svg"));

        // Обычная иконка выигрывает у символьной, даже если символьная лежит
        // в каталоге, который стоит раньше по алфавиту.
        let both = fixture("symbolic-and-normal");
        let normal = place(&both, "48x48/apps/terminal.png", &png());
        place(&both, "symbolic/apps/terminal-symbolic.svg", b"<svg/>");
        assert_eq!(
            locate_in(&[Root::theme(both)], "terminal").expect("найдена"),
            normal
        );
    }

    /// Раскладка `<size>/apps` по теме и порядок корней: каталог пользователя
    /// перекрывает системный.
    #[test]
    fn a_theme_icon_is_found_and_roots_win_in_order() {
        let first = fixture("order-first");
        let second = fixture("order-second");
        let icon = place(&second, "scalable/apps/app.svg", b"<svg/>");
        place(&first, "scalable/apps/other.svg", b"<svg/>");
        assert_eq!(
            locate_in(&[Root::theme(first), Root::theme(second.clone())], "app").expect("найдена"),
            icon
        );
    }

    /// Вектор из системной темы должен перебивать мелкий PNG из пользовательского
    /// каталога: значок рисуется в 26 логических пикселей, и на масштабе 2
    /// растр вдвое меньше нужного.
    #[test]
    fn a_vector_from_a_later_root_beats_a_raster_from_an_earlier_one() {
        let user = fixture("user-raster");
        let raster = place(&user, "hicolor/48x48/apps/app.png", &png());
        let theme = fixture("system-vector");
        let vector = place(&theme, "scalable/apps/app.svg", b"<svg/>");

        let found = candidates_in(&[Root::base(user), Root::theme(theme)], "app");
        assert_eq!(found[0], vector, "вектор должен идти первым");
        assert!(found.contains(&raster), "растр остаётся запасным вариантом");
    }

    #[test]
    fn an_absent_icon_is_simply_absent() {
        let root = fixture("missing");
        assert!(locate_in(&[Root::theme(root)], "definitely-not-here").is_none());
        assert!(locate("").is_none());
        assert!(locate("   ").is_none());
        assert!(candidates("nope-not-here").is_empty());
    }

    /// L0: у AppImage и игровых лаунчеров `Icon` — абсолютный путь. Если это
    /// исполняемый файл, рядом нередко лежит одноимённая картинка.
    #[test]
    fn an_absolute_path_is_taken_as_is_and_probed_for_siblings() {
        let dir = fixture("absolute");
        let icon = place(&dir, "app.png", &png());
        assert_eq!(
            candidates(icon.to_str().expect("utf-8"))[0],
            icon,
            "сам путь — первый кандидат"
        );

        // L0, War Thunder: `Icon` указывает на ELF-файл без расширения.
        let launcher = place(&dir, "War Thunder/launcher", b"ELF");
        let sibling = place(&dir, "War Thunder/launcher.png", &png());
        let found = candidates(launcher.to_str().expect("utf-8"));
        assert!(
            found.contains(&sibling),
            "картинка рядом с исполняемым файлом обязана попасть в кандидаты: {found:?}"
        );
        assert!(
            !found.contains(&launcher),
            "исполняемый файл растеризатору не отдать"
        );
    }

    /// L0 п. 3: файл найден, а `rasterize` вернул None. Запасной вариант —
    /// следующий кандидат: растр того же имени из соседнего размера.
    #[test]
    fn a_broken_svg_falls_back_to_the_next_candidate() {
        let theme = fixture("broken-svg");
        place(&theme, "scalable/apps/app.svg", "не svg вовсе".as_bytes());
        let good = place(&theme, "48x48/apps/app.png", &png());
        let candidates = candidates_in(&[Root::theme(theme)], "app");
        assert_eq!(candidates.len(), 2, "сначала вектор, потом растр");
        assert!(
            rasterize(&candidates[0], RASTER).is_none(),
            "битый SVG обязан давать None"
        );
        let bitmap = rasterize_first(&candidates, RASTER).expect("запасной вариант");
        assert_eq!((bitmap.w, bitmap.h), (RASTER, RASTER));
        assert_eq!(candidates[1], good);
    }

    #[test]
    fn rasterize_first_gives_up_when_nothing_renders() {
        let theme = fixture("nothing-renders");
        place(&theme, "scalable/apps/app.svg", "не svg".as_bytes());
        let candidates = candidates_in(&[Root::theme(theme)], "app");
        assert!(rasterize_first(&candidates, RASTER).is_none());
        assert!(rasterize_first(&[], RASTER).is_none());
    }

    // ───────────────────────── темы, корни, GTK ─────────────────────────

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
        assert!(themes.contains(&"Adwaita".to_string()));
    }

    #[test]
    fn user_and_export_roots_are_bases_and_system_ones_are_themes() {
        let roots = roots();
        let by_path = |needle: &str| {
            roots
                .iter()
                .find(|root| root.path.to_string_lossy().contains(needle))
                .expect("корень есть")
        };
        assert!(
            by_path("/var/lib/flatpak/exports/share/icons").themes,
            "экспорт flatpak — базовый каталог"
        );
        assert!(
            !by_path("/usr/share/pixmaps").themes,
            "pixmaps — каталог без тем"
        );
        assert!(
            !by_path("/usr/share/icons/hicolor").themes,
            "каталог темы спускаться не должен"
        );
    }

    // ───────────────────────── раст ─────────────────────────

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
        let at = ((RASTER / 2 * RASTER + RASTER / 2) * 4) as usize;
        assert!(bitmap.rgba[at] > 200, "красный канал: {}", bitmap.rgba[at]);
        assert!(bitmap.rgba[at + 3] > 200, "альфа: {}", bitmap.rgba[at + 3]);
        assert!(bitmap.rgba[at + 1] < 40, "зелёный канал должен быть пуст");
    }

    #[test]
    fn the_svg_raster_is_straight_alpha() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <rect x="0" y="0" width="5" height="10" fill="#ffffff" fill-opacity="0.5"/></svg>"##;
        let bitmap = rasterize_svg(svg, 8).expect("растр");
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
        let root = fixture("raster-file");
        let png_file = place(
            &root,
            "48x48/apps/app.png",
            &png_bytes(48, 48, [1, 2, 3, 255]),
        );
        let bitmap = rasterize(&png_file, RASTER).expect("растр");
        assert_eq!((bitmap.w, bitmap.h), (RASTER, RASTER));

        let unknown = place(&root, "scalable/apps/app.xpm", b"xpm");
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

    // ───────────────────────── блит ─────────────────────────

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
        blit(&mut pixmap, &icon, -20.0, -20.0, 2.0);
        blit(&mut pixmap, &icon, 0.0, 0.0, 0.0);
        for pixel in pixmap.pixels() {
            assert_eq!(pixel.alpha(), 0);
        }
    }

    // ───────────────────────── загрузчик ─────────────────────────

    /// Пока ответ не пришёл, иконка «ещё едет»: `has_failed` обязан молчать, иначе
    /// первая отрисовка показала бы «иконки не будет» вместо заглушки загрузки.
    #[test]
    fn a_queued_icon_is_not_yet_failed() {
        let loader = Loader::new();
        let spec = format!("definitely-not-here-{}", std::process::id());
        assert!(loader.get(&spec).is_none());
        assert!(!loader.has_failed(&spec), "в пути — не значит провал");
    }

    /// Провал фиксируется один раз и после него запрос не повторяется: без
    /// этого очередь росла бы на каждое перерисовывание кадра.
    #[test]
    fn the_loader_marks_a_failed_icon_and_keeps_it_failed() {
        let mut loader = Loader::new();
        let spec = format!("definitely-not-here-{}", std::process::id());
        loader.request(&spec);
        loader.request(&spec);
        assert_eq!(loader.pending(), 1, "повтор во время ожидания игнорируется");
        for _ in 0..400 {
            if loader.take() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(loader.get(&spec).is_none());
        assert!(loader.has_failed(&spec), "провал должен быть зафиксирован");
        assert!(loader.cache.contains_key(&spec), "и кэш, и провал");
    }
}
