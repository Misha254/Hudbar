// Точка входа плагина opencode → dunst.
//
// API opencode v2: модуль экспортирует по умолчанию определение с `id` и
// `setup(ctx)`. Всё остальное живёт в lib/ и импортируется относительными
// путями. Хелперы наружу не экспортируются — загрузчик может принять любой
// экспорт файла за отдельный плагин.
//
// Ничего, что может упасть, не обёрнуто в try/catch. Плагин не имеет права
// ронять opencode: ни из-за недоступного dunst, ни из-за сломанного ответа
// session.get, ни из-за странного заголовка сессии.

import { createDebugLog } from "./lib/debug.js";
import { decide, isQuestionEvent, payloadOf, sessionIdOf } from "./lib/decide.js";
import { deliver } from "./lib/deliver.js";
import { isSelfTerminalFocused } from "./lib/focus.js";
import { PermissionMemory, Throttle } from "./lib/throttle.js";

/** Идентификатор плагина: по нему opencode его показывает и чинит storage. */
const ID = "hudbar-notify";

export default {
  id: ID,
  setup(ctx) {
    const debug = createDebugLog();
    const log = (level, message, extra) => debug.log(level, message, extra);

    log("info", "плагин загружен", {
      version: ctx?.app?.version ?? null,
      directory: ctx?.location?.directory ?? null,
    });

    const busySessions = new Set();
    const permissions = new PermissionMemory();
    const forms = new PermissionMemory();
    const throttle = new Throttle();

    /** sessionID → { title, parentID }. Заголовок меняется, поэтому кэш короткий. */
    const sessionCache = new Map();

    /**
     * Сессия из API: заголовок и признак субагента.
     * Ошибка запроса = считаем основной сессией без заголовка.
     */
    const sessionInfo = async (sessionID) => {
      if (!sessionID) return { title: null, parentID: undefined };
      if (sessionCache.has(sessionID)) return sessionCache.get(sessionID);
      let info = { title: null, parentID: undefined };
      try {
        const data = await ctx.session.get({ sessionID });
        info = { title: data?.title ?? null, parentID: data?.parentID };
      } catch (error) {
        log("warn", "session.get не удался, считаю сессию основной", {
          sessionID,
          error: String(error?.message ?? error),
        });
      }
      sessionCache.set(sessionID, info);
      return info;
    };

    /** Заголовок сессии для тела уведомления. null при неудаче. */
    const sessionTitle = async (sessionID) => (await sessionInfo(sessionID)).title;

    /** Субагент ли сессия. Результат кэшируется вместе с заголовком. */
    const isSubagent = async (sessionID) => Boolean((await sessionInfo(sessionID)).parentID);

    const handle = async (event) => {
      const payload = payloadOf(event);
      const sessionID = sessionIdOf(event, payload);

      // Переименование сбрасывает кэш заголовка, иначе в уведомлении останется
      // старое имя сессии.
      if (event?.type === "session.renamed" && sessionID) {
        sessionCache.set(sessionID, { title: payload.title ?? null, parentID: undefined });
      }

      // Субагента узнаём только там, где это влияет на решение.
      const needsSubagentCheck =
        event?.type === "session.idle" ||
        event?.type === "session.execution.succeeded" ||
        event?.type === "session.execution.interrupted" ||
        event?.type === "session.error" ||
        event?.type === "session.execution.failed" ||
        event?.type === "permission.asked" ||
        isQuestionEvent(event?.type, payload) ||
        String(event?.type ?? "").startsWith("question");
      const subagent = needsSubagentCheck && sessionID ? await isSubagent(sessionID) : false;

      const title = await sessionTitle(sessionID);
      const decision = decide({
        event,
        isSubagent: subagent,
        sessionTitle: title,
        busySessions,
        permissions,
        forms,
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
    };

    const controller = new AbortController();
    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
          try {
            await handle(event);
          } catch (error) {
            log("error", "обработчик события упал", {
              event: event?.type ?? "-",
              error: String(error?.message ?? error),
            });
          }
        }
      } catch (error) {
        // Подписка рвётся только по abort (выгрузка плагина) или по смерти
        // сервера — в обоих случаях это не ошибка плагина.
        if (!controller.signal.aborted) {
          log("error", "подписка на события оборвалась", {
            error: String(error?.message ?? error),
          });
        }
      }
    })();

    return () => controller.abort();
  },
};