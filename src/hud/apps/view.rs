//! Отрисовка окна приложений: карточка, шапка с поиском, строки с иконками
//! или сетка плиток, подвал.
//!
//! Геометрия и цвета взяты у карточки меню (`menu::view`), поэтому окно
//! выглядит частью того же HUD, а не отдельной программой: та же ширина, та же
//! высота шапки и подвала, тот же ход подсветки, тот же ползунок прокрутки.
//!
//! Отличие одно — вместо глифа Nerd Font настоящая иконка приложения. В сетке
//! подписей нет: имя выбранного приложения показывает подвал. Заглушка на месте
//! иконки — кружок: он означает «иконка ещё едет» и не выдаётся ни за букву, ни
//! за значок категории.

use super::super::menu::view::RowsHighlight;
use super::icons::{self, Loader};
use super::model::{GRID_COLS, MAX_TILE_ROWS, MIN_TILE_ROWS, Model, Row, ViewMode};

pub use super::super::canvas::{fill_rect, fill_round_rect, stroke_rect};
use super::super::geom::Rect;
/// Прозрачность карточки — общая с менем: правило одна, а две реализации
/// разъехались бы на пиксель.
pub use super::super::menu::view::apply_card_alpha;
use super::super::palette::{self, Rgba};
use super::super::settings::Language;
use super::super::settings_icons;
use super::super::text::{Align, SCALE, TextPainter};
use super::super::ui_tokens::{
    TypeScale, UiPalette, radii, radius, spacing, type_scale, ui_palette,
};

