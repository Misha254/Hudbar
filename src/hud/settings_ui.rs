//! Раскладка окна настроек HUD: модули бара, строки, попадания и переход фокуса.
//! Только арифметика, без cosmic-text и Wayland — как `binds_layout`.

/// Модули бара, которые `hud-setting hud-toggle` умеет включать и выключать.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Tray,
    Weather,
    Webcam,
    Clock,
    Recorder,
    Battery,
    System,
    Audio,
    Network,
    Dnd,
}

impl Module {
    /// Порядок раскладки: сначала системные, потом индикаторы. Две колонки
    /// заполняются построчно, поэтому список читается по диагонали.
    pub const ALL: [Module; 10] = [
        Module::Tray,
        Module::Weather,
        Module::Webcam,
        Module::Clock,
        Module::Recorder,
        Module::Battery,
        Module::System,
        Module::Audio,
        Module::Network,
        Module::Dnd,
    ];

    /// Ключ в `settings.json` и в `hud-setting hud-toggle`.
    pub fn key(self) -> &'static str {
        match self {
            Module::Tray => "tray",
            Module::Weather => "weather",
            Module::Webcam => "webcam",
            Module::Clock => "clock",
            Module::Recorder => "recorder",
            Module::Battery => "battery",
            Module::System => "system",
            Module::Audio => "audio",
            Module::Network => "network",
            Module::Dnd => "dnd",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Module::Tray => "Трей",
            Module::Weather => "Погода",
            Module::Webcam => "Вебка",
            Module::Clock => "Часы",
            Module::Recorder => "Запись",
            Module::Battery => "Батарея",
            Module::System => "Система",
            Module::Audio => "Звук",
            Module::Network => "Сеть",
            Module::Dnd => "Не беспокоить",
        }
    }

    pub fn from_key(key: &str) -> Option<Module> {
        Module::ALL.into_iter().find(|m| m.key() == key)
    }
}

pub const HEIGHT_MIN: u32 = 24;
pub const HEIGHT_MAX: u32 = 48;

/// Пределы и шаг высоты: `hud-setting` и панель читают те же границы.
pub fn clamp_height(value: i64) -> u32 {
    value.clamp(HEIGHT_MIN as i64, HEIGHT_MAX as i64) as u32
}

/// Новая высота или `None`, если шаг ничего не меняет.
pub fn step_height(current: u32, delta: i32) -> Option<u32> {
    let next = clamp_height(current as i64 + delta as i64);
    (next != current).then_some(next)
}

/// Подпись и шрифт карточки оформления.
pub fn theme_card(pixel: bool) -> (&'static str, &'static str, &'static str) {
    if pixel {
        ("Pixel", "Minecraft Rus и чёткие границы", "Minecraft Rus")
    } else {
        (
            "Обычный",
            "Плавный системный стиль",
            "JetBrainsMono Nerd Font",
        )
    }
}

/// Интерактивный элемент окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Карточка оформления: `true` — pixel.
    Theme(bool),
    Toggle(Module),
    /// Кнопка высоты бара: −1 или +1.
    Height(i32),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }
}

/// Подсказки в подвале окна.
pub const HINTS: [&str; 4] = [
    "↑↓ выбрать",
    "←→ соседний",
    "Space применить",
    "Esc закрыть",
];
/// Шаг подсказок по горизонтали.
pub const HINT_GAP: f32 = 148.0;

/// Строка окна: либо элемент, либо заголовок/разделитель/подпись.
/// Позиции живут здесь же, чтобы рисование и hit-test не разъезжались.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    Theme {
        pixel: bool,
        rect: Rect,
    },
    Toggle {
        module: Module,
        rect: Rect,
    },
    /// Подпись «Высота панели» и её шкала: не кликаются, кликают кнопки.
    HeightLabel {
        y: f32,
        scale: Rect,
    },
    Height {
        dir: i32,
        rect: Rect,
    },
    Close {
        rect: Rect,
    },
    Header {
        text: &'static str,
        y: f32,
    },
    Rule {
        y: f32,
    },
    Status {
        y: f32,
    },
    Footer {
        y: f32,
    },
}

