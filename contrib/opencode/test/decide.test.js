// Таблица решений: конец работы, разрешения, вопросы-формы, ошибки, субагент.

import test from "node:test";
import assert from "node:assert/strict";
import {
  decide,
  firstQuestionText,
  isQuestionEvent,
  payloadOf,
  sessionIdOf,
} from "../lib/decide.js";
import { PermissionMemory, Throttle } from "../lib/throttle.js";
import { STRINGS } from "../lib/strings.js";

/** Свежее состояние на каждый тест. */
function fresh() {
  return {
    busySessions: new Set(),
    permissions: new PermissionMemory(),
    forms: new PermissionMemory(),
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

const started = (sessionID = "s1") => ({
  type: "session.execution.started",
  data: { sessionID },
});

const done = (sessionID = "s1") => ({
  type: "session.execution.succeeded",
  data: { sessionID },
});

const busy = (sessionID = "s1") => ({
  type: "session.status",
  data: { sessionID, status: { type: "busy" } },
});

const idle = (sessionID = "s1") => ({ type: "session.idle", data: { sessionID } });

// --- полезная нагрузка -------------------------------------------------------

test("payloadOf берёт data, а на старых событиях properties", () => {
  assert.deepEqual(payloadOf({ type: "x", data: { a: 1 } }), { a: 1 });
  assert.deepEqual(payloadOf({ type: "x", properties: { b: 2 } }), { b: 2 });
  assert.deepEqual(payloadOf({}), {});
  assert.deepEqual(payloadOf(null), {});
});

test("sessionIdOf у question берёт сессию из формы, у permission — не из id", () => {
  assert.equal(sessionIdOf({ type: "form.created", data: { form: { sessionID: "s9" } } }), "s9");
  assert.equal(
    sessionIdOf({ type: "permission.asked", data: { id: "prt_1", sessionID: "s1" } }),
    "s1",
  );
  // У разрешения id — это id разрешения, сессии там может не быть вовсе.
  assert.equal(sessionIdOf({ type: "permission.asked", data: { id: "prt_1" } }), "");
  assert.equal(sessionIdOf({ type: "session.created", data: { id: "ses_1" } }), "ses_1");
});

// --- начало работы -----------------------------------------------------------

test("session.execution.started помечает сессию и сам не уведомляет", () => {
  const state = fresh();
  const decision = run(state, started("s1"));
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "marked-busy");
  assert.equal(state.busySessions.has("s1"), true);
});

test("session.execution.started без sessionID отбрасывается", () => {
  const state = fresh();
  assert.equal(run(state, started("")).reason, "no-session-id");
});

// --- конец работы ------------------------------------------------------------

test("конец работы без предшествующего старта молчит", () => {
  const state = fresh();
  const decision = run(state, done("s1"));
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "never-busy");
});

test("конец работы после старта уведомляет", () => {
  const state = fresh();
  run(state, started("s1"));
  const decision = run(state, done("s1"));
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "done");
  assert.equal(decision.urgency, "low");
  assert.equal(decision.title, STRINGS.titles.done);
  assert.equal(decision.body, "Моя сессия");
  assert.equal(decision.tag, "opencode-s1-done");
});

test("прерывание тоже считается концом работы", () => {
  const state = fresh();
  run(state, started("s1"));
  const decision = run(state, {
    type: "session.execution.interrupted",
    data: { sessionID: "s1", reason: "user" },
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "done");
});

test("после уведомления busy сбрасывается, второй конец молчит", () => {
  const state = fresh();
  run(state, started("s1"));
  assert.equal(run(state, done("s1")).notify, true);
  assert.equal(run(state, done("s1")).reason, "never-busy");
});

test("succeeded и idle от одной сессии не дают двух уведомлений", () => {
  const state = fresh();
  run(state, started("s1"));
  assert.equal(run(state, done("s1")).notify, true);
  assert.equal(run(state, idle("s1")).reason, "never-busy");
});

test("конец работы субагента молчит, но busy сбрасывается", () => {
  const state = fresh();
  run(state, started("sub"));
  const decision = run(state, done("sub"), { isSubagent: true });
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "subagent-silent");
  assert.equal(state.busySessions.has("sub"), false);
});

