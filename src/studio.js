// «Студия» (как у Whisper Yandex, просьба владельца 2026-11-17): своя версия трека библиотеки — обрезка.
// Трек — играющий («Обрезать играющий трек») или из меню «⋯» → «Обрезать в студии». Файл трека отдаёт Rust
// (studio.rs, studio_source) — по нему рисуется волна и слушается отрезок (Web Audio, отдельно от плеера).
// «Сохранить версию» — studio_save: ffmpeg без перекодирования, новый файл в папке музыки — обычный трек библиотеки.
// Страница — через modulePage (app.js), как «Друзья»; на телефоне студии нет (нет ffmpeg).
(() => {
  if (typeof MOBILE !== "undefined" && MOBILE) return;
  const { invoke } = window.__TAURI__.core;
  const $s = (id) => document.getElementById(id);
  const page = $s("studioPage");
  if (!page) return;
  const MIN_LEN = 1;
  const W = $s("stuWave"), CV = $s("stuCanvas"), SEL = $s("stuSel"), HEAD = $s("stuHead"), STATE = $s("stuState");
  const HANDLES = { a: W.querySelector('.stuh[data-h="a"]'), b: W.querySelector('.stuh[data-h="b"]') };

  // трек в редакторе: { t, buf (AudioBuffer), peaks, a, b } ; preview — { src, start, from, to }
  let cur = null, loadSeq = 0, ctx = null, preview = null, raf = 0;

  const fmt = (s) => {
    s = Math.max(0, s || 0);
    const m = Math.floor(s / 60), r = s - m * 60;
    return `${m}:${r.toFixed(1).padStart(4, "0")}`;
  };
  const parse = (v) => {
    const m = /^\s*(?:(\d{1,3}):)?(\d{1,4}(?:[.,]\d{1,3})?)\s*$/.exec(String(v));
    return m ? +(m[1] || 0) * 60 + parseFloat(m[2].replace(",", ".")) : NaN;
  };
  const lenText = (s) => (s < 60 ? L("{0} с", s.toFixed(1).replace(".", ",")) : fmt(s).replace(/\.\d$/, ""));
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
  }

  async function open(t) {
    if (!can(t)) return toast(L("В студии обрезаются только треки библиотеки"), "error");
    stop();
    const seq = ++loadSeq;
    cur = { t, buf: null, peaks: null, a: 0, b: 0 };
    $s("stuTitle").textContent = t.title;
    $s("stuArtist").textContent = t.artist;
    const img = $s("stuCover");
    img.hidden = !t.cover;
    if (t.cover) setCover(img, t.cover);
    $s("stuName").value = "";
    $s("stuName").placeholder = L("{0} (обрезка)", t.title);
    if (S.page !== "studio") pg.open();
    render();
    state(L("Загружаю трек…"));
    try {
      const bytes = await invoke("studio_source", { key: t.key });
      if (seq !== loadSeq) return;
      state(L("Рисую волну…"));
      ctx = ctx || new AudioContext();
      const buf = await ctx.decodeAudioData(bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer);
      if (seq !== loadSeq) return;
      Object.assign(cur, { buf, a: 0, b: buf.duration, peaks: peaksOf(buf, 1200) });
      state(null);
      sync();
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
    for (const id of ["stuPlay", "stuTail", "stuSave"]) $s(id).disabled = !!text;
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
        const tm = (i / bars) * d;
        g.fillStyle = tm >= cur.a && tm <= cur.b ? acc : muted;
        g.globalAlpha = tm >= cur.a && tm <= cur.b ? 0.95 : 0.45;
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
      const name = $s("stuName").value.trim();
      await invoke("studio_save", {
        key: cur.t.key,
        start_ms: Math.round(cur.a * 1000),
        end_ms: Math.round(cur.b * 1000),
        title: name || cur.t.title,
        artist: cur.t.artist,
      });
      toast(L("Версия сохранена в папку музыки — она уже в библиотеке"), "ok");
      await loadLibrary();
    } catch (e) {
      toast(errMsg(e), "error");
    }
    b.disabled = false;
  });

  // открыть из меню «⋯» (whisper-vk.js) — и обновить «Обрезать играющий трек», когда трек сменился
  window.Studio = { can, open };
  Whisper.on("track", () => { if (S.page === "studio") render(); });
})();
