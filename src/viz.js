"use strict";
// Визуализатор VK Микса (просьба владельца, 2026-11-15: «как в Whisper Yandex», на компьютере и на телефоне).
// Отрисовка — перенос из Whisper Yandex (main.js: vizFrame, drawBars, drawOrbit, drawBarsBottom, orbBeat): кольца
// вокруг кнопки микса («Круговой») или столбцы по частотам («Столбцами»), на весь экран — свои лёгкие варианты.
// Звук играет не WebView, поэтому WebAudio-анализатора нет: спектр присылает плеер событием player://event
// { type: "viz", b: [24 уровня полос, дБ] } — на компьютере ветка анализа mpv (viz.rs), на Android — ответвитель
// звука ExoPlayer (VkmPlugin.kt). Полосы — 40 Гц … 12 кГц по логарифму, их раскладываем по тем же частотам, что брал
// анализатор Whisper Yandex. На Windows данных нет — спокойная анимация без такта.
// Замеры идут, только пока визуализатор виден (player_viz { enabled, live }); при энергосбережении (power.js) его нет.
// Настройка — localStorage vkm_viz { on, style }, карточка «Визуализатор» во вкладке «Оформление».
(() => {
  const KEY = "vkm_viz";
  const STYLES = ["circle", "bars"];
  const NOTE = {
    circle: "Неровные круги вокруг кнопки: дышат с басом и вздрагивают на каждую долю.",
    bars: "Столбцы разной высоты по частотам: слева и справа от кнопки, бас ближе к центру.",
  };
  const cfg = { on: true, style: "circle" };
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    if (v && typeof v.on === "boolean") cfg.on = v.on;
    if (v && STYLES.includes(v.style)) cfg.style = v.style;
  } catch {}
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(cfg)); } catch {} };

  // ---------- данные от плеера ----------
  const BANDS = 24, LO = 40, HI = 12000, LR = Math.log(HI / LO) / (BANDS - 1);
  const lv = new Float32Array(BANDS);       // последний замер, 0…1
  let lastFrame = 0;
  // дБ → 0…1: шкала подобрана на глаз по трекам на телефоне (громкий трек — бас около −13 дБ, середина −30…−40,
  // верха −45…−80); верхам — небольшой подъём, как у Whisper Yandex («верха тише — немного подтягиваем»)
  const level = (db, i) => Math.max(0, Math.min(1, (db + 60 + (i / (BANDS - 1)) * 6) / 50));
  /** Уровень на частоте f (Гц): между соседними полосами — по логарифму частоты. */
  function at(f) {
    const p = Math.log(Math.max(f, LO) / LO) / LR;
    const i = Math.min(BANDS - 2, Math.floor(p)), u = Math.min(1, p - i);
    return lv[i] + (lv[i + 1] - lv[i]) * u;
  }
  listen("player://event", (ev) => {
    const e = ev.payload;
    if (!e || e.type !== "viz" || !Array.isArray(e.b) || e.b.length !== BANDS) return;
    for (let i = 0; i < BANDS; i++) lv[i] = level(Number(e.b[i]) || -100, i);
    lastFrame = performance.now();
  });
  const playing = () => !S.pl.paused && !S.pl.idle;
  const live = (now) => lastFrame > 0 && now - lastFrame < 400 && playing();

  // ---------- когда рисовать и когда просить замеры ----------
  const page = $("#wavePage");
  const allowed = () => cfg.on && !(window.Power && Power.active());
  const visible = () => allowed() && !document.hidden && (!page.hidden || fullOn);
  let sent = "";
  function syncPlayer() {
    const enabled = allowed(), want = enabled && visible();
    const k = enabled + "|" + want;
    if (k === sent) return;
    sent = k;
    invoke("player_viz", { enabled, live: want }).catch(() => { sent = ""; });
  }
  function apply() {
    const on = allowed();
    $("#viz").hidden = !on;
    $("#vizFsBtn").hidden = !on;
    if (!on && fullOn) setFull(false);
    $(".stage").classList.toggle("viz", on);
    if (visible()) start(); else stop();
    syncPlayer();
  }

  // ---------- отрисовка (Whisper Yandex) ----------
  const viz = {
    run: false, bass: 0, mid: 0, sm: new Float32Array(128),
    last: 0, kick: 0, eAvg: 0, ePrev: 0, lastBeat: 0,
    bars: new Float32Array(64), peaks: new Float32Array(64),
  };
  const vz = { cv: null, g: null, cvF: null, gF: null, key: "", br: 56, grad: null, glow: null, bgrad: null, floor: null,
    N: 120, cos: null, sin: null, uIdx: null, pts: null, A: "#a66bff", B: "#e060ff", colKey: "" };
  // частоты, которые брал анализатор Whisper Yandex (бин ~43 Гц: fftSize 1024 при 44,1 кГц)
  const BIN = 44100 / 1024;
  const SM_F = new Float32Array(128);
  for (let k = 0; k < 128; k++) SM_F[k] = BIN * (1 + Math.floor(Math.pow(k / 127, 1.7) * 150));

  function start() { if (!viz.run) { viz.run = true; viz.lastDraw = 0; requestAnimationFrame(frame); } }
  function stop() { viz.run = false; orbReset(); }

  function orbReset() {
    if (viz.orbKick !== undefined) { $(".stage").style.removeProperty("--kick"); viz.orbKick = undefined; }
    if (viz.orbLive) { $("#waveOrbits").classList.remove("beats"); viz.orbLive = false; }
  }
  function orbBeat(isLive) {
    const k = isLive ? Math.round(viz.kick * 40) / 40 : 0;
    if (k !== viz.orbKick) { viz.orbKick = k; vz.stage.style.setProperty("--kick", k); }
    if (isLive !== viz.orbLive) { viz.orbLive = isLive; vz.orb.classList.toggle("beats", isLive); }
    if (isLive && viz.beatNow) {
      viz.orbN = (viz.orbN + 1) % 2 || 0;
      const r = vz.orb.children[3 + viz.orbN];
      if (r && r.animate) {
        r.animate([
          { opacity: 0.95, scale: 1, borderWidth: "3px" },
          { opacity: 0, scale: 1.85 + viz.kick * 0.25, borderWidth: "0.5px" },
        ], { duration: 950, easing: "cubic-bezier(.1, .6, .3, 1)" });
      }
    }
  }
  function tables() {
    const N = vz.N;
    vz.cos = new Float32Array(N); vz.sin = new Float32Array(N); vz.uIdx = new Uint8Array(N); vz.pts = new Float32Array(N * 2);
    for (let j = 0; j < N; j++) {
      const th = (j / N) * Math.PI * 2;
      vz.cos[j] = Math.cos(th); vz.sin[j] = Math.sin(th);
      vz.uIdx[j] = Math.floor((Math.abs(th - Math.PI) / Math.PI) * 127);
    }
  }
  // --accent зарегистрирован как <color> — приходит как rgb(…)
  function hex(v, d) {
    v = (v || "").trim();
    if (/^#[0-9a-f]{6}$/i.test(v)) return v;
    const m = /^rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(v);
    return m ? "#" + [m[1], m[2], m[3]].map((x) => Math.min(255, +x).toString(16).padStart(2, "0")).join("") : d;
  }
  function rgba(h, a) {
    const n = parseInt(h.slice(1), 16);
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
  }
  function colors() {
    const cs = getComputedStyle(document.documentElement);
    vz.A = hex(cs.getPropertyValue("--accent"), "#a66bff");
    vz.B = hex(cs.getPropertyValue("--accent2"), "#e060ff");
  }

  function frame(ts) {
    if (!viz.run) return;
    if (document.hidden) { viz.run = false; return; }      // окно скрыто — цикл стоит (вернётся по visibilitychange)
    requestAnimationFrame(frame);
    const isLive = live(performance.now());
    // не чаще ~60 кадров/с, пока играет, и ~30 в простое
    if (ts - (viz.lastDraw || 0) < (isLive ? 13 : 30)) return;
    viz.lastDraw = ts;
    const full = fullOn;
    const cv = full ? (vz.cvF ||= $("#vizFullCv")) : (vz.cv ||= $("#viz")), w = cv.clientWidth, h = cv.clientHeight;
    if (!w || !h) return;
    const g = full ? (vz.gF ||= cv.getContext("2d")) : (vz.g ||= cv.getContext("2d"));
    const dpr = window.devicePixelRatio || 1;
    if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
      cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr);
      vz.br = ($("#waveBtn").offsetWidth || 112) / 2;
    }
    g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, w, h);
    // цвета темы (и фона по обложке) — раз в полсекунды, getComputedStyle на каждый кадр дорог
    if (!vz.colAt || ts - vz.colAt > 500) { vz.colAt = ts; colors(); }
    const A = vz.A, B = vz.B;
    const root = document.documentElement;
    const key = `${full}|${cfg.style}|${root.dataset.style}|${root.dataset.anim}|${w}x${h}|${A}|${B}`;
    const fresh = key !== vz.key;
    if (fresh) {
      vz.key = key; vz.br = ($("#waveBtn").offsetWidth || 112) / 2;
      vz.stage = $(".stage"); vz.orb = $("#waveOrbits");
      // отступ от кнопки: стиль или тема может задать его переменной --viz-gap на .stage (Неону нужно место под орбиты)
      vz.gapCss = parseFloat(getComputedStyle(vz.stage).getPropertyValue("--viz-gap"));
      vz.orbOn = !!vz.orb && root.dataset.anim === "on" && getComputedStyle(vz.orb).display !== "none";
      if (!vz.orbOn) orbReset();
    }

    let bass = 0, mid = 0;
    if (isLive) {
      for (let i = 0; i <= 5; i++) bass += lv[i];         // 40…135 Гц
      for (let i = 6; i <= 14; i++) mid += lv[i];         // 170 Гц … 1,7 кГц
      bass /= 6; mid /= 9;
    }
    const dt = Math.min(0.1, Math.max(0.001, (ts - (viz.last || ts)) / 1000)) || 0.016; viz.last = ts;
    const k60 = dt * 60;
    const ease = (cur, target, up, down) => cur + (target - cur) * (1 - Math.pow(1 - (target > cur ? up : down), k60));

    // такт: энергия самого низа (бочка, 40–110 Гц) против её среднего за последние ~полсекунды
    if (isLive) {
      const e = (lv[0] + lv[1] + lv[2] + lv[3] + lv[4]) / 5;
      viz.eAvg += (e - viz.eAvg) * (1 - Math.pow(0.95, k60));
      if (e > viz.eAvg * 1.18 + 0.03 && e - viz.ePrev > 0.015 && ts - viz.lastBeat > 200) { viz.kick = 1; viz.lastBeat = ts; viz.beatNow = true; }
      viz.ePrev = e;
    }
    viz.kick *= Math.exp(-dt * 6.5);

    viz.bass = ease(viz.bass, bass, 0.75, 0.16);
    viz.mid = ease(viz.mid, mid, 0.35, 0.15);
    for (let k = 0; k < 128; k++) viz.sm[k] = ease(viz.sm[k], isLive ? at(SM_F[k]) : 0, 0.6, 0.14);

    if (!full && vz.orbOn) orbBeat(isLive);
    const t = ts / 1000, cx = w / 2, cy = h / 2, R = Math.min(w, h) / 2, N = vz.N, RINGS = 5;
    if (full) {
      if (cfg.style === "bars") drawBarsBottom(g, w, h, t, isLive, ease, fresh, A, B); else drawOrbit(g, w, h, t, A, B);
      viz.beatNow = false; return;
    }
    viz.beatNow = false;
    const br = vz.br;
    if (cfg.style === "bars") { drawBars(g, w, h, t, br, isLive, ease, fresh, A, B); return; }
    if (!vz.cos) tables();
    const base0 = root.dataset.style;
    const gap = Number.isFinite(vz.gapCss) ? vz.gapCss : base0 === "steam" || base0 === "lolitta" ? 28 : 14;
    const inner = Math.min(br + gap, R * 0.5), outer = Math.max(inner + 20, R * 0.76);
    if (fresh) {
      vz.grad = g.createLinearGradient(0, 0, w, h);
      vz.grad.addColorStop(0, A); vz.grad.addColorStop(1, B);
      vz.glow = g.createRadialGradient(cx, cy, 0, cx, cy, R * 0.92);
      vz.glow.addColorStop(0, rgba(A, 1)); vz.glow.addColorStop(1, rgba(A, 0));
    }
    const grad = vz.grad;
    g.globalAlpha = 0.1 + viz.bass * 0.25 + viz.kick * 0.18;
    g.fillStyle = vz.glow; g.fillRect(0, 0, w, h);

    g.globalCompositeOperation = "lighter";
    const cs = vz.cos, sn = vz.sin, ui = vz.uIdx, pts = vz.pts, wob = 0.7 + viz.mid * 1.6;
    for (let i = 0; i < RINGS; i++) {
      const base = (inner + ((outer - inner) * i) / (RINGS - 1)) * (1 + viz.bass * (0.08 - i * 0.012) + viz.kick * (0.075 - i * 0.01));
      const room = Math.max(R - 16 - base, 4);
      const p1 = t * 0.5 + i * 1.3, p2 = -t * 0.7 + i * 2.1, p3 = t * 0.9 + i * 0.7, amp = R * 0.13 * (1 - i * 0.1), floorDev = -(base - br - 6);
      for (let j = 0; j < N; j++) {
        const th = (j / N) * Math.PI * 2;
        const s = viz.sm[(ui[j] + i * 9) % 128];
        const noise = 0.05 * Math.sin(2 * th + p1) + 0.04 * Math.sin(3 * th + p2) + 0.03 * Math.sin(5 * th + p3);
        let dev = base * noise * wob + s * amp;
        if (dev > 0) dev = room * Math.tanh(dev / room);
        else if (dev < floorDev) dev = floorDev;
        const r = base + dev;
        pts[j * 2] = cx + cs[j] * r; pts[j * 2 + 1] = cy + sn[j] * r;
      }
      const path = new Path2D();
      path.moveTo((pts[(N - 1) * 2] + pts[0]) / 2, (pts[(N - 1) * 2 + 1] + pts[1]) / 2);
      for (let j = 0; j < N; j++) {
        const a = j * 2, b = ((j + 1) % N) * 2;
        path.quadraticCurveTo(pts[a], pts[a + 1], (pts[a] + pts[b]) / 2, (pts[a + 1] + pts[b + 1]) / 2);
      }
      path.closePath();
      if (i === 0) { g.globalAlpha = 0.07; g.fillStyle = grad; g.fill(path); }
      g.strokeStyle = grad;
      g.globalAlpha = 0.05; g.lineWidth = 18; g.stroke(path);
      g.globalAlpha = 0.12; g.lineWidth = 7; g.stroke(path);
      g.globalAlpha = Math.min(1, 0.6 - i * 0.07 + viz.kick * 0.3); g.lineWidth = 1.6 + viz.kick * 0.8; g.stroke(path);
    }
    g.globalAlpha = 1; g.globalCompositeOperation = "source-over";
  }

  // уровень k-го столбца из n: частоты по логарифмической шкале (43 Гц … ~14,6 кГц, как BAR_BINS 340), бас — k = 0
  function barLevel(k, n, t, isLive) {
    if (!isLive) return 0.035 + 0.025 * Math.sin(t * 1.4 + k * 0.55);       // в тишине — лёгкая рябь
    const v = Math.max(at(BIN * Math.pow(340, k / n)), at(BIN * Math.pow(340, (k + 1) / n)));
    return Math.pow(v, 1.6) + viz.kick * 0.12 * (1 - k / n);
  }

  // весь экран, «Столбцами»: эквалайзер вдоль нижнего края, бас в центре, высокие к краям; нижняя треть экрана
  function drawBarsBottom(g, w, h, t, isLive, ease, fresh, A, B) {
    const half = w / 2 - 6, n = Math.max(16, Math.min(64, Math.floor(half / 15))), step = half / n, bw = Math.max(3, step * 0.58);
    const base = h, maxH = h * 0.32;
    if (fresh) {
      vz.floor = g.createLinearGradient(0, h, 0, h - h * 0.22);
      vz.floor.addColorStop(0, rgba(A, 1)); vz.floor.addColorStop(1, rgba(A, 0));
      vz.bgrad = g.createLinearGradient(0, base - maxH, 0, base);
      vz.bgrad.addColorStop(0, rgba(B, 0.35)); vz.bgrad.addColorStop(0.55, rgba(A, 0.8)); vz.bgrad.addColorStop(1, rgba(A, 0.95));
    }
    g.globalAlpha = 0.10 + viz.bass * 0.12 + viz.kick * 0.10;
    g.fillStyle = vz.floor; g.fillRect(0, h - h * 0.22, w, h * 0.22);
    g.globalCompositeOperation = "lighter";
    const glowP = new Path2D(), capP = new Path2D();
    g.fillStyle = vz.bgrad;
    for (let k = 0; k < n; k++) {
      viz.bars[k] = ease(viz.bars[k], barLevel(k, n, t, isLive), 0.65, 0.18);
      const hh = Math.max(3, Math.min(1, viz.bars[k]) * maxH);
      viz.peaks[k] = Math.max(hh, viz.peaks[k] - maxH * 0.5 / 60 * (isLive ? 1 : 3));
      const barP = new Path2D();
      for (let d = -1; d <= 1; d += 2) {
        const x = w / 2 + d * (k * step + step / 2) - bw / 2;
        glowP.roundRect(x - 3, base - hh - 3, bw + 6, hh + 3, [(bw + 6) / 2, (bw + 6) / 2, 0, 0]);
        barP.roundRect(x, base - hh, bw, hh, [bw / 2, bw / 2, 0, 0]);
        if (isLive) capP.rect(x, base - viz.peaks[k] - 7, bw, 2);
      }
      g.globalAlpha = 0.6 + Math.min(0.4, viz.bars[k] * 0.5); g.fill(barP);
    }
    g.globalAlpha = 0.1; g.fill(glowP);
    if (isLive) { g.globalAlpha = 0.75; g.fillStyle = B; g.fill(capP); }
    g.globalAlpha = 1; g.globalCompositeOperation = "source-over";
  }

  // весь экран, «Круговой» → «Орбита»: пустой центр, вокруг — тонкие лучи-частоты, на каждую долю — волна от кольца
  function drawOrbit(g, w, h, t, A, B) {
    const cx = w / 2, cy = h / 2, m = Math.min(w, h);
    const r0 = m * 0.15 * (1 + viz.bass * 0.05 + viz.kick * 0.06), L = m * 0.19;
    g.globalCompositeOperation = "lighter";
    viz.ripples ||= [];
    if (viz.beatNow) { viz.ripples.push(t); if (viz.ripples.length > 6) viz.ripples.shift(); }
    while (viz.ripples.length && t - viz.ripples[0] >= 1.8) viz.ripples.shift();
    g.strokeStyle = A;
    for (const b of viz.ripples) {
      const age = (t - b) / 1.8, r = r0 + age * m * 0.42;
      g.globalAlpha = Math.pow(1 - age, 2) * 0.32; g.lineWidth = 1.2 + (1 - age) * 1.5;
      g.beginPath(); g.arc(cx, cy, Math.min(r, m * 0.5 - 4), 0, Math.PI * 2); g.stroke();
    }
    const N = 160, rot = t * 0.035, path = new Path2D();
    for (let j = 0; j < N; j++) {
      const u = (j <= N / 2 ? j : N - j) / (N / 2);
      const s = viz.sm[Math.min(127, Math.floor(u * 110))];
      const len = 2 + Math.pow(s, 1.25) * L + viz.kick * 5;
      const th = rot + (j / N) * Math.PI * 2 - Math.PI / 2, c = Math.cos(th), sn = Math.sin(th);
      path.moveTo(cx + c * (r0 + 6), cy + sn * (r0 + 6));
      path.lineTo(cx + c * (r0 + 6 + len), cy + sn * (r0 + 6 + len));
    }
    const rg = g.createRadialGradient(cx, cy, r0, cx, cy, r0 + L * 0.9);
    rg.addColorStop(0, A); rg.addColorStop(1, B);
    g.strokeStyle = rg; g.lineCap = "round";
    g.globalAlpha = 0.14; g.lineWidth = 6; g.stroke(path);
    g.globalAlpha = 0.9; g.lineWidth = 2; g.stroke(path);
    g.strokeStyle = A;
    g.globalAlpha = 0.55 + viz.kick * 0.45; g.lineWidth = 1.5 + viz.kick * 2;
    g.beginPath(); g.arc(cx, cy, r0, 0, Math.PI * 2); g.stroke();
    g.globalAlpha = 0.18; g.lineWidth = 1;
    g.beginPath(); g.arc(cx, cy, r0 * 0.9, 0, Math.PI * 2); g.stroke();
    g.globalAlpha = 1; g.globalCompositeOperation = "source-over"; g.lineCap = "butt";
  }

  // столбцы: зеркально слева и справа от кнопки и вверх-вниз от средней линии; бас — у кнопки
  function drawBars(g, w, h, t, br, isLive, ease, fresh, A, B) {
    const cy = h / 2, gapC = br + (Number.isFinite(vz.gapCss) ? vz.gapCss : 14), side = w / 2 - gapC - 6;
    if (side < 30) return;
    const n = Math.max(8, Math.min(64, Math.floor(side / 10))), step = side / n, bw = Math.max(2, step * 0.6);
    const maxH = Math.max(10, Math.min(h / 2 - 10, side * 0.9));
    if (fresh) {
      vz.glow = g.createRadialGradient(w / 2, cy, 0, w / 2, cy, Math.min(w, h) * 0.46);
      vz.glow.addColorStop(0, rgba(A, 1)); vz.glow.addColorStop(1, rgba(A, 0));
      vz.bgrad = g.createLinearGradient(0, cy - maxH, 0, cy + maxH);
      vz.bgrad.addColorStop(0, B); vz.bgrad.addColorStop(0.5, A); vz.bgrad.addColorStop(1, B);
    }
    g.globalAlpha = 0.08 + viz.bass * 0.2 + viz.kick * 0.15;
    g.fillStyle = vz.glow; g.fillRect(0, 0, w, h);
    g.globalCompositeOperation = "lighter";
    const glowP = new Path2D(), capP = new Path2D();
    g.fillStyle = vz.bgrad;
    for (let k = 0; k < n; k++) {
      viz.bars[k] = ease(viz.bars[k], barLevel(k, n, t, isLive), 0.65, 0.18);
      const hh = Math.max(2, Math.min(1, viz.bars[k]) * maxH);
      viz.peaks[k] = Math.max(hh, viz.peaks[k] - maxH * 0.6 * (1 / 60) * (isLive ? 1 : 3));
      const barP = new Path2D(), p = Math.min(maxH, viz.peaks[k] + 4);
      for (let d = -1; d <= 1; d += 2) {
        const x = w / 2 + d * (gapC + k * step + step / 2) - bw / 2;
        glowP.roundRect(x - 2, cy - hh - 2, bw + 4, hh * 2 + 4, (bw + 4) / 2);
        barP.roundRect(x, cy - hh, bw, hh * 2, bw / 2);
        if (isLive) { capP.rect(x, cy - p - 1.5, bw, 1.5); capP.rect(x, cy + p, bw, 1.5); }
      }
      g.globalAlpha = 0.55 + Math.min(0.45, viz.bars[k] * 0.6); g.fill(barP);
    }
    g.globalAlpha = 0.12; g.fill(glowP);
    if (isLive) { g.globalAlpha = 0.7; g.fillStyle = B; g.fill(capP); }
    g.globalAlpha = 1; g.globalCompositeOperation = "source-over";
  }

  // ---------- на весь экран: панели прячутся, остаются фон и визуализатор ----------
  let fullOn = false, idleT = 0;
  const vf = $("#vizFull");
  function poke() {                                   // показать кнопку выхода и курсор, через пару секунд спрятать
    vf.classList.remove("idle");
    clearTimeout(idleT);
    idleT = setTimeout(() => vf.classList.add("idle"), 2200);
  }
  function setFull(on) {
    if (on === fullOn || (on && (!allowed() || page.hidden))) return;
    fullOn = on;
    document.body.classList.toggle("vizfull-on", on);
    vf.hidden = !on;
    viz.peaks.fill(0);
    if (on) {
      // кнопки «Пуск» в этом режиме нет — если ничего не играет, продолжаем трек или включаем VK Микс
      if (!playing()) {
        if (S.current) togglePlay(); else $("#waveBtn").click();
      }
      poke();
    } else clearTimeout(idleT);
    apply();
    if (MOBILE) return;                               // на телефоне приложение и так на весь экран
    if (HOST) {
      try {
        if (on && !document.fullscreenElement) document.documentElement.requestFullscreen().catch(() => {});
        else if (!on && document.fullscreenElement) document.exitFullscreen().catch(() => {});
      } catch {}
    } else invoke("win_set_fullscreen", { on }).catch(() => {});
  }
  $("#vizFsBtn").addEventListener("click", () => setFull(true));
  $("#vizFsExit").addEventListener("click", () => setFull(false));
  vf.addEventListener("mousemove", poke);
  vf.addEventListener("pointerdown", poke);
  vf.addEventListener("dblclick", () => setFull(false));
  // браузер (--host) вышел из полноэкранного сам (Esc браузера) — и мы выходим
  document.addEventListener("fullscreenchange", () => { if (!document.fullscreenElement && fullOn && HOST) setFull(false); });
  document.addEventListener("keydown", (e) => {
    if (e.target.closest && e.target.closest("input, textarea, [contenteditable]")) return;
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    if (fullOn) {
      if (e.key === "Escape" || e.code === "KeyF") { e.preventDefault(); e.stopPropagation(); setFull(false); }
      else if (e.key === " ") { e.preventDefault(); e.stopPropagation(); togglePlay(); poke(); }
    } else if (e.code === "KeyF" && !page.hidden && $("#full").hidden && !document.querySelector(".modal, .ccmodal")) setFull(true);
  }, true);

  // ---------- карточка «Визуализатор» (Оформление) ----------
  let card = null;
  function renderVizCard() {
    const old = card && card.isConnected ? card : null;
    const c = el("div", "card own vizcard");
    c.dataset.tab = "look";
    c.append(el("h3", null, L("Визуализатор")));
    c.append(switchRow(L("Аудиовизуализатор"), cfg.on, (on) => { cfg.on = on; save(); apply(); renderVizCard(); }));
    if (cfg.on) {
      const box = el("div", "vizstyles");
      const pick = el("div", "vizpick");
      for (const [v, label, icon] of [["circle", L("Круговой"), "vizcircle"], ["bars", L("Столбцами"), "vizbars"]]) {
        const b = el("button", "chip vizopt" + (cfg.style === v ? " active" : ""));
        b.type = "button";
        b.append(Icons.icon(icon), el("span", null, label));
        b.addEventListener("click", () => { cfg.style = v; save(); renderVizCard(); });
        pick.append(b);
      }
      box.append(pick, el("p", "status", L(NOTE[cfg.style])));
      if (window.Power && Power.active()) box.append(el("p", "status", L("Сейчас не рисуется: включено энергосбережение.")));
      c.append(box);
    }
    if (old) old.replaceWith(c);
    else $("#settingsPage").append(c);
    card = c;
  }
  window.renderVizCard = renderVizCard;
  // «Назад» Android (mobile.js) закрывает весь экран
  window.Viz = { full: () => fullOn, exitFull: () => setFull(false) };

  // ---------- что меняет видимость ----------
  new MutationObserver(apply).observe(page, { attributes: true, attributeFilter: ["hidden"] });
  document.addEventListener("visibilitychange", apply);
  window.addEventListener("power-change", () => {
    apply();
    if (card && card.isConnected) renderVizCard();
  });
  // плеер (mpv) мог запуститься позже: просим ещё раз, когда пришло первое состояние
  listen("player://event", (ev) => { if (ev.payload && ev.payload.type === "state" && !sent) syncPlayer(); });
  apply();
})();
