// Таблица решений: что делать с очередным событием.
//
// Решение принимает чистая функция без сети и без /proc. Всё, что нужно для
// решения, передаётся аргументами: память busy-сессий, множество показанных
// разрешений, троттлинг и признак субагента (который добывается вызывающий —
// там нужен сетевой запрос).
//
// Решение намеренно НЕ включает проверку фокуса и сам вызов notify-send:
// это побочные эффекты, их делает точка входа.

import { Throttle } from "./throttle.js";
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
 *   | { notify: true, title: string, body: string, urgency: "low"|"normal"|"critical", kind: string, sessionID: string, tag: string }
 * } Decision
 */

/** Помечаем сессию работающей: делается на session.status === "busy". */
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
 * Правда ли, что тип события — вопрос агента.
 *
 * В 1.18.35 вопросы есть только в v2-схеме, поэтому сверяемся по строке:
 * подходит и `question.asked`, и `question.v2.asked` — на случай, если
 * событие появится в этой версии или в следующей.
 *
 * @param {string} type
 * @returns {boolean}
 */
export function isQuestionEvent(type) {
  return (
    typeof type === "string" &&
    type.startsWith("question") &&
    type.endsWith(".asked")
  );
}

/**
 * Достать текст первого вопроса из payload.
 * @param {any} properties
 * @returns {unknown}
 */
export function firstQuestionText(properties) {
  if (!properties || typeof properties !== "object") return "";
  const questions = properties.questions;
  if (Array.isArray(questions) && questions.length > 0) {
    const first = questions[0];
    if (first && typeof first === "object") {
      const text = first.question ?? first.header;
      if (text) return text;
    }
    if (typeof first === "string") return first;
  }
  return properties.question ?? properties.header ?? "";
}

/**
 * Тело уведомления для события.
 * @param {any} event
 * @param {boolean} isSubagent
 * @param {any} sessionTitle
 * @returns {string}
 */
function bodyFor(event, isSubagent, sessionTitle) {
  if (isQuestionEvent(event.type)) {
    return markSubagent(formatQuestionText(firstQuestionText(event.properties)), isSubagent);
  }
  if (event.type === "session.error" && !event.properties?.sessionID) {
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
 * @param {Throttle} input.throttle
 * @returns {Decision}
 */
export function decide({
  event,
  isSubagent,
  sessionTitle,
  busySessions,
  permissions,
  throttle,
}) {
  const type = event?.type;
  const properties = event?.properties ?? {};
  const sessionID = properties.sessionID ?? properties.id ?? "";

  const reject = (reason) => ({ notify: false, reason });

  if (!type) return reject("no-type");

  // --- session.status: только пометка, уведомления нет -----------------------
  if (type === "session.status") {
    if (!sessionID) return reject("no-session-id");
    if (properties.status?.type === "busy") {
      markBusy(busySessions, sessionID);
      return reject("marked-busy");
    }
    return reject("status-not-busy");
  }

  // --- session.idle: только если реально работали ---------------------------
  if (type === "session.idle") {
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
      body: bodyFor(event, false, sessionTitle),
      urgency: "low",
      kind: "done",
      sessionID,
      tag: `opencode-${sessionID}-done`,
    };
  }

  // --- permission.updated ----------------------------------------------------
  if (type === "permission.updated") {
    const id = properties.id;
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
        properties.title ? formatSessionTitle(properties.title) : formatSessionTitle(sessionTitle),
        isSubagent,
      ),
      urgency: "critical",
      kind,
      sessionID,
      tag: `opencode-${sessionID}-${kind}`,
    };
  }

  // --- permission.replied: чистим память -------------------------------------
  if (type === "permission.replied") {
    const id = properties.permissionID;
    if (id) permissions.remove(id);
    return reject("permission-answered");
  }

  // --- question.*: подписка по строковому типу ------------------------------
  if (isQuestionEvent(type)) {
    if (!sessionID) return reject("no-session-id");
    const kind = "question";
    const verdict = throttle.check(sessionID, kind);
    if (!verdict.allowed) return reject(verdict.reason);
    return {
      notify: true,
      title: titleFor(kind),
      body: bodyFor(event, isSubagent, sessionTitle),
      urgency: "critical",
      kind,
      sessionID,
      tag: `opencode-${sessionID}-${kind}`,
    };
  }

  // --- session.error ---------------------------------------------------------
  if (type === "session.error") {
    // У error оба поля опциональны: без sessionID не знаем, субагент ли он.
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
      body: bodyFor(event, false, sessionTitle),
      urgency: "critical",
      kind,
      sessionID,
      tag: `opencode-${sessionID}-${kind}`,
    };
  }

  return reject("unhandled");
}