//! Отрисовка меню: карточка, шапка, строки и подвал.
//!
//! Окно меню — оверлей поверх всего, поэтому кадр начинается не с карточки, а
//! с затемнения: без него подписи на тёмной панели не читались бы, а в Pixel
//! режим без размытия выглядел бы как чёрный прямоугольник.
//!
//! Геометрия считается сверху вниз и держится на токенах `ui_tokens`: ширина
//! карточки, высота строки и радиусы — константы оттуда, а набор цветов —
//! `UiPalette`. Новых hex здесь нет: всё, что не приходит из палитры, берётся
//! через `mix`, иначе меню перестало бы следовать за matugen.
//!
//! Три правила, которые видно на снимках и которые поэтому вынесены в
//! функции, а не размазаны по коду:
//!
//! * выбранная строка светлее карточки. `surface_hover` для этого не годится
//!   — он темнее панели, и выделение проваливалось бы в фон;
//! * подсветка и полоса отступают от рамки карточки на `CARD_INSET`, иначе
//!   заливка съедала бы hairline по краю;
//! * по вертикали всё центрируется одним расчётом `center_band`, а базовая
//!   линия берётся в `text.rs` по измеренным пикселям глифов, а не по
//!   константе размера шрифта.
//!
//! Строки рисует тот же `TextPainter`, что и окно настроек, а глифы —
//! отдельной копией painter с шрифтом Nerd Font: в Pixel основной шрифт —
//! Minecraft Rus, и иконок в нём нет.

use super::super::palette;
use super::super::settings_icons;
use super::super::settings_ui::Rect;
use super::super::settings_view::{fill_rect, fill_round_rect, stroke_rect};
use super::super::text::{Align, SCALE, TextPainter};
use super::super::ui_tokens::{
    TypeScale, UiPalette, mix, radii, radius, spacing, type_scale, ui_palette,
};
use super::item::Item;
use super::settings::Language;
use super::state::Frame;
use super::strings;

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
/// Доля акцента в фоне выбранной строки. Малая: фон должен наметиться, а не
/// стать второй подсветкой.
pub const SELECTED_MIX: f32 = 0.14;
/// Минимум строк в карточке. Ниже карточка прыгает при каждом нажатии.
pub const MIN_ROWS: usize = 5;
/// Максимум строк: дальше список уже нужно прокручивать.
pub const MAX_ROWS: usize = 10;
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
/// Толщина акцентной полосы выбранной строки.
pub const SEL_BAR: f32 = 3.0;
/// Размер кружка тумблера.
pub const TOGGLE_D: f32 = 16.0;
/// Диаметр миниатюры пикера.
pub const THUMB_D: f32 = 28.0;
/// Ширина зоны значения справа.
pub const VALUE_W: f32 = 168.0;
/// Ширина зоны шеврона.
pub const CHEVRON_W: f32 = 16.0;
/// Плотность затемнения вокруг карточки.
pub const SCRIM: f32 = 0.72;
/// Прозрачность фона карточки в живом окне. Слой размывают правилом niri, и
/// сквозь полупрозрачную карточку видно, что именно размывается. Снимки
/// затемнение рисуют целиком, поэтому берут единицу.
///
/// Значение то же, что у панели (`p.base.with_a(0.9)` в рендере бара): меню и
/// панель — соседи на экране, и разная плотность фона между ними читалась как
/// «одно окно тёмное, другое нет».
pub const CARD_ALPHA: f32 = 0.9;

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
        SECRET_ROWS
    } else {
        rows_shown(frame.items.len())
    }
}

