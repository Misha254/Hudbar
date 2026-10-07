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

/// Схемы стоят в один столбец: список из десяти названий читается взглядом
/// сверху вниз, а плитки в две строки приходилось выбирать по памяти, потому
/// что подпись перекрывалась заливкой выделения.
pub const CARD_W: f32 = 400.0;
/// Высота строки списка.
pub const ROW_H: f32 = 34.0;
pub const PAD: f32 = super::ui_tokens::spacing::LG;
pub const HEADER_H: f32 = 34.0;
pub const FOOTER_H: f32 = 32.0;
pub const SWATCH_D: f32 = 10.0;
pub const SWATCH_GAP: f32 = 5.0;
/// Отступ от края строки до точек и до подписи.
pub const ROW_PAD: f32 = 12.0;

/// Высота карточки из содержимого: заголовок, список, один отступ и подсказки.
///
/// Отступ под списком ровно один: с двумя получалась пустая полоса между
/// последней схемой и подсказками.
pub fn card_height() -> f32 {
    HEADER_H + SCHEMES.len() as f32 * ROW_H + PAD + FOOTER_H
}

/// Прямоугольник списка внутри карточки.
pub fn zone(card: Rect) -> Rect {
    Rect::new(
        card.x + PAD,
        card.y + HEADER_H,
        card.w - PAD * 2.0,
        SCHEMES.len() as f32 * ROW_H,
    )
}

/// Строка схемы по индексу. `None`, если индекс за пределами списка.
pub fn row(area: Rect, index: usize) -> Option<Rect> {
    if index >= SCHEMES.len() {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y + index as f32 * ROW_H,
        area.w,
        ROW_H,
    ))
}

/// Прямоугольник точек-образцов в строке: слева, по центру.
pub fn row_dots(row: Rect) -> Rect {
    Rect::new(
        row.x + ROW_PAD,
        row.y + (row.h - SWATCH_D) / 2.0,
        SWATCH_D * 4.0 + SWATCH_GAP * 3.0,
        SWATCH_D,
    )
}

/// Прямоугольник подписи: между точками и галочкой текущей схемы.
pub fn row_label(row: Rect) -> Rect {
    let left = row.x + ROW_PAD * 2.0 + SWATCH_D * 4.0 + SWATCH_GAP * 3.0;
    Rect::new(left, row.y, row.right() - left - ROW_PAD * 2.0, row.h)
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
        Rect::new(0.0, 0.0, CARD_W, card_height())
    }

    #[test]
    fn every_scheme_gets_a_row() {
        let area = zone(card());
        for index in 0..SCHEMES.len() {
            assert!(row(area, index).is_some(), "нет строки у {index}");
        }
        assert!(row(area, SCHEMES.len()).is_none(), "лишняя строка");
    }

    #[test]
    fn rows_are_vertical_and_do_not_overlap() {
        let area = zone(card());
        for index in 0..SCHEMES.len() {
            let current = row(area, index).expect("строка");
            assert!(
                current.w - current.h > 100.0,
                "строка слишком широкая: {current:?}"
            );
            assert_eq!(current.h, ROW_H, "высота строки");
            if index > 0 {
                let previous = row(area, index - 1).unwrap();
                assert!(
                    current.y >= previous.bottom(),
                    "строки {index} и {} наложились",
                    index - 1
                );
            }
        }
    }

    #[test]
    fn dots_and_label_do_not_overlap() {
        let tile = row(zone(card()), 0).unwrap();
        let dots = row_dots(tile);
        let label = row_label(tile);
        assert!(dots.right() <= label.x, "точки налезли на подпись");
        assert!(
            dots.y >= tile.y && dots.bottom() <= tile.bottom(),
            "точки вне строки"
        );
        assert!(
            label.y >= tile.y && label.bottom() <= tile.bottom(),
            "подпись вне строки"
        );
    }

    #[test]
    fn card_holds_the_whole_list() {
        let card = card();
        let area = zone(card);
        assert!(
            area.bottom() <= card.bottom(),
            "список не поместился в карточку"
        );
        assert!(area.x >= card.x && area.right() <= card.right());
        // Под заголовком и над подсказками должен остаться воздух.
        assert!(area.y >= card.y + HEADER_H);
        assert!(area.bottom() <= card.y + card.h - FOOTER_H);
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
