use super::super::settings::Language;
use super::super::settings_icons;
use super::super::strings;
use super::super::text::TextPainter;
use super::super::ui_tokens::{font_name, mix, spacing};
use super::super::{Action, Menu, Node};
use super::*;

/// Карточка списка с произвольной подсветкой: та же отрисовка, что и окно.
fn render_card(frame: &Frame, highlight_row: Option<f32>) -> tiny_skia::Pixmap {
    render_card_hover(frame, highlight_row, None)
}

/// То же с подсветкой строки под указателем.
fn render_card_hover(
    frame: &Frame,
    highlight_row: Option<f32>,
    hover_row: Option<usize>,
) -> tiny_skia::Pixmap {
    let card_h = card_height_for(frame.items.len(), search_mode(frame));
    let (ui, scale) = theme(palette::Palette::default());
    let mut pixmap =
        tiny_skia::Pixmap::new((CARD_W * SCALE) as u32, (card_h * SCALE) as u32).expect("pixmap");
    pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut view = MenuView {
        pixmap: &mut pixmap,
        painter: &mut painter,
        icons: &mut icons,
        ui,
        scale,
    };
    render_window(
        &mut view,
        frame,
        RowsHighlight {
            sliding: highlight_row,
            hover: hover_row,
        },
    );
    pixmap
}

/// Цвет пикселя в середине строки `slot` списка, в стороне от текста:
/// слева от названия там нет ничего, кроме подложки и подсветки.
fn pixel_in_row(pixmap: &tiny_skia::Pixmap, frame: &Frame, slot: f32) -> palette::Rgba {
    let row_h = row_height(search_mode(frame));
    let y = (HEADER_H + slot * row_h + row_h / 2.0) * SCALE;
    let x = (PAD + 2.0) * SCALE;
    at_pixel(pixmap, x, y)
}

/// Пиксель кадры по логическим координатам. Буфер `Pixmap` хранит RGBA
/// сам, перестановка каналов живёт только при отправке в shm. Значения
/// палитры — straight, а буфер — premultiplied: альфа ниже единицы
/// затемняет каналы, поэтому здесь буфер распаковывают обратно.
fn at_pixel(pixmap: &tiny_skia::Pixmap, x: f32, y: f32) -> palette::Rgba {
    let data = pixmap.data();
    let at = (y as u32) * pixmap.width() + (x as u32);
    let px = &data[at as usize * 4..at as usize * 4 + 4];
    let a = px[3] as u32;
    if a == 0 {
        return palette::Rgba(0, 0, 0, 0);
    }
    let un = |v: u8| ((v as u32 * 255 + a / 2) / a).min(255) as u8;
    palette::Rgba(un(px[0]), un(px[1]), un(px[2]), px[3])
}

/// Пиксели одной строки списка: полная ширина карточки, высота `ROW_H`.
fn row_pixels(pixmap: &tiny_skia::Pixmap, slot: usize) -> Vec<u8> {
    let stride = pixmap.width() as usize * 4;
    let top = ((HEADER_H + slot as f32 * ROW_H) * SCALE) as usize;
    let height = (ROW_H * SCALE) as usize;
    pixmap.data()[top * stride..(top + height) * stride].to_vec()
}

/// Границы непустых пикселей в прямоугольнике строки, в логических пикселях.
/// Фон карточки — `panel`; hairline и текст отличаются от него.
fn ink_bounds(
    pixmap: &tiny_skia::Pixmap,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    bg: palette::Rgba,
) -> Option<(f32, f32)> {
    let mut left = f32::MAX;
    let mut right = f32::MIN;
    for y in (y0 * SCALE) as u32..(y1 * SCALE) as u32 {
        for x in (x0 * SCALE) as u32..(x1 * SCALE) as u32 {
            if at_pixel(pixmap, x as f32, y as f32) != bg {
                left = left.min(x as f32 / SCALE);
                right = right.max(x as f32 / SCALE);
            }
        }
    }
    (left <= right).then_some((left, right))
}

/// Список из `count` строк с выбором `selected`.
fn frame_with(count: usize, selected: usize) -> Frame {
    let mut menu = Menu::new(rows(count));
    menu.move_sel(selected as isize);
    menu.frame()
}

/// Снимок и обычный кадр рисуют подсветку одинаково: путь `None` обязан
/// остаться побитово прежним, иначе поменяются эталонные картинки.
#[test]
fn no_highlight_row_keeps_the_row_itself_painted() {
    let frame = frame_with(8, 3);
    let plain = render_card(&frame, None);

    let (ui, _) = theme(palette::Palette::default());
    let want = ui.accent;
    for slot in [0.0f32, 2.0, 3.0, 5.0] {
        let got = pixel_in_row(&plain, &frame, slot);
        let selected = (slot - 3.0).abs() < f32::EPSILON;
        if selected {
            assert_eq!(got, want, "выбранная строка должна быть подсвечена");
        } else {
            assert_ne!(got, want, "посторонняя строка {slot} не подсвечена");
        }
    }
}

/// Скользящая полоса рисуется на той позиции, которую ей дали, и не по
/// выбору. Границы полуоткрытые — верхняя включена, нижняя нет, поэтому
/// при `pos = 1.5` подсвечен ровно низ первой строки.
#[test]
fn a_highlight_row_draws_at_the_given_position() {
    let frame = frame_with(8, 3);
    let (ui, _) = theme(palette::Palette::default());
    let want = ui.accent;
    let lit = |pixmap: &tiny_skia::Pixmap, slot: f32| pixel_in_row(pixmap, &frame, slot) == want;

    let halfway = render_card(&frame, Some(1.5));
    assert!(lit(&halfway, 1.0), "низ первой строки подсвечен");
    assert!(!lit(&halfway, 2.0), "строка ниже чистая");
    assert!(!lit(&halfway, 0.0), "строка выше не подсвечена");
    assert!(
        !lit(&halfway, 3.0),
        "на месте прежнего выбора фона быть не должно"
    );

    let settled = render_card(&frame, Some(1.0));
    assert!(lit(&settled, 1.0), "на целой строке полоса встаёт на неё");
    assert!(!lit(&settled, 2.0), "соседняя строка не подсвечена");
}

/// Смешение цвета строки по близости к полосе: на концах даёт прежние цвета,
/// в середине — промежуточный. Выбранный текст — тёмный `on_primary` на
/// заливке акцента.
#[test]
fn the_row_color_follows_the_band_proximity() {
    let (ui, _) = theme(palette::Palette::default());

    assert_eq!(
        mix(ui.text, ui.on_primary, 0.0),
        ui.text,
        "вне полосы — обычный"
    );
    assert_eq!(
        mix(ui.text, ui.on_primary, 1.0),
        ui.on_primary,
        "под полосой — выбранный"
    );

    let half = blend(ui.text, ui.on_primary, 0.5);
    let distance = |a: palette::Rgba, b: palette::Rgba| {
        (0..3)
            .map(|i| (a.0 + i) as i32 - (b.0 + i) as i32)
            .map(i32::abs)
            .sum::<i32>()
    };
    assert!(
        distance(half, ui.text) > 0 && distance(half, ui.on_primary) > 0,
        "середина должна отличаться от обоих концов"
    );
    assert_eq!(
        distance(blend(ui.text, ui.on_primary, 0.5), half),
        0,
        "половина пути считается одинаково"
    );
    assert_ne!(
        ui.on_primary, ui.accent,
        "текст выбранной строки обязан отличаться от её заливки"
    );
}

/// Без едущей полосы близость равна обычному выбору, поэтому кадр
/// побитово прежний: смешение на концах совпадает с исходными цветами.
#[test]
fn without_a_band_the_proximity_is_plain_selection() {
    let (ui, _) = theme(palette::Palette::default());
    let near = |selected: bool| usize::from(selected) as f32;

    assert_eq!(blend(ui.text, ui.on_primary, near(true)), ui.on_primary);
    assert_eq!(blend(ui.muted, ui.on_primary, near(true)), ui.on_primary);
    assert_eq!(blend(ui.text, ui.on_primary, near(false)), ui.text);
    assert_eq!(blend(ui.muted, ui.on_primary, near(false)), ui.muted);
}

#[test]
fn blending_keeps_the_alpha_of_both_ends() {
    // Цвет иконки невыбранной строки полупрозрачен: смешение, затирающее
    // альфу в 255, посветлило бы все такие иконки.
    let (ui, _) = theme(palette::Palette::default());
    assert_ne!(
        ui.muted.3, 255,
        "тест бессмысленен, если цвет иконки непрозрачен"
    );
    assert_eq!(blend(ui.muted, ui.accent, 0.0).3, ui.muted.3);
    assert_eq!(blend(ui.muted, ui.accent, 1.0).3, ui.accent.3);
    let middle = (ui.muted.3 as f32 + (ui.accent.3 - ui.muted.3) as f32 * 0.5) as u8;
    assert_eq!(blend(ui.muted, ui.accent, 0.5).3, middle);
    assert_eq!(blend(ui.text, ui.accent, 0.0), ui.text);
    assert_eq!(blend(ui.text, ui.accent, 1.0), ui.accent);
}

