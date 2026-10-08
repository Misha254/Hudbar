// Единственная точка транспорта: отправка в dunst.
//
// Позже она заменится на вызов HUDbar, поэтому всё остальное о ней не знает.
// Важно здесь три вещи:
//
//   * ТОЛЬКО argv, никакого shell. Заголовок сессии приходит из подсказок
//     opencode и может содержать `$(touch /tmp/x)`. Через `sh -c` такой
//     заголовок выполнил бы команду — это проверено в тестах. Массив
//     аргументов такой возможности не даёт.
//   * `--` перед позиционными аргументами: заголовок, начинающийся с `-`,
//     иначе прочитался бы как опция notify-send.
//   * Ненулевой код выхода и ENOENT — это лог, а не исключение. Плагин не
//     имеет права ронять opencode из-за того, что нет dunst.

import { execFile } from "node:child_process";

/** Демон уведомлений и имя, по которому он нас узнает. */
export const APP_NAME = "opencode";

/** Таймаут внешнего вызова, мс. */
export const DELIVER_TIMEOUT_MS = 2000;

/**
 * Собрать argv для notify-send.
 *
 * Вынесено отдельно, чтобы тесты проверяли сам массив, а не результат
 * запуска: инъекция проверяется именно составом аргументов.
 *
 * @param {string} title
 * @param {string} body
 * @param {{ urgency?: string, tag?: string, appName?: string }} [options]
 * @returns {string[]}
 */
export function buildNotifyArgs(title, body, options = {}) {
  const args = [
    "-a",
    options.appName ?? APP_NAME,
    "-u",
    options.urgency ?? "normal",
  ];
  if (options.tag) {
    args.push("-h", `string:x-dunst-stack-tag:${options.tag}`);
  }
  // Всё после "--" notify-send считает позиционными аргументами, даже если
  // строка начинается с минуса.
  args.push("--", String(title ?? ""), String(body ?? ""));
  return args;
}

/**
 * Отправить уведомление. Никогда не бросает.
 *
 * @param {string} title
 * @param {string} body
 * @param {object} [options]
 * @param {"low"|"normal"|"critical"} [options.urgency]
 * @param {string} [options.tag]
 * @param {string} [options.program] подмена бинаря для тестов
 * @param {(entry: {level: string, message: string, extra?: object}) => void} [options.log]
 * @returns {Promise<{ ok: boolean, reason?: string, code?: number }>}
 */
export function deliver(title, body, options = {}) {
  const log =
    options.log ??
    (() => {
      /* без логгера просто молчим */
    });
  const args = buildNotifyArgs(title, body, options);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      resolve(result);
    };
    try {
      const child = execFile(
        options.program ?? "notify-send",
        args,
        {
          timeout: DELIVER_TIMEOUT_MS,
          // Свой stdout не нужен: вывод уведомления не важен.
          encoding: "utf8",
        },
        (error, _stdout, stderr) => {
          if (!error) {
            finish({ ok: true });
            return;
          }
          if (error.code === "ENOENT") {
            log({
              level: "warn",
              message: "notify-send не найден, уведомление пропущено",
              extra: { title: String(title ?? "") },
            });
            finish({ ok: false, reason: "enoent" });
            return;
          }
          if (error.killed || error.signal === "SIGTERM") {
            log({
              level: "warn",
              message: `notify-send не ответил за ${DELIVER_TIMEOUT_MS} мс`,
              extra: { title: String(title ?? "") },
            });
            finish({ ok: false, reason: "timeout" });
            return;
          }
          log({
            level: "warn",
            message: `notify-send вернул ошибку: ${String(stderr ?? "").trim() || error.message}`,
            extra: { title: String(title ?? ""), code: error.code ?? null },
          });
          finish({ ok: false, reason: "failed", code: error.code ?? null });
        },
      );
      // Если процесс всё же не завершился по таймауту — добиваем, чтобы не
      // держать обработчик opencode.
      child.on("error", (error) => {
        finish({ ok: false, reason: String(error?.code ?? "spawn-error") });
      });
    } catch (error) {
      log({
        level: "error",
        message: `не удалось запустить notify-send: ${String(error?.message ?? error)}`,
      });
      finish({ ok: false, reason: "throw" });
    }
  });
}