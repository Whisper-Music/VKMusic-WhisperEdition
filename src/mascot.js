// Маскот (как у Whisper Yandex, просьба владельца 2026-11-17): чёрный пиксельный кот Whisper гуляет по верхнему краю
// нижней панели. Сидит, моргает, кивает в такт, ходит к случайному месту, спит на паузе и в режиме сна, радуется
// «сердцу». Выключен по умолчанию: Настройки → Оформление → «Маскот» (localStorage vkm_mascot). Спрайты — те же, что у
// Whisper Yandex. Рисуется на одном <canvas> поверх окна (pointer-events: none), кадр — только когда он другой;
// выключен или окно скрыто — таймер стоит. Тема без маскота — settings whisper-music → features.mascot = false.
// На телефоне маскота нет (там нижняя панель — мини-плеер карточкой).
(() => {
  if (typeof MOBILE !== "undefined" && MOBILE) return;
  const KEY = "vkm_mascot";
  const COLS = 20, ROWS = 22, BODY_ROWS = 18;      // спрайт 20×18; во сне ещё 4 ряда — лапка свисает на плеер
  const PAL = { K: "#100e16", D: "#342e42", P: "#b078ff", p: "#7042d2", W: "#f6f0ff" };

  const SPR = {
    idle: ["...K........K.......", "..KDK......KDK......", "..KpKK....KKpK......", "..KppKKKKKKppK......", "..KKKKKKKKKKKK......", ".KKKKKKKKKKKKKK.....", ".KKPPKKKKKKPPKK.....", ".KKPWKKKKKKPWKK.....", "PPKKKKKppKKKKKPP....", ".PKKKKKKKKKKKKP.....", "...KKKKKKKKKK.......", "....KKKKKKKK........", "...DKKKKKKKKK....PP.", "..DKKKPPKKKKKK....p.", "..DKKPKKPPKKKK....K.", "..DKKKKKKKKKKKK..K..", "..DKKKKKKKKKKKKKK...", "..KDDKKKKKKDDKK....."],
    blink: ["...K........K.......", "..KDK......KDK......", "..KpKK....KKpK......", "..KppKKKKKKppK......", "..KKKKKKKKKKKK......", ".KKKKKKKKKKKKKK.....", ".KKKKKKKKKKKKKK.....", ".KKppKKKKKKppKK.....", "PPKKKKKppKKKKKPP....", ".PKKKKKKKKKKKKP.....", "...KKKKKKKKKK.......", "....KKKKKKKK........", "...DKKKKKKKKK....PP.", "..DKKKPPKKKKKK....p.", "..DKKPKKPPKKKK....K.", "..DKKKKKKKKKKKK..K..", "..DKKKKKKKKKKKKKK...", "..KDDKKKKKKDDKK....."],
    listen: [".................PP.", "...K........K....PW.", "..KDK......KDK...P..", "..KpKK....KKpK.PPP..", "..KppKKKKKKppK.PP...", "..KKKKKKKKKKKK......", ".KKKKKKKKKKKKKK.....", ".KKPPKKKKKKPPKK.....", ".KKPWKKKKKKPWKK.....", "PPKKKKKppKKKKKPP....", ".PKKKKKKKKKKKKP.....", "....KKKKKKKK........", "...DKKKKKKKKK....PP.", "..DKKKPPKKKKKK....p.", "..DKKPKKPPKKKK....K.", "..DKKKKKKKKKKKK..K..", "..DKKKKKKKKKKKKKK...", "..KDDKKKKKKDDKK....."],
    sleep: ["...........PPPP.....", "..............P.....", ".............P......", "......PPP...P.......", "........P..PPPP.....", ".......P............", "......PPP...........", "....................", "..K....K............", ".KpK..KpK...........", ".KKKKKKKKK..........", "KKKKKKKKKKKKKKKK....", "KKppKKppKKKKKKKKKK..", "PKKKKppKKKKKKKKKKKK.", "KKKKKKKKKKKKKKKKKKK.", "DKKKKKKKKKKKKKKKKKKD", ".DKKKKKKKKKKKKKKKpPK", "..KDDKKKKKKKKKKKKKK.", "...KK...............", "...KK...............", "...KK...............", "..KDDK.............."],
    like: ["...K........K..PP.PP", "..KDK......KDK.PWPPP", "..KpKK....KKpK.PPPPP", "..KppKKKKKKppK..PPP.", "..KKKKKKKKKKKK...P..", ".KKKKKKKKKKKKKK.....", ".KKPKKKKKKKKPKK.....", ".KPKPKKKKKKPKPK.....", "PPKKKKKppKKKKKPP....", ".PKKKKKKKKKKKKP.....", "...KKKKKKKKKK.......", "....KKKKKKKK........", "...DKKKKKKKKK....PP.", "..DKKKPPKKKKKK....p.", "..DKKPKKPPKKKK....K.", "..DKKKKKKKKKKKK..K..", "..DKKKKKKKKKKKKKK...", "..KDDKKKKKKDDKK....."],
    walk0: ["....................", ".PP.................", "Pp..................", "K.............K..K..", "K............KpKKpK.", ".K...........KKKKKK.", ".K...........KKKPWK.", "..KKKKKKKKKKKKKKKKpP", ".DKKKKKKKKKKKKKKKKKP", "..DKKKKKKKKKKKKKKK..", "..DKKKKKKKKKKKPKK...", "..DKKKKKKKKKKKKPK...", "...KKKKKKKKKKKPK....", "...KKKKKKKKKKKKK....", "....KKKKKKKKKKK.....", "....DKK.....DKK.....", "....KKD....DD.KK....", "...KK.DD..DD...KK..."],
    walk1: ["....................", "....................", ".PP.................", "Pp..................", "K.............K..K..", "K............KpKKpK.", ".K...........KKKKKK.", ".K...........KKKPWK.", "..KKKKKKKKKKKKKKKKpP", ".DKKKKKKKKKKKKKKKKKP", "..DKKKKKKKKKKKKKKK..", "..DKKKKKKKKKKKPKK...", "..DKKKKKKKKKKKKPK...", "...KKKKKKKKKKKPK....", "...KKKKKKKKKKKKK....", "....DKK.....DKK.....", "....DKK.....DKK.....", "....DKK.....DKK....."],
    walk2: ["....................", ".PP.................", "Pp..................", "K.............K..K..", "K............KpKKpK.", ".K...........KKKKKK.", ".K...........KKKPWK.", "..KKKKKKKKKKKKKKKKpP", ".DKKKKKKKKKKKKKKKKKP", "..DKKKKKKKKKKKKKKK..", "..DKKKKKKKKKKKPKK...", "..DKKKKKKKKKKKKPK...", "...KKKKKKKKKKKPK....", "...KKKKKKKKKKKKK....", "....KKKKKKKKKKK.....", "....DKK.....DKK.....", "...DD.KK....KKD.....", "..DD...KK..KK.DD...."],
  };
  SPR.lie = SPR.sleep.map((r, i) => (i < 8 ? ".".repeat(COLS) : i === 12 ? "KKPWKKPWKKKKKKKKKK.." : r));   // лежит, но не спит
  const WALK = ["walk0", "walk1", "walk2", "walk1"];

  // ---------- настройки ----------
  const st = (() => {
    let v = {};
    try {
      v = JSON.parse(localStorage.getItem(KEY) || "{}") || {};
    } catch {}
    return { on: v.on === true, size: v.size === 2 ? 2 : 3, react: v.react !== false };
  })();
  const save = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(st));
    } catch {}
  };

  // ---------- холст ----------
  const cv = document.createElement("canvas");
  cv.className = "mascot";
  cv.hidden = true;
  cv.setAttribute("aria-hidden", "true");
  document.body.append(cv);
  const g = cv.getContext("2d");
  let scale = 0, drawn = "", flipDrawn = false;
  function fit() {
    const k = st.size * Math.max(1, Math.round(window.devicePixelRatio || 1));
    if (k === scale) return;
    scale = k;
    cv.width = COLS * k;
    cv.height = ROWS * k;
    cv.style.width = COLS * st.size + "px";
    cv.style.height = ROWS * st.size + "px";
    drawn = "";
  }
  function draw(name, flip) {
    if (name === drawn && flip === flipDrawn) return;
    drawn = name;
    flipDrawn = flip;
    g.clearRect(0, 0, cv.width, cv.height);
    const rows = SPR[name];
    for (let y = 0; y < rows.length; y++)
      for (let x = 0; x < COLS; x++) {
        const c = PAL[rows[y][x]];
        if (!c) continue;
        g.fillStyle = c;
        g.fillRect((flip ? COLS - 1 - x : x) * scale, y * scale, scale, scale);
      }
  }

  // ---------- поведение ----------
  const now = () => performance.now();
  const rnd = (a, b) => a + Math.random() * (b - a);
  // mode: sit — сидит и слушает, lie — лежит на краю (лапка свисает), walk — идёт; until — когда решать, что дальше
  let mode = "sit", until = 0, lastT = 0;
  let x = -1, target = -1, pausedSince = 0, likeUntil = 0, blinkAt = now() + rnd(4000, 7000), blinkUntil = 0;
  let headDown = false, nextBob = 0, prevKey = null, prevLiked = false, timer = 0;

  const isPlaying = () => !!(S.current && !S.pl.paused && !S.pl.idle);
  const sleepMode = () => { const l = document.getElementById("sleepLeft"); return !!l && !l.hidden; };
  const reduced = matchMedia("(prefers-reduced-motion: reduce)");
  const calm = () => document.documentElement.dataset.anim === "off" || reduced.matches;
  const player = () => document.querySelector(".app .player");
  // стиль без маскота (settings whisper-music → features.mascot = false): выбор пользователя сохраняется
  const allowed = () => !(S.themeSettings && S.themeSettings.features && S.themeSettings.features.mascot === false);
  // кивок — два раза в секунду, пока играет
  const beat = (t) => (t > nextBob ? ((nextBob = t + 500), true) : false);

  function tick() {
    if (!st.on || document.hidden) return;
    const pl = player(), r = pl && pl.getBoundingClientRect();
    // плеера не видно (консольный стиль, тема без панели) или стиль без маскота — кота тоже
    if (!allowed() || !r || r.width < COLS * st.size * 3 || r.height < 8 || document.body.classList.contains("cli-on")) {
      cv.hidden = true;
      return;
    }
    cv.hidden = false;
    fit();
    const t = now(), w = COLS * st.size, minX = 12, maxX = Math.max(minX, r.width - w - 12);
    if (x < 0) { x = target = Math.round(maxX * 0.82); mode = "sit"; until = t + rnd(40000, 70000); }
    x = Math.min(Math.max(x, minX), maxX);
    target = Math.min(Math.max(target, minX), maxX);
    const dt = Math.min(200, t - (lastT || t));
    lastT = t;

    const playing = isPlaying();
    if (playing) pausedSince = 0;
    else pausedSince ||= t;
    const still = calm();
    const asleep = st.react && (sleepMode() || (pausedSince && t - pausedSince > 120000));

    // «сердце» играющему треку — порадоваться полторы секунды (переход «не в библиотеке → в библиотеке» у того же трека)
    const key = S.current ? S.current.key : "", liked = !!key && !!libraryMatch(S.current);
    if (liked && !prevLiked && key === prevKey) likeUntil = t + 1500;
    prevKey = key;
    prevLiked = liked;
    const liking = st.react && t < likeUntil;

    // характер — спокойный: подолгу сидит, часто ложится, ходит редко и недалеко
    if (asleep || still) { if (mode === "walk") { target = x; mode = "sit"; } until = Math.max(until, t + 30000); }
    else if (mode === "walk") {
      if (Math.abs(target - x) < 1) { mode = "sit"; until = t + rnd(45000, 90000); }
    } else if (!playing && mode === "sit" && pausedSince && t - pausedSince > 10000) { mode = "lie"; until = t + rnd(60000, 120000); }
    else if (t > until) {
      const q = Math.random();
      if (mode === "lie") { mode = "sit"; until = t + rnd(25000, 45000); }
      else if (q < 0.5) { mode = "lie"; until = t + rnd(60000, 150000); }
      else if (q < 0.8 || !playing) { until = t + rnd(30000, 60000); }
      else {
        const dir = x < minX + w * 2 ? 1 : x > maxX - w * 2 ? -1 : Math.random() < 0.5 ? -1 : 1;
        target = Math.round(Math.min(maxX, Math.max(minX, x + dir * rnd(w * 1.5, w * 5))));
        if (Math.abs(target - x) >= w) mode = "walk";
        else { target = x; until = t + rnd(30000, 60000); }
      }
    }
    if (mode !== "walk") target = x;

    let name, flip = false;
    const speed = 15 * st.size;
    if (mode === "walk" && Math.abs(target - x) >= 1) {
      const step = Math.min(Math.abs(target - x), (speed * dt) / 1000);
      flip = target < x;
      x += flip ? -step : step;
      name = WALK[Math.floor(t / 140) % WALK.length];
    } else if (liking) name = "like";
    else if (asleep) name = "sleep";
    else if (mode === "lie") name = "lie";
    else {
      if (st.react && playing && !still && beat(t)) headDown = !headDown;
      if (!playing || !st.react || still) headDown = false;
      if (!still && t > blinkAt) { blinkUntil = t + 120; blinkAt = t + rnd(4000, 7000); }
      name = t < blinkUntil ? "blink" : headDown ? "listen" : "idle";
    }
    draw(name, flip);
    cv.style.transform = `translate(${Math.round(r.left + x)}px, ${Math.round(r.top - BODY_ROWS * st.size)}px)`;
  }

  function run() {
    clearInterval(timer);
    timer = 0;
    prevKey = null;
    lastT = 0;
    if (st.on && !document.hidden) { timer = setInterval(tick, 50); tick(); }
    else cv.hidden = true;
  }
  document.addEventListener("visibilitychange", run);

  /** Карточка «Маскот» во вкладке «Оформление» (renderSettings в app.js). */
  window.renderMascotCard = function renderMascotCard() {
    const c = setCard("look", L("Маскот"));
    c.append(el("p", "status", L("Чёрный пиксельный кот Whisper гуляет по нижней панели, кивает в такт и засыпает на паузе.")));
    c.append(switchRow(L("Показывать маскота"), st.on, (on) => { st.on = on; save(); x = -1; run(); renderSettings(); }));
    if (!st.on) return;
    c.append(switchRow(L("Реагирует на музыку"), st.react, (on) => { st.react = on; save(); }));
    const row = el("div", "lyrow");
    const seg = el("div", "lyseg");
    for (const [v, label] of [[2, "Маленький"], [3, "Обычный"]]) {
      const b = el("button", "chip" + (st.size === v ? " active" : ""), L(label));
      b.type = "button";
      b.addEventListener("click", () => { st.size = v; save(); scale = 0; x = -1; run(); renderSettings(); });
      seg.append(b);
    }
    row.append(el("span", "", L("Размер")), seg);
    c.append(row);
  };
  run();
})();
