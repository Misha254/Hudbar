// Таблица решений: idle, permission, question, error, субагент, без sessionID.

import test from "node:test";
import assert from "node:assert/strict";
import { decide, isQuestionEvent, firstQuestionText } from "../lib/decide.js";
import { PermissionMemory, Throttle } from "../lib/throttle.js";
import { STRINGS } from "../lib/strings.js";

/** Свежее состояние на каждый тест. */
function fresh() {
  return {
    busySessions: new Set(),
    permissions: new PermissionMemory(),
    throttle: new Throttle({ now: () => 1_000_000 }),
  };
}

const run = (state, event, extra = {}) =>
  decide({
    event,
    isSubagent: false,
    sessionTitle: "Моя сессия",
    ...state,
    ...extra,
  });

const idle = (sessionID = "s1") => ({
  type: "session.idle",
  properties: { sessionID },
});

const busy = (sessionID = "s1") => ({
  type: "session.status",
  properties: { sessionID, status: { type: "busy" } },
});

// --- session.status ---------------------------------------------------------

test("status busy помечает сессию и сам не уведомляет", () => {
  const state = fresh();
  const decision = run(state, busy("s1"));
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "marked-busy");
  assert.equal(state.busySessions.has("s1"), true);
});

test("status idle не помечает", () => {
  const state = fresh();
  const decision = run(state, {
    type: "session.status",
    properties: { sessionID: "s1", status: { type: "idle" } },
  });
  assert.equal(decision.reason, "status-not-busy");
  assert.equal(state.busySessions.has("s1"), false);
});

test("status без sessionID отбрасывается", () => {
  const state = fresh();
  const decision = run(state, {
    type: "session.status",
    properties: { status: { type: "busy" } },
  });
  assert.equal(decision.reason, "no-session-id");
});

// --- session.idle -----------------------------------------------------------

test("idle без предшествующего busy молчит", () => {
  const state = fresh();
  const decision = run(state, idle("s1"));
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "never-busy");
});

test("idle после busy уведомляет", () => {
  const state = fresh();
  run(state, busy("s1"));
  const decision = run(state, idle("s1"));
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "done");
  assert.equal(decision.urgency, "low");
  assert.equal(decision.title, STRINGS.titles.done);
  assert.equal(decision.body, "Моя сессия");
  assert.equal(decision.tag, "opencode-s1-done");
});

test("после уведомления busy сбрасывается, второй idle молчит", () => {
  const state = fresh();
  run(state, busy("s1"));
  assert.equal(run(state, idle("s1")).notify, true);
  assert.equal(run(state, idle("s1")).reason, "never-busy");
});

test("idle субагента молчит, но busy сбрасывается", () => {
  const state = fresh();
  run(state, busy("sub"));
  const decision = run(state, idle("sub"), { isSubagent: true });
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "subagent-silent");
  assert.equal(state.busySessions.has("sub"), false);
});

test("idle без sessionID отбрасывается", () => {
  const state = fresh();
  const decision = run(state, { type: "session.idle", properties: {} });
  assert.equal(decision.reason, "no-session-id");
});

// --- permission -------------------------------------------------------------

const permission = (id = "p1", sessionID = "s1", title = "bash: rm -rf") => ({
  type: "permission.updated",
  properties: { id, sessionID, title },
});

test("permission уведомляет critical", () => {
  const state = fresh();
  const decision = run(state, permission());
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "permission");
  assert.equal(decision.urgency, "critical");
  assert.equal(decision.title, STRINGS.titles.permission);
  assert.equal(decision.body, "bash: rm -rf");
});

test("повтор того же permission молчит", () => {
  const state = fresh();
  run(state, permission("p1"));
  const again = run(state, permission("p1"));
  assert.equal(again.notify, false);
  assert.equal(again.reason, "permission-dup");
});

test("permission.replied снимает дедуп", () => {
  const state = fresh();
  run(state, permission("p1"));
  const replied = run(state, {
    type: "permission.replied",
    properties: { sessionID: "s1", permissionID: "p1", response: "once" },
  });
  assert.equal(replied.reason, "permission-answered");
  assert.equal(state.permissions.has("p1"), false);
  // Троттлинг по паре снимаем вручную: тест про память разрешений, а не про 5 с.
  state.throttle.forget("s1", "permission");
  assert.equal(run(state, permission("p1")).notify, true);
});

test("permission субагента НЕ молчит, но помечается", () => {
  const state = fresh();
  const decision = run(state, permission("p1", "sub"), { isSubagent: true });
  assert.equal(decision.notify, true);
  assert.ok(decision.body.includes(STRINGS.subagentSuffix.trim()));
});

