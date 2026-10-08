// Тексты и форматирование тел уведомлений.
//
// Все видимые строки лежат здесь одной таблицей, чтобы потом можно было
// подставить i18n, не трогая логику. Файл сознательно без импортов: его
// можно грузить откуда угодно.

/**
 * Заголовок = тип события. Тело = заголовок сессии.
 * Русские значения.
 */
export const STRINGS = {
  titles: {
    done: "Готово",
    permission: "Нужно разрешение",
    question: "Агент задал вопрос",
    error: "Ошибка",
  },
  subagentSuffix: " (субагент)",
  untitled: "(без названия)",
  errorWithoutSession: "Сессия неизвестна",
  fallbackQuestion: "Без текста вопроса",
};

/** Максимальная длина заголовка сессии в теле уведомления. */
export const TITLE_MAX = 80;

/**
 * Экранирование разметки dunst.
 *
 * dunst разбирает HTML-подобную разметку, и его `format = "<b>%s</b>\n%b"`
 * обрамляет заголовок в `<b>`. Заголовок сессии приходит из подсказок
 * opencode и может содержать что угодно, включая `<` и `&`. Без экранирования
 * получится либо битый вывод, либо — с тегом — подмена оформления.
 *
 * Порядок важен: сначала амперсанд, потом угловые скобки. Иначе `&lt;`
 * превратится в `&amp;lt;`.
 *
 * @param {unknown} text
 * @returns {string}
 */
export function escapeMarkup(text) {
  if (text === null || text === undefined) return "";
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

/**
 * Усечение по символам без разрыва суррогатных пар.
 * @param {string} text
 * @param {number} [max]
 * @returns {string}
 */
export function truncate(text, max = TITLE_MAX) {
  const chars = Array.from(text);
  if (chars.length <= max) return text;
  return chars.slice(0, Math.max(0, max - 1)).join("") + "…";
}

/**
 * Тело уведомления из заголовка сессии: усечь и экранировать.
 * @param {unknown} sessionTitle
 * @returns {string}
 */
export function formatSessionTitle(sessionTitle) {
  const raw =
    sessionTitle === null || sessionTitle === undefined
      ? ""
      : String(sessionTitle).trim();
  if (!raw) return escapeMarkup(STRINGS.untitled);
  return escapeMarkup(truncate(raw, TITLE_MAX));
}

/**
 * Тело из текста вопроса агента: обрезаем заметно сильнее, вопросы бывают
 * многострочными и длинными.
 * @param {unknown} text
 * @returns {string}
 */
export function formatQuestionText(text) {
  const raw =
    text === null || text === undefined ? "" : String(text).trim();
  if (!raw) return escapeMarkup(STRINGS.fallbackQuestion);
  return escapeMarkup(truncate(raw.replace(/\s+/g, " "), TITLE_MAX));
}

/**
 * Заголовок уведомления для kind.
 * @param {string} kind
 * @returns {string}
 */
export function titleFor(kind) {
  const found = STRINGS.titles[kind];
  return found ?? STRINGS.titles.done;
}

/**
 * Пометка субагента в теле.
 * @param {string} body уже готовое тело
 * @param {boolean} isSubagent
 * @returns {string}
 */
export function markSubagent(body, isSubagent) {
  if (!isSubagent) return body;
  return body === STRINGS.untitled || body === STRINGS.fallbackQuestion
    ? STRINGS.subagentSuffix
    : body + STRINGS.subagentSuffix;
}

/**
 * Тег для группировки уведомлений dunst (x-dunst-stack-tag).
 * @param {string} sessionID
 * @param {string} kind
 * @returns {string}
 */
export function stackTag(sessionID, kind) {
  return `opencode-${sessionID}-${kind}`;
}