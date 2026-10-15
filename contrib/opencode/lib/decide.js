// Таблица решений: что делать с очередным событием.
//
// Решение принимает чистая функция без сети и без /proc. Всё, что нужно для
// решения, передаётся аргументами: память busy-сессий, множество показанных
// разрешений, троттлинг и признак субагента (который добывается вызывающий —
// там нужен сетевой запрос).
//
// Решение намеренно НЕ включает проверку фокуса и сам вызов notify-send:
// это побочные эффекты, их делает точка входа.
//
// События v2. Сервер отдаёт `{ type, data }`:
//   session.execution.started      сессия начала работать
//   session.execution.succeeded    работа закончилась успешно
//   session.execution.interrupted  работа прервана
//   session.execution.failed       ошибка внутри исполнения
//   session.status / session.idle legacy-пара, они тоже эмитятся
//   permission.asked/replied       id разрешения лежит в `id` / `requestID`
//   form.created/replied/cancelled вопрос агента — это теперь «форма»

import { PermissionMemory, Throttle } from "./throttle.js";
import {
  STRINGS,
  formatQuestionText,
  formatSessionTitle,
  markSubagent,
  titleFor,
} from "./strings.js";

/**
 * Ответ на событие.
 * @typedef {{ notify: false, reason: string }
 *   | { notify: true, title: string, body: string, urgency: "low"|"normal"|"critical", timeoutMs: number, kind: string, sessionID: string, tag: string }
 * } Decision
 */

/** События, которыми сессия помечается работающей. */
const BUSY_TYPES = new Set(["session.execution.started"]);

/** События, которыми работа сессии закончилась. */
const DONE_TYPES = new Set([
  "session.idle",
  "session.execution.succeeded",
  "session.execution.interrupted",
]);

/** События об ошибке. */
const ERROR_TYPES = new Set(["session.error", "session.execution.failed"]);

/**
 * Сколько живёт уведомление на экране, мс.
 *
 * Критичные живут дольше: на них нужно успеть ответить. Без явного `-t` dunst
 * с незаданным в dunstrc `timeout` не гасит critical вообще — висят, пока не
 * закрыть руками (проверено на машине пользователя).
 */
const TIMEOUT_MS = { low: 8000, normal: 10000, critical: 20000 };

/** Помечаем сессию работающей. */
export function markBusy(busySessions, sessionID) {
  if (sessionID) busySessions.add(sessionID);
}

/** Снимаем пометку: сессия закончила работу. */
export function clearBusy(busySessions, sessionID) {
  busySessions.delete(sessionID);
}

/** Видели ли мы, что сессия работала. */
export function wasBusy(busySessions, sessionID) {
  return busySessions.has(sessionID);
}

/**
 * Полезная нагрузка события.
 *
 * В v2 это `data`. Старые сборки клали в `properties` — берём первое
 * непустое, чтобы смена формы события не ломала плагин.
 *
 * @param {any} event
 * @returns {any}
 */
export function payloadOf(event) {
  if (!event || typeof event !== "object") return {};
  if (event.data && typeof event.data === "object") return event.data;
  if (event.properties && typeof event.properties === "object") return event.properties;
  return {};
}

/**
 * sessionID из события.
 *
 * У `form.created` сессия лежит внутри `data.form`, у `permission.*` ключ
 * `id` — это id разрешения, а не сессии, поэтому там берём только `sessionID`.
 *
 * @param {any} event
 * @param {any} [payload]
 * @returns {string}
 */
export function sessionIdOf(event, payload = payloadOf(event)) {
  const type = typeof event?.type === "string" ? event.type : "";
  const data = payload && typeof payload === "object" ? payload : {};
  if (type === "form.created") return data.form?.sessionID ?? "";
  if (type.startsWith("permission.")) return data.sessionID ?? "";
  return data.sessionID ?? data.id ?? "";
}

/**
 * Правда ли, что тип события — вопрос агента.
 *
 * Формами в v2 называется всё, что требует ответа: вопрос агента, запрос MCP
 * на ввод и выбор провайдера веб-поиска. Их заголовки — «Questions»,
 * «Web Search», «<server> is requesting input», поэтому по одному `form.created`
 * вопрос не отличить: приходит «Агент задал вопрос / Web Search», хотя
 * вопроса никто не задавал.
 *
 * Различает вид формы `data.form.metadata.kind`: у вопроса он `question`.
 * Строковая проверка `question*.asked` оставлена для сборок, где вопрос ещё
 * отдельным событием.
 *
 * @param {string} type
 * @param {any} [payload]
 * @returns {boolean}
 */