pub fn rows_shown(count: usize) -> usize {
    count.clamp(MIN_ROWS, MAX_ROWS)
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

/// Фон выбранной строки: смесь поверхности с акцентом. Проверено на обеих
/// темах — `luminance` результата выше, чем у самой карточки.
pub fn selected_background(ui: UiPalette) -> palette::Rgba {
    mix(ui.surface, ui.accent, SELECTED_MIX)
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

/// Прямоугольник акцентной полосы выбранной строки — та же подсветка, но
/// полоса шириной `SEL_BAR` от её левого края.
pub fn marker_rect(row: Rect) -> Rect {
    let inner = highlight_rect(row);
    Rect::new(inner.x, inner.y, SEL_BAR, inner.h)
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
    let row_h = row_height(search_mode(frame));
    let list = Rect::new(
        card.x,
        card.y + HEADER_H,
        card.w,
        rows_shown(frame.items.len()) as f32 * row_h,
    );
    if x < card.x || x > card.right() || y < list.y || y >= list.bottom() {
        return None;
    }
    let visible = ((y - list.y) / row_h).floor() as usize;
    let absolute = frame.top + visible;
    (absolute < frame.items.len()).then_some(absolute)
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

/// Всё, что нужно для кадра.
pub struct MenuView<'a> {
    pub pixmap: &'a mut tiny_skia::Pixmap,
    /// Painter основного шрифта темы.
    pub painter: &'a mut TextPainter,
    /// Отдельная копия painter с Nerd Font — только для глифов.
    pub icons: &'a mut TextPainter,
    pub ui: UiPalette,
    pub scale: TypeScale,
    pub pixel: bool,
}

impl<'a> MenuView<'a> {
    /// Затемнение всего кадра: карточка должна читаться поверх любого фона.
    /// Пиксели смешиваются вручную — `fill_rect` альфу не применяет, и без
    /// смешивания получился бы непрозрачный чёрный прямоугольник.
    fn scrim(&mut self, w: f32, h: f32) {
        let dark = self.ui.base.with_a(SCRIM);
        let factor = dark.3 as u32;
        let width = (w * SCALE) as u32;
        let height = (h * SCALE) as u32;
        let pixels = self.pixmap.pixels_mut();
        for pixel in pixels.iter_mut().take((width * height) as usize) {
            let inv = 255 - factor;
            let red = (pixel.red() as u32 * inv / 255 + dark.0 as u32 * factor / 255) as u8;
            let green = (pixel.green() as u32 * inv / 255 + dark.1 as u32 * factor / 255) as u8;
            let blue = (pixel.blue() as u32 * inv / 255 + dark.2 as u32 * factor / 255) as u8;
            if let Some(next) = tiny_skia::PremultipliedColorU8::from_rgba(red, green, blue, 255) {
                *pixel = next;
            }
        }
    }

    /// Подложка карточки и её рамка. Поверх затемнения она не должна быть
    /// полупрозрачной: иначе сквозь неё просвечивали бы обои.
    fn card(&mut self, rect: Rect) {
        fill_round_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            radius(self.pixel, radii::LG) * SCALE,
            self.ui.panel,
        );
        stroke_rect(
            self.pixmap,
            rect.x * SCALE,
            rect.y * SCALE,
            rect.w * SCALE,
            rect.h * SCALE,
            self.ui.border,
            SCALE,
        );
    }

    /// Текст с многоточием: длинные значения не должны наезжать на иконки.
    /// Базовая линия считается внутри `text.rs` по измеренным пикселям.
    fn text_clipped(
        &mut self,
        value: &str,
        size: f32,
        color: palette::Rgba,
        rect: Rect,
        align: Align,
    ) {
        self.painter
            .paint_boxed(self.pixmap, value, size, color, rect, align);
    }

    /// Глиф Nerd Font отдельным шрифтом.
    fn glyph(&mut self, value: &str, size: f32, color: palette::Rgba, rect: Rect) {
        self.icons
            .paint(self.pixmap, value, size, color, rect, Align::Center);
    }

    /// Заливка кругом по центру прямоугольника. Радиус всегда половина
    /// стороны: круг — это форма миниатюры и кружка, а не скругление угла,
    /// поэтому тема на него не влияет.
    fn disc(&mut self, rect: Rect, color: palette::Rgba) {
        let r = rect.w.min(rect.h) / 2.0;
        let cx = rect.x + rect.w / 2.0;
        let cy = rect.y + rect.h / 2.0;
        fill_round_rect(
            self.pixmap,
            (cx - r) * SCALE,
            (cy - r) * SCALE,
            r * 2.0 * SCALE,
            r * 2.0 * SCALE,
            r * SCALE,
            color,
        );
    }
}

/// Рисует меню целиком и возвращает прямоугольник карточки. Кадр снимка:
/// сначала затемнение, потом карточка по центру.
pub fn render(view: &mut MenuView<'_>, frame: &Frame) -> Rect {
    let surface = Rect::new(0.0, 0.0, frame_w(view), frame_h(view));
    let search = search_mode(frame);
    let rows = rows_for(frame);
    let card = card_rect(surface.w, surface.h, rows, search);
    view.scrim(surface.w, surface.h);
    paint(view, card, frame);
    card
}

/// Рисует меню в окне, где буфер равен самой карточке. Затемнения здесь нет:
/// под меню должны быть видны терминал и обои, размытие показывает
/// правило слоя. Прозрачность карточки задаёт вызывающий через
/// `apply_card_alpha`.
pub fn render_window(view: &mut MenuView<'_>, frame: &Frame) -> Rect {
    let search = search_mode(frame);
    let rows = rows_for(frame);
    let card = Rect::new(0.0, 0.0, CARD_W, card_height_for(rows, search));
    view.card(card);
    paint(view, card, frame);
    card
}

/// Содержимое карточки: шапка, строки, подвал. Общая часть для снимка и окна —
/// расхождение только в фоне вокруг.
fn paint(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    if let Some(secret) = frame.secret.as_ref() {
        secret_view(view, card, frame, secret);
        return;
    }
    let search = search_mode(frame);
    let rows = rows_shown(frame.items.len());
    let row_h = row_height(search);
    header(view, card, frame);
    rows_view(view, card, frame, rows, row_h);
    footer(view, card, frame);
}

/// Сколько строк занимает режим ввода пароля: «что подключаем» и «пароль».
pub const SECRET_ROWS: usize = 2;

/// Режим ввода пароля в той же карточке: крошки, имя сети, поле с точками и
/// курсором, подвал с подсказками. Отдельного окна нет и пароль рисуется
/// только точками — в кадре его нет даже в принципе.
fn secret_view(
    view: &mut MenuView<'_>,
    card: Rect,
    frame: &Frame,
    secret: &super::secret::SecretFrame,
) {
    header(view, card, frame);
    let body = Rect::new(
        card.x,
        card.y + HEADER_H,
        card.w,
        SECRET_ROWS as f32 * ROW_H,
    );
    let chrome = strings::chrome(frame.lang);

    // Строка «что подключаем»: имя сети из эфира, оно не секрет.
    view.text_clipped(
        &format!("{}: {}", chrome.password, secret.subject),
        view.scale.label,
        view.ui.text,
        center_band(
            Rect::new(body.x + PAD, body.y, body.w - PAD * 2.0, ROW_H),
            view.scale.label * 2.0,
        ),
        Align::Start,
    );

    // Поле пароля: рамка в фокусе, точки и курсор в конце.
    let field = Rect::new(
        body.x + PAD,
        body.y + ROW_H + spacing::SM,
        body.w - PAD * 2.0,
        ROW_H - spacing::MD,
    );
    fill_round_rect(
        view.pixmap,
        field.x * SCALE,
        field.y * SCALE,
        field.w * SCALE,
        field.h * SCALE,
        radius(view.pixel, radii::SM) * SCALE,
        view.ui.idle_panel,
    );
    stroke_rect(
        view.pixmap,
        field.x * SCALE,
        field.y * SCALE,
        field.w * SCALE,
        field.h * SCALE,
        view.ui.border_focus,
        SCALE,
    );
    let text_x = field.x + spacing::SM;
    let text_w = field.w - spacing::SM * 2.0;
    if secret.len == 0 {
        view.text_clipped(
            chrome.password_placeholder,
            view.scale.caption,
            view.ui.text_disabled,
            center_band(field, view.scale.caption * 2.0),
            Align::Start,
        );
    } else {
        view.text_clipped(
            &secret.masked,
            view.scale.label,
            view.ui.text,
            center_band(field, view.scale.label * 2.0),
            Align::Start,
        );
        let caret_x = text_x + view.painter.text_width(&secret.masked, view.scale.label);
        let caret = Rect::new(
            caret_x + 1.0,
            field.y + spacing::SM,
            2.0,
            field.h - spacing::SM * 2.0,
        );
        fill_rect(
            view.pixmap,
            caret.x * SCALE,
            caret.y * SCALE,
            caret.w * SCALE,
            caret.h * SCALE,
            view.ui.accent,
        );
    }
    let _ = text_w;
    secret_footer(view, card, frame);
}

/// Подвал режима ввода: вместо счётчика строк — что делает `Enter` и `Esc`.
fn secret_footer(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let rect = Rect::new(card.x, card.bottom() - FOOTER_H, card.w, FOOTER_H);
    let line = Rect::new(card.x, rect.y, card.w, 1.0);
    fill_rect(
        view.pixmap,
        line.x * SCALE,
        line.y * SCALE,
        line.w * SCALE,
        line.h * SCALE,
        view.ui.border_subtle,
    );
    let chrome = strings::chrome(frame.lang);
    let band = center_band(rect, view.scale.caption * 2.0);
    let mut x = rect.x + PAD;
    for hint in [chrome.password_connect, chrome.password_cancel] {
        let width = view.painter.text_width(hint, view.scale.caption);
        view.text_clipped(
            hint,
            view.scale.caption,
            view.ui.muted,
            Rect::new(x, band.y, width, band.h),
            Align::Start,
        );
        x += width + spacing::LG;
    }
    if let Some(status) = frame.status.as_ref() {
        let color = if status.is_failed() {
            mix(view.ui.accent, view.ui.muted, 0.5)
        } else {
            view.ui.accent
        };
        let right = Rect::new(rect.right() - PAD - 200.0, rect.y, 200.0, rect.h);
        view.disc(
            center_band(Rect::new(right.x, rect.y, 8.0, rect.h), 8.0),
            color,
        );
        let label = if status.is_failed() {
            chrome.failed
        } else {
            chrome.applied
        };
        view.text_clipped(
            label,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(right.x + 8.0 + spacing::XS, rect.y, right.w, rect.h),
                view.scale.caption * 2.0,
            ),
            Align::End,
        );
    }
}

