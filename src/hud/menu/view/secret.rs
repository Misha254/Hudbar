use super::super::canvas::{fill_rect, fill_round_rect, stroke_rect};
use super::super::geom::Rect;
use super::super::secret::SecretFrame;
use super::super::state::Frame;
use super::super::strings;
use super::super::text::{Align, SCALE};
use super::super::ui_tokens::{mix, radii, radius, spacing};
use super::MenuView;
use super::footer::right_zone_width;
use super::geometry::{FOOTER_H, HEADER_H, PAD, ROW_H, STATUS_DOT, center_band};
use super::header::header;

/// Сколько строк занимает режим ввода пароля: «что подключаем» и «пароль».
pub const SECRET_ROWS: usize = 2;

/// Режим ввода пароля в той же карточке: крошки, имя сети, поле с точками и
/// курсором, подвал с подсказками. Отдельного окна нет и пароль рисуется
/// только точками — в кадре его нет даже в принципе.
pub(crate) fn secret_view(
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
        let right_zone = right_zone_width(view, frame.lang);
        let right = Rect::new(rect.right() - PAD - right_zone, rect.y, right_zone, rect.h);
        view.disc(
            center_band(Rect::new(right.x, rect.y, STATUS_DOT, rect.h), STATUS_DOT),
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
                Rect::new(right.x + STATUS_DOT + spacing::XS, rect.y, right.w, rect.h),
                view.scale.caption * 2.0,
            ),
            Align::End,
        );
    }
}
