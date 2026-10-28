use super::super::geom::Rect;
use super::super::palette;
use super::super::state::Frame;
use super::super::text::SCALE;
use super::super::ui_tokens::spacing;
use super::MenuView;
use super::secret::SECRET_ROWS;

/// Ширина карточки. Фиксированная: подменю того же меню не должны менять
/// размер окна при переходе.
pub const CARD_W: f32 = 440.0;
/// Высота строки списка в обычном режиме.
pub const ROW_H: f32 = 40.0;
/// Высота строки в режиме поиска: две строки текста плюс зазор.
pub const ROW_H_SEARCH: f32 = 52.0;
/// Зазор между названием и путём в строке поиска.
pub const CAPTION_GAP: f32 = 3.0;
/// Отступ подсветки выбранной строки от края карточки. Равен толщине
/// hairline: иначе заливка ложилась бы прямо на рамку и рвала её.
pub const CARD_INSET: f32 = 1.0;
/// Минимум строк в карточке. Ниже карточка прыгает при каждом нажатии.
pub const MIN_ROWS: usize = 5;
/// Максимум строк: столько занимает самый длинный уровень меню — карточка
/// вывода дисплея со справкой, настройками и выключением. Дальше список уже
/// нужно прокручивать.
pub const MAX_ROWS: usize = 14;
/// Высота шапки с крошками и поиском.
pub const HEADER_H: f32 = 52.0;
/// Высота подвала с подсказками.
pub const FOOTER_H: f32 = 40.0;
/// Высота поля поиска.
pub const SEARCH_H: f32 = 28.0;
/// Ширина поля поиска.
pub const SEARCH_W: f32 = 168.0;
/// Размер иконки строки.
pub const ICON: f32 = 20.0;
/// Отступ текста от края карточки.
pub const PAD: f32 = spacing::LG;
/// Размер кружка тумблера.
pub const TOGGLE_D: f32 = 16.0;
/// Диаметр миниатюры пикера.
pub const THUMB_D: f32 = 28.0;
/// Ширина зоны значения справа.
pub const VALUE_W: f32 = 168.0;
/// Ширина полосы громкости между названием и процентом. Места под полосу
/// взято у значения: `VALUE_W` в 168 px нужен длинным значениям вроде
/// «Масштаб 100%», а громкости хватает короткой подписи, и оставшееся
/// ушло бы в пустоту.
pub const METER_W: f32 = 150.0;
/// Место под процент громкости: «100% выкл» — самый длинный текст.
pub const VOLUME_VALUE_W: f32 = 92.0;
/// Толщина полосы: тоньше строки текста, но не волосок.
pub const METER_H: f32 = 6.0;
/// Ширина зоны шеврона.
pub const CHEVRON_W: f32 = 16.0;
/// Ширина полосы прокрутки у правого края списка.
pub const SCROLLBAR_W: f32 = 2.0;
/// Отступ полосы прокрутки от края карточки.
pub const SCROLLBAR_INSET: f32 = PAD / 2.0;
/// Диаметр точки статуса в подвале. Точка сообщает, что сообщение именно об
/// изменении, а не о позиции в списке.
pub const STATUS_DOT: f32 = 8.0;
/// Плотность затемнения вокруг карточки.
pub const SCRIM: f32 = 0.72;
/// Прозрачность фона карточки в живом окне. Слой размывают правилом niri, и
/// сквозь полупрозрачную карточку видно, что именно размывается. Снимки
/// затемнение рисуют целиком, поэтому берут единицу.
///
/// Значение то же, что у панели (`p.base.with_a(0.8)` в рендере бара): меню и
/// панель — соседи на экране, и разная плотность фона между ними читалась как
/// «одно окно тёмное, другое нет».
pub const CARD_ALPHA: f32 = 0.8;

/// Режим ли поиск: в нём строки двухстрочные и выше. Флаг один, а не вывод
/// из `caption`: путь есть только у найденной строки, а режим задаёт запрос.
pub fn search_mode(frame: &Frame) -> bool {
    !frame.query.is_empty()
}