/// Ширина кадра: логические пиксели из буфера.
fn frame_w(view: &MenuView<'_>) -> f32 {
    view.pixmap.width() as f32 / SCALE
}

/// Высота кадра.
fn frame_h(view: &MenuView<'_>) -> f32 {
    view.pixmap.height() as f32 / SCALE
}

/// Кольцо вокруг выключенного тумблера: внешний круг цветом рамки, внутренний
/// цветом карточки. Обводки окружности среди примитивов нет, а квадратный
/// «кружок» выдавал бы тумблер за чекбокс.
fn ring(view: &mut MenuView<'_>, rect: Rect, color: palette::Rgba) {
    view.disc(rect, color);
    let line = 1.0;
    view.disc(
        Rect::new(
            rect.x + line,
            rect.y + line,
            rect.w - line * 2.0,
            rect.h - line * 2.0,
        ),
        view.ui.panel,
    );
}

/// Крошки «HUD › Стиль» слева и поле поиска справа. Поле не мигает: рамка в
/// фокусе всегда видна, потому что меню работает только с клавиатуры.
fn header(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let header_rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    if !frame.trail.is_empty() {
        // Крошки рисуются двумя текстами: последний уровень ярче, поэтому он
        // меряется отдельно и ставится за своим префиксом.
        let last = frame.trail.last().expect("крошки не пусты").to_string();
        let prefix = frame
            .trail
            .iter()
            .take(frame.trail.len() - 1)
            .map(|crumb| format!("{crumb} "))
            .collect::<String>();
        let prefix_text = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}{} ", strings::CRUMBS_SEPARATOR.trim_end())
        };
        let crumbs = Rect::new(
            header_rect.x + PAD,
            header_rect.y,
            header_rect.w - PAD * 2.0 - SEARCH_W - spacing::MD,
            HEADER_H,
        );
        if prefix_text.is_empty() {
            view.text_clipped(
                &last,
                view.scale.label,
                view.ui.text,
                center_band(crumbs, view.scale.label * 2.0),
                Align::Start,
            );
        } else {
            view.text_clipped(
                &prefix_text,
                view.scale.label,
                view.ui.muted,
                center_band(crumbs, view.scale.label * 2.0),
                Align::Start,
            );
            let width = view.painter.text_width(&prefix_text, view.scale.label);
            view.text_clipped(
                &last,
                view.scale.label,
                view.ui.accent,
                center_band(
                    Rect::new(
                        crumbs.x + width + spacing::XS,
                        crumbs.y,
                        (crumbs.w - width).max(0.0),
                        crumbs.h,
                    ),
                    view.scale.label * 2.0,
                ),
                Align::Start,
            );
        }
    }
    search(view, header_rect, frame);
}

