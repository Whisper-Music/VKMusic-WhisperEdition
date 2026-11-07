"use strict";
// Глобальные горячие клавиши (как hotkeys.js у Whisper Yandex): карточка в «Настройки → Общее» и окно назначения.
// Регистрирует их Rust (src-tauri/src/hotkeys.rs): работают и в свёрнутом окне. Нажатие приходит событием `hotkey`,
// действие выполняет app.js (onHotkey). Сочетания — в localStorage (vkm_hotkeys) этого компьютера.
(() => {
  const KEY = "vkm_hotkeys";
  // По умолчанию — Ctrl+Alt, как у классических плееров, и только то, что не занято ни Windows, ни популярными
  // оболочками Linux (без Alt+Shift — раскладка, Win/Super — системные, Ctrl+Alt+стрелки, F1…F12, L/K/T/D/Del).
  const ACTIONS = [
    ["toggle", "Пауза / играть", "Ctrl+Alt+Space"],
    ["next", "Следующий трек", "Ctrl+Alt+PageDown"],
    ["prev", "Предыдущий трек", "Ctrl+Alt+PageUp"],
    ["vol_up", "Громче", "Ctrl+Alt+Equal"],
    ["vol_down", "Тише", "Ctrl+Alt+Minus"],
    ["mute", "Выключить / включить звук", "Ctrl+Alt+Digit0"],
    ["seek_fwd", "Перемотать вперёд на 10 с", "Ctrl+Alt+Period"],
    ["seek_back", "Перемотать назад на 10 с", "Ctrl+Alt+Comma"],
    ["like", "В библиотеку", "Ctrl+Alt+Insert"],
    ["repeat", "Повтор", "Ctrl+Alt+KeyR"],
    ["shuffle", "Перемешать", "Ctrl+Alt+KeyH"],
    ["show", "Показать / свернуть окно", "Ctrl+Alt+Home"],
  ];
  // один порядок модификаторов — «Shift+Alt+X» и «Alt+Shift+X» одно сочетание
  const ORDER = ["Ctrl", "Alt", "Shift", "Super"];
  const canon = (accel) => {
    if (typeof accel !== "string" || !accel) return "";
    const parts = accel.split("+"), code = parts.pop();
    return [...ORDER.filter((m) => parts.includes(m)), code].join("+");
  };
  const DEF = () => Object.fromEntries(ACTIONS.map(([a, , k]) => [a, canon(k)]));
  const hk = { on: true, binds: DEF() };
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    if (v && typeof v === "object") {
      hk.on = v.on !== false;
      for (const [a] of ACTIONS) if (typeof v.binds?.[a] === "string") hk.binds[a] = canon(v.binds[a]);
    }
  } catch {}
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(hk)); } catch {} };
  const call = (cmd, args) => Promise.resolve(window.__TAURI__.core.invoke(cmd, args));
  let errors = {};    // действие → почему не зарегистрировалось
  let paused = false; // окно назначения открыто — сочетания сняты, чтобы их можно было нажать
  let note = null, sw = null;

  const KEYNAME = {
    Space: "Пробел", ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓", Comma: ",", Period: ".", Slash: "/",
    Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Backquote: "`", Minus: "−", Equal: "=",
    PageUp: "PgUp", PageDown: "PgDn", Escape: "Esc", PrintScreen: "PrtSc", ScrollLock: "ScrLk",
    MediaPlayPause: "⏯", MediaTrackNext: "⏭", MediaTrackPrevious: "⏮", MediaStop: "⏹",
    AudioVolumeUp: "🔊", AudioVolumeDown: "🔉", AudioVolumeMute: "🔇",
    NumpadAdd: "Num +", NumpadSubtract: "Num −", NumpadMultiply: "Num *", NumpadDivide: "Num /", NumpadDecimal: "Num .", NumpadEnter: "Num Enter",
  };
  const keyName = (code) => (KEYNAME[code] ? L(KEYNAME[code]) : code.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Numpad(\d)$/, "Num $1"));
  const MOD_NAME = { Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift", Super: "Win" };
  const pretty = (accel) => {
    if (!accel) return "";
    const parts = accel.split("+"), code = parts.pop();
    return [...parts.map((m) => MOD_NAME[m] || m), keyName(code)].join(" + ");
  };
  // без Ctrl/Alt/Shift можно только медиаклавиши и F13–F24: не мешают ни набору текста, ни играм
  const SOLO = /^(Media\w+|AudioVolume\w+|F(1[3-9]|2[0-4]))$/;
  const SUPPORTED = /^(Key[A-Z]|Digit\d|Numpad\w+|F\d{1,2}|Arrow(Up|Down|Left|Right)|Space|Comma|Period|Slash|Semicolon|Quote|BracketLeft|BracketRight|Backslash|Backquote|Minus|Equal|Enter|Tab|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Escape|PrintScreen|ScrollLock|Pause|Media\w+|AudioVolume\w+)$/;

  async function apply() {
    const binds = hk.on && !paused ? ACTIONS.map(([a]) => ({ action: a, accel: hk.binds[a] || "" })).filter((b) => b.accel) : [];
    let res = [];
    try { res = await call("hotkeys_set", { binds }); } catch (e) { console.warn("hotkeys:", e); }
    if (!paused) {
      errors = {};
      for (const r of res || []) if (r.error) errors[r.action] = r.error;
    }
    paintCard();
    paintRows();
  }

  function paintCard() {
    if (sw) sw.checked = hk.on;
    if (!note) return;
    const set = ACTIONS.filter(([a]) => hk.binds[a]).length, bad = Object.keys(errors).length;
    note.textContent = !hk.on ? L("Выключены") : bad ? L("Не сработают: {0}", bad) : L("Назначено: {0}", set);
    note.classList.toggle("bad", hk.on && bad > 0);
  }

  // ---------- окно назначения ----------
  let m = null, recording = null;
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };

  function paintRows() {
    if (!m) return;
    for (const [a] of ACTIONS) {
      const r = m.rows[a], acc = hk.binds[a] || "";
      r.key.classList.toggle("rec", recording === a);
      r.key.classList.toggle("empty", !acc && recording !== a);
      r.key.textContent = recording === a ? L("Нажмите сочетание…") : acc ? pretty(acc) : L("Не задано");
      r.clear.hidden = !acc || recording === a;
      r.note.textContent = recording === a ? "" : errors[a] ? L(errors[a]) : "";
      r.note.classList.toggle("bad", !!errors[a]);
    }
    m.hint.textContent = recording ? L("Esc — отмена, Backspace — убрать сочетание") : "";
  }

  function onKey(e) {
    if (!m) return;
    if (!recording) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    const code = e.code;
    if (/^(Shift|Alt|Control|Meta|OS)(Left|Right)$/.test(code)) return;
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
    if (!mods.length && code === "Escape") { recording = null; return paintRows(); }
    if (!mods.length && (code === "Backspace" || code === "Delete")) {
      hk.binds[recording] = "";
      recording = null;
      save();
      return paintRows();
    }
    if (!SUPPORTED.test(code)) return (m.hint.textContent = L("Эту клавишу назначить нельзя"));
    if (!mods.length && !SOLO.test(code)) return (m.hint.textContent = L("Нужен Ctrl, Alt или Shift"));
    const accel = canon([...mods, code].join("+"));
    // то же сочетание у другого действия — переносится сюда
    for (const [a] of ACTIONS) if (a !== recording && hk.binds[a] === accel) hk.binds[a] = "";
    hk.binds[recording] = accel;
    recording = null;
    save();
    paintRows();
  }

  function open() {
    if (m) return;
    paused = true;
    apply(); // сочетания сняты — иначе система не даст нажать их здесь
    const wrap = el("div", "login modal");
    const box = el("div", "panel login-box modalbox hkmodal");
    box.append(el("h2", null, L("Горячие клавиши")));
    const list = el("div", "hklist");
    const rows = {};
    for (const [a, label] of ACTIONS) {
      const row = el("div", "hkrow");
      const key = el("button", "hkkey");
      key.type = "button";
      key.addEventListener("click", () => { recording = recording === a ? null : a; paintRows(); });
      const clear = el("button", "hkclear", "✕");
      clear.type = "button";
      clear.title = L("Убрать сочетание");
      clear.addEventListener("click", () => { hk.binds[a] = ""; delete errors[a]; save(); paintRows(); });
      const rn = el("small", "hknote");
      row.append(el("span", "hkname", L(label)), key, clear, rn);
      list.append(row);
      rows[a] = { key, clear, note: rn };
    }
    const hint = el("p", "status hkhint");
    const foot = el("div", "row");
    const reset = el("button", "nav", L("По умолчанию"));
    reset.type = "button";
    reset.addEventListener("click", () => { hk.binds = DEF(); errors = {}; recording = null; save(); paintRows(); });
    const done = el("button", "nav active", L("Готово"));
    done.type = "button";
    done.addEventListener("click", close);
    foot.append(reset, done);
    box.append(el("p", "hksub", L("Работают и в свёрнутом окне. Нажмите на сочетание, чтобы назначить новое.")), list, hint, foot);
    wrap.append(box);
    document.body.append(wrap);
    wrap.addEventListener("pointerdown", (e) => e.target === wrap && close());
    document.addEventListener("keydown", onKey, true);
    m = { wrap, rows, hint };
    paintRows();
  }

  function close() {
    if (!m) return;
    document.removeEventListener("keydown", onKey, true);
    m.wrap.remove();
    m = null;
    recording = null;
    paused = false;
    apply();
  }

  /** Карточка в настройках (app.js, renderGeneral): переключатель, сколько назначено, «Настроить». */
  function card(c) {
    const row = el("label", "switch");
    sw = el("input");
    sw.type = "checkbox";
    sw.addEventListener("change", () => { hk.on = sw.checked; save(); apply(); });
    row.append(sw, el("i"), el("span", null, L("Глобальные горячие клавиши")));
    note = el("p", "status hkcount");
    const btn = el("button", "nav", L("Настроить…"));
    btn.type = "button";
    btn.addEventListener("click", open);
    const r = el("div", "row");
    r.append(btn);
    // клавиши внутри окна (keynav.js) — их список
    if (window.kbHelp) {
      const kb = el("button", "nav", L("Клавиши в окне (F1)"));
      kb.type = "button";
      kb.addEventListener("click", () => window.kbHelp());
      r.append(kb);
    }
    c.append(row, note, r);
    paintCard();
  }

  window.Hotkeys = { card };
  // В браузере (--host) глобальных сочетаний нет — окна приложения у вкладки нет
  if (!window.__TAURI__?.__host) apply();
})();
