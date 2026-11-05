"use strict";
// Режим --host: интерфейс в браузере (vkmusic --host, src-tauri/src/host.rs). Сервер вставляет этот файл первым
// скриптом index.html только в этом режиме. Он подменяет window.__TAURI__ — ровно то, чем пользуются app.js,
// whisper-vk.js и модули ядра: invoke — POST /invoke/<команда>, события — поток /events (SSE),
// convertFileSrc — /p/<схема>/… вместо vkm:// и wa://.
(() => {
  const handlers = new Map(); // имя события → Map(id → функция)
  let seq = 0;
  // Ошибки — русским текстом, как у команд Rust (app.js переводит их через errMsg)
  const OFFLINE = "Нет связи с VK Music: приложение закрыто?";

  async function invoke(cmd, args) {
    const raw = args instanceof Uint8Array || args instanceof ArrayBuffer;
    let r;
    try {
      r = await fetch("/invoke/" + encodeURIComponent(cmd), {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": raw ? "application/octet-stream" : "application/json" },
        body: raw ? args : JSON.stringify(args ?? {}),
      });
    } catch {
      throw OFFLINE;
    }
    const type = r.headers.get("Content-Type") || "";
    if (!r.ok) throw type.includes("json") ? await r.json() : await r.text();
    return type.includes("octet-stream") ? r.arrayBuffer() : r.json();
  }

  function listen(event, fn) {
    const id = ++seq;
    if (!handlers.has(event)) handlers.set(event, new Map());
    handlers.get(event).set(id, fn);
    return Promise.resolve(() => handlers.get(event)?.delete(id));
  }

  // Работает одна вкладка: открыли ещё одну — эта отдаёт ей плеер и закрывается заглушкой.
  function taken() {
    const t = (s) => (typeof L === "function" ? L(s) : s);
    const box = document.createElement("div");
    box.className = "host-taken";
    const msg = document.createElement("div");
    msg.textContent = t("VK Music открыт в другой вкладке");
    const btn = document.createElement("button");
    btn.className = "btn";
    btn.textContent = t("Открыть здесь");
    btn.addEventListener("click", () => location.reload());
    box.append(msg, btn);
    document.body.append(box);
  }

  const es = new EventSource("/events");
  es.onmessage = (m) => {
    let d;
    try {
      d = JSON.parse(m.data);
    } catch {
      return;
    }
    if (d.event === "host://taken") {
      es.close();
      taken();
      return;
    }
    const hs = handlers.get(d.event);
    if (!hs) return;
    for (const fn of [...hs.values()]) {
      try {
        fn({ event: d.event, id: 0, payload: d.payload });
      } catch (e) {
        console.error(e);
      }
    }
  };

  window.__TAURI__ = {
    __host: true,
    core: {
      invoke,
      convertFileSrc: (path, protocol = "asset") => "/p/" + protocol + "/" + encodeURIComponent(path),
    },
    event: { listen },
    window: { getCurrentWindow: () => ({ requestUserAttention: () => Promise.resolve() }) },
  };
  document.documentElement.classList.add("host-mode");
})();
