//! Модель состояния wallpaper-окна: зона, папка, фильтр, сетка, схемы и
//! исходы действий. Никакого Wayland: только данные и переходы.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::super::bind_data::transliterate;
use super::grid::{Dir, Grid};
use super::layout::SCHEME_COLS;
use super::scan::{ALL, Folder};

/// Три активные области окна (фокус выбора).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Folders,
    Grid,
    Schemes,
}

/// Куда перечислять по экрану.
pub const SCHEMES: [&str; 10] = [
    "scheme-tonal-spot",
    "scheme-expressive",
    "scheme-fidelity",
    "scheme-fruit-salad",
    "scheme-monochrome",
    "scheme-neutral",
    "scheme-rainbow",
    "scheme-content",
    "scheme-vibrant",
    "scheme-smart",
];

/// Подсветка под курсором: выбор не трогает, только вид. Клик уже решает,
// выбирать или применять, глядя и на неё, и на выбор.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hover {
    #[default]
    None,
    Folder(usize),
    Cell(usize),
    Scheme(usize),
}

/// Чем завершилось действие.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Ничего не делать дальше (используется, когда введено действие не в фокусе).
    None,
    /// Закрыть окно (Esc).
    Close,
    /// Применить: приклеить файл и схему.
    Apply { path: PathBuf, scheme: String },
}

/// Состояние окна.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub zone: Zone,
    pub folders: Vec<Folder>,
    pub all_files: Vec<PathBuf>,
    pub current_folder: String,
    pub files: Vec<PathBuf>,
    pub filtered: Vec<PathBuf>,
    pub grid: Grid,
    pub schemes: Vec<String>,
    pub scheme_sel: usize,
    pub current_scheme: String,
    pub current_wallpaper: Option<PathBuf>,
    pub query: String,
    pub folder_sel: usize,
    pub hover: Hover,
    /// Размеры файлов текущей папки: подписи ratio без ввода-вывода в рендере.
    // Пересчитывается при смене папки, а не каждый кадр.
    pub ratios: HashMap<PathBuf, (u32, u32)>,
    /// Цвета схем из файла: читается один раз при открытии, а не каждый кадр.
    pub swatch_map: Option<super::swatches::Swatches>,
}

impl State {
    /// Открывает состояние по реальной системе.
    pub fn open() -> Self {
        Self::open_with(None)
    }

