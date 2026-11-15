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

  // ---------- верхняя строка: поиск и аккаунт ----------
  const search = $("#search");
  const main = $(".main");
  const account = el("button", "maccount");
  account.type = "button";
  account.setAttribute("aria-label", L("Сервисы"));
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
  const NAV_ICONS = { wave: "mix", library: "library", vk: "playlist", stats: "chart", settings: "gear" };
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
    full.style.transform = "";
    if (close) closeFull();
  };
  full.addEventListener("touchend", endSwipe);
  full.addEventListener("touchcancel", endSwipe);

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

  // Tauri: «Назад» приходит событием, пока на него подписаны (иначе Android сам закрыл бы приложение)
  const app = TAURI.app;
  if (app && typeof app.onBackButtonPress === "function") app.onBackButtonPress(onBack).catch(() => {});
})();