test("конец работы без sessionID отбрасывается", () => {
  assert.equal(run(fresh(), { type: "session.idle", data: {} }).reason, "no-session-id");
});

// --- session.status ----------------------------------------------------------

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
    data: { sessionID: "s1", status: { type: "idle" } },
  });
  assert.equal(decision.reason, "status-not-busy");
  assert.equal(state.busySessions.has("s1"), false);
});

test("status без sessionID отбрасывается", () => {
  const decision = run(fresh(), {
    type: "session.status",
    data: { status: { type: "busy" } },
  });
  assert.equal(decision.reason, "no-session-id");
});

// --- permission --------------------------------------------------------------

const permission = (id = "prt_1", sessionID = "s1", message = "bash: rm -rf") => ({
  type: "permission.asked",
  data: { id, sessionID, action: "bash", resources: ["rm -rf"], message },
});

test("permission уведомляет critical", () => {
  const decision = run(fresh(), permission());
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "permission");
  assert.equal(decision.urgency, "critical");
  assert.equal(decision.title, STRINGS.titles.permission);
  assert.equal(decision.body, "bash: rm -rf");
});

test("повтор того же permission молчит", () => {
  const state = fresh();
  run(state, permission("prt_1"));
  const again = run(state, permission("prt_1"));
  assert.equal(again.notify, false);
  assert.equal(again.reason, "permission-dup");
});

test("permission.replied снимает дедуп по requestID", () => {
  const state = fresh();
  run(state, permission("prt_1"));
  const replied = run(state, {
    type: "permission.replied",
    data: { sessionID: "s1", requestID: "prt_1", reply: "once" },
  });
  assert.equal(replied.reason, "permission-answered");
  assert.equal(state.permissions.has("prt_1"), false);
  // Троттлинг по паре снимаем вручную: тест про память разрешений, а не про 5 с.
  state.throttle.forget("s1", "permission");
  assert.equal(run(state, permission("prt_1")).notify, true);
});

test("permission субагента НЕ молчит, но помечается", () => {
  const decision = run(fresh(), permission("prt_1", "sub"), { isSubagent: true });
  assert.equal(decision.notify, true);
  assert.ok(decision.body.includes(STRINGS.subagentSuffix.trim()));
});

test("permission без id отбрасывается", () => {
  const decision = run(fresh(), {
    type: "permission.asked",
    data: { sessionID: "s1" },
  });
  assert.equal(decision.reason, "no-permission-id");
});

test("permission без message показывает заголовок сессии", () => {
  const decision = run(fresh(), {
    type: "permission.asked",
    data: { id: "prt_1", sessionID: "s1" },
  });
  assert.equal(decision.body, "Моя сессия");
});

// --- вопросы (form.created) --------------------------------------------------

const questionForm = (sessionID = "s1", extra = {}) => ({
  type: "form.created",
  data: {
    form: {
      id: "frm_1",
      sessionID,
      title: "Questions",
      metadata: { kind: "question" },
      fields: [{ key: "q0", title: "Заголовок", description: "Сам вопрос?" }],
      ...extra,
    },
  },
});

test("isQuestionEvent узнаёт question-форму и старую question.asked", () => {
  assert.equal(isQuestionEvent("form.created", questionForm().data), true);
  assert.equal(isQuestionEvent("question.asked"), true);
  assert.equal(isQuestionEvent("question.v2.asked"), true);
  assert.equal(isQuestionEvent("form.replied"), false);
  assert.equal(isQuestionEvent("session.idle"), false);
  assert.equal(isQuestionEvent(undefined), false);
});

