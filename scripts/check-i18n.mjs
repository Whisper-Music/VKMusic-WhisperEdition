// Проверка переводов: каждая строка интерфейса — L("…"), Lc("уточнение", "…"), plural(n, "…", …) в src/*.js и
// русский текст статичной разметки index.html (текст и title / placeholder / aria-label, как applyStatic) —
// есть в src/i18n/en.js. Запуск: node scripts/check-i18n.mjs (из корня; код выхода 1 — есть непереведённые).
// Учитываются только строковые литералы в двойных кавычках: L(PAGES[page]) и подобное проверить так нельзя.
import { readFileSync, readdirSync } from "node:fs";
import vm from "node:vm";

const root = new URL("../", import.meta.url);
const read = (p) => readFileSync(new URL(p, root), "utf8");

const ctx = { window: {} };
vm.createContext(ctx);
vm.runInContext(read("src/i18n/en.js"), ctx);
const en = ctx.window.I18N_DICTS.en;

const missing = new Map(); // ключ → где встретился
const need = (key, where) => {
  if (!(key in en) && !missing.has(key)) missing.set(key, where);
};
const cyr = /[а-яё]/i;
// строковый литерал в двойных кавычках (с экранированием)
const STR = String.raw`"((?:[^"\\\n]|\\.)*)"`;
const unq = (s) => JSON.parse(`"${s}"`);

const own = readdirSync(new URL("src/", root)).filter((f) => f.endsWith(".js") && f !== "host.js");
for (const f of own) {
  const src = read("src/" + f);
  const line = (i) => `src/${f}:${src.slice(0, i).split("\n").length}`;
  // L("начало " + "продолжение") — строка, склеенная из литералов
  const CHAIN = String.raw`((?:"(?:[^"\\\n]|\\.)*"\s*\+\s*)*"(?:[^"\\\n]|\\.)*")`;
  for (const m of src.matchAll(new RegExp(String.raw`(?<![\w.])L\(\s*` + CHAIN, "g"))) {
    const k = [...m[1].matchAll(new RegExp(STR, "g"))].map((x) => unq(x[1])).join("");
    if (cyr.test(k)) need(k, line(m.index));
  }
  for (const m of src.matchAll(new RegExp(String.raw`(?<![\w.])Lc\(\s*` + STR + String.raw`\s*,\s*` + STR, "g"))) {
    const k = unq(m[2]);
    if (!(`${unq(m[1])}|${k}` in en)) need(k, line(m.index));
  }
  for (const m of src.matchAll(new RegExp(String.raw`(?<![\w.])plural\([^,]+,\s*` + STR, "g"))) {
    const k = unq(m[1]);
    if (!Array.isArray(en[k])) need(k, line(m.index) + " (plural: нужна пара [одна, много])");
  }
}

// статичная разметка: тексты между тегами и атрибуты подсказок
const html = read("src/index.html").replace(/<script[\s\S]*?<\/script>|<style[\s\S]*?<\/style>|<!--[\s\S]*?-->/g, "");
const ent = (s) => s.replace(/&nbsp;/g, " ").replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"');
const body = html.slice(html.indexOf("<body"));
for (const m of body.matchAll(/>([^<]+)</g)) {
  const t = ent(m[1]).trim();
  if (t && cyr.test(t)) need(t, "src/index.html");
}
for (const m of body.matchAll(/\s(?:title|placeholder|aria-label)="([^"]*)"/g)) {
  const t = ent(m[1]);
  if (cyr.test(t)) need(t, "src/index.html");
}

// один ключ дважды — второй молча перекрывает первый
const seen = new Set();
const dups = [];
for (const m of read("src/i18n/en.js").matchAll(/^\s*("(?:[^"\\]|\\.)*")\s*:/gm)) {
  const k = JSON.parse(m[1]);
  if (seen.has(k)) dups.push(k);
  seen.add(k);
}
if (dups.length) {
  console.error(`Повторяются ключи в src/i18n/en.js: ${dups.map((k) => JSON.stringify(k)).join(", ")}`);
  process.exitCode = 1;
}

if (missing.size) {
  console.error(`Нет в src/i18n/en.js (${missing.size}):`);
  for (const [k, where] of missing) console.error(`  ${JSON.stringify(k)}  — ${where}`);
  process.exit(1);
}
if (!process.exitCode) console.log("Переводы: всё на месте");