impl Row {
    pub fn rect(&self) -> Option<Rect> {
        match self {
            Row::Theme { rect, .. }
            | Row::Toggle { rect, .. }
            | Row::Height { rect, .. }
            | Row::Close { rect } => Some(*rect),
            _ => None,
        }
    }

    pub fn control(&self) -> Option<Control> {
        match self {
            Row::Theme { pixel, .. } => Some(Control::Theme(*pixel)),
            Row::Toggle { module, .. } => Some(Control::Toggle(*module)),
            Row::Height { dir, .. } => Some(Control::Height(*dir)),
            Row::Close { .. } => Some(Control::Close),
            _ => None,
        }
    }
}

// Геометрия окна. Рисование и hit-test берут числа отсюда, чтобы не разъезжаться.
pub const WIDTH: f32 = 660.0;
pub const HEIGHT: f32 = 704.0;
pub const PAD_X: f32 = 26.0;
const COL_W: f32 = 298.0;
const COL_GAP: f32 = 12.0;
const CARD_H: f32 = 110.0;
const ITEM_H: f32 = 28.0;
const ITEM_GAP: f32 = 4.0;
const STEP_W: f32 = 34.0;

const CARDS_Y: f32 = 92.0;
const TOGGLES_Y: f32 = 248.0;
const HEIGHT_Y: f32 = 460.0;
const SCALE_Y: f32 = 480.0;
const SCALE_W: f32 = 400.0;
const STATUS_Y: f32 = 520.0;
const HINTS_Y: f32 = 640.0;
const CLOSE_Y: f32 = 666.0;

/// Полная раскладка окна сверху вниз.
pub fn rows() -> Vec<Row> {
    let mut rows = vec![Row::Header {
        text: "Оформление системы",
        y: 58.0,
    }];
    for (i, pixel) in [false, true].into_iter().enumerate() {
        rows.push(Row::Theme {
            pixel,
            rect: Rect::new(PAD_X + i as f32 * (COL_W + COL_GAP), CARDS_Y, COL_W, CARD_H),
        });
    }

    rows.push(Row::Rule {
        y: CARDS_Y + CARD_H + 18.0,
    });
    rows.push(Row::Header {
        text: "Модули бара",
        y: 220.0,
    });
    for (i, module) in Module::ALL.into_iter().enumerate() {
        let col = i % 2;
        let line = i / 2;
        rows.push(Row::Toggle {
            module,
            rect: Rect::new(
                PAD_X + col as f32 * (COL_W + COL_GAP),
                TOGGLES_Y + line as f32 * (ITEM_H + ITEM_GAP),
                COL_W,
                ITEM_H,
            ),
        });
    }

    let after_toggles = TOGGLES_Y + (Module::ALL.len() / 2) as f32 * (ITEM_H + ITEM_GAP) + 12.0;
    rows.push(Row::Rule { y: after_toggles });
    rows.push(Row::Header {
        text: "Высота панели",
        y: 432.0,
    });
    rows.push(Row::HeightLabel {
        y: HEIGHT_Y,
        scale: Rect::new(PAD_X, SCALE_Y, SCALE_W, 6.0),
    });
    // кнопки у правого края: от любой колонки тумблеров вниз ведёт к «−»
    let minus = Rect::new(WIDTH - PAD_X - STEP_W * 2.0 - 8.0, HEIGHT_Y, STEP_W, ITEM_H);
    rows.push(Row::Height {
        dir: -1,
        rect: minus,
    });
    rows.push(Row::Height {
        dir: 1,
        rect: Rect::new(minus.right() + 8.0, HEIGHT_Y, STEP_W, ITEM_H),
    });

    rows.push(Row::Status { y: STATUS_Y });
    rows.push(Row::Footer { y: HINTS_Y });
    rows.push(Row::Close {
        rect: Rect::new(WIDTH - PAD_X - 86.0, CLOSE_Y, 86.0, 30.0),
    });
    rows
}

/// Что под курсором.
pub fn hit(rows: &[Row], x: f32, y: f32) -> Option<Control> {
    rows.iter().rev().find_map(|row| {
        row.rect()
            .filter(|rect| rect.contains(x, y))
            .and_then(|_| row.control())
    })
}

