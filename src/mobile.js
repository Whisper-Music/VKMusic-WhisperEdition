"use strict";
// Телефон (Android; класс html.mobile ставит boot.js, раскладка — mobile.css). Разметка та же, что на компьютере
// (контракт с темами Whisper), только несколько элементов переставляются туда, где до них дотянется палец:
// * сверху — строка поиска (из боковой панели) и кнопка аккаунта: «Сервисы» (вход во ВКонтакте и SoundCloud);
// * боковая панель — нижняя полоса разделов со значками (раздел «Сервисы» — кнопкой аккаунта);
// * плеер внизу — мини-плеер: обложка, трек, «Пуск», «Вперёд», тонкая полоса прогресса; нажатие — «Сейчас играет»;
// * режимы плеера (перемешать, повтор, текст, «Постоянный уровень громкости», сон, кнопки модулей ядра —
//   «Слушать вместе», очередь) и «⋯» — на экран «Сейчас играет»; id и обработчики остаются прежними;
// * «Сейчас играет» закрывается смахиванием вниз;
// * «Назад» Android: закрыть всплывающее → «Сейчас играет» → подстраницу (плейлист, поиск, магазин тем) → к
//   библиотеке → свернуть приложение (музыка играет дальше, app_background).
(() => {
  if (!MOBILE) return;

  // ---------- без наведения ----------
  // Наведения у пальца нет, а WebView оставляет :hover на нажатой кнопке («липкое» наведение): у Vantablack выбранный
  // раздел после нажатия становился белым на белом (2026-11-15). На телефоне правила с :hover не срабатывают никогда —
  // и в файлах основы, и в CSS темы и цветов (<style> меняются при смене стиля — обрабатываются заново)
  const seen = new WeakSet();
  const noHover = (rules) => {
    for (const r of rules) {
      if (r.selectorText && r.selectorText.includes(":hover")) r.selectorText = r.selectorText.replace(/:hover(?!:not\(:hover\))/g, ":hover:not(:hover)");
      if (r.cssRules && r.cssRules.length) noHover(r.cssRules);
    }
  };
  const stripHover = () => {
    for (const sh of document.styleSheets) {
      if (seen.has(sh)) continue;
      seen.add(sh);
      try { noHover(sh.cssRules); } catch {}
    }
  };
  stripHover();
  new MutationObserver(stripHover).observe(document.head, { subtree: true, childList: true, characterData: true });

  // ---------- системные панели — цветом фона темы ----------
  // Фон страницы (как снимок для boot.js, saveBoot) — не --bg: у части тем это полупрозрачная панель (rgba), и строка
  // состояния оставалась цветом прошлого стиля (зелёной от Osaka Jade у Vantablack, 2026-11-15)
  let barsSent = "", barsT = 0;
  const hex = (c) => {
    const m = c && c.match(/rgba?\(([^)]+)\)/);
    if (!m) return "";
    const v = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
    if (v.length > 3 && v[3] < 0.5) return "";
    return "#" + v.slice(0, 3).map((x) => Math.round(x).toString(16).padStart(2, "0")).join("");
  };
  const syncBars = () => {
    clearTimeout(barsT);
    barsT = setTimeout(() => {
      if (document.documentElement.classList.contains("booting")) return;
      const bg = hex(getComputedStyle(document.body).backgroundColor) || hex(getComputedStyle(document.documentElement).backgroundColor) || "#000000";
      if (bg === barsSent) return;
      barsSent = bg;
      // тот же цвет — под строкой поиска сверху (продолжает строку состояния; --bg темы бывает полупрозрачным)
      document.documentElement.style.setProperty("--mbar", bg);
      invoke("app_bars", { bg }).catch(() => {});
    }, 300);
  };
  new MutationObserver(syncBars).observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-style", "style"] });
  syncBars();

  // ---------- верхняя строка: поиск и аккаунт ----------
  const search = $("#search");
  const main = $(".main");
  const account = el("button", "maccount");
  account.type = "button";
  account.setAttribute("aria-label", L("Сервисы и интеграции"));
  account.innerHTML = Icons.svg("user");
  account.addEventListener("click", () => go("services"));
  if (search && main) {
    const box = el("div", "msearch");
    const field = el("label", "mfield");
    field.innerHTML = Icons.svg("search");
    field.append(search);
    box.append(field, account);
    main.prepend(box);
  }

  // ---------- нижняя полоса разделов: значки ----------
  const NAV_ICONS = { wave: "mix", library: "library", vk: "playlist", stats: "chart", friends: "user", settings: "gear" };
  for (const b of document.querySelectorAll(".side .nav[data-page]")) {
    const icon = NAV_ICONS[b.dataset.page];
    if (!icon) continue;
    const i = el("span", "nicon");
    i.innerHTML = Icons.svg(icon);
    b.prepend(i);
  }
  // кнопка аккаунта горит на странице «Сервисы» (go в app.js переключает .active у разделов — по нему и следим)
  const syncAccount = () => account.classList.toggle("active", typeof S !== "undefined" && S.page === "services");
  new MutationObserver(syncAccount).observe($(".side"), { subtree: true, attributes: true, attributeFilter: ["class"] });

  // ---------- мини-плеер и «Сейчас играет» ----------
  const fctrl = $("#full .fctrl");
  const pright = $(".player .pright");
  const more = $("#pMore");
  if (fctrl) {
    const modes = el("div", "fmodes");
    for (const id of ["shuffleBtn", "repeatBtn"]) {
      const b = document.getElementById(id);
      if (b) modes.append(b);
    }
    fctrl.append(modes);
    // правый край плеера (текст, громкость, сон и кнопки модулей ядра — «Слушать вместе», очередь) — сюда же;
    // модули подключаются позже и добавляют кнопки сами, поэтому — и всё, что появится там потом; «⋯» — последним
    const adopt = () => {
      for (const b of [...pright.children]) if (b.id !== "volWrap" && b !== more) modes.append(b);
      if (more && modes.lastElementChild !== more) modes.append(more);
    };
    if (pright) {
      adopt();
      new MutationObserver(adopt).observe(pright, { childList: true });
    }
  }

  // нажатие на трек в мини-плеере — «Сейчас играет» с большой обложкой (на компьютере обложка открывает текст)
  const pinfo = $(".player .pinfo");
  if (pinfo) {
    pinfo.addEventListener("click", (e) => {
      if (e.target.closest("button")) return; // «сердце»
      e.stopPropagation();
      openFull("cover");
    }, true);
  }

  // ничего не включено — мини-плеер прячется (mobile.css), как в мобильных плеерах
  const syncNone = () => document.documentElement.classList.toggle("mnone", typeof S === "undefined" || !S.current);
  syncNone();
  Whisper.on("track", syncNone);

  // смахнуть «Сейчас играет» вниз — закрыть (как шторку); не с полосы перемотки, кнопок и прокручиваемого текста
  const full = $("#full");
  let swipe = null;
  full.addEventListener("touchstart", (e) => {
    const t = e.target;
    if (e.touches.length !== 1 || t.closest("input, button, .fseek, .lyr-note")) return;
    const lyr = t.closest(".flyrics");
    if (lyr && lyr.scrollTop > 0) return;
    swipe = { y: e.touches[0].clientY, x: e.touches[0].clientX, dy: 0, on: false };
  }, { passive: true });
  full.addEventListener("touchmove", (e) => {
    if (!swipe) return;
    const dy = e.touches[0].clientY - swipe.y, dx = e.touches[0].clientX - swipe.x;
    if (!swipe.on) {
      if (Math.abs(dx) > Math.abs(dy) || dy < 8) {
        if (Math.abs(dx) > 12 || dy < -8) swipe = null;
        return;
      }
      swipe.on = true;
      full.classList.add("mdrag");
    }
    swipe.dy = Math.max(0, dy);
    full.style.transform = `translateY(${swipe.dy}px)`;
  }, { passive: true });
  const endSwipe = () => {
    if (!swipe) return;
    const close = swipe.on && swipe.dy > Math.min(160, innerHeight * 0.22);
    swipe = null;
    full.classList.remove("mdrag");
    if (close) closeFull(); // уезжает вниз с того места, куда дотянули
    else full.style.transform = "";
  };
  full.addEventListener("touchend", endSwipe);
  full.addEventListener("touchcancel", endSwipe);

  // ---------- «Сейчас играет»: выезжает из мини-плеера и уезжает обратно ----------
  // Открытие — класс mopen (анимация в mobile.css), закрытие — обёртка над closeFull из app.js (все её вызовы —
  // крестик, «Назад», Escape, смахивание — идут через неё): сначала экран уезжает вниз, потом прячется.
  const animOn = () => document.documentElement.dataset.anim !== "off" && !matchMedia("(prefers-reduced-motion: reduce)").matches;
  new MutationObserver(() => {
    if (full.hidden || !animOn()) return;
    full.classList.remove("mopen");
    void full.offsetWidth;
    full.classList.add("mopen");
  }).observe(full, { attributes: true, attributeFilter: ["hidden"] });
  full.addEventListener("animationend", () => full.classList.remove("mopen"));
  const realClose = closeFull;
  let closing = 0;
  closeFull = function () {
    if (full.hidden || !animOn()) return realClose.apply(this, arguments);
    if (closing) return;
    full.classList.remove("mopen");
    full.style.transition = "transform .22s cubic-bezier(.4, 0, 1, 1)";
    full.style.transform = "translateY(100%)";
    closing = setTimeout(() => {
      closing = 0;
      realClose();
      full.style.transition = full.style.transform = "";
    }, 220);
  };

  // ---------- «Очередь и история»: смахнуть вбок — закрыть ----------
  // Панель (модуль ядра, core/queue.js) на телефоне — на весь экран (mobile.css). Тянется за пальцем по горизонтали;
  // отпустили дальше трети ширины — уезжает и закрывается своей кнопкой. Не с ручки перетаскивания строк (.grip).
  let qs = null;
  document.addEventListener("touchstart", (e) => {
    const p = e.target.closest(".qpanel");
    if (!p || p.hidden || e.touches.length !== 1 || e.target.closest(".grip, input, .tmenu")) return;
    qs = { p, x: e.touches[0].clientX, y: e.touches[0].clientY, dx: 0, on: false };
  }, { passive: true });
  document.addEventListener("touchmove", (e) => {
    if (!qs) return;
    const dx = e.touches[0].clientX - qs.x, dy = e.touches[0].clientY - qs.y;
    if (!qs.on) {
      if (Math.abs(dy) > 10 && Math.abs(dy) > Math.abs(dx)) { qs = null; return; } // прокрутка списка
      if (Math.abs(dx) < 12) return;
      qs.on = true;
      qs.p.style.transition = "none";
    }
    qs.dx = dx;
    qs.p.style.transform = `translateX(${dx}px)`;
    qs.p.style.opacity = String(Math.max(0.3, 1 - Math.abs(dx) / qs.p.offsetWidth));
  }, { passive: true });
  const qEnd = () => {
    if (!qs) return;
    const { p, dx, on } = qs;
    qs = null;
    if (!on) return;
    p.style.transition = "transform .18s ease-out, opacity .18s ease-out";
    if (Math.abs(dx) > p.offsetWidth / 3) {
      p.style.transform = `translateX(${dx > 0 ? "" : "-"}110%)`;
      p.style.opacity = "0";
      setTimeout(() => {
        p.querySelector(".qclose")?.click();
        p.style.transition = p.style.transform = p.style.opacity = "";
      }, 180);
    } else {
      p.style.transform = p.style.opacity = "";
      setTimeout(() => (p.style.transition = ""), 200);
    }
  };
  document.addEventListener("touchend", qEnd);
  document.addEventListener("touchcancel", qEnd);

  // ---------- «Назад» Android ----------
  // открыто меню, окошко подтверждения или пояснение Genius
  const popupOpen = () =>
    [...document.querySelectorAll(".popup, .tmenu, #lyrNote")].some((p) => !p.hidden && getComputedStyle(p).display !== "none") ||
    !!document.querySelector(".confirm-pop");

  function onBack() {
    const a = document.activeElement;
    if (a && a.tagName === "INPUT" && a.type !== "range") return a.blur();
    // окно (эквалайзер, цвета, справка) закрывается своим Escape: оно ловит его на document при погружении,
    // поэтому — событие на body (до обработчика app.js, который закрыл бы и «Сейчас играет», событие не дойдёт)
    if (document.querySelector(".modal, .ccmodal")) {
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      return;
    }
    if (window.Viz && Viz.full()) return Viz.exitFull();
    if (popupOpen()) {
      closePopups();
      closeNote();
      return;
    }
    // очередь и история (модуль ядра) — своей кнопкой закрытия
    const q = document.querySelector(".qpanel:not([hidden]) .qclose");
    if (q) return q.click();
    if (!full.hidden) return closeFull();
    const back = $("#back");
    if (back && !back.hidden) return back.click();
    if (S.page !== "library") return go("library");
    invoke("app_background").catch(() => {});
  }

  // ---------- поворот экрана ----------
  // «Общее → Поворот экрана» (просьба владельца, 2026-11-18): выключено — всегда вертикально, включено — как велит
  // автоповорот телефона. Выбор — localStorage vkm_rotate (входит в резервную копию); Kotlin запоминает его
  // и сам (VkmPlugin.rotate → MainActivity), чтобы при следующем запуске экран не успел повернуться до интерфейса
  const ROT_KEY = "vkm_rotate";
  let rotate = true;
  try { rotate = localStorage.getItem(ROT_KEY) !== "0"; } catch {}
  const sendRotate = () => invoke("app_rotate", { on: rotate }).catch(() => {});
  sendRotate();
  window.renderRotateCard = () => {
    const c = setCard("ui", L("Поворот экрана"));
    c.append(switchRow(L("Поворачивать экран вместе с телефоном"), rotate, (on) => {
      rotate = on;
      try { localStorage.setItem(ROT_KEY, on ? "1" : "0"); } catch {}
      sendRotate();
    }));
    c.append(el("p", "status", L("Выключено — VK Music всегда вертикально. Включено — как велит автоповорот в настройках телефона.")));
  };

  // Tauri: «Назад» приходит событием, пока на него подписаны (иначе Android сам закрыл бы приложение)
  const app = TAURI.app;
  if (app && typeof app.onBackButtonPress === "function") app.onBackButtonPress(onBack).catch(() => {});
})();
