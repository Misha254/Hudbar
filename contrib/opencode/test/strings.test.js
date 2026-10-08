// Разметка, усечение, таблица строк.

import test from "node:test";
import assert from "node:assert/strict";
import {
  STRINGS,
  TITLE_MAX,
  escapeMarkup,
  formatQuestionText,
  formatSessionTitle,
  markSubagent,
  stackTag,
  titleFor,
  truncate,
} from "../lib/strings.js";

test("экранирует три спецсимвола разметки", () => {
  assert.equal(escapeMarkup("a & b < c > d"), "a &amp; b &lt; c &gt; d");
});

test("амперсанд экранируется первым, иначе получилось бы &amp;lt;", () => {
  assert.equal(escapeMarkup("<"), "&lt;");
  assert.equal(escapeMarkup("&lt;"), "&amp;lt;");
});

test("escapeMarkup терпит null и undefined", () => {
  assert.equal(escapeMarkup(null), "");
  assert.equal(escapeMarkup(undefined), "");
  assert.equal(escapeMarkup(42), "42");
});

test("усечение не ломает суррогатные пары", () => {
  // Эмодзи — два code unit; режем по code point, а не по UTF-16.
  const emoji = "😀😀😀";
  const cut = truncate(emoji, 2);
  assert.equal(Array.from(cut).length, 2);
  assert.ok(!cut.includes("\uFFFD"));
});

test("усечение короче лимита не трогает строку", () => {
  assert.equal(truncate("коротко", 80), "коротко");
});

test("усечение обрезает и добавляет многоточие", () => {
  const long = "я".repeat(200);
  const out = truncate(long, TITLE_MAX);
  assert.equal(Array.from(out).length, TITLE_MAX);
  assert.ok(out.endsWith("…"));
});

test("тело из заголовка сессии экранируется и усекается", () => {
  const evil = "a & b <script> " + "я".repeat(200);
  const out = formatSessionTitle(evil);
  assert.ok(out.includes("&amp;"));
  assert.ok(out.includes("&lt;script&gt;"));
  assert.ok(!out.includes("<script>"));
  // Лимит относится к видимому тексту: экранирование удлиняет строку,
  // поэтому считаем символы после обратного преобразования.
  const visible = out
    .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
  assert.equal(Array.from(visible).length, TITLE_MAX);
});

test("пустой заголовок даёт '(без названия)'", () => {
  assert.equal(formatSessionTitle(""), escapeMarkup(STRINGS.untitled));
  assert.equal(formatSessionTitle(null), escapeMarkup(STRINGS.untitled));
  assert.equal(formatSessionTitle("   "), escapeMarkup(STRINGS.untitled));
});

test("текст вопроса схлопывает переводы строк", () => {
  const out = formatQuestionText("первая\nвторая\tтретья");
  assert.equal(out, "первая вторая третья");
});

test("пустой вопрос даёт запасной текст", () => {
  assert.equal(formatQuestionText(""), escapeMarkup(STRINGS.fallbackQuestion));
});

test("заголовки берутся из таблицы", () => {
  assert.equal(titleFor("done"), STRINGS.titles.done);
  assert.equal(titleFor("permission"), STRINGS.titles.permission);
  assert.equal(titleFor("question"), STRINGS.titles.question);
  assert.equal(titleFor("error"), STRINGS.titles.error);
});

test("субагент помечается, основная сессия — нет", () => {
  assert.equal(markSubagent("тело", false), "тело");
  assert.equal(markSubagent("тело", true), "тело (субагент)");
});

test("субагент без текста не получает пустой префикс", () => {
  const out = markSubagent(escapeMarkup(STRINGS.untitled), true);
  assert.equal(out, STRINGS.subagentSuffix);
});

test("тег однозначный на пару сессия+вид", () => {
  assert.equal(stackTag("ses_1", "done"), "opencode-ses_1-done");
  assert.equal(stackTag("ses_1", "permission"), "opencode-ses_1-permission");
});

test("все строки русские, пустых нет", () => {
  for (const value of Object.values(STRINGS.titles)) {
    assert.ok(value.length > 0);
    assert.ok(/[а-яА-Я]/.test(value), `ожидался русский текст: ${value}`);
  }
});