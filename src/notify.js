"use strict";
// Уведомления «Слушать вместе» (как у Whisper Yandex, просьба владельца 2026-11-17): друг зовёт в комнату, участник
// «подмигнул». Модули ядра зовут Whisper.ui.kit.notify (whisper-vk.js → window.sysNotify). Rust — notify.rs:
// на Linux — уведомление системы (D-Bus; на Wayland окно в угол экрана не поставить), на Windows — своя карточка
// поверх всех окон в выбранном углу (notifywin.html). Внутри окна и так есть подсказка, поэтому по умолчанию —
// только когда окно свёрнуто или в фоне; «Даже когда окно открыто» — всегда. Звук — отсюда (короткий «дзынь»).
// Настройки — карточка «Уведомления» в «Общем», localStorage vkm_notify. На телефоне нет (там — подсказка в окне).
(() => {
  if (typeof MOBILE !== "undefined" && MOBILE) return;
  const { invoke } = window.__TAURI__.core;
  const KEY = "vkm_notify";
  const WIN = /Windows/.test(navigator.userAgent);
  const DEF = { on: true, invite: true, wink: true, always: false, sound: true, info: "full", corner: "br" };
  const KINDS = ["invite", "wink"];
  const INFO = [["full", "Полностью", "Кто и что: «Вадим зовёт тебя в комнату»"],
    ["short", "Без имён", "Только что случилось: «Тебя зовут в комнату»"],
    ["none", "Скрыть", "Просто «Новое уведомление» — без подробностей"]];
  const CORNERS = [["tl", "Слева сверху"], ["tr", "Справа сверху"], ["bl", "Слева снизу"], ["br", "Справа снизу"]];
  const SHORT = { invite: "Тебя зовут в комнату", wink: "Тебе подмигнули", test: "Так выглядят уведомления" };

  let st = load();
  function load() {
    const out = { ...DEF };
    try {
      const v = JSON.parse(localStorage.getItem(KEY) || "{}");
      if (v && typeof v === "object") {
        for (const k of ["on", "invite", "wink", "always", "sound"]) if (typeof v[k] === "boolean") out[k] = v[k];
        if (INFO.some(([id]) => id === v.info)) out.info = v.info;
        if (CORNERS.some(([id]) => id === v.corner)) out.corner = v.corner;
      }
    } catch {}
    return out;
  }
  const save = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(st));
    } catch {}
  };
  const away = () => document.hidden || !document.hasFocus();
  let lastText = "", lastAt = 0;

  // ---------- звук: два тона с мягкой атакой и затуханием (тот же, что у Whisper Yandex) ----------
  let chimeUrl = "";
  function chimeWav() {
    const SR = 44100, DUR = 0.45, n = Math.floor(SR * DUR), TAU = Math.PI * 2;
    const buf = new DataView(new ArrayBuffer(44 + n * 2));
    const str = (o, s) => { for (let i = 0; i < s.length; i++) buf.setUint8(o + i, s.charCodeAt(i)); };
    str(0, "RIFF"); buf.setUint32(4, 36 + n * 2, true); str(8, "WAVEfmt ");
    buf.setUint32(16, 16, true); buf.setUint16(20, 1, true); buf.setUint16(22, 1, true);
    buf.setUint32(24, SR, true); buf.setUint32(28, SR * 2, true); buf.setUint16(32, 2, true); buf.setUint16(34, 16, true);
    str(36, "data"); buf.setUint32(40, n * 2, true);
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const tone = (f, start, amp) => {
        const u = t - start;
        if (u < 0) return 0;
        return amp * Math.min(1, u / 0.006) * Math.exp(-u * 9) * (Math.sin(TAU * f * u) + 0.25 * Math.sin(TAU * 2 * f * u));
      };
      const fade = Math.min(1, Math.max(0, (DUR - t) / 0.03));
      const v = (tone(987.8, 0, 0.32) + tone(1318.5, 0.09, 0.3)) * fade * 0.6;
      buf.setInt16(44 + i * 2, Math.max(-1, Math.min(1, v)) * 32767, true);
    }
    return new Blob([buf.buffer], { type: "audio/wav" });
  }
  function beep() {
    try {
      if (!chimeUrl) chimeUrl = URL.createObjectURL(chimeWav());
      const a = new Audio(chimeUrl);
      a.volume = 0.8;
      a.play().catch(() => {});
    } catch {}
  }

  function accent() {
    const cs = getComputedStyle(document.documentElement);
    const hex = (v) => {
      if (/^#[0-9a-f]{3}([0-9a-f]{3}([0-9a-f]{2})?)?$/i.test(v)) return v;
      const m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(v);
      return m ? "#" + m.slice(1, 4).map((x) => Math.max(0, Math.min(255, Math.round(+x))).toString(16).padStart(2, "0")).join("") : null;
    };
    return { accent: hex(cs.getPropertyValue("--accent").trim()), accent2: hex(cs.getPropertyValue("--accent2").trim()) };
  }
  async function show(kind, title, body) {
    let t = String(title || "Whisper"), b = String(body || "");
    if (st.info === "short") b = L(SHORT[kind] || "");
    else if (st.info === "none") {
      t = "VK Music";
      b = L("Новое уведомление");
      kind = "test";
    }
    await invoke("notify_show", { note: { kind, title: t, body: b, corner: st.corner, ...accent() } });
    if (st.sound) beep();
  }
  // { kind, title, body } → true — показано
  window.sysNotify = async ({ kind, title, body } = {}) => {
    if (!st.on || !KINDS.includes(kind) || !st[kind]) return false;
    if (!st.always && !away()) return false;
    const t = `${title}\n${body}`;
    if (t === lastText && Date.now() - lastAt < 4000) return false;
    lastText = t;
    lastAt = Date.now();
    try {
      await show(kind, title, body);
      return true;
    } catch (e) {
      console.warn("Уведомление не показано:", e);
      return false;
    }
  };

  /** Карточка «Уведомления» во вкладке «Общее» (renderSettings в app.js). */
  window.renderNotifyCard = function renderNotifyCard() {
    const c = setCard("ui", L("Уведомления"), "ntcard");
    c.append(el("p", "status", WIN
      ? L("Приглашения в комнату и подмигивания «Слушать вместе» — карточкой в углу экрана, поверх всех окон.")
      : L("Приглашения в комнату и подмигивания «Слушать вместе» — уведомлением системы. Нажатие открывает VK Music.")));
    const sw = (key, text) => switchRow(text, st[key], (on) => {
      st[key] = on;
      save();
      renderSettings();
    });
    c.append(sw("on", L("Показывать уведомления")));
    if (!st.on) return;
    c.append(sw("invite", L("Приглашения в комнату")), sw("wink", L("Подмигивания в комнате")),
      sw("always", L("Даже когда окно открыто")), sw("sound", L("Звук уведомления")));
    const row = (title, ...kids) => {
      const r = el("div", "lyrow");
      r.append(el("span", "", title), ...kids);
      return r;
    };
    const seg = (list, cur, set) => {
      const s = el("div", "lyseg");
      for (const [id, label, hint] of list) {
        const b = el("button", "chip" + (cur === id ? " active" : ""), L(label));
        b.type = "button";
        if (hint) b.title = L(hint);
        b.addEventListener("click", () => {
          set(id);
          save();
          renderSettings();
        });
        s.append(b);
      }
      return s;
    };
    c.append(row(L("Текст"), seg(INFO, st.info, (v) => (st.info = v))));
    if (WIN) c.append(row(L("Где появляться"), seg(CORNERS, st.corner, (v) => (st.corner = v))));
    const test = el("button", "nav", L("Проверить"));
    test.type = "button";
    test.addEventListener("click", async () => {
      test.disabled = true;
      try {
        await show("test", "VK Music", L("Так выглядят уведомления: друг зовёт в комнату или подмигивает тебе"));
      } catch (e) {
        toast(L("Уведомление не показано: {0}", errMsg(e)), "error");
      }
      test.disabled = false;
    });
    const r = el("div", "row");
    r.append(test);
    c.append(r);
  };
})();