    /// Открывает состояние сразу в папке: `hud-wallpaper-rs anime`. Папки нет
    /// или не передали — корневой вид «all», без ошибки.
    pub fn open_with(folder: Option<&str>) -> Self {
        let all = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("wallpapers"))
            .unwrap_or_default();
        let mut state = Self::open_at(all.as_path());
        if let Some(name) = folder
            && let Some(index) = state.folders.iter().position(|item| item.name == name)
        {
            state.select_folder(index);
            state.focus_current();
        }
        state
    }

    /// Открывает состояние по явному каталогу: актуально для тестов.
    pub fn open_at(root: &Path) -> Self {
        let scan = super::scan::scan(root);
        let current_wallpaper = read_current_wallpaper();
        let current_scheme = read_current_scheme()
            .and_then(|name| schemes_have(&name, name.as_str()).then_some(name));
        let schemes = SCHEMES
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        let scheme_sel = SCHEMES
            .iter()
            .position(|name| current_scheme.as_deref() == Some(*name))
            .unwrap_or(0);

        let mut folders = Vec::with_capacity(scan.folders.len() + 1);
        folders.push(Folder {
            name: ALL.to_string(),
            path: root.to_path_buf(),
            count: scan.total,
            files: scan.all.clone(),
        });
        folders.extend(scan.folders.clone());

        let mut new = State {
            zone: Zone::Grid,
            folders,
            all_files: scan.all,
            current_folder: ALL.to_string(),
            files: Vec::new(),
            filtered: Vec::new(),
            grid: Grid::new(4, 3, 0),
            schemes,
            scheme_sel,
            current_scheme: current_scheme.unwrap_or_else(|| SCHEMES[0].to_string()),
            current_wallpaper,
            query: String::new(),
            folder_sel: 0,
            hover: Hover::None,
            ratios: HashMap::new(),
            swatch_map: super::swatches::load(),
        };

        // Переключаться на реальный каталог только если уверены: файл
        // считает папка сразу, потом все-условно. Номера 1.. — не-ALL.
        if let Some(current) = new.current_wallpaper.as_ref() {
            let containing = new
                .folders
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, folder)| folder.files.contains(current))
                .map(|(index, _)| index);
            if let Some(index) = containing {
                new.folder_sel = index;
            }
        }
        new.select_folder(new.folder_sel);
        new.focus_current();
        new.set_current_scheme_index(new.scheme_sel);
        new
    }

    /// Ставит выбор на текущие обои, если они есть в списке папки. Без этого
    /// бейдж ACTIVE оказывался на второй странице и в снимке не был виден.
    fn focus_current(&mut self) {
        let Some(current) = self.current_wallpaper.as_ref() else {
            return;
        };
        let Some(index) = self
            .filtered
            .iter()
            .position(|path| path.as_path() == current.as_path())
        else {
            return;
        };
        self.grid.sel = index;
        self.grid.ensure_visible();
    }

    fn recount(&mut self) {
        let query_lower = transliterate(&self.query.to_lowercase());
        self.filtered = self
            .files
            .iter()
            .filter(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_lowercase();
                transliterate(&name).contains(&query_lower)
            })
            .cloned()
            .collect();
        self.grid.set_len(self.filtered.len());
    }

    /// Выбрать папку по индексу `all`-включающего списка.
    pub fn select_folder(&mut self, index: usize) {
        if index >= self.folders.len() {
            return;
        }
        self.folder_sel = index;
        self.current_folder = self.folders[index].name.clone();
        self.files = self.folders[index].files.clone();
        self.query.clear();
        self.hover = Hover::None;
        self.grid = Grid::new(self.grid.cols, self.grid.rows, self.files.len());
        self.recount();
        self.ratios = load_ratios(&self.files);
    }

    /// Размеры файлов папки одним проходом по заголовкам: подписи в кадре
    /// потом берутся из карты и диска не касаются.
    pub fn ratio_label(&self, path: &Path) -> String {
        self.ratios
            .get(path)
            .map(|(w, h)| super::imginfo::ratio_label(*w, *h))
            .unwrap_or_else(|| "—".to_string())
    }

    /// Прокрутка рядов колесом: окно едет, выбор подтягивается, чтобы не
    /// уехать из виду. Чисто вид, содержимое не меняется.
    pub fn scroll_rows(&mut self, delta: isize) {
        self.grid.scroll_rows(delta);
    }

    /// Пересобрать/подстроить строки для длины.
    fn recalc(&mut self) {
        // Helper to reset.
        self.recount();
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.recount();
    }

    /// Табличное переключение зоны.
    pub fn tab(&mut self) {
        self.zone = match self.zone {
            Zone::Folders => Zone::Grid,
            Zone::Grid => Zone::Schemes,
            Zone::Schemes => Zone::Folders,
        };
    }

    /// Переход по зонам из текущей активности не переключает её:
    /// переключает именно выбор внутри, но зона не меняется.
    pub fn move_dir(&mut self, dir: Dir) {
        match self.zone {
            Zone::Folders => match dir {
                // Папки зациклены, как и списки меню: вверх с первой — на
                // последнюю, вниз с последней — на первую.
                Dir::Up => {
                    let len = self.folders.len();
                    if len > 0 {
                        self.folder_sel = (self.folder_sel + len - 1) % len;
                    }
                }
                Dir::Down => {
                    let len = self.folders.len();
                    if len > 0 {
                        self.folder_sel = (self.folder_sel + 1) % len;
                    }
                }
                Dir::Left => {}
                Dir::Right => {}
                _ => {}
            },
            Zone::Grid => self.grid.move_dir(dir),
            Zone::Schemes => match dir {
                // Схемы — кольцо из чипов: ←/→ идут по кругу, ↑/↓ держат
                // колонку и тоже заворачивают с края на край.
                Dir::Left => {
                    let len = self.schemes.len();
                    if len > 0 {
                        self.scheme_sel = (self.scheme_sel + len - 1) % len;
                    }
                }
                Dir::Right => {
                    let len = self.schemes.len();
                    if len > 0 {
                        self.scheme_sel = (self.scheme_sel + 1) % len;
                    }
                }
                Dir::Up => {
                    let len = self.schemes.len();
                    if len > 0 {
                        if self.scheme_sel >= SCHEME_COLS {
                            self.scheme_sel -= SCHEME_COLS;
                        } else {
                            let col = self.scheme_sel % SCHEME_COLS;
                            self.scheme_sel =
                                ((len - 1) / SCHEME_COLS * SCHEME_COLS + col).min(len - 1);
                        }
                    }
                }
                Dir::Down => {
                    let len = self.schemes.len();
                    if len > 0 {
                        if self.scheme_sel + SCHEME_COLS < len {
                            self.scheme_sel += SCHEME_COLS;
                        } else {
                            self.scheme_sel = (self.scheme_sel % SCHEME_COLS).min(len - 1);
                        }
                    }
                }
                _ => {}
            },
        }
    }

    pub fn type_char(&mut self, ch: char) {
        self.query.push(ch);
        self.recalc();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.recalc();
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.recalc();
    }

    pub fn set_scheme(&mut self, scheme_index: usize) {
        self.scheme_sel = scheme_index.min(self.schemes.len().saturating_sub(1));
        self.set_current_scheme_index(self.scheme_sel);
    }

    fn set_current_scheme_index(&mut self, index: usize) {
        if let Some(scheme) = self.schemes.get(index) {
            self.current_scheme = scheme.clone();
        }
    }

    /// Enter в активной зоне.
    pub fn enter(&mut self) -> Outcome {
        match self.zone {
            Zone::Folders => {
                self.select_folder(self.folder_sel);
                Outcome::None
            }
            Zone::Schemes => {
                self.set_scheme(self.scheme_sel);
                Outcome::None
            }
            Zone::Grid => {
                if self.filtered.is_empty() {
                    return Outcome::None;
                }
                let index = self.grid.sel.min(self.filtered.len() - 1);
                Outcome::Apply {
                    path: self.filtered[index].clone(),
                    scheme: self.current_scheme.clone(),
                }
            }
        }
    }

    /// Esc: в активной зонеFolders закрывает всё; остальные сбрасывают фильтр?
    /// По управлению spec: Esc закрывает.
    pub fn escape(&self) -> Outcome {
        Outcome::Close
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.filtered.get(self.grid.sel)
    }
}