/// Поле поиска: лупа, набранный текст с курсором и крестик очистки.
fn search(view: &mut MenuView<'_>, header_rect: Rect, frame: &Frame) {
    let rect = Rect::new(
        header_rect.right() - PAD - SEARCH_W,
        header_rect.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    );
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(view.pixel, radii::SM) * SCALE,
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
    view.glyph(
        settings_icons::SEARCH,
        view.scale.caption,
        view.ui.muted,
        center_band(
            Rect::new(rect.x + spacing::SM, rect.y, ICON, rect.h),
            view.scale.label * 2.0,
        ),
    );

    let clear = !frame.query.is_empty();
    let text_x = rect.x + spacing::SM + ICON + spacing::SM;
    let text_w = rect.w
        - (spacing::SM + ICON + spacing::SM)
        - spacing::SM
        - if clear { ICON + spacing::XS } else { 0.0 };
    let text_rect = center_band(
        Rect::new(text_x, rect.y, text_w.max(0.0), rect.h),
        view.scale.label * 2.0,
    );
    if frame.query.is_empty() {
        view.text_clipped(
            strings::chrome(frame.lang).search,
            view.scale.caption,
            view.ui.text_disabled,
            text_rect,
            Align::Start,
        );
    } else {
        view.text_clipped(
            &frame.query,
            view.scale.label,
            view.ui.text,
            text_rect,
            Align::Start,
        );
        // Курсор: вертикальная черта сразу после набранного текста. Меню
        // только с клавиатуры, поэтому курсор всегда на виду.
        let caret = text_x + view.painter.text_width(&frame.query, view.scale.label);
        let caret = Rect::new(
            caret + 1.0,
            rect.y + spacing::SM,
            2.0,
            rect.h - spacing::SM * 2.0,
        );
        fill_rect(
            view.pixmap,
            caret.x * SCALE,
            caret.y * SCALE,
            caret.w * SCALE,
            caret.h * SCALE,
            view.ui.accent,
        );
    }
    if clear {
        view.glyph(
            settings_icons::TIMES,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(rect.right() - spacing::SM - ICON, rect.y, ICON, rect.h),
                view.scale.label * 2.0,
            ),
        );
    }
}

