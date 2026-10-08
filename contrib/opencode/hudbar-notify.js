// Точка входа плагина opencode → dunst.
//
// ОДИН экспорт: загрузчик может принять любой экспорт из файла за плагин, а
// хелперы в этом файле принял бы за плагины-пустышки. Всё остальное живёт в
// lib/ и импортируется относительными путями.
//
// Ничего, что может упасть, не обёрнуто в try/catch. Плагин не имеет права
// ронять opencode: ни из-за недоступного dunst, ни из-за сломанного JSON от
// niri, ни из-за странного заголовка сессии.

import { createDebugLog } from "./lib/debug.js";
import { decide } from "./lib/decide.js";
import { deliver } from "./lib/deliver.js";
import { isSelfTerminalFocused } from "./lib/focus.js";
import { PermissionMemory, Throttle } from "./lib/throttle.js";

const SERVICE = "hudbar-notify";

export const HudbarNotify = async ({ client, directory }) => {
  const debug = createDebugLog();

  /** Писать в лог opencode, никогда не бросая. */
  const log = (level, message, extra) => {
    try {
      const body = { service: SERVICE, level, message };
      if (extra !== undefined) body.extra = extra;
      return client?.app?.log?.({ body });
    } catch {
      return undefined;
    }
  };

  try {
    log("info", "плагин загружен", { directory: directory ?? null });
  } catch {
    // лог не критичен
  }

  const busySessions = new Set();
  const permissions = new PermissionMemory();
  const throttle = new Throttle();

  /**
   * Субагент ли сессия: `parentID` есть — субагент.
   * Результат кэшируется по sessionID. Ошибка запроса = считаем основной.
   */
  const subagentCache = new Map();
  const isSubagent = async (sessionID) => {
    if (!sessionID) return false;
    if (subagentCache.has(sessionID)) return subagentCache.get(sessionID);
    let result = false;
    try {
      const response = await client.session.get({ path: { id: sessionID } });
      const data = response?.data;
      result = Boolean(data?.parentID);
    } catch (error) {
      log("warn", "session.get не удался, считаю сессию основной", {
        sessionID,
        error: String(error?.message ?? error),
      });
      result = false;
    }
    subagentCache.set(sessionID, result);
    return result;
  };

  /** Заголовок сессии для тела уведомления. null при неудаче. */
  const sessionTitle = async (sessionID) => {
    if (!sessionID) return null;
    try {
      const response = await client.session.get({ path: { id: sessionID } });
      const data = response?.data;
      return data?.title ?? null;
    } catch {
      return null;
    }
  };

  const titleCache = new Map();
  const titleForSession = async (sessionID) => {
    if (!sessionID) return null;
    if (titleCache.has(sessionID)) return titleCache.get(sessionID);
    const title = await sessionTitle(sessionID);
    titleCache.set(sessionID, title);
    return title;
  };

  return {
    event: async ({ event }) => {
      try {
        const properties = event?.properties ?? {};
        const sessionID = properties.sessionID ?? properties.id ?? "";

        // Субагента узнаём только там, где это влияет на решение.
        const needsSubagentCheck =
          event?.type === "session.idle" ||
          event?.type === "session.error" ||
          event?.type === "permission.updated" ||
          String(event?.type ?? "").startsWith("question");
        const subagent = needsSubagentCheck && sessionID
          ? await isSubagent(sessionID)
          : false;

        const title = await titleForSession(sessionID);
        const decision = decide({
          event,
          isSubagent: subagent,
          sessionTitle: title,
          busySessions,
          permissions,
          throttle,
        });

        debug.write(event, decision);
        if (!decision.notify) return;

        // Фокус учитывается и для critical: пользователь и так видит
        // разрешение или вопрос, если смотрит в это окно.
        const focus = await isSelfTerminalFocused();
        if (focus.focused) {
          log("debug", "окно с opencode в фокусе, уведомление пропущено", {
            sessionID: decision.sessionID,
            kind: decision.kind,
            reason: focus.reason,
          });
          return;
        }

        await deliver(decision.title, decision.body, {
          urgency: decision.urgency,
          tag: decision.tag,
          log: (entry) => log(entry.level, entry.message, entry.extra),
        });
      } catch (error) {
        log("error", "обработчик события упал", {
          error: String(error?.message ?? error),
        });
      }
    },
  };
};