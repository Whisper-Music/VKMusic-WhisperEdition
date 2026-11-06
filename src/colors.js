"use strict";
// Цвета темы (как у Whisper Yandex): основной и второй цвет и «Тонкая настройка…» — цвет каждого места отдельно
// (кнопки, заголовки, панели, текст…). Всё хранится для каждого стиля отдельно в localStorage (vkm_colors):
// { "<id стиля>": { accent, accent2, cc: { ключ: "#rrggbb" | "#rrggbbaa" } } }. Пусто — «как в теме».
// Основной/второй цвет подставляет applyStyleVars (app.js, Colors.main), правила тонкой настройки — <style id="cc-css">
// последним в <head>: настройки пользователя главнее CSS темы. Значения — только #rrggbb(aa): в CSS попадают
// цвета, а не произвольный текст.
(() => {
  const KEY = "vkm_colors";
  const HEX = /^#[0-9a-f]{6}([0-9a-f]{2})?$/i;
  let db = {};
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "{}");
    if (v && typeof v === "object") db = v;
  } catch {}
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(db)); } catch {} };
  const root = document.documentElement;
  const style = () => root.dataset.style || "neon";
  const base = () => root.dataset.base || "neon";
  const T = (s) => (typeof L === "function" ? L(s) : s);
  const of = (st = style()) => {
    const o = (db[st] && typeof db[st] === "object" ? db[st] : (db[st] = {}));
    if (!o.cc || typeof o.cc !== "object") o.cc = {};
    for (const k of Object.keys(o.cc)) if (!HEX.test(o.cc[k])) delete o.cc[k];
    return o;
  };
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };

  // ---------- места для тонкой настройки (CC_GROUPS Whisper Yandex; css: { all | <основа>: [[селекторы, объявления с $]] }) ----------
  const PANEL = "color-mix(in srgb, $ calc(var(--panel-a) * 100%), transparent)";
  const BALL = "radial-gradient(circle at 35% 30%, color-mix(in srgb, $ 55%, #fff), $ 60%, color-mix(in srgb, $ 70%, #000))";
  const FRAMED = ["royal", "steam", "lolitta"]; // у кнопок этих стилей настоящая рамка (border)
  const bordered = (sel, notActive) => Object.fromEntries([["all", [[notActive, "box-shadow: inset 0 0 0 1px $"]]], ...FRAMED.map((st) => [st, [[sel, "border-color: $"]]])]);
  const SIDE_NAV = ".side .nav:not(.active)", SIDE_ACT = ".side .nav.active";
  const BTN = ".main .nav:not(.active):not(.danger), .chip:not(.active), .pill:not(.active), .round, .modalbox .nav:not(.active):not(.danger)";
  const BTN_ALL = ".main .nav:not(.danger), .chip, .pill, .round, .modalbox .nav:not(.danger)";
  const BTN_ACT = ".main .nav.active:not(.danger), .chip.active, .pill.active, .modalbox .nav.active:not(.danger), .login-box .nav.active:not(.danger)";
  const PLAY = ".player .playbtn, .fbtns .playbtn, .wavebtn";
  const SEEK_TRACK = { all: "rgba(255,255,255,.16)", steam: "rgba(255,255,255,.12)", lolitta: "color-mix(in srgb, var(--accent2) 22%, transparent)" };
  const VOL_TRACK = { all: "rgba(255,255,255,.16)", steam: "rgba(255,255,255,.12)", lolitta: "color-mix(in srgb, var(--accent) 30%, transparent)" };
  const GROUPS = [
    { name: "Текст", items: [
      { k: "text", label: "Основной текст", vars: ["--text"], probe: ["body", "color"] },
      { k: "muted", label: "Второстепенный текст", vars: ["--muted"], probe: [".status", "color"] },
      { k: "heading", label: "Заголовки", css: { all: [["#title, .main h2, .main h3, .card h3", "color: $; -webkit-text-fill-color: $"]] }, probe: ["#title", "color"] },
    ] },
    { name: "Фон окна", items: [
      { k: "winBg", label: "Фон окна (когда нет обоев)", css: { all: [["body", "background: $"]] }, probe: ["body", "background-color"] },
      { k: "titleBarBg", label: "Полоса заголовка окна", css: { all: [[".titlebar", "background: $"]] }, probe: [".titlebar", "background-color"] },
    ] },
    { name: "Боковая панель", items: [
      { k: "sideBg", label: "Фон панели", css: { all: [[".side", "background: " + PANEL]], steam: [[".side::before", "background: " + PANEL]] } },
      { k: "sideBorder", label: "Обводка панели", fb: "accent", css: { all: [[".side", "border-color: $"]], steam: [[".side::before", "border-color: $"]] } },
      { k: "navBg", label: "Фон кнопок", css: { all: [[SIDE_NAV, "background: $"]] }, probe: [SIDE_NAV, "background-color"] },
      { k: "navText", label: "Текст и значки кнопок", css: { all: [[SIDE_NAV, "color: $"]], steam: [[SIDE_NAV, "color: $"], [SIDE_NAV + "::before", "background: $"]], lolitta: [[SIDE_NAV, "color: $"], [SIDE_NAV + "::before", "background: $"]] }, probe: [SIDE_NAV, "color"] },
      { k: "navBorder", label: "Обводка кнопок", fb: "accent", css: bordered(".side .nav", SIDE_NAV) },
      { k: "navActBg", label: "Фон активной кнопки", fb: "accent2", css: { all: [[SIDE_ACT, "background: $"]] } },
      { k: "navActText", label: "Текст активной кнопки", css: { all: [[SIDE_ACT, "color: $"]] }, probe: [SIDE_ACT, "color"] },
      { k: "searchBg", label: "Фон поиска", css: { all: [[".search", "background: $"]] }, probe: [".search", "background-color"] },
      { k: "searchText", label: "Текст поиска", css: { all: [[".search", "color: $"], [".search::placeholder", "color: color-mix(in srgb, $ 62%, transparent)"]] }, probe: [".search", "color"] },
    ] },
    { name: "Кнопки в окне", note: "Чипы, «Показать ещё», кнопки в окнах", items: [
      { k: "btnBg", label: "Фон кнопок", css: { all: [[BTN, "background: $"]] } },
      { k: "btnText", label: "Текст кнопок", css: { all: [[BTN, "color: $"]] }, probe: [".chip:not(.active)", "color"] },
      { k: "btnBorder", label: "Обводка кнопок", fb: "accent", css: bordered(BTN_ALL, BTN) },
      { k: "btnActBg", label: "Фон выбранной кнопки", fb: "accent2", css: { all: [[BTN_ACT, "background: $"]] } },
      { k: "btnActText", label: "Текст выбранной кнопки", fb: "onAccent", css: { all: [[BTN_ACT, "color: $; text-shadow: none"]] } },
    ] },
    { name: "Нижняя панель", items: [
      { k: "plBg", label: "Фон панели", css: { all: [[".player", "background: " + PANEL]], steam: [[".player::before", "background: " + PANEL]] } },
      { k: "plBorder", label: "Обводка панели", fb: "accent", css: { all: [[".player", "border-color: $"]], steam: [[".player::before", "border-color: $"]] } },
      { k: "plTitle", label: "Название трека", css: { all: [["#pTitle", "color: $"]] }, probe: ["#pTitle", "color"] },
      { k: "plArtist", label: "Исполнитель и время", css: { all: [["#pArtist, .player .time", "color: $"]] }, probe: ["#pArtist", "color"] },
      { k: "plIcons", label: "Значки кнопок", css: { all: [[".player .ctrls button:not(.playbtn):not(.on), .player .pright button:not(.on), .pinfo .likebtn:not(.on)", "color: $"]] }, probe: ["#prev", "color"] },
      { k: "plModeOn", label: "Включённые режимы", fb: "accent2", css: { all: [[".modebtn.on", "color: $"]] } },
    ] },
    { name: "Кнопка «Пуск» и VK Микс", items: [
      { k: "playBg", label: "Фон кнопки", fb: "accent2", css: { all: [[PLAY, "background: " + BALL]] } },
      { k: "playIcon", label: "Значок", fb: "onAccent", css: { all: [[PLAY, "color: $; text-shadow: none"]] } },
    ] },
    { name: "Полоса воспроизведения", items: [
      { k: "seekFill", label: "Пройденная часть", fb: "accent2", gen: (cc, st) => !cc.seekFill && !cc.seekTrack ? "" :
        `#seek::-webkit-slider-runnable-track, #fSeek::-webkit-slider-runnable-track { background: linear-gradient(90deg, ${cc.seekFill || "var(--accent2)"} var(--p, 0%), ${cc.seekTrack || SEEK_TRACK[st] || SEEK_TRACK.all} var(--p, 0%)) !important; }` },
      { k: "seekTrack", label: "Оставшаяся часть", gen: () => "" },
      { k: "seekThumb", label: "Ползунок", fb: "#ffffff", css: { all: [["#seek::-webkit-slider-thumb, #fSeek::-webkit-slider-thumb", "background: $"]] } },
    ] },
    { name: "Громкость", items: [
      { k: "volFill", label: "Уровень громкости", fb: "accent", gen: (cc, st) => !cc.volFill ? "" :
        `#vol::-webkit-slider-runnable-track { background: linear-gradient(90deg, ${cc.volFill} var(--p, 0%), ${VOL_TRACK[st] || VOL_TRACK.all} var(--p, 0%)) !important; }` },
    ] },
    { name: "В библиотеке («сердце»)", items: [
      { k: "like", label: "Цвет «сердца»", fb: "accent2", css: { all: [[".likebtn.on", "color: $"]] } },
    ] },
    { name: "Список треков", items: [
      { k: "rowHover", label: "Подсветка строки при наведении", fb: "accent", css: { all: [[".list li:hover", "background: color-mix(in srgb, $ 18%, transparent)"]] } },
      { k: "rowTitle", label: "Название трека", css: { all: [[".list .t", "color: $"]] }, probe: [".list .t", "color"] },
      { k: "rowArtist", label: "Исполнитель", css: { all: [[".list .a", "color: $"]] }, probe: [".list .a", "color"] },
    ] },
    { name: "Средняя панель", note: "Основная область со списками и страницами", items: [
      { k: "mainBg", label: "Фон средней панели", css: { all: [[".main", "background: $"]] }, probe: [".main", "background-color"] },
      { k: "listBg", label: "Фон списка треков", css: { all: [[".main .list:not(:empty)", "background: $"]] } },
      { k: "titleBg", label: "Фон заголовка страницы", css: { all: [[".head h2", "background: $; -webkit-text-fill-color: currentColor"]] } },
    ] },
    { name: "Меню и окна", note: "Меню трека, подсказки, окна, панель очереди", items: [
      { k: "menuBg", label: "Фон", css: { all: [[".tmenu, .toast, .modalbox, .login-box, .qpanel", "background: $"]] }, probe: [".tmenu", "background-color"] },
      { k: "menuText", label: "Текст", css: { all: [[".tmenu, .tmenu button, .toast, .modalbox, .login-box, .qpanel", "color: $"]] }, probe: [".tmenu", "color"] },
    ] },
    { name: "Карточки и блоки", note: "Блоки настроек, статистики, плейлистов", items: [
      { k: "cardBg", label: "Фон", css: { all: [[".card, .lastAlbum, .simcard, .stylecard", "background: " + PANEL]] }, probe: [".card", "background-color"] },
      { k: "cardBorder", label: "Обводка", css: { all: [[".card, .lastAlbum, .simcard, .stylecard", "border-color: $"]] }, probe: [".card", "border-top-color"] },
    ] },
    { name: "Переключатели и ползунки", note: "В настройках", items: [
      { k: "ctl", label: "Цвет включённого", fb: "accent", css: { all: [[".switch input:checked + i", "background: $"]] },
        gen: (cc) => cc.ctl ? `.settings input[type=range]::-webkit-slider-runnable-track { background: linear-gradient(90deg, ${cc.ctl} var(--p, 0%), rgba(255,255,255,.16) var(--p, 0%)) !important; }` : "" },
    ] },
    { name: "Свечение", only: "neon", note: "Только в неоновом стиле", items: [
      { k: "glow", label: "Цвет свечения", fb: "accent2", css: { neon: [[":root", "--glow-c: $; --btn-glow: $"]] } },
    ] },
  ];
  const ITEMS = GROUPS.flatMap((g) => g.items.map((it) => ({ ...it, group: g })));
  // «Сразу для всех»: один цвет — во все перечисленные места (потом любое можно поправить отдельно)
  const BULK = [
    { label: "Фон всех панелей", keys: ["titleBarBg", "sideBg", "mainBg", "plBg", "menuBg"] },
    { label: "Обводка всех панелей", keys: ["sideBorder", "plBorder", "cardBorder"] },
    { label: "Фон всех кнопок", keys: ["navBg", "btnBg"] },
    { label: "Текст и значки всех кнопок", keys: ["navText", "btnText", "plIcons"] },
    { label: "Обводка всех кнопок", keys: ["navBorder", "btnBorder"] },
    { label: "Фон выбранных кнопок", keys: ["navActBg", "btnActBg"] },
    { label: "Текст выбранных кнопок", keys: ["navActText", "btnActText"] },
  ];

  // «Тонкая настройка» из файла темы (settings whisper-music → colorOverrides, как у Whisper Yandex; так её
  // сохраняет «Сохранить стиль в файл…»): только для её собственного стиля и только цвета. Свои настройки главнее.
  function themeCc(st) {
    const th = typeof S !== "undefined" ? S : null;
    const co = th && th.theme && th.theme.file && th.theme.id === st && th.themeSettings && th.themeSettings.colorOverrides;
    const out = {};
    if (co && typeof co === "object") for (const it of ITEMS) if (HEX.test(co[it.k] || "")) out[it.k] = co[it.k];
    return out;
  }

  function ccCss(st) {
    // повтор :root поднимает специфичность, сам блок — последним в <head>: главнее правил темы с !important
    const cc = { ...themeCc(st), ...of(st).cc }, pre = `:root:root:root:root[data-style="${st}"]`, b = base(), out = [];
    const scope = (sel) => sel.split(",").map((x) => x.trim()).map((x) => (x === ":root" ? pre : `${pre} ${x}`)).join(", ");
    // у стиля нет обводки (толщина 0) — выбранный цвет рисуется рамкой в 1px
    const noBorder = (sel) => sel.split(",").some((x) => {
      let e = null;
      try { e = document.querySelector(x.trim().replace(/::?(before|after)$/, "")); } catch {}
      return e && parseFloat(getComputedStyle(e).borderTopWidth) === 0 && !/::?(before|after)$/.test(x.trim());
    });
    for (const it of ITEMS) {
      if (it.group.only && it.group.only !== b) continue;
      const v = cc[it.k];
      if (it.gen) {
        const g = it.gen(cc, b);
        if (g) out.push(g.replace(/(^|\})\s*([^{}]+)\{/g, (m, br, sel) => `${br}\n${scope(sel)} {`));
      }
      if (!v) continue;
      if (it.vars) out.push(`${pre} { ${it.vars.map((n) => `${n}: ${v} !important`).join("; ")}; }`);
      for (const [sel, decl0] of (it.css && (it.css[b] || it.css.all)) || []) {
        const decl = /^border-color:/.test(decl0) && noBorder(sel) ? decl0 + "; border-style: solid; border-width: 1px" : decl0;
        const d = decl.replaceAll("$", v).split(/;\s*/).filter(Boolean).map((x) => x + " !important").join("; ");
        out.push(`${scope(sel)} { ${d}; }`);
      }
    }
    return out.join("\n");
  }

  let styleEl = null;
  function apply() {
    if (!styleEl) {
      styleEl = el("style");
      styleEl.id = "cc-css";
      document.head.append(styleEl);
    }
    styleEl.textContent = "";            // обводки проверяем по виду стиля без старых настроек
    styleEl.textContent = ccCss(style());
  }

  const toHex = (c) => {
    const m = /rgba?\(([\d.]+),?\s*([\d.]+),?\s*([\d.]+)(?:[,/]\s*([\d.]+))?/.exec(c || "");
    if (!m || (m[4] !== undefined && +m[4] < 0.35)) return null;
    return "#" + [m[1], m[2], m[3]].map((x) => Math.round(+x).toString(16).padStart(2, "0")).join("");
  };
  const cssHex = (name) => {
    const v = getComputedStyle(root).getPropertyValue(name).trim();
    return /^#[0-9a-f]{6}$/i.test(v) ? v : toHex(v);
  };
  // что показать в пипетке у «авто»: настоящий цвет элемента, если его можно прочитать, иначе — основной/второй
  function auto(it) {
    if (it.probe) {
      const [sel, prop] = it.probe, e = document.querySelector(sel);
      if (e) { const h = toHex(getComputedStyle(e).getPropertyValue(prop)); if (h) return h; }
    }
    if (it.fb === "onAccent") return cssHex("--on-accent") || "#ffffff";
    if (it.fb && it.fb[0] === "#") return it.fb;
    if (it.vars) { const h = cssHex(it.vars[0]); if (h) return h; }
    return (it.fb === "accent" ? cssHex("--accent") : cssHex("--accent2")) || "#a66bff";
  }
  const fill = (r) => r.style.setProperty("--p", ((r.value - r.min) / (r.max - r.min)) * 100 + "%");

  function openEditor() {
    if (document.querySelector(".ccmodal")) return;
    const wrap = el("div", "login modal ccmodal");
    const box = el("div", "panel login-box modalbox ccbox");
    const scroll = el("div", "ccscroll");
    const cc = of().cc;
    const rows = {}, bulks = [], groupDots = [];
    const alphaOf = (v) => (/^#[0-9a-f]{8}$/i.test(v || "") ? Math.round((parseInt(v.slice(7), 16) / 255) * 100) : 100);
    const dotsOf = (dots, keys) => dots.replaceChildren(...keys.filter((k) => cc[k]).slice(0, 6).map((k) => { const i = el("i"); i.style.background = cc[k]; return i; }));
    const syncAllDots = () => groupDots.forEach(([d, keys]) => dotsOf(d, keys));
    const refreshBulk = () => bulks.forEach((b) => b());
    const changed = () => { apply(); save(); };
    // строка: название, «авто», пипетка, непрозрачность, ✕
    const makeRow = (label, autoHex, onSet, onClear) => {
      const row = el("label", "ccrow");
      const am = el("small", null, T("авто"));
      const inp = el("input");
      inp.type = "color";
      const al = el("input", "ccalpha");
      Object.assign(al, { type: "range", min: "0", max: "100", title: T("Непрозрачность: влево — прозрачнее") });
      const pct = el("output", "ccpct");
      const compose = () => (+al.value >= 100 ? inp.value : inp.value + Math.round((+al.value / 100) * 255).toString(16).padStart(2, "0"));
      const put = () => { pct.textContent = al.value + "%"; fill(al); row.classList.add("set"); am.textContent = T("авто"); onSet(compose()); };
      inp.addEventListener("input", put);
      al.addEventListener("input", put);
      const clr = el("button", "ccclr", "✕");
      clr.type = "button";
      clr.title = T("Вернуть «авто»");
      clr.addEventListener("click", (e) => { e.preventDefault(); onClear(); });
      const show = (v, hint) => {
        row.classList.toggle("set", !!v);
        am.textContent = hint || T("авто");
        row.classList.toggle("mixed", !!hint);
        inp.value = (v || autoHex()).slice(0, 7);
        al.value = alphaOf(v);
        pct.textContent = al.value + "%";
        fill(al);
      };
      row.append(el("span", null, T(label)), am, inp, al, pct, clr);
      return { row, show };
    };
    const groups = GROUPS.filter((g) => !g.only || g.only === base());
    const shown = new Set(groups.flatMap((g) => g.items.map((it) => it.k)));
    const itemOf = (k) => ITEMS.find((it) => it.k === k);
    {
      const d = el("details", "ccgrp ccbulk");
      d.open = true;
      const sm = el("summary");
      sm.append(el("span", null, T("Сразу для всех")), el("small", null, T("Один цвет сразу во все места; потом любое можно поправить ниже")));
      const rws = el("div", "ccrows");
      for (const b of BULK) {
        const keys = b.keys.filter((k) => shown.has(k));
        if (!keys.length) continue;
        const r = makeRow(b.label, () => auto(itemOf(keys[0])),
          (v) => { for (const k of keys) { cc[k] = v; rows[k]?.show(v); } changed(); syncAllDots(); },
          () => { for (const k of keys) delete cc[k]; changed(); for (const k of keys) rows[k]?.show(); syncAllDots(); r.show(); });
        const sync = () => {
          const vals = keys.map((k) => cc[k] || "");
          if (vals.every((v) => v === vals[0])) r.show(vals[0]);
          else r.show(vals.find(Boolean) || "", T("разные"));
        };
        bulks.push(sync);
        sync();
        rws.append(r.row);
      }
      d.append(sm, rws);
      scroll.append(d);
    }
    for (const g of groups) {
      const d = el("details", "ccgrp");
      const sm = el("summary");
      const dots = el("span", "ccdots");
      sm.append(el("span", null, T(g.name)), dots);
      if (g.note) sm.append(el("small", null, T(g.note)));
      const rws = el("div", "ccrows");
      const keys = g.items.map((it) => it.k);
      for (const it of g.items) {
        const r = makeRow(it.label, () => auto(it),
          (v) => { cc[it.k] = v; changed(); dotsOf(dots, keys); refreshBulk(); },
          () => { delete cc[it.k]; changed(); r.show(); dotsOf(dots, keys); refreshBulk(); });
        r.show(cc[it.k]);
        rows[it.k] = r;
        rws.append(r.row);
      }
      groupDots.push([dots, keys]);
      dotsOf(dots, keys);
      d.append(sm, rws);
      scroll.append(d);
    }
    const foot = el("div", "row");
    const reset = el("button", "nav", T("Сбросить всё"));
    const ok = el("button", "nav active", T("Готово"));
    reset.type = ok.type = "button";
    reset.addEventListener("click", () => {
      for (const k of Object.keys(cc)) delete cc[k];
      changed();
      Object.values(rows).forEach((r) => r.show());
      syncAllDots();
      refreshBulk();
    });
    foot.append(reset, ok);
    box.append(el("h2", null, T("Тонкая настройка цветов")), el("p", "ccnote", T("Для текущего стиля. Блёклые кружки — «авто»: цвет из темы.")), scroll, foot);
    const close = () => { document.removeEventListener("keydown", onKey, true); wrap.remove(); };
    const onKey = (e) => { if (e.key === "Escape") { e.stopPropagation(); close(); } };
    document.addEventListener("keydown", onKey, true);
    ok.addEventListener("click", close);
    wrap.addEventListener("pointerdown", (e) => e.target === wrap && close());
    wrap.append(box);
    document.body.append(wrap);
  }

  /** Основной и второй цвета пользователя для стиля (или null — как в теме). */
  function main(st = style()) {
    const o = db[st];
    if (!o) return null;
    const ok = (v) => (/^#[0-9a-f]{6}$/i.test(v || "") ? v : null);
    const r = { accent: ok(o.accent), accent2: ok(o.accent2) };
    return r.accent || r.accent2 ? r : null;
  }

  /** Карточка «Цвета» (app.js, renderThemes): основной и второй, «Цвета темы», «Тонкая настройка…». */
  function card(c, onChange) {
    const o = of();
    const cur = (name) => cssHex(name) || "#a66bff";
    const sw = el("div", "swatches");
    const mk = (key, varName, label) => {
      const lab = el("label", "swatch");
      const inp = el("input");
      inp.type = "color";
      inp.value = o[key] || cur(varName);
      inp.addEventListener("input", () => { o[key] = inp.value; save(); onChange(); reset.disabled = false; });
      lab.append(inp, el("span", null, T(label)));
      return lab;
    };
    const btns = el("div", "swbtns");
    const reset = el("button", "nav ccopen", T("Цвета темы"));
    reset.id = "colorReset";
    reset.type = "button";
    reset.title = T("Вернуть основной и второй цвета темы");
    reset.disabled = !o.accent && !o.accent2;
    reset.addEventListener("click", () => { delete o.accent; delete o.accent2; save(); onChange(true); });
    const fine = el("button", "nav ccopen", T("Тонкая настройка…"));
    fine.type = "button";
    fine.addEventListener("click", openEditor);
    sw.append(mk("accent", "--accent", "Основной"), mk("accent2", "--accent2", "Второй"));
    btns.append(reset, fine);
    const row = el("div", "swrow");
    row.append(sw, btns);
    c.append(row);
  }

  /** Тонкая настройка стиля целиком (из файла темы и своя) — для «Сохранить стиль в файл…». */
  const ccOf = (st = style()) => ({ ...themeCc(st), ...of(st).cc });
  window.Colors = { main, card, apply, ccOf };
})();
