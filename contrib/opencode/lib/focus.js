// Проверка фокуса: не показывать уведомление, если окно с этим opencode и так
// на экране.
//
// Идея: у окна niri есть pid терминала. Если пройти вверх по родителям от
// процесса плагина и встретить этот pid, значит терминал с этим opencode в
// фокусе — человек и так видит ответ.
//
// ЛЮБОЙ сбой означает «не в фокусе», то есть уведомляем. Лишнее уведомление
// раздражает меньше, чем промолчавшее.

import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { ancestorChain, parsePpid } from "./proc.js";

/** Таймаут вызова niri, мс. */
export const NIRI_TIMEOUT_MS = 500;

/**
 * pid сфокусированного окна. null при любой ошибке.
 *
 * @param {{ run?: Function }} [deps]
 * @returns {Promise<number | null>}
 */
export async function focusedWindowPid(deps = {}) {
  const run =
    deps.run ??
    ((args) =>
      new Promise((resolve) => {
        try {
          execFile(
            args[0],
            args.slice(1),
            { timeout: NIRI_TIMEOUT_MS, encoding: "utf8" },
            (error, stdout) => {
              if (error) {
                resolve(null);
                return;
              }
              resolve(String(stdout));
            },
          );
        } catch {
          resolve(null);
        }
      }));
  const raw = await run(["niri", "msg", "--json", "focused-window"]);
  if (typeof raw !== "string" || !raw.trim()) return null;
  try {
    const parsed = JSON.parse(raw);
    const pid = parsed?.pid;
    return Number.isSafeInteger(pid) && pid > 0 ? pid : null;
  } catch {
    return null;
  }
}

/**
 * Читает ppid из /proc. null, если процесса нет.
 * @param {number} pid
 * @returns {number | null}
 */
export function readPpidFromProc(pid) {
  try {
    return parsePpid(readFileSync(`/proc/${pid}/stat`, "utf8"));
  } catch {
    return null;
  }
}

/**
 * Находится ли терминал с этим opencode в фокусе.
 *
 * @param {object} [deps]
 * @param {number} [deps.selfPid]
 * @param {(pid: number) => number | null} [deps.readPpid]
 * @param {() => Promise<number | null>} [deps.getFocusedPid]
 * @returns {Promise<{ focused: boolean, reason: string }>}
 */
export async function isSelfTerminalFocused(deps = {}) {
  const selfPid = deps.selfPid ?? process.pid;
  const readPpid = deps.readPpid ?? readPpidFromProc;
  const getFocusedPid = deps.getFocusedPid ?? focusedWindowPid;

  let focusedPid;
  try {
    focusedPid = await getFocusedPid();
  } catch {
    return { focused: false, reason: "niri-error" };
  }
  if (!focusedPid) return { focused: false, reason: "no-focused-pid" };

  const chain = ancestorChain(selfPid, readPpid);
  if (!chain.includes(focusedPid)) {
    return { focused: false, reason: "not-ancestor" };
  }
  return { focused: true, reason: "ancestor" };
}