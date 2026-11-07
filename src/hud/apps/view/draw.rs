use super::*;

/// Подсветка выбранной строки: сплошная заливка акцентом — та же, что в меню.
pub(crate) fn highlight_band(view: &mut AppsView<'_>, rect: Rect) {
    let inner = Rect::new(
        rect.x + CARD_INSET,
        rect.y,
        rect.w - CARD_INSET * 2.0,
        rect.h,
    );
    fill_rect(
        view.pixmap,
        inner.x * SCALE,
        inner.y * SCALE,
        inner.w * SCALE,
        inner.h * SCALE,
        view.ui.accent,
    );
}

/// Рисует окно целиком и возвращает прямоугольник карточки.
///
/// `height` — текущая высота окна, а не расчётная высота карточки: во время
/// переключения вида и поиска слой едет к новой высоте, и подвал должен идти
/// вместе с ним, а не ждать последнего кадра. В покое значения совпадают.
///
/// `highlight` — скользящая полоса в строках экрана (`None`, пока подсветка не
/// начала ход), `hover` — строка под указателем: она подсвечивается, но не
/// выбирается, как и в меню.
pub fn render_window(
    view: &mut AppsView<'_>,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
    height: f32,
) -> Rect {
    let frame = Rect::new(0.0, 0.0, CARD_W, height);
    card(view, frame);
    header(view, frame, model);
    match model.view() {
        ViewMode::List => rows_view(view, frame, model, loader, highlight),
        ViewMode::Tiles => tiles_view(view, frame, model, loader, highlight),
    }
    footer(view, frame, model);
    frame
}

/// Подложка карточки и рамка.
pub(crate) fn card(view: &mut AppsView<'_>, rect: Rect) {
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::WINDOW) * SCALE,
        view.ui.panel,
    );
    stroke_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        view.ui.border,
        SCALE,
    );
}

/// Шапка: название окна слева, поле поиска справа.
pub(crate) fn header(view: &mut AppsView<'_>, card: Rect, model: &Model) {
    let rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    let title = view.scale.section_title;
    let label = match model.lang() {
        Language::Ru => "Приложения",
        Language::En => "Applications",
    };
    view.painter.paint_boxed(
        view.pixmap,
        label,
        title,
        view.ui.text,
        center_band(
            Rect::new(rect.x + PAD, rect.y, rect.w / 2.0 - PAD, rect.h),
            title * 2.0,
        ),
        Align::Start,
    );
    search(view, rect, model);
}

/// Поле поиска: лупа, набранный текст с курсором и крестик очистки.
pub(crate) fn search(view: &mut AppsView<'_>, header: Rect, model: &Model) {
    let rect = search_rect(header);
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::SM) * SCALE,
        view.ui.idle_panel,
    );
    stroke_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        view.ui.border_focus,
        SCALE,
    );
    view.icons.paint(
        view.pixmap,
        settings_icons::SEARCH,
        view.scale.caption,
        view.ui.muted,
        center_band(
            Rect::new(rect.x + spacing::SM, rect.y, 16.0, rect.h),
            view.scale.label * 2.0,
        ),
        Align::Center,
    );
    let clear = model.searching();
    let text_x = rect.x + spacing::SM + 16.0 + spacing::SM;
    let text_w = rect.w
        - (spacing::SM + 16.0 + spacing::SM)
        - spacing::SM
        - if clear { 16.0 + spacing::XS } else { 0.0 };
    let text_rect = center_band(
        Rect::new(text_x, rect.y, text_w.max(0.0), rect.h),
        view.scale.label * 2.0,
    );
    let query = model.query();
    if query.is_empty() {
        let placeholder = match model.lang() {
            Language::Ru => "Поиск",
            Language::En => "Search",
        };
        view.painter.paint_boxed(
            view.pixmap,
            placeholder,
            view.scale.caption,
            view.ui.text_disabled,
            text_rect,
            Align::Start,
        );
    } else {
        view.painter.paint_boxed(
            view.pixmap,
            query,
            view.scale.label,
            view.ui.text,
            text_rect,
            Align::Start,
        );
        // Курсор: окно работает только с клавиатуры, поэтому он всегда виден.
        let caret_x = text_x + view.painter.text_width(query, view.scale.label);
        fill_rect(
            view.pixmap,
            (caret_x + 1.0) * SCALE,
            (rect.y + spacing::SM) * SCALE,
            2.0 * SCALE,
            (rect.h - spacing::SM * 2.0) * SCALE,
            view.ui.accent,
        );
    }
    if clear {
        view.icons.paint(
            view.pixmap,
            settings_icons::TIMES,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(rect.right() - spacing::SM - 16.0, rect.y, 16.0, rect.h),
                view.scale.label * 2.0,
            ),
            Align::Center,
        );
    }
}