export function isQuestionEvent(type, payload) {
  if (typeof type !== "string") return false;
  if (type === "form.created") {
    const form = payload?.form;
    if (!form || typeof form !== "object") return false;
    const kind = form.metadata?.kind;
    if (kind !== undefined) return kind === "question";
    // Формы без metadata в 2.0.28 не бывает; на случай другой сборки считаем
    // вопросом только форму с полями.
    return Array.isArray(form.fields) && form.fields.length > 0;
  }
  return type.startsWith("question") && type.endsWith(".asked");
}

/**
 * Достать текст первого вопроса из payload.
 *
 * У формы заголовок — служебный («Questions»), текст лежит в полях:
 * `description` это сам вопрос, `title` — короткий заголовок вопроса.
 * Порядок именно такой: description полезнее.
 *
 * @param {any} payload
 * @returns {unknown}
 */
export function firstQuestionText(payload) {
  if (!payload || typeof payload !== "object") return "";
  const form = payload.form;
  if (form && typeof form === "object") {
    const fields = Array.isArray(form.fields) ? form.fields : [];
    for (const field of fields) {
      if (!field || typeof field !== "object") continue;
      const text = field.description ?? field.title;
      if (typeof text === "string" && text.trim()) return text;
    }
    // Заголовок формы намеренно НЕ используется: у вопросов он служебный
    // («Questions»), а у чужих форм вроде выбора провайдера — «Web Search».
    return "";
  }
  const questions = payload.questions;
  if (Array.isArray(questions) && questions.length > 0) {
    const first = questions[0];
    if (first && typeof first === "object") {
      const text = first.question ?? first.header;
      if (text) return text;
    }
    if (typeof first === "string") return first;
  }
  return payload.question ?? payload.header ?? "";
}

/**
 * Текст разрешения: в v2 это `message`, в старых сборках был `title`.
 * @param {any} payload
 * @returns {unknown}
 */
function permissionText(payload) {
  return payload?.message ?? payload?.title ?? "";
}

/**
 * Тело уведомления для события.
 * @param {any} event
 * @param {any} payload
 * @param {boolean} isSubagent
 * @param {any} sessionTitle
 * @returns {string}
 */
function bodyFor(event, payload, isSubagent, sessionTitle) {
  if (isQuestionEvent(event.type, payload)) {
    // Текста вопроса может не оказаться (пустые поля) — тогда полезнее
    // заголовок сессии, чем служебное «Questions».
    const text = firstQuestionText(payload);
    return markSubagent(
      text ? formatQuestionText(text) : formatSessionTitle(sessionTitle),
      isSubagent,
    );
  }
  if (ERROR_TYPES.has(event.type) && !sessionIdOf(event, payload)) {
    return markSubagent(STRINGS.errorWithoutSession, isSubagent);
  }
  return markSubagent(formatSessionTitle(sessionTitle), isSubagent);
}

/**
 * Основное решение по событию.
 *
 * @param {object} input
 * @param {any} input.event
 * @param {boolean} input.isSubagent субагент ли сессия
 * @param {any} [input.sessionTitle] заголовок сессии
 * @param {Set<string>} input.busySessions
 * @param {import("./throttle.js").PermissionMemory} input.permissions
 * @param {import("./throttle.js").PermissionMemory} [input.forms] показанные формы
 * @param {Throttle} input.throttle
 * @returns {Decision}
 */
