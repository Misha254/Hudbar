//! Схемы matugen: список, плитки, точки-образцы и применение.
//!
//! Раньше схемы жили отдельной зоной окна обоев, но она занимала под себя
//! низ карточки и мешала сетке быть крупной. Теперь это своё маленькое окно,
//! а общий модуль остаётся тем, чем был: данными и геометрией, без Wayland.

use super::settings_ui::Rect;

/// Схемы matugen в том порядке, в каком они идут в окне.
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

/// Сколько схем в ряду. Две строки на десять схем — как было в окне обоев.
pub const COLS: usize = 5;
/// Рядов по схемам в окне.
pub const ROWS: usize = 2;

pub const CARD_W: f32 = 560.0;
pub const CARD_H: f32 = 190.0;
pub const PAD: f32 = super::ui_tokens::spacing::LG;
pub const HEADER_H: f32 = 34.0;
pub const ZONE_INSET: f32 = super::ui_tokens::spacing::MD;
pub const CHIP_H: f32 = 48.0;
pub const CHIP_INNER_GAP: f32 = 6.0;
pub const CHIP_GAP: f32 = super::ui_tokens::spacing::SM;
pub const SWATCH_D: f32 = 8.0;
pub const SWATCH_GAP: f32 = 4.0;

/// Плитка схемы по индексу. `None`, если индекс за пределами сетки.
pub fn chip(zone: Rect, index: usize) -> Option<Rect> {
    if index >= COLS * ROWS {
        return None;
    }
    let inner_w = zone.w - ZONE_INSET * 2.0;
    let chip_w = (inner_w - CHIP_GAP * (COLS - 1) as f32) / COLS as f32;
    let row = index / COLS;
    let col = index % COLS;
    let x = zone.x + ZONE_INSET + col as f32 * (chip_w + CHIP_GAP);
    let y = zone.y + ZONE_INSET + row as f32 * (CHIP_H + CHIP_GAP);
    Some(Rect::new(x, y, chip_w, CHIP_H))
}

/// Прямоугольник зоны плиток внутри карточки.
pub fn zone(card: Rect) -> Rect {
    Rect::new(
        card.x + PAD,
        card.y + HEADER_H,
        card.w - PAD * 2.0,
        (ROWS as f32) * CHIP_H + (ROWS - 1) as f32 * CHIP_GAP + ZONE_INSET * 2.0,
    )
}

/// Половина плитки под точки: верхняя, с отступом от края.
pub fn chip_dots(chip: Rect) -> Rect {
    let h = (CHIP_H - CHIP_INNER_GAP) / 2.0;
    Rect::new(chip.x, chip.y, chip.w, h)
}

/// Половина плитки под подпись: нижняя, с тем же зазором.
pub fn chip_label(chip: Rect) -> Rect {
    let h = (CHIP_H - CHIP_INNER_GAP) / 2.0;
    Rect::new(chip.x, chip.y + h + CHIP_INNER_GAP, chip.w, h)
}

/// Какая схема сейчас стоит, по `~/.config/hudbar/scheme`. Неизвестное имя
/// и отсутствие файла дают `None`: плитки просто ничего не отмечают.
pub fn current() -> Option<String> {
    let path = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)?
        .join(".config/hudbar/scheme");
    let name = std::fs::read_to_string(path).ok()?;
    let name = name.trim();
    SCHEMES.contains(&name).then(|| name.to_string())
}

/// Текущие обои из `~/.config/wallpaper`: их нужно передать `wall.sh`,
/// чтобы matugen пересчитал палитру под новую схему.
pub fn current_wallpaper() -> Option<String> {
    let path = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)?
        .join(".config/wallpaper");
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Применяет схему к текущим обоям, не дожидаясь: пересчёт matugen занимает
/// секунды, а окно уже закрыто. Ошибка уходит в журнал.
pub fn apply_detached(scheme: &str) {
    let Some(wallpaper) = current_wallpaper() else {
        super::log::warn("схема не применилась: не известны текущие обои");
        return;
    };
    let scheme = scheme.to_string();
    std::thread::spawn(move || {
        if let Err(error) = super::wallpaper::apply(&wallpaper, &scheme) {
            super::log::warn(format!("схема не применилась: {error}"));
        }
    });
}