/// Список строк: заголовки секций и приложения.
pub(crate) fn rows_view(
    view: &mut AppsView<'_>,
    card: Rect,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
) {
    let visible = model.visible();
    let rows = rows_shown(visible.len());
    let list = list_rect(card, model);
    let selected = model.selected();
    if let Some(position) = highlight.sliding
        && !visible.is_empty()
    {
        let slot = position.clamp(0.0, visible.len() as f32 - 1.0);
        highlight_band(
            view,
            Rect::new(list.x, list.y + slot * ROW_H, list.w, ROW_H),
        );
    }
    for (slot, row) in visible.iter().enumerate() {
        let rect = Rect::new(list.x, list.y + slot as f32 * ROW_H, list.w, ROW_H);
        let absolute = model.top() + slot;
        match row {
            Row::Section(title) => section_row(view, rect, title),
            Row::Blank => {}
            Row::App(_) => {
                let Some(app) = model.app_at(row) else {
                    continue;
                };
                let is_selected = absolute == selected;
                let near = match highlight.sliding {
                    Some(position) => (1.0 - (position - slot as f32).abs()).clamp(0.0, 1.0),
                    None => usize::from(is_selected) as f32,
                };
                app_row(
                    view,
                    rect,
                    app,
                    loader,
                    show_pin(model, absolute, is_selected),
                    is_selected && highlight.sliding.is_none(),
                    near,
                    !is_selected && absolute == highlight.hover.unwrap_or(usize::MAX),
                );
            }
        }
    }
    if let Some(track) = scrollbar_rect(
        list,
        card.right(),
        model.total(),
        model.top(),
        visible.len(),
    ) {
        fill_rect(
            view.pixmap,
            track.x * SCALE,
            track.y * SCALE,
            track.w * SCALE,
            track.h * SCALE,
            view.ui.muted,
        );
    }
    if model.total() == 0 {
        let notice = match model.lang() {
            Language::Ru => "Ничего не найдено",
            Language::En => "Nothing found",
        };
        view.painter.paint_boxed(
            view.pixmap,
            notice,
            view.scale.label,
            view.ui.muted,
            Rect::new(list.x, list.y, list.w, rows as f32 * ROW_H),
            Align::Center,
        );
    }
}

/// Заголовок секции: подпись без иконки, выбрать её нельзя.
pub(crate) fn section_row(view: &mut AppsView<'_>, rect: Rect, title: &str) {
    view.painter.paint_boxed(
        view.pixmap,
        title,
        view.scale.caption,
        view.ui.muted,
        center_band(
            Rect::new(rect.x + PAD, rect.y, rect.w - PAD * 2.0, rect.h),
            view.scale.caption * 2.0,
        ),
        Align::Start,
    );
}

/// Сетка плиток: только иконки, закреплённые идут первыми, блоки разделены
/// подписью с линией.
pub(crate) fn tiles_view(
    view: &mut AppsView<'_>,
    card: Rect,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
) {
    let list = list_rect(card, model);
    let visible = model.visible();
    let selected = model.selected();
    for (slot, row) in visible.iter().enumerate() {
        let Some(app) = model.app_at(row) else {
            continue;
        };
        let absolute = model.top() + slot;
        let is_selected = absolute == selected;
        let hover = !is_selected && absolute == highlight.hover.unwrap_or(usize::MAX);
        tile(
            view,
            tile_rect(list, slot, model),
            app,
            loader,
            model.is_pinned(&app.id),
            is_selected,
            hover,
        );
    }
    // Граница блоков: закреплённые кончаются на границе ряда, и разделитель
    // садится в существующий зазор, не раздвигая сетку.
    let scrollbar = scrollbar_rect(
        list,
        card.right(),
        model.total(),
        model.top(),
        visible.len(),
    );
    if let Some(at) = model.tiles_break() {
        let break_row = at / GRID_COLS;
        let top_row = model.top() / GRID_COLS;
        if break_row > top_row && break_row < top_row + tile_rows_shown(model.total()) {
            let row = break_row - top_row;
            let row_y = list.y + row as f32 * (TILE + TILE_GAP) + SECTION_EXTRA;
            let band = TILE_GAP + SECTION_EXTRA;
            tiles_divider(view, list, row_y - band / 2.0, band, model.lang());
        }
    }
    if let Some(track) = scrollbar {
        fill_rect(
            view.pixmap,
            track.x * SCALE,
            track.y * SCALE,
            track.w * SCALE,
            track.h * SCALE,
            view.ui.muted,
        );
    }
    if model.total() == 0 {
        let notice = match model.lang() {
            Language::Ru => "Ничего не найдено",
            Language::En => "Nothing found",
        };
        view.painter.paint_boxed(
            view.pixmap,
            notice,
            view.scale.label,
            view.ui.muted,
            Rect::new(list.x, list.y, list.w, list.h.max(TILE)),
            Align::Center,
        );
    }
}

