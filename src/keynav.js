// Навигация клавиатурой внутри окна (как keynav.js у Whisper Yandex; не путать с глобальными сочетаниями
// hotkeys.js — те работают и в свёрнутом окне).
//  • ↑/↓ — по трекам, карточкам и кнопкам шапки; в списке ← — в меню слева, из меню → — обратно; Enter — включить / открыть
//  • Home / End / PgUp / PgDn — по списку треков
//  • меню «⋯» (клавиша Menu или Shift+F10) — стрелками по пунктам, → открыть подменю, ← назад
//  • Ctrl+↑/↓ — громкость, Ctrl+←/→ — трек, M — звук, L — «сердце», Alt+← / Backspace / боковая кнопка мыши — назад,
//    F1 — список клавиш.
// Свои клавиши app.js остаются: Пробел — пауза, ←/→ (когда ничего не выбрано) — перемотка, Ctrl+F и / — поиск.
// Обработчик — на фазе захвата: обработанную клавишу дальше не пускает (иначе ←/→ в списке ещё и перематывали бы).
"use strict";

(() => {
  const $k = (s, root = document) => root.querySelector(s);
  const $$k = (s, root = document) => [...root.querySelectorAll(s)];
  const main = $k(".main"), side = $k(".side");
  const root = document.documentElement;
  if (!main || !side) return;
  const tmenu = () => $k(".popup.tmenu:not(.confirm-pop)");

  // ---------- что считается вводом текста и чужими клавишами ----------
  const TYPING = 'textarea, select, [contenteditable=""], [contenteditable="true"], input:not([type=range]):not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]):not([type=file])';
  const typing = (el) => !!el?.closest?.(TYPING);
  const ownKeys = (el) => !!el?.closest?.("input[type=range], input[type=checkbox], input[type=radio]");
  const blocked = () => !!document.querySelector(".modal, .confirm-pop") || !$k("#full")?.hidden;

  // ---------- видимость и фокус ----------
  const visible = (el) => !!el && !el.hidden && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";
  function focusEl(el) {
    if (!el) return false;
    if (el.tabIndex < 0 && !el.hasAttribute("tabindex")) el.tabIndex = -1; // строки и карточки — фокус только программно
    el.focus({ preventScroll: true });
    el.scrollIntoView({ block: "nearest", inline: "nearest" });
    return true;
  }
  // подсветка фокуса — только пока работают с клавиатуры
  const kbOn = () => root.classList.add("kbnav");
  document.addEventListener("pointerdown", () => root.classList.remove("kbnav"), true);

  // ---------- зоны ----------
  const sideItems = () => $$k(".nav", side).filter(visible);
  const MAIN_SEL = [
    "li", ".simgrid > *", ".stgrid > *", ".chips > button", ".chips > .chip", ".plcard",
    ".head .nav", ".head .round", ".head .stbtn", "#more", ".pill",
  ].join(", ");
  const inSettings = (el) => !!el.closest("#settingsPage");
  const mainItems = () => $$k(MAIN_SEL, main).filter((el) => !inSettings(el) && !el.closest(".tmenu") && visible(el));
  const zoneOf = (el) => (!el || el === document.body ? null : side.contains(el) ? "side" : main.contains(el) && el.matches(MAIN_SEL) ? "main" : null);

  // вход в список: играющий трек, если он на экране, иначе первый видимый
  function enterMain() {
    const items = mainItems();
    if (!items.length) return false;
    const playing = items.find((el) => el.classList.contains("playing"));
    const box = main.getBoundingClientRect();
    const onScreen = (el) => { const r = el.getBoundingClientRect(); return r.bottom > box.top && r.top < box.bottom; };
    return focusEl(playing && onScreen(playing) ? playing : items.find(onScreen) || items[0]);
  }
  const enterSide = () => {
    const act = $k(".nav.active", side);
    return focusEl(act && visible(act) ? act : sideItems()[0]);
  };

  // ближайший элемент в нужную сторону (и для сеток карточек, и для рядов кнопок)
  function nearest(from, items, dir) {
    const a = from.getBoundingClientRect();
    const ax = a.left + a.width / 2, ay = a.top + a.height / 2;
    let best = null, bestScore = Infinity;
    for (const el of items) {
      if (el === from || el.contains(from) || from.contains(el)) continue;
      const b = el.getBoundingClientRect();
      const bx = b.left + b.width / 2, by = b.top + b.height / 2;
      let along, cross;
      if (dir === "down") { if (b.top < a.bottom - 4) continue; along = b.top - a.bottom; cross = Math.abs(bx - ax); }
      else if (dir === "up") { if (b.bottom > a.top + 4) continue; along = a.top - b.bottom; cross = Math.abs(bx - ax); }
      else if (dir === "right") { if (b.left < a.right - 4) continue; along = b.left - a.right; cross = Math.abs(by - ay); }
      else { if (b.right > a.left + 4) continue; along = a.left - b.right; cross = Math.abs(by - ay); }
      const score = Math.max(0, along) + cross * 2;
      if (score < bestScore) { bestScore = score; best = el; }
    }
    return best;
  }
  // соседняя видимая строка того же списка — без перебора всех элементов (в библиотеке их тысячи)
  function sibling(li, step) {
    let el = li;
    while ((el = step > 0 ? el.nextElementSibling : el.previousElementSibling)) if (el.tagName === "LI" && visible(el)) return el;
    return null;
  }
  const listOf = (li) => $$k(":scope > li", li.parentElement).filter(visible);

  function moveMain(cur, key) {
    const isLi = cur.tagName === "LI";
    if (isLi && (key === "Home" || key === "End")) { const l = listOf(cur); return focusEl(key === "Home" ? l[0] : l[l.length - 1]); }
    if (isLi && (key === "PageDown" || key === "PageUp")) {
      const l = listOf(cur), i = l.indexOf(cur);
      const page = Math.max(1, Math.floor(main.clientHeight / Math.max(24, cur.offsetHeight)) - 1);
      return focusEl(l[Math.max(0, Math.min(l.length - 1, i + (key === "PageDown" ? page : -page)))]);
    }
    if (isLi && (key === "ArrowDown" || key === "ArrowUp")) {
      const s = sibling(cur, key === "ArrowDown" ? 1 : -1);
      if (s) return focusEl(s);
    }
    const dir = { ArrowDown: "down", ArrowUp: "up", ArrowLeft: "left", ArrowRight: "right" }[key];
    if (!dir) return false;
    const n = nearest(cur, mainItems(), dir);
    if (n) return focusEl(n);
    if (dir === "left") return enterSide(); // левее ничего нет — в меню слева
    if (dir === "up") { main.scrollTo({ top: 0, behavior: "smooth" }); return true; }
    return true;
  }
  function moveSide(cur, key) {
    const l = sideItems(), i = l.indexOf(cur);
    if (!l.length) return false;
    if (key === "ArrowDown") return focusEl(l[(i + 1) % l.length]);
    if (key === "ArrowUp") return focusEl(l[(i - 1 + l.length) % l.length]);
    if (key === "Home") return focusEl(l[0]);
    if (key === "End") return focusEl(l[l.length - 1]);
    if (key === "ArrowRight") return enterMain();
    return false;
  }

  // ---------- меню «⋯» с клавиатуры ----------
  let kbMenu = false, menuFrom = null;
  const menuBtns = () => { const m = tmenu(); return m ? $$k("button", m).filter(visible) : []; };
  function openMenuFor(li) {
    const mb = li?.querySelector(".morebtn");
    if (!mb) return false;
    kbMenu = true;
    menuFrom = li;
    mb.click();
    return true;
  }
  // меню собирается асинхронно и пересобирается для подменю — когда в нём появились пункты, фокус на первый
  function watchMenu() {
    const m = tmenu();
    if (!m) return setTimeout(watchMenu, 500); // меню создаёт whisper-vk.js
    new MutationObserver(() => {
      if (m.hidden) {
        if (kbMenu && menuFrom?.isConnected && (document.activeElement === document.body || !document.activeElement)) focusEl(menuFrom);
        kbMenu = false;
        menuFrom = null;
        return;
      }
      if (kbMenu && !m.contains(document.activeElement)) {
        const first = $k(".mi", m) || menuBtns()[0];
        if (first) first.focus({ preventScroll: true });
      }
    }).observe(m, { childList: true, attributes: true, attributeFilter: ["hidden"] });
  }
  watchMenu();
  function moveMenu(key) {
    const l = menuBtns();
    if (!l.length) return false;
    const i = l.indexOf(document.activeElement);
    if (key === "ArrowDown") return focusEl(l[(i + 1) % l.length]);
    if (key === "ArrowUp") return focusEl(l[i <= 0 ? l.length - 1 : i - 1]);
    if (key === "Home") return focusEl(l[0]);
    if (key === "End") return focusEl(l[l.length - 1]);
    if (key === "ArrowRight" && document.activeElement?.querySelector?.(".miarrow")) { kbMenu = true; document.activeElement.click(); return true; }
    if (key === "ArrowLeft") { const b = $k(".mback", tmenu()); if (b) { kbMenu = true; b.click(); return true; } }
    return false;
  }

  // ---------- действия ----------
  function goBack() {
    const b = $k("#back");
    if (b && !b.hidden && visible(b)) { b.click(); return true; }
    return false;
  }
  function likeKey() {
    const li = document.activeElement?.closest?.("li");
    const hb = li && main.contains(li) ? li.querySelector(".likebtn") : null;
    if (hb) hb.click();
    else if (S.current) $k("#likeBtn")?.click();
  }

  // ---------- окно со списком клавиш (F1) ----------
  const HELP = [
    ["Пробел", "Пауза / играть"],
    ["← / →", "Перемотать на 5 с (Shift — на 30 с), когда трек в списке не выбран"],
    ["Ctrl + → / ←", "Следующий / предыдущий трек"],
    ["Ctrl + ↑ / ↓", "Громче / тише"],
    ["M", "Выключить / включить звук"],
    ["L", "«Сердце» выбранному или играющему треку"],
    ["Ctrl + F или /", "Поиск"],
    ["↑ / ↓", "Ходить по трекам, карточкам и меню слева"],
    ["← / → в списке", "В меню слева и обратно"],
    ["Enter", "Включить трек / открыть"],
    ["Home / End, PgUp / PgDn", "К началу / концу списка, на страницу"],
    ["Menu или Shift + F10", "Меню «⋯» у выбранного трека"],
    ["Alt + ← или Backspace", "Назад"],
    ["Esc", "Закрыть меню и окна"],
    ["F1", "Этот список"],
  ];
  let help = null;
  function closeHelp() {
    if (!help) return;
    document.removeEventListener("keydown", help.onKey, true);
    help.wrap.remove();
    help = null;
  }
  function openHelp() {
    if (help) return closeHelp();
    const wrap = el("div", "login modal");
    const box = el("div", "panel login-box modalbox hkmodal kbhelp");
    const list = el("div", "hklist");
    for (const [k, what] of HELP) {
      const row = el("div", "hkrow kbrow");
      row.append(el("span", "hkname", L(what)), el("span", "hkkey", L(k)));
      list.append(row);
    }
    const foot = el("div", "row");
    const ok = el("button", "nav active", L("Понятно"));
    ok.type = "button";
    ok.addEventListener("click", closeHelp);
    foot.append(ok);
    box.append(el("h2", null, L("Клавиши в окне")),
      el("p", "hksub", L("Работают, пока окно приложения активно и курсор не в поле ввода. Сочетания для свёрнутого окна — в «Горячих клавишах».")),
      list, foot);
    wrap.append(box);
    document.body.append(wrap);
    wrap.addEventListener("pointerdown", (e) => { if (e.target === wrap) closeHelp(); });
    const onKey = (e) => { if (e.key === "Escape" || e.key === "F1") { e.preventDefault(); e.stopPropagation(); closeHelp(); } };
    document.addEventListener("keydown", onKey, true);
    help = { wrap, onKey };
    ok.focus();
  }
  window.kbHelp = openHelp;

  // ---------- главный обработчик ----------
  const NAV_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"]);
  const done = (e) => { e.preventDefault(); e.stopPropagation(); kbOn(); };
  document.addEventListener("keydown", (e) => {
    if (e.defaultPrevented || e.isComposing) return;
    const t = document.activeElement;
    const mod = e.ctrlKey || e.metaKey, plain = !mod && !e.altKey && !e.shiftKey;
    if (e.key === "F1") {
      if (!document.querySelector(".modal")) { e.preventDefault(); openHelp(); }
      return;
    }
    if (typing(t) || blocked()) return;

    // меню «⋯» открыто — стрелки ходят по его пунктам
    const m = tmenu();
    if (m && !m.hidden && NAV_KEYS.has(e.key) && plain) {
      if (moveMenu(e.key)) done(e);
      return;
    }

    if (mod && !e.altKey && !e.shiftKey && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "ArrowRight") next(false);
      else prev();
      return;
    }
    if (mod && !e.altKey && !e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      e.stopPropagation();
      setVolume(S.volume + (e.key === "ArrowUp" ? 5 : -5));
      return;
    }
    if (plain && e.code === "KeyM") { $k("#volBtn")?.click(); return; }
    if (plain && e.code === "KeyL") { likeKey(); return; }

    // назад
    if ((e.altKey && !mod && !e.shiftKey && e.key === "ArrowLeft") || (plain && e.key === "Backspace")) {
      if (goBack()) { e.preventDefault(); e.stopPropagation(); }
      return;
    }

    // меню «⋯» у выбранного трека
    if ((e.key === "ContextMenu" || (e.shiftKey && !mod && !e.altKey && e.key === "F10")) && t?.tagName === "LI" && main.contains(t)) {
      if (openMenuFor(t)) done(e);
      return;
    }

    // Enter по строке или карточке (кнопки и ссылки Enter нажимают сами)
    if (plain && e.key === "Enter" && t && zoneOf(t) === "main" && !t.matches("button, a, input")) {
      done(e);
      t.click();
      return;
    }

    // стрелки (фокус на кнопке внутри строки — после щелчка по «сердцу» или «⋯» — считаем выбранной саму строку)
    if (!plain || !NAV_KEYS.has(e.key) || ownKeys(t)) return;
    const row = t?.closest?.("li");
    const cur = row && main.contains(row) && !inSettings(row) ? row : t;
    const zone = zoneOf(cur);
    let ok = false;
    if (zone === "main") ok = moveMain(cur, e.key);
    else if (zone === "side") ok = moveSide(cur, e.key);
    else if ((!t || t === document.body || t === main || (main.contains(t) && !inSettings(t))) && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      // фокуса нигде нет: ↑/↓ — в список (←/→ остаются перемоткой app.js)
      ok = enterMain() || enterSide();
    }
    if (ok) done(e);
  }, true);

  // боковая кнопка мыши «назад»
  document.addEventListener("mouseup", (e) => { if (e.button === 3 && !blocked() && goBack()) e.preventDefault(); });
  document.addEventListener("auxclick", (e) => { if (e.button === 3) e.preventDefault(); });
})();