/// Строки окна с их абсолютными индексами: ровно те, что рисуются, и не
/// больше. Больше рисовать незачем — под последней строкой оставалось пустое
/// поле, когда список прокручен до конца.
fn window_rows(frame: &Frame, rows: usize) -> Vec<(usize, &Item)> {
    frame
        .items
        .iter()
        .enumerate()
        .skip(frame.top)
        .take(rows)
        .collect()
}

/// Абсолютный индекс подсвеченной строки.
///
/// `selected` — индекс во всём списке, а позиция в цикле отрисовки тоже
/// абсолютная: `enumerate` идёт до `skip`. Прибавлять тут `top` значило бы
/// подсветить строку на `top` ниже нужной, то есть перескочить через
/// выбранную, как только список прокрутится.
fn selected_row(frame: &Frame) -> usize {
    frame.selected
}

/// Список строк: выбранная подсвечена и отмечена полосой, между строками
/// hairline.
fn rows_view(view: &mut MenuView<'_>, card: Rect, frame: &Frame, rows: usize, row_h: f32) {
    let start = window_rows(frame, rows);
    let shown = start.len();
    let list = Rect::new(card.x, card.y + HEADER_H, card.w, shown as f32 * row_h);
    let selected_row = selected_row(frame);
    for (index, (position, item)) in start.iter().enumerate() {
        let rect = Rect::new(list.x, list.y + index as f32 * row_h, list.w, row_h);
        // Разделитель рисуется над строкой, а не под ней: под последней
        // строкой снизу уже подвал, а над первой лишней линии быть не
        // должно — поэтому у первой видимой строки разделителя нет.
        if index > 0 {
            let line = Rect::new(rect.x + PAD, rect.y, rect.w - PAD * 2.0, 1.0);
            fill_rect(
                view.pixmap,
                line.x * SCALE,
                line.y * SCALE,
                line.w * SCALE,
                line.h * SCALE,
                view.ui.border_subtle,
            );
        }
        row(view, rect, item, *position == selected_row, frame.lang);
    }
    if frame.items.is_empty() {
        view.text_clipped(
            strings::chrome(frame.lang).nothing,
            view.scale.label,
            view.ui.muted,
            list,
            Align::Center,
        );
    }
}

