"use strict";
// Режим сна (как sleep.js у Whisper Yandex): кнопка с луной в нижней панели (#sleepBtn в .pright, перед громкостью;
// осталось минут — #sleepLeft) и карточка «Режим сна» в «Настройки → Общее». Таймер, затухание (последние 3 минуты —
// громкостью mpv, ползунок не двигается) и выключение компьютера — в Rust (sleep.rs): срабатывают, даже если окно
// в трее. Здесь — настройки (localStorage vkm_sleep), подтверждение выключения окошком у кнопки и событие sleep://event.
// У Whisper Yandex режим сна ещё включает «Мою волну» под настроение — у нас играет то, что играло.
// Discord пишет «Засыпает» — через kit.sleeping (whisper-vk.js → Sleep.active).
(() => {
  const KEY = "vkm_sleep";
  const TIMES = [15, 30, 45, 60, 90, 120, 180];
  const cfg = (() => {
    try {
      const v = JSON.parse(localStorage.getItem(KEY) || "{}") || {};
      return { min: TIMES.includes(v.min) ? v.min : 30, off: v.off === true };
    } catch {
      return { min: 30, off: false };
    }
  })();
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(cfg)); } catch {} };
  const lang = () => document.documentElement.lang || "ru";
  const fmtMin = (m) => (m < 60 ? L("{0} мин", m) : L("{0} ч", (m / 60).toLocaleString(lang())));
  const fmtTime = (ms) => new Date(ms).toLocaleTimeString(lang(), { hour: "2-digit", minute: "2-digit" });

  let deadline = 0, willOff = false, tickT = 0, card = null;
  const active = () => deadline > Date.now();

  // ---------- кнопка в нижней панели ----------
  const btn = $("#sleepBtn"), left = $("#sleepLeft");
  btn.prepend(Icons.icon("moon"));
  function paint() {
    const on = active();
    btn.classList.toggle("on", on);
    left.hidden = !on;
    if (on) {
      const m = Math.max(1, Math.ceil((deadline - Date.now()) / 60000));
      const shown = m >= 60 ? L("{0}ч", Math.floor(m / 60)) : String(m);
      if (left.textContent !== shown) left.textContent = shown;
      btn.setAttribute("aria-label", L("Режим сна: осталось {0} — нажмите, чтобы выключить", fmtMin(m)));
    } else {
      btn.setAttribute("aria-label", L("Режим сна"));
    }
    if (card) paintCard();
  }
  function setDeadline(d, shutdown) {
    deadline = d || 0;
    willOff = !!shutdown;
    clearInterval(tickT);
    tickT = deadline ? setInterval(paint, 15000) : 0;
    paint();
  }

  // ---------- включение / выключение ----------
  async function start() {
    let st;
    try {
      st = await invoke("sleep_start", { minutes: cfg.min, shutdown: cfg.off });
    } catch (e) {
      return toast(L("Не удалось включить режим сна: ") + errMsg(e), "error");
    }
    setDeadline(st.deadline, st.shutdown);
    toast(st.shutdown
      ? L("Режим сна на {0}: потом музыка остановится и компьютер выключится", fmtMin(cfg.min))
      : L("Режим сна на {0}: потом музыка остановится", fmtMin(cfg.min)), "ok");
  }
  async function stop() {
    await invoke("sleep_cancel").catch(() => {});
    setDeadline(0);
    toast(L("Режим сна выключен"), "ok");
  }
  // выключение компьютера — только после подтверждения окошком у кнопки (как остальные опасные действия)
  function toggle(anchor) {
    if (active()) return stop();
    if (!cfg.off) return start();
    confirmClick(anchor, start, L("Через {0} музыка остановится, а компьютер выключится. Включить?", fmtMin(cfg.min)), L("Включить"));
  }
  btn.addEventListener("click", (e) => { e.stopPropagation(); toggle(btn); });

  // время вышло (sleep.rs): музыка на паузе; компьютер выключится через минуту — её можно отменить
  listen("sleep://event", (e) => {
    const p = e.payload || {};
    if (p.type !== "end") return;
    setDeadline(0);
    if (p.error) return toast(errMsg(p.error), "error");
    if (p.shutdown) {
      toast(L("Режим сна: компьютер выключится через минуту"), null, {
        label: L("Не выключать"),
        fn: () => invoke("sleep_cancel").then(() => toast(L("Выключение отменено"), "ok"), (err) => toast(errMsg(err), "error")),
      }, 60000);
    }
  });
  // страницу перезагрузили, а таймер в Rust идёт — показываем его
  invoke("sleep_status").then((st) => { if (st && st.deadline) setDeadline(st.deadline, st.shutdown); }).catch(() => {});

  // ---------- карточка «Режим сна» (Общее) ----------
  let parts = null;
  function paintCard() {
    if (!card || !card.isConnected) return (card = null);
    const on = active();
    parts.start.textContent = on ? L("Выключить режим сна") : L("Включить режим сна");
    parts.start.classList.toggle("active", !on);
    parts.note.textContent = !on ? ""
      : willOff ? L("Включён: в {0} музыка остановится, а компьютер выключится", fmtTime(deadline))
      : L("Включён: музыка остановится в {0}", fmtTime(deadline));
  }
  function seg(label, items, cur, pick) {
    const row = el("div", "lyrow");
    const g = el("div", "lyseg");
    for (const [v, text] of items) {
      const b = el("button", "chip" + (v === cur ? " active" : ""), text);
      b.type = "button";
      b.addEventListener("click", () => { pick(v); renderSleepCard(); });
      g.append(b);
    }
    row.append(el("span", null, label), g);
    return row;
  }
  function renderSleepCard() {
    const old = card && card.isConnected ? card : null;
    const c = el("div", "card own sleepcard");
    c.dataset.tab = "ui";
    c.append(el("h3", null, L("Режим сна")));
    c.append(el("p", "status", L("Через заданное время музыка плавно затихнет (последние 3 минуты) и встанет на паузу. Луна в нижней панели включает и выключает режим.")));
    c.append(seg(L("Через"), TIMES.map((m) => [m, fmtMin(m)]), cfg.min, (m) => { cfg.min = m; save(); }));
    c.append(seg(L("Потом"), [[false, L("Остановить музыку")], [true, L("Выключить компьютер")]], cfg.off, (v) => {
      cfg.off = v;
      save();
      if (active() && v !== willOff) toast(L("Сработает при следующем включении режима сна"));
    }));
    const row = el("div", "row");
    const startBtn = el("button", "nav");
    startBtn.type = "button";
    startBtn.addEventListener("click", (e) => { e.stopPropagation(); toggle(startBtn); });
    row.append(startBtn);
    const note = el("p", "status sleepnote");
    c.append(row, note);
    if (old) old.replaceWith(c);
    else $("#settingsPage").append(c);
    card = c;
    parts = { start: startBtn, note };
    paintCard();
  }

  window.renderSleepCard = renderSleepCard;
  window.Sleep = { active };
})();
