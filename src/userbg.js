// userbg.js — свой фон стиля (как у Whisper Yandex, просьба владельца 2026-11-17): картинка или гиф, видео (mp4 / webm,
// без звука), папка обоев Wallpaper Engine (обои типа «video»: project.json + файл), обои рабочего стола (Rust —
// desktop.rs) и подбор основного и второго цвета по фону (palette — та же формула, что у Whisper Yandex).
// Фон — у каждого стиля свой, в IndexedDB (файлы большие для localStorage): база «vkmusic», ключ «bg:<стиль>».
// Применяет его app.js (bgSource: свой фон главнее фона темы). Файл выбирает пользователь (<input type=file>) —
// страница видит только его; адреса — blob:, CSP их пускает (img-src и media-src).
(() => {
  const IMAGE_MAX = 50 * 1048576, VIDEO_MAX = 500 * 1048576;
  const urls = {}; // стиль → { url, kind: "image" | "video", poster }
  let note = "";

  const IDB = {
    db: null,
    open() {
      return (this.db ||= new Promise((res, rej) => {
        const r = indexedDB.open("vkmusic", 1);
        r.onupgradeneeded = () => r.result.createObjectStore("kv");
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
      }));
    },
    async tx(mode, fn) {
      const db = await this.open();
      return new Promise((res, rej) => {
        const t = db.transaction("kv", mode), rq = fn(t.objectStore("kv"));
        t.oncomplete = () => res(rq && rq.result);
        t.onerror = t.onabort = () => rej(t.error);
      });
    },
    keys: () => IDB.tx("readonly", (s) => s.getAllKeys()),
    get: (k) => IDB.tx("readonly", (s) => s.get(k)),
    set: (k, v) => IDB.tx("readwrite", (s) => s.put(v, k)),
    del: (k) => IDB.tx("readwrite", (s) => s.delete(k)),
  };

  function remember(st, blob) {
    if (urls[st]) URL.revokeObjectURL(urls[st].url);
    delete urls[st];
    if (blob) urls[st] = { url: URL.createObjectURL(blob), kind: /^video\//.test(blob.type) ? "video" : "image" };
  }
  // при запуске — все сохранённые фоны (boot ждёт ready, чтобы первая отрисовка была уже со своим фоном)
  const ready = (async () => {
    try {
      for (const k of await IDB.keys()) {
        if (typeof k !== "string" || !k.startsWith("bg:")) continue;
        const b = await IDB.get(k);
        if (b instanceof Blob) remember(k.slice(3), b);
      }
    } catch (e) {
      console.warn("Свой фон не прочитался:", e);
    }
  })();

  const style = () => document.documentElement.dataset.style || "";
  const setNote = (s) => {
    note = s || "";
    const p = document.getElementById("bgNote");
    if (p) p.textContent = note;
  };

  // ---------- проверка файла ----------
  const head = async (f, n = 16) => new Uint8Array(await f.slice(0, n).arrayBuffer());
  function sniffImage(b) {
    const at = (i, s) => [...s].every((c, k) => b[i + k] === c.charCodeAt(0));
    if (b[0] === 0xff && b[1] === 0xd8) return "image/jpeg";
    if (b[0] === 0x89 && at(1, "PNG")) return "image/png";
    if (at(0, "BM")) return "image/bmp";
    if (at(0, "GIF8")) return "image/gif";
    if (at(0, "RIFF") && at(8, "WEBP")) return "image/webp";
    if (at(4, "ftyp") && (at(8, "avif") || at(8, "avis"))) return "image/avif";
    return null;
  }
  function sniffVideo(b) {
    if (b.length > 12 && b[4] === 0x66 && b[5] === 0x74 && b[6] === 0x79 && b[7] === 0x70) {
      const brand = String.fromCharCode(b[8], b[9], b[10], b[11]);
      if (/^avi[fs]$/.test(brand) || /^(heic|heix|mif1|msf1)$/.test(brand)) return null;
      return "video/mp4";
    }
    if (b[0] === 0x1a && b[1] === 0x45 && b[2] === 0xdf && b[3] === 0xa3) return "video/webm";
    return null;
  }
  // первый кадр видео — проверить, что окно умеет его декодировать, и взять цвета
  function videoFrame(url, side = 160) {
    return new Promise((res, rej) => {
      const v = document.createElement("video");
      v.muted = true;
      v.preload = "auto";
      v.src = url;
      const done = (fn, x) => { clearTimeout(tm); v.removeAttribute("src"); v.load(); fn(x); };
      const tm = setTimeout(() => done(rej, new Error("timeout")), 15000);
      v.onerror = () => done(rej, new Error("decode"));
      v.onloadeddata = () => { v.currentTime = Math.min(1, (v.duration || 2) / 3); };
      v.onseeked = () => {
        const k = side / Math.max(v.videoWidth, v.videoHeight), c = document.createElement("canvas");
        c.width = Math.max(1, Math.round(v.videoWidth * k));
        c.height = Math.max(1, Math.round(v.videoHeight * k));
        c.getContext("2d").drawImage(v, 0, 0, c.width, c.height);
        done(res, c);
      };
    });
  }
  /** Файл → Blob фона с правильным типом (или ошибка с понятным текстом). */
  async function prepare(f) {
    const b = await head(f);
    const vt = sniffVideo(b), it = vt ? null : sniffImage(b);
    if (vt) {
      if (f.size > VIDEO_MAX) throw new Error(L("Видео больше {0} МБ — такое фоном не поставить", VIDEO_MAX / 1048576));
      const blob = new Blob([f], { type: vt });
      const url = URL.createObjectURL(blob);
      try {
        await videoFrame(url, 64);
      } catch {
        throw new Error(L("Видео «{0}» не воспроизводится: подойдёт MP4 (H.264) или WebM (VP9/AV1)", f.name));
      } finally {
        URL.revokeObjectURL(url);
      }
      return blob;
    }
    if (!it) throw new Error(L("«{0}» — не картинка и не видео", f.name));
    if (f.size > IMAGE_MAX) throw new Error(L("Картинка больше {0} МБ — такую фоном не поставить", IMAGE_MAX / 1048576));
    const blob = new Blob([f], { type: it });
    const img = new Image();
    img.src = URL.createObjectURL(blob);
    try {
      await img.decode();
    } catch {
      throw new Error(L("Картинка «{0}» не открылась", f.name));
    } finally {
      URL.revokeObjectURL(img.src);
    }
    return blob;
  }
  /** Из набора файлов (несколько или папка Wallpaper Engine) — файл фона: в папке обоев его называет project.json. */
  async function pickFrom(files) {
    files = [...files].filter(Boolean);
    const pj = files.find((f) => f.name.toLowerCase() === "project.json");
    if (pj) {
      let meta;
      try {
        meta = JSON.parse(await pj.text());
      } catch {
        throw new Error(L("project.json повреждён — не получилось его прочитать"));
      }
      const type = String((meta && meta.type) || "").toLowerCase();
      if (type && type !== "video") throw new Error(L("Это обои Wallpaper Engine типа «{0}» — их показывает только сама программа. Подойдут обои типа «video».", type));
      const want = String((meta && meta.file) || "").split(/[\\/]/).pop().toLowerCase();
      const hit = files.find((f) => f.name.toLowerCase() === want);
      if (hit) return hit;
    }
    const media = files.filter((f) => f !== pj && !/\.(json|url|txt|pdf|html?)$/i.test(f.name));
    return media.find((f) => /^video\//.test(f.type) || /\.(mp4|webm|m4v|mov|mkv)$/i.test(f.name))
      || media.find((f) => /^image\//.test(f.type)) || media[0] || null;
  }

  async function setFile(f, st = style()) {
    const blob = await prepare(f);
    await IDB.set("bg:" + st, blob);
    remember(st, blob);
    refresh();
    return blob;
  }
  async function clear(st = style()) {
    await IDB.del("bg:" + st).catch(() => {});
    remember(st, null);
    refresh();
  }
  function refresh() {
    if (typeof applyBg === "function") applyBg();
    if (S.page === "settings") renderSettings();
  }

  // ---------- палитра по фону (как у Whisper Yandex) ----------
  function rgb2hsl(r, g, b) {
    r /= 255; g /= 255; b /= 255;
    const mx = Math.max(r, g, b), mn = Math.min(r, g, b), l = (mx + mn) / 2;
    if (mx === mn) return [0, 0, l];
    const d = mx - mn, s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn);
    const h = mx === r ? (g - b) / d + (g < b ? 6 : 0) : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return [h * 60, s, l];
  }
  function hsl2hex(h, s, l) {
    s = Math.min(1, Math.max(0, s));
    l = Math.min(1, Math.max(0, l));
    const a = s * Math.min(l, 1 - l);
    const f = (n) => {
      const k = (n + h / 30) % 12;
      return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)))).toString(16).padStart(2, "0");
    };
    return "#" + f(0) + f(8) + f(4);
  }
  async function paletteFrom(u) {
    let img;
    if (u.kind === "video") img = await videoFrame(u.url, 160);
    else {
      img = new Image();
      img.src = u.url;
      await img.decode();
    }
    const N = 64, c = document.createElement("canvas");
    c.width = c.height = N;
    const g = c.getContext("2d", { willReadFrequently: true });
    g.drawImage(img, 0, 0, N, N);
    const d = g.getImageData(0, 0, N, N).data;
    const bins = Array.from({ length: 24 }, (_, i) => ({ h: i * 15 + 7.5, w: 0, s: 0 }));
    let sumS = 0, n = 0;
    for (let i = 0; i < d.length; i += 4) {
      if (d[i + 3] < 128) continue;
      const [h, s, l] = rgb2hsl(d[i], d[i + 1], d[i + 2]);
      sumS += s;
      n++;
      const w = s * (1 - Math.abs(2 * l - 1));
      const b = bins[Math.floor(h / 15) % 24];
      b.w += w;
      b.s += s * w;
    }
    if (!n || sumS / n < 0.1) return { accent: "#e8e8f4", accent2: "#9a9ab4", mono: true };
    const sorted = bins.filter((b) => b.w > 0).sort((a, b) => b.w - a.w);
    const first = sorted[0];
    const dist = (a, b) => { const x = Math.abs(a - b) % 360; return Math.min(x, 360 - x); };
    const second = sorted.find((b) => b !== first && dist(b.h, first.h) >= 45 && b.w >= first.w * 0.25);
    const sat = (b) => Math.min(0.95, Math.max(0.6, b.s / b.w));
    return {
      accent: hsl2hex(first.h, sat(first), 0.64),
      accent2: second ? hsl2hex(second.h, sat(second), 0.62) : hsl2hex((first.h + 45) % 360, 0.8, 0.66),
    };
  }
  /** Подобрать цвета по фону стиля: свой фон, иначе фон темы. */
  async function palette() {
    const st = style();
    const src = urls[st] || (typeof themeBg === "function" ? themeBg() : null);
    if (!src) return setNote(L("Сначала поставь фон: цвета подбираются по нему."));
    try {
      const p = await paletteFrom(src);
      window.Colors.setMain(st, p);
      applyStyleVars();
      setNote(p.mono ? L("Фон почти без цвета — палитра получилась сдержанной серебристой.") : L("Цвета подобраны по фону."));
      if (S.page === "settings") renderSettings();
    } catch {
      setNote(L("Не удалось разобрать картинку фона."));
    }
  }

  // ---------- выбор ----------
  function choose({ dir } = {}) {
    const inp = document.createElement("input");
    inp.type = "file";
    if (dir) inp.webkitdirectory = true;
    else {
      inp.multiple = true;
      inp.accept = "image/*,video/mp4,video/webm,.mp4,.webm,.m4v,.mov,.mkv,.json";
    }
    inp.addEventListener("change", async () => {
      const files = [...(inp.files || [])];
      if (!files.length) return;
      setNote(L("Ставлю фон…"));
      try {
        const f = await pickFrom(files);
        if (!f) throw new Error(L("В выбранном нет ни картинки, ни видео"));
        const blob = await setFile(f);
        setNote(/^video\//.test(blob.type) ? L("Видеообои применены. Звука у них нет; пока окно свёрнуто, видео не играет.") : L("Фон применён."));
      } catch (e) {
        setNote(e && e.message ? e.message : errMsg(e));
      }
    });
    inp.click();
  }
  async function desktop() {
    setNote(L("Беру обои с рабочего стола…"));
    try {
      const buf = await window.__TAURI__.core.invoke("desktop_wallpaper");
      const bytes = new Uint8Array(buf);
      const type = sniffImage(bytes);
      if (!type) throw new Error(L("Не удалось распознать формат обоев рабочего стола — выбери картинку вручную"));
      await setFile(new File([bytes], "desktop-wallpaper", { type }));
      await palette();
      setNote(L("Готово: фон с рабочего стола применён, цвета подобраны."));
    } catch (e) {
      setNote(e && e.message ? e.message : errMsg(e));
    }
  }

  /** Кнопки своего фона — в карточке «Фон и панели» (renderThemes в app.js). */
  function card(c) {
    const own = urls[style()];
    const row = el("div", "row ubgrow");
    const b = (text, fn, title) => {
      const x = el("button", "nav", text);
      x.type = "button";
      if (title) x.title = title;
      x.addEventListener("click", fn);
      row.append(x);
      return x;
    };
    b(L("Картинка или видео…"), () => choose(), L("Картинка, гиф или видео (mp4, webm) — фоном этого стиля"));
    b(L("Папка Wallpaper Engine…"), () => choose({ dir: true }), L("Видеообои Wallpaper Engine: папка с project.json"));
    if (!MOBILE) b(L("Обои рабочего стола"), desktop, L("Взять картинку с рабочего стола и подобрать цвета"));
    b(L("Цвета по фону"), palette, L("Основной и второй цвет — по картинке фона"));
    if (own) b(L("Убрать свой фон"), () => clear().then(() => setNote(L("Свой фон убран — снова фон темы."))));
    c.append(el("p", "status", own ? (own.kind === "video" ? L("У этого стиля свои видеообои.") : L("У этого стиля свой фон.")) : L("Фон — из темы. Свой фон запоминается для каждого стиля.")));
    c.append(row);
    const n = el("p", "status");
    n.id = "bgNote";
    n.textContent = note;
    c.append(n);
  }

  window.UserBg = { ready, of: (st) => urls[st] || null, card, choose: () => choose(), clear: () => clear(), has: () => !!urls[style()] };
})();
