// Троттлинг и память разрешений на внедрённых часах.

import test from "node:test";
import assert from "node:assert/strict";
import {
  DONE_LIMIT_PER_MINUTE,
  PermissionMemory,
  Throttle,
} from "../lib/throttle.js";

/** Часы, которые двигает тест вручную. */
function fakeClock() {
  let now = 1_000_000;
  return {
    now: () => now,
    advance: (ms) => {
      now += ms;
    },
  };
}

test("первое уведомление пары проходит", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  assert.equal(throttle.check("s1", "done").allowed, true);
});

test("второе в пределах 5 с молчит", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  throttle.check("s1", "done");
  clock.advance(4999);
  const second = throttle.check("s1", "done");
  assert.equal(second.allowed, false);
  assert.equal(second.reason, "throttled");
});

test("через 5 с та же пара снова проходит", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  throttle.check("s1", "done");
  clock.advance(5000);
  assert.equal(throttle.check("s1", "done").allowed, true);
});

test("разные сессии не мешают друг другу", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  throttle.check("s1", "done");
  assert.equal(throttle.check("s2", "done").allowed, true);
});

test("разные kind не мешают друг другу", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  throttle.check("s1", "done");
  assert.equal(throttle.check("s1", "permission").allowed, true);
});

test("глобальный лимит 3/мин только для done", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  assert.equal(throttle.check("s1", "done").allowed, true);
  clock.advance(6000);
  assert.equal(throttle.check("s2", "done").allowed, true);
  clock.advance(6000);
  assert.equal(throttle.check("s3", "done").allowed, true);
  // четвёртое упирается в глобальный лимит
  clock.advance(6000);
  const blocked = throttle.check("s4", "done");
  assert.equal(blocked.allowed, false);
  assert.equal(blocked.reason, "done-limit");
});

test("лимит сбрасывается через минуту", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  for (const s of ["a", "b", "c"]) throttle.check(s, "done");
  clock.advance(60_001);
  assert.equal(throttle.check("d", "done").allowed, true);
});

test("critical проходит даже при исчерпанном лимите done", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  for (const s of ["a", "b", "c"]) throttle.check(s, "done");
  clock.advance(6000);
  assert.equal(throttle.check("a", "question").allowed, true);
  assert.equal(throttle.check("a", "permission").allowed, true);
  assert.equal(throttle.check("a", "error").allowed, true);
  // а вот четвёртый done — по-прежнему блокируется
  assert.equal(throttle.check("z", "done").allowed, false);
});

test("лимит done по умолчанию равен 3", () => {
  assert.equal(DONE_LIMIT_PER_MINUTE, 3);
});

test("forget сбрасывает пару", () => {
  const clock = fakeClock();
  const throttle = new Throttle({ now: clock.now });
  throttle.check("s1", "permission");
  assert.equal(throttle.check("s1", "permission").allowed, false);
  throttle.forget("s1", "permission");
  assert.equal(throttle.check("s1", "permission").allowed, true);
});

test("память разрешений: показали один раз", () => {
  const memory = new PermissionMemory();
  assert.equal(memory.has("p1"), false);
  memory.add("p1");
  assert.equal(memory.has("p1"), true);
  assert.equal(memory.size, 1);
});

test("ответ на разрешение убирает id", () => {
  const memory = new PermissionMemory();
  memory.add("p1");
  memory.remove("p1");
  assert.equal(memory.has("p1"), false);
  assert.equal(memory.size, 0);
});

test("remove отсутствующего id не бросает", () => {
  const memory = new PermissionMemory();
  assert.doesNotThrow(() => memory.remove("нет-такого"));
});