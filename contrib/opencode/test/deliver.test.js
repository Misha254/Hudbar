// Транспорт: argv без shell, инъекции, обработка отказов.
//
// Тесты НЕ бьют по живому dunst: для этого в deliver есть options.program,
// и тесты подставляют заглушку-node, которая только пишет argv в файл.
// Один реальный вызов делается отдельно, руками, при приёмке.
//
// Проверка инъекции опирается на наблюдение из разведки N0: через `sh -c`
// заголовок с `$(touch ...)` выполняет команду, через argv — нет. Здесь
// проверяем, что argv не даёт такой возможности, на настоящем execFile.

import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DELIVER_TIMEOUT_MS, buildNotifyArgs, deliver } from "../lib/deliver.js";

/** Заголовки, которые пытаются что-то выполнить. */
const EVIL_TITLES = ["$(touch /tmp/HUDBAR_INJECTED)", "'; touch /tmp/HUDBAR_INJECTED; '"];

/**
 * Заглушка notify-send: пишет полученные argv в файл и выходит с 0.
 * Ничего не отправляет, живой dunst не трогает.
 */
function stubNotifySend(dir) {
  const script = join(dir, "notify-send-stub");
  const out = join(dir, "argv.txt");
  writeFileSync(
    script,
    `#!/usr/bin/env node\n` +
      `import { writeFileSync } from "node:fs";\n` +
      `writeFileSync(${JSON.stringify(out)}, JSON.stringify(process.argv.slice(2)));\n`,
    "utf8",
  );
  chmodSync(script, 0o755);
  return { program: script, out };
}

// --- форма argv -------------------------------------------------------------

test("argv собирается ровно как задумано", () => {
  const args = buildNotifyArgs("Готово", "Моя сессия", {
    urgency: "low",
    tag: "opencode-s1-done",
  });
  assert.deepEqual(args, [
    "-a",
    "opencode",
    "-u",
    "low",
    "-h",
    "string:x-dunst-stack-tag:opencode-s1-done",
    "--",
    "Готово",
    "Моя сессия",
  ]);
});

test("тег не передаётся, если его нет", () => {
  const args = buildNotifyArgs("t", "b");
  assert.equal(args.includes("--"), true);
  assert.equal(args.some((a) => a.startsWith("string:x-dunst-stack-tag")), false);
});

test("заголовок с минусом остаётся позиционным аргументом", () => {
  const args = buildNotifyArgs("-rf -- да", "тело");
  const dashIndex = args.indexOf("--");
  assert.equal(args[dashIndex + 1], "-rf -- да");
  assert.equal(args[dashIndex + 2], "тело");
});

test("таймаут задан", () => {
  assert.equal(DELIVER_TIMEOUT_MS, 2000);
});

// --- заглушка вместо живого dunst ------------------------------------------

test("deliver доставляет argv до подставленного бинаря", async () => {
  const dir = mkdtempSync(join(tmpdir(), "hudbar-stub-"));
  const stub = stubNotifySend(dir);
  const result = await deliver("Готово", "Моя сессия", {
    urgency: "low",
    tag: "opencode-s1-done",
    program: stub.program,
  });
  assert.equal(result.ok, true);
  const argv = JSON.parse(readFileSync(stub.out, "utf8"));
  assert.deepEqual(argv, [
    "-a",
    "opencode",
    "-u",
    "low",
    "-h",
    "string:x-dunst-stack-tag:opencode-s1-done",
    "--",
    "Готово",
    "Моя сессия",
  ]);
  rmSync(dir, { recursive: true, force: true });
});

// --- инъекция ---------------------------------------------------------------

test("argv не выполняет подстановку: файлы не создаются", async () => {
  const markers = ["/tmp/HUDBAR_INJECTED"];
  for (const marker of markers) rmSync(marker, { force: true });
  const dir = mkdtempSync(join(tmpdir(), "hudbar-stub-"));
  const stub = stubNotifySend(dir);
  for (const title of EVIL_TITLES) {
    // Ровно тот путь, который использует плагин: массив аргументов.
    const result = await deliver(title, "тело", {
      program: stub.program,
      tag: "inject-test",
    });
    assert.equal(result.ok, true);
  }
  // Заголовок дошёл до процесса как есть, то есть был передан буквально.
  const argv = JSON.parse(readFileSync(stub.out, "utf8"));
  assert.equal(argv[argv.length - 2], EVIL_TITLES[1]);
  for (const marker of markers) {
    assert.equal(existsSync(marker), false, `инъекция создала ${marker}`);
  }
  rmSync(dir, { recursive: true, force: true });
});

test("опасность реальна: sh -c подстановку выполняет", async () => {
  // Фиксируем, от чего мы защищаемся. Если бы через shell ушёл заголовок,
  // файл появился бы.
  const marker = "/tmp/HUDBAR_SHELL_RISK_BASELINE";
  rmSync(marker, { force: true });
  const { execFileSync } = await import("node:child_process");
  execFileSync("sh", ["-c", `echo "$(touch ${marker})"`]);
  assert.equal(existsSync(marker), true, "через shell файл создаётся — риск настоящий");
  rmSync(marker, { force: true });
});

test("заголовок с минусом не становится опцией: доходит как аргумент", async () => {
  const dir = mkdtempSync(join(tmpdir(), "hudbar-stub-"));
  const stub = stubNotifySend(dir);
  await deliver("-h", "тело", { program: stub.program, tag: "dash-test" });
  const argv = JSON.parse(readFileSync(stub.out, "utf8"));
  // "-h" остался заголовком, а не флагом: дальше идёт tag, потом --, потом тело.
  const dashIndex = argv.indexOf("--");
  assert.equal(argv[dashIndex + 1], "-h");
  assert.equal(argv[dashIndex + 2], "тело");
  // Два вхождения "-h": наш hint-флаг и заголовок. Главное, что заголовок
  // стоит после "--", то есть notify-send прочитает его как позиционный.
  assert.equal(argv.filter((a) => a === "-h").length, 2);
  rmSync(dir, { recursive: true, force: true });
});

// --- отказы не бросают ------------------------------------------------------

test("ENOENT перехватывается и логируется", async () => {
  const entries = [];
  const originalPath = process.env.PATH;
  process.env.PATH = "/nonexistent-bin";
  try {
    const result = await deliver("t", "b", { log: (entry) => entries.push(entry) });
    assert.equal(result.ok, false);
    assert.equal(result.reason, "enoent");
    assert.ok(entries.some((e) => e.message.includes("notify-send не найден")));
  } finally {
    process.env.PATH = originalPath;
  }
});

test("ненулевой код выхода не бросает, а попадает в лог", async () => {
  const dir = mkdtempSync(join(tmpdir(), "hudbar-stub-"));
  const failing = join(dir, "fail");
  writeFileSync(failing, "#!/usr/bin/env node\nprocess.exit(3);\n", "utf8");
  chmodSync(failing, 0o755);
  const entries = [];
  const result = await deliver("t", "b", {
    program: failing,
    log: (entry) => entries.push(entry),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 3);
  assert.ok(entries.some((e) => e.level === "warn"));
  rmSync(dir, { recursive: true, force: true });
});

test("deliver всегда резолвится, даже если всё плохо", async () => {
  const result = await Promise.race([
    deliver("t", "b", { program: "/nonexistent/program" }),
    new Promise((resolve) => setTimeout(() => resolve({ ok: false, reason: "hang" }), 5000)),
  ]);
  assert.notEqual(result.reason, "hang");
});