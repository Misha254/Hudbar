//! Живая статус-карточка промпта: один виджет на всё время работы.
//!
//! Карточка открывается сразу после принятия промпта и правится на месте,
//! пока агент работает: строка состояния с прошедшим временем, а во время
//! работы — текущий инструмент и его цель. Ответ агента карточку не правит:
//! он уходит новыми сообщениями, чтобы телефон дал push. Финал (готово,
//! ошибка, остановка) правит карточку в последний раз и снимает клавиатуру.
//!
//! Всё здесь — чистые функции без сети: рендер проверяется тестами, а
//! отправкой и троттлингом занимается мост.

use super::api::escape_html;

/// Состояние карточки. Состояний всего пять специально: карточка —
/// индикатор, а не лог, и мелькание десятка состояний читается хуже.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardStatus {
    /// Агент работает.
    Running,
    /// Агент ждёт разрешения.
    Waiting,
    /// Агент закончил.
    Done,
    /// Агент упал с ошибкой.
    Error,
    /// Работу прервали.
    Stopped,
}

/// Что нарисовать: подпись сессии, состояние, прошедшее время, текущий
/// инструмент и текст ошибки для финала.
pub struct CardView<'a> {
    /// Подпись сессии: короткое название или короткий id.
    pub session_label: &'a str,
    /// Состояние.
    pub status: CardStatus,
    /// Прошедших секунд с начала промпта.
    pub elapsed_secs: u64,
    /// Текущий инструмент и его цель: название и строка цели.
    pub tool: Option<(&'a str, &'a str)>,
    /// Короткая причина для состояния ошибки.
    pub error: Option<&'a str>,
}

/// Верхняя строка карточки: `🛠 <b>hudbar</b> · подпись`.
pub fn render(view: &CardView<'_>) -> String {
    let mut text = format!("🛠 <b>hudbar</b> · {}", escape_html(view.session_label));
    text.push_str(&format!("\n{}", status_line(view)));
    if view.status == CardStatus::Running
        && let Some((tool, target)) = view.tool
    {
        text.push_str(&format!("\n{}", tool_line(tool, target)));
    }
    if view.status == CardStatus::Error
        && let Some(error) = view.error
    {
        let error = one_line(error, 120);
        if !error.is_empty() {
            text.push_str(&format!("\n{}", escape_html(&error)));
        }
    }
    text
}

/// Вторая строка: состояние с эмодзи и прошедшим временем. У ожидания времени
/// нет: оно про человека, а не про работу.
fn status_line(view: &CardView<'_>) -> String {
    match view.status {
        CardStatus::Running => {
            format!("⏳ В работе · {}", format_elapsed(view.elapsed_secs))
        }
        CardStatus::Waiting => "⏸ Ждёт разрешения".to_string(),
        CardStatus::Done => format!("✅ Готово · {}", format_elapsed(view.elapsed_secs)),
        CardStatus::Error => "❌ Ошибка".to_string(),
        CardStatus::Stopped => "⏹ Остановлено".to_string(),
    }
}

/// Третья строка: инструмент и цель в одну строку, не длиннее 80 символов.
fn tool_line(tool: &str, target: &str) -> String {
    let tool = tool.trim();
    let budget = 80usize.saturating_sub(tool.chars().count() + 3);
    let target = one_line(target, budget);
    if target.is_empty() {
        escape_html(tool)
    } else {
        format!("{} · {}", escape_html(tool), escape_html(&target))
    }
}

/// Прошедшее время словами: «35 с», «2 мин 14 с», «4 мин», «1 ч 5 мин».
/// Нулевые хвосты опускаются: «2 мин», а не «2 мин 0 с».
pub fn format_elapsed(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = total_secs % 3600 / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        if minutes > 0 {
            format!("{hours} ч {minutes} мин")
        } else {
            format!("{hours} ч")
        }
    } else if minutes > 0 {
        if seconds > 0 {
            format!("{minutes} мин {seconds} с")
        } else {
            format!("{minutes} мин")
        }
    } else {
        format!("{seconds} с")
    }
}

/// Одна строка не длиннее `max_chars` символов: переносы сворачиваются в
/// пробелы, лишнее режется. Пустой предел даёт пустую строку, а не панику.
pub fn one_line(text: &str, max_chars: usize) -> String {
    let single: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= max_chars {
        return single;
    }
    single.chars().take(max_chars).collect()
}

