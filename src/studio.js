// «Студия» (как у Whisper Yandex, просьба владельца 2026-11-17): свои версии треков библиотеки — обрезки.
// Трек — играющий («Обрезать играющий трек») или из меню «⋯» → «Обрезать в студии». Файл трека отдаёт Rust
// (studio.rs, studio_source) — по нему рисуется волна и слушается отрезок (Web Audio, отдельно от плеера); сводка —
// темп, тональность, громкость (studioinfo.js) и файл (studio_info). «Сохранить версию» — studio_save: ffmpeg без
// перекодирования, файл в папке музыки — обычный трек библиотеки. «Мои версии» — studio_list: играть, открыть
// в редакторе, переименовать, «любимая», поделиться через Whisper Archive (приватный альбом + ссылка-приглашение,
// как у Whisper Yandex), удалить. Страница — через modulePage (app.js), как «Друзья»; на телефоне студии нет.
(() => {
  if (typeof MOBILE !== "undefined" && MOBILE) return;
  const { invoke } = window.__TAURI__.core;
  const $s = (id) => document.getElementById(id);
  const page = $s("studioPage");
  if (!page) return;
  const MIN_LEN = 1;
  const SHARED_KEY = "vkm_studio_shared"; // id версии → { album }: второй раз не загружаем (ссылку с ключом не храним)
  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
  const W = $s("stuWave"), CV = $s("stuCanvas"), SEL = $s("stuSel"), HEAD = $s("stuHead"), STATE = $s("stuState");
  const HANDLES = { a: W.querySelector('.stuh[data-h="a"]'), b: W.querySelector('.stuh[data-h="b"]') };
  const EN = () => document.documentElement.lang === "en";

  // трек в редакторе: { t, buf, peaks, a, b, info: { file, sound } } ; preview — { src, start, from, to }
  let cur = null, loadSeq = 0, ctx = null, preview = null, raf = 0;
  let versions = [], filter = "all", renaming = null;

  const fmt = (s, dec = true) => {
    s = Math.max(0, s || 0);
    const m = Math.floor(s / 60), r = s - m * 60;
    return `${m}:${(dec ? r.toFixed(1) : Math.floor(r).toString()).padStart(dec ? 4 : 2, "0")}`;
  };
  const parse = (v) => {
    const m = /^\s*(?:(\d{1,3}):)?(\d{1,4}(?:[.,]\d{1,3})?)\s*$/.exec(String(v));
    return m ? +(m[1] || 0) * 60 + parseFloat(m[2].replace(",", ".")) : NaN;
  };
  const lenText = (s) => (s < 60 ? L("{0} с", s.toFixed(1).replace(".", ",")) : fmt(s, false));
  /** Трек, который студия может открыть: из библиотеки (файл или сохранённый трек сервиса). */
  const can = (t) => !!t && !t.room && !t.archive && !t.unavailable && (!!t.path || S.saved.some((x) => x.key === t.key));

  // ---------- страница ----------
  const pg = modulePage("studio", { render });
  if (!pg) return;

  function render() {
    const t = S.current;
    $s("stuTake").hidden = !(can(t) && (!cur || cur.t.key !== t.key));
    $s("stuEmpty").hidden = !!cur;
    $s("stuEditor").hidden = !cur;
    if (cur) requestAnimationFrame(draw);
    loadVersions();
  }

  async function open(t, sel) {
    if (!can(t)) return toast(L("В студии обрезаются только треки библиотеки"), "error");
    stop();
    const seq = ++loadSeq;
    cur = { t, buf: null, peaks: null, a: 0, b: 0, info: { file: null, sound: null } };
    $s("stuTitle").textContent = t.title;
    $s("stuArtist").textContent = t.artist;
    const img = $s("stuCover");
    img.hidden = !t.cover;
    if (t.cover) setCover(img, t.cover);
    $s("stuName").value = "";
    $s("stuName").placeholder = L("{0} (обрезка)", t.title);
    if (S.page !== "studio") pg.open();
    render();
    paintInfo();
    state(L("Загружаю трек…"));
    invoke("studio_info", { key: t.key })
      .then((f) => { if (seq === loadSeq && cur) { cur.info.file = f; paintInfo(); } })
      .catch(() => {});
    try {
      const bytes = await invoke("studio_source", { key: t.key });
      if (seq !== loadSeq) return;
      state(L("Рисую волну…"));
      ctx = ctx || new AudioContext();
      const buf = await ctx.decodeAudioData(bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer);
      if (seq !== loadSeq) return;
      const a = sel ? Math.max(0, sel.a) : 0, b = sel ? Math.min(buf.duration, sel.b) : buf.duration;
      Object.assign(cur, { buf, a, b: b > a ? b : buf.duration, peaks: peaksOf(buf, 1200) });
      state(null);
      sync();
      paintInfo();
      // темп и тональность — после отрисовки волны (считается ~полсекунды)
      setTimeout(() => {
        if (seq !== loadSeq || !cur || !cur.buf || !window.studioAnalyze) return;
        cur.info.sound = window.studioAnalyze(cur.buf);
        paintInfo();
      }, 60);
    } catch (e) {
      if (seq === loadSeq) state(L("Трек не открылся: {0}", errMsg(e)));
    }
  }
  function close() {
    stop();
    loadSeq++;
    cur = null;
    render();
  }
  function state(text) {
    STATE.hidden = !text;
    STATE.textContent = text || "";
    W.classList.toggle("busy", !!text);
    for (const id of ["stuPlay", "stuTail", "stuSave", "stuFromNow", "stuToNow"]) $s(id).disabled = !!text;
  }

  // ---------- сводка о треке ----------
  const num1 = (v, d = 1) => (Math.round(v * 10 ** d) / 10 ** d).toString().replace(".", EN() ? "." : ",").replace("-", "−");
  const tempoWord = (b) => L(b < 66 ? "медленно — ларго, адажио" : b < 76 ? "неспешно — адажио" : b < 108 ? "умеренно — анданте, модерато"
    : b < 120 ? "оживлённо — аллегретто" : b < 156 ? "быстро — аллегро" : b < 176 ? "очень быстро — виваче" : "стремительно — престо");
  const sureWord = (s) => (s >= 0.6 ? "" : s >= 0.3 ? L("примерно") : L("оценка неуверенная"));
  const mb = (n) => (n >= 1048576 ? L("{0} МБ", num1(n / 1048576)) : L("{0} КБ", Math.round(n / 1024)));
  function paintInfo() {
    const box = $s("stuInfo");
    if (!cur) { box.hidden = true; return; }
    box.hidden = false;
    const f = cur.info.file, s = cur.info.sound, wait = L("считаю…");
    const rows = [];
    const add = (label, value, sub = "", title = "") => rows.push({ label, value, sub, title });
    if (!cur.buf) add(L("Темп"), wait);
    else if (!s) add(L("Темп"), wait);
    else if (s.bpm) add(L("Темп"), `${num1(s.bpm, s.bpm % 1 ? 1 : 0)} BPM`, [tempoWord(s.bpm), sureWord(s.bpmSure)].filter(Boolean).join(" · "),
      L("Оценка по ударам в середине трека. Бывает, что верный темп — вдвое меньше или больше"));
    else add(L("Темп"), "—", L("не удалось определить"));
    if (!s) add(L("Тональность"), wait);
    else if (s.key) add(L("Тональность"), EN() ? s.keyShort : s.key, [EN() ? "" : s.keyShort, sureWord(s.keySure)].filter(Boolean).join(" · "),
      L("Оценка по нотам за весь трек (профили Крумхансла). У модальной и атональной музыки может ошибаться"));
    else add(L("Тональность"), "—", L("не удалось определить"));
    if (s && s.rmsDb != null && isFinite(s.rmsDb)) add(L("Громкость"), L("{0} дБ RMS", num1(s.rmsDb)), L("пик {0} дБFS", num1(s.peakDb)),
      L("Средняя громкость (RMS) и самый громкий момент относительно максимума"));
    if (cur.buf) add(L("Длительность"), fmt(cur.buf.duration, false));
    if (f) {
      add(L("Формат"), f.format || "—", f.codec || "");
      if (f.bitrate) add(L("Битрейт"), L("{0} кбит/с", f.bitrate));
      if (f.sample_rate) add(L("Частота"), L("{0} кГц", num1(f.sample_rate / 1000)), f.channels === 1 ? L("моно") : f.channels === 2 ? L("стерео") : f.channels ? L("{0} каналов", f.channels) : "");
      add(L("Размер"), mb(f.size), f.local ? L("файл из папки музыки") : L("из кэша трека сервиса"));
    } else add(L("Формат"), wait);
    box.textContent = "";
    for (const r of rows) {
      const cell = el("div", "stuinfo-i");
      if (r.title) cell.title = r.title;
      const dd = el("dd", r.value === wait ? "wait" : null, r.value);
      cell.append(el("dt", null, r.label), dd);
      if (r.sub) cell.append(el("small", null, r.sub));
      box.append(cell);
    }
  }

  // ---------- волна ----------
  function peaksOf(buf, n) {
    const ch = [...Array(buf.numberOfChannels).keys()].map((i) => buf.getChannelData(i));
    const len = ch[0].length, step = Math.max(1, Math.floor(len / n)), out = new Float32Array(n);
    let max = 0;
    for (let i = 0; i < n; i++) {
      let m = 0;
      const from = i * step, to = Math.min(len, from + step);
      // каждый 4-й отсчёт — волне хватает, а длинный трек считается быстро
      for (const c of ch) for (let j = from; j < to; j += 4) { const v = Math.abs(c[j]); if (v > m) m = v; }
      out[i] = m;
      if (m > max) max = m;
    }
    if (max > 0) for (let i = 0; i < n; i++) out[i] /= max;
    return out;
  }
  function draw() {
    if (!cur) return;
    const dpr = window.devicePixelRatio || 1, w = W.clientWidth, h = W.clientHeight;
    if (!w || !h) return;
    CV.width = Math.round(w * dpr);
    CV.height = Math.round(h * dpr);
    const g = CV.getContext("2d");
    g.clearRect(0, 0, CV.width, CV.height);
    if (cur.peaks && cur.buf) {
      const cs = getComputedStyle(page);
      const muted = cs.getPropertyValue("--muted").trim() || "#888", acc = cs.getPropertyValue("--accent").trim() || "#4cc";
      const bars = Math.max(20, Math.floor(w / 3)), bw = (CV.width / bars) * 0.7;
      const d = cur.buf.duration;
      for (let i = 0; i < bars; i++) {
        const p = cur.peaks[Math.floor((i / bars) * cur.peaks.length)] || 0;
        const tm = (i / bars) * d, inSel = tm >= cur.a && tm <= cur.b;
        g.fillStyle = inSel ? acc : muted;
        g.globalAlpha = inSel ? 0.95 : 0.45;
        const bh = Math.max(2 * dpr, p * (CV.height - 8 * dpr));
        g.fillRect((i / bars) * CV.width, (CV.height - bh) / 2, bw, bh);
      }
    }
    place();
  }
  const pct = (s) => (cur && cur.buf ? (s / cur.buf.duration) * 100 : 0);
  function place() {
    if (!cur || !cur.buf) return;
    SEL.style.left = pct(cur.a) + "%";
    SEL.style.width = pct(cur.b - cur.a) + "%";
    HANDLES.a.style.left = pct(cur.a) + "%";
    HANDLES.b.style.left = pct(cur.b) + "%";
  }
  function sync() {
    if (!cur || !cur.buf) return;
    const d = cur.buf.duration;
    cur.a = Math.max(0, Math.min(cur.a, d - MIN_LEN));
    cur.b = Math.min(d, Math.max(cur.b, cur.a + MIN_LEN));
    $s("stuA").value = fmt(cur.a);
    $s("stuB").value = fmt(cur.b);
    $s("stuLen").textContent = lenText(cur.b - cur.a);
    draw();
  }
  const timeAt = (x) => {
    const r = W.getBoundingClientRect();
    return Math.max(0, Math.min(1, (x - r.left) / r.width)) * cur.buf.duration;
  };
  let drag = null;
  W.addEventListener("pointerdown", (e) => {
    if (!cur || !cur.buf || W.classList.contains("busy")) return;
    const s = timeAt(e.clientX);
    drag = e.target.dataset && e.target.dataset.h ? e.target.dataset.h : Math.abs(s - cur.a) <= Math.abs(s - cur.b) ? "a" : "b";
    cur[drag] = s;
    sync();
    W.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  W.addEventListener("pointermove", (e) => {
    if (!drag || !cur || !cur.buf) return;
    const s = timeAt(e.clientX);
    if (drag === "a") cur.a = Math.min(s, cur.b - MIN_LEN);
    else cur.b = Math.max(s, cur.a + MIN_LEN);
    sync();
  });
  const endDrag = () => { drag = null; };
  W.addEventListener("pointerup", endDrag);
  W.addEventListener("pointercancel", endDrag);
  W.addEventListener("wheel", (e) => {
    if (!cur || !cur.buf) return;
    e.preventDefault();
    const s = timeAt(e.clientX), h = Math.abs(s - cur.a) <= Math.abs(s - cur.b) ? "a" : "b";
    cur[h] += (e.deltaY > 0 ? 1 : -1) * (e.shiftKey ? 1 : 0.1);
    sync();
  }, { passive: false });
  for (const [id, k] of [["stuA", "a"], ["stuB", "b"]]) {
    $s(id).addEventListener("change", (e) => {
      const v = parse(e.target.value);
      if (cur && Number.isFinite(v)) cur[k] = v;
      sync();
    });
  }
  new ResizeObserver(() => cur && draw()).observe(W);

  // ---------- «отсюда» / «досюда»: где звучит сейчас — отрезок в студии или этот же трек в плеере ----------
  function nowPos() {
    if (preview && ctx) return preview.from + (ctx.currentTime - preview.start);
    if (cur && S.current && S.current.key === cur.t.key && !S.pl.idle) return S.pl.pos || 0;
    return null;
  }
  for (const [id, k] of [["stuFromNow", "a"], ["stuToNow", "b"]]) {
    $s(id).addEventListener("click", () => {
      if (!cur || !cur.buf) return;
      const p = nowPos();
      if (p == null) return toast(L("Сейчас этот трек не звучит: включи его в плеере или послушай отрезок"));
      cur[k] = p;
      sync();
    });
  }

  // ---------- прослушивание отрезка ----------
  function stop() {
    if (preview) {
      try { preview.src.stop(); } catch {}
      preview = null;
    }
    cancelAnimationFrame(raf);
    HEAD.hidden = true;
    $s("stuPlay").textContent = L("▶ Послушать отрезок");
  }
  function listen(from, to) {
    if (!cur || !cur.buf) return;
    stop();
    // плеер — на паузу: два звука сразу не нужны
    if (S.current && !S.pl.paused && !S.pl.idle) togglePlay();
    ctx.resume().catch(() => {});
    const src = ctx.createBufferSource();
    src.buffer = cur.buf;
    src.connect(ctx.destination);
    src.start(0, from, Math.max(0.05, to - from));
    preview = { src, start: ctx.currentTime, from, to };
    src.onended = () => { if (preview && preview.src === src) stop(); };
    $s("stuPlay").textContent = L("■ Остановить");
    HEAD.hidden = false;
    const tick = () => {
      if (!preview) return;
      HEAD.style.left = pct(preview.from + (ctx.currentTime - preview.start)) + "%";
      raf = requestAnimationFrame(tick);
    };
    tick();
  }
  $s("stuPlay").addEventListener("click", () => (preview ? stop() : cur && listen(cur.a, cur.b)));
  $s("stuTail").addEventListener("click", () => cur && listen(Math.max(cur.a, cur.b - 3), cur.b));
  $s("stuClose").addEventListener("click", close);
  $s("stuTake").addEventListener("click", () => open(S.current));

  // ---------- сохранение ----------
  $s("stuSave").addEventListener("click", async () => {
    if (!cur || !cur.buf) return;
    const b = $s("stuSave");
    b.disabled = true;
    try {
      await invoke("studio_save", {
        key: cur.t.key,
        start_ms: Math.round(cur.a * 1000),
        end_ms: Math.round(cur.b * 1000),
        title: $s("stuName").value.trim(),
        track_title: cur.t.title,
        artist: cur.t.artist,
        cover: /^https:\/\//.test(cur.t.cover || "") ? cur.t.cover : null,
      });
      toast(L("Версия сохранена в папку музыки — она уже в библиотеке"), "ok");
      await loadLibrary();
      await loadVersions(true);
    } catch (e) {
      toast(errMsg(e), "error");
    }
    b.disabled = false;
  });

  // ---------- «Мои версии» ----------
  async function loadVersions(force) {
    if (S.page !== "studio" && !force) return;
    try {
      versions = await invoke("studio_list");
    } catch {
      versions = [];
    }
    paintVersions();
  }
  for (const c of $s("stuFilter").querySelectorAll(".chip")) {
    c.addEventListener("click", () => {
      filter = c.dataset.f;
      $s("stuFilter").querySelectorAll(".chip").forEach((x) => x.classList.toggle("active", x === c));
      paintVersions();
    });
  }
  const sourceOf = (v) => (v.src ? libraryByKey(v.src) : null);
  const libOf = (v) => libraryByKey(v.path);
  function paintVersions() {
    const ul = $s("stuList");
    ul.replaceChildren();
    const list = versions.filter((v) => filter !== "liked" || v.liked);
    $s("stuNone").hidden = list.length > 0;
    if (!list.length && versions.length) $s("stuNone").textContent = L("Любимых версий пока нет — отметь версию сердцем.");
    else $s("stuNone").textContent = L("Пока ни одной версии. Обрежь трек выше — версия появится здесь и в библиотеке.");
    for (const v of list) {
      const li = el("li", "track");
      const src = sourceOf(v);
      li.append(trackImg(v.cover || (src && src.cover) || null));
      const txt = el("div", "txt");
      const sub = [v.artist, v.end_ms > v.start_ms ? `${fmt(v.start_ms / 1000, false)}–${fmt(v.end_ms / 1000, false)}` : ""].filter(Boolean).join(" · ");
      if (renaming === v.id) {
        const inp = el("input", "field stuname");
        inp.value = v.title;
        inp.maxLength = 200;
        const done = async (ok) => {
          renaming = null;
          if (ok && inp.value.trim() && inp.value.trim() !== v.title) {
            try {
              await invoke("studio_update", { id: v.id, title: inp.value.trim() });
              await loadLibrary();
            } catch (e) {
              toast(errMsg(e), "error");
            }
          }
          loadVersions(true);
        };
        inp.addEventListener("keydown", (e) => {
          if (e.key === "Enter") done(true);
          else if (e.key === "Escape") done(false);
          e.stopPropagation();
        });
        inp.addEventListener("blur", () => renaming === v.id && done(true));
        inp.addEventListener("click", (e) => e.stopPropagation());
        txt.append(inp);
        setTimeout(() => inp.focus(), 0);
      } else txt.append(el("span", "t", v.title), el("span", "a", sub));
      const dur = el("span", "dur", fmt(Math.max(0, v.end_ms - v.start_ms) / 1000, false));
      const like = el("button", "likebtn" + (v.liked ? " on" : ""));
      like.title = v.liked ? L("Убрать из любимых") : L("В любимые");
      like.append(Icons.icon(v.liked ? "heartfill" : "heart"));
      like.addEventListener("click", async (e) => {
        e.stopPropagation();
        try {
          await invoke("studio_update", { id: v.id, liked: !v.liked });
        } catch (er) {
          toast(errMsg(er), "error");
        }
        loadVersions(true);
      });
      const more = el("button", "morebtn");
      more.title = L("Ещё");
      more.append(Icons.icon("dots"));
      more.addEventListener("click", (e) => {
        e.stopPropagation();
        versionMenu(v, more);
      });
      li.append(txt, dur, like, more);
      li.addEventListener("click", () => playVersion(v));
      ul.append(li);
    }
  }
  function playVersion(v) {
    const t = libOf(v);
    if (!t) return toast(L("Версии нет в библиотеке — обнови библиотеку"), "error");
    playList([t], 0);
  }
  function versionMenu(v, btn) {
    const src = sourceOf(v);
    const items = [
      { label: L("Играть"), fn: () => playVersion(v) },
      ...(src && can(src) ? [{ label: L("Открыть в редакторе"), fn: () => open(src, { a: v.start_ms / 1000, b: v.end_ms / 1000 }) }] : []),
      { label: L("Переименовать"), fn: () => { renaming = v.id; paintVersions(); } },
      { label: L("Поделиться через архив"), fn: () => share(v) },
      { label: L("Удалить версию"), fn: () => confirmClick(btn, () => remove(v), L("Удалить версию и её файл из папки музыки?"), L("Удалить")) },
    ];
    window.openPickMenu(btn, items.map((x) => ({ ...x, on: false })));
  }
  async function remove(v) {
    try {
      await invoke("studio_delete", { id: v.id });
      const sh = shared();
      if (sh[v.id]) {
        delete sh[v.id];
        saveShared(sh);
      }
      await loadLibrary();
      loadVersions(true);
    } catch (e) {
      toast(errMsg(e), "error");
    }
  }

  // ---------- поделиться через Whisper Archive (как у Whisper Yandex) ----------
  // Версия → приватный альбом архива (wa_save_track: файл Rust берёт сам по ключу — путь версии), затем
  // wa_album_share — ссылка-приглашение. Альбом версии запоминается (vkm_studio_shared), ссылка — нет: в ней ключ.
  const shared = () => {
    try {
      const v = JSON.parse(localStorage.getItem(SHARED_KEY) || "{}");
      return v && typeof v === "object" && !Array.isArray(v) ? v : {};
    } catch {
      return {};
    }
  };
  const saveShared = (v) => { try { localStorage.setItem(SHARED_KEY, JSON.stringify(v)); } catch {} };
  const bodyOf = (r) => { try { return r && r.body ? JSON.parse(r.body) : null; } catch { return null; } };
  const httpErr = (r, what) => {
    const b = bodyOf(r), m = b && (b.message || b.error || b.title);
    if (r.status === 401 || r.status === 403) return L("архив не принял вход — войди заново (Настройки → Интеграции)");
    return `${what}: HTTP ${r.status}${m ? " — " + String(m).slice(0, 160) : ""}`;
  };
  // обложка альбома, если у исходника её нет: название на фоне в цветах стиля
  function plainCover(title) {
    const c = document.createElement("canvas");
    c.width = c.height = 600;
    const g = c.getContext("2d"), cs = getComputedStyle(document.documentElement);
    const grad = g.createLinearGradient(0, 0, 600, 600);
    grad.addColorStop(0, cs.getPropertyValue("--accent").trim() || "#6b5bd2");
    grad.addColorStop(1, cs.getPropertyValue("--accent2").trim() || "#2a2340");
    g.fillStyle = grad;
    g.fillRect(0, 0, 600, 600);
    g.fillStyle = "rgba(0,0,0,.35)";
    g.fillRect(0, 380, 600, 220);
    g.fillStyle = "#fff";
    g.font = "bold 40px sans-serif";
    const words = String(title).split(/\s+/), lines = [];
    let line = "";
    for (const w of words) {
      const t = line ? line + " " + w : w;
      if (g.measureText(t).width > 540 && line) { lines.push(line); line = w; } else line = t;
    }
    if (line) lines.push(line);
    lines.slice(0, 3).forEach((l, i) => g.fillText(l, 30, 440 + i * 48));
    return new Promise((res) => c.toBlob((b) => b.arrayBuffer().then((a) => res([...new Uint8Array(a)])), "image/png"));
  }
  const sharing = new Set();
  async function share(v) {
    if (sharing.has(v.id)) return;
    if (!(await invoke("wa_token_exists").catch(() => false))) return toast(L("Чтобы делиться версиями, войди в Whisper Archive: Настройки → Интеграции"), "error");
    sharing.add(v.id);
    toast(L("Готовлю ссылку…"));
    try {
      const sh = shared();
      let album = UUID.test((sh[v.id] && sh[v.id].album) || "") ? sh[v.id].album : "";
      if (!album) {
        const src = sourceOf(v);
        const cover = v.cover || (src && /^https:\/\//.test(src.cover || "") ? src.cover : null);
        const r = await invoke("wa_save_track", {
          key: v.path,
          file_name: v.title,
          album_id: null,
          album_title: v.title.slice(0, 75),
          cover_url: cover,
          cover_bytes: cover ? null : await plainCover(v.title),
        });
        if (r.status < 200 || r.status >= 300) throw new Error(httpErr(r, L("Не удалось загрузить в архив")));
        const b = bodyOf(r);
        album = String((b && (b.id ?? b.albumId ?? (b.album && b.album.id))) || "");
        if (!UUID.test(album)) throw new Error(L("архив не вернул номер альбома"));
        sh[v.id] = { album };
        saveShared(sh);
      }
      const r = await invoke("wa_album_share", { id: album });
      if (r.status === 404) {
        const s2 = shared();
        delete s2[v.id];
        saveShared(s2);
        throw new Error(L("альбом этой версии в архиве удалён — нажми «Поделиться» ещё раз"));
      }
      if (r.status < 200 || r.status >= 300) throw new Error(httpErr(r, L("Не удалось получить ссылку")));
      let url = String((bodyOf(r) || {}).url || "");
      if (!/^https:\/\//.test(url)) throw new Error(L("архив не вернул ссылку"));
      // приглашение ведёт на выбор «слушать в приложении / на сайте» (как у Whisper Yandex)
      if (/\/invite\/[A-Za-z0-9_-]+$/.test(url)) url += "?to=version";
      await navigator.clipboard.writeText(url);
      toast(L("Ссылка на версию скопирована — отправь её другу"), "ok");
    } catch (e) {
      toast(L("Не получилось поделиться: {0}", e && e.message ? e.message : errMsg(e)), "error");
    }
    sharing.delete(v.id);
  }

  // открыть из меню «⋯» (whisper-vk.js) — и обновить «Обрезать играющий трек», когда трек сменился
  window.Studio = { can, open };
  Whisper.on("track", () => { if (S.page === "studio") render(); });
})();
