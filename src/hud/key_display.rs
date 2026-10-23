//! Показ и колонка клавиши бинда: срезаем жаргон аппаратных кодов «XF86»
//! и считаем ширину колонки клавиш по самой длинной строке списка.
//! Общее для обоих окон биндов — раньше эти хелперы жили копиями в
//! `hud_keybinds.rs`, а в yazi-окне ширина считалась inline (KB2.1).

use super::bind_data::Entry;
use super::text::TextPainter;

/// Показ имени клавиши: префикс «XF86» — жаргон аппаратных кодов ядра,
/// в интерфейсе он только шумит (XF86AudioRaiseVolume → AudioRaiseVolume).
/// Только отображение: ключи в bind_data и поиск по ним не меняются.
pub fn display_key(key: &str) -> &str {
    key.strip_prefix("XF86").unwrap_or(key)
}

/// Ширина колонки клавиш: по самой длинной клавише списка, но не больше
/// 45% ширины карточки — описаниям остаётся место. Клавиши шире потолка
/// честно усекаются painter'ом, «…» остаётся запасом. Срезание «XF86»
/// тут же: yazi-ключи префикса не имеют, срез для них нейтрален.
pub fn key_column_width(items: &[Entry], painter: &mut TextPainter, size: f32, card_w: f32) -> f32 {
    items
        .iter()
        .map(|e| painter.text_width(display_key(&e.key), size))
        .fold(0.0f32, f32::max)
        .min(card_w * 0.45)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// «XF86» срезается только на отображении: прочие клавиши не тронуты,
    /// данные в bind_data остаются как есть.
    #[test]
    fn display_key_strips_only_the_xf86_prefix() {
        assert_eq!(display_key("XF86AudioRaiseVolume"), "AudioRaiseVolume");
        assert_eq!(display_key("XF86ScreenSaver"), "ScreenSaver");
        assert_eq!(display_key("Super+T"), "Super+T");
        assert_eq!(display_key("KP_8"), "KP_8");
    }

    /// Колонка берёт максимум ширины клавиш списка и упирается в потолок
    /// 45% карточки: широкий список из одной длинной клавиши усёкся до
    /// потолка, узкий — ровно по самой длинной клавише.
    #[test]
    fn key_column_takes_longest_and_respects_cap() {
        let wide = [
            Entry::new("XF86AudioRaiseVolume".into(), String::new()),
            Entry::new("a".into(), String::new()),
        ];
        let mut painter = TextPainter::new("JetBrainsMono Nerd Font Propo");
        let raw = painter.text_width("AudioRaiseVolume", 14.0);
        let cap = 400.0 * 0.45;
        // Потолок ниже ширины самой длинной клавиши — колонка усечена.
        assert_eq!(
            key_column_width(&wide, &mut painter, 14.0, 400.0),
            raw.min(cap)
        );
        let short = [Entry::new("g".into(), String::new())];
        assert_eq!(
            key_column_width(&short, &mut painter, 14.0, 400.0),
            painter.text_width("g", 14.0)
        );
    }
}
