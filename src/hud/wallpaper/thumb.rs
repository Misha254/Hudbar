//! Фоновый загрузчик миниатюр: UI-поток только читает готовое.
//!
//! Правила из W3: для файла путь превью берётся из [`super::cache`]; свежее
//! превью декодируется из PNG и уменьшается до размера плитки; отсутствующее
//! или stale генерируется командами из `cache` (`vipsthumbnail`, иначе
//! `magick`) через ограниченный пул потоков, запись — через `.tmp` + rename.
//! Приоритет — видимые ячейки, затем соседние ряды; при прокрутке устаревшие
//! задания отбрасываются по счётчику поколения. Кэш в памяти — LRU на 128.
//! Первый кадр рисуется сразу с заглушками: рендер в ожидание диска не уходит
//! и сам ввод-вывод не делает.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};

/// Сколько миниатюр держит кэш в памяти: 200×112×4 ≈ 87.5 KiB на штуку,
// 128 штук ≈ 11 МБ — в разы меньше одного кадра.
pub const LRU_CAPACITY: usize = 128;
/// Потолок размера хранимой миниатюры: превью 400×225 уменьшается сразу при
/// загрузке, чтобы LRU не распухал и блит в кадр был дешёвым.
pub const THUMB_MAX_W: u32 = 224;
pub const THUMB_MAX_H: u32 = 128;
/// Число рабочих потоков по умолчанию: генерация `vipsthumbnail`/`magick`
/// тяжёлая, больше трёх нет смысла.
pub const WORKERS: usize = 2;

/// Миниатюра: прямые RGBA8. Фото непрозрачные, alpha всегда 255 — поэтому
/// блит в кадр пишет пиксели напрямую без возни с premultiplied.
#[derive(Clone, Debug)]
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Thumb {
    /// Сплошная заглушка для тестов.
    pub fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            pixels.extend_from_slice(&rgba);
        }
        Thumb { w, h, rgba: pixels }
    }
}

/// Источник пикселей. Живой читает диск и запускает генераторы, тестовый —
/// подставной без диска.
pub trait Source: Send + Sync + 'static {
    fn load(&self, path: &Path) -> Option<Thumb>;
}

/// Чтение без потоков: то, что видит отрисовка. Рендер делает только `get`.
pub trait Provider {
    fn get(&self, path: &Path) -> Option<Thumb>;
}

/// Пустой провайдер для снимков: только заглушки, никакого диска.
pub struct Empty;

impl Provider for Empty {
    fn get(&self, _path: &Path) -> Option<Thumb> {
        None
    }
}

/// LRU на фиксированную ёмкость: чистая структура без потоков, чтобы тесты
/// проверяли вытеснение без гонок.
pub struct Lru {
    cap: usize,
    map: HashMap<PathBuf, Thumb>,
    order: VecDeque<PathBuf>,
}