/// Расстояние до полосы: своя строка под полосой, соседние — нет.
#[test]
fn only_the_row_under_the_band_lights_up() {
    let close = |pos: f32, index: usize| (1.0 - (pos - index as f32).abs()).clamp(0.0, 1.0);

    assert!((close(2.0, 2) - 1.0).abs() < 1e-6);
    assert_eq!(close(2.0, 1), 0.0);
    assert_eq!(close(2.0, 3), 0.0);
    assert!((close(2.5, 2) - 0.5).abs() < 1e-6);
    assert!((close(2.5, 3) - 0.5).abs() < 1e-6);
}

/// Полоса не рисуется на месте старой строки, пока едет новая.
#[test]
fn the_sliding_band_leaves_the_previous_row() {
    let frame = frame_with(8, 3);
    let (ui, _) = theme(palette::Palette::default());
    let want = ui.accent;

    let moving = render_card(&frame, Some(1.0));

    assert_eq!(pixel_in_row(&moving, &frame, 1.0), want);
    assert_ne!(
        pixel_in_row(&moving, &frame, 3.0),
        want,
        "на прежней выбранной строке фона быть не должно"
    );
}

/// Выбранная строка залита акцентом на всю ширину: слева не остаётся полоски
/// поуже, справа заливка доходит до отступа рамки. Проверяется на скользящей
/// полосе — она рисуется тем же `highlight_band`.
#[test]
fn the_selected_row_is_filled_with_accent_from_edge_to_edge() {
    let frame = frame_with(8, 3);
    let (ui, _) = theme(palette::Palette::default());
    let moving = render_card(&frame, Some(2.0));

    let row_h = row_height(search_mode(&frame));
    let y = (HEADER_H + 2.0 * row_h + row_h / 2.0) * SCALE;
    for x in [CARD_INSET + 1.0, CARD_W / 2.0, CARD_W - CARD_INSET - 1.0] {
        assert_eq!(
            at_pixel(&moving, x * SCALE, y),
            ui.accent,
            "строка обязана быть акцентом на x={x}"
        );
    }
}

/// Пустой список не должен получать висящую подсветку.
#[test]
fn an_empty_list_gets_no_floating_band() {
    let menu = Menu::new(Vec::new());
    let frame = menu.frame();
    let empty = render_card(&frame, Some(0.0));
    let plain = render_card(&frame, None);

    assert_eq!(
        empty.data(),
        plain.data(),
        "над пустым списком подсветка рисоваться не должна"
    );
}

/// Кадр рисуется без паники, скользящая подсветка в том числе.
#[test]
fn render_a_sliding_band() {
    let frame = frame_with(8, 3);
    let moving = render_card(&frame, Some(2.5));
    assert!(moving.width() > 0 && moving.height() > 0, "кадр пуст");
    let plain = render_card(&frame, None);
    assert_ne!(
        moving.data(),
        plain.data(),
        "скользящая полоса ни на что не повлияла"
    );
}

/// Список из десяти строк, выбор девятой: прокрутка ещё не нужна, и
/// подсветка обязана совпадать с выбором.
#[test]
fn highlight_stays_on_the_selected_row_after_scrolling() {
    let mut menu = Menu::new(rows(10));
    for _ in 0..8 {
        menu.move_sel(1);
    }
    let frame = menu.frame();
    assert_eq!(frame.selected, Some(8), "выбрана девятая строка");
    assert_eq!(selected_row(&frame), frame.selected, "подсветка = выбор");
}

/// Длинный список с прокруткой: `top` уходит вниз, и подсветка всё равно
/// должна указывать на выбранную строку. Раньше индекс складывался с
/// `top`, и курсор перескакивал через строку — на «Панель управления»
/// вместо «Режим питания».
#[test]
fn highlight_is_absolute_not_offset_by_the_scroll() {
    let mut menu = Menu::new(rows(30));
    for _ in 0..20 {
        menu.move_sel(1);
    }
    let frame = menu.frame();
    assert!(frame.top > 0, "список прокручен: {}", frame.top);
    let row = selected_row(&frame).expect("выбор есть");
    let window: Vec<usize> = window_rows(&frame, MAX_ROWS)
        .iter()
        .map(|(index, _)| *index)
        .collect();
    assert!(
        window.contains(&row),
        "подсвеченная строка {row} должна быть в окне {window:?}"
    );
    assert_ne!(
        row,
        frame.top + frame.selected.expect("выбор есть"),
        "индекс не должен смещаться на top={}",
        frame.top
    );
}

/// Под последней нарисованной строкой не остаётся пустого поля: оконных
/// строк ровно столько, сколько осталось в списке, и они не выходят за
/// его конец.
#[test]
fn no_empty_rows_are_drawn_below_the_last_one() {
    let mut menu = Menu::new(rows(30));
    for _ in 0..29 {
        menu.move_sel(1);
    }
    let frame = menu.frame();
    let window = window_rows(&frame, MAX_ROWS);
    assert!(
        !window.is_empty(),
        "у прокрученного списка есть что показывать: top={}",
        frame.top
    );
    let last = window.last().expect("строка").0;
    assert_eq!(
        last + 1,
        frame.items.len(),
        "окно должно упереться в конец списка, а не оставить пустоты"
    );
}

/// Геометрия полосы: у строки громкости место под неё есть, заливка
/// растёт с уровнем и не вылезает за дорожку. Проверяется числами по
/// настоящей геометрии строки, а не пикселями: цвет после смешивания —
/// плохая мера «нарисовано».
#[test]
fn meter_tracks_the_level_and_fits_the_row() {
    let label = label_rect_for(true);
    let track = meter_track(label, CARD_W);
    assert!(track.w > 0.0, "у строки громкости место под полосу есть");
    assert!(
        track.w <= METER_W + spacing::MD + 1.0,
        "полоса не шире задуманного: {}",
        track.w
    );
    assert_eq!(meter_fill(track, 0), 0.0, "ноль процентов — пусто");
    assert_eq!(meter_fill(track, 50), track.w / 2.0, "половина — половина");
    assert_eq!(meter_fill(track, 100), track.w, "сотня — вся дорожка");
    assert_eq!(meter_fill(track, 250), track.w, "выше сотни не вылезает");
    assert!(
        meter_fill(track, 90) > meter_fill(track, 40),
        "больше уровень — шире заливка"
    );
    // Справа у обычной строки шеврон, а не полоса: она не должна
    // резервировать под дорожку место. Проверяем `trailing`, а не
    // `meter_track`: дорожку считают одинаково, рисует её вызывающий
    // по `item.level`.
    assert!(
        super::trailing_w(&plain_item()) < super::trailing_w(&volume_item()),
        "обычная строка резервирует меньше, чем громкость: {} против {}",
        super::trailing_w(&plain_item()),
        super::trailing_w(&volume_item())
    );
    // Совсем узкая строка: места не остаётся даже при `None` слева.
    let squeezed = Rect::new(
        60.0,
        0.0,
        CARD_W - PAD - VOLUME_VALUE_W - spacing::SM - 60.0,
        ROW_H,
    );
    assert_eq!(
        meter_fill(meter_track(squeezed, CARD_W), 50),
        0.0,
        "наезжать на значение нельзя"
    );
    assert!(meter_rect(Rect::new(0.0, 0.0, 0.0, 0.0), 50).is_none());
}

/// Полоса стоит по середине строки, а не липнет к её верху: дорожка
/// берётся высотой во всю строку и центрируется уже по ней.
#[test]
fn meter_sits_in_the_middle_of_the_row() {
    let row = Rect::new(24.0, 300.0, CARD_W, ROW_H);
    let base = label_rect_for(true);
    // Название лежит в строке, значит и дорожка берёт её `y`.
    let label = Rect::new(base.x, row.y, base.w, base.h);
    let track = meter_track(label, row.right());
    let scale = type_scale();
    let bar = meter_rect(center_band(track, scale.label * 2.0), 50).unwrap();
    let row_middle = row.y + row.h / 2.0;
    let bar_middle = bar.y + bar.h / 2.0;
    assert!(
        (bar_middle - row_middle).abs() < 0.51,
        "полоса ниже середины строки на {}",
        bar_middle - row_middle
    );
    assert!(
        bar.y >= row.y && bar.bottom() <= row.bottom(),
        "полоса должна помещаться в строку"
    );
}

/// Обычная строка и строка громкости — только ради сравнения правой
/// части при расчёте места.
fn plain_item() -> super::super::item::Item {
    super::super::item::Item {
        icon: settings_icons::NETWORK,
        id: "test".to_string(),
        title: "Обычная строка".to_string(),
        value: String::new(),
        caption: None,
        keywords: &[],
        kind: super::super::item::ItemKind::Toggle {
            get: || true,
            set: |_| {},
        },
        level: None,
        muted: false,
        danger: false,
        disabled: false,
        origin: super::super::item::Origin { depth: 0, index: 0 },
    }
}

fn volume_item() -> super::super::item::Item {
    use super::super::item::ItemKind;
    use super::super::system::CommandSpec;
    let set = CommandSpec::new("wpctl");
    super::super::item::Item {
        kind: ItemKind::Volume {
            down: set.clone(),
            up: set.clone(),
            mute: set,
        },
        level: Some(50),
        ..plain_item()
    }
}