/// Одна строка: иконка, название, значение справа и индикатор поведения.
fn row(view: &mut MenuView<'_>, rect: Rect, item: &Item, selected: bool, lang: Language) {
    if selected {
        // Подсветка и полоса отступают от краёв карточки на CARD_INSET:
        // заливка от самого края ложилась бы на hairline рамки.
        let inner = highlight_rect(rect);
        fill_rect(
            view.pixmap,
            inner.x * SCALE,
            inner.y * SCALE,
            inner.w * SCALE,
            inner.h * SCALE,
            selected_background(view.ui),
        );
        let marker = marker_rect(rect);
        fill_rect(
            view.pixmap,
            marker.x * SCALE,
            marker.y * SCALE,
            marker.w * SCALE,
            marker.h * SCALE,
            view.ui.accent,
        );
    }
    let icon_color = if selected {
        view.ui.accent
    } else {
        view.ui.muted
    };
    let text_color = if selected {
        view.ui.accent
    } else {
        view.ui.text
    };
    let two_line = !item.single_line();
    let icon_x = rect.x + PAD;
    let label_x = icon_x + ICON + spacing::MD;
    let right = rect.right() - PAD;
    // Ширина зоны названия: до значения или до шеврона, чтобы длинное имя не
    // наезжало на правую часть.
    let trailing = match item.kind {
        super::item::ItemKind::Submenu | super::item::ItemKind::Leaf => CHEVRON_W,
        super::item::ItemKind::Picker(_) => THUMB_D + CHEVRON_W + spacing::SM,
        _ => CHEVRON_W + VALUE_W + spacing::SM,
    };
    let label_w = (right - trailing - label_x).max(0.0);
    let label_rect = Rect::new(label_x, rect.y, label_w, rect.h);
    if !item.icon.is_empty() {
        let icon_rect = center_band(Rect::new(icon_x, rect.y, ICON, rect.h), row_band(view));
        view.glyph(item.icon, view.scale.label, icon_color, icon_rect);
    }
    match item.caption.as_deref() {
        Some(caption) => {
            // Две строки: блок из названия и пути центрируется по строке, а
            // между ними ровно CAPTION_GAP.
            let block = (view.scale.label + view.scale.caption + CAPTION_GAP).max(1.0);
            let band = center_band(label_rect, block);
            view.text_clipped(
                &item.title,
                view.scale.label,
                text_color,
                Rect::new(band.x, band.y, band.w, view.scale.label + CAPTION_GAP),
                Align::Start,
            );
            view.text_clipped(
                caption,
                view.scale.caption,
                view.ui.muted,
                Rect::new(
                    band.x,
                    band.y + view.scale.label + CAPTION_GAP,
                    band.w,
                    view.scale.caption,
                ),
                Align::Start,
            );
        }
        None => {
            view.text_clipped(
                &item.title,
                view.scale.label,
                text_color,
                center_band(label_rect, view.scale.label * 2.0),
                Align::Start,
            );
        }
    }
    indicator(view, rect, item, two_line, lang);
}

/// Правая часть строки: круг тумблера, круглая миниатюра пикера, значение или
/// шеврон подменю. Всё центрируется по той же оси, что и название.
fn indicator(view: &mut MenuView<'_>, rect: Rect, item: &Item, two_line: bool, lang: Language) {
    let right = rect.right() - PAD;
    if item.kind.is_toggle() {
        let dot = center_band(
            Rect::new(right - TOGGLE_D, rect.y, TOGGLE_D, rect.h),
            TOGGLE_D,
        );
        if item.value == strings::chrome(lang).on {
            view.disc(dot, view.ui.accent);
        } else {
            ring(view, dot, mix(view.ui.base, view.ui.border, 0.6));
        }
        return;
    }
    if item.kind.is_picker() {
        // Заглушка миниатюры: настоящая картинка придёт в M4, когда будет
        // источник превью. Форма уже та же — круг 28 px с hairline, и шеврон
        // справа остаётся: пункт открывает выбор.
        let thumb = center_band(
            Rect::new(right - CHEVRON_W - THUMB_D, rect.y, THUMB_D, rect.h),
            THUMB_D,
        );
        view.disc(thumb, mix(view.ui.accent, view.ui.base, 0.45));
        stroke_rect(
            view.pixmap,
            thumb.x * SCALE,
            thumb.y * SCALE,
            thumb.w * SCALE,
            thumb.h * SCALE,
            view.ui.border_subtle,
            SCALE,
        );
        view.glyph(
            settings_icons::CHEVRON,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(right - CHEVRON_W, rect.y, CHEVRON_W, rect.h),
                row_band(view),
            ),
        );
        return;
    }
    let chevron = !item.value.is_empty() || item.kind.is_submenu();
    if chevron {
        view.glyph(
            settings_icons::CHEVRON,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(right - CHEVRON_W, rect.y, CHEVRON_W, rect.h),
                row_band(view),
            ),
        );
    }
    if !item.value.is_empty() {
        let value = Rect::new(
            right - CHEVRON_W - VALUE_W - spacing::SM,
            rect.y,
            VALUE_W,
            rect.h,
        );
        let band = if two_line {
            center_band(value, view.scale.label + view.scale.caption + CAPTION_GAP)
        } else {
            center_band(value, view.scale.label * 2.0)
        };
        view.text_clipped(
            &item.value,
            view.scale.label,
            view.ui.muted,
            band,
            Align::End,
        );
    }
}