impl Lru {
    pub fn new(cap: usize) -> Self {
        Lru {
            cap,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Чтение с освежением: недавно спрошенное вытесняется последним.
    pub fn get(&mut self, path: &Path) -> Option<Thumb> {
        let thumb = self.map.get(path)?.clone();
        self.touch(path);
        Some(thumb)
    }

    /// Проверка без освежения: по ней `request` решает, слать ли задание.
    pub fn contains(&self, path: &Path) -> bool {
        self.map.contains_key(path)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn put(&mut self, path: PathBuf, thumb: Thumb) {
        if self.map.contains_key(&path) {
            self.map.insert(path.clone(), thumb);
            self.touch(&path);
            return;
        }
        while self.map.len() >= self.cap.max(1) {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(path.clone());
        self.map.insert(path, thumb);
    }

    fn touch(&mut self, path: &Path) {
        if let Some(index) = self.order.iter().position(|key| key == path) {
            self.order.remove(index);
        }
        self.order.push_back(path.to_path_buf());
    }
}

/// Порядок очереди: видимые первыми, затем соседи; дубли и то, что уже в
/// кэше, не планируются. Чистая функция — приоритет тестируется без потоков.
pub fn plan_order(
    visible: &[PathBuf],
    prefetch: &[PathBuf],
    is_cached: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut planned = Vec::new();
    let mut seen = HashSet::new();
    for path in visible.iter().chain(prefetch.iter()) {
        if !seen.insert(path.clone()) || is_cached(path) {
            continue;
        }
        planned.push(path.clone());
    }
    planned
}

/// Живой источник: диск + генерация. Вне `Loader`, чтобы тесты подставляли
/// своё без диска.
pub struct FsSource {
    cache_dir: PathBuf,
}

impl FsSource {
    pub fn new() -> Self {
        FsSource {
            cache_dir: super::cache::cache_dir(),
        }
    }

    #[cfg(test)]
    pub fn with_dir(dir: PathBuf) -> Self {
        FsSource { cache_dir: dir }
    }
}

impl Default for FsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for FsSource {
    fn load(&self, path: &Path) -> Option<Thumb> {
        let preview = super::cache::preview_for(&self.cache_dir, path);
        if preview.exists()
            && !super::cache::is_stale(path, &preview)
            && let Some(thumb) = decode_png_file(&preview)
        {
            return Some(thumb);
        }
        self.generate(path, &preview)
    }
}

impl FsSource {
    /// Генерация превью командами из `cache`: `vipsthumbnail`, иначе `magick`.
    /// Пишет во временный файл рядом и переименовывает — читатель никогда не
    /// видит половину PNG.
    fn generate(&self, file: &Path, preview: &Path) -> Option<Thumb> {
        if std::fs::create_dir_all(&self.cache_dir).is_err() {
            return None;
        }
        let tmp = super::cache::temp_preview(preview);
        let spec = if command_exists("vipsthumbnail") {
            super::cache::vipsthumbnail(file, &tmp)
        } else {
            super::cache::magick(file, &tmp)
        };
        let output = std::process::Command::new(spec.program)
            .args(&spec.args)
            .output()
            .ok()?;
        if !output.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        if std::fs::rename(&tmp, preview).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        decode_png_file(preview)
    }
}

/// Есть ли программа в `PATH`: выбор между `vipsthumbnail` и `magick`.
fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).exists()))
}

/// Декодирует PNG-файл в миниатюру с уменьшением до потолка.
pub fn decode_png_file(path: &Path) -> Option<Thumb> {
    let bytes = std::fs::read(path).ok()?;
    decode_png_bytes(&bytes)
}