/// Разделитель блоков сетки: подпись и тонкая линия в зазоре между рядами.
pub(crate) fn tiles_divider(
    view: &mut AppsView<'_>,
    list: Rect,
    y: f32,
    band: f32,
    lang: Language,
) {
    let label = match lang {
        Language::Ru => "Все приложения",
        Language::En => "All apps",
    };
    let size = view.scale.caption;
    let width = view.painter.text_width(label, size);
    view.painter.paint_boxed(
        view.pixmap,
        label,
        size,
        view.ui.muted,
        Rect::new(list.x, y - band / 2.0, width, band),
        Align::Start,
    );
    let x = list.x + width + spacing::SM;
    fill_rect(
        view.pixmap,
        x * SCALE,
        (y - 0.5) * SCALE,
        (list.right() - x) * SCALE,
        SCALE,
        view.ui.border,
    );
}

/// Плитка приложения: иконка на подложке, булавка у закреплённых.
pub(crate) fn tile(
    view: &mut AppsView<'_>,
    rect: Rect,
    app: &crate::apps::scan::App,
    loader: &Loader,
    pinned: bool,
    selected: bool,
    hover: bool,
) {
    let color = if selected {
        view.ui.accent
    } else if hover {
        view.ui.hover_bg
    } else {
        view.ui.idle_panel
    };
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::MD) * SCALE,
        color,
    );
    let icon = Rect::new(
        rect.x + (TILE - TILE_ICON) / 2.0,
        rect.y + (TILE - TILE_ICON) / 2.0,
        TILE_ICON,
        TILE_ICON,
    );
    match loader.get(&app.icon) {
        Some(bitmap) => icons::blit(view.pixmap, &bitmap, icon.x, icon.y, TILE_ICON),
        None => placeholder(
            view,
            icon,
            if selected { 1.0 } else { 0.0 },
            loader.has_failed(&app.icon),
        ),
    }
    if pinned {
        view.icons.paint(
            view.pixmap,
            settings_icons::PIN,
            view.scale.caption,
            if selected {
                view.ui.on_primary
            } else {
                view.ui.muted
            },
            Rect::new(rect.right() - 18.0, rect.y + 4.0, 14.0, 14.0),
            Align::Center,
        );
    }
}

/// Рисовать ли булавку на строке.
/// Булавка рисуется у каждого закреплённого приложения всегда, включая секцию
/// «Закреплённые»: это единый признак закреплённости во всём списке.
pub(crate) fn show_pin(model: &Model, row: usize, _selected: bool) -> bool {
    let Some(Row::App(index)) = model.rows().get(row) else {
        return false;
    };
    let Some(app) = model.apps().get(*index) else {
        return false;
    };
    model.is_pinned(&app.id)
}

