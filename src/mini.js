// Мини-плеер (tray.rs). Что играет, цвета стиля и подписи (на языке интерфейса) присылает главное окно через Rust
// (mini_state → событие mini://state); кнопки уходят обратно (mini_ctl → tray://ctl главному окну).
// Данные — только textContent; обложка — только https или vkm:// (Rust отдаёт мини-плееру лишь /cover).
"use strict";

(() => {
  const T = window.__TAURI__;
  const $ = (id) => document.getElementById(id);
  const ctl = (action) => T.core.invoke("mini_ctl", { action }).catch(() => {});
  const SVG = {
    play: '<svg viewBox="0 0 24 24" width="22" height="22"><path fill="currentColor" d="M8 5v14l11-7z"/></svg>',
    pause: '<svg viewBox="0 0 24 24" width="22" height="22"><path fill="currentColor" d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z"/></svg>',
    heart: '<svg viewBox="0 0 24 24" width="18" height="18"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/></svg>',
    heartfill: '<svg viewBox="0 0 24 24" width="18" height="18"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" fill="currentColor"/></svg>',
  };
  const COLOR = /^(#[0-9a-f]{3,8}|rgba?\([\d\s.,%]+\))$/i;
  let s = {}, timer = 0;
  const text = (k, def) => (s.labels && typeof s.labels[k] === "string" ? s.labels[k] : def);

  function render(st) {
    s = st && typeof st === "object" ? st : {};
    const has = !!s.has;
    $("t").textContent = has ? s.title || text("untitled", "Без названия") : text("idle", "Ничего не играет");
    $("a").textContent = has ? s.artist || "" : "";
    const cv = $("cv");
    const cover = typeof s.cover === "string" && /^(https:\/\/|vkm:\/\/localhost\/cover\?|http:\/\/vkm\.localhost\/cover\?)/.test(s.cover) ? s.cover : "";
    if (cover) { if (cv.getAttribute("src") !== cover) cv.src = cover; } else cv.removeAttribute("src");
    $("pp").innerHTML = s.playing ? SVG.pause : SVG.play; // свои значки, не данные
    $("pp").title = s.playing ? text("pause", "Пауза") : text("play", "Играть");
    $("lk").innerHTML = s.liked ? SVG.heartfill : SVG.heart;
    $("lk").classList.toggle("on", !!s.liked);
    $("lk").title = text("like", "В библиотеку");
    $("sh").classList.toggle("on", !!s.shuffle);
    $("sh").title = text("shuffle", "Перемешать");
    $("pv").title = text("prev", "Предыдущий");
    $("nx").title = text("next", "Следующий");
    $("open").title = text("show", "Открыть VK Music");
    $("hide").title = text("hide", "Закрыть");
    for (const b of document.querySelectorAll(".ctl button")) b.disabled = !has;
    $("lk").disabled = !has || !s.likeOk;
    const root = document.documentElement.style;
    for (const [k, v] of [["--ac", s.accent], ["--ac2", s.accent2], ["--bg", s.bg], ["--text", s.text], ["--muted", s.muted]]) {
      if (typeof v === "string" && COLOR.test(v.trim())) root.setProperty(k, v.trim());
    }
    document.documentElement.lang = s.lang === "en" ? "en" : "ru";
    tick();
  }

  // полоска времени идёт сама, пока играет и окно видно
  function tick() {
    clearTimeout(timer);
    const bar = $("bar");
    if (!s.has || !s.dur) { bar.style.width = "0"; return; }
    const pos = s.pos + (s.playing ? ((Date.now() - s.at) / 1000) * (s.rate || 1) : 0);
    bar.style.width = Math.min(100, Math.max(0, (pos / s.dur) * 100)) + "%";
    if (s.playing && document.visibilityState === "visible") timer = setTimeout(tick, 250);
  }
  document.addEventListener("visibilitychange", tick);

  for (const b of document.querySelectorAll("button[data-a]")) {
    b.addEventListener("click", () => {
      const a = b.dataset.a;
      if (a === "toggle" && s.has) { // отклик сразу, не дожидаясь главного окна
        s.pos += s.playing ? ((Date.now() - s.at) / 1000) * (s.rate || 1) : 0;
        s.at = Date.now();
        s.playing = !s.playing;
        render(s);
      }
      ctl(a);
    });
  }
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") ctl("hide");
    else if (e.key === " ") { e.preventDefault(); $("pp").click(); }
    else if (e.key === "ArrowRight") ctl("next");
    else if (e.key === "ArrowLeft") ctl("prev");
  });
  document.addEventListener("contextmenu", (e) => e.preventDefault());

  T.event.listen("mini://state", (e) => render(e.payload));
  T.core.invoke("mini_state_get").then(render).catch(() => render({}));
})();
