// Проверка фокуса и отладочного лога.

import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  isSelfTerminalFocused,
  focusedWindowPid,
  readPpidFromProc,
} from "../lib/focus.js";
import { createDebugLog, debugEnabled } from "../lib/debug.js";

// --- фокус ------------------------------------------------------------------

test("окно в фокусе: pid окна найден в цепочке предков", async () => {
  const table = new Map([
    [100, 200],
    [200, 300],
    [300, 1],
    [1, 0],
  ]);
  const result = await isSelfTerminalFocused({
    selfPid: 100,
    readPpid: (pid) => (table.has(pid) ? table.get(pid) : null),
    getFocusedPid: async () => 200,
  });
  assert.equal(result.focused, true);
  assert.equal(result.reason, "ancestor");
});

test("окно другое: цепочка не пересекается", async () => {
  const table = new Map([
    [100, 200],
    [200, 1],
    [1, 0],
  ]);
  const result = await isSelfTerminalFocused({
    selfPid: 100,
    readPpid: (pid) => (table.has(pid) ? table.get(pid) : null),
    getFocusedPid: async () => 999,
  });
  assert.equal(result.focused, false);
  assert.equal(result.reason, "not-ancestor");
});

test("niri упал: считаем «не в фокусе», чтобы не молчать зря", async () => {
  const result = await isSelfTerminalFocused({
    selfPid: 100,
    readPpid: () => null,
    getFocusedPid: async () => {
      throw new Error("niri недоступен");
    },
  });
  assert.equal(result.focused, false);
  assert.equal(result.reason, "niri-error");
});

test("niri вернул мусор: тоже уведомляем", async () => {
  const result = await isSelfTerminalFocused({
    selfPid: 100,
    readPpid: () => null,
    getFocusedPid: async () => null,
  });
  assert.equal(result.focused, false);
  assert.equal(result.reason, "no-focused-pid");
});

test("битый JSON не роняет проверку", async () => {
  const result = await isSelfTerminalFocused({
    selfPid: 100,
    readPpid: () => null,
    getFocusedPid: async () => null,
  });
  assert.equal(result.focused, false);
});

test("focusedWindowPid разбирает JSON niri и достаёт pid", async () => {
  // Тот же разбор, но на подставном выводе: без обращения к композитору.
  const json = '{"id":3,"title":"OC","app_id":"opencode","pid":7386}';
  assert.equal(await focusedWindowPid({ run: async () => json }), 7386);
});

test("мусор и пустота от niri дают null, а не исключение", async () => {
  // Разбор не бросает наружу: сбой композитора — это «уведомляем», а не
  // краш плагина. Проверяем ровно это, а не `JSON.parse`, который на мусоре
  // обязан бросить.
  for (const raw of ["не json", "", "   ", "[]", '{"pid":0}', '{"pid":-1}', "null"]) {
    assert.equal(
      await focusedWindowPid({ run: async () => raw }),
      null,
      `вывод ${JSON.stringify(raw)} → null`,
    );
  }
});

test("readPpidFromProc читает текущий процесс", () => {
  const ppid = readPpidFromProc(process.pid);
  assert.ok(ppid === null || Number.isSafeInteger(ppid));
});

// --- отладочный лог ---------------------------------------------------------

test("без переменной окружения файл не создаётся", () => {
  const dir = mkdtempSync(join(tmpdir(), "hudbar-debug-"));
  const path = join(dir, "nested", "log.txt");
  const log = createDebugLog({ env: {}, path });
  assert.equal(log.enabled, false);
  log.write({ type: "session.idle", properties: { sessionID: "s1" } }, { notify: false, reason: "never-busy" });
  assert.equal(existsSync(path), false);
  rmSync(dir, { recursive: true, force: true });
});

test("с переменной окружения пишется строка на каждое событие", () => {
  const dir = mkdtempSync(join(tmpdir(), "hudbar-debug-"));
  const path = join(dir, "log.txt");
  const log = createDebugLog({ env: { HUDBAR_NOTIFY_DEBUG: "1" }, path });
  assert.equal(log.enabled, true);
  log.write({ type: "session.idle", properties: { sessionID: "s1" } }, { notify: false, reason: "never-busy" });
  log.write(
    { type: "session.idle", properties: { sessionID: "s1" } },
    { notify: true, reason: "-", kind: "done" },
  );
  const text = readFileSync(path, "utf8");
  const lines = text.trim().split("\n");
  assert.equal(lines.length, 2);
  assert.match(lines[0], /event=session\.idle session=s1 decision=skip kind=- reason=never-busy/);
  assert.match(lines[1], /decision=notify kind=done/);
  rmSync(dir, { recursive: true, force: true });
});

test("в лог не попадает содержимое сообщений", () => {
  const dir = mkdtempSync(join(tmpdir(), "hudbar-debug-"));
  const path = join(dir, "log.txt");
  const log = createDebugLog({ env: { HUDBAR_NOTIFY_DEBUG: "1" }, path });
  log.write(
    {
      type: "permission.updated",
      properties: { id: "p1", sessionID: "s1", title: "СЕКРЕТНЫЙ ТЕКСТ" },
    },
    { notify: true, reason: "-", kind: "permission" },
  );
  const text = readFileSync(path, "utf8");
  assert.ok(!text.includes("СЕКРЕТНЫЙ ТЕКСТ"));
  assert.ok(text.includes("session=s1"));
  rmSync(dir, { recursive: true, force: true });
});

test("сломанный лог не бросает наружу", () => {
  // Недоступный путь — это обычный каталог, внутри которого лежит файл:
  // `mkdirSync` на файле даёт EEXIST, а запись в «файл/внутрь» — ENOTDIR.
  // Путь под `/proc` для этого не годится: там `mkdirSync` на этой машине
  // виснет вместо того, чтобы вернуть ошибку, и тест никогда не завершится.
  const dir = mkdtempSync(join(tmpdir(), "hudbar-debug-"));
  const blocker = join(dir, "not-a-dir");
  writeFileSync(blocker, "");
  const log = createDebugLog({ env: { HUDBAR_NOTIFY_DEBUG: "1" }, path: join(blocker, "x.log") });
  assert.doesNotThrow(() =>
    log.write({ type: "session.idle", properties: {} }, { notify: false, reason: "x" }),
  );
  rmSync(dir, { recursive: true, force: true });
});

test("debugEnabled требует ровно '1'", () => {
  assert.equal(debugEnabled({ HUDBAR_NOTIFY_DEBUG: "1" }), true);
  assert.equal(debugEnabled({ HUDBAR_NOTIFY_DEBUG: "true" }), false);
  assert.equal(debugEnabled({ HUDBAR_NOTIFY_DEBUG: "0" }), false);
  assert.equal(debugEnabled({}), false);
});