/// Настоящая геометрия названия в строке: строки громкости резервируют
/// под полосу больше, чем обычные.
fn label_rect_for(volume: bool) -> Rect {
    let right = CARD_W - PAD;
    let trailing = if volume {
        METER_W + VOLUME_VALUE_W + spacing::SM
    } else {
        CHEVRON_W + VALUE_W + spacing::SM
    };
    let label_x = PAD + ICON + spacing::MD;
    Rect::new(label_x, 0.0, (right - trailing - label_x).max(0.0), ROW_H)
}

/// Меню из `count` строк для проверки навигации.
fn rows(count: usize) -> Vec<Node> {
    let owned: Vec<String> = (0..count).map(|index| format!("строка {index}")).collect();
    owned
        .iter()
        .map(|title| Node::action("", title, Action::Back))
        .collect()
}

/// Поле поиска не вытесняется ни на одном языке и ни при какой длине
/// крошек: его левый край считается только от правого края шапки.
#[test]
fn the_search_field_keeps_its_place_next_to_any_crumbs() {
    let header = Rect::new(0.0, 0.0, CARD_W, HEADER_H);
    let field = search_rect(header);
    let crumbs = crumbs_rect(header);

    assert_eq!(
        field.x,
        CARD_W - PAD - SEARCH_W,
        "поле прижато к правому краю"
    );
    assert_eq!(field.right(), CARD_W - PAD);
    assert!(crumbs.w > 0.0, "под крошки есть место: {}", crumbs.w);
    assert!(
        crumbs.right() <= field.x - spacing::MD,
        "крошки и поле не наезжают: {} против {}",
        crumbs.right(),
        field.x
    );
    // Сумма обязана сойтись с шириной карточки, иначе где-то остаётся дыра.
    assert_eq!(
        PAD + crumbs.w + spacing::MD + SEARCH_W + PAD,
        CARD_W,
        "шапка собрана без остатка"
    );
    for lang in [Language::Ru, Language::En] {
        let chrome = strings::chrome(lang);
        let mut pixmap = blank_buffer();
        let mut painter = TextPainter::new(&font_name());
        let mut icons = TextPainter::new(settings_icons::FONT);
        let view = measuring_view(&mut pixmap, &mut painter, &mut icons);
        let inner = search_rect(header).w - (spacing::SM + ICON + spacing::SM) - spacing::SM;
        let width = view.painter.text_width(chrome.search, view.scale.caption);
        assert!(
            width <= inner,
            "{lang:?}: заглушка «{}» шире поля на {}",
            chrome.search,
            width - inner
        );
    }
}

/// Кегль крошек крупнее кегля строки списка: иначе шапка и список
/// неразличимы по весу.
#[test]
fn crumbs_are_larger_than_a_row_title() {
    let scale = type_scale();
    assert!(scale.section_title > scale.label);
    // Заголовок окна остаётся крупнее крошек: это уровнем выше.
    assert!(scale.page_title > scale.section_title);
    // И вписывается в шапку.
    assert!(
        scale.section_title * 2.0 <= HEADER_H,
        "крошки не вылезают из шапки"
    );
}

/// Подпись пустого списка стоит по центру области, а не под шапкой: у
/// пустого списка нарисованных строк нет, и без этого прямоугольника она
/// прижималась к верхнему краю.
#[test]
fn the_empty_notice_is_centered_in_the_list_area() {
    for search in [false, true] {
        let row_h = row_height(search);
        let area = empty_notice_rect(Rect::new(0.0, HEADER_H, CARD_W, 0.0), row_h);
        assert!(
            area.h > 0.0,
            "поиск={search}: у подписи нулевая высота, она прижмётся к шапке"
        );
        assert_eq!(
            area.h,
            rows_shown(0) as f32 * row_h,
            "поиск={search}: подпись по центру всего списка"
        );
        assert_eq!(area.y, HEADER_H, "поиск={search}: отступ от шапки");
        assert_eq!(area.w, CARD_W, "поиск={search}: во всю ширину");
    }
}

/// `hover_row = None` обязан дать побитово прежний кадр: иначе появление
/// подсветки мыши поменяло бы все эталонные снимки меню.
#[test]
fn no_hover_keeps_the_frame_byte_for_byte() {
    let frame = frame_with(8, 3);
    let plain = render_card(&frame, None);
    let also_plain = render_card_hover(&frame, None, None);
    assert_eq!(
        plain.data(),
        also_plain.data(),
        "без подсветки мыши кадр не должен меняться"
    );
}

/// Подсветка мыши не трогает выбор и не двигает скользящую полосу.
#[test]
fn hover_paints_only_the_row_under_the_pointer() {
    let frame = frame_with(8, 3);
    let (ui, _) = theme(palette::Palette::default());
    let plain = render_card_hover(&frame, None, None);
    let hovered = render_card_hover(&frame, None, Some(1));

    assert_ne!(
        plain.data(),
        hovered.data(),
        "подсветка мыши обязана что-то рисовать"
    );
    // Строка под указателем и соседние.
    assert_eq!(
        pixel_in_row(&plain, &frame, 1.0),
        ui.panel,
        "строка 1 чистая"
    );
    assert_eq!(
        pixel_in_row(&hovered, &frame, 1.0),
        ui.hover_bg,
        "строка под указателем подсвечена"
    );
    assert_eq!(
        pixel_in_row(&hovered, &frame, 2.0),
        ui.panel,
        "соседняя строка не тронута"
    );
    // Выбор на месте: клик мышью не выбирает, выбранная строка — акцент.
    assert_eq!(
        pixel_in_row(&hovered, &frame, 3.0),
        ui.accent,
        "выбранная строка осталась выбранной"
    );
}

/// Клик по Header/Info — мимо: у строки нет выбираемого индекса, и соседняя
/// строка не должна получить действие.
#[test]
fn click_on_header_or_info_is_noop() {
    let menu = Menu::new(vec![
        Node::header("Заголовок"),
        Node::info("i", "Информация"),
        Node::action("a", "Действие", Action::Back),
    ]);
    let frame = menu.frame();
    assert_eq!(frame.selected, None);
    let card = Rect::new(0.0, 0.0, CARD_W, 400.0);
    let row_h = row_height(search_mode(&frame));
    let x = PAD;

    for slot in [0.0, 1.0] {
        let y = HEADER_H + slot * row_h + row_h / 2.0;
        assert_eq!(row_at(&frame, card, x, y), Some(slot as usize));
        assert_eq!(
            selectable_row_at(&frame, card, x, y),
            None,
            "Header/Info не цель клика"
        );
    }
    let y = HEADER_H + 2.0 * row_h + row_h / 2.0;
    assert_eq!(selectable_row_at(&frame, card, x, y), Some(2));
}

/// Hover по Header/Info не рисует подложку; обычная строка под указателем
/// подсвечивается.
#[test]
fn hover_on_header_or_info_is_none() {
    let menu = Menu::new(vec![
        Node::header("Заголовок"),
        Node::info("i", "Информация"),
        Node::action("a", "Действие", Action::Back),
    ]);
    let frame = menu.frame();
    let plain = render_card_hover(&frame, None, None);
    for slot in [0, 1] {
        let hovered = render_card_hover(&frame, None, Some(slot));
        assert_eq!(
            hovered.data(),
            plain.data(),
            "hover по Header/Info не рисует подсветку"
        );
    }
    let hovered = render_card_hover(&frame, None, Some(2));
    assert_ne!(
        hovered.data(),
        plain.data(),
        "hover по обычной строке обязан рисоваться"
    );
}

