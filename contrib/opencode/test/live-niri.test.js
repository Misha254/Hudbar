// Живой вызов niri. НЕ запускается в обычном прогоне: висит в песочнице
// раннера, хотя из shell отвечает мгновенно. Запуск вручную:
//   node --test test/live-niri.test.js
import test from "node:test";
import assert from "node:assert/strict";
import { focusedWindowPid } from "../lib/focus.js";

test("focusedWindowPid отвечает из живого niri", async () => {
  const pid = await Promise.race([
    focusedWindowPid(),
    new Promise((resolve) => setTimeout(() => resolve("TIMEOUT"), 3000)),
  ]);
  assert.notEqual(pid, "TIMEOUT");
  assert.ok(pid === null || (Number.isSafeInteger(pid) && pid > 0));
});
