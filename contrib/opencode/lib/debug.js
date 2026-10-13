// Отладочный журнал.
//
// Включается переменной HUDBAR_NOTIFY_DEBUG=1. Без неё файл не создаётся
// вообще: лишний лог на каждый запуск opencode ни к чему.
//
// В v2 у плагина нет `client.app.log`, а console.log серверного плагина
// пишет прямо в терминал и портит TUI. Поэтому единственный канал журнала —
// этот файл, и `log()` тоже молчит без переменной.
//
// В файл пишутся ТОЛЬКО типы событий и идентификаторы. Содержимое сообщений
// и переписка с моделью сюда не попадают никогда.

import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { homedir } from "node:os";

export const DEBUG_ENV = "HUDBAR_NOTIFY_DEBUG";
export const LOG_PATH = `${homedir()}/.cache/hudbar/opencode-notify.log`;

/** Разрешён ли отладочный режим. */
export function debugEnabled(env = process.env) {
  return env?.[DEBUG_ENV] === "1";
}

/**
 * Полезная нагрузка события.
 *
 * В v2 сервер отдаёт `{ type, data }`. Старые сборки — `{ type, properties }`,
 * поэтому берём первое непустое: плагин не должен ломаться от смены формы
 * события.
 *
 * @param {any} event
 * @returns {any}
 */
function payloadOf(event) {
  if (!event || typeof event !== "object") return {};
  if (event.data && typeof event.data === "object") return event.data;
  if (event.properties && typeof event.properties === "object") return event.properties;
  return {};
}

/**
 * Создать отладочный логгер.
 * @param {{ env?: object, path?: string, now?: () => Date }} [options]
 * @returns {{ enabled: boolean, write: (event: any, decision: {notify: boolean, reason?: string, kind?: string}) => void, log: (level: string, message: string, extra?: object) => void }}
 */
export function createDebugLog(options = {}) {
  const env = options.env ?? process.env;
  const path = options.path ?? LOG_PATH;
  const now = options.now ?? (() => new Date());

  if (!debugEnabled(env)) {
    return { enabled: false, write: () => {}, log: () => {} };
  }

  let ready = false;
  const append = (line) => {
    try {
      if (!ready) {
        mkdirSync(dirname(path), { recursive: true });
        ready = true;
      }
      appendFileSync(path, line, "utf8");
    } catch {
      // Отладочный лог не имеет права ничего ломать.
    }
  };

  return {
    enabled: true,
    write(event, decision) {
      const payload = payloadOf(event);
      // У form.created сессия лежит внутри формы, у остальных — наверху.
      const sessionID =
        payload.form?.sessionID ?? payload.sessionID ?? payload.id ?? "-";
      const kind = decision?.kind ?? "-";
      const notify = decision?.notify ? "notify" : "skip";
      const reason = decision?.reason ?? "-";
      append(
        `${now().toISOString()} event=${event?.type ?? "-"} session=${sessionID} decision=${notify} kind=${kind} reason=${reason}\n`,
      );
    },
    log(level, message, extra) {
      let tail = "";
      if (extra !== undefined) {
        try {
          tail = " " + JSON.stringify(extra);
        } catch {
          tail = "";
        }
      }
      append(`${now().toISOString()} log level=${level ?? "-"} msg=${message ?? "-"}${tail}\n`);
    },
  };
}