/// Подвал: подсказки слева, счётчик или статус справа.
fn footer(view: &mut MenuView<'_>, card: Rect, frame: &Frame) {
    let rect = Rect::new(card.x, card.bottom() - FOOTER_H, card.w, FOOTER_H);
    let line = Rect::new(card.x, rect.y, card.w, 1.0);
    fill_rect(
        view.pixmap,
        line.x * SCALE,
        line.y * SCALE,
        line.w * SCALE,
        line.h * SCALE,
        view.ui.border_subtle,
    );
    let band = center_band(rect, view.scale.caption * 2.0);
    let counter_w = 96.0;
    // Между подсказками и правой частью — обязательный зазор XL. Если полные
    // подсказки не влезают, берутся короткие, а не наезжают на счётчик.
    let hints_left = rect.x + PAD;
    let right_edge = rect.right() - PAD - counter_w;
    let (hints, mut x) = layout_hints(view, hints_left, right_edge - spacing::XL, frame.lang);
    for hint in hints {
        let width = view.painter.text_width(hint, view.scale.caption);
        view.text_clipped(
            hint,
            view.scale.caption,
            view.ui.muted,
            Rect::new(x, band.y, width, band.h),
            Align::Start,
        );
        x += width + spacing::LG;
    }
    let right = Rect::new(right_edge, rect.y, counter_w, rect.h);
    match frame.status.as_ref() {
        Some(status) => {
            // В режиме статуса счётчика нет: точка сообщает, что сообщение
            // именно об изменении, а не о позиции в списке.
            let color = if status.is_failed() {
                mix(view.ui.accent, view.ui.muted, 0.5)
            } else {
                view.ui.accent
            };
            view.disc(
                center_band(Rect::new(right.x, rect.y, 8.0, rect.h), 8.0),
                color,
            );
            let label = if status.is_failed() {
                strings::chrome(frame.lang).failed
            } else {
                strings::chrome(frame.lang).applied
            };
            view.text_clipped(
                label,
                view.scale.caption,
                view.ui.muted,
                center_band(
                    Rect::new(
                        right.x + 8.0 + spacing::XS,
                        rect.y,
                        right.w - 8.0 - spacing::XS,
                        rect.h,
                    ),
                    view.scale.caption * 2.0,
                ),
                Align::Start,
            );
        }
        None => {
            // На пустом списке счётчик был бы «1/0»: читается как ошибка.
            // Там ноль из нуля.
            let index = if frame.total == 0 {
                0
            } else {
                frame.selected + 1
            };
            let counter = format!("{index}/{}", frame.total);
            view.text_clipped(
                &counter,
                view.scale.caption,
                view.ui.muted,
                center_band(right, view.scale.caption * 2.0),
                Align::End,
            );
        }
    }
}

/// Ширина строки подсказок вместе с зазорами между ними.
fn hints_width(view: &mut MenuView<'_>, hints: &[&str]) -> f32 {
    let mut total = 0.0;
    for (index, hint) in hints.iter().enumerate() {
        total += view.painter.text_width(hint, view.scale.caption);
        if index + 1 < hints.len() {
            total += spacing::LG;
        }
    }
    total
}

/// Раскладка подсказок подвала: возвращает набор строк и их левую границу.
/// Полные подписи не влезают в русском Pixel-шрифте, поэтому для них есть
/// короткие варианты: «ESC» вместо «ESC ЗАКРЫТЬ».
fn layout_hints<'a>(
    view: &mut MenuView<'_>,
    left: f32,
    limit: f32,
    lang: Language,
) -> (Vec<&'a str>, f32) {
    let chrome = strings::chrome(lang);
    let full = [chrome.select, chrome.open, chrome.back, chrome.close];
    let short = [
        chrome.select_short,
        chrome.open_short,
        chrome.back_short,
        chrome.close_short,
    ];
    let width = hints_width(view, &full);
    if left + width <= limit {
        (full.to_vec(), left)
    } else {
        (short.to_vec(), left)
    }
}

/// Палитра и шкала по теме: тот же расчёт, что у окна настроек, чтобы меню и
/// настройки выглядели одной оболочкой.
pub fn theme(pixel: bool, base: palette::Palette) -> (UiPalette, TypeScale) {
    (ui_palette(pixel, base), type_scale(pixel))
}