/// Строка приложения: иконка, имя и метка закрепления справа.
#[allow(clippy::too_many_arguments)]
pub(crate) fn app_row(
    view: &mut AppsView<'_>,
    rect: Rect,
    app: &crate::apps::scan::App,
    loader: &Loader,
    pinned: bool,
    selected: bool,
    near: f32,
    hover: bool,
) {
    // Подложка под указателем — та же заливка без смены цвета текста: мышь
    // показывает, где кликнёт, но не выбирает. Выбранная строка всегда
    // акцентом — даже под курсором и при едущей полосе.
    if selected {
        highlight_band(view, rect);
    } else if hover {
        let inner = Rect::new(
            rect.x + CARD_INSET,
            rect.y,
            rect.w - CARD_INSET * 2.0,
            rect.h,
        );
        fill_rect(
            view.pixmap,
            inner.x * SCALE,
            inner.y * SCALE,
            inner.w * SCALE,
            inner.h * SCALE,
            view.ui.hover_bg,
        );
    }
    // Цвет текста смешивается с близостью к едущей полосе: строка, которую полоса
    // сейчас проходит, проявляется постепенно. Выбранный текст — тёмный
    // `on_primary`: акцентом он не читался бы на заливке акцента.
    let near = near.clamp(0.0, 1.0);
    let text_color = blend(view.ui.text, view.ui.on_primary, near);
    let icon_rect = center_band(Rect::new(rect.x + PAD, rect.y, ICON, rect.h), ICON);
    let mark_w = if pinned { 16.0 + spacing::SM } else { 0.0 };
    let label = Rect::new(
        icon_rect.right() + GAP,
        rect.y,
        rect.right() - PAD - mark_w - (icon_rect.right() + GAP),
        rect.h,
    );
    match loader.get(&app.icon) {
        Some(icon) => icons::blit(view.pixmap, &icon, icon_rect.x, icon_rect.y, ICON),
        // Провал и ожидание рисуются по-разному: провал — плитка с глифом,
        // ожидание — просто плитка.
        None => placeholder(view, icon_rect, near, loader.has_failed(&app.icon)),
    }
    view.painter.paint_boxed(
        view.pixmap,
        &app.name,
        view.scale.label,
        text_color,
        center_band(label, view.scale.label * 2.0),
        Align::Start,
    );
    if pinned {
        view.icons.paint(
            view.pixmap,
            settings_icons::PIN,
            view.scale.caption,
            blend(view.ui.muted, view.ui.on_primary, near),
            center_band(
                Rect::new(rect.right() - PAD - 16.0, rect.y, 16.0, rect.h),
                view.scale.label * 2.0,
            ),
            Align::Center,
        );
    }
}

/// Потолок смешения заглушки с цветом текста на акценте.
///
/// Плитка идёт к тому же цвету, что и текст строки, но не дальше: на полном
/// смешении она становилась белой плиткой и читалась как значок, которого нет,
/// — то есть выдавала себя за то, чего не случилось.
pub(crate) const PLACEHOLDER_MIX: f32 = 0.35;

/// Потолок зафиксирован намертво: это требование из L1, а не вкусовое число, и
/// сдвинуть его вверх можно только вместе с новым снимком и новой проверкой
/// контраста.
const _: () = assert!(
    PLACEHOLDER_MIX <= 0.35,
    "заглушка не должна доходить до акцента"
);

/// Заглушка на месте иконки, которой нет.
///
/// Цвет — отдельная роль `ui.placeholder`: своя альфа и свой порог контраста
/// (3:1, это графика), потому что приглушённый текст на карточке даёт 4.5:1 и
/// на строке смотрелся бы слишком заметно. Проверка роли на всех модельных
/// схемах — в тестах `ui_tokens`.
pub(crate) fn placeholder(view: &mut AppsView<'_>, rect: Rect, near: f32, failed: bool) {
    // В Pixel полутонов быть не должно: смешение идёт ступенями, а плитка —
    // единственное, что на строке не пиксельное. Поэтому там смесь нулевая.
    let share = placeholder_share(near);
    let color = blend(view.ui.placeholder, view.ui.on_primary, share);
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::SM) * SCALE,
        color,
    );
    // Глиф только у провала: он отвечает на вопрос «иконки не будет», а пока
    // иконка едет, пустая плитка честнее — значок ещё появится.
    if let Some(glyph) = placeholder_glyph(failed) {
        view.icons.paint(
            view.pixmap,
            glyph,
            view.scale.caption,
            blend(view.ui.muted, view.ui.on_primary, share),
            rect,
            Align::Center,
        );
    }
}

/// Глиф заглушки: только при провале загрузки.
///
/// Глиф «приложения» — тот же, что у раздела «Приложения» в меню, отдельный
/// значок не нужен: смысл у него один, а второй кодпойнт читался бы как
/// «другая программа».
pub fn placeholder_glyph(failed: bool) -> Option<&'static str> {
    failed.then_some(settings_icons::APPS)
}

