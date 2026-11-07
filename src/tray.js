// Значок в трее и мини-плеер — сторона главного окна (Rust: src-tauri/src/tray.rs, окно mini.html).
// * Что играет — мини-плееру (mini_state) и меню значка (tray_state: «Пауза» / «Играть», подписи на языке интерфейса,
//   подсказка). Источник — шина ядра Whisper (track, playstate, like, modes, seek, rate), без своих опросов.
// * Кнопки меню и мини-плеера приходят событием tray://ctl → playerAction (app.js), как глобальные горячие клавиши.
// * Настройки — карточка «Трей» во вкладке «Общее»: значок, его вид (tray.rs ICONS), «Закрывать в трей».
//   Вид «Цвет темы» — полоски цветом --accent: при смене стиля новый цвет уходит в tray_prefs (accent).
"use strict";

(function () {
  if (window.__TAURI__?.__host) return; // в браузере (--host) ни значка, ни мини-плеера
  let timer = 0;
  let trayInfo = { available: false, tray: false, close_to_tray: false, icon: "app" };
  let sentAccent = "";
  // виды значка: id (tray.rs ICONS) → подпись и подсказка
  const LOOKS = [
    ["app", "Цветной", "Цветами значка приложения"],
    ["light", "Светлый", "Для тёмной панели"],
    ["dark", "Тёмный", "Для светлой панели"],
    ["accent", "Цвет темы", "Цветом текущего стиля оформления"],
  ];

  // --accent темы как #rrggbb: в теме он может быть любым CSS-цветом (rgb(), color-mix()…) — приводит холст
  let probe = null;
  function accentHex() {
    try {
      probe ||= document.createElement("canvas").getContext("2d", { willReadFrequently: true });
      const cs = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
      if (!cs) return "";
      probe.clearRect(0, 0, 1, 1);
      probe.fillStyle = "#000";
      probe.fillStyle = cs;
      probe.fillRect(0, 0, 1, 1);
      const [r, g, b] = probe.getImageData(0, 0, 1, 1).data;
      return "#" + [r, g, b].map((x) => x.toString(16).padStart(2, "0")).join("");
    } catch {
      return "";
    }
  }

  function syncAccent() {
    if (!trayInfo.tray || trayInfo.icon !== "accent") return;
    const a = accentHex();
    if (!a || a === sentAccent) return;
    sentAccent = a;
    invoke("tray_prefs", { accent: a }).catch(() => (sentAccent = ""));
  }

  const labels = () => ({
    show: L("Открыть VK Music"), mini: L("Мини-плеер"), prev: L("Предыдущий"), next: L("Следующий"),
    play: L("Играть"), pause: L("Пауза"), quit: L("Выход"), hide: L("Закрыть"), like: L("В библиотеку"),
    shuffle: L("Перемешать"), idle: L("Ничего не играет"), untitled: L("Без названия"),
  });

  function snapshot() {
    const t = S.current;
    const cs = getComputedStyle(document.documentElement);
    const v = (name) => cs.getPropertyValue(name).trim();
    const playing = !!t && !S.pl.paused && !S.pl.idle;
    return {
      has: !!t,
      title: t ? t.title || "" : "",
      artist: t ? t.artist || "" : "",
      cover: t && t.cover ? t.cover : "",
      playing,
      pos: S.pl.pos || 0,
      dur: S.pl.duration || (t && t.duration) || 0,
      at: Date.now(),
      rate: S.rate || 1,
      shuffle: !!S.shuffle,
      liked: !!(t && libraryMatch(t)),
      likeOk: !!t && !t.room,
      accent: v("--accent"), accent2: v("--accent2") || v("--accent"),
      bg: v("--bg"), text: v("--text"), muted: v("--muted"),
      lang: I18N.lang,
      labels: labels(),
    };
  }

  function push() {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const s = snapshot();
      invoke("mini_state", { state: s }).catch(() => {});
      syncAccent();
      if (trayInfo.tray) invoke("tray_state", { labels: s.labels, playing: s.playing, title: s.has ? [s.artist, s.title].filter(Boolean).join(" — ") : "" }).catch(() => {});
    }, 120);
  }

  for (const ev of ["track", "playstate", "like", "modes", "seek", "rate", "app"]) Whisper.on(ev, push);
  // смена стиля (цвета мини-плеера) и языка — после применения темы / setLang
  new MutationObserver(push).observe(document.documentElement, { attributes: true, attributeFilter: ["data-style", "lang", "style"] });

  /** Карточка «Трей» во вкладке «Общее» (renderSettings в app.js). */
  window.renderTraySettings = function renderTraySettings() {
    if (!trayInfo.available) return;
    const c = setCard("ui", L("Трей"));
    c.append(switchRow(L("Значок в трее"), trayInfo.tray, async (on) => {
      try {
        await invoke("tray_prefs", { tray: on });
        trayInfo.tray = on;
        push();
        renderSettings();
      } catch (e) {
        toast(errMsg(e), "error");
      }
    }));
    if (trayInfo.tray) {
      const row = el("div", "lyrow trayrow");
      const seg = el("div", "lyseg trayicons");
      for (const [id, name, tip] of LOOKS) {
        const b = el("button", "chip trayico" + (trayInfo.icon === id ? " active" : ""));
        b.type = "button";
        b.dataset.v = id;
        b.title = L(tip);
        b.setAttribute("aria-pressed", trayInfo.icon === id ? "true" : "false");
        const pic = el("span", "tib tib-" + id);
        pic.setAttribute("aria-hidden", "true");
        pic.append(el("i"), el("i"), el("i"));
        b.append(pic, el("span", "", L(name)));
        b.addEventListener("click", async () => {
          if (trayInfo.icon === id) return;
          try {
            const accent = id === "accent" ? accentHex() : "";
            await invoke("tray_prefs", accent ? { icon: id, accent } : { icon: id });
            trayInfo.icon = id;
            sentAccent = accent;
            for (const x of seg.children) {
              const on = x.dataset.v === id;
              x.classList.toggle("active", on);
              x.setAttribute("aria-pressed", on ? "true" : "false");
            }
          } catch (e) {
            toast(errMsg(e), "error");
          }
        });
        seg.append(b);
      }
      row.append(el("span", "", L("Вид значка")), seg);
      c.append(row);
      c.append(switchRow(L("Закрывать в трей — музыка играет дальше"), trayInfo.close_to_tray, (on) => {
        trayInfo.close_to_tray = on;
        invoke("tray_prefs", { close_to_tray: on }).catch((e) => toast(errMsg(e), "error"));
      }));
    }
    c.append(el("p", "status", L("В меню значка — «Мини-плеер»: маленькое окно с обложкой и кнопками поверх остальных. В GNOME значок виден только с расширением AppIndicator.")));
    const row = el("div", "row");
    const b = el("button", "nav", L("Мини-плеер"));
    b.type = "button";
    b.addEventListener("click", () => invoke("mini_toggle").catch((e) => toast(errMsg(e), "error")));
    row.append(b);
    c.append(row);
  };

  /** При запуске (boot в app.js): что умеет и что включено. */
  window.trayBoot = function trayBoot(info) {
    trayInfo = { available: !!info.tray_available, tray: !!info.tray_available && !!info.tray, close_to_tray: !!info.close_to_tray, icon: info.tray_icon || "app" };
    push();
  };
})();