/// Соседний элемент: `dy` — по своей колонке, `dx` — между колонками.
/// Ищет ближайший по вертикали элемент в той же или соседней колонке,
/// поэтому и сетка тумблеров, и карточки, и кнопки высоты ходят одним кодом.
/// Вниз сначала ищем строго в своей колонке, а если там конец — берём ближайший
/// элемент снизу вообще: иначе низ левой колонки упирался бы в тупик.
pub fn neighbour(rows: &[Row], from: Control, dx: i32, dy: i32) -> Option<Control> {
    if dx == 0 && dy == 0 {
        return Some(from);
    }
    let from_rect = rows
        .iter()
        .find(|row| row.control() == Some(from))
        .and_then(Row::rect)?;
    let candidates: Vec<(Control, Rect)> = rows
        .iter()
        .filter_map(|row| Some((row.control()?, row.rect()?)))
        .filter(|(control, _)| *control != from)
        .collect();
    let pick = |same_column: bool| -> Option<(i32, Control)> {
        let mut best: Option<(i32, Control)> = None;
        for (control, rect) in &candidates {
            let step = if dy != 0 {
                if same_column && (rect.x - from_rect.x).abs() > 1.0 {
                    continue;
                }
                let diff = (rect.y - from_rect.y) as i32;
                if diff.signum() != dy.signum() {
                    continue;
                }
                // одинаковый y — это соседняя колонка, а не следующий ряд
                diff.abs() * 2
                    + if same_column {
                        0
                    } else {
                        (rect.x - from_rect.x).abs() as i32
                    }
            } else {
                if (rect.y - from_rect.y).abs() > ITEM_H {
                    continue;
                }
                let diff = (rect.x - from_rect.x) as i32;
                if diff.signum() != dx.signum() {
                    continue;
                }
                diff.abs() + ((rect.y - from_rect.y).abs() as i32) * 2
            };
            if best.is_none_or(|(best_step, _)| step < best_step) {
                best = Some((step, *control));
            }
        }
        best
    };
    let picked = if dy != 0 {
        pick(true).or_else(|| pick(false))
    } else {
        pick(true)
    };
    picked.map(|(_, control)| control)
}

