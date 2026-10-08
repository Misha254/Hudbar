// Разбор /proc/<pid>/stat и цепочка предков.
//
// comm (второе поле stat) заключено в скобки и может содержать пробелы и
// сами скобки: у реальных процессов встречается `kworker/R-rcu_gp`, а в
// тестах мы разбираем `a b`. Поэтому режем строку по ПОСЛЕДНЕЙ закрывающей
// скобке, а не по пробелу: только после неё начинаются настоящие поля.
// Поле 1 после `) ` — это ppid.

/**
 * ppid процесса по строке `/proc/<pid>/stat`.
 * Возвращает число либо null, если строка не разобралась.
 * @param {string} stat полная строка /proc/<pid>/stat
 * @returns {number | null}
 */
export function parsePpid(stat) {
  if (typeof stat !== "string") return null;
  const cut = stat.lastIndexOf(")");
  if (cut < 0) return null;
  // После ")" идёт пробел, затем state, затем ppid.
  const tail = stat.slice(cut + 1).trim();
  if (!tail) return null;
  const parts = tail.split(/\s+/);
  if (parts.length < 2) return null;
  const ppid = Number.parseInt(parts[1], 10);
  if (!Number.isSafeInteger(ppid) || ppid < 0) return null;
  return ppid;
}

/**
 * Цепочка предков от `pid` вверх до PID 1 или корня.
 *
 * Ветки задаются функцией: в тестах это подставная таблица, в бою чтение
 * /proc. Так цепочка проверяется без живых процессов.
 *
 * @param {number} pid с кого начинать
 * @param {(pid: number) => number | null} readPpid читает ppid
 * @param {number} [maxDepth] страховка от испорченных данных
 * @returns {number[]} pid и все его предки, по возрастанию вверх
 */
export function ancestorChain(pid, readPpid, maxDepth = 64) {
  const chain = [];
  let current = pid;
  const seen = new Set();
  for (let depth = 0; depth < maxDepth; depth++) {
    if (!Number.isSafeInteger(current) || current <= 0) break;
    if (seen.has(current)) break;
    seen.add(current);
    chain.push(current);
    let parent;
    try {
      parent = readPpid(current);
    } catch {
      break;
    }
    if (parent === null || parent === undefined) break;
    if (parent === current) break;
    current = parent;
  }
  return chain;
}