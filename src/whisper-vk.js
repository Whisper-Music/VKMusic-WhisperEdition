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
  // Разметка — как у Whisper Yandex, поэтому и роли те же (yandex.js там). Публичный контракт с темами
  // (docs/THEMES.md): меняются вместе с ним.
  const NAV_BTN = ".main .nav, .settings .nav, .modalbox .nav, .login-box .nav, .tmenu .nav";
  W.ui.define({
    "titlebar": ".titlebar",
    "titlebar.button": ".tbctl button",
    "tabs": ".tabs",
    "tab": ".tabs .tab:not(.active)",
    "tab:active": ".tabs .tab.active",
    "sidebar": ".side",
    "nav": ".side .nav:not(.active)",
    "nav:active": ".side .nav.active",
    "search:global": "#search",
    "search:settings": ".tlfind",
    "main": ".main",
    "page.title": "#title",
    "page.subtitle": "#sub",
    "page.cover": "#headImg",
    "button": NAV_BTN.split(", ").map((x) => x.replace(".nav", ".nav:not(.active)")).join(", "),
    "button:active": ".main .nav.active:not(.danger), .modalbox .nav.active:not(.danger), .login-box .nav.active:not(.danger)",
    "button:danger": ".nav.danger",
    "chip": ".chip:not(.active)",
    "chip:active": ".chip.active",
    "tracklist": ".list",
    "track": ".list li.track:not(.playing)",
    "track:playing": ".list li.track.playing",
    "track.cover": ".list li.track > img",
    "track.title": ".list li.track .t",
    "track.artist": ".list li.track .a",
    "track.like": ".list .likebtn",
    "track.more": ".list .morebtn",
    "card": ".settings > .card, .stats .card",
    "tile": ".simcard, .lastAlbum",
    "player": ".player",
    "player.cover": "#cover",
    "player.info": ".player .meta",
    "player.title": "#pTitle",
    "player.artist": "#pArtist",
    "player.like": "#likeBtn",
    "player.controls": ".player .ctrls",
    "player.play": "#play",
    "player.prev": "#prev",
    "player.next": "#next",
    "player.mode": ".player .modebtn:not(.on)",
    "player.mode:on": ".player .modebtn.on",
    "player.seek": "#seek",
    "player.time": ".player .time",
    "player.actions": ".player .pright",
    "player.extra": "#togetherBtn, #normBtn",
    "player.volume": "#volWrap",
    "player.volume.button": "#volBtn",
    "player.volume.slider": "#vol",
    "player.more": "#pMore",
    "radio": ".wavebtn",
    "visualizer": ".stage",
    "menu": ".tmenu",
    "menu.item": ".tmenu .mi",
    "toast": ".toast",
    "dialog": ".modalbox, .login-box",
    "dialog.backdrop": ".modal",
    "queue": ".qpanel",
    "fullplayer": "#full",
    "fullplayer.seek": "#fSeek",
    "fullplayer.prev": "#fPrev",
    "fullplayer.next": "#fNext",
    "lyrics": "#fLyrics",
  });

  // ---------- треки: наш объект трека (app.js: localTrack / svcTrack) ↔ сущность ядра ----------
  // id в ядре — до 100 символов; ключ библиотеки (путь файла, vk:…, sc:…) бывает длиннее — тогда хэш и хвост
  const fnv = (s) => {
    let h = 0x811c9dc5;
    for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
    return (h >>> 0).toString(16);
  };
  function trackId(key) {
    key = String(key || "");
    return key.length <= 100 ? key : "h" + fnv(key) + ":" + key.slice(-80);
  }
  // сущность ядра (или её сохранённые в истории данные) → трек app.js: из библиотеки, если он там есть (свежие
  // обложка и ссылки), иначе — как сохранили. Трек комнаты «Слушать вместе» (roomId) и метка архива ведущего
  // (waShared) сохраняются: по ним ядро узнаёт трек комнаты и находит его в архиве.
  function fromCore(x) {
    const raw = x && x.kind === "track" ? x.raw : x;
    if (!raw || typeof raw !== "object" || !raw.key) return null;
    const lib = allTracks().find((t) => t.key === raw.key) || (S.queue || []).find((t) => t.key === raw.key);
    if (!lib) return raw;
    return raw.roomId || raw.waShared ? { ...lib, roomId: raw.roomId, waShared: raw.waShared } : lib;
  }

  // ---------- «Слушать вместе»: трек ↔ формат сервера комнат ----------
  // Сервер хранит поля трека, а не ссылку на сервис: гость ищет у себя тот же трек — по ключу (тот же файл / тот же
  // трек сервиса в библиотеке), по исполнителю + названию + длительности (libraryMatch), трек SoundCloud играет по id.
  // Не нашёлся — заготовка «room:…»: при включении её найдёт архив ведущего (waShared) или один поиск ВКонтакте.
  const https = (u) => (/^https:\/\//.test(u || "") ? String(u).slice(0, 300) : null);
  function fromRoom(x) {
    const id = String(x?.id || ""), artist = (x?.artists || []).map((a) => a?.name).filter(Boolean).join(", ");
    const want = { key: id, artist, title: String(x?.title || ""), duration: Math.round((x?.durationMs || 0) / 1000) };
    const lib = allTracks().find((t) => t.key === id) || (artist ? libraryMatch(want) : null);
    if (lib) return { ...lib, roomId: id };
    if (/^sc:\d+$/.test(id)) return { ...svcTrack("sc", { id: id.slice(3), artist, title: want.title, duration: want.duration, cover: https(x.coverUri) }), roomId: id };
    return { key: "room:" + id, roomId: id, room: true, artist: artist || L("Неизвестный исполнитель"), title: want.title || L("Без названия"),
      duration: want.duration, cover: https(x?.coverUri) };
  }
  // заготовку комнаты — в трек ВК: один поиск, совпадение по названию, исполнителю и длительности ±3 с
  async function resolveRoom(t) {
    const p = await invoke("vk_search", { query: `${t.artist} ${t.title}`.slice(0, 200), more: null });
    const found = (p.tracks || []).map((x) => svcTrack("vk", x)).filter((x) => !x.unavailable);
    const same = found.find((x) => normText(x.title) === normText(t.title) && normText(x.artist) === normText(t.artist)
      && (!t.duration || !x.duration || Math.abs(x.duration - t.duration) <= 3));
    return same ? { ...same, roomId: t.roomId, waShared: t.waShared } : null;
  }
  window.resolveRoomTrack = resolveRoom;

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
      id: trackId(t.roomId || t.key),
      title: t.title,
      artists: t.artist ? [{ id: "", name: t.artist }] : [],
      cover: t.cover || "",
      durationMs: (t.duration || 0) * 1000,
      available: !t.unavailable,
      raw: t,
    }),
    // что модули ядра сохраняют о треке (история очереди): поля трека app.js; id нужен истории и у файлов
    store: (t) => (t && t.key ? { ...t, id: t.id != null ? t.id : t.key } : null),
    // Whisper Archive: файл трека Rust берёт сам по ключу библиотеки (whisper_vk.rs); обложка файла (vkm://) —
    // не адрес для скачивания: Rust возьмёт её из тегов файла
    archiveSave: (t, a) => {
      const r = t.raw || {};
      if (r.room || !(r.path || S.saved.some((x) => x.key === r.key))) return Promise.reject(new Error(L("В архив сохраняются только треки библиотеки")));
      return invoke("wa_save_track", { key: r.key, file_name: a.fileName, album_id: a.albumId, album_title: a.albumTitle,
        cover_url: https(a.coverUrl), cover_bytes: a.coverBytes });
    },
    // id файла из папки музыки — не путь (в нём имя пользователя), а хэш: гостю файл всё равно не открыть,
    // он ищет трек у себя по названию и исполнителю
    toRoom: (e) => ({
      id: e.raw && e.raw.path && !e.raw.roomId ? "local:" + fnv(e.raw.key) + fnv("vkm" + e.raw.key) : e.id,
      albumId: null,
      title: String(e.title || "Без названия").slice(0, 300),
      version: null,
      artists: e.artists.slice(0, 10).map((a) => ({ id: null, name: String(a.name).slice(0, 200) })),
      coverUri: https(e.cover),
      durationMs: Math.max(0, Math.round(e.durationMs || 0)),
    }),
    fromRoom,
    accountLabel: "аккаунта ВКонтакте или SoundCloud",
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
    setRate: (r) => setRate(r),
    // «Слушать вместе»: включить трек комнаты (сразу за играющим, очередь остаётся) с позиции at; hold — без звука
    playTrack: (e, { at = 0, hold = false } = {}) => {
      const t = fromCore(e);
      if (!t) return Promise.resolve();
      if (S.qi < 0 || !S.queue.length) { S.queue = [t]; S.qi = -1; }
      else S.queue.splice(S.qi + 1, 0, t);
      return playAt(S.qi + 1, false, { at, hold });
    },
    reload: (at = 0, hold = false) => (S.current ? playAt(S.qi, false, { at, hold }) : Promise.resolve()),
    // что с самим файлом: загрузка — от включения до первого состояния mpv с длительностью (S.loading в app.js)
    media: () => ({
      paused: !playing(),
      ended: !!S.current && S.pl.idle && !S.loading,
      duration: S.pl.duration || 0,
      pending: false,
      loading: !!S.loading,
      startAt: S.loading ? S.startAt || 0 : null,
      has: true,
      time: S.pl.pos || 0,
      rate: S.rate || 1,
      buffering: false,
      error: false,
    }),
  });
  // меню трека, консоль и «Слушать вместе» зовут это с треком app.js (старое имя из ядра)
  window.playNext = (t, quiet) => W.queue.playNext(t, { quiet: !!quiet });

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

  // ---------- меню трека «⋯» (пункты модулей ядра: «Слушать вместе», «Сохранить в архив») ----------
  const tmenu = document.createElement("div");
  tmenu.className = "popup tmenu";
  tmenu.hidden = true;
  tmenu.setAttribute("role", "menu");
  document.body.append(tmenu);
  let tmenuBtn = null, menuSeq = 0;
  function closeTrackMenu() {
    if (tmenu.hidden) return;
    menuSeq++;
    tmenu.hidden = true;
    tmenu.replaceChildren();
    if (tmenuBtn) tmenuBtn.classList.remove("open");
    tmenuBtn = null;
  }
  function openMenu(btn, build) {
    if (tmenuBtn === btn) return closeTrackMenu();
    closeTrackMenu();
    closePopups();
    tmenuBtn = btn;
    btn.classList.add("open");
    tmenu.hidden = false;
    build(menuSeq);
    placeMenu();
  }
  const menuReset = () => (tmenu.replaceChildren(), ++menuSeq);
  const menuAlive = (seq) => !tmenu.hidden && seq === menuSeq;
  // icon — SVG-строка (свои значки модулей ядра и наши Icons.svg), текст — только textContent
  function menuItem(icon, text, fn, { keep = false, arrow = false, danger = false, to = tmenu } = {}) {
    const b = el("button", "mi" + (danger ? " danger" : ""));
    b.innerHTML = icon || "";
    b.append(el("span", "mt", text));
    if (arrow) b.append(el("i", "miarrow", "›"));
    b.addEventListener("click", (e) => { e.stopPropagation(); if (!keep) closeTrackMenu(); fn(); });
    to.append(b);
    return b;
  }
  function menuHead(text, back) {
    const h = el("div", "mhead"), b = el("button", "mback", "←");
    b.title = L("Назад");
    b.addEventListener("click", (e) => { e.stopPropagation(); back(); });
    h.append(b, el("b", null, text));
    tmenu.append(h);
  }
  const menuNote = (text, to = tmenu) => { const p = el("p", "mnote", text); to.append(p); return p; };
  function menuSep() {
    const last = tmenu.lastElementChild;
    if (last && !last.classList.contains("msep")) tmenu.append(el("hr", "msep"));
  }
  function placeMenu() {
    if (tmenu.hidden || !tmenuBtn) return;
    const r = tmenuBtn.getBoundingClientRect(), m = tmenu.getBoundingClientRect();
    const left = Math.max(8, Math.min(r.right - m.width, innerWidth - m.width - 8));
    const top = r.bottom + 6 + m.height <= innerHeight - 8 ? r.bottom + 6 : Math.max(8, r.top - m.height - 6);
    tmenu.style.left = left + "px";
    tmenu.style.top = top + "px";
  }
  function trackMenuMain(t) {
    const anchor = tmenuBtn;                                          // для окошка «В плейлист» после закрытия меню
    menuReset();
    if (W.ui.trackMenu.build(t, null, "top") > 0) menuSep();       // «Слушать вместе · КОД» (core/together.js)
    if (!t.room && !t.unavailable && !(S.current && S.current.key === t.key)) menuItem(Icons.svg("playnext"), L("Играть следующим"), () => window.playNext(t));
    if (canAddToPlaylist(t)) menuItem(Icons.svg("listadd"), L("В плейлист ВК"), () => addToPlaylistMenu(anchor, t));
    W.ui.trackMenu.build(t, null);                                    // «Сохранить в архив» (core/archive.js)
    if (!tmenu.children.length) menuNote(L("Здесь пока нечего делать"));
    placeMenu();
  }
  /** Меню «⋯» трека app.js у кнопки btn (строка списка, плеер). */
  window.openTrackMenu = (t, btn) => openMenu(btn, () => trackMenuMain(t));
  document.addEventListener("pointerdown", (e) => { if (!tmenu.hidden && !tmenu.contains(e.target) && !(tmenuBtn && tmenuBtn.contains(e.target))) closeTrackMenu(); }, true);
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeTrackMenu(); });
  window.addEventListener("resize", closeTrackMenu);
  document.addEventListener("scroll", (e) => { if (!tmenu.contains(e.target)) closeTrackMenu(); }, true);

  // ---------- карточки настроек модулей: вкладка «Интеграции» ----------
  // Карточка вставляется сразу и остаётся в документе (модули ищут свои элементы по id): #settingsPage — постоянная
  // секция, как у Whisper; видна ли карточка — решает вкладка (data-tab).
  function settingsCard(card, { first, colbreak } = {}) {
    const page = document.getElementById("settingsPage");
    if (!page) return false;
    card.classList.add("card");
    if (colbreak) card.classList.add("colbreak");
    card.dataset.tab = "int";
    const firstOf = page.querySelector(':scope > .card[data-tab="int"]');
    if (first && firstOf) firstOf.before(card);
    else page.append(card);
    return true;
  }
  // цвета стиля для виджета OBS: строго #rrggbb — из переменных стиля (app.js ставит их на <html>), иначе по умолчанию
  function accentColors() {
    const cs = document.documentElement.style, hex = (v) => (/^#[0-9a-f]{6}$/i.test(v) ? v : null);
    const accent = hex(cs.getPropertyValue("--accent").trim()), accent2 = hex(cs.getPropertyValue("--accent2").trim());
    return accent ? { accent, accent2: accent2 || accent } : null;
  }

  // Оформление виджета OBS — секции whisper-obs текущей темы. Разбирает их движок ядра (WMT.open + obsCss) из файла
  // темы (theme_file), поэтому файл читается, только когда виджет спросил (он включён) и только раз на тему.
  let obsCache = null, obsLoading = "";
  function obsTheme() {
    const th = S.theme;
    if (!th) return null;
    if (obsCache && obsCache.key === th.id) return obsCache;
    if (!th.obs) return (obsCache = { key: th.id, css: "", settings: {} });
    if (obsLoading !== th.id) {
      obsLoading = th.id;
      invoke("theme_file", { id: th.id })
        .then((buf) => WMT.open(new Uint8Array(buf)))
        .then((wmt) => { if (S.theme && S.theme.id === th.id) obsCache = { key: th.id, ...WhisperThemeCss.obsCss(wmt) }; })
        .catch((e) => console.warn("whisper-vk: оформление OBS темы", e))
        .finally(() => { obsLoading = ""; });
    }
    return null;          // виджет спросит снова (раз в 1,5 с) — к тому времени оформление готово
  }

  // ---------- набор интерфейса для модулей ядра ----------
  // Подтверждение — наше окошко у кнопки (confirmClick), не window.confirm (правило владельца): кнопка — та,
  // на которую только что нажали. «Отмена» просто закрывает окошко — действие не выполняется.
  let lastButton = null;
  document.addEventListener("pointerdown", (ev) => { lastButton = ev.target.closest && ev.target.closest("button"); }, true);
  W.ui.provide({
    toast: (text, ms, opts) => toast(text, opts && opts.kind === "err" ? "error" : opts && opts.kind === "ok" ? "ok" : null, opts && opts.action, ms),
    confirm: ({ title, text, ok } = {}) => new Promise((resolve) => {
      const btn = lastButton && lastButton.isConnected ? lastButton : document.activeElement;
      if (!btn || btn === document.body) return resolve(false);
      // после текущего нажатия: оно ещё всплывает до document, а там нажатие мимо окошка его закрывает
      setTimeout(() => confirmClick(btn, () => resolve(true), [title, text].filter(Boolean).join(" "), ok || L("Удалить")), 0);
    }),
    closeMenu: () => { closeTrackMenu(); closePopups(); closeConfirm(); },
    trackMenu: (t, btn) => { const raw = fromCore(t); if (raw) window.openTrackMenu(raw, btn); },
    icon: (name) => ({ more: Icons.svg("dots"), plus: Icons.svg("plus"), grip: Icons.svg("grip") }[name] || ""),
    menu: (raw) => ({
      item: menuItem,
      note: menuNote,
      head: (text) => menuHead(text, () => trackMenuMain(raw)),
      scroll: () => { const b = el("div", "mscroll"); tmenu.append(b); return b; },
      place: placeMenu,
      reset: menuReset,
      alive: menuAlive,
      anchor: () => tmenuBtn,
      reopen: (anchor, build) => openMenu(anchor, build),
      close: closeTrackMenu,
      append: (x) => tmenu.append(x),
    }),
    settingsCard,
    accentColors,
    obsTheme: () => obsTheme(),
    // ключ архива лёг не в связку ключей, а в файл открытым текстом — предупредить
    secretSaved: () => invoke("secret_backend").then((b) => {
      if (b === "file") toast(L("Связки ключей в системе нет — ключ Whisper Archive сохранён в файле ~/.config/vkmusic (только для вас)"), "error");
    }).catch(() => {}),
  });
})();
