"use strict";
// «Живые» темы (как у Whisper Yandex — его themefx.js, просьба владельца 2026-11-17): то, чего на одном CSS не сделать,
// приложение даёт теме готовым — а тема только описывает, как это выглядит. Своего кода у темы по-прежнему нет.
// Разбор настроек — app.js → themeHooksOf (window.themeHook), вызов — app.js → applyHooks → window.themeFxApply().
// Выключатели эффектов — window.themeFx (vkm_fx). Описание для авторов тем — THEMES.md Whisper Yandex, «Живые темы».
//
//   1. События для CSS. Щелчок по кнопке, строке, вкладке — элемент получает класс .w-tap на ~0,65 с (и --tap-lx/--tap-ly —
//      место щелчка внутри него), на <html> — --tap-x/--tap-y. На <body> на короткое время — классы событий:
//      ev-tap ev-like ev-unlike ev-dislike ev-play ev-pause ev-track ev-page ev-start ev-beat.
//      Включается, только если CSS темы упоминает .ev-… / .w-tap / --tap-… (иначе ни одного лишнего пересчёта стилей).
//   2. Курсор: --mx/--my (−1…1 от центра окна) и --mpx/--mpy (px) на <html>, не чаще 30 раз в секунду.
//      Включается, только если CSS темы упоминает --mx/--my/--mpx/--mpy.
//   3. Слой украшений: settings "layer": { "count": 24, "place": "front" | "back" } — #fxLayer с N пустыми <i>
//      (у каждого --i — номер и --r1…--r3 — постоянные «случайные» 0…1, чтобы раскидать их одним правилом).
//   4. Частицы: settings "particles": { … } — рисует приложение на одном холсте поверх окна (не мешает щелчкам).
//      Каждая стайка — эффект темы: выключается в «Настроить тему…». Выключены анимации или система просит меньше
//      движения — частиц нет. Окно скрыто — холст стоит.
(() => {
  const de = document.documentElement;
  const W = window.Whisper;
  const reduce = matchMedia?.("(prefers-reduced-motion: reduce)");
  const animOk = () => de.dataset.anim !== "off" && !reduce?.matches;
  let st = { id: "", events: false, pointer: false, layer: null, particles: [] };
  let booted = false;

  // ---------- 1. события ----------
  const evT = {};
  // перезапуск класса без принудительной перекладки страницы: снять → через два кадра поставить снова
  function pulse(el, cls, ms, timers, key) {
    clearTimeout(timers[key]);
    if (el.classList.contains(cls)) {
      el.classList.remove(cls);
      requestAnimationFrame(() => requestAnimationFrame(() => el.classList.add(cls)));
    } else el.classList.add(cls);
    timers[key] = setTimeout(() => el.classList.remove(cls), ms);
  }
  function ev(name, ms = 700) { if (st.events) pulse(document.body, "ev-" + name, ms, evT, name); }

  const TAP = "button, a, .nav, .chip, .pill, .tab, .prail, .list li, .stylecard, .simcard, .switch, [role=button], .wavebtn";
  let lastTap = null;
  const tapT = new WeakMap();
  document.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const el = e.target instanceof Element ? e.target.closest(TAP) : null;
    lastTap = { x: e.clientX, y: e.clientY, t: performance.now(), el };
    if (!el) return;
    if (st.events) {
      de.style.setProperty("--tap-x", Math.round(e.clientX) + "px");
      de.style.setProperty("--tap-y", Math.round(e.clientY) + "px");
      const r = el.getBoundingClientRect();
      el.style.setProperty("--tap-lx", Math.round(e.clientX - r.left) + "px");
      el.style.setProperty("--tap-ly", Math.round(e.clientY - r.top) + "px");
      const tm = tapT.get(el) || {}; tapT.set(el, tm);
      pulse(el, "w-tap", 650, tm, "t");
      ev("tap", 600);
    }
    burst("tap");
  }, true);

  W?.on("like", (p) => { if (p.liked) { ev("like"); burst("like"); } else ev("unlike"); });
  W?.on("dislike", (p) => { if (p.disliked) { ev("dislike"); burst("dislike"); } });
  W?.on("play", () => { if (!booted) return; ev("play"); burst("play"); });
  W?.on("pause", () => { if (!booted) return; ev("pause"); burst("pause"); });
  // трек меняется и при запуске (восстановление последнего) — это ещё не событие для темы
  W?.on("track", () => { if (!booted) return; ev("track", 900); burst("track"); });
  window.themeFxBeat = () => { ev("beat", 180); burst("beat"); };
  // смена страницы — по data-page у .main (его ставит setActive)
  const main = document.querySelector(".main");
  if (main) new MutationObserver(() => { if (booted) { ev("page", 800); burst("page"); } }).observe(main, { attributes: true, attributeFilter: ["data-page"] });
  // запуск: шторка (html.booting) и интро темы (html.intro-on) ушли — один раз
  {
    const t0 = Date.now();
    const wait = () => {
      if ((de.classList.contains("booting") || de.classList.contains("intro-on")) && Date.now() - t0 < 15000) return setTimeout(wait, 120);
      booted = true;
      ev("start", 1600); burst("start");
    };
    setTimeout(wait, 300);
  }

  // ---------- 2. курсор ----------
  let mx = 0, my = 0, px = 0, py = 0, mRaf = 0, mLast = 0;
  // Каждая запись переменной на <html> — пересчёт стилей всего окна, поэтому: не чаще 30 раз в секунду, с шагом 0,01
  // (−1…1 — это 200 шагов, глазу хватает) и только если значение правда изменилось.
  const mSent = { mx: "", my: "", px: "", py: "" };
  const mSet = (k, v, name) => { if (mSent[k] !== v) { mSent[k] = v; de.style.setProperty(name, v); } };
  function mPush(ts) {
    mRaf = 0;
    if (ts - mLast < 33) { mRaf = requestAnimationFrame(mPush); return; }
    mLast = ts;
    mSet("mx", mx.toFixed(2), "--mx"); mSet("my", my.toFixed(2), "--my");
    mSet("px", Math.round(px / 2) * 2 + "px", "--mpx"); mSet("py", Math.round(py / 2) * 2 + "px", "--mpy");
  }
  const onMove = (e) => {
    px = e.clientX; py = e.clientY;
    mx = Math.max(-1, Math.min(1, (px / Math.max(1, innerWidth)) * 2 - 1));
    my = Math.max(-1, Math.min(1, (py / Math.max(1, innerHeight)) * 2 - 1));
    if (!mRaf) mRaf = requestAnimationFrame(mPush);
  };
  // курсор ушёл из окна — к центру (плавность возврата — transition в CSS темы)
  const onLeave = () => { mx = my = 0; px = innerWidth / 2; py = innerHeight / 2; if (!mRaf) mRaf = requestAnimationFrame(mPush); };
  function pointer(on) {
    document.removeEventListener("pointermove", onMove);
    document.documentElement.removeEventListener("mouseleave", onLeave);
    if (on) {
      document.addEventListener("pointermove", onMove, { passive: true });
      document.documentElement.addEventListener("mouseleave", onLeave);
      onLeave();
    } else { for (const v of ["--mx", "--my", "--mpx", "--mpy"]) de.style.removeProperty(v); for (const k in mSent) mSent[k] = ""; }
  }

  // ---------- 3. слой украшений ----------
  const rnd = (i, k) => { const x = Math.sin((i + 1) * 12.9898 + k * 78.233) * 43758.5453; return +(x - Math.floor(x)).toFixed(4); };
  let layerEl = null;
  function layer(spec) {
    layerEl?.remove(); layerEl = null;
    if (!spec) return;
    const d = document.createElement("div");
    d.id = "fxLayer"; d.className = "fxlayer"; d.dataset.place = spec.place; d.setAttribute("aria-hidden", "true");
    for (let i = 0; i < spec.count; i++) {
      const it = document.createElement("i");
      it.style.setProperty("--i", String(i));
      it.style.setProperty("--r1", String(rnd(i, 1))); it.style.setProperty("--r2", String(rnd(i, 2))); it.style.setProperty("--r3", String(rnd(i, 3)));
      d.append(it);
    }
    // сзади — над обоями и затемнением, под окном; спереди — над окном, под меню и окнами-диалогами
    const dim = document.getElementById("dim");
    if (spec.place === "back" && dim) dim.after(d);
    else document.body.append(d);
    layerEl = d;
  }

  // ---------- 4. частицы ----------
  // Как сделано быстро:
  //   * каждая частица — готовая картинка (спрайт): форма, цвет, размер и свечение рисуются один раз в маленький
  //     холст и дальше только копируются (drawImage). Свечение (shadowBlur) на каждую частицу в каждом кадре и
  //     растеризация SVG при каждой отрисовке съедали больше всего;
  //   * поворот, размер и «трепыхание» конфетти — одним setTransform, без save/restore;
  //   * холст стирается только там, где частицы были в прошлом кадре, а не целиком;
  //   * плотность пикселей — не больше 1,5 (частицам больше не нужно, а холст во всё окно 4K дорог);
  //   * кадры тяжелеют (меньше ~40 в секунду) — постоянные стайки сами становятся реже, вспышки — меньше.
  const MAX = 500;
  let cv = null, ctx = null, dpr = 1, raf = 0, lastTs = 0, box = null, slow = 0, quality = 1;
  const parts = [];
  const acc = new Map();         // "always": накопленная доля следующей частицы
  const imgs = new Map();        // имя файла → Image (когда загрузилась)
  const hook = (k) => (typeof window.themeHook === "function" ? window.themeHook(k) : null);
  const hookEffect = (k) => (hook("effects") || []).find((o) => o.k === k);
  const enabled = (p) => { const e = hookEffect(p.k); return typeof window.themeFx === "function" ? window.themeFx(p.k, e ? e.def : true) : true; };

  function canvas() {
    if (cv) return true;
    cv = document.createElement("canvas");
    cv.id = "fxCanvas"; cv.className = "fxcanvas"; cv.setAttribute("aria-hidden", "true");
    ctx = cv.getContext("2d");
    if (!ctx) { cv = null; return false; }
    document.body.append(cv);
    fit();
    return true;
  }
  function fit() {
    if (!cv) return;
    const d = Math.min(1.5, window.devicePixelRatio || 1);
    if (d !== dpr) sprites.clear();                       // спрайты нарисованы под старую плотность
    dpr = d;
    cv.width = Math.round(innerWidth * dpr); cv.height = Math.round(innerHeight * dpr);
    box = null;
  }
  addEventListener("resize", fit);

  // палитра стайки: цвета стиля читаются не чаще раза в секунду (getComputedStyle в каждом кадре — пересчёт стилей)
  const palCache = new Map();
  const css = (cs, v) => cs.getPropertyValue(v).trim();
  function palette(p) {
    const now = performance.now(), c = palCache.get(p);
    if (c && now - c.t < 1000) return c.cols;
    const cs = getComputedStyle(de);
    const map = { accent: css(cs, "--accent"), accent2: css(cs, "--accent2"), text: css(cs, "--text"), muted: css(cs, "--muted") };
    const cols = p.colors.map((x) => map[x] || x).filter(Boolean);
    palCache.set(p, { cols, t: now });
    return cols;
  }

  // ---------- спрайты ----------
  const sprites = new Map();
  function sprite(p, color, size, img) {
    const sz = Math.max(1, Math.round(size)), key = `${p.shape}|${p.img}|${color}|${sz}|${p.glow}`;
    let s = sprites.get(key);
    if (s) return s;
    if (sprites.size > 400) sprites.clear();
    const g = p.glow, isSpark = p.shape === "spark";
    const w = (isSpark ? sz * 2.6 : sz) + g * 2 + 2, h = (isSpark ? Math.max(2, sz / 2) : sz) + g * 2 + 2;
    const imgK = img && img.naturalWidth && img.naturalHeight ? img.naturalHeight / img.naturalWidth : 1;
    const H = p.shape === "image" ? sz * imgK + g * 2 + 2 : h;
    const c = document.createElement("canvas");
    c.width = Math.max(1, Math.ceil(w * dpr)); c.height = Math.max(1, Math.ceil(H * dpr));
    const x = c.getContext("2d");
    x.setTransform(dpr, 0, 0, dpr, (w * dpr) / 2, (H * dpr) / 2);
    if (g) { x.shadowBlur = g * dpr; x.shadowColor = color; }
    x.fillStyle = x.strokeStyle = color;
    const r = sz / 2;
    switch (p.shape) {
      case "square": x.fillRect(-r, -r, sz, sz); break;
      case "confetti": x.fillRect(-r, -sz * 0.22, sz, sz * 0.45); break;
      case "ring": x.lineWidth = Math.max(1, sz / 5); x.beginPath(); x.arc(0, 0, r - x.lineWidth / 2, 0, Math.PI * 2); x.stroke(); break;
      case "spark": x.lineWidth = Math.max(1, sz / 3); x.lineCap = "round"; x.beginPath(); x.moveTo(-sz * 1.3, 0); x.lineTo(sz * 1.3, 0); x.stroke(); break;
      case "star": {
        x.beginPath();
        for (let k = 0; k < 10; k++) { const rr = k % 2 ? sz * 0.22 : r, a = (k * Math.PI) / 5 - Math.PI / 2; x.lineTo(Math.cos(a) * rr, Math.sin(a) * rr); }
        x.closePath(); x.fill(); break;
      }
      case "heart": {
        x.beginPath(); x.moveTo(0, r * 0.9);
        x.bezierCurveTo(-r * 1.4, -r * 0.1, -r * 0.6, -r * 1.1, 0, -r * 0.4);
        x.bezierCurveTo(r * 0.6, -r * 1.1, r * 1.4, -r * 0.1, 0, r * 0.9);
        x.fill(); break;
      }
      case "snow": {
        x.lineWidth = Math.max(1, sz / 7); x.lineCap = "round"; x.beginPath();
        for (let k = 0; k < 3; k++) { const a = (k * Math.PI) / 3; x.moveTo(Math.cos(a) * r, Math.sin(a) * r); x.lineTo(-Math.cos(a) * r, -Math.sin(a) * r); }
        x.stroke(); break;
      }
      case "image": x.drawImage(img, -r, (-sz * imgK) / 2, sz, sz * imgK); break;   // SVG растеризуется здесь один раз
      default: x.beginPath(); x.arc(0, 0, r, 0, Math.PI * 2); x.fill();
    }
    s = { c, w, h: H };
    sprites.set(key, s);
    return s;
  }

  const vis = (el) => { if (!el || !el.offsetParent) return null; const r = el.getBoundingClientRect(); return r.width > 2 && r.height > 2 ? r : null; };
  const mid = (r) => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  const center = () => ({ x: innerWidth / 2, y: innerHeight / 2 });
  // откуда летят частицы: "from" стайки или место по событию
  function origin(p, from) {
    const recent = lastTap && performance.now() - lastTap.t < 900;
    switch (from) {
      case "tap": return recent ? { x: lastTap.x, y: lastTap.y } : center();
      case "cover": { const r = vis(document.querySelector(".player .cover")); return r ? mid(r) : center(); }
      case "play": { const r = vis(document.getElementById("play")); return r ? mid(r) : center(); }
      case "wave": { const r = vis(document.querySelector(".wavebtn")); return r ? mid(r) : center(); }
      case "like": {
        if (recent && lastTap.el?.closest?.(".likebtn, #likeBtn, #disBtn, .thbtn")) return { x: lastTap.x, y: lastTap.y };
        const r = vis(document.getElementById(p.on === "dislike" ? "disBtn" : "likeBtn"));
        return r ? mid(r) : center();
      }
      case "top": return { x: Math.random() * innerWidth, y: -12 };
      case "bottom": return { x: Math.random() * innerWidth, y: innerHeight + 12 };
      case "screen": return { x: Math.random() * innerWidth, y: Math.random() * innerHeight };
      default: { const r = vis(document.querySelector(".main")); return r ? { x: r.left + r.width / 2, y: r.top + 60 } : center(); }
    }
  }
  const DEF_FROM = { tap: "tap", like: "like", dislike: "like", play: "play", pause: "play", track: "cover", start: "center", page: "page", beat: "wave", always: "top" };
  const between = ([a, b]) => a + Math.random() * (b - a);

  function spawn(p, n, cols) {
    const from = p.from || DEF_FROM[p.on];
    const each = from === "top" || from === "bottom" || from === "screen";     // у каждой частицы своё место
    const o = each ? null : origin(p, from);
    const img = p.shape === "image" ? imgs.get(p.img) : null;
    if (p.shape === "image" && !img) return;                                   // картинка ещё грузится
    for (let i = 0; i < n && parts.length < MAX; i++) {
      const at = each ? origin(p, from) : o;
      const a = ((p.dir + (Math.random() - 0.5) * p.spread) * Math.PI) / 180, v = between(p.speed);
      const color = cols[(Math.random() * cols.length) | 0] || "#fff";
      parts.push({
        p, x: at.x, y: at.y, vx: Math.cos(a) * v, vy: Math.sin(a) * v,
        age: 0, life: p.life * (0.75 + Math.random() * 0.5),
        sp: sprite(p, color, between(p.size), img),
        rot: Math.random() * Math.PI * 2, vr: ((Math.random() - 0.5) * 2 * p.spin * Math.PI) / 180, ph: Math.random() * Math.PI * 2,
      });
    }
  }
  function burst(on) {
    if (!st.particles.length || !animOk() || document.hidden) return;
    const list = st.particles.filter((p) => p.on === on && enabled(p));
    if (!list.length || !canvas()) return;
    for (const p of list) spawn(p, Math.max(1, Math.round(p.count * quality)), palette(p));
    run();
  }
  const alwaysList = () => (animOk() && !document.hidden ? st.particles.filter((p) => p.on === "always" && enabled(p)) : []);
  function run() { if (!raf && cv) { lastTs = 0; raf = requestAnimationFrame(frame); } }

  function clearBox() {
    if (!box) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    const x = Math.max(0, Math.floor(box[0] * dpr) - 2), y = Math.max(0, Math.floor(box[1] * dpr) - 2);
    ctx.clearRect(x, y, Math.ceil(box[2] * dpr) + 4 - x, Math.ceil(box[3] * dpr) + 4 - y);
    box = null;
  }
  function frame(ts) {
    raf = 0;
    if (!animOk() || document.hidden) { parts.length = 0; clearBox(); return; }
    const dt = lastTs ? Math.min(0.05, (ts - lastTs) / 1000) : 0.016;
    lastTs = ts;
    // кадры тяжелеют — частиц становится меньше; полегчало — постепенно возвращаем
    if (dt > 0.025) { if (++slow > 20) { quality = Math.max(0.35, quality * 0.8); slow = 0; } }
    else { slow = Math.max(0, slow - 1); if (quality < 1) quality = Math.min(1, quality + dt * 0.1); }
    // постоянные стайки (снег, звёзды): N в секунду
    const al = alwaysList();
    for (const p of al) {
      let a = (acc.get(p) || 0) + p.rate * quality * dt;
      if (a >= 1) { const n = Math.floor(a); a -= n; spawn(p, n, palette(p)); }
      acc.set(p, a);
    }
    clearBox();
    const H = innerHeight, Wd = innerWidth;
    let x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9, blend = "";
    for (let i = parts.length - 1; i >= 0; i--) {
      const q = parts[i], p = q.p;
      q.age += dt;
      const k = Math.exp(-p.drag * dt);
      q.vx *= k; q.vy = q.vy * k + p.gravity * dt;
      q.x += q.vx * dt + (p.sway ? Math.cos(q.age * 1.6 + q.ph) * p.sway * dt : 0);
      q.y += q.vy * dt;
      q.rot += q.vr * dt;
      const out = q.y > H + 80 || q.y < -120 || q.x < -120 || q.x > Wd + 120;
      if (q.age >= q.life || (out && q.age > 0.3)) { parts[i] = parts[parts.length - 1]; parts.pop(); continue; }
      const t = q.age / q.life;
      let alpha = p.fade ? 1 - t * t : 1;
      if (p.on === "always") alpha *= Math.min(1, q.age / 0.4);               // постоянные — проявляются плавно
      if (alpha <= 0.01) continue;
      if (p.blend !== blend) { blend = p.blend; ctx.globalCompositeOperation = blend; }
      ctx.globalAlpha = alpha > 1 ? 1 : alpha;
      // поворот: у искры — по направлению полёта, у остальных — свой; у конфетти ещё и «переворот» по высоте
      const ang = p.shape === "spark" ? Math.atan2(q.vy, q.vx) : q.rot;
      const cs = Math.cos(ang) * dpr, sn = Math.sin(ang) * dpr, fy = p.shape === "confetti" ? Math.cos(q.age * 9 + q.ph) : 1;
      ctx.setTransform(cs, sn, -sn * fy, cs * fy, q.x * dpr, q.y * dpr);
      const s = q.sp;
      ctx.drawImage(s.c, -s.w / 2, -s.h / 2, s.w, s.h);
      const ext = (s.w > s.h ? s.w : s.h) * 0.75;                              // с запасом на поворот
      if (q.x - ext < x0) x0 = q.x - ext; if (q.y - ext < y0) y0 = q.y - ext;
      if (q.x + ext > x1) x1 = q.x + ext; if (q.y + ext > y1) y1 = q.y + ext;
    }
    ctx.globalAlpha = 1; ctx.globalCompositeOperation = "source-over";
    if (x1 > x0) box = [Math.max(0, x0), Math.max(0, y0), Math.min(Wd, x1), Math.min(H, y1)];
    if (parts.length || al.length) raf = requestAnimationFrame(frame);
  }
  document.addEventListener("visibilitychange", () => { if (!document.hidden && alwaysList().length && canvas()) run(); });
  reduce?.addEventListener?.("change", () => window.themeFxApply());

  function particles(list, id) {
    parts.length = 0; acc.clear(); palCache.clear(); sprites.clear();
    if (ctx) { ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.clearRect(0, 0, cv.width, cv.height); box = null; }
    for (const p of list) {
      if (p.shape !== "image" || imgs.has(p.img)) continue;
      imgs.set(p.img, null);
      window.wmtAssetUrl?.(id, p.img).then((u) => {
        if (!u) return;
        const im = new Image();
        im.onload = () => imgs.set(p.img, im);
        im.src = u;
      });
    }
    if (!list.length && cv) { cancelAnimationFrame(raf); raf = 0; cv.remove(); cv = null; ctx = null; }
  }

  // ---------- применить к текущему стилю ----------
  // applyCfg (а с ним и это) зовут часто — пересобираем, только когда что-то реально поменялось
  window.themeFxApply = () => {
    const h = hook;
    const next = { id: de.dataset.style || "", events: !!h("events"), pointer: !!h("pointer"), layer: h("layer") || null, particles: h("particles") || [] };
    if (next.id !== st.id) imgs.clear();                 // картинки частиц — у каждой темы свои
    if (next.id !== st.id || next.particles !== st.particles) particles(next.particles, next.id);
    if (next.id !== st.id || JSON.stringify(next.layer) !== JSON.stringify(st.layer)) layer(next.layer);
    if (next.pointer !== st.pointer) pointer(next.pointer);
    if (!next.events && st.events) {
      for (const c of [...document.body.classList]) if (c.startsWith("ev-")) document.body.classList.remove(c);
      document.querySelectorAll(".w-tap").forEach((e) => e.classList.remove("w-tap"));
      de.style.removeProperty("--tap-x"); de.style.removeProperty("--tap-y");
    }
    st = next;
    if (alwaysList().length && canvas()) run();          // постоянные стайки: включили эффект или анимации
  };
  window.themeFxApply();
})();