/// Доля акцента в заглушке: не больше потолка.
pub fn placeholder_share(near: f32) -> f32 {
    near.clamp(0.0, 1.0) * PLACEHOLDER_MIX
}

/// Промежуток между подсказками. Меньше, чем `LG` у меню: подсказок тут на
/// одну больше, а карточка та же, и на `LG` четвёртая строка не влезала ни в
/// каком виде.
pub(crate) const HINT_GAP: f32 = spacing::MD;

/// Сколько подвала отдаётся имени выбранной плитки. Длинное имя обрезается
/// многоточием: лучше узнать приложение по началу, чем остаться без подсказок.
pub(crate) const NAME_MAX_W: f32 = 144.0;

/// Ширина подсказок уровня: сумма текстов и промежутков между ними.
pub(crate) fn hints_width(view: &mut AppsView<'_>, hints: &[&str]) -> f32 {
    let mut width = 0.0;
    for (index, hint) in hints.iter().enumerate() {
        if index > 0 {
            width += HINT_GAP;
        }
        width += view.painter.text_width(hint, view.scale.caption);
    }
    width
}

/// Подсказки, которые влезают в отведённую ширину: уровень целиком, а если не
/// влезает даже последний — по одной, пока место есть.
pub(crate) fn fit_hints(
    view: &mut AppsView<'_>,
    available: f32,
    mode: ViewMode,
    lang: Language,
) -> Vec<&'static str> {
    let levels = hint_levels(mode, lang);
    for level in levels {
        if hints_width(view, level) <= available {
            return level.to_vec();
        }
    }
    let shortest = levels[2];
    let mut chosen: Vec<&'static str> = Vec::new();
    let mut used = 0.0;
    for hint in shortest {
        let width = view.painter.text_width(hint, view.scale.caption);
        let step = if chosen.is_empty() {
            width
        } else {
            width + spacing::LG
        };
        if used + step > available {
            break;
        }
        used += step;
        chosen.push(hint);
    }
    chosen
}

/// Подвал: подсказки слева, статус или счётчик справа.
pub(crate) fn footer(view: &mut AppsView<'_>, card: Rect, model: &Model) {
    let rect = Rect::new(card.x, card.bottom() - FOOTER_H, card.w, FOOTER_H);
    fill_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        SCALE,
        view.ui.border_subtle,
    );
    let band = center_band(rect, view.scale.caption * 2.0);
    // Правая зона резервируется по ширине, а не по факту нарисованному: иначе
    // подсказки прыгали бы влево в момент появления статуса.
    let right_zone = right_zone_width(view, model);
    let available = (rect.w - PAD * 2.0 - right_zone - spacing::LG).max(0.0);
    debug_assert!(
        right_zone <= rect.w,
        "правая зона не должна быть шире подвала"
    );
    let hints = fit_hints(view, available, model.view(), model.lang());
    let mut x = rect.x + PAD;
    for hint in hints {
        let width = view.painter.text_width(hint, view.scale.caption);
        view.painter.paint_boxed(
            view.pixmap,
            hint,
            view.scale.caption,
            view.ui.muted,
            Rect::new(x, band.y, width, band.h),
            Align::Start,
        );
        x += width + spacing::LG;
    }
    // Справа: результат действия; в сетке — имя выбранной плитки (иконки
    // подписаны не бывают), в списке — счётчик «выбрано n из m».
    let (text, is_status) = right_label(model);
    let color = if is_status {
        view.ui.accent
    } else if model.view() == ViewMode::Tiles {
        view.ui.text
    } else {
        view.ui.muted
    };
    let mut right = rect.right() - PAD;
    if !text.is_empty() {
        let width = right_text_width(view, model, &text, is_status);
        right -= width;
        view.painter.paint_boxed(
            view.pixmap,
            &text,
            view.scale.caption,
            color,
            Rect::new(right, band.y, width, band.h),
            Align::End,
        );
        right -= spacing::SM;
    }
    if is_status {
        // Точка состояния перед текстом: на ошибке запуска она читается
        // быстрее, чем сама строка.
        let dot = 6.0_f32;
        right -= dot + spacing::XS;
        fill_rect(
            view.pixmap,
            right * SCALE,
            (band.y + (band.h - dot) / 2.0) * SCALE,
            dot * SCALE,
            dot * SCALE,
            view.ui.accent,
        );
    }
}