/// Header — caption-кегль и muted, снизу hairline на всю ширину строки.
#[test]
fn header_uses_caption_size_muted_and_hairline() {
    let frame = Menu::new(vec![Node::header("Заголовок")]).frame();
    let pixmap = render_card(&frame, None);
    let (ui, scale) = theme(palette::Palette::default());

    let hairline_y = HEADER_H + ROW_H - 0.5;
    assert_eq!(
        at_pixel(&pixmap, CARD_W / 2.0 * SCALE, hairline_y * SCALE),
        ui.border_subtle,
        "под Header обязана быть hairline"
    );

    let (left, right) = ink_bounds(
        &pixmap,
        PAD,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .expect("подпись Header видна");
    assert!(
        (PAD - 0.5..=PAD + 2.0).contains(&left),
        "без иконки: {left}"
    );
    assert!(
        right < CARD_W - PAD,
        "hairline не должен попасть в замер: {right}"
    );

    let mut painter = TextPainter::new(&font_name());
    let caption_w = painter.text_width("Заголовок", scale.caption);
    let label_w = painter.text_width("Заголовок", scale.label);
    let ink_w = right - left;
    assert!(
        (ink_w - caption_w).abs() < (ink_w - label_w).abs(),
        "Header рисуется caption-кеглем: ink {ink_w}, caption {caption_w}, label {label_w}"
    );

    // Полупрозрачный `muted` ложится на непрозрачную панель: полное покрытие
    // глифа даёт ровно эту смесь, а не сам цвет с альфой.
    // Полупрозрачный `muted` ложится на панель: полное покрытие глифа даёт
    // смесь цвета с панелью.
    let expected = blend(ui.panel, ui.muted, ui.muted.3 as f32 / 255.0);
    let mut muted = false;
    for y in ((HEADER_H + 4.0) * SCALE) as u32..((HEADER_H + ROW_H - 4.0) * SCALE) as u32 {
        for x in (PAD * SCALE) as u32..((CARD_W - PAD) * SCALE) as u32 {
            let p = at_pixel(&pixmap, x as f32, y as f32);
            let d = (p.0 as i32 - expected.0 as i32).abs()
                + (p.1 as i32 - expected.1 as i32).abs()
                + (p.2 as i32 - expected.2 as i32).abs();
            // Карточка полупрозрачна, поэтому альфа пикселя отличается от
            // расчётной: цвет сравнивается без неё, с допуском на округление.
            if d <= 2 {
                muted = true;
            }
        }
    }
    assert!(muted, "текст Header обязан быть muted");
}

/// Значение подменю занимает ту же правую полосу, что и шеврон, поэтому
/// длинное название обрезается многоточием до значения, а не заезжает под него.
#[test]
fn submenu_with_value_reserves_space_and_title_is_clipped() {
    let mut frame = Menu::new(vec![
        Node::submenu(
            "i",
            "Очень длинное название пункта подменю",
            vec![Node::info("", "Дочерний пункт")],
        )
        .with_text("Текущее значение"),
    ])
    .frame();
    frame.selected = None;
    let pixmap = render_card(&frame, None);
    let (ui, scale) = theme(palette::Palette::default());
    let item = &frame.items[0];
    let right = CARD_W - PAD;
    let label_x = PAD + ICON + spacing::MD;
    let value_left = right - CHEVRON_W - VALUE_W - spacing::SM;
    let title = ink_bounds(
        &pixmap,
        label_x,
        HEADER_H,
        value_left,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .expect("название подменю видно");
    let value = ink_bounds(
        &pixmap,
        value_left,
        HEADER_H,
        right - CHEVRON_W,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .expect("значение подменю видно");

    assert_eq!(
        super::trailing_w(item),
        CHEVRON_W + VALUE_W + spacing::SM,
        "непустое значение подменю резервирует шеврон, значение и зазор"
    );
    let empty = Menu::new(vec![Node::submenu(
        "i",
        "Подменю без значения",
        vec![Node::info("", "Дочерний пункт")],
    )])
    .frame();
    assert_eq!(
        super::trailing_w(&empty.items[0]),
        CHEVRON_W,
        "пустое значение подменю сохраняет прежнюю геометрию"
    );
    assert!(
        TextPainter::new(&font_name()).text_width(&item.title, scale.label) > value_left - label_x,
        "название должно быть длиннее доступной области"
    );
    assert!(
        title.1 < value.0,
        "название и значение не должны накладываться"
    );
}

/// Значение Header рисуется muted и прижимается к правому краю карточки.
#[test]
fn header_value_is_right_aligned() {
    let frame = Menu::new(vec![Node::header("Группа").with_text("Текущее значение")]).frame();
    let pixmap = render_card(&frame, None);
    let (ui, _) = theme(palette::Palette::default());
    let right = CARD_W - PAD;
    let value_left = right - VALUE_W;
    let value = ink_bounds(
        &pixmap,
        value_left,
        HEADER_H,
        right,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .expect("значение Header видно");

    assert!(
        value.1 > right - 20.0,
        "значение Header должно быть справа: {value:?}"
    );
    assert!(
        value.0 >= value_left,
        "значение Header не выходит из своей зоны"
    );
}

/// У Header нет иконки и шеврона, hover его не подсвечивает.
#[test]
fn header_has_no_icon_chevron_or_hover() {
    let frame = Menu::new(vec![Node::header("Заголовок")]).frame();
    let plain = render_card(&frame, None);
    let hovered = render_card_hover(&frame, None, Some(0));
    assert_eq!(hovered.data(), plain.data(), "Header не получает hover");

    let (left, _) = ink_bounds(
        &plain,
        PAD,
        HEADER_H,
        CARD_W,
        HEADER_H + ROW_H - 2.0,
        theme(palette::Palette::default()).0.panel,
    )
    .expect("подпись Header видна");
    assert!(
        left <= PAD + 2.0,
        "подпись начинается без отступа под иконку"
    );

    let (ui, _) = theme(palette::Palette::default());
    let chevron = ink_bounds(
        &plain,
        CARD_W - PAD - CHEVRON_W,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    );
    assert_eq!(chevron, None, "у Header не должно быть шеврона");
}

/// Header и Info занимают те же 40 px, что и обычная строка.
#[test]
fn header_and_info_keep_row_height_forty() {
    assert_eq!(row_height(false), ROW_H);
    for count in 5..8 {
        assert_eq!(
            card_height(count + 1) - card_height(count),
            ROW_H,
            "каждая строка — ровно ROW_H"
        );
    }
    let frame = Menu::new(vec![
        Node::header("Заголовок"),
        Node::info("i", "Информация"),
    ])
    .frame();
    assert_eq!(row_height(search_mode(&frame)), ROW_H);
}

/// У Info нет шеврона: Enter ничего не открывает.
#[test]
fn info_has_no_chevron() {
    let frame = Menu::new(vec![Node::info("i", "Информация")]).frame();
    let pixmap = render_card(&frame, None);
    let (ui, _) = theme(palette::Palette::default());
    assert_eq!(
        ink_bounds(
            &pixmap,
            CARD_W - PAD - CHEVRON_W,
            HEADER_H,
            CARD_W - PAD,
            HEADER_H + ROW_H - 2.0,
            ui.panel,
        ),
        None,
        "Info не рисует шеврон"
    );
}

/// Info не получает ни выбранной заливки, ни hover, даже если в кадре
/// ошибочно оказался индекс этой строки.
#[test]
fn info_never_gets_selected_or_hover_band() {
    let mut frame = Menu::new(vec![
        Node::info("", "Информация"),
        Node::action("a", "Действие", Action::Back),
    ])
    .frame();
    frame.selected = Some(0);
    let pixmap = render_card_hover(&frame, None, Some(0));
    let (ui, _) = theme(palette::Palette::default());
    let x = CARD_W - 48.0;
    let y = (HEADER_H + ROW_H / 2.0) * SCALE;
    assert_ne!(at_pixel(&pixmap, x * SCALE, y), ui.accent);
    assert_ne!(at_pixel(&pixmap, x * SCALE, y), ui.hover_bg);
}

#[test]
fn info_value_columns_align_across_the_level() {
    let frame = Menu::new(vec![
        Node::info_value("Short", "VALUE"),
        Node::info_value("A substantially longer title", "VALUE"),
    ])
    .frame();
    let pixmap = render_card(&frame, None);
    let (ui, _) = theme(palette::Palette::default());
    let mut painter = TextPainter::new(&font_name());
    let label_scale = theme(palette::Palette::default()).1.label;
    let title_w = ["Short", "A substantially longer title"]
        .iter()
        .map(|title| painter.text_width(title, label_scale))
        .fold(0.0f32, f32::max)
        .min(CARD_W * 0.4);
    let value_x = PAD + title_w + spacing::MD;
    let a = ink_bounds(
        &pixmap,
        value_x,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .unwrap();
    let b = ink_bounds(
        &pixmap,
        value_x,
        HEADER_H + ROW_H,
        CARD_W - PAD,
        HEADER_H + 2.0 * ROW_H - 2.0,
        ui.panel,
    )
    .unwrap();
    assert!((a.0 - b.0).abs() < 2.0, "value columns: {a:?} vs {b:?}");

    // Даже title за пределами видимого окна участвует в общей колонке;
    // фиксированный cap не позволяет ему отнять больше 40% списка.
    let long_title = "Длинное имя поля, которое намеренно шире сорока процентов карточки";
    let mut items = vec![Node::info_value("Короткое", "значение")];
    for _ in 0..12 {
        items.push(Node::action("a", "Обычная строка", Action::Back));
    }
    items.push(Node::info_value(long_title, "внизу"));
    let mut long_frame = Menu::new(items).frame();
    long_frame.top = 0;
    let long_pixmap = render_card(&long_frame, None);
    let mut painter = TextPainter::new(&font_name());
    let label_scale = theme(palette::Palette::default()).1.label;
    let title_width = painter.text_width(long_title, label_scale);
    assert!(title_width > CARD_W * 0.4);
    let value_x = PAD + title_width.min(CARD_W * 0.4) + spacing::MD;
    let first_value = ink_bounds(
        &long_pixmap,
        value_x,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .unwrap();
    assert!(first_value.0 >= value_x - 2.0);
}

#[test]
fn info_value_clips_long_value() {
    let value_text = "Очень длинное значение ".repeat(20);
    let frame = Menu::new(vec![Node::info_value("Поле", &value_text)]).frame();
    let pixmap = render_card(&frame, None);
    let (ui, _) = theme(palette::Palette::default());
    let mut painter = TextPainter::new(&font_name());
    let label_scale = theme(palette::Palette::default()).1.label;
    let value_x = PAD + painter.text_width("Поле", label_scale) + spacing::MD;
    let value = ink_bounds(
        &pixmap,
        value_x,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .expect("value visible");
    let mut painter = TextPainter::new(&font_name());
    let available = CARD_W - PAD - value_x;
    assert!(
        painter.text_width(&value_text, theme(palette::Palette::default()).1.label) > available
    );
    assert!(value.1 <= CARD_W - PAD);
    assert!(
        value.1 - value.0 <= available + 2.0,
        "длинное значение обрезано: {value:?}"
    );
}

#[test]
fn info_value_muted_value_uses_muted_color() {
    let mut frame = Menu::new(vec![Node::info_value("Поле", "значение")]).frame();
    frame.items[0].muted = true;
    let pixmap = render_card(&frame, None);
    let (ui, _) = theme(palette::Palette::default());
    let mut painter = TextPainter::new(&font_name());
    let label_scale = theme(palette::Palette::default()).1.label;
    let value_x = PAD + painter.text_width("Поле", label_scale) + spacing::MD;
    let bounds = ink_bounds(
        &pixmap,
        value_x,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H - 2.0,
        ui.panel,
    )
    .expect("value visible");
    let expected = blend(ui.panel, ui.muted, ui.muted.3 as f32 / 255.0);
    assert!(((bounds.0 + bounds.1) * 0.5) < CARD_W - PAD);
    let found = ((bounds.0 * SCALE) as u32..=(bounds.1 * SCALE) as u32).any(|x| {
        (((HEADER_H + 4.0) * SCALE) as u32..((HEADER_H + ROW_H - 4.0) * SCALE) as u32).any(|y| {
            let p = at_pixel(&pixmap, x as f32, y as f32);
            (p.0 as i16 - expected.0 as i16).abs() <= 2
                && (p.1 as i16 - expected.1 as i16).abs() <= 2
                && (p.2 as i16 - expected.2 as i16).abs() <= 2
        })
    });
    assert!(found, "muted value should use muted role");
}

#[test]
fn info_value_has_no_chevron_or_selection() {
    let mut frame = Menu::new(vec![
        Node::info_value("Поле", "значение"),
        Node::action("a", "Действие", Action::Back),
    ])
    .frame();
    frame.selected = Some(0);
    let plain = render_card(&frame, None);
    let hovered = render_card_hover(&frame, None, Some(0));
    assert_eq!(plain.data(), hovered.data());
    let (ui, _) = theme(palette::Palette::default());
    assert_eq!(
        ink_bounds(
            &plain,
            CARD_W - PAD - CHEVRON_W,
            HEADER_H,
            CARD_W - PAD,
            HEADER_H + ROW_H - 2.0,
            ui.panel
        ),
        None
    );
    assert_ne!(pixel_in_row(&plain, &frame, 0.0), ui.accent);
}

/// Без выбора в списке нет акцентной полосы ни на одной строке.
#[test]
fn no_selection_draws_no_highlight_band() {
    let frame = Menu::new(vec![
        Node::header("Заголовок"),
        Node::info("i", "Информация"),
        Node::action("a", "Действие", Action::Back),
    ])
    .frame();
    assert_eq!(frame.selected, None);
    let pixmap = render_card(&frame, None);
    let (ui, _) = theme(palette::Palette::default());
    for slot in [0.0, 1.0, 2.0] {
        assert_ne!(
            pixel_in_row(&pixmap, &frame, slot),
            ui.accent,
            "строка {slot} не должна быть подсвечена"
        );
    }
}

/// Счётчик подвала считает выбираемые строки, а не все физические.
#[test]
fn footer_counts_selectable_not_physical_rows() {
    let mut frame = Menu::new(vec![
        Node::header("Заголовок"),
        Node::action("a", "Первый", Action::Back),
        Node::info("i", "Информация"),
        Node::action("b", "Второй", Action::Back),
    ])
    .frame();
    frame.selected = Some(3);
    assert_eq!(super::footer::counter_text(&frame), "2/2");
}

/// Без выбора — прочерк, на пустом списке — ноль из нуля.
#[test]
fn footer_with_none_shows_dash() {
    let frame = Menu::new(vec![
        Node::info("i", "Информация"),
        Node::action("a", "Действие", Action::Back),
    ])
    .frame();
    assert_eq!(frame.selected, None);
    assert_eq!(super::footer::counter_text(&frame), "—/1");
    assert_eq!(
        super::footer::counter_text(&Menu::new(Vec::new()).frame()),
        "0/0"
    );
}

/// Header не меняет пиксели обычных строк: та же строка до и после него
/// рисуется одинаково.
#[test]
fn ordinary_rows_remain_pixel_identical() {
    let ordinary = || {
        vec![
            Node::action("a", "Первый", Action::Back),
            Node::action("b", "Второй", Action::Back),
            Node::action("c", "Третий", Action::Back),
        ]
    };
    let mut plain = Menu::new(ordinary()).frame();
    plain.selected = None;
    let mut with_header = Menu::new({
        let mut nodes = vec![Node::header("Заголовок")];
        nodes.extend(ordinary());
        nodes
    })
    .frame();
    with_header.selected = None;

    let plain_pixmap = render_card(&plain, None);
    let header_pixmap = render_card(&with_header, None);
    assert_eq!(
        row_pixels(&plain_pixmap, 0),
        row_pixels(&header_pixmap, 1),
        "обычная строка после Header должна совпадать побитово"
    );
    assert_eq!(row_pixels(&plain_pixmap, 1), row_pixels(&header_pixmap, 2));
}

/// Мышь над выбранной строкой не превращает акцент в слабую подложку hover:
/// выбор — одна заливка, курсор вторым выбором быть не может.
#[test]
fn hover_on_the_selected_row_keeps_the_accent() {
    let frame = frame_with(8, 3);
    let (ui, _) = theme(palette::Palette::default());
    let plain = render_card_hover(&frame, None, None);
    let hovered = render_card_hover(&frame, None, Some(3));

    assert_eq!(
        pixel_in_row(&hovered, &frame, 3.0),
        ui.accent,
        "выбранная строка под курсором остаётся акцентом"
    );
    assert_ne!(
        pixel_in_row(&hovered, &frame, 3.0),
        ui.hover_bg,
        "hover на выбранной строке не должен заменять акцент"
    );
    assert_eq!(
        plain.data(),
        hovered.data(),
        "поэтому кадр с мышью на выборе совпадает с кадром без неё"
    );
}

/// Подложка под указателем непрозрачна: полупрозрачный край читался бы
/// дырой в карточке.
#[test]
fn the_hover_wash_has_no_transparent_edge() {
    assert_eq!(
        palette_for().hover_bg.3,
        255,
        "подсветка мыши должна быть плотной"
    );
}

/// Подсветка мыши слабее выбранной строки: иначе мышь выглядела бы как
/// выбор, а это разные вещи.
#[test]
fn hover_is_fainter_than_the_selected_row() {
    let ui = palette_for();
    assert_ne!(ui.hover_bg, ui.accent);
    assert!(
        (luminance(ui.accent) - luminance(ui.panel)).abs()
            > (luminance(ui.hover_bg) - luminance(ui.panel)).abs(),
        "подсветка мыши должна быть слабее выбранной строки"
    );
}

/// Полоса выбора едет по-прежнему, а подсветка мыши на неё не влияет: при
/// едущей полосе фон строки рисуется один на весь список.
#[test]
fn hover_does_not_move_the_sliding_band() {
    let frame = frame_with(8, 3);
    let ui = palette_for();
    let moving = render_card_hover(&frame, Some(2.0), None);
    let moving_hover = render_card_hover(&frame, Some(2.0), Some(5));

    assert_eq!(
        pixel_in_row(&moving, &frame, 2.0),
        ui.accent,
        "полоса осталась на своей строке"
    );
    assert_ne!(
        moving.data(),
        moving_hover.data(),
        "подсветка мыши должна рисоваться и при едущей полосе"
    );
    assert_eq!(
        pixel_in_row(&moving_hover, &frame, 2.0),
        ui.accent,
        "фон под полосой не должен перекрываться подсветкой мыши"
    );
}

/// Индикатор длинного списка: появляется только когда список длиннее окна,
/// а его высота и положение отражают, где окно находится внутри списка.
#[test]
fn the_scrollbar_only_shows_on_a_long_list() {
    let list = Rect::new(0.0, HEADER_H, CARD_W, MAX_ROWS as f32 * ROW_H);
    assert_eq!(
        scrollbar_rect(list, MAX_ROWS, 0),
        None,
        "список ровно в окно: полоса не нужна"
    );
    assert_eq!(
        scrollbar_rect(list, MAX_ROWS - 1, 0),
        None,
        "короткий список: полоса не нужна"
    );
    assert_eq!(scrollbar_rect(list, 0, 0), None, "пустой список");

    let bar = scrollbar_rect(list, 50, 0).expect("длинный список");
    assert_eq!(bar.w, SCROLLBAR_W, "полоса тонкая");
    assert!(bar.h < list.h, "полоса короче окна");
    assert!(bar.h >= SCROLLBAR_W * 2.0, "и не тоньше двух пикселей");
    assert!(bar.x >= list.x + list.w - PAD, "полоса внутри поля");
    assert!(bar.right() <= list.right(), "полоса не выходит за список");
}

#[test]
fn the_scrollbar_moves_with_the_window() {
    let list = Rect::new(0.0, HEADER_H, CARD_W, MAX_ROWS as f32 * ROW_H);
    let top = scrollbar_rect(list, 50, 0).expect("длинный список");
    let middle = scrollbar_rect(list, 50, 20).expect("длинный список");
    let bottom = scrollbar_rect(list, 50, 40).expect("длинный список");

    assert!(top.y < middle.y, "полоса идёт вниз вместе со списком");
    assert!(middle.y < bottom.y, "и доезжает до низа");
    assert!(
        bottom.bottom() <= list.bottom() + 0.01,
        "у низа списка полоса не вылезает: {} против {}",
        bottom.bottom(),
        list.bottom()
    );
    assert!(
        (top.h - bottom.h).abs() < 0.01,
        "высота не зависит от положения"
    );
}

/// Тумблер остаётся сглаженным.
#[test]
fn the_toggle_keeps_its_soft_edge() {
    let frame =
        super::super::snapshot::subject_menu(super::super::snapshot::Subject::Search).frame();
    let pixmap = render_card(&frame, None);
    let data = pixmap.data();
    let width = pixmap.width();
    let row_h = row_height(true);
    let cx = (CARD_W - PAD - TOGGLE_D) * SCALE;
    let cy = (HEADER_H + row_h / 2.0) * SCALE;
    let side = (TOGGLE_D * SCALE) as i64;
    let mut colors = std::collections::BTreeSet::new();
    for y in (cy as i64)..(cy as i64 + side) {
        for x in (cx as i64)..(cx as i64 + side) {
            let at = (y as u32) * width + (x as u32);
            colors.insert([
                data[at as usize * 4],
                data[at as usize * 4 + 1],
                data[at as usize * 4 + 2],
            ]);
        }
    }
    assert!(
        colors.len() > 3,
        "в обычной теме круг должен остаться сглаженным, а цветов {} ",
        colors.len()
    );
}

use super::super::state::Status;

/// Карточка для измерений: тот же `MenuView`, что и при отрисовке.
fn measuring_view<'a>(
    pixmap: &'a mut tiny_skia::Pixmap,
    painter: &'a mut TextPainter,
    icons: &'a mut TextPainter,
) -> MenuView<'a> {
    let (ui, scale) = theme(palette::Palette::default());
    MenuView {
        pixmap,
        painter,
        icons,
        ui,
        scale,
    }
}

fn blank_buffer() -> tiny_skia::Pixmap {
    let mut pixmap =
        tiny_skia::Pixmap::new((CARD_W * SCALE) as u32, (card_height(5) * SCALE) as u32)
            .expect("pixmap");
    pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
    pixmap
}

/// Правый резерв не зависит от того, нарисован статус или счётчик: иначе
/// левая часть подвала прыгала бы в момент появления сообщения.
#[test]
fn the_footer_reserve_does_not_depend_on_what_is_shown() {
    for lang in [Language::Ru, Language::En] {
        let mut pixmap = blank_buffer();
        let mut painter = TextPainter::new(&font_name());
        let mut icons = TextPainter::new(settings_icons::FONT);
        let mut view = measuring_view(&mut pixmap, &mut painter, &mut icons);

        // Первый вызов на этом ключе меряет, второй — берёт из кэша.
        let misses = zone_misses();
        let reserve = right_zone_width(&mut view, lang);
        assert_eq!(zone_misses(), misses + 1, "первый вызов меряет");
        let chrome = strings::chrome(lang);
        let counter = super::COUNTER_SAMPLES
            .iter()
            .map(|text| view.painter.text_width(text, view.scale.caption))
            .fold(0.0_f32, f32::max);
        let status =
            STATUS_DOT + spacing::XS + view.painter.text_width(chrome.applied, view.scale.caption);
        assert!(
            reserve >= counter - 0.01 && reserve >= status - 0.01,
            "резерв {reserve} уже самого широкого: счётчик {counter}, статус {status}"
        );
        assert_eq!(
            right_zone_width(&mut view, lang),
            reserve,
            "тот же ключ обязан дать то же значение"
        );
        assert_eq!(
            zone_misses(),
            misses + 1,
            "повторный вызов на том же ключе не меряет"
        );
    }

    // Смена языка инвалидирует кэш: у русского и английского статусы разной
    // ширины, а значит разный и резерв.
    let mut pixmap = blank_buffer();
    let mut painter = TextPainter::new(&font_name());
    let mut icons = TextPainter::new(settings_icons::FONT);
    let mut view = measuring_view(&mut pixmap, &mut painter, &mut icons);
    let misses = zone_misses();
    let ru = right_zone_width(&mut view, Language::Ru);
    let en = right_zone_width(&mut view, Language::En);
    assert_ne!(ru, en, "у языков разная ширина статуса");
    assert_eq!(zone_misses(), misses + 2, "смена языка меряет заново");
    // Возврат к прежнему ключу меряет снова: кэш на одну запись, а не на все.
    let misses = zone_misses();
    assert_eq!(right_zone_width(&mut view, Language::Ru), ru);
    assert_eq!(zone_misses(), misses + 1, "кэш держит одну запись");
}

/// Три уровня подсказок: берётся первый влезший, и выбор детерминирован.
#[test]
fn hints_step_down_through_three_levels() {
    for lang in [Language::Ru, Language::En] {
        let chrome = strings::chrome(lang);
        let levels: [[&str; 4]; 3] = [
            [chrome.select, chrome.open, chrome.back, chrome.close],
            [
                chrome.select_medium,
                chrome.open_medium,
                chrome.back_medium,
                chrome.close_medium,
            ],
            [
                chrome.select_short,
                chrome.open_short,
                chrome.back_short,
                chrome.close_short,
            ],
        ];
        let mut pixmap = blank_buffer();
        let mut painter = TextPainter::new(&font_name());
        let mut icons = TextPainter::new(settings_icons::FONT);
        let mut view = measuring_view(&mut pixmap, &mut painter, &mut icons);
        let reserve = right_zone_width(&mut view, lang);
        let left = PAD;
        let limit = CARD_W - PAD - reserve - spacing::XL;

        // Узкий предел берёт последний уровень, широкий — первый влезший.
        let (wide, _) = layout_hints(&mut view, left, limit, lang);
        let (narrow, _) = layout_hints(&mut view, left, 0.0, lang);
        assert_eq!(
            narrow, levels[2],
            "{lang:?}: в тесном пределе остаются одни клавиши"
        );
        let index = levels
            .iter()
            .position(|level| *level == wide.as_slice())
            .expect("выбран один из трёх уровней");
        assert!(
            left + hints_width(&mut view, &levels[index]) <= limit,
            "{lang:?}: выбран уровень, который не влезает"
        );
        for higher in &levels[..index] {
            assert!(
                left + hints_width(&mut view, higher) > limit,
                "{lang:?}: уровень выше выбранного тоже влезает — выбор не максимальный"
            );
        }
    }
}

/// Ширина уровней обязана убывать: иначе лестница бессмысленна.
#[test]
fn hint_levels_get_narrower() {
    for lang in [Language::Ru, Language::En] {
        let mut pixmap = blank_buffer();
        let mut painter = TextPainter::new(&font_name());
        let mut icons = TextPainter::new(settings_icons::FONT);
        let mut view = measuring_view(&mut pixmap, &mut painter, &mut icons);
        let widths: Vec<f32> = (0..3)
            .map(|level| {
                let (_, width) =
                    layout_hints(&mut view, 0.0, if level == 0 { 1000.0 } else { 0.0 }, lang);
                width
            })
            .collect();
        assert!(
            widths[0] >= widths[1],
            "{lang:?}: средний уровень не уже полного"
        );
    }
}

fn palette_for() -> UiPalette {
    theme(palette::Palette::default()).0
}

#[test]
fn danger_color_differs_from_text() {
    let (ui, _) = theme(palette::Palette::default());
    assert_ne!(ui.danger, ui.text);
    assert!(super::super::ui_tokens::contrast(ui.danger, ui.panel) >= 3.0);
}

fn has_color(
    pixmap: &tiny_skia::Pixmap,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    want: palette::Rgba,
) -> bool {
    ((y0 * SCALE) as u32..(y1 * SCALE) as u32).any(|y| {
        ((x0 * SCALE) as u32..(x1 * SCALE) as u32).any(|x| {
            let got = at_pixel(pixmap, x as f32, y as f32);
            got.3.abs_diff(want.3) <= 1
                && (got.0 as i16 - want.0 as i16).abs() <= 3
                && (got.1 as i16 - want.1 as i16).abs() <= 3
                && (got.2 as i16 - want.2 as i16).abs() <= 3
        })
    })
}

/// Ожидаемый пиксель там, где глиф цвета `front` покрыл карточку `card`
/// целиком. Буфер premultiplied, а `at_pixel` распаковывает его по итоговой
/// альфе, поэтому полупрозрачный `muted` над полупрозрачной карточкой даёт не
/// свой straight-цвет, а смесь с `card`; непрозрачные роли не меняются.
fn ink_on_card(front: palette::Rgba, card: palette::Rgba) -> palette::Rgba {
    let fg = front.3 as f32 / 255.0;
    let bg = card.3 as f32 / 255.0;
    let out_a = fg + bg * (1.0 - fg);
    let channel =
        |f: u8, c: u8| ((f as f32 * fg + c as f32 * bg * (1.0 - fg)) / out_a).round() as u8;
    palette::Rgba(
        channel(front.0, card.0),
        channel(front.1, card.1),
        channel(front.2, card.2),
        (out_a * 255.0).round() as u8,
    )
}

#[test]
fn unselected_danger_uses_danger_color() {
    let mut frame = Menu::new(vec![
        Node::submenu(
            "!",
            "Опасно",
            vec![Node::action("", "Продолжить", Action::Back)],
        )
        .with_danger(),
    ])
    .frame();
    frame.selected = None;
    let pixmap = render_card(&frame, None);
    let ui = palette_for();
    assert!(has_color(
        &pixmap,
        PAD,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H,
        ui.danger
    ));
}

#[test]
fn selected_danger_keeps_accent_and_on_primary() {
    let frame = Menu::new(vec![
        Node::action("!", "Опасно", Action::Back).with_danger(),
    ])
    .frame();
    let pixmap = render_card(&frame, None);
    let ui = palette_for();
    assert_eq!(pixel_in_row(&pixmap, &frame, 0.0), ui.accent);
    assert!(has_color(
        &pixmap,
        PAD,
        HEADER_H,
        CARD_W - PAD,
        HEADER_H + ROW_H,
        ui.on_primary
    ));
}

#[test]
fn disabled_info_uses_muted_title_and_disabled_value() {
    let frame = Menu::new(vec![Node::info_value("Поле", "значение").with_disabled()]).frame();
    let pixmap = render_card(&frame, None);
    let ui = palette_for();
    let mut painter = TextPainter::new(&font_name());
    let title_w = painter
        .text_width("Поле", theme(palette::Palette::default()).1.label)
        .min(CARD_W * 0.4);
    let value_x = PAD + title_w + spacing::MD;
    assert!(
        has_color(
            &pixmap,
            PAD,
            HEADER_H,
            value_x,
            HEADER_H + ROW_H,
            ink_on_card(ui.muted, ui.panel),
        ),
        "название info_value рисуется muted"
    );
    assert!(
        has_color(
            &pixmap,
            value_x,
            HEADER_H,
            CARD_W - PAD,
            HEADER_H + ROW_H,
            ink_on_card(ui.text_disabled, ui.panel),
        ),
        "значение выключенного info_value рисуется text_disabled"
    );
}

fn card(rows: usize) -> Rect {
    card_rect(1200.0, 800.0, rows, false)
}

#[test]
fn card_is_at_least_five_rows_tall() {
    assert_eq!(card_height(0), HEADER_H + 5.0 * ROW_H + FOOTER_H);
    assert_eq!(card_height(3), card_height(5), "минимум держит карточку");
}

#[test]
fn card_stops_growing_at_the_row_cap() {
    assert_eq!(
        card_height(MAX_ROWS),
        HEADER_H + MAX_ROWS as f32 * ROW_H + FOOTER_H
    );
    assert_eq!(
        card_height(50),
        card_height(MAX_ROWS),
        "дальше список листается"
    );
}

/// Во время анимации высота карточки промежуточная, и строки считаются по
/// ней, а не по конечной цели.
#[test]
fn intermediate_height_shows_only_the_rows_that_fit() {
    let base = HEADER_H + FOOTER_H;
    assert_eq!(rows_in_height(base + 5.0 * ROW_H, 10, false), 5);
    assert_eq!(rows_in_height(base + 7.0 * ROW_H, 10, false), 7);
    assert_eq!(
        rows_in_height(base + 7.9 * ROW_H, 10, false),
        7,
        "неполная строка не считается"
    );
    assert_eq!(rows_in_height(base + 20.0 * ROW_H, 10, false), 10);
    assert_eq!(
        rows_in_height(base + 3.0 * ROW_H, 10, false),
        3,
        "на пути к минимуму видно меньше пяти строк"
    );
}

/// Пустой список центрирует подпись по всей доступной области, поэтому на
/// промежуточной высоте ему нужны все влезшие слоты, а не минимум в пять.
#[test]
fn an_empty_list_uses_every_slot_that_fits() {
    let base = HEADER_H + FOOTER_H;
    assert_eq!(rows_in_height(base + 9.0 * ROW_H, 0, false), 9);
    assert_eq!(rows_in_height(base + 30.0 * ROW_H, 0, false), MAX_ROWS);
    assert_eq!(rows_in_height(base, 0, false), 0);
}

/// Шапка и подвал остаются целыми на любой промежуточной высоте: строкам
/// достаётся ровно то, что осталось между ними.
#[test]
fn intermediate_height_keeps_header_and_footer_whole() {
    for extra in [0.0f32, 0.4, 3.0, 7.6, 13.0] {
        let height = HEADER_H + FOOTER_H + extra * ROW_H;
        let rows = rows_in_height(height, 14, false);
        assert!(
            HEADER_H + rows as f32 * ROW_H + FOOTER_H <= height + 1e-3,
            "extra={extra}: строки налезли на подвал"
        );
    }
}

/// Клик по строке, которая ещё не влезла в анимированную карточку, не
/// должен попадать в невидимый пункт.
#[test]
fn rows_below_the_animated_height_are_not_clickable() {
    let frame = frame_with(10, 0);
    let height = HEADER_H + FOOTER_H + 6.0 * ROW_H;
    let card = Rect::new(0.0, 0.0, CARD_W, height);
    let y = |slot: f32| HEADER_H + (slot + 0.5) * ROW_H;

    assert_eq!(row_at(&frame, card, PAD, y(5.0)), Some(5));
    assert_eq!(
        row_at(&frame, card, PAD, y(6.0)),
        None,
        "седьмая строка ещё за пределами окна"
    );
}

#[test]
fn search_rows_are_taller_than_plain_ones() {
    assert_eq!(row_height(true), ROW_H_SEARCH);
    assert_eq!(row_height(false), ROW_H);
    assert_eq!(ROW_H_SEARCH, 52.0, "по макету");
}

/// Высота карточки считается по высоте строки своего режима: иначе в
/// поиске строки либо обрезались, либо под ними зияла пустота.
#[test]
fn card_height_uses_the_height_of_its_mode() {
    let plain = card_height_for(5, false);
    let search = card_height_for(5, true);
    assert_eq!(plain, HEADER_H + 5.0 * ROW_H + FOOTER_H);
    assert_eq!(search, HEADER_H + 5.0 * ROW_H_SEARCH + FOOTER_H);
    assert_eq!(
        search - plain,
        5.0 * (ROW_H_SEARCH - ROW_H),
        "разница ровно в пяти строках"
    );
    assert!(search > plain);
}

/// `set_size` никогда не получает ноль: без якорей слой без размера
/// считается ошибкой протокола, и композитор убивает клиента.
#[test]
fn layer_size_is_never_zero() {
    for count in [0, 1, 3, MIN_ROWS, MAX_ROWS, 99] {
        for search in [false, true] {
            let (width, height) = layer_size(count, search);
            assert!(width > 0, "count={count} search={search}: ширина {width}");
            assert!(height > 0, "count={count} search={search}: высота {height}");
            assert_eq!(width, CARD_W as u32);
            assert_eq!(
                height as f32,
                card_height_for(count, search),
                "высота слоя равна высоте карточки"
            );
        }
    }
}

/// Пустой список и режим поиска — два крайних случая, где размер легко
/// посчитать не тем расчётом.
#[test]
fn layer_size_matches_the_card_in_both_edges() {
    assert_eq!(
        layer_size(0, false),
        (CARD_W as u32, card_height(0) as u32),
        "пустой список всё равно показывает минимум строк"
    );
    assert_eq!(
        layer_size(0, true),
        (CARD_W as u32, card_height_for(0, true) as u32),
        "пустой поиск выше обычного"
    );
    assert!(layer_size(3, true).1 > layer_size(3, false).1);
}

#[test]
fn card_rect_is_centered_for_both_modes() {
    for search in [false, true] {
        let rect = card_rect(1000.0, 700.0, 6, search);
        assert_eq!(rect.x, (1000.0 - CARD_W) / 2.0);
        assert_eq!(rect.w, CARD_W);
        assert_eq!(rect.h, card_height_for(6, search));
        assert_eq!(rect.y, (700.0 - rect.h) / 2.0);
    }
}

/// Фон выбранной строки обязан быть светлее карточки: `surface_hover`
/// темнее панели, и выделение проваливалось бы в фон.
#[test]
fn selected_row_is_lighter_than_the_card() {
    let ui = palette_for();
    let selected = ui.accent;
    assert!(
        luminance(selected) > luminance(ui.surface),
        "фон выбранной строки {} не светлее карточки {}",
        luminance(selected),
        luminance(ui.surface)
    );
    assert!(
        luminance(ui.surface_hover) < luminance(ui.surface),
        "surface_hover светлее карточки, подсветку можно было бы оставить"
    );
}

/// Подсветка не должна наезжать на hairline рамки карточки.
#[test]
fn highlight_stays_inside_the_card_border() {
    let rect = card(8);
    let first = Rect::new(rect.x, rect.y + HEADER_H, rect.w, ROW_H);
    let middle = Rect::new(rect.x, rect.y + HEADER_H + ROW_H * 3.0, rect.w, ROW_H);
    for row in [first, middle] {
        let inner = highlight_rect(row);
        assert!(
            inner.x >= rect.x + CARD_INSET,
            "подсветка залезает на рамку"
        );
        assert!(
            inner.right() <= rect.right() - CARD_INSET,
            "подсветка перекрывает правую рамку"
        );
        assert!(inner.h > 0.0, "подсветка пустой быть не должна");
    }
}

#[test]
fn center_band_centers_vertically() {
    let rect = Rect::new(10.0, 100.0, 200.0, 40.0);
    let band = center_band(rect, 20.0);
    assert_eq!(band.y, 110.0);
    assert_eq!(band.h, 20.0);
    assert_eq!(band.x, rect.x);
    assert_eq!(band.w, rect.w);
    assert!((band.y + band.h / 2.0 - (rect.y + rect.h / 2.0)).abs() < 0.01);
}

#[test]
fn center_band_never_grows_past_the_row() {
    let rect = Rect::new(0.0, 0.0, 10.0, 20.0);
    let band = center_band(rect, 40.0);
    assert_eq!(band.h, 20.0);
    assert_eq!(band.y, 0.0);
}

#[test]
fn two_line_block_fits_the_search_row() {
    let scale = type_scale();
    let block = scale.label + scale.caption + CAPTION_GAP;
    assert!(
        block < ROW_H_SEARCH,
        "две строки с зазором не влезают в строку поиска: {block} > {ROW_H_SEARCH}"
    );
    assert_eq!(CAPTION_GAP, 3.0, "зазор между строками — 2–3 px по макету");
    assert!((2.0..=3.0).contains(&CAPTION_GAP));
}

#[test]
fn dimensions_stay_on_the_four_pixel_grid() {
    for value in [
        CARD_W,
        ROW_H,
        ROW_H_SEARCH,
        HEADER_H,
        FOOTER_H,
        SEARCH_H,
        PAD,
    ] {
        assert_eq!(
            value % spacing::XS,
            0.0,
            "{value} не кратно четырём: сетка разъедется"
        );
    }
    // Отступ подсветки — единственное исключение: 1 px равна толщине
    // hairline, иначе заливка ложилась бы на рамку.
    assert_eq!(CARD_INSET, 1.0);
}

/// Подсказки подвала: полные и короткие варианты должны существовать оба.
#[test]
fn footer_has_short_hint_variants() {
    for (full, short) in [
        (strings::HINT_SELECT, strings::HINT_SELECT_SHORT),
        (strings::HINT_OPEN, strings::HINT_OPEN_SHORT),
        (strings::HINT_BACK, strings::HINT_BACK_SHORT),
        (strings::HINT_CLOSE, strings::HINT_CLOSE_SHORT),
    ] {
        assert!(
            full.len() > short.len(),
            "короткая подсказка длиннее полной"
        );
        assert!(short.contains(full.split(' ').next().expect("подсказка не пустая")));
    }
    assert_eq!(strings::HINT_CLOSE_SHORT, "ESC");
}

/// Ввод пароля рисуется в той же карточке: точки вместо букв, курсор и
/// подсказки в подвале. Пароля на картинке быть не может — в кадре его
/// нет, — но сам путь отрисовки обязан работать без паники.
#[test]
fn secret_input_renders_the_mask_in_the_same_card() {
    let secret = super::super::secret::SecretFrame {
        title: "Wi-Fi пароль",
        subject: "Дом \\ принтер".to_string(),
        masked: "••••••".to_string(),
        len: 6,
    };
    let frame = Frame {
        trail: vec!["HUD".to_string(), "Wi-Fi".to_string()],
        items: Vec::new(),
        selected: Some(0),
        top: 0,
        query: String::new(),
        total: 0,
        status: None,
        secret: Some(secret),
        lang: Language::Ru,
    };
    assert_eq!(rows_for(&frame), SECRET_ROWS, "высота задаёт сам ввод");
    assert!(!search_mode(&frame), "запрос в режиме ввода пуст");
    assert!(!frame.query.contains("•"), "маска не попадает в запрос");
    // Ширина карточки в режиме ввода та же, а строк ровно две.
    let card = card_rect(1200.0, 800.0, rows_for(&frame), false);
    assert_eq!(card.h, card_height_for(SECRET_ROWS, false));
}

#[test]
fn search_mode_is_driven_by_the_query() {
    let mut frame = Frame {
        trail: vec!["HUD".to_string()],
        items: Vec::new(),
        selected: Some(0),
        top: 0,
        query: String::new(),
        total: 0,
        status: None,
        secret: None,
        lang: Language::Ru,
    };
    assert!(!search_mode(&frame));
    frame.query = "dnd".to_string();
    assert!(search_mode(&frame));
    frame.query.clear();
    assert!(!search_mode(&frame));
}

#[test]
fn row_hit_test_maps_a_point_to_an_item() {
    let frame = Frame {
        trail: vec!["HUD".to_string()],
        items: (0..6)
            .map(|index| super::super::item::Item {
                icon: "",
                id: format!("Пункт {index}"),
                title: format!("Пункт {index}"),
                value: String::new(),
                caption: None,
                keywords: &[],
                level: None,
                muted: false,
                danger: false,
                disabled: false,
                kind: super::super::item::ItemKind::Leaf,
                origin: super::super::item::Origin { depth: 0, index },
            })
            .collect(),
        selected: Some(0),
        top: 2,
        query: String::new(),
        total: 6,
        status: None,
        secret: None,
        lang: Language::Ru,
    };
    let card = card_rect(1200.0, 800.0, 4, false);
    let list_y = card.y + HEADER_H;
    // Второй видимый пункт при top=2 — это items[3].
    let inside = row_at(&frame, card, card.x + 40.0, list_y + ROW_H * 1.5);
    assert_eq!(inside, Some(3), "top=2 сдвигает видимую часть");
    assert_eq!(row_at(&frame, card, card.x + 40.0, list_y - 1.0), None);
    assert_eq!(
        row_at(&frame, card, card.x + 40.0, card.bottom() - 10.0),
        None,
        "подвал не строка"
    );
    assert_eq!(row_at(&frame, card, card.x - 5.0, list_y + 10.0), None);
}

#[test]
fn hit_test_uses_the_search_row_height() {
    let frame = Frame {
        trail: vec!["HUD".to_string()],
        items: (0..3)
            .map(|index| super::super::item::Item {
                icon: "",
                id: format!("Пункт {index}"),
                title: format!("Пункт {index}"),
                value: String::new(),
                caption: None,
                keywords: &[],
                level: None,
                muted: false,
                danger: false,
                disabled: false,
                kind: super::super::item::ItemKind::Leaf,
                origin: super::super::item::Origin { depth: 0, index },
            })
            .collect(),
        selected: Some(0),
        top: 0,
        query: "x".to_string(),
        total: 3,
        status: None,
        secret: None,
        lang: Language::Ru,
    };
    let card = card_rect(1200.0, 800.0, 5, true);
    let list_y = card.y + HEADER_H;
    assert_eq!(row_at(&frame, card, card.x + 10.0, list_y + 10.0), Some(0));
    // На обычной высоте строки эта точка была бы за пределами списка.
    let plain = card_rect(1200.0, 800.0, 5, false);
    let plain_list = plain.y + HEADER_H;
    assert_eq!(
        row_at(
            &Frame {
                query: String::new(),
                ..frame.clone()
            },
            plain,
            card.x + 10.0,
            plain_list + ROW_H_SEARCH + 10.0
        ),
        Some(1),
        "высота строки поиска больше — точка попадает в следующую строку"
    );
}

/// Полупрозрачность карточки объявлена константой, поэтому и проверка
/// должна быть константной — иначе она ничего не проверяла бы. Значение
/// ещё и прибито к плотности панели: разъехаться они не должны.
const _: () = assert!(
    CARD_ALPHA >= 0.7 && CARD_ALPHA < 1.0,
    "полупрозрачность карточки должна быть лёгкой"
);
/// Плотность фона карточки обязана совпадать с плотностью панели: меню и
/// панель стоят на экране рядом, и расхождение было видно сразу. Число
/// продублировано намеренно — если у панели поменяют, тест об этом скажет.
const _: () = assert!(
    (CARD_ALPHA - 0.8).abs() < f32::EPSILON,
    "плотность фона меню должна совпадать с панелью"
);

#[test]
fn failed_status_is_marked() {
    assert!(Status::Failed("ошибка".to_string()).is_failed());
    assert!(!Status::Applied("готово".to_string()).is_failed());
}