test("permission без id отбрасывается", () => {
  const state = fresh();
  const decision = run(state, {
    type: "permission.updated",
    properties: { sessionID: "s1" },
  });
  assert.equal(decision.reason, "no-permission-id");
});

// --- question ---------------------------------------------------------------

test("isQuestionEvent узнаёт question.asked и question.v2.asked", () => {
  assert.equal(isQuestionEvent("question.asked"), true);
  assert.equal(isQuestionEvent("question.v2.asked"), true);
  assert.equal(isQuestionEvent("question.replied"), false);
  assert.equal(isQuestionEvent("session.idle"), false);
  assert.equal(isQuestionEvent(undefined), false);
});

test("текст вопроса достаётся из questions[0]", () => {
  assert.equal(
    firstQuestionText({ questions: [{ question: "какой вариант?", header: "Выбор" }] }),
    "какой вариант?",
  );
  assert.equal(firstQuestionText({ questions: [{ header: "Только header" }] }), "Только header");
  assert.equal(firstQuestionText({}), "");
  assert.equal(firstQuestionText(null), "");
});

test("question.asked уведомляет critical с текстом вопроса", () => {
  const state = fresh();
  const decision = run(state, {
    type: "question.asked",
    properties: {
      id: "q1",
      sessionID: "s1",
      questions: [{ question: "Удалить ветку?", header: "Удалить" }],
    },
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "question");
  assert.equal(decision.urgency, "critical");
  assert.equal(decision.title, STRINGS.titles.question);
  assert.equal(decision.body, "Удалить ветку?");
});

test("question субагента помечается и не молчит", () => {
  const state = fresh();
  const decision = run(
    state,
    {
      type: "question.asked",
      properties: { id: "q1", sessionID: "sub", questions: [{ question: "А?" }] },
    },
    { isSubagent: true },
  );
  assert.equal(decision.notify, true);
  assert.ok(decision.body.includes("субагент"));
});

test("question.v2.asked тоже обрабатывается", () => {
  const state = fresh();
  const decision = run(state, {
    type: "question.v2.asked",
    properties: { id: "q2", sessionID: "s1", questions: [{ question: "Что дальше?" }] },
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.body, "Что дальше?");
});

// --- session.error ----------------------------------------------------------

test("error уведомляет critical", () => {
  const state = fresh();
  const decision = run(state, {
    type: "session.error",
    properties: { sessionID: "s1", error: { name: "ApiError" } },
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "error");
  assert.equal(decision.urgency, "critical");
  assert.equal(decision.body, "Моя сессия");
});

test("error без sessionID даёт общий текст и всё равно уведомляет", () => {
  const state = fresh();
  const decision = run(state, {
    type: "session.error",
    properties: {},
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.sessionID, "");
  assert.equal(decision.body, STRINGS.errorWithoutSession);
});

test("error субагента молчит", () => {
  const state = fresh();
  const decision = run(
    state,
    { type: "session.error", properties: { sessionID: "sub" } },
    { isSubagent: true },
  );
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "subagent-silent");
});

test("error не проходит под глобальный лимит done", () => {
  const clock = { t: 1_000_000 };
  const state = {
    busySessions: new Set(),
    permissions: new PermissionMemory(),
    throttle: new Throttle({ now: () => clock.t }),
  };
  for (const s of ["a", "b", "c"]) state.throttle.check(s, "done");
  clock.t += 6000;
  const decision = decide({
    event: { type: "session.error", properties: { sessionID: "s4" } },
    isSubagent: false,
    sessionTitle: "тест",
    ...state,
  });
  assert.equal(decision.notify, true);
});

// --- прочее -----------------------------------------------------------------

test("неизвестный тип молчит", () => {
  const state = fresh();
  assert.equal(run(state, { type: "file.edited", properties: {} }).reason, "unhandled");
});

test("событие без type молчит", () => {
  const state = fresh();
  assert.equal(run(state, {}).reason, "no-type");
});

test("опасный заголовок сессии экранируется в теле", () => {
  const state = fresh();
  // Заголовок сессии приходит отдельно от события: сессия busy, а текст
  // подставляется при разборе idle.
  run(state, busy("s1"));
  const out = run(state, idle("s1"), { sessionTitle: "<b>&жирно</b>" });
  assert.equal(out.body, "&lt;b&gt;&amp;жирно&lt;/b&gt;");
  assert.ok(!out.body.includes("<b>"));
});