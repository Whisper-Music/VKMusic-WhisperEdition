"use strict";
// Энергосбережение (просьба владельца, 2026-11-15): «Настройки → Общее → Энергосбережение» — выключено, всегда или
// при заряде ниже порога (не на зарядке — как экономия заряда у Android). Пока действует (класс eco на <html>):
// анимации выключены (data-anim="off", как у темы без анимаций), нет размытия под панелями и фона по обложке,
// видеообои на паузе, визуализатор VK Микса не рисует (viz.js). Музыка, плеер и всё остальное — как обычно.
// Заряд: navigator.getBattery (телефон), иначе battery_status из Rust (Linux, /sys/class/power_supply); нет ни того,
// ни другого (настольный компьютер) — работает только «Всегда». Настройка — localStorage vkm_power { mode, at }.
(() => {
  const KEY = "vkm_power";
  const MODES = ["off", "on", "auto"];
  const LEVELS = [15, 20, 30, 50];
  const cfg = { mode: "off", at: 20 };
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    if (v && MODES.includes(v.mode)) cfg.mode = v.mode;
    if (v && LEVELS.includes(v.at)) cfg.at = v.at;
  } catch {}
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(cfg)); } catch {} };

  /** Заряд: { level 0…1, charging } или null — батареи нет / узнать нельзя. */
  let bat = null;
  let on = false;
  let card = null;

  const want = () => cfg.mode === "on" || (cfg.mode === "auto" && !!bat && !bat.charging && bat.level * 100 <= cfg.at);

  function apply() {
    const next = want();
    if (next === on) return paintCard();
    on = next;
    const root = document.documentElement;
    root.classList.toggle("eco", on);
    // анимации: при выходе из экономии — как задаёт тема (applyStyleVars в app.js); при входе — выключены
    if (on) root.dataset.anim = "off";
    else if (typeof applyStyleVars === "function") applyStyleVars();
    const v = document.getElementById("bgVideo");
    if (v && !v.hidden) {
      if (on) v.pause();
      else if (!document.body.classList.contains("dyn-on")) v.play().catch(() => {});
    }
    window.dispatchEvent(new CustomEvent("power-change", { detail: { on } }));
    // фон по обложке — включается и выключается тем же путём, что и при смене трека
    if (typeof dynUpdate === "function") dynUpdate(S.current);
    paintCard();
  }

  // ---------- заряд ----------
  async function watchBattery() {
    if (navigator.getBattery) {
      try {
        const b = await navigator.getBattery();
        const upd = () => { bat = { level: b.level, charging: b.charging }; apply(); };
        b.addEventListener("levelchange", upd);
        b.addEventListener("chargingchange", upd);
        return upd();
      } catch {}
    }
    const poll = async () => {
      try { bat = await invoke("battery_status"); } catch { bat = null; }
      apply();
    };
    await poll();
    if (bat) setInterval(poll, 60_000);
  }

  // ---------- карточка «Энергосбережение» (Общее) ----------
  function seg(label, items, cur, pick) {
    const row = el("div", "lyrow");
    const g = el("div", "lyseg");
    for (const [v, text] of items) {
      const b = el("button", "chip" + (v === cur ? " active" : ""), text);
      b.type = "button";
      b.addEventListener("click", () => { pick(v); save(); apply(); renderPowerCard(); });
      g.append(b);
    }
    row.append(el("span", null, label), g);
    return row;
  }
  function paintCard() {
    const n = card && card.isConnected && card.querySelector(".powernote");
    if (!n) return;
    const parts = [on ? L("Сейчас включено") : L("Сейчас выключено")];
    if (bat) parts.push(bat.charging ? L("заряд {0}% · заряжается", Math.round(bat.level * 100)) : L("заряд {0}%", Math.round(bat.level * 100)));
    n.textContent = parts.join(" · ");
  }
  function renderPowerCard() {
    const old = card && card.isConnected ? card : null;
    const c = el("div", "card own powercard");
    c.dataset.tab = "ui";
    c.append(el("h3", null, L("Энергосбережение")));
    c.append(el("p", "status", L("Без анимаций, размытия, фона по обложке, видеообоев и визуализатора — музыка играет как обычно.")));
    const modes = [["off", L("Выключено")], ["on", L("Всегда")]];
    if (bat) modes.push(["auto", L("При низком заряде")]);
    c.append(seg(L("Включать"), modes, cfg.mode === "auto" && !bat ? "off" : cfg.mode, (v) => (cfg.mode = v)));
    if (bat && cfg.mode === "auto") {
      c.append(seg(L("Заряд ниже"), LEVELS.map((p) => [p, p + "%"]), cfg.at, (v) => (cfg.at = v)));
      c.append(el("p", "status", L("На зарядке не включается.")));
    }
    if (!bat) c.append(el("p", "status", L("Батареи нет — включается только вручную.")));
    c.append(el("p", "status powernote"));
    if (old) old.replaceWith(c);
    else $("#settingsPage").append(c);
    card = c;
    paintCard();
  }

  window.renderPowerCard = renderPowerCard;
  window.Power = { active: () => on };
  // до первой отрисовки — сразу, без ожидания заряда («Всегда»)
  apply();
  watchBattery().then(() => { if (card && card.isConnected) renderPowerCard(); });
})();
