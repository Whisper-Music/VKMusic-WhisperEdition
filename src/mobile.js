"use strict";
// Телефон (Android; класс html.mobile ставит boot.js, раскладка — mobile.css). Разметка та же, что на компьютере
// (контракт с темами Whisper), только несколько элементов переставляются туда, где до них дотянется палец:
// * поиск — из боковой панели (на телефоне это полоса разделов внизу) наверх страницы;
// * режимы плеера (перемешать, повтор, текст, «Постоянный уровень громкости», сон, кнопки модулей ядра —
//   «Слушать вместе», очередь) — на экран «Сейчас играет», «⋯» играющего трека — к кнопкам плеера; id и
//   обработчики остаются прежними;
// * «Назад» Android: закрыть всплывающее → «Сейчас играет» → подстраницу (плейлист, поиск, магазин тем) → к
//   библиотеке → свернуть приложение (музыка играет дальше, app_background).
(() => {
  if (!MOBILE) return;

  const search = $("#search");
  const main = $(".main");
  if (search && main) {
    const box = el("div", "msearch");
    main.prepend(box);
    box.append(search);
  }

  const fctrl = $("#full .fctrl");
  const pright = $(".player .pright");
  const more = $("#pMore");
  const ctrls = $(".player .ctrls");
  if (more && ctrls) ctrls.append(more);
  if (fctrl) {
    const modes = el("div", "fmodes");
    for (const id of ["shuffleBtn", "repeatBtn"]) {
      const b = document.getElementById(id);
      if (b) modes.append(b);
    }
    fctrl.append(modes);
    // правый край плеера (текст, громкость, сон и кнопки модулей ядра — «Слушать вместе», очередь) — сюда же;
    // модули подключаются позже и добавляют кнопки сами, поэтому — и всё, что появится там потом
    const adopt = () => {
      for (const b of [...pright.children]) if (b.id !== "volWrap" && b !== more) modes.append(b);
    };
    if (pright) {
      adopt();
      new MutationObserver(adopt).observe(pright, { childList: true });
    }
  }

  // название в плеере открывает «Сейчас играет», как обложка
  const meta = $(".player .meta");
  if (meta) meta.addEventListener("click", () => $("#cover").click());

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
    if (!$("#full").hidden) return closeFull();
    const back = $("#back");
    if (back && !back.hidden) return back.click();
    if (S.page !== "library") return go("library");
    invoke("app_background").catch(() => {});
  }

  // Tauri: «Назад» приходит событием, пока на него подписаны (иначе Android сам закрыл бы приложение)
  const app = TAURI.app;
  if (app && typeof app.onBackButtonPress === "function") app.onBackButtonPress(onBack).catch(() => {});
})();
