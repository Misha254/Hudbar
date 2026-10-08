// Троттлинг и дедупликация уведомлений.
//
// Два независимых ограничителя:
//   * пара (sessionID, kind) не повторяется чаще, чем раз в THROTTLE_MS —
//     это защита от «idle пришёл трижды подряд»;
//   * глобальный лимит в минуту касается ТОЛЬКО kind "done": «готово» при
//     десяти параллельных сессиях — шум, а «нужно разрешение» — нет.
//
// Часы внедряются, чтобы тесты не ждали реальное время.

export const THROTTLE_MS = 5000;
export const DONE_LIMIT_PER_MINUTE = 3;
export const WINDOW_MS = 60000;

/**
 * Ограничитель уведомлений.
 */
export class Throttle {
  /**
   * @param {{ now?: () => number, throttleMs?: number, doneLimit?: number, windowMs?: number }} [options]
   */
  constructor(options = {}) {
    this.now = options.now ?? (() => Date.now());
    this.throttleMs = options.throttleMs ?? THROTTLE_MS;
    this.doneLimit = options.doneLimit ?? DONE_LIMIT_PER_MINUTE;
    this.windowMs = options.windowMs ?? WINDOW_MS;
    /** @type {Map<string, number>} последнее уведомление по паре session+kind */
    this.lastSent = new Map();
    /** @type {number[]} времена последних уведомлений kind=done */
    this.doneTimes = [];
  }

  /**
   * Можно ли отправить уведомление для этой пары.
   * @param {string} sessionID
   * @param {string} kind
   * @returns {{ allowed: boolean, reason?: string }}
   */
  check(sessionID, kind) {
    const now = this.now();
    const key = `${sessionID}\u0000${kind}`;
    const last = this.lastSent.get(key);
    if (last !== undefined && now - last < this.throttleMs) {
      return { allowed: false, reason: "throttled" };
    }
    if (kind === "done") {
      this.doneTimes = this.doneTimes.filter((t) => now - t < this.windowMs);
      if (this.doneTimes.length >= this.doneLimit) {
        return { allowed: false, reason: "done-limit" };
      }
    }
    this.lastSent.set(key, now);
    if (kind === "done") this.doneTimes.push(now);
    return { allowed: true };
  }

  /** Забыть пару: например, разрешение от answered, чтобы можно было снова. */
  forget(sessionID, kind) {
    this.lastSent.delete(`${sessionID}\u0000${kind}`);
  }
}

/**
 * Множество id разрешений, по которым уже показали уведомление.
 * Ответ на разрешение (`permission.replied`) убирает id.
 */
export class PermissionMemory {
  constructor() {
    /** @type {Set<string>} */
    this.pending = new Set();
  }

  /** Уже показывали? */
  has(id) {
    return this.pending.has(id);
  }

  /** Пометить как показанное. */
  add(id) {
    this.pending.add(id);
  }

  /** Убрать: разрешение получило ответ. */
  remove(id) {
    this.pending.delete(id);
  }

  get size() {
    return this.pending.size;
  }
}