/// Высота строки в текущем режиме.
pub fn row_height(search: bool) -> f32 {
    if search { ROW_H_SEARCH } else { ROW_H }
}

/// Высота полосы, внутри которой центрируется всё содержимое строки. В
/// поисковой строке это две строки текста, в обычной — одна.
pub fn row_band(view: &MenuView<'_>) -> f32 {
    let single = view.scale.label * 2.0;
    if view.scale.label + view.scale.caption + CAPTION_GAP > single {
        view.scale.label + view.scale.caption + CAPTION_GAP
    } else {
        single
    }
}

/// Сколько строк показывает карточка: от `MIN_ROWS` до `MAX_ROWS`.
/// Сколько строк рисует карточка: в режиме ввода пароля список сетей скрыт,
/// и высоту задаёт сам ввод.
pub fn rows_for(frame: &Frame) -> usize {
    if frame.secret.is_some() {
        return SECRET_ROWS;
    }
    // Столько строк реально нарисуется: сколько осталось ниже `top`. Считать
    // по `items.len()` нельзя — при прокрутке к концу карточка вырастала на
    // прокрученные строки, и под последней строкой зияла пустота.
    rows_shown(frame.items.len().saturating_sub(frame.top))
}

pub fn rows_shown(count: usize) -> usize {
    count.clamp(MIN_ROWS, MAX_ROWS)
}

/// Сколько строк помещается в карточку высотой `height`.
///
/// Пока окно едет между старой и новой высотой, рисовать и попадать кликом
/// можно только по строкам, которые реально влезли: остальные оказались бы
/// под подвалом или за краем буфера. Пустой список — особый случай: строк нет,
/// но подпись «пусто» центрируется по всей доступной области, поэтому для
/// него возвращается всё, что помещается, а не минимум в пять строк.
pub fn rows_in_height(height: f32, count: usize, search: bool) -> usize {
    let available = (height - HEADER_H - FOOTER_H).max(0.0);
    let fits = (available / row_height(search)).floor() as usize;
    if count == 0 {
        fits.min(MAX_ROWS)
    } else {
        rows_shown(count).min(fits)
    }
}

/// Высота карточки: строки считаются по высоте своего режима, иначе поиск
/// либо обрезался, либо оставлял пустое поле.
pub fn card_height_for(rows: usize, search: bool) -> f32 {
    HEADER_H + rows_shown(rows) as f32 * row_height(search) + FOOTER_H
}

/// Высота карточки в обычном режиме.
pub fn card_height(rows: usize) -> f32 {
    card_height_for(rows, false)
}

/// Размер слоя для `set_size`. Обе оси строго больше нуля: слой без якорей
/// не имеет права получить `0` ни по ширине, ни по высоте, и композитор
/// рвёт протокол на первом же commit. Высота берётся из карточки своего
/// режима, поэтому поиск и пустой список тоже дают честный размер.
pub fn layer_size(count: usize, search: bool) -> (u32, u32) {
    let rows = rows_shown(count);
    let width = CARD_W.round().max(1.0) as u32;
    let height = card_height_for(rows, search).round().max(1.0) as u32;
    (width, height)
}

/// Относительная яркость цвета: 0 — чёрный, 1 — белый. Нужна тесту «фон
/// выбранной строки светлее карточки» в обеих темах.
pub fn luminance(color: palette::Rgba) -> f32 {
    (0.2126 * color.0 as f32 + 0.7152 * color.1 as f32 + 0.0722 * color.2 as f32) / 255.0
}

/// Прямоугольник подсветки выбранной строки: та же полоса списка, но с отступом
/// от краёв карточки, чтобы hairline остался целым.
pub fn highlight_rect(row: Rect) -> Rect {
    Rect::new(row.x + CARD_INSET, row.y, row.w - CARD_INSET * 2.0, row.h)
}