#[path = "view/draw.rs"]
mod draw;
#[path = "view/layout.rs"]
mod layout;
pub(crate) use draw::*;
pub(crate) use layout::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::model::{Model, Row};
    use crate::apps::scan::App;
    use std::path::PathBuf;

    fn app(id: &str, name: &str) -> App {
        App {
            id: id.into(),
            name: name.into(),
            name_plain: name.into(),
            generic: String::new(),
            keywords: Vec::new(),
            exec: "true".into(),
            icon: String::new(),
            terminal: false,
            dbus_activatable: false,
            path: PathBuf::from(format!("{id}.desktop")),
        }
    }

    fn model(apps: Vec<App>, pinned: Vec<String>) -> Model {
        Model::from_apps(apps, pinned, MAX_ROWS, Language::Ru)
    }

    fn many(count: usize) -> Vec<App> {
        (0..count)
            .map(|i| app(&format!("app{i}"), &format!("App {i}")))
            .collect()
    }

    /// Модель из двух секций и 45 приложений: 5 закреплённых плюс 40 прочих.
    fn two_sections() -> Model {
        let apps = many(45);
        let pinned = vec!["app0".to_string(), "app1".to_string(), "app2".to_string()];
        model(apps, pinned)
    }

    // ───────────────────────── счётчик ─────────────────────────

    /// Заголовки секций — не приложения, поэтому «выбрано 1 из 45» считает и
    /// позицию, и итог по приложениям, а не по строкам списка.
    #[test]
    fn the_counter_counts_apps_not_rows() {
        let mut model = two_sections();
        assert_eq!(model.app_count(), 45);

        model.select(model.first_app_row());
        assert_eq!(counter(&model), "выбрано 1 из 45");

        model.jump(true);
        assert_eq!(counter(&model), "выбрано 45 из 45");
    }

    /// При поиске секций нет, и счётчик считает только то, что нашлось.
    #[test]
    fn the_counter_follows_the_filtered_list() {
        let mut model = two_sections();
        for ch in "app4".chars() {
            model.type_char(ch);
        }
        // Под этот запрос подходят только приложения с цифрой 4 в id.
        let found = model.app_count();
        assert!(
            found > 0 && found < 45,
            "запрос что-то отфильтровал: {found}"
        );
        assert_eq!(counter(&model), format!("выбрано 1 из {found}"));
        model.jump(true);
        assert_eq!(counter(&model), format!("выбрано {found} из {found}"));
    }

    #[test]
    fn the_counter_speaks_both_languages() {
        assert_eq!(counter_text(Language::Ru, 7, 45), "выбрано 7 из 45");
        assert_eq!(counter_text(Language::En, 7, 45), "selected 7 of 45");
    }

    /// На заголовке секции счётчика нет: выбрать заголовок нельзя, а пустая
    /// строка в подвале читалась бы как сбой.
    #[test]
    fn an_unselectable_row_has_no_counter() {
        let model = two_sections();
        let header = model
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Section(_)))
            .expect("заголовок есть");
        assert_eq!(model.app_position(header), None);
        assert_eq!(
            counter_text(Language::Ru, 1, model.app_count()),
            "выбрано 1 из 45"
        );
        let empty =
            super::super::model::Model::from_apps(Vec::new(), Vec::new(), MAX_ROWS, Language::Ru);
        assert!(counter(&empty).is_empty(), "пустой список — пустой счётчик");
    }

    // ───────────────────────── подвал ─────────────────────────

    /// Подсказки укладываются в карточку 440 px целиком, на обоих языках и в
    /// обоих видах: если нет — окно начало бы прятать половину подсказок по
    /// умолчанию.
    #[test]
    fn the_full_hint_level_fits_the_card_in_both_languages_and_views() {
        let mut view = view_for();
        for mode in [ViewMode::List, ViewMode::Tiles] {
            for lang in [Language::Ru, Language::En] {
                let levels = hint_levels(mode, lang);
                let full = hints_width(&mut view, levels[0]);
                assert!(
                    full <= CARD_W - PAD * 2.0,
                    "{mode:?}/{lang:?}: полный уровень не влезает: {full}"
                );
                let shown = fit_hints(&mut view, CARD_W - PAD * 2.0, mode, lang);
                assert_eq!(
                    shown,
                    levels[0].to_vec(),
                    "{mode:?}/{lang:?}: в карточку влезает полный уровень, а показан не он"
                );
            }
        }
    }

    /// Когда места мало, подсказки сокращаются до следующего уровня, а не
    /// обрезаются многоточием.
    #[test]
    fn the_hints_shrink_instead_of_being_cut() {
        let mut view = view_for();
        let levels = hint_levels(ViewMode::List, Language::Ru);
        let medium = hints_width(&mut view, levels[1]);
        let tight = medium + 1.0;
        assert_eq!(
            fit_hints(&mut view, tight, ViewMode::List, Language::Ru),
            levels[1].to_vec(),
            "на ширине между уровнями берётся средний"
        );

        // Уже не влезает даже самый короткий набор — берётся то, что влезло.
        let shortest = hints_width(&mut view, levels[2]);
        let shown = fit_hints(&mut view, shortest - 4.0, ViewMode::List, Language::Ru);
        assert_eq!(shown, vec!["Tab"]);
        assert!(
            hints_width(&mut view, &shown) <= shortest - 4.0,
            "подсказки не выходят за отведённую ширину"
        );
        // Совсем без места не рисуем ничего, но и не рисуем за краем карточки.
        assert!(fit_hints(&mut view, 0.0, ViewMode::List, Language::Ru).is_empty());
    }

    /// Правая зона зарезервирована по образцам — счётчику и короткому статусу, —
    /// иначе подсказки прыгали бы при появлении статуса. Длинное имя
    /// приложения в статусе место сужает один раз, но не наезжает.
    #[test]
    fn the_right_zone_is_reserved_for_the_samples() {
        let mut view = view_for();
        let model = two_sections();
        let zone = right_zone_width(&mut view, &model);
        for sample in ["выбрано 45 из 45", "выбрано 100 из 100"] {
            let width = view.painter.text_width(sample, view.scale.caption);
            assert!(
                zone >= width,
                "счётчик «{sample}» должен помещаться: {zone} < {width}"
            );
        }
        for sample in status_samples(Language::Ru) {
            let width = view.painter.text_width(sample, view.scale.caption) + 6.0 + spacing::XS;
            assert!(
                zone >= width,
                "статус «{sample}» должен помещаться: {zone} < {width}"
            );
        }
    }

    /// Длинный статус (имя приложения) сужает подвал один раз — иначе он наехал
    /// бы на подсказки или обрезал их.
    #[test]
    fn a_long_status_shrinks_the_hints_instead_of_overlapping() {
        let mut view = view_for();
        let mut model = two_sections();
        model.set_status("Откреплено: An Anime Game Launcher".to_string());
        let zone = right_zone_width(&mut view, &model);
        let status_width = view
            .painter
            .text_width("Откреплено: An Anime Game Launcher", view.scale.caption);
        assert!(
            zone >= status_width,
            "статус должен помещаться: {zone} < {status_width}"
        );
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.view(), model.lang());
        assert!(
            hints_width(&mut view, &shown) <= available,
            "подсказки обязаны уместиться рядом с длинным статусом"
        );
    }

    /// Обычное состояние: полный набор не влезает, средний — да, и именно он
    /// показывается. Переключение вида и закрепление на месте.
    #[test]
    fn the_footer_shows_the_medium_hints_at_card_width() {
        let mut view = view_for();
        let model = two_sections();
        let zone = right_zone_width(&mut view, &model);
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.view(), model.lang());
        let levels = hint_levels(ViewMode::List, Language::Ru);
        assert_eq!(
            shown, levels[1],
            "средний уровень должен влезать при 440 px"
        );
        assert!(shown.contains(&"↑↓ выбор"));
        assert!(shown.contains(&"Ctrl+F"));
        assert!(shown.contains(&"Tab"));
        assert!(
            hints_width(&mut view, levels[0]) > available,
            "полный уровень на карточке не помещается — иначе проверка выше врала бы"
        );
    }

    /// Длинное имя плитки в подвале не съедает подсказки целиком: оно
    /// обрезается, а Ctrl+F и Tab остаются на месте.
    #[test]
    fn a_long_tile_name_still_leaves_the_hints() {
        let mut view = view_for();
        let mut model = model(vec![app("a", "AppImageLauncher Settings")], Vec::new());
        model.toggle_view();
        let zone = right_zone_width(&mut view, &model);
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.view(), model.lang());
        assert!(
            shown.contains(&"Ctrl+F") && shown.contains(&"Tab"),
            "подсказки схлопнулись из-за имени: {shown:?}"
        );
    }

    // ───────────────────────── заглушка ─────────────────────────

    /// Смещение к акценту ограничено потолком: на полном смешении плитка
    /// становилась белой и читалась как значок, которого нет.
    #[test]
    fn the_placeholder_never_reaches_the_accent() {
        assert_eq!(placeholder_share(1.0), PLACEHOLDER_MIX);
        assert_eq!(placeholder_share(0.5), 0.175);
        assert_eq!(placeholder_share(0.0), 0.0);
        assert_eq!(
            placeholder_share(2.0),
            0.35,
            "смешение не выходит за потолок"
        );
    }

    /// Провал и ожидание рисуются по-разному: глиф появляется только когда
    /// иконки уже не будет.
    #[test]
    fn the_failed_placeholder_carries_a_glyph_and_the_pending_one_does_not() {
        assert_eq!(placeholder_glyph(true), Some(settings_icons::APPS));
        assert_eq!(placeholder_glyph(false), None);
    }

    /// Заглушка — графика, а не текст: порог 3:1, а не 4.5:1, и проверка роли
    /// идёт по всем модельным схемам в тестах `ui_tokens`.
    #[test]
    fn the_placeholder_keeps_the_graphic_threshold_on_the_default_scheme() {
        let ui = ui_palette(palette::Palette::default());
        let ratio = crate::ui_tokens::contrast(ui.placeholder, ui.panel);
        assert!(ratio >= 3.0, "заглушка к карточке: {ratio}");
    }

    // ───────────────────────── булавка ─────────────────────────

    /// Закреплённое приложение носит булавку везде, включая свою секцию:
    /// это единый признак закреплённости во всём списке.
    #[test]
    fn the_pin_shows_inside_the_pinned_section() {
        let mut model = two_sections();
        let pinned_row = model
            .rows()
            .iter()
            .position(|row| matches!(row, Row::App(_)))
            .expect("первое приложение");
        assert!(model.in_pinned_section(pinned_row));
        model.select(pinned_row);
        assert!(
            show_pin(&model, pinned_row, false),
            "в своей секции булавка тоже рисуется"
        );
        assert!(
            show_pin(&model, pinned_row, true),
            "на выбранной строке булавка остаётся"
        );
    }

    /// Незакреплённые строки булавку не получают нигде — ни обычные, ни
    /// выбранные. В списке с секциями закреплённое приложение живёт только в
    /// своей секции, поэтому здесь проверяется обратное: ни одна строка второй
    /// секции булавку не получает. Случай «закреплённая строка вне своей
    /// секции» — это поиск, он разобран следующим тестом.
    #[test]
    fn unpinned_rows_carry_no_pin() {
        let model = two_sections();
        let outside: Vec<usize> = model
            .rows()
            .iter()
            .enumerate()
            .filter(|(index, row)| matches!(row, Row::App(_)) && !model.in_pinned_section(*index))
            .map(|(index, _)| index)
            .collect();
        assert!(!outside.is_empty(), "вторая секция должна быть непустой");
        for row in outside {
            assert!(
                !show_pin(&model, row, false),
                "строка {row} вне закреплённых не должна носить булавку"
            );
            assert!(
                !show_pin(&model, row, true),
                "и на выбранной такой строке булавки нет: она не закреплена"
            );
        }
    }

    /// При поиске секций нет вовсе: любая закреплённая строка показывает
    /// булавку.
    #[test]
    fn the_pin_shows_while_searching() {
        let mut model = two_sections();
        for ch in "App".chars() {
            model.type_char(ch);
        }
        assert!(model.app_count() > 0);
        let pinned = model
            .rows()
            .iter()
            .position(|row| {
                let Row::App(index) = row else {
                    return false;
                };
                model.apps()[*index].id == "app0"
            })
            .expect("закреплённое среди результатов");
        assert!(
            !model.in_pinned_section(pinned),
            "при поиске секций нет вовсе"
        );
        assert!(show_pin(&model, pinned, false));
    }

    /// На заголовке секции булавки быть не может: строки-приложения там нет.
    #[test]
    fn a_section_header_has_no_pin() {
        let model = two_sections();
        let header = model
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Section(_)))
            .expect("заголовок есть");
        assert!(!show_pin(&model, header, true));
    }

    // ───────────────────────── геометрия ─────────────────────────

    #[test]
    fn the_card_grows_with_the_list_but_no_further_than_the_window() {
        assert_eq!(
            rows_shown(1),
            MIN_ROWS,
            "короткий список не схлопывает карточку"
        );
        assert_eq!(rows_shown(MAX_ROWS), MAX_ROWS);
        assert_eq!(rows_shown(100), MAX_ROWS, "длинный список прокручивается");
        assert_eq!(tile_rows_shown(1), MIN_TILE_ROWS);
        assert_eq!(tile_rows_shown(100), MAX_TILE_ROWS);
        assert!(
            card_height(&model(many(100), Vec::new())) > card_height(&model(many(1), Vec::new()))
        );
    }

    #[test]
    fn the_layer_size_matches_the_card() {
        let short = model(many(1), Vec::new());
        let (width, height) = layer_size(&short);
        assert_eq!(width, CARD_W as u32);
        assert_eq!(height, card_height(&short).round() as u32);
        let tall = model(many(100), Vec::new());
        let (_, tall) = layer_size(&tall);
        assert!(tall > height);
    }

    #[test]
    fn the_search_field_is_pinned_to_the_right_of_the_header() {
        let model = model(many(1), Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let field = search_rect(Rect::new(card.x, card.y, card.w, HEADER_H));
        assert_eq!(field.right(), card.right() - PAD);
        assert!(field.w <= SEARCH_W);
        assert!(field.y >= card.y && field.bottom() <= card.y + HEADER_H);
    }

    /// Клик должен выбирать строку окна, а не строку списка: при прокрутке это
    /// разные индексы, и ошибка здесь означала бы запуск не того приложения.
    #[test]
    fn a_click_picks_the_row_under_the_pointer() {
        let mut model = model(many(20), Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));

        let list = list_rect(card, &model);
        let hit = row_at(&model, card, 10.0, list.y + ROW_H * 1.5).expect("строка");
        assert_eq!(hit, 1);

        model.select(5);
        assert_eq!(model.top(), 0);
        model.select(12);
        assert_eq!(model.top(), 3);
        let hit = row_at(&model, card, 10.0, list.y + 2.0).expect("строка");
        assert_eq!(hit, 3, "первая видимая строка — это top");

        assert!(row_at(&model, card, 10.0, card.bottom() - 5.0).is_none());
        assert!(row_at(&model, card, 10.0, 2.0).is_none(), "клик в шапку");
    }

    #[test]
    fn a_click_below_the_last_row_hits_nothing() {
        let model = model(vec![app("a", "A")], Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        assert!(row_at(&model, card, 10.0, list.y + ROW_H * 3.5).is_none());
    }

    /// Четыре плитки и три зазора должны ровно заполнить ширину карточки без
    /// отступов: иначе сетка съедет вправо или не влезет в последнюю колонку.
    #[test]
    fn four_tiles_fit_the_card_exactly() {
        let row = GRID_COLS as f32 * TILE + (GRID_COLS - 1) as f32 * TILE_GAP;
        assert_eq!(row, CARD_W - PAD * 2.0);
    }

    /// Клик по плитке выбирает её, клик в зазор — ничего: иначе запускалось бы
    /// приложение, по которому не попали.
    #[test]
    fn a_click_picks_the_tile_under_the_pointer() {
        let mut model = model(many(20), Vec::new());
        model.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        let step = TILE + TILE_GAP;

        let hit = row_at(&model, card, list.x + TILE / 2.0, list.y + TILE / 2.0).expect("плитка");
        assert_eq!(hit, 0);
        let hit = row_at(&model, card, list.x + step + 1.0, list.y + 1.0).expect("плитка");
        assert_eq!(hit, 1);
        let hit =
            row_at(&model, card, list.x + 3.0 * step + 1.0, list.y + step + 1.0).expect("плитка");
        assert_eq!(hit, GRID_COLS + 3);
        assert!(
            row_at(&model, card, list.x + TILE + 1.0, list.y + TILE / 2.0).is_none(),
            "зазор между плитками — не плитка"
        );
        assert!(row_at(&model, card, list.x + 1.0, 2.0).is_none(), "шапка");

        model.select(19);
        assert_eq!(model.top(), GRID_COLS);
        let hit = row_at(&model, card, list.x + 1.0, list.y + 1.0).expect("плитка");
        assert_eq!(hit, model.top(), "клик по первой видимой плите — это top");
    }

    /// Ползунок живёт в поле карточки справа и не ложится на последнюю колонку
    /// плиток.
    #[test]
    fn the_scrollbar_does_not_cover_the_tiles() {
        let mut model = model(many(40), Vec::new());
        model.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        let track = scrollbar_rect(
            list,
            card.right(),
            model.total(),
            model.top(),
            model.visible().len(),
        )
        .expect("список длиннее окна");
        assert!(track.x >= list.right(), "ползунок за сеткой");
    }

    /// Пустая клетка после ряда закреплённых не кликается: она не приложение,
    /// и запускать по ней нечего.
    #[test]
    fn a_click_on_the_padding_cell_does_nothing() {
        let pinned = vec!["app0".to_string(), "app1".to_string(), "app2".to_string()];
        let mut model = model(many(20), pinned);
        model.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        let x = list.x + 3.0 * (TILE + TILE_GAP) + TILE / 2.0;
        assert!(row_at(&model, card, x, list.y + TILE / 2.0).is_none());
    }

    /// Подпись «Все приложения» получает собственный зазор: ряд под ней
    /// отодвигается, а клик в этот зазор ничего не выбирает.
    #[test]
    fn the_section_label_gets_its_own_gap() {
        let pinned = vec!["app0".to_string(), "app1".to_string(), "app2".to_string()];
        let mut grid = model(many(20), pinned);
        grid.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&grid));
        let list = list_rect(card, &grid);
        let above = tile_rect(list, 0, &grid);
        let below = tile_rect(list, GRID_COLS, &grid);

        assert_eq!(
            below.y - above.y,
            TILE + TILE_GAP + SECTION_EXTRA,
            "под подписью должен быть дополнительный зазор"
        );
        assert!(
            row_at(&grid, card, list.x + TILE / 2.0, above.bottom() + 1.0).is_none(),
            "зазор с подписью — не плитка"
        );

        let mut flat = model(many(20), Vec::new());
        flat.toggle_view();
        assert_eq!(
            card_height(&grid),
            card_height(&flat) + SECTION_EXTRA,
            "разделитель удлиняет карточку ровно на свой зазор"
        );

        let height = card_height(&grid);
        grid.select(grid.total().saturating_sub(1));
        assert_eq!(
            card_height(&grid),
            height,
            "прокрутка не должна менять высоту окна"
        );
    }

    /// В сетке подпись выбранного приложения живёт в подвале: на плитках
    /// только иконки, и без подписи одинаковые значки не различить.
    #[test]
    fn the_tiles_footer_names_the_selected_app() {
        let mut model = model(vec![app("a", "Alpha"), app("b", "Beta")], Vec::new());
        model.toggle_view();
        model.select(1);
        assert_eq!(right_label(&model), ("Beta".to_string(), false));
        model.set_status("Закреплено: Beta".to_string());
        assert_eq!(right_label(&model), ("Закреплено: Beta".to_string(), true));

        // Имя плитки обрезается по потолку, статус — нет: длинный статус
        // сужает подсказки, но не теряет текст.
        let mut view = view_for();
        let long = "Очень длинное имя приложения, которое не влезет";
        assert_eq!(right_text_width(&mut view, &model, long, false), NAME_MAX_W);
        assert!(right_text_width(&mut view, &model, long, true) > NAME_MAX_W);
    }

    #[test]
    fn section_rows_do_not_break_the_row_math() {
        let mut model = model(vec![app("a", "A"), app("b", "B")], vec!["a".to_string()]);
        assert!(matches!(model.rows()[0], Row::Section(_)));
        assert_eq!(model.total(), 4);
        model.select(3);
        assert_eq!(model.selected(), 3);
    }

    #[test]
    fn blending_keeps_the_endpoints_exactly() {
        let a = Rgba(10, 20, 30, 40);
        let b = Rgba(200, 210, 220, 230);
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
        assert_eq!(blend(a, b, 0.5).3, 135, "альфа тоже смешивается");
    }

    /// Вид для измерения подсказок: окно не нужно, нужен только painter.
    fn view_for() -> AppsView<'static> {
        // `TextPainter` живёт в куче и переживает создание вида: владеть им
        // внутри структуры, которая возвращается, нельзя, поэтому вид берёт
        //painter по ссылке на «вытекшую» память — под тест это безопасно, но
        // написано явно.
        let painter: &'static mut TextPainter =
            Box::leak(Box::new(TextPainter::new(&crate::ui_tokens::font_name())));
        let icons: &'static mut TextPainter =
            Box::leak(Box::new(TextPainter::new(settings_icons::FONT)));
        let pixmap: &'static mut tiny_skia::Pixmap =
            Box::leak(Box::new(tiny_skia::Pixmap::new(1, 1).expect("буфер")));
        let (ui, scale) = theme(palette::Palette::default());
        AppsView {
            pixmap,
            painter,
            icons,
            ui,
            scale,
        }
    }
}
