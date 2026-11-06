"use strict";
// Локализация интерфейса. Ключ перевода — русский текст (русский — язык по умолчанию), словари —
// ui/i18n/<язык>.js: I18N_DICTS.en["Библиотека"] = "Library". Подстановки — {0}, {1}…; множественное число —
// plural(n, "трек", "трека", "треков"): в словаре по первой форме лежит пара [одна, много].
// Статичная разметка index.html переводится сама (applyStatic) — атрибуты в HTML не нужны.

window.I18N_DICTS = window.I18N_DICTS || {};

const I18N = {
  lang: "ru",
  /** для дат и чисел (toLocaleDateString и т. п.) */
  get locale() {
    return this.lang === "en" ? "en-GB" : "ru-RU";
  },
};

/** Перевод строки: L("Скачано: {0}", name). Нет перевода — русский текст. */
function L(src, ...args) {
  const d = I18N.lang === "ru" ? null : I18N_DICTS[I18N.lang];
  let s = (d && d[src]) || src;
  if (Array.isArray(s)) s = s[0];
  return args.length ? s.replace(/\{(\d+)\}/g, (m, i) => (args[i] == null ? "" : String(args[i]))) : s;
}

/**
 * Перевод с уточнением — когда один русский текст переводится по-разному: «Свернуть» окно — Minimize,
 * а боковую панель — Collapse. Ключ словаря — «уточнение|текст»; его нет — как L(src).
 */
function Lc(ctx, src) {
  const d = I18N.lang === "ru" ? null : I18N_DICTS[I18N.lang];
  const s = d && d[ctx + "|" + src];
  return typeof s === "string" ? s : L(src);
}

/** Множественное число: русские три формы; в английском словаре по первой форме — [одна, много]. */
function plural(n, one, few, many) {
  if (I18N.lang !== "ru") {
    const d = I18N_DICTS[I18N.lang] || {};
    const f = d[one];
    if (Array.isArray(f)) return Math.abs(n) === 1 ? f[0] : f[1];
    if (typeof f === "string") return f;
  }
  const n10 = n % 10, n100 = n % 100;
  if (n10 === 1 && n100 !== 11) return one;
  if (n10 >= 2 && n10 <= 4 && (n100 < 12 || n100 > 14)) return few;
  return many;
}

/** Исходные (русские) тексты статичной разметки: текстовые узлы и подсказки. Собираются один раз. */
let staticTexts = null;
const STATIC_ATTRS = ["title", "placeholder", "aria-label"];

function collectStatic() {
  staticTexts = [];
  const cyr = /[а-яё]/i;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const src = n.nodeValue.trim();
    if (src && cyr.test(src)) staticTexts.push({ node: n, src });
  }
  for (const e of document.body.querySelectorAll("*")) {
    for (const a of STATIC_ATTRS) {
      const v = e.getAttribute(a);
      if (v && cyr.test(v)) staticTexts.push({ el: e, attr: a, src: v });
    }
  }
}

/** Перевести статичную разметку на текущий язык (при запуске и смене языка). */
function applyStatic() {
  if (!staticTexts) collectStatic();
  for (const x of staticTexts) {
    if (x.node) x.node.nodeValue = L(x.src);
    else x.el.setAttribute(x.attr, L(x.src));
  }
  document.documentElement.lang = I18N.lang;
}

function setLang(lang) {
  I18N.lang = lang === "en" ? "en" : "ru";
  applyStatic();
}