test("форма выбора провайдера веб-поиска вопросом НЕ считается", () => {
  // Ровно тот случай из жизни: opencode сам открывает форму «Web Search» с
  // выбором провайдера, никакого вопроса агента при этом нет.
  const payload = {
    form: {
      id: "frm_2",
      sessionID: "s1",
      title: "Web Search",
      metadata: { kind: "websearch.provider" },
      fields: [
        { key: "choice", description: "Allow OpenCode to search the web?", type: "string" },
      ],
    },
  };
  assert.equal(isQuestionEvent("form.created", payload), false);
  const decision = run(fresh(), { type: "form.created", data: payload });
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "unhandled");
});

test("форма MCP-элицитации вопросом НЕ считается", () => {
  const payload = {
    form: {
      id: "frm_3",
      sessionID: "s1",
      title: "weather is requesting input",
      metadata: { kind: "mcp-elicitation", server: "weather" },
      fields: [{ key: "city", description: "Какой город?", type: "string" }],
    },
  };
  assert.equal(isQuestionEvent("form.created", payload), false);
  assert.equal(run(fresh(), { type: "form.created", data: payload }).notify, false);
});

test("форма без metadata с полями считается вопросом", () => {
  const payload = { form: { id: "frm_4", sessionID: "s1", fields: [{ title: "А?" }] } };
  assert.equal(isQuestionEvent("form.created", payload), true);
});

test("форма без metadata и без полей вопросом не считается", () => {
  assert.equal(isQuestionEvent("form.created", { form: { id: "frm_5" } }), false);
  assert.equal(isQuestionEvent("form.created", {}), false);
  assert.equal(isQuestionEvent("form.created", undefined), false);
});

test("текст вопроса берётся из поля, а не из заголовка формы", () => {
  // Заголовок формы у вопросов всегда служебный «Questions», брать его нельзя.
  assert.equal(firstQuestionText(questionForm().data), "Сам вопрос?");
  assert.equal(
    firstQuestionText({
      form: { title: "Questions", fields: [{ title: "Только title" }] },
    }),
    "Только title",
  );
  assert.equal(firstQuestionText({ form: { fields: [] } }), "");
  assert.equal(firstQuestionText({ form: { title: "Пустой вопрос" } }), "");
  assert.equal(firstQuestionText({}), "");
  assert.equal(firstQuestionText(null), "");
});

test("текст вопроса из старых событий questions[] не потерян", () => {
  assert.equal(
    firstQuestionText({ questions: [{ question: "какой вариант?", header: "Выбор" }] }),
    "какой вариант?",
  );
  assert.equal(firstQuestionText({ questions: [{ header: "Только header" }] }), "Только header");
});

test("вопрос агента уведомляет critical с текстом вопроса", () => {
  const decision = run(fresh(), questionForm());
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "question");
  assert.equal(decision.urgency, "critical");
  assert.equal(decision.title, STRINGS.titles.question);
  assert.equal(decision.body, "Сам вопрос?");
  assert.equal(decision.tag, "opencode-s1-question");
});

test("повтор той же формы молчит, form.replied снимает дедуп", () => {
  const state = fresh();
  assert.equal(run(state, questionForm()).notify, true);
  assert.equal(run(state, questionForm()).reason, "form-dup");
  const replied = run(state, {
    type: "form.replied",
    data: { id: "frm_1", sessionID: "s1" },
  });
  assert.equal(replied.reason, "form-answered");
  assert.equal(state.forms.has("frm_1"), false);
  state.throttle.forget("s1", "question");
  assert.equal(run(state, questionForm()).notify, true);
});

test("form.cancelled тоже снимает дедуп", () => {
  const state = fresh();
  run(state, questionForm());
  assert.equal(run(state, { type: "form.cancelled", data: { id: "frm_1" } }).reason, "form-answered");
  assert.equal(state.forms.has("frm_1"), false);
});

