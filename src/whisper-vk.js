"use strict";
// VK Music в ядре Whisper (src/core, docs/CORE.md там): кто мы, какие у нас элементы играют какие роли в темах,
// провайдер, плеер и очередь для общих модулей, набор интерфейса (подсказки, подтверждения).
// Подключается после app.js и до движка тем и модулей ядра: им при запуске уже нужны роли и привязка.
// Всё, что здесь вызывается, — функции и состояние app.js (S, playAt, next…); при загрузке файла ничего не зовётся.
(() => {
  const W = window.Whisper;
  if (!W) return console.error("whisper-vk.js: нет ядра Whisper — подключите core/whisper-core.js раньше");

  // id — имя секции темы (@@ css whisper-vk); storage — приставка ключей localStorage модулей ядра (история очереди)
  W.setApp({ id: "whisper-vk", name: "VK Music", version: "0.1.0", storage: "vkm_" });

  // ---------- роли интерфейса: [data-w="…"] в темах → наши селекторы ----------
  // Классы и id ниже — публичный контракт с темами (docs/THEMES.md): меняются вместе с этим списком.
  W.ui.define({
    sidebar: ".side",
    nav: ".side .nav-item, .side-foot",
    "nav:active": ".side .nav-item.active, .side-foot.active",
    "search:global": "#searchBox",
    "search:settings": ".content input[type=\"search\"]",
    main: ".main",
    "page.title": "#pageTitle",
    button: ".btn",
    "button:active": ".btn.primary",
    "button:danger": ".btn.danger",
    tabs: ".tabs",
    tab: ".tab",
    "tab:active": ".tab.active",
    tracklist: ".tracks",
    track: ".track",
    "track:playing": ".track.playing",
    "track.cover": ".t-cover",
    "track.title": ".t-title",
    "track.artist": ".t-artist",
    card: ".svc-card, .theme-card, .stats-tile",
    tile: ".vk-pl, .vk-mix",
    player: ".player",
    "player.cover": ".np-cover",
    "player.title": "#npTitle",
    "player.artist": "#npArtist",
    "player.info": ".np-meta",
    "player.controls": ".controls",
    "player.play": "#playBtn",
    "player.prev": "#prevBtn",
    "player.next": "#nextBtn",
    "player.mode": "#shuffleBtn, #repeatBtn",
    "player.mode:on": "#shuffleBtn.on, #repeatBtn.on",
    "player.seek": "#bar",
    "player.time": ".progress .time",
    "player.actions": ".vol",
    "player.volume": ".vol-box",
    "player.volume.button": "#volBtn",
    "player.volume.slider": "#vol",
    radio: ".vk-mix",
    menu: ".popup",
    "menu.item": ".popup-item",
    toast: ".toast",
    queue: "#qPanel",
    lyrics: "#npLyrics",
  });

  // ---------- треки: наш объект трека (app.js: localTrack / svcTrack) ↔ сущность ядра ----------
  // id в ядре — до 100 символов; ключ библиотеки (путь файла, vk:…, sc:…) бывает длиннее — тогда хэш и хвост
  function trackId(key) {
    key = String(key || "");
    if (key.length <= 100) return key;
    let h = 0x811c9dc5;
    for (let i = 0; i < key.length; i++) h = Math.imul(h ^ key.charCodeAt(i), 0x01000193);
    return "h" + (h >>> 0).toString(16) + ":" + key.slice(-80);
  }
  // сущность ядра (или её сохранённые в истории данные) → трек app.js: из библиотеки, если он там есть (свежие
  // обложка и ссылки), иначе — как сохранили
  function fromCore(x) {
    const raw = x && x.kind === "track" ? x.raw : x;
    if (!raw || typeof raw !== "object" || !raw.key) return null;
    return allTracks().find((t) => t.key === raw.key) || (S.queue || []).find((t) => t.key === raw.key) || raw;
  }

  W.registerProvider({
    id: "vk",
    name: "VK Music",
    caps: {
      playback: "local",
      queue: true,
      library: true,
      playlists: { read: true, create: true, edit: true, reorder: false, delete: true },
      radio: true,
      radioSeeds: ["user"],
      lyrics: "plain",
      search: ["track"],
      download: true,
    },
    toTrack: (t) => ({
      provider: t.path ? "local" : t.service || "vk",
      id: trackId(t.key),
      title: t.title,
      artists: t.artist ? [{ id: "", name: t.artist }] : [],
      cover: t.cover || "",
      durationMs: (t.duration || 0) * 1000,
      available: !t.unavailable,
      raw: t,
    }),
    // что модули ядра сохраняют о треке (история очереди): поля трека app.js; id нужен истории и у файлов
    store: (t) => (t && t.key ? { ...t, id: t.id != null ? t.id : t.key } : null),
  });

  // ---------- плеер ----------
  const playing = () => !!S.current && !S.pl.paused && !S.pl.idle;
  W.player.bind({
    position: () => S.pl.pos || 0,
    duration: () => S.pl.duration || (S.current && S.current.duration) || 0,
    context: () => (S.queueMix ? { type: "radio", id: S.queueMix, title: L("VK Микс") } : null),
    play: () => { if (!playing()) togglePlay(); },
    pause: () => { if (playing()) togglePlay(); },
    toggle: () => togglePlay(),
    next: () => next(false),
    prev: () => prev(),
    seek: (sec) => { if (S.current) seekTo(sec); },
    setVolume: (v) => {
      S.volume = Math.round(v * 100);
      S.muted = false;
      $("#vol").value = S.volume;
      syncVolume();
      invoke("player_volume", { volume: S.volume }).catch(() => {});
    },
    media: () => ({
      paused: !playing(),
      duration: S.pl.duration || 0,
      time: S.pl.pos || 0,
      has: true,
      pending: !!S.current && S.pl.idle,
    }),
  });

  // ---------- очередь ----------
  // Номера треков S.queue, которые заиграют дальше, по порядку. Перемешивание знает только следующий
  // (он выбирается заранее и кэшируется — shufflePick в app.js), дальше — случай.
  function order(max) {
    const n = S.queue.length, out = [];
    if (!n || S.qi < 0 || S.repeat === "one") return out;
    if (S.shuffle && n > 1) return max > 0 ? [shufflePick()] : out;
    for (let i = S.qi + 1; i < n && out.length < max; i++) out.push(i);
    if (S.repeat === "all") for (let i = 0; i < S.qi && out.length < max; i++) out.push(i);
    return out;
  }
  function changed() {
    updateQueueInfo();
    W.player.report({ queue: true });
  }
  W.queue.bind({
    list: () => S.queue,
    index: () => S.qi,
    upcoming: (max) => order(max).map((index) => ({ index, track: S.queue[index] })),
    upcomingCount: () => order(Infinity).length,
    label: () => (S.queueMix ? L("VK Микс") : ""),
    playAt: (i) => { if (S.queue[i]) playAt(i); },
    playNext(x, opts = {}) {
      const t = fromCore(x);
      if (!t || t.unavailable) return;
      if (!S.queue.length || S.qi < 0) return playList([t], 0);
      S.queue.splice(S.qi + 1, 0, t);
      if (S.shuffle) S.shuffleNext = S.qi + 1;
      else if (S.shuffleNext != null && S.shuffleNext > S.qi) S.shuffleNext++;
      S.history = S.history.map((i) => (i > S.qi ? i + 1 : i));
      changed();
      if (!opts.quiet) toast(L("«{0}» заиграет следующим", t.title));
    },
    add(x) {
      const t = fromCore(x);
      if (!t || t.unavailable) return;
      if (!S.queue.length) return playList([t], 0);
      S.queue.push(t);
      changed();
    },
    remove(i) {
      if (!S.queue[i] || i === S.qi) return null;
      const [t] = S.queue.splice(i, 1);
      if (i < S.qi) S.qi--;
      if (S.shuffleNext === i) S.shuffleNext = null;
      else if (S.shuffleNext > i) S.shuffleNext--;
      S.history = S.history.filter((h) => h !== i).map((h) => (h > i ? h - 1 : h));
      changed();
      return t;
    },
    // from / to — места в upcoming(): переставляем «что дальше»; с повтором очереди она поворачивается так,
    // чтобы играющий трек стоял первым (порядок по кругу тот же)
    move(from, to) {
      if (S.shuffle) return;
      const idx = order(Infinity);
      if (!(from >= 0 && from < idx.length && to >= 0 && to < idx.length) || from === to) return;
      const items = idx.map((i) => S.queue[i]);
      items.splice(to, 0, ...items.splice(from, 1));
      if (idx.some((i) => i < S.qi)) {
        S.queue = [S.queue[S.qi], ...items];
        S.qi = 0;
      } else S.queue = S.queue.slice(0, S.qi + 1).concat(items);
      S.history = [];
      changed();
    },
    play(tracks, start = 0) {
      const list = (tracks || []).map(fromCore).filter((t) => t && !t.unavailable);
      if (list.length) playList(list, Math.min(Math.max(0, start), list.length - 1));
    },
  });

  // ---------- набор интерфейса для модулей ядра ----------
  // Подтверждение — наше окошко у кнопки (confirmClick), не window.confirm (правило владельца): кнопка — та,
  // на которую только что нажали. «Отмена» просто закрывает окошко — действие не выполняется.
  let lastButton = null;
  document.addEventListener("pointerdown", (ev) => { lastButton = ev.target.closest && ev.target.closest("button"); }, true);
  W.ui.provide({
    toast: (text, ms, opts) => toast(text, null, opts && opts.action, ms),
    confirm: ({ title, text, ok } = {}) => new Promise((resolve) => {
      const btn = lastButton && lastButton.isConnected ? lastButton : document.activeElement;
      if (!btn || btn === document.body) return resolve(false);
      // после текущего нажатия: оно ещё всплывает до document, а там нажатие мимо окошка его закрывает
      setTimeout(() => confirmClick(btn, () => resolve(true), [title, text].filter(Boolean).join(" "), ok || L("Удалить")), 0);
    }),
    closeMenu: () => { closePopups(); closeConfirm(); },
  });
})();