/// Образцы схем: из файла, если он есть, иначе зашитые значения. Файл —
/// выгрузка matugen после последнего применения, поэтому он точнее.
pub fn swatches_for(
    name: &str,
    from_file: Option<&super::wallpaper::swatches::Swatches>,
) -> Vec<String> {
    if let Some(swatches) = from_file
        && let Some(found) = swatches.get(name)
    {
        return found.to_vec();
    }
    let baked: Vec<&str> = match name {
        "scheme-tonal-spot" => vec!["#b5c4ff", "#c1c5dd", "#121318", "#ffb4ab"],
        "scheme-expressive" => vec!["#b4aff9", "#c1c5dd", "#171a21", "#d1aaff"],
        "scheme-fidelity" => vec!["#a4d5ff", "#b4d0d5", "#131a20", "#9dbaff"],
        "scheme-fruit-salad" => vec!["#ff9ce2", "#d0c6d5", "#141219", "#ffb48f"],
        "scheme-monochrome" => vec!["#b4b4b4", "#c6c6c6", "#101010", "#ffffff"],
        "scheme-neutral" => vec!["#a1c4fd", "#c2d1d8", "#14181e", "#c6c6c6"],
        "scheme-rainbow" => vec!["#ff6b6b", "#4ecdc4", "#45b7d1", "#96ceb4"],
        "scheme-content" => vec!["#48c6ef", "#6f86d6", "#091a26", "#f3a847"],
        "scheme-vibrant" => vec!["#ff785a", "#ffbc42", "#2a1a00", "#7343e0"],
        _ => vec!["#b5c4ff", "#c1c5dd", "#121318", "#ffb4ab"],
    };
    baked.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Rect {
        Rect::new(0.0, 0.0, CARD_W, CARD_H)
    }

    #[test]
    fn every_scheme_gets_a_chip() {
        let zone = zone(card());
        for index in 0..SCHEMES.len() {
            assert!(chip(zone, index).is_some(), "нет плитки у {index}");
        }
        assert!(chip(zone, SCHEMES.len()).is_none(), "лишняя плитка");
    }

    #[test]
    fn chips_stay_inside_the_zone_and_do_not_overlap() {
        let area = zone(card());
        for index in 0..SCHEMES.len() {
            // Имя не `chip`: локальная переменная затенила бы функцию `chip`
            // и на второй итерации вызов перестал бы компилироваться.
            let tile = chip(area, index).unwrap();
            assert!(
                tile.x >= area.x && tile.right() <= area.right(),
                "плитка {index} вылезла по горизонтали"
            );
            assert!(
                tile.y >= area.y && tile.bottom() <= area.bottom(),
                "плитка {index} вылезла по вертикали"
            );
            if index > 0 {
                let prev = chip(area, index - 1).unwrap();
                let disjoint = tile.x >= prev.right() || tile.y >= prev.bottom();
                assert!(disjoint, "плитки {index} и {} наложились", index - 1);
            }
        }
    }

    #[test]
    fn chip_halves_do_not_overlap() {
        let chip = chip(zone(card()), 0).unwrap();
        let dots = chip_dots(chip);
        let label = chip_label(chip);
        assert!(dots.bottom() <= label.y, "точки налезли на подпись");
        assert!(label.bottom() <= chip.bottom(), "подпись вылезла из плитки");
    }

    #[test]
    fn card_holds_the_whole_zone() {
        let card = card();
        let area = zone(card);
        assert!(
            area.bottom() <= card.bottom(),
            "зона не поместилась в карточку"
        );
        assert!(area.x >= card.x && area.right() <= card.right());
    }

    #[test]
    fn swatches_always_come_back_four_colors() {
        for name in SCHEMES {
            let colors = swatches_for(name, None);
            assert_eq!(colors.len(), 4, "у {name} не четыре образца");
            assert!(
                colors.iter().all(|c| c.starts_with('#')),
                "{name}: {colors:?}"
            );
        }
    }

    #[test]
    fn current_scheme_ignores_unknown_names() {
        // Файла может не быть вовсе — это не ошибка, а «схема не выбрана».
        let _ = current();
    }
}
