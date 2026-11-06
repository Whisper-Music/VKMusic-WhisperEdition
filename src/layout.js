"use strict";
// Расположение интерфейса (как layout.js у Whisper Yandex, без закреплённых — у VK Music их нет): сторона и вид
// боковой панели, вид нижней панели, показ времени трека, размер панелей перетаскиванием за край.
// Подключён в <head> без defer: атрибуты ставятся на <html> до отрисовки — без «прыжка» при запуске.
// Внешний вид режимов — в layout.css. Карточку в «Настройки → Общее» рисует Layout.card (из app.js).
(() => {
  const KEY = "vkm_layout";
  const OPTS = { side: ["left", "right"], sb: ["full", "mini"], pl: ["full", "mini"], time: ["always", "hover"] };
  // sw — ширина боковой панели, ph — высота нижней (px, перетаскиванием за край); 0 — размер по умолчанию
  const ly = { side: "left", sb: "full", pl: "full", time: "always", sw: 0, ph: 0 };
  const SW_MIN = 170, SW_MAX = 440, PH_GROW = 160;
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    for (const k of Object.keys(OPTS)) if (OPTS[k].includes(v?.[k])) ly[k] = v[k];
    if (Number.isFinite(v?.sw) && v.sw >= SW_MIN && v.sw <= SW_MAX) ly.sw = Math.round(v.sw);
    if (Number.isFinite(v?.ph) && v.ph > 0 && v.ph <= 600) ly.ph = Math.round(v.ph);
  } catch {}
  const persist = () => { try { localStorage.setItem(KEY, JSON.stringify(ly)); } catch {} };
  const root = document.documentElement;
  const T = (s) => (typeof L === "function" ? L(s) : s);
  const Tc = (ctx, s) => (typeof Lc === "function" ? Lc(ctx, s) : s);

  function apply() {
    for (const k of Object.keys(OPTS)) root.setAttribute("data-ly-" + k, ly[k]);
    const useSw = ly.sw > 0 && ly.sb !== "mini";
    const usePh = ly.ph > 0 && ly.pl !== "mini";
    if (useSw) { root.style.setProperty("--ly-sw", ly.sw + "px"); root.setAttribute("data-ly-sw", ""); }
    else { root.style.removeProperty("--ly-sw"); root.removeAttribute("data-ly-sw"); }
    if (usePh) { root.style.setProperty("--ly-ph", ly.ph + "px"); root.setAttribute("data-ly-ph", ""); }
    else { root.style.removeProperty("--ly-ph"); root.removeAttribute("data-ly-ph"); }
    const fold = document.getElementById("sideFold");
    if (fold) {
      fold.textContent = Tc("панель", "Свернуть");
      fold.setAttribute("aria-label", ly.sb === "mini" ? T("Развернуть панель") : T("Свернуть панель"));
      fold.setAttribute("aria-expanded", ly.sb === "mini" ? "false" : "true");
    }
    measureSide();
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
      root.classList.add("lyresizing", "lyresizing-" + axis);
      const move = (ev) => onMove(st, ev);
      const up = () => {
        h.removeEventListener("pointermove", move);
        h.removeEventListener("pointerup", up);
        h.removeEventListener("pointercancel", up);
        h.classList.remove("drag");
        root.classList.remove("lyresizing", "lyresizing-" + axis);
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
    const lvl = ly.sb === "mini" || !w ? "" : w < 205 ? "xs" : w < 240 ? "s" : "";
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
    const ROWS = [
      ["side", "Боковая панель", [["left", "Слева"], ["right", "Справа"]]],
      ["sb", "Вид боковой панели", [["full", "Обычный"], ["mini", "Свёрнутый"]]],
      ["pl", "Нижняя панель", [["full", "Обычная"], ["mini", "Свёрнутая"]]],
      ["time", "Время трека", [["always", "Всегда"], ["hover", "При наведении"]]],
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
    c.append(p);
    apply();
  }
  window.Layout = { card, refresh: apply };
})();