fn schemes_have(cur: &str, name: &str) -> bool {
    cur == name
}

/// Заголовки всех файлов папки: один `open` + префикс на файл. Вызывается при
/// смене папки, а не в кадре — рендер диска не касается.
fn load_ratios(files: &[PathBuf]) -> HashMap<PathBuf, (u32, u32)> {
    let mut ratios = HashMap::with_capacity(files.len());
    for path in files {
        if let Some(size) = super::imginfo::dimensions(path) {
            ratios.insert(path.clone(), size);
        }
    }
    ratios
}

fn read_current_wallpaper() -> Option<PathBuf> {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".config/wallpaper");
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(PathBuf::from(trimmed))
}

fn read_current_scheme() -> Option<String> {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".config/hudbar/scheme");
    std::fs::read_to_string(path).ok().and_then(|name| {
        let name = name.trim();
        if SCHEMES.contains(&name) {
            Some(name.to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Временное дерево обоев; каталог с pid уникален для параллельных тестов.
    fn mock_tree() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "hudbar-wp-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join("anime")).unwrap();
        std::fs::create_dir_all(root.join("pixelart/light")).unwrap();
        std::fs::write(root.join("anime/sakura.png"), b"x").unwrap();
        std::fs::write(root.join("anime/city.png"), b"x").unwrap();
        std::fs::write(root.join("pixelart/light/tile.png"), b"x").unwrap();
        std::fs::write(root.join("README.md"), "x").unwrap();
        (root.clone(), root)
    }

    /// Открытие по временному каталогу: папки посчитаны, схема прочитана,
    /// текущее изображение выставляет folder_sel.
    #[test]
    fn open_at_builds_folders_and_detects_current_folder() {
        let (_dir, root) = mock_tree();
        let state = State::open_at(&root);
        assert_eq!(
            state.current_folder, ALL,
            "без совпадающего пути текущая папка = all"
        );
        assert!(state.folders.iter().any(|folder| folder.name == "anime"));
        assert!(state.folders.iter().any(|folder| folder.name == "pixelart"));
        assert!(state.folders.iter().any(|folder| folder.count >= 3));
    }

    /// Tab переключает зоны по кругу.
    #[test]
    fn tab_cycles_zones() {
        let mut state = State::open();
        state.zone = Zone::Folders;
        state.tab();
        assert_eq!(state.zone, Zone::Grid);
        state.tab();
        assert_eq!(state.zone, Zone::Schemes);
        state.tab();
        assert_eq!(state.zone, Zone::Folders);
    }

    /// ←/→ внутри сетки двигают колlocaliz; фиксируем, что зона не меняется.
    #[test]
    fn left_right_in_grid_do_not_leave_the_grid_zone() {
        let (_dir, root) = mock_tree();
        let mut state = State::open_at(root.as_path());
        state.select_folder(1); // выберём первую не-all
        state.grid = Grid::new(4, 3, 20);
        state.filtered = (0..20)
            .map(|i| PathBuf::from(format!("/tmp/{}.png", i)))
            .collect();
        state.zone = Zone::Grid;
        state.grid.sel = 5;
        state.move_dir(Dir::Left);
        assert_eq!(
            state.zone,
            Zone::Grid,
            "зона не уходит в Schemes или Folders"
        );
        assert_eq!(state.grid.sel, 4);
    }

    /// Фильтр по подстроке имени, регистро/транслит не важен, пустой результат.
    #[test]
    fn filter_matches_substring_case_insensitively_and_translitaware() {
        let (_dir, root) = mock_tree();
        let mut state = State::open_at(root.as_path());
        state.select_folder(0); // all
        state.files = vec![
            PathBuf::from("/tmp/sakura.png"),
            PathBuf::from("/tmp/Sakura_Tile.png"),
            PathBuf::from("/tmp/city.png"),
        ];
        state.recount();
        state.set_query("sAkURa".into());
        assert_eq!(state.filtered.len(), 2, "регистр не важен");
        state.set_query("city".into());
        assert_eq!(state.filtered.len(), 1);
        state.set_query("zzz".into());
        assert_eq!(state.filtered.len(), 0, "ничего не найдено");
        assert!(
            matches!(state.enter(), Outcome::None),
            "пустой результат — Enter ничего не дал"
        );
    }

    /// Apply‑выход с путём и схемой для выбранного элемента.
    #[test]
    fn enter_returns_apply_with_selected_scheme_and_path() {
        let mut state = State::open();
        state.zone = Zone::Grid;
        state.filtered = vec![PathBuf::from("/tmp/wall.png")];
        state.grid = Grid::new(4, 3, 1);
        state.grid.sel = 0;
        state.current_scheme = "scheme-smart".into();
        let outcome = state.enter();
        match outcome {
            Outcome::Apply { path, scheme } => {
                assert_eq!(path, PathBuf::from("/tmp/wall.png"));
                assert_eq!(scheme, "scheme-smart");
            }
            other => panic!("ожидали Apply, получили {:?}", other),
        }
    }

    /// Esc всегда закрывает окно, независимо от зоны.
    #[test]
    fn escape_closes_from_any_zone() {
        let mut state = State::open();
        for zone in [Zone::Folders, Zone::Grid, Zone::Schemes] {
            state.zone = zone;
            assert_eq!(state.escape(), Outcome::Close);
        }
    }
}