/// `callback_data` кнопки «Стоп»: `stop:<id сессии>`. Полный id, а не
/// короткий: точное совпадение вместо поиска по префиксу.
pub fn stop_callback(session_id: &str) -> String {
    format!("stop:{session_id}")
}

/// Разбор нажатия «Стоп»: возвращает id сессии или `None` для чужих кнопок.
pub fn parse_stop_callback(data: &str) -> Option<&str> {
    let id = data.strip_prefix("stop:")?;
    if id.is_empty() { None } else { Some(id) }
}

/// Длина `callback_data` в байтах: лимит Bot API — 64.
pub fn callback_len(data: &str) -> usize {
    data.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> CardView<'static> {
        CardView {
            session_label: "Мост",
            status: CardStatus::Running,
            elapsed_secs: 134,
            tool: Some(("bash", "cargo test")),
            error: None,
        }
    }

    #[test]
    fn card_has_three_lines() {
        let text = render(&running());
        assert!(text.contains("🛠 <b>hudbar</b> · Мост"), "шапка: {text}");
        assert!(text.contains("⏳ В работе · 2 мин 14 с"), "статус: {text}");
        assert!(text.contains("bash · cargo test"), "инструмент: {text}");
    }

    #[test]
    fn waiting_has_no_elapsed_and_no_tool() {
        let text = render(&CardView {
            status: CardStatus::Waiting,
            tool: Some(("bash", "rm -rf /")),
            ..running()
        });
        assert!(text.contains("⏸ Ждёт разрешения"), "статус: {text}");
        assert!(
            !text.contains("rm -rf"),
            "цель не светится в ожидании: {text}"
        );
    }

    #[test]
    fn finals_are_compact() {
        let done = render(&CardView {
            status: CardStatus::Done,
            elapsed_secs: 240,
            tool: None,
            ..running()
        });
        assert!(done.contains("✅ Готово · 4 мин"), "финал: {done}");
        let stopped = render(&CardView {
            status: CardStatus::Stopped,
            ..running()
        });
        assert!(stopped.contains("⏹ Остановлено"), "стоп: {stopped}");
        assert!(!stopped.contains("bash"), "без инструмента: {stopped}");
    }

    #[test]
    fn titles_and_targets_are_escaped() {
        let text = render(&CardView {
            session_label: "a & b <c>",
            tool: Some(("edit", "x > y")),
            ..running()
        });
        assert!(text.contains("a &amp; b &lt;c&gt;"), "подпись: {text}");
        assert!(text.contains("x &gt; y"), "цель: {text}");
        assert!(!text.contains("a & b"), "сырого не осталось: {text}");
    }

    #[test]
    fn tool_line_is_truncated_to_one_line() {
        let long = format!("one\ntwo {}", "z".repeat(200));
        let line = tool_line("bash", &long);
        assert!(!line.contains('\n'), "одна строка: {line:?}");
        assert!(line.chars().count() <= 80, "влезает: {line:?}");
        assert!(tool_line("bash", "   ").contains("bash"), "пустая цель");
    }

    #[test]
    fn elapsed_formats_without_zero_tails() {
        assert_eq!(format_elapsed(0), "0 с");
        assert_eq!(format_elapsed(35), "35 с");
        assert_eq!(format_elapsed(60), "1 мин");
        assert_eq!(format_elapsed(134), "2 мин 14 с");
        assert_eq!(format_elapsed(240), "4 мин");
        assert_eq!(format_elapsed(3900), "1 ч 5 мин");
        assert_eq!(format_elapsed(7200), "2 ч");
    }

    #[test]
    fn one_line_collapses_whitespace() {
        assert_eq!(one_line("  a\n\nb\tc  ", 100), "a b c");
        assert_eq!(one_line("abcdef", 4), "abcd");
        assert_eq!(one_line("x", 0), "");
    }

    #[test]
    fn stop_callback_fits_and_parses() {
        let data = stop_callback("ses_abcdef1234567890");
        assert!(callback_len(&data) <= 64, "влезает в лимит: {data}");
        assert_eq!(parse_stop_callback(&data), Some("ses_abcdef1234567890"));
        assert!(parse_stop_callback("perm:abc:once").is_none());
        assert!(parse_stop_callback("stop:").is_none());
    }
}
