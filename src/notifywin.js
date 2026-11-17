// Окно уведомлений Windows (notify.rs; как у Whisper Yandex). Уведомления забираем у Rust (notify_take) при загрузке и по событию notify-poke,
// рисуем стопкой (не больше трёх), а высоту стопки сообщаем обратно (notify_layout) — Rust ставит окно в угол.
// Щелчок по карточке открывает главное окно, крестик — просто закрывает. Наведение мыши останавливает отсчёт.
(() => {
  const T = window.__TAURI__;
  const inv = (cmd, args) => T.core.invoke(cmd, args);
  const stack = document.getElementById("stack");
  const MAX = 3, LIFE = 6000;
  let taken = 0, emptyT = 0;
  const ICON = {
    invite: '<svg viewBox="0 0 24 24"><path d="M4 15v-3a8 8 0 0 1 16 0v3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><rect x="3" y="14" width="5" height="7" rx="2" fill="currentColor"/><rect x="16" y="14" width="5" height="7" rx="2" fill="currentColor"/></svg>',
    bell: '<svg viewBox="0 0 24 24"><path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20a2 2 0 0 0 4 0" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/></svg>',
  };
  const EMOJI = /^((?:\p{Extended_Pictographic}|\p{Regional_Indicator})(?:[️‍]|\p{Extended_Pictographic})*)\s*/u;
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

  function layout() {
    clearTimeout(emptyT);
    const live = [...stack.children].filter((c) => !c.classList.contains("out"));
    if (!stack.children.length) {
      // чуть подождать: вдруг следом уже идёт новое — тогда окно не пересоздаётся
      emptyT = setTimeout(() => { if (!stack.children.length) inv("notify_layout", { h: 0, taken }).catch(() => {}); }, 400);
      return;
    }
    if (!live.length) return; // последняя ещё уезжает — размер не трогаем
    const h = Math.ceil(stack.getBoundingClientRect().height) + 20; // + поля body
    inv("notify_layout", { h, taken }).catch(() => {});
  }

  function close(card) {
    if (card.classList.contains("out")) return;
    card.classList.add("out");
    let done = false;
    const rm = () => { if (done) return; done = true; card.remove(); layout(); };
    card.addEventListener("animationend", rm, { once: true });
    setTimeout(rm, 400); // анимации выключены или окно скрыто — всё равно убрать
  }

  function add(n) {
    if (!n || typeof n !== "object") return;
    document.documentElement.dataset.corner = /^(tl|tr|bl|br)$/.test(n.corner) ? n.corner : "br";
    let title = String(n.title || "Whisper"), icon = null;
    const m = n.kind === "wink" && title.match(EMOJI);
    if (m) { icon = m[1]; title = title.slice(m[0].length) || "Whisper"; }
    const card = el("div", "nt");
    card.setAttribute("role", "alert");
    if (n.accent) card.style.setProperty("--ac", n.accent);
    if (n.accent2) card.style.setProperty("--ac2", n.accent2);
    const ic = el("div", "ic");
    if (icon) ic.textContent = icon; else ic.innerHTML = n.kind === "invite" ? ICON.invite : ICON.bell;
    const tx = el("div", "tx");
    tx.append(el("b", "", title), el("span", "", String(n.body || "")));
    const x = el("button", "x", "✕"); x.type = "button"; x.title = "Закрыть";
    x.onclick = (e) => { e.stopPropagation(); close(card); };
    const life = el("i", "life");
    card.append(ic, tx, x, life);
    card.onclick = () => { inv("notify_ctl", { action: "open" }).catch(() => {}); close(card); };
    stack.append(card);
    // отсчёт — анимацией полоски: на наведении она на паузе, закончилась — карточка закрывается
    const a = life.animate([{ transform: "scaleX(1)" }, { transform: "scaleX(0)" }], { duration: LIFE, easing: "linear", fill: "forwards" });
    a.onfinish = () => close(card);
    card.onmouseenter = () => a.pause();
    card.onmouseleave = () => a.play();
    // больше трёх — самые старые уходят
    const live = [...stack.children].filter((c) => !c.classList.contains("out"));
    for (const old of live.slice(0, Math.max(0, live.length - MAX))) old.remove();
  }

  let busy = false, again = false;
  async function take() {
    if (busy) { again = true; return; }
    busy = true;
    try {
      do {
        again = false;
        let list = [];
        try { list = await inv("notify_take"); } catch {}
        for (const n of Array.isArray(list) ? list : []) { taken++; add(n); }
      } while (again);
    } finally { busy = false; }
    layout();
  }

  document.addEventListener("contextmenu", (e) => e.preventDefault());
  T.event.listen("notify-poke", take);
  take();
})();