test("вопрос субагента помечается и не молчит", () => {
  const decision = run(fresh(), questionForm("sub"), { isSubagent: true });
  assert.equal(decision.notify, true);
  assert.ok(decision.body.includes("субагент"));
});

test("вопрос без текста показывает заголовок сессии", () => {
  const decision = run(fresh(), {
    type: "form.created",
    data: { form: { id: "frm_9", sessionID: "s1", title: "Questions", metadata: { kind: "question" }, fields: [] } },
  });
  // Заголовок формы тут служебный, в уведомление он бы не попал.
  assert.equal(decision.body, "Моя сессия");
});

test("форма без sessionID отбрасывается", () => {
  const decision = run(fresh(), {
    type: "form.created",
    data: { form: { id: "frm_1", title: "Questions", metadata: { kind: "question" } } },
  });
  assert.equal(decision.reason, "no-session-id");
});

// --- ошибки ------------------------------------------------------------------

test("session.error уведомляет critical", () => {
  const decision = run(fresh(), {
    type: "session.error",
    data: { sessionID: "s1", error: { name: "ApiError" } },
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "error");
  assert.equal(decision.urgency, "critical");
  assert.equal(decision.body, "Моя сессия");
});

test("session.execution.failed уведомляет так же", () => {
  const decision = run(fresh(), {
    type: "session.execution.failed",
    data: { sessionID: "s1", error: { name: "ProviderError", message: "429" } },
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.kind, "error");
});

test("ошибка без sessionID даёт общий текст и всё равно уведомляет", () => {
  const decision = run(fresh(), {
    type: "session.error",
    data: {},
  });
  assert.equal(decision.notify, true);
  assert.equal(decision.sessionID, "");
  assert.equal(decision.body, STRINGS.errorWithoutSession);
});

test("ошибка субагента молчит", () => {
  const decision = run(
    fresh(),
    { type: "session.error", data: { sessionID: "sub" } },
    { isSubagent: true },
  );
  assert.equal(decision.notify, false);
  assert.equal(decision.reason, "subagent-silent");
});

test("ошибка не проходит под глобальный лимит done", () => {
  const clock = { t: 1_000_000 };
  const state = {
    busySessions: new Set(),
    permissions: new PermissionMemory(),
    throttle: new Throttle({ now: () => clock.t }),
  };
  for (const s of ["a", "b", "c"]) state.throttle.check(s, "done");
  clock.t += 6000;
  const decision = decide({
    event: { type: "session.error", data: { sessionID: "s4" } },
    isSubagent: false,
    sessionTitle: "тест",
    ...state,
  });
  assert.equal(decision.notify, true);
});

test("таймауты жизни на экране различаются по важности", () => {
  // Критичные живут дольше, «готово» — мимо: его и так видно в окне.
  const state = fresh();
  run(state, started("s1"));
  assert.equal(run(state, done("s1")).timeoutMs, 8000);
  assert.equal(run(fresh(), permission()).timeoutMs, 20000);
  assert.equal(run(fresh(), questionForm()).timeoutMs, 20000);
  assert.equal(
    run(fresh(), { type: "session.error", data: { sessionID: "s1" } }).timeoutMs,
    20000,
  );
});

// --- прочее ------------------------------------------------------------------

test("неизвестный тип молчит", () => {
  assert.equal(run(fresh(), { type: "file.edited", data: {} }).reason, "unhandled");
});

test("событие без type молчит", () => {
  assert.equal(run(fresh(), {}).reason, "no-type");
});

test("опасный заголовок сессии экранируется в теле", () => {
  const state = fresh();
  // Заголовок сессии приходит отдельно от события: сессия работает, а текст
  // подставляется в момент конца работы.
  run(state, started("s1"));
  const out = run(state, done("s1"), { sessionTitle: "<b>&жирно</b>" });
  assert.equal(out.body, "&lt;b&gt;&amp;жирно&lt;/b&gt;");
  assert.ok(!out.body.includes("<b>"));
});