/// Декодирует PNG-байты в миниатюру. Чистая функция без диска: тесты гоняют
/// её на синтетических PNG.
pub fn decode_png_bytes(bytes: &[u8]) -> Option<Thumb> {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut reader = reader;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 {
        return None;
    }
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf
            .iter()
            .flat_map(|gray| [*gray, *gray, *gray, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        _ => return None,
    };
    Some(shrink_to_fit(Thumb { w, h, rgba }))
}

/// Уменьшает миниатюру до потолка с сохранением пропорций. Вверх не тянет:
// мелкие превью остаются как есть, их растянет блит.
pub fn shrink_to_fit(thumb: Thumb) -> Thumb {
    if thumb.w <= THUMB_MAX_W && thumb.h <= THUMB_MAX_H {
        return thumb;
    }
    let scale = (THUMB_MAX_W as f32 / thumb.w as f32).min(THUMB_MAX_H as f32 / thumb.h as f32);
    let target_w = ((thumb.w as f32 * scale).round() as u32).max(1);
    let target_h = ((thumb.h as f32 * scale).round() as u32).max(1);
    scale_nearest(&thumb, target_w, target_h)
}

/// Масштабирование ближайшим соседом: дёшево и без новых зависимостей.
pub fn scale_nearest(thumb: &Thumb, target_w: u32, target_h: u32) -> Thumb {
    if target_w == 0 || target_h == 0 {
        return Thumb {
            w: 1,
            h: 1,
            rgba: vec![0, 0, 0, 255],
        };
    }
    let mut rgba = Vec::with_capacity((target_w * target_h * 4) as usize);
    for y in 0..target_h {
        let src_y = (y * thumb.h / target_h).min(thumb.h.saturating_sub(1));
        for x in 0..target_w {
            let src_x = (x * thumb.w / target_w).min(thumb.w.saturating_sub(1));
            let offset = ((src_y * thumb.w + src_x) * 4) as usize;
            rgba.extend_from_slice(&thumb.rgba[offset..offset + 4]);
        }
    }
    Thumb {
        w: target_w,
        h: target_h,
        rgba,
    }
}

/// Задание с поколением: устаревшие отбрасываются и в очереди, и в ответе.
struct Job {
    path: PathBuf,
    generation: u64,
}

/// Загрузчик: кэш + поколения + пул потоков. Создаётся через
/// [`Loader::new`], отдаётся в `Arc` — потоки держат свою копию.
pub struct Loader<S> {
    source: Arc<S>,
    cache: Mutex<Lru>,
    inflight: Mutex<HashSet<PathBuf>>,
    generation: AtomicU64,
    tx: mpsc::Sender<Job>,
    dirty: AtomicBool,
}

impl<S: Source> Loader<S> {
    /// Запускает пул из `workers` потоков. Ноль потоков — только кэш и
    /// очередь без исполнения: для тестов чистых переходов.
    pub fn new(source: S, workers: usize) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let loader = Arc::new(Loader {
            source: Arc::new(source),
            cache: Mutex::new(Lru::new(LRU_CAPACITY)),
            inflight: Mutex::new(HashSet::new()),
            generation: AtomicU64::new(0),
            tx,
            dirty: AtomicBool::new(false),
        });
        let rx = Arc::new(Mutex::new(rx));
        for index in 0..workers {
            let loader = Arc::clone(&loader);
            let rx = Arc::clone(&rx);
            std::thread::Builder::new()
                .name(format!("hud-wp-thumb-{index}"))
                .spawn(move || worker(loader, rx))
                .expect("поток миниатюр");
        }
        loader
    }

    /// Чтение без ввода-вывода: то, что вызывает рендер каждый кадр.
    pub fn get(&self, path: &Path) -> Option<Thumb> {
        self.cache.lock().ok()?.get(path)
    }

    /// Заказывает миниатюры: видимые первыми, затем соседи. Уже лежащие в
    /// кэше или в полёте не дублируются.
    pub fn request(&self, visible: &[PathBuf], prefetch: &[PathBuf]) {
        let planned = {
            let cache = self.cache.lock().expect("кэш миниатюр");
            plan_order(visible, prefetch, &|path| cache.contains(path))
        };
        let generation = self.generation.load(Ordering::Relaxed);
        let mut inflight = self.inflight.lock().expect("полёт миниатюр");
        for path in planned {
            if !inflight.insert(path.clone()) {
                continue;
            }
            if self.tx.send(Job { path, generation }).is_err() {
                break;
            }
        }
    }

    /// Новое поколение: вызывается при прокрутке и смене набора. Старые
    /// задания рабочие потоки пропустят, а их ответы `insert_result`
    /// отбросит. Полёт очищается, чтобы те же пути заказались заново.
    pub fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.inflight.lock().expect("полёт миниатюр").clear();
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Появилось ли готовое со прошлой проверки. UI-поток опрашивает каждый
    /// тик цикла (там же, где `poll` с таймаутом) и ставит `dirty` слоя.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    /// Приём ответа от рабочего потока. Ложь — поколение устарело, ответ
    /// выброшен и в кэш не попал.
    fn insert_result(&self, path: PathBuf, thumb: Thumb, generation: u64) -> bool {
        if generation != self.generation.load(Ordering::Relaxed) {
            return false;
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(path, thumb);
        }
        self.dirty.store(true, Ordering::Relaxed);
        true
    }
}

impl<S: Source> Provider for Loader<S> {
    fn get(&self, path: &Path) -> Option<Thumb> {
        Loader::get(self, path)
    }
}