/// Полоса высотой `height`, отцентрованная по вертикали внутри `rect`.
/// Единственный расчёт вертикального центра: иконка, название, значение и
/// подпись идут через него, поэтому не могут разойтись на пиксель.
pub fn center_band(rect: Rect, height: f32) -> Rect {
    let height = height.min(rect.h);
    Rect::new(rect.x, rect.y + (rect.h - height) / 2.0, rect.w, height)
}

/// Какая строка под указателем. Возвращает индекс в `Frame::items`, а не
/// номер видимой строки: с прокруткой они разойдутся, и клик уедет.
pub fn row_at(frame: &Frame, card: Rect, x: f32, y: f32) -> Option<usize> {
    let search = search_mode(frame);
    let row_h = row_height(search);
    // Строки считаются по фактической высоте карточки, а не по целевой:
    // во время анимации окно ниже, и нижние строки ещё не видны — клик по
    // ним не должен попадать в невидимый пункт.
    let rows = rows_in_height(card.h, frame.items.len(), search);
    let list = Rect::new(card.x, card.y + HEADER_H, card.w, rows as f32 * row_h);
    if x < card.x || x > card.right() || y < list.y || y >= list.bottom() {
        return None;
    }
    let visible = ((y - list.y) / row_h).floor() as usize;
    let absolute = frame.top + visible;
    (absolute < frame.items.len()).then_some(absolute)
}

/// Строка под указателем, если её можно выбрать. Header и Info не получают
/// ни клика, ни hover: указатель над ними означает «мимо».
pub fn selectable_row_at(frame: &Frame, card: Rect, x: f32, y: f32) -> Option<usize> {
    row_at(frame, card, x, y).filter(|index| {
        frame
            .items
            .get(*index)
            .is_some_and(|item| item.kind.is_selectable())
    })
}

/// Делает пиксели карточки полупрозрачными. Буфер premultiplied, поэтому
/// домножаются все четыре канала: цвета уже «внутри» альфы.
pub fn apply_card_alpha(pixmap: &mut tiny_skia::Pixmap, card: Rect, alpha: f32) {
    let factor = (alpha.clamp(0.0, 1.0) * 255.0) as u32;
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let x0 = (card.x * SCALE).floor() as i32;
    let y0 = (card.y * SCALE).floor() as i32;
    let x1 = (card.right() * SCALE).ceil() as i32;
    let y1 = (card.bottom() * SCALE).ceil() as i32;
    let pixels = pixmap.pixels_mut();
    for y in y0.max(0)..y1.min(height) {
        for x in x0.max(0)..x1.min(width) {
            let pixel = &mut pixels[y as usize * width as usize + x as usize];
            if let Some(scaled) = tiny_skia::PremultipliedColorU8::from_rgba(
                (pixel.red() as u32 * factor / 255) as u8,
                (pixel.green() as u32 * factor / 255) as u8,
                (pixel.blue() as u32 * factor / 255) as u8,
                (pixel.alpha() as u32 * factor / 255) as u8,
            ) {
                *pixel = scaled;
            }
        }
    }
}

/// Прямоугольник карточки по центру кадра. Затемнение рисуется по всему
/// кадру, а карточка — по этому прямоугольнику.
pub fn card_rect(frame_w: f32, frame_h: f32, rows: usize, search: bool) -> Rect {
    let height = card_height_for(rows, search);
    Rect::new(
        (frame_w - CARD_W) / 2.0,
        (frame_h - height) / 2.0,
        CARD_W,
        height,
    )
}

/// Смешение цвета строки по близости к едущей полосе.
///
/// Отдельная функция, а не `ui_tokens::mix`: та выставляет альфу в 255, а цвета
/// текста в меню полупрозрачны — на `t = 0` кадр бы посветлел. Здесь смешиваются
/// все четыре канала, поэтому на концах получаются ровно исходные цвета.
pub fn blend(a: palette::Rgba, b: palette::Rgba, t: f32) -> palette::Rgba {
    let t = t.clamp(0.0, 1.0);
    let channel = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * t) as u8;
    palette::Rgba(
        channel(a.0, b.0),
        channel(a.1, b.1),
        channel(a.2, b.2),
        channel(a.3, b.3),
    )
}