export function decide({
  event,
  isSubagent,
  sessionTitle,
  busySessions,
  permissions,
  forms = new PermissionMemory(),
  throttle,
}) {
  const type = event?.type;
  const payload = payloadOf(event);
  const sessionID = sessionIdOf(event, payload);

  const reject = (reason) => ({ notify: false, reason });

  if (!type) return reject("no-type");

  // --- начало работы: только пометка, уведомления нет ------------------------
  if (BUSY_TYPES.has(type)) {
    if (!sessionID) return reject("no-session-id");
    markBusy(busySessions, sessionID);
    return reject("marked-busy");
  }

  // --- session.status: та же пометка, для сборок, где эмитится она ----------
  if (type === "session.status") {
    if (!sessionID) return reject("no-session-id");
    if (payload.status?.type === "busy") {
      markBusy(busySessions, sessionID);
      return reject("marked-busy");
    }
    return reject("status-not-busy");
  }

  // --- конец работы: только если реально работали ---------------------------
  if (DONE_TYPES.has(type)) {
    if (!sessionID) return reject("no-session-id");
    if (isSubagent) {
      clearBusy(busySessions, sessionID);
      return reject("subagent-silent");
    }
    if (!wasBusy(busySessions, sessionID)) {
      return reject("never-busy");
    }
    clearBusy(busySessions, sessionID);
    const verdict = throttle.check(sessionID, "done");
    if (!verdict.allowed) return reject(verdict.reason);
    return {
      notify: true,
      title: titleFor("done"),
      body: bodyFor(event, payload, false, sessionTitle),
      urgency: "low",
      timeoutMs: TIMEOUT_MS.low,
      kind: "done",
      sessionID,
      tag: `opencode-${sessionID}-done`,
    };
  }

  // --- permission.asked ------------------------------------------------------
  if (type === "permission.asked") {
    const id = payload.id;
    if (!id) return reject("no-permission-id");
    if (permissions.has(id)) return reject("permission-dup");
    permissions.add(id);
    const kind = "permission";
    const verdict = throttle.check(sessionID, kind);
    if (!verdict.allowed) return reject(verdict.reason);
    return {
      notify: true,
      title: titleFor(kind),
      body: markSubagent(
        permissionText(payload)
          ? formatSessionTitle(permissionText(payload))
          : formatSessionTitle(sessionTitle),
        isSubagent,
      ),
      urgency: "critical",
      timeoutMs: TIMEOUT_MS.critical,
      kind,
      sessionID,
      tag: `opencode-${sessionID}-${kind}`,
    };
  }

  // --- permission.replied: чистим память -------------------------------------
  if (type === "permission.replied") {
    const id = payload.requestID ?? payload.permissionID;
    if (id) permissions.remove(id);
    return reject("permission-answered");
  }

  // --- form.created (и question.* из старых сборок) ---------------------------
  if (isQuestionEvent(type, payload)) {
    if (!sessionID) return reject("no-session-id");
    // Форму могут переотдать повторно, пока она ждёт ответа: молчим по второму
    // разу, пока не придёт form.replied/form.cancelled.
    const formID = payload.form?.id ?? payload.id;
    if (formID) {
      if (forms.has(formID)) return reject("form-dup");
      forms.add(formID);
    }
    const kind = "question";
    const verdict = throttle.check(sessionID, kind);
    if (!verdict.allowed) return reject(verdict.reason);
    return {
      notify: true,
      title: titleFor(kind),
      body: bodyFor(event, payload, isSubagent, sessionTitle),
      urgency: "critical",
      timeoutMs: TIMEOUT_MS.critical,
      kind,
      sessionID,
      tag: `opencode-${sessionID}-${kind}`,
    };
  }

  // --- form.replied / form.cancelled: форма получила ответ -------------------
  if (type === "form.replied" || type === "form.cancelled") {
    const id = payload.id;
    if (id) forms.remove(id);
    return reject("form-answered");
  }

  // --- ошибки ----------------------------------------------------------------
  if (ERROR_TYPES.has(type)) {
    // У session.error sessionID опционален: без него не знаем, субагент ли он.
    if (isSubagent) return reject("subagent-silent");
    if (!sessionID) {
      const kind = "error";
      const verdict = throttle.check("", kind);
      if (!verdict.allowed) return reject(verdict.reason);
      return {
        notify: true,
        title: titleFor(kind),
        body: markSubagent(STRINGS.errorWithoutSession, false),
        urgency: "critical",
        timeoutMs: TIMEOUT_MS.critical,
        kind,
        sessionID: "",
        tag: `opencode--${kind}`,
      };
    }
    const kind = "error";
    const verdict = throttle.check(sessionID, kind);
    if (!verdict.allowed) return reject(verdict.reason);
    return {
      notify: true,
      title: titleFor(kind),
      body: bodyFor(event, payload, false, sessionTitle),
      urgency: "critical",
      timeoutMs: TIMEOUT_MS.critical,
      kind,
      sessionID,
      tag: `opencode-${sessionID}-${kind}`,
    };
  }

  return reject("unhandled");
}