/// Тело рабочего потока: пропустить устаревшее, загрузить, принять ответ,
// снять с полёта. `recv` на мьютексе канала — классический общий приёмник.
fn worker<S: Source>(loader: Arc<Loader<S>>, rx: Arc<Mutex<mpsc::Receiver<Job>>>) {
    loop {
        let job = {
            let guard = rx.lock().expect("канал миниатюр");
            guard.recv()
        };
        let Ok(job) = job else {
            return;
        };
        if job.generation != loader.generation.load(Ordering::Relaxed) {
            loader
                .inflight
                .lock()
                .expect("полёт миниатюр")
                .remove(&job.path);
            continue;
        }
        if let Some(thumb) = loader.source.load(&job.path) {
            loader.insert_result(job.path.clone(), thumb, job.generation);
        }
        loader
            .inflight
            .lock()
            .expect("полёт миниатюр")
            .remove(&job.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Подставной источник без диска: записывает порядок вызовов и отдаёт
    /// сплошную миниатюру. Блокировок нет — детерминированность дают один
    /// рабочий поток и очередь FIFO.
    struct Fake {
        calls: Mutex<Vec<PathBuf>>,
    }

    impl Fake {
        fn new() -> Self {
            Fake {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<PathBuf> {
            self.calls.lock().expect("вызовы").clone()
        }
    }

    impl Source for Fake {
        fn load(&self, path: &Path) -> Option<Thumb> {
            self.calls.lock().expect("вызовы").push(path.to_path_buf());
            Some(Thumb::solid(8, 8, [9, 9, 9, 255]))
        }
    }

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/tmp/{name}.png"))
    }

    /// LRU: свежие живут, давно не спрошенные вытесняются первым.
    #[test]
    fn lru_evicts_the_least_recently_used() {
        let mut lru = Lru::new(3);
        lru.put(path("a"), Thumb::solid(1, 1, [1, 0, 0, 255]));
        lru.put(path("b"), Thumb::solid(1, 1, [2, 0, 0, 255]));
        lru.put(path("c"), Thumb::solid(1, 1, [3, 0, 0, 255]));
        assert!(lru.get(&path("a")).is_some(), "a освежили чтением");
        lru.put(path("d"), Thumb::solid(1, 1, [4, 0, 0, 255]));
        assert!(lru.get(&path("a")).is_some(), "свежая a живёт");
        assert!(
            lru.get(&path("b")).is_none(),
            "b давно не спрашивали — вытеснена"
        );
        assert!(lru.get(&path("c")).is_some());
        assert!(lru.get(&path("d")).is_some());
        assert_eq!(lru.len(), 3);
    }

    /// LRU: повторная вставка того же пути не раздувает ёмкость.
    #[test]
    fn lru_put_of_the_same_path_does_not_grow() {
        let mut lru = Lru::new(2);
        lru.put(path("a"), Thumb::solid(1, 1, [1, 0, 0, 255]));
        lru.put(path("a"), Thumb::solid(1, 1, [2, 0, 0, 255]));
        assert_eq!(lru.len(), 1);
        let thumb = lru.get(&path("a")).expect("a на месте");
        assert_eq!(thumb.rgba[0], 2, "значение обновилось");
    }

    /// Приоритет — чистая функция: видимые первыми, дубли и кэш мимо.
    #[test]
    fn plan_order_puts_visible_first_without_dupes() {
        let visible = vec![path("a"), path("b"), path("c")];
        let prefetch = vec![path("c"), path("d"), path("b"), path("e")];
        let planned = plan_order(&visible, &prefetch, &|p| p == path("b"));
        assert_eq!(
            planned,
            vec![path("a"), path("c"), path("d"), path("e")],
            "b уже в кэше, дубли схлопнуты, видимые впереди"
        );
    }

    /// Пустые входы — пустой план, без паники.
    #[test]
    fn plan_order_of_empty_lists_is_empty() {
        let planned = plan_order(&[], &[], &|_| false);
        assert!(planned.is_empty());
    }

    /// Поколения: ответ из прошлого поколения в кэш не попадает.
    #[test]
    fn insert_result_ignores_a_stale_generation() {
        let loader = Loader::new(Fake::new(), 0);
        loader.bump_generation();
        let current = loader.generation();
        assert!(!loader.insert_result(
            path("old"),
            Thumb::solid(1, 1, [1, 0, 0, 255]),
            current - 1
        ));
        assert!(
            loader.get(&path("old")).is_none(),
            "устаревший ответ выброшен"
        );
        assert!(!loader.take_dirty(), "грязный флаг не взведён зря");
        assert!(loader.insert_result(path("new"), Thumb::solid(1, 1, [2, 0, 0, 255]), current));
        assert!(loader.get(&path("new")).is_some());
        assert!(loader.take_dirty());
    }

    /// Поколение растёт и очищает полёт: те же пути заказываются заново.
    #[test]
    fn bump_generation_clears_the_inflight_set() {
        let loader = Loader::new(Fake::new(), 0);
        loader.request(&[path("a")], &[]);
        loader.bump_generation();
        // Без очистки второй request решил бы, что путь уже летит.
        loader.request(&[path("a")], &[]);
        assert_eq!(loader.generation(), 1);
    }

    /// Сквозной путь на подставном источнике: заказ → get отдаёт готовое.
    #[test]
    fn request_flows_into_the_cache() {
        let loader = Loader::new(Fake::new(), 1);
        assert!(loader.get(&path("a")).is_none());
        loader.request(&[path("a")], &[path("b")]);
        let deadline = Instant::now() + Duration::from_secs(2);
        while loader.get(&path("b")).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(loader.get(&path("a")).is_some(), "видимая загрузилась");
        assert!(loader.get(&path("b")).is_some(), "соседняя загрузилась");
        assert!(loader.take_dirty());
    }

    /// Приоритет в потоке: один рабочий берёт очередь по FIFO, поэтому
    /// видимые оказываются в вызовах раньше соседей.
    #[test]
    fn visible_thumbs_load_before_prefetch() {
        let loader = Loader::new(Fake::new(), 1);
        let visible = vec![path("v1"), path("v2"), path("v3")];
        let prefetch = vec![path("p1"), path("p2"), path("p3")];
        loader.request(&visible, &prefetch);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let done = loader.get(&path("p3")).is_some();
            if done || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let calls = loader.source.calls();
        assert_eq!(calls.len(), 6, "все шесть загружены: {calls:?}");
        assert_eq!(
            &calls[..3],
            &visible[..],
            "первые три вызова — видимые: {calls:?}"
        );
    }

    /// Повторный заказ того же пути не дублирует задание.
    #[test]
    fn request_does_not_duplicate_inflight_jobs() {
        let loader = Loader::new(Fake::new(), 1);
        loader.request(&[path("a")], &[path("a")]);
        let deadline = Instant::now() + Duration::from_secs(2);
        while loader.get(&path("a")).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Даём потоку шанс выполнить дубль, если бы он был.
        std::thread::sleep(Duration::from_millis(50));
        let calls = loader.source.calls();
        assert_eq!(calls.len(), 1, "один путь — один вызов: {calls:?}");
    }

    /// Декодер на синтетическом PNG в памяти: без диска.
    #[test]
    fn decode_png_bytes_round_trips_rgba() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 4, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("заголовок");
            let pixels: Vec<u8> = (0..8)
                .flat_map(|i| [i * 10, i * 10 + 1, i * 10 + 2, 255])
                .collect();
            writer.write_image_data(&pixels).expect("пиксели");
        }
        let thumb = decode_png_bytes(&encoded).expect("декодируется");
        assert_eq!((thumb.w, thumb.h), (4, 2));
        assert_eq!(thumb.rgba[0..4], [0, 1, 2, 255]);
        assert_eq!(thumb.rgba[thumb.rgba.len() - 4..], [70, 71, 72, 255]);
    }

    /// Серый PNG превращается в RGBA без паники.
    #[test]
    fn decode_png_bytes_handles_grayscale() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("заголовок");
            writer.write_image_data(&[10, 200]).expect("пиксели");
        }
        let thumb = decode_png_bytes(&encoded).expect("декодируется");
        assert_eq!((thumb.w, thumb.h), (2, 1));
        assert_eq!(thumb.rgba[0..4], [10, 10, 10, 255]);
        assert_eq!(thumb.rgba[4..8], [200, 200, 200, 255]);
    }

    /// Мусор — не миниатюра, а `None`.
    #[test]
    fn decode_png_bytes_rejects_garbage() {
        assert!(decode_png_bytes(b"definitely not png").is_none());
        assert!(decode_png_bytes(&[]).is_none());
    }

    /// Уменьшение держит пропорции и потолок.
    #[test]
    fn shrink_to_fit_keeps_aspect_inside_the_cap() {
        let big = Thumb::solid(400, 225, [7, 7, 7, 255]);
        let small = shrink_to_fit(big);
        assert!(small.w <= THUMB_MAX_W && small.h <= THUMB_MAX_H);
        let ratio = small.w as f32 / small.h as f32;
        assert!(
            (ratio - 400.0 / 225.0).abs() < 0.05,
            "пропорции держатся: {ratio}"
        );
        let tiny = Thumb::solid(100, 60, [7, 7, 7, 255]);
        let kept = shrink_to_fit(tiny);
        assert_eq!((kept.w, kept.h), (100, 60), "мелочь вверх не тянем");
    }

    /// Nearest: углы картинки остаются углами.
    #[test]
    fn scale_nearest_keeps_corners() {
        let thumb = Thumb {
            w: 2,
            h: 2,
            rgba: vec![10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255],
        };
        let scaled = scale_nearest(&thumb, 4, 4);
        assert_eq!((scaled.w, scaled.h), (4, 4));
        assert_eq!(scaled.rgba[0], 10, "левый верхний угол");
        assert_eq!(scaled.rgba[3 * 4], 20, "правый верхний угол");
        assert_eq!(scaled.rgba[scaled.rgba.len() - 4], 40, "правый нижний угол");
    }
}
