// Отладочный журнал.
//
// Включается переменной HUDBAR_NOTIFY_DEBUG=1. Без неё файл не создаётся
// вообще: лишний лог на каждый запуск opencode ни к чему.
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
 * Создать отладочный логгер.
 * @param {{ env?: object, path?: string, now?: () => Date }} [options]
 * @returns {{ enabled: boolean, write: (event: any, decision: {notify: boolean, reason?: string, kind?: string}) => void }}
 */
export function createDebugLog(options = {}) {
  const env = options.env ?? process.env;
  const path = options.path ?? LOG_PATH;
  const now = options.now ?? (() => new Date());

  if (!debugEnabled(env)) {
    return { enabled: false, write: () => {} };
  }

  let ready = false;
  return {
    enabled: true,
    write(event, decision) {
      try {
        if (!ready) {
          mkdirSync(dirname(path), { recursive: true });
          ready = true;
        }
        const properties = event?.properties ?? {};
        const sessionID = properties.sessionID ?? properties.id ?? "-";
        const kind = decision?.kind ?? "-";
        const notify = decision?.notify ? "notify" : "skip";
        const reason = decision?.reason ?? "-";
        appendFileSync(
          path,
          `${now().toISOString()} event=${event?.type ?? "-"} session=${sessionID} decision=${notify} kind=${kind} reason=${reason}\n`,
          "utf8",
        );
      } catch {
        // Отладочный лог не имеет права ничего ломать.
      }
    },
  };
}