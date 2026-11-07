"use strict";
// Расположение интерфейса (как layout.js у Whisper Yandex, без закреплённых — у VK Music их нет): сторона и вид
// боковой панели, вид нижней панели, показ времени трека и кружка на полосе трека, размер панелей перетаскиванием за край.
// Подключён в <head> без defer: атрибуты ставятся на <html> до отрисовки — без «прыжка» при запуске.
// Внешний вид режимов — в layout.css. Карточку в «Настройки → Общее» рисует Layout.card (из app.js).
(() => {
  const KEY = "vkm_layout";
  const OPTS = { side: ["left", "right"], sb: ["full", "mini"], pl: ["full", "mini"], time: ["always", "hover"], dot: ["on", "off"] };
  // sw — ширина боковой панели, ph — высота нижней (px, перетаскиванием за край); 0 — размер по умолчанию
  // dot — кружок на полосе трека (.seekdot в app.css / app.js)
  const ly = { side: "left", sb: "full", pl: "full", time: "always", dot: "on", sw: 0, ph: 0 };
  const SW_MIN = 170, SW_MAX = 440, PH_GROW = 160;
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    for (const k of Object.keys(OPTS)) if (OPTS[k].includes(v?.[k])) ly[k] = v[k];
    if (Number.isFinite(v?.sw) && v.sw >= SW_MIN && v.sw <= SW_MAX) ly.sw = Math.round(v.sw);
    if (Number.isFinite(v?.ph) && v.ph > 0 && v.ph <= 600) ly.ph = Math.round(v.ph);
  } catch {}
  const persist = () => { try { localStorage.setItem(KEY, JSON.stringify(ly)); } catch {} };
  // тема задаёт расположение сама (settings whisper-music → "layout": "fixed", как у Whisper Yandex): действуют
  // значения по умолчанию, свои — не теряются и вернутся с другим стилем
  const DEF = { side: "left", sb: "full", pl: "full", time: "always", dot: "on" };
  let locked = false;
  const eff = (k) => (locked ? DEF[k] : ly[k]);
  const root = document.documentElement;
  const T = (s) => (typeof L === "function" ? L(s) : s);
  const Tc = (ctx, s) => (typeof Lc === "function" ? Lc(ctx, s) : s);

  // Перетаскивание края зовёт apply() на каждом шаге, поэтому оно трогает только то, что изменилось: атрибуты <html>
  // ставятся, только если значение другое, а размер (--ly-sw / --ly-ph) пишется на .app, не на <html>, и эти
  // переменные ненаследуемые (@property в layout.css). Раньше переменная в style корня на каждом шаге пересчитывала
  // стили всего документа — с большой библиотекой ~120 мс на шаг, панель тянулась рывками (2026-11-07).
  const attr = (k, v) => {
    if (v == null) { if (root.hasAttribute(k)) root.removeAttribute(k); }
    else if (root.getAttribute(k) !== v) root.setAttribute(k, v);
  };
  const size = (app, k, v) => {
    if (!app) return;
    if (v == null) { if (app.style.getPropertyValue(k)) app.style.removeProperty(k); }
    else if (app.style.getPropertyValue(k) !== v) app.style.setProperty(k, v);
  };
  function apply() {
    for (const k of Object.keys(OPTS)) attr("data-ly-" + k, eff(k));
    const useSw = !locked && ly.sw > 0 && ly.sb !== "mini";
    const usePh = !locked && ly.ph > 0 && ly.pl !== "mini";
    const app = document.querySelector(".app");
    size(app, "--ly-sw", useSw ? ly.sw + "px" : null);
    size(app, "--ly-ph", usePh ? ly.ph + "px" : null);
    attr("data-ly-sw", useSw ? "" : null);
    attr("data-ly-ph", usePh ? "" : null);
    document.querySelectorAll(".lyhandle").forEach((h) => (h.hidden = locked));
    const card = document.getElementById("layoutCard");
    if (card) {
      card.classList.toggle("lylock", locked);
      const note = card.querySelector(".lylock-note");
      if (note) note.hidden = !locked;
    }
    const fold = document.getElementById("sideFold");
    if (fold) {
      fold.hidden = locked;
      const text = Tc("панель", "Свернуть");
      if (fold.textContent !== text) fold.textContent = text;
      fold.setAttribute("aria-label", ly.sb === "mini" ? T("Развернуть панель") : T("Свернуть панель"));
      fold.setAttribute("aria-expanded", ly.sb === "mini" ? "false" : "true");
    }
    // ширину боковой панели меряет ResizeObserver (init); без него — здесь
    if (!("ResizeObserver" in window)) measureSide();
    document.querySelectorAll(".lyseg[data-k]").forEach((g) => {
      g.querySelectorAll(".chip").forEach((c) => {
        const on = c.dataset.v === ly[g.dataset.k];
        c.classList.toggle("active", on);
        c.setAttribute("aria-pressed", on ? "true" : "false");
      });
    });
  }
  apply();

  function set(k, v) {
    if (!OPTS[k]?.includes(v) || ly[k] === v) return;
    ly[k] = v;
    persist();
    apply();
    window.dispatchEvent(new Event("resize"));
  }

  // ---------- размер панелей перетаскиванием за край ----------
  // Боковая: уже SNAP_SW — сворачивается в полосу (мини-режим), из полосы тянется обратно, шире — до SW_MAX.
  // Нижняя: ниже обычной высоты на SNAP_PH — становится однострочной (мини-режим), выше — растёт до +PH_GROW.
  // Двойной щелчок по краю возвращает размер по умолчанию.
  const SNAP_SW = 130, SNAP_PH = 28;
  let rzFrame = 0;
  const changed = () => {
    apply();
    if (!rzFrame) rzFrame = requestAnimationFrame(() => { rzFrame = 0; window.dispatchEvent(new Event("resize")); });
  };
  function dragger(h, axis, onStart, onMove) {
    h.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      h.setPointerCapture(e.pointerId);
      const st = onStart(e);
      h.classList.add("drag");
      // ширина боковой панели меняет ширину списка — браузер раскладывал бы заново все строки библиотеки (тысячи) на
      // каждом шаге. На время перетаскивания строки вне экрана пропускаются (lyfast: content-visibility в layout.css).
      // Не у списков со счётчиком (номера треков «Стимпанка»): content-visibility замыкает счётчик в строке — все «1»
      const li = axis === "x" && document.querySelector(".list li");
      const fast = li && getComputedStyle(li).counterIncrement === "none";
      root.classList.add("lyresizing", "lyresizing-" + axis, ...(fast ? ["lyfast"] : []));
      const move = (ev) => onMove(st, ev);
      const up = () => {
        h.removeEventListener("pointermove", move);
        h.removeEventListener("pointerup", up);
        h.removeEventListener("pointercancel", up);
        h.classList.remove("drag");
        root.classList.remove("lyresizing", "lyresizing-" + axis, "lyfast");
        persist();
        window.dispatchEvent(new Event("resize"));
      };
      h.addEventListener("pointermove", move);
      h.addEventListener("pointerup", up);
      h.addEventListener("pointercancel", up);
    });
  }
  function initResize() {
    const app = document.querySelector(".app"), side = document.querySelector(".side"), player = document.querySelector(".player");
    if (!app || document.querySelector(".lyhandle")) return;
    const mk = (cls, tip) => {
      const h = document.createElement("div");
      h.className = "lyhandle " + cls;
      h.title = tip;
      h.setAttribute("aria-hidden", "true");
      app.append(h);
      return h;
    };
    if (side) {
      const h = mk("lyh-side", T("Потяните, чтобы изменить ширину панели. Двойной щелчок — обычная ширина"));
      dragger(h, "x", (e) => ({ x: e.clientX, w: side.getBoundingClientRect().width, dir: ly.side === "right" ? -1 : 1 }), (st, e) => {
        const w = st.w + st.dir * (e.clientX - st.x);
        if (w < SNAP_SW) { if (ly.sb !== "mini") { ly.sb = "mini"; changed(); } return; }
        const sw = Math.round(Math.min(SW_MAX, window.innerWidth * 0.45, Math.max(SW_MIN, w)));
        if (ly.sb !== "full" || ly.sw !== sw) { ly.sb = "full"; ly.sw = sw; changed(); }
      });
      h.addEventListener("dblclick", () => { ly.sw = 0; ly.sb = "full"; persist(); changed(); });
    }
    if (player) {
      const h = mk("lyh-pl", T("Потяните, чтобы изменить высоту панели. Двойной щелчок — обычная высота"));
      const natural = () => {
        const had = root.hasAttribute("data-ly-ph");
        if (had) root.removeAttribute("data-ly-ph");
        const v = player.getBoundingClientRect().height;
        if (had) root.setAttribute("data-ly-ph", "");
        return v;
      };
      dragger(h, "y", (e) => ({ y: e.clientY, h: player.getBoundingClientRect().height, nat: ly.pl === "full" ? natural() : 0 }), (st, e) => {
        const want = st.h + (st.y - e.clientY);
        const rebase = () => { st.y = e.clientY; st.h = player.getBoundingClientRect().height; };
        if (ly.pl === "mini") {
          if (want < st.h + SNAP_PH) return;
          ly.pl = "full";
          ly.ph = 0;
          apply();
          st.nat = natural();
          rebase();
          changed();
          return;
        }
        if (!st.nat) st.nat = natural();
        if (want < st.nat - SNAP_PH) { ly.pl = "mini"; ly.ph = 0; apply(); rebase(); changed(); return; }
        const ph = Math.round(Math.min(st.nat + PH_GROW, want));
        const next = ph > st.nat + 3 ? ph : 0;
        if (ly.ph !== next) { ly.ph = next; changed(); }
      });
      h.addEventListener("dblclick", () => { ly.ph = 0; ly.pl = "full"; persist(); changed(); });
    }
  }

  // узкая боковая панель: по настоящей ширине — data-ly-sbw = s | xs (layout.css ужимает кнопки у стилей со значками)
  function measureSide() {
    const side = document.querySelector(".side");
    if (!side) return;
    const w = side.getBoundingClientRect().width;
    const lvl = eff("sb") === "mini" || !w ? "" : w < 205 ? "xs" : w < 240 ? "s" : "";
    if (lvl) { if (root.getAttribute("data-ly-sbw") !== lvl) root.setAttribute("data-ly-sbw", lvl); }
    else root.removeAttribute("data-ly-sbw");
  }

  function init() {
    // поиск — в обёртку: в свёрнутой панели он кружок со значком и раскрывается поверх контента
    const s = document.getElementById("search");
    if (s && !s.parentElement.classList.contains("sbox")) {
      const box = document.createElement("div");
      box.className = "sbox";
      s.before(box);
      box.append(s);
    }
    // кнопка сворачивания прямо на панели — в самом низу
    const side = document.querySelector(".side");
    if (side && !document.getElementById("sideFold")) {
      const b = document.createElement("button");
      b.className = "nav sidefold";
      b.id = "sideFold";
      b.type = "button";
      b.addEventListener("click", () => set("sb", ly.sb === "mini" ? "full" : "mini"));
      side.append(b);
    }
    if (side && "ResizeObserver" in window) new ResizeObserver(() => measureSide()).observe(side);
    initResize();
    apply();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init, { once: true });
  else init();

  /** Карточка «Расположение» в настройках (app.js, renderGeneral). */
  function card(c) {
    c.id = "layoutCard";
    const ROWS = [
      ["side", "Боковая панель", [["left", "Слева"], ["right", "Справа"]]],
      ["sb", "Вид боковой панели", [["full", "Обычный"], ["mini", "Свёрнутый"]]],
      ["pl", "Нижняя панель", [["full", "Обычная"], ["mini", "Свёрнутая"]]],
      ["time", "Время трека", [["always", "Всегда"], ["hover", "При наведении"]]],
      ["dot", "Кружок на полосе трека", [["on", "Показывать"], ["off", "Скрывать"]]],
    ];
    for (const [k, label, opts] of ROWS) {
      const row = document.createElement("div");
      row.className = "lyrow";
      const name = document.createElement("span");
      name.textContent = T(label);
      const seg = document.createElement("div");
      seg.className = "lyseg";
      seg.dataset.k = k;
      for (const [v, text] of opts) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = "chip";
        b.dataset.v = v;
        b.textContent = T(text);
        b.addEventListener("click", () => set(k, v));
        seg.append(b);
      }
      row.append(name, seg);
      c.append(row);
    }
    const p = document.createElement("p");
    p.className = "status";
    p.textContent = T("Размер панелей меняется перетаскиванием за край.");
    p.className = "status lyrow-note";
    const lockNote = document.createElement("p");
    lockNote.className = "status lylock-note";
    lockNote.textContent = T("Расположение задаёт текущий стиль оформления.");
    c.append(p, lockNote);
    apply();
  }
  /** Тема задаёт расположение сама (applyHooks в app.js). */
  function lock(on) {
    if (locked === !!on) return;
    locked = !!on;
    apply();
    window.dispatchEvent(new Event("resize"));
  }
  window.Layout = { card, refresh: apply, lock };
})();