/// Все элементы в порядке чтения — для Home/End и пересборки фокуса.
pub fn controls(rows: &[Row]) -> Vec<Control> {
    rows.iter().filter_map(Row::control).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls_of(rows: &[Row]) -> Vec<Control> {
        controls(rows)
    }

    #[test]
    fn modules_map_to_hud_setting_keys() {
        for module in Module::ALL {
            assert_eq!(Module::from_key(module.key()), Some(module));
        }
        assert_eq!(Module::from_key("nope"), None);
        assert_eq!(Module::ALL.len(), 10);
    }

    #[test]
    fn height_is_clamped_and_step_reports_change() {
        assert_eq!(clamp_height(10), HEIGHT_MIN);
        assert_eq!(clamp_height(999), HEIGHT_MAX);
        assert_eq!(step_height(30, 1), Some(31));
        assert_eq!(step_height(30, -1), Some(29));
        assert_eq!(step_height(HEIGHT_MAX, 1), None);
        assert_eq!(step_height(HEIGHT_MIN, -1), None);
    }

    #[test]
    fn layout_has_every_control_once() {
        let rows = rows();
        let list = controls_of(&rows);

        assert_eq!(list.len(), Module::ALL.len() + 5);
        assert_eq!(
            list.iter()
                .filter(|c| matches!(c, Control::Theme(_)))
                .count(),
            2
        );
        assert_eq!(
            list.iter()
                .filter(|c| matches!(c, Control::Toggle(_)))
                .count(),
            10
        );
        assert_eq!(
            list.iter()
                .filter(|c| matches!(c, Control::Height(_)))
                .count(),
            2
        );
        assert_eq!(list.iter().filter(|c| **c == Control::Close).count(), 1);
    }

    #[test]
    fn toggles_fill_two_columns_row_major() {
        let rows = rows();
        let toggles: Vec<(Module, Rect)> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Toggle { module, rect } => Some((*module, *rect)),
                _ => None,
            })
            .collect();

        // порядок в списке — по строкам, значим y растёт через каждые два
        for (i, (_, rect)) in toggles.iter().enumerate() {
            let expected_col = i % 2;
            let expected_line = i / 2;
            assert_eq!(rect.x, PAD_X + expected_col as f32 * (COL_W + COL_GAP));
            assert_eq!(
                rect.y,
                TOGGLES_Y + expected_line as f32 * (ITEM_H + ITEM_GAP)
            );
        }
        // строки не наезжают
        for pair in toggles.windows(2) {
            let (a, b) = (pair[0].1, pair[1].1);
            if a.x == b.x {
                assert!(b.y >= a.y + ITEM_H);
            }
        }
    }

    #[test]
    fn hit_finds_controls_and_misses_the_gaps() {
        let rows = rows();
        let toggle = rows
            .iter()
            .find_map(|row| match row {
                Row::Toggle { module, rect } if *module == Module::Weather => Some(*rect),
                _ => None,
            })
            .expect("тумблер погоды");

        let center = Control::Toggle(Module::Weather);
        assert_eq!(hit(&rows, toggle.x + 4.0, toggle.y + 4.0), Some(center));
        assert_eq!(
            hit(&rows, toggle.right() - 1.0, toggle.y + toggle.h - 1.0),
            Some(center)
        );
        // пустое место между строками и под шапкой
        assert_eq!(hit(&rows, toggle.x + 4.0, 20.0), None);
        assert_eq!(hit(&rows, toggle.x + 4.0, toggle.y + toggle.h + 1.0), None);
    }

    #[test]
    fn focus_walks_columns_and_rows() {
        let rows = rows();

        // карточки оформления — соседи по горизонтали
        assert_eq!(
            neighbour(&rows, Control::Theme(false), 1, 0),
            Some(Control::Theme(true))
        );
        assert_eq!(
            neighbour(&rows, Control::Theme(true), -1, 0),
            Some(Control::Theme(false))
        );
        // за краем карточек пусто
        assert_eq!(neighbour(&rows, Control::Theme(false), -1, 0), None);
        assert_eq!(neighbour(&rows, Control::Theme(false), 0, -1), None);

        // тумблеры: вниз по своей колонке, вбок — в соседнюю
        assert_eq!(
            neighbour(&rows, Control::Toggle(Module::Tray), 0, 1),
            Some(Control::Toggle(Module::Webcam))
        );
        assert_eq!(
            neighbour(&rows, Control::Toggle(Module::Tray), 1, 0),
            Some(Control::Toggle(Module::Weather))
        );
        assert_eq!(
            neighbour(&rows, Control::Toggle(Module::Weather), -1, 0),
            Some(Control::Toggle(Module::Tray))
        );
        // с карточки оформления вниз — первый тумблер своей колонки
        assert_eq!(
            neighbour(&rows, Control::Theme(false), 0, 1),
            Some(Control::Toggle(Module::Tray))
        );
        // низ сетки вниз ведёт к кнопкам высоты: своей колонкой или ближайшей
        assert_eq!(
            neighbour(&rows, Control::Toggle(Module::Audio), 0, 1),
            Some(Control::Toggle(Module::Dnd))
        );
        assert_eq!(
            neighbour(&rows, Control::Toggle(Module::Dnd), 0, 1),
            Some(Control::Height(-1))
        );
        // из левой колонки своей нет — берём ближайшую кнопку высоты
        assert_eq!(
            neighbour(&rows, Control::Toggle(Module::Network), 0, 1),
            Some(Control::Height(-1))
        );
        // кнопки высоты — соседи по горизонтали
        assert_eq!(
            neighbour(&rows, Control::Height(-1), 1, 0),
            Some(Control::Height(1))
        );
        assert_eq!(
            neighbour(&rows, Control::Height(1), 0, -1),
            Some(Control::Toggle(Module::Dnd))
        );
    }

    #[test]
    fn focus_can_reach_every_control() {
        let rows = rows();
        let all = controls_of(&rows);
        let mut reached = vec![Control::Theme(false)];
        let mut queue = vec![Control::Theme(false)];
        while let Some(current) = queue.pop() {
            for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                if let Some(next) = neighbour(&rows, current, dx, dy)
                    && !reached.contains(&next)
                {
                    reached.push(next);
                    queue.push(next);
                }
            }
        }
        for control in all {
            assert!(reached.contains(&control), "недостижим элемент {control:?}");
        }
    }
}