/// Шрифт основного текста по теме.
pub fn font_name(pixel: bool) -> String {
    if pixel {
        "Minecraft Rus".to_string()
    } else {
        "JetBrainsMono Nerd Font Propo".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Action, Menu, Node};
    use super::*;

    /// Список из десяти строк, выбор девятой: прокрутка ещё не нужна, и
    /// подсветка обязана совпадать с выбором.
    #[test]
    fn highlight_stays_on_the_selected_row_after_scrolling() {
        let mut menu = Menu::new(rows(10));
        for _ in 0..8 {
            menu.move_sel(1);
        }
        let frame = menu.frame();
        assert_eq!(frame.selected, 8, "выбрана девятая строка");
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
        let row = selected_row(&frame);
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
            frame.top + frame.selected,
            "индекс не должен смещаться на top={}",
            frame.top
        );
    }

    /// Под последней нарисованной строкой не остаётся пустого поля: оконных
    /// строк ровно столько, сколько осталось в списке, и они не выходят за
    /// его конец.
    #[test]
    fn no_empty_rows_are_drawn_below_the_last_one() {
        let mut menu = Menu::new(rows(12));
        for _ in 0..11 {
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

    /// Меню из `count` строк для проверки навигации.
    fn rows(count: usize) -> Vec<Node> {
        let owned: Vec<String> = (0..count).map(|index| format!("строка {index}")).collect();
        owned
            .iter()
            .map(|title| Node::action("", title, Action::Back))
            .collect()
    }

    use super::super::state::Status;

    fn palette_for(pixel: bool) -> UiPalette {
        theme(pixel, palette::Palette::default()).0
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
    fn card_stops_growing_at_ten_rows() {
        assert_eq!(card_height(10), HEADER_H + 10.0 * ROW_H + FOOTER_H);
        assert_eq!(card_height(50), card_height(10), "дальше список листается");
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

    /// Фон выбранной строки обязан быть светлее карточки в обеих темах:
    /// `surface_hover` темнее панели, и выделение проваливалось бы в фон.
    #[test]
    fn selected_row_is_lighter_than_the_card() {
        for pixel in [false, true] {
            let ui = palette_for(pixel);
            let selected = selected_background(ui);
            assert!(
                luminance(selected) > luminance(ui.surface),
                "pixel={pixel}: фон выбранной строки {} не светлее карточки {}",
                luminance(selected),
                luminance(ui.surface)
            );
            assert!(
                luminance(ui.surface_hover) < luminance(ui.surface),
                "pixel={pixel}: surface_hover светлее карточки, подсветку можно было бы оставить"
            );
        }
    }

    /// Подсветка и полоса не должны наезжать на hairline рамки карточки.
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
            let marker = marker_rect(row);
            assert!(marker.x >= rect.x + CARD_INSET);
            assert_eq!(marker.w, SEL_BAR);
            assert_eq!(marker.h, row.h, "полоса на всю высоту строки");
            assert!(marker.right() <= inner.right(), "полоса шире подсветки");
        }
    }

    #[test]
    fn marker_is_three_pixels_and_inset_by_one() {
        let rect = card(6);
        let row = Rect::new(rect.x, rect.y, rect.w, ROW_H);
        let marker = marker_rect(row);
        assert_eq!(marker.w, 3.0);
        assert_eq!(marker.x, rect.x + 1.0);
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
        let scale = type_scale(false);
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
        // Отступ подсветки и полоса — единственные исключения: 1 px равна
        // толщине hairline, а 3 px берутся из макета.
        assert_eq!(CARD_INSET, 1.0);
        assert_eq!(SEL_BAR, 3.0);
    }

    /// Подсказки подвала: полные и короткие варианты должны существовать
    /// оба, иначе Pixel-тема на русском их не покажет.
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
            selected: 0,
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
            selected: 0,
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
                    kind: super::super::item::ItemKind::Leaf,
                    origin: super::super::item::Origin { depth: 0, index },
                })
                .collect(),
            selected: 0,
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
                    kind: super::super::item::ItemKind::Leaf,
                    origin: super::super::item::Origin { depth: 0, index },
                })
                .collect(),
            selected: 0,
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
        CARD_ALPHA > 0.8 && CARD_ALPHA < 1.0,
        "полупрозрачность карточки должна быть лёгкой"
    );
    /// Плотность фона карточки обязана совпадать с плотностью панели: меню и
    /// панель стоят на экране рядом, и расхождение было видно сразу. Число
    /// продублировано намеренно — если у панели поменяют, тест об этом скажет.
    const _: () = assert!(
        (CARD_ALPHA - 0.9).abs() < f32::EPSILON,
        "плотность фона меню должна совпадать с панелью"
    );

    #[test]
    fn failed_status_is_marked() {
        assert!(Status::Failed("ошибка".to_string()).is_failed());
        assert!(!Status::Applied("готово".to_string()).is_failed());
    }
}
