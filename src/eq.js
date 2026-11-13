"use strict";
// Эквалайзер (как eq.js у Whisper Yandex): 10 полос (32 Гц — 16 кГц), предусиление, готовые настройки.
// Окно открывается из «⋯» плеера (whisper-vk.js, playerMenuItems). Звук меняет mpv — фильтр `@vkeq` в цепочке `af`
// (player::Eq в player.rs): пока эквалайзер выключен, фильтра нет вовсе; полосы двигаются на ходу (af-command),
// без перезапуска трека. В настройки (config.json → eq) — когда ползунок отпустили (player_eq { save: true }).
(() => {
  const BANDS = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
  const MAX = 12;
  // id — то, что хранится (латиница), подпись — на языке интерфейса
  const presets = () => [
    ["flat", Lc("эквалайзер", "Обычный"), [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
    ["bass", L("Больше баса"), [6, 5, 4, 2, 0, 0, 0, 0, 0, 0]],
    ["bassplus", L("Мощный бас"), [9, 8, 6, 3, 0, -1, 0, 0, 0, 0]],
    ["treble", L("Высокие"), [0, 0, 0, 0, 0, 1, 2, 4, 5, 6]],
    ["vocal", L("Вокал"), [-2, -2, -1, 1, 3, 4, 4, 3, 1, 0]],
    ["rock", L("Рок"), [5, 4, 2, 0, -1, -1, 1, 3, 4, 4]],
    ["pop", L("Поп"), [-1, 1, 3, 4, 3, 0, -1, -1, 0, 1]],
    ["electro", L("Электроника"), [6, 5, 2, 0, -2, 1, 0, 2, 4, 5]],
    ["jazz", L("Джаз"), [3, 2, 1, 2, -1, -1, 0, 1, 2, 3]],
    ["classic", L("Классика"), [3, 2, 1, 0, 0, 0, -1, 0, 2, 3]],
    ["night", L("Тихо, для сна"), [2, 1, 0, 0, 0, -1, -2, -4, -6, -8]],
  ];
  const clampDb = (v) => Math.max(-MAX, Math.min(MAX, Math.round((Number(v) || 0) * 2) / 2));

  // состояние — из app_info (S.eq), дальше — своё; в Rust уходит целиком
  let st = null;
  function state() {
    if (st) return st;
    const v = (typeof S === "object" && S.eq) || {};
    const gains = Array.isArray(v.gains) && v.gains.length === BANDS.length ? v.gains.map(clampDb) : BANDS.map(() => 0);
    st = { on: !!v.on, pre: clampDb(v.pre), gains, guard: v.guard !== false, preset: typeof v.preset === "string" ? v.preset : "flat" };
    return st;
  }

  // ---------- в Rust ----------
  // Пока тянут ползунок — не чаще раза в 60 мс и без записи на диск; отпустили, выбрали настройку — сразу и с записью.
  let sendT = 0, lastAt = 0;
  function send(save) {
    clearTimeout(sendT);
    sendT = 0;
    const eq = { ...st, gains: st.gains.slice() };
    S.eq = eq;
    lastAt = Date.now();
    invoke("player_eq", { eq, save }).then(() => note(""), (e) => note(errMsg(e)));
  }
  function sendSoon() {
    if (sendT) return;
    sendT = setTimeout(() => send(false), Math.max(0, 60 - (Date.now() - lastAt)));
  }

  // ---------- окно ----------
  let ui = null;
  const fmtDb = (v) => (v > 0 ? "+" : v < 0 ? "−" : "") + Math.abs(v).toLocaleString(document.documentElement.lang || "ru");
  const fmtHz = (f) => (f >= 1000 ? L("{0}к", f / 1000) : String(f));
  function note(text) { if (ui) ui.note.textContent = text; }

  // вертикальный ползунок: обычный горизонтальный, повёрнутый на 90° (eq-стили в app.css), заливка — от нуля
  function slider(label, value, onInput, cls = "") {
    const band = el("div", "eqband " + cls);
    const val = el("span", "eqv");
    const slot = el("div", "eqslot");
    const inp = el("input");
    inp.type = "range";
    inp.min = -MAX;
    inp.max = MAX;
    inp.step = 0.5;
    inp.value = value;
    inp.setAttribute("aria-label", label);
    const show = () => {
      val.textContent = fmtDb(Number(inp.value));
      const p = ((inp.value - -MAX) / (2 * MAX)) * 100;
      inp.style.setProperty("--a", Math.min(p, 50) + "%");
      inp.style.setProperty("--b", Math.max(p, 50) + "%");
    };
    inp.addEventListener("input", () => { onInput(Number(inp.value)); show(); sendSoon(); });
    inp.addEventListener("change", () => send(true));
    inp.addEventListener("dblclick", () => { inp.value = 0; onInput(0); show(); send(true); }); // двойной щелчок — в ноль
    slot.append(inp);
    band.append(val, slot, el("small", null, label));
    show();
    return { band, inp, show };
  }

  function paintMain() {
    if (!ui) return;
    ui.sw.checked = st.on;
    ui.box.classList.toggle("off", !st.on);
    ui.chips.querySelectorAll(".chip").forEach((c) => c.classList.toggle("active", c.dataset.p === st.preset));
    ui.guard.checked = st.guard;
  }
  function syncSliders() {
    if (!ui) return;
    ui.bands.forEach((b, i) => { b.inp.value = st.gains[i]; b.show(); });
    ui.pre.inp.value = st.pre;
    ui.pre.show();
  }

  function open() {
    if (ui) return;
    state();
    const wrap = el("div", "login modal eqwrap");
    const box = el("div", "panel login-box modalbox eqbox");
    box.setAttribute("role", "dialog");
    box.setAttribute("aria-label", L("Эквалайзер"));

    const head = el("div", "eqhead");
    const swl = el("label", "switch eqsw");
    const sw = el("input");
    sw.type = "checkbox";
    swl.append(sw, el("i"), el("span", null, L("Включён")));
    const x = el("button", "round eqclose", "✕");
    x.type = "button";
    x.setAttribute("aria-label", L("Закрыть"));
    head.append(el("h2", null, L("Эквалайзер")), swl, x);

    const chips = el("div", "chips eqpresets");
    for (const [id, name, g] of presets()) {
      const c = el("button", "chip", name);
      c.type = "button";
      c.dataset.p = id;
      c.addEventListener("click", () => { st.gains = g.slice(); st.preset = id; st.on = true; syncSliders(); paintMain(); send(true); });
      chips.append(c);
    }
    const custom = el("button", "chip", L("Свой"));
    custom.type = "button";
    custom.dataset.p = "custom";
    custom.disabled = true;
    chips.append(custom);

    const grid = el("div", "eqgrid");
    const touched = () => { if (!st.on) { st.on = true; paintMain(); } };
    const preS = slider(L("Усил."), st.pre, (v) => { st.pre = v; touched(); }, "eqpre");
    const bands = BANDS.map((f, i) => slider(fmtHz(f), st.gains[i], (v) => {
      st.gains[i] = v;
      if (st.preset !== "custom") { st.preset = "custom"; paintMain(); }
      touched();
    }));
    grid.append(preS.band, el("div", "eqsep"), ...bands.map((b) => b.band));

    const foot = el("div", "eqfoot");
    const gl = el("label", "switch");
    const guard = el("input");
    guard.type = "checkbox";
    gl.append(guard, el("i"), el("span", null, L("Без искажений")));
    const reset = el("button", "nav", L("Сбросить"));
    reset.type = "button";
    reset.addEventListener("click", () => { st.gains = BANDS.map(() => 0); st.pre = 0; st.preset = "flat"; syncSliders(); paintMain(); send(true); });
    foot.append(gl, reset);
    const hint = el("p", "eqhint", L("«Без искажений»: когда полосы подняты, общая громкость немного снижается, чтобы звук не хрипел. Двойной щелчок по ползунку — в ноль."));
    const nt = el("p", "eqnote");

    box.append(head, chips, grid, foot, hint, nt);
    wrap.append(box);
    document.body.append(wrap);

    ui = { wrap, box, sw, chips, guard, bands, pre: preS, note: nt };
    sw.addEventListener("change", () => { st.on = sw.checked; paintMain(); send(true); });
    guard.addEventListener("change", () => { st.guard = guard.checked; send(true); });
    const onKey = (e) => { if (e.key === "Escape") { e.stopPropagation(); close(); } };
    function close() {
      if (sendT) send(true); // последний шаг ползунка — не потерять
      document.removeEventListener("keydown", onKey, true);
      wrap.remove();
      ui = null;
    }
    document.addEventListener("keydown", onKey, true);
    x.addEventListener("click", close);
    wrap.addEventListener("pointerdown", (e) => e.target === wrap && close());
    paintMain();
    sw.focus();
  }

  window.Eq = { open, on: () => !!state().on };
})();
