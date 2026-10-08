// Разбор /proc/<pid>/stat и цепочка предков.

import test from "node:test";
import assert from "node:assert/strict";
import { ancestorChain, parsePpid } from "../lib/proc.js";

test("stat с пробелом в comm", () => {
  // Формат: pid (comm) state ppid ...
  const stat = "123 (a b) c) S 45 123 123 0 -1 4194304 1000 0 0 0 1 2 3 4 20 0 1 0 100";
  assert.equal(parsePpid(stat), 45);
});

test("comm со скобками: режем по ПОСЛЕДНЕЙ ')'", () => {
  // comm = "c) S", настоящий state = S, ppid = 7
  const stat = "999 (c) S) S 7 1 1 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 20 0 1 0 100";
  assert.equal(parsePpid(stat), 7);
});

test("реальный comm без пробелов", () => {
  assert.equal(parsePpid("1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 15 0"), 0);
});

test("pid 1: ppid равен 0, цепочка обрывается на нуле", () => {
  const stat = "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0";
  assert.equal(parsePpid(stat), 0);
  const chain = ancestorChain(1, (pid) => (pid === 1 ? 0 : null));
  assert.deepEqual(chain, [1]);
});

test("битый вход даёт null, а не исключение", () => {
  assert.equal(parsePpid(""), null);
  assert.equal(parsePpid("мусор"), null);
  assert.equal(parsePpid("1 (sys) S"), null);
  assert.equal(parsePpid(undefined), null);
  assert.equal(parsePpid("1 (sys) S нечисло 1"), null);
});

test("негативный ppid не проходит", () => {
  assert.equal(parsePpid("1 (sys) S -5 1 1"), null);
});

test("цепочка предков на подставных данных", () => {
  const table = new Map([
    [100, 200],
    [200, 300],
    [300, 1],
    [1, 0],
  ]);
  const chain = ancestorChain(100, (pid) => (table.has(pid) ? table.get(pid) : null));
  assert.deepEqual(chain, [100, 200, 300, 1]);
});

test("цепочка обрывается, если процесс исчез", () => {
  const chain = ancestorChain(500, (pid) => (pid === 500 ? 501 : null));
  assert.deepEqual(chain, [500, 501]);
});

test("цикл в данных не зацикливает", () => {
  const table = new Map([
    [10, 20],
    [20, 10],
  ]);
  const chain = ancestorChain(10, (pid) => (table.has(pid) ? table.get(pid) : null));
  assert.deepEqual(chain, [10, 20]);
});

test("читатель, который бросает, не роняет цепочку", () => {
  const chain = ancestorChain(7, (pid) => {
    if (pid === 7) return 8;
    throw new Error("boom");
  });
  assert.deepEqual(chain, [7, 8]);
});

test("слишком глубокая цепочка упирается в maxDepth", () => {
  // Каждый процесс — родитель предыдущего, поэтому цепочка растёт линейно.
  const chain = ancestorChain(1, (pid) => pid + 1, 5);
  assert.equal(chain.length, 5);
  assert.deepEqual(chain, [1, 2, 3, 4, 5]);
});