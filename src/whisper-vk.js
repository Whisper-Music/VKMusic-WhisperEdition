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
    "search:settings": ".tlfind, #fontFind",
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
    "player.extra": "#togetherBtn, #lyrBtn, #normBtn",
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
  // сохранённое store → трек app.js: свой id из rid
  function unstore(raw) {
    if (!("rid" in raw)) return raw;
    const { rid, ...t } = raw;
    if (rid == null) delete t.id; else t.id = rid;
    return t;
  }
  // История очереди до 2026-11-06 хранила в id свой id трека (store), а не id ядра — повторы не узнавались,
  // одно и то же прослушивание копилось строками. Переводим записи на id ядра, пока core/queue.js не прочитал
  // историю: при загрузке он сам оставляет одно, самое свежее прослушивание каждого трека.
  (() => {
    const KEY = "vkm_history";
    let h;
    try { h = JSON.parse(localStorage.getItem(KEY)); } catch { return; }
    if (!Array.isArray(h) || !h.some((x) => x && x.t && x.t.key && !("rid" in x.t))) return;
    for (const x of h) if (x && x.t && x.t.key && !("rid" in x.t)) x.t = { ...x.t, id: trackId(x.t.roomId || x.t.key), rid: x.t.path ? null : x.t.id ?? null }; // у файла id не было — store подставлял ключ
    try { localStorage.setItem(KEY, JSON.stringify(h)); } catch {}
  })();
  function fromCore(x) {
    const raw = x && x.kind === "track" ? x.raw : x;
    if (!raw || typeof raw !== "object" || !raw.key) return null;
    const lib = libraryByKey(raw.key) || (S.queue || []).find((t) => t.key === raw.key);
    if (!lib) return unstore(raw);
    return raw.roomId || raw.waShared ? { ...lib, roomId: raw.roomId, waShared: raw.waShared } : lib;
  }

  // ---------- «Слушать вместе»: трек ↔ формат сервера комнат ----------
  // Сервер хранит поля трека, а не ссылку на сервис: гость ищет у себя тот же трек — по ключу (тот же файл / тот же
  // трек сервиса в библиотеке), по исполнителю + названию + длительности (libraryMatch), трек SoundCloud играет по id.
  // Не нашёлся — заготовка «room:…»: при включении её найдёт архив ведущего (waShared) или один поиск ВКонтакте.
  const https = (u) => (/^https:\/\//.test(u || "") ? String(u).slice(0, 300) : null);
  // Что fromRoom ответил на трек комнаты (id → трек app.js). Ядро (together.js, resolveRoom) зовёт fromRoom перед
  // tracks([id]), поэтому tracks отвечает без сети: исполнитель и название трека комнаты уже известны.
  const roomSeen = new Map();
  function fromRoom(x) {
    const id = String(x?.id || ""), artist = (x?.artists || []).map((a) => a?.name).filter(Boolean).join(", ");
    const want = { key: id, artist, title: String(x?.title || ""), duration: Math.round((x?.durationMs || 0) / 1000) };
    const lib = libraryByKey(id) || (artist ? libraryMatch(want) : null);
    let t;
    if (lib) t = { ...lib, roomId: id };
    else if (/^sc:\d+$/.test(id)) t = { ...svcTrack("sc", { id: id.slice(3), artist, title: want.title, duration: want.duration, cover: https(x.coverUri) }), roomId: id };
    else t = { key: "room:" + id, roomId: id, room: true, artist: artist || L("Неизвестный исполнитель"), title: want.title || L("Без названия"),
      duration: want.duration, cover: https(x?.coverUri) };
    roomSeen.delete(id);
    roomSeen.set(id, t);
    if (roomSeen.size > 300) roomSeen.delete(roomSeen.keys().next().value);
    return t;
  }
  // Поиск трека ВКонтакте для комнаты (ядро: provider.search, наше resolveRoom): один запрос на одну строку — ответ
  // (и ошибка) помнится минуту, чтобы ядро и плеер не искали одно и то же дважды (правило «без повторов»).
  const vkFound = new Map();
  function vkSearchOnce(query) {
    const q = String(query || "").trim().slice(0, 200);
    const hit = vkFound.get(q);
    if (hit && Date.now() - hit.at < 60e3) return hit.p;
    const hit2 = { at: Date.now(), p: null, failed: false };
    const p = invoke("vk_search", { query: q, more: null }).then((r) => (r.tracks || []).map((x) => svcTrack("vk", x)));
    p.catch(() => { hit2.failed = true; });
    hit2.p = p;
    vkFound.set(q, hit2);
    if (vkFound.size > 50) vkFound.delete(vkFound.keys().next().value);
    return p;
  }
  // заготовку комнаты — в трек ВК: один поиск, совпадение по названию, исполнителю и длительности ±3 с
  async function resolveRoom(t) {
    const found = (await vkSearchOnce(`${t.artist} ${t.title}`)).filter((x) => !x.unavailable);
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
    // что модули ядра сохраняют о треке (история очереди): поля трека app.js, id — как у трека ядра (toTrack):
    // по нему история находит прошлое прослушивание того же трека (иначе каждое — новой строкой); свой id — в rid
    store: (t) => (t && t.key ? { ...t, id: trackId(t.roomId || t.key), rid: t.id ?? null } : null),
    // Whisper Archive: файл трека Rust берёт сам по ключу библиотеки (whisper_vk.rs); обложка файла (vkm://) —
    // не адрес для скачивания: Rust возьмёт её из тегов файла
    archiveSave: (t, a) => {
      const r = t.raw || {};
      if (r.room || !(r.path || S.saved.some((x) => x.key === r.key))) return Promise.reject(new Error(L("В архив сохраняются только треки библиотеки")));
      return invoke("wa_save_track", { key: r.key, file_name: a.fileName, album_id: a.albumId, album_title: a.albumTitle,
        cover_url: https(a.coverUrl), cover_bytes: a.coverBytes });
    },
    // id файла из папки музыки — не путь (в нём имя пользователя), а хэш: гостю файл всё равно не открыть,
    // он ищет трек у себя по названию и исполнителю.
    // coverUri — всегда null: сервер комнат общий с Whisper Yandex и принимает только обложки Яндекса
    // (avatars.yandex.net/…/%%); адрес ВКонтакте/SoundCloud он считает «некорректным треком» и отклоняет трек
    // целиком — и предложение, и состояние ведущего (гости не получали трек). Обложку гость берёт у себя.
    toRoom: (e) => ({
      id: e.raw && e.raw.path && !e.raw.roomId ? "local:" + fnv(e.raw.key) + fnv("vkm" + e.raw.key) : e.id,
      albumId: null,
      title: String(e.title || "Без названия").slice(0, 300),
      version: null,
      artists: e.artists.slice(0, 10).map((a) => ({ id: null, name: String(a.name).slice(0, 200) })),
      coverUri: null,
      durationMs: Math.max(0, Math.round(e.durationMs || 0)),
    }),
    fromRoom,
    accountLabel: "аккаунта ВКонтакте или SoundCloud",
    // «Слушать вместе» (ядро: трек комнаты у себя). tracks — без сети: трек библиотеки (по ключу или исполнителю,
    // названию и длительности) или SoundCloud по id. Нет — ядро ищет сам (search), затем в своём архиве, иначе
    // «нет на площадке» (ведущий может поделиться файлом).
    tracks: async (ids) => (ids || []).map((id) => {
      id = String(id);
      const t = roomSeen.get(id) || fromRoom({ id });
      return t && !t.room ? W.to("track", t) : null;
    }).filter(Boolean),
    // поиск ВКонтакте — только для комнаты (у приложения свой поиск): один запрос на трек комнаты, без входа — пусто
    search: async (text, type) => {
      const none = { tracks: [], artists: [], albums: [], playlists: [], best: null };
      if (type && type !== "track") return none;
      try {
        return { ...none, tracks: (await vkSearchOnce(text)).map((t) => W.to("track", t)) };
      } catch (e) {
        if (errText(e) === "Вы не вошли во ВКонтакте") return none;
        throw e;
      }
    },
    // ведущий делится треком, которого нет у гостя: файл — во временный кэш комнаты на сервере архива.
    // Только трек библиотеки (файл Rust берёт сам по ключу, как для «Сохранить в архив»)
    roomCache: (t, code) => {
      const r = t.raw || {};
      const lib = r.room ? null : libraryMatch(r);
      if (!lib || !(lib.path || S.saved.some((x) => x.key === lib.key))) return Promise.reject(new Error(L("Поделиться с гостями можно только треком из библиотеки")));
      return invoke("wa_room_cache_track", { key: lib.key, code: String(code || ""), file_name: `${lib.artist} - ${lib.title}`.slice(0, 150) });
    },
  });

  // ---------- плеер ----------
  const playing = () => !!S.current && !S.pl.paused && !S.pl.idle;
  W.player.bind({
    position: () => S.pl.pos || 0,
    duration: () => S.pl.duration || (S.current && S.current.duration) || 0,
    context: () => (S.queueMix ? { type: "radio", id: S.queueMix, title: L("VK Микс") } : null),
    // Плеер стоит без файла (после перезапуска, выхода из комнаты, доигравший трек): включаем трек заново — с места,
    // куда ядро перед этим перемотало (seek сохраняет его в S.idleSeek), а не с начала. Так гость, который вошёл
    // в комнату, где играет тот же трек, что стоит у него, сразу слышит его в такт с ведущим.
    // Гостю комнаты «играть» от синхронизации ядра (раз в 3 с) не включает трек, который у него доиграл до конца
    // (ждём следующего от ведущего; иначе он начинался бы снова) или не открылся (снова — только когда гость
    // войдёт или вернётся в комнату, без повторов запросов к сервису).
    play: () => {
      if (playing()) return;
      const at = S.idleSeek;
      S.idleSeek = null;
      if (!S.current || !S.pl.idle || S.loading) return togglePlay();
      if (roomGuest()) {
        if (S.playError) return;
        const d = S.current.duration || 0;
        if (at != null ? d > 0 && at > d - 2 : S.roomEnded === S.current) return;
      }
      playAt(S.qi, false, at > 0 ? { at } : undefined);
    },
    pause: () => { if (playing()) togglePlay(); },
    toggle: () => togglePlay(),
    next: () => next(false),
    prev: () => prev(),
    // файла в плеере нет — перематывать нечего: место запоминается для следующего «играть» (выше)
    seek: (sec) => {
      if (!S.current) return;
      if (S.pl.idle && !S.loading) S.idleSeek = Math.max(0, Number(sec) || 0);
      else seekTo(sec);
    },
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
      // трек не открылся (и повтор со свежей ссылкой тоже): гость комнаты по этому говорит ведущему «нет у меня»
      error: !!S.playError && !S.loading,
    }),
  });
  // Гость вошёл в комнату или вернулся к ведущему — это действие пользователя: трек, который раньше не открылся,
  // можно запросить снова, и прошлые неудачные поиски ВКонтакте не в счёт (иначе перезаход минуту не находил трек)
  let roomFollowed = "";
  W.on("room", (r) => {
    const now = r && r.inRoom && !r.host && r.follow ? String(r.code || "") : "";
    if (now && now !== roomFollowed) {
      S.playError = false;
      for (const [k, v] of vkFound) if (v.failed) vkFound.delete(k);
    }
    roomFollowed = now;
  });
  // ---------- кнопки плеера у гостя комнаты ----------
  // Ядро (together.js) у гостя глушит «вперёд / назад» и кнопки режимов (роли player.next/prev, fullplayer.next/prev,
  // player.mode) перехватом на document — даже когда гость отошёл от ведущего и слушает своё: переключить свой трек
  // было нельзя. И «Текст песни» и «Постоянный уровень громкости» у нас тоже .modebtn — их ядро глушило у любого гостя.
  // Эти нажатия ловим раньше — на window (фаза захвата идёт от window к document) — и выполняем сами:
  // «Текст песни» и громкость — всегда, остальное — когда гость отошёл (следует за ведущим — пусть решает ядро).
  const GUEST_OWN = [
    ["#lyrBtn", () => toggleLyrics(), true],
    ["#normBtn", () => toggleNormalize(), true],
    ["#next, #fNext", () => next(false)],
    ["#prev, #fPrev", () => prev()],
    ["#shuffleBtn", () => toggleShuffle()],
    ["#repeatBtn", () => cycleRepeat()],
  ];
  window.addEventListener("click", (e) => {
    const room = W.modules.together, info = room && room.info && room.info();
    if (!info || info.host || !e.target.closest) return;
    for (const [sel, fn, always] of GUEST_OWN) {
      const b = e.target.closest(sel);
      if (!b || b.disabled || (!always && info.follow)) continue;
      e.stopPropagation();
      e.preventDefault();
      fn();
      return;
    }
  }, true);

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
  const menuReset = () => (tmenu.classList.remove("mlong"), tmenu.replaceChildren(), ++menuSeq);
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
    if (anchor && anchor.id === "pMore") playerMenuItems();
    if (!tmenu.children.length) menuNote(L("Здесь пока нечего делать"));
    placeMenu();
  }

  // ---------- «⋯» плеера: скорость и устройство вывода (как у Whisper Yandex — подменю того же .tmenu) ----------
  const RATES = [0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 2];
  const rateText = (r) => r.toLocaleString(document.documentElement.lang || "ru") + "×";
  function playerMenuItems() {
    menuSep();
    const rate = S.rate || 1;
    menuItem(Icons.svg("speed"), L("Скорость: {0}", rateText(rate)), () => rateMenu(), { keep: true, arrow: true });
    menuItem(Icons.svg("output"), L("Устройство вывода"), () => outputMenu(), { keep: true, arrow: true });
    if (!window.__TAURI__?.__host) menuItem(Icons.svg("mini"), L("Мини-плеер"), () => invoke("mini_toggle").catch((e) => toast(errMsg(e), "error")));
  }
  function rateMenu() {
    menuReset();
    menuHead(L("Скорость"), () => trackMenuMain(S.current));
    for (const r of RATES) {
      const b = menuItem(r === (S.rate || 1) ? Icons.svg("check") : "", rateText(r), () => setRate(r));
      b.classList.toggle("on", r === (S.rate || 1));
    }
    placeMenu();
  }
  async function outputMenu() {
    const seq = menuReset();
    menuHead(L("Устройство вывода"), () => trackMenuMain(S.current));
    const wait = menuNote(L("Загрузка…"));
    placeMenu();
    let d;
    try {
      d = await invoke("player_devices");
    } catch (e) {
      if (menuAlive(seq)) { wait.textContent = errMsg(e); placeMenu(); }
      return;
    }
    if (!menuAlive(seq)) return;
    wait.remove();
    // основной список (player::shown_devices): «Системное» и настоящие устройства, по картам — заголовок группы
    const list = d.list.length ? d.list : [{ name: "auto", label: "" }];
    let grp = null;
    for (const dev of list) {
      const g = dev.group || null;
      if (g !== grp) {
        if (g) menuNote(g).classList.add("mgroup");
        else menuSep();
        grp = g;
      }
      deviceItem(dev, d.current);
    }
    // остальное из списка mpv — те же устройства через другие звуковые системы и служебные устройства ALSA
    if (d.more?.length) {
      menuSep();
      menuItem("", L("Все устройства ({0})", d.more.length), () => outputMore(d), { keep: true, arrow: true });
    }
    placeMenu();
  }
  function outputMore(d) {
    menuReset();
    tmenu.classList.add("mlong");
    menuHead(L("Все устройства"), () => outputMenu());
    menuNote(L("Те же устройства через другие звуковые системы и служебные устройства. Обычно хватает основного списка."));
    for (const dev of d.more) deviceItem(dev, d.current);
    placeMenu();
  }
  function deviceItem(dev, current) {
    const on = dev.name === current;
    const label = dev.name === "auto" ? L("Системное (по умолчанию)") : dev.label || dev.name;
    const b = menuItem(on ? Icons.svg("check") : "", label, () => {
      invoke("player_device", { name: dev.name })
        .then(() => toast(L("Звук: {0}", label)))
        .catch((e) => toast(errMsg(e), "error"));
    });
    b.classList.toggle("on", on);
    b.title = dev.group ? dev.group + " — " + label : dev.name;
  }
  /**
   * Меню выбора у кнопки (язык текста песни): items — [{ label, on, fn }]. То же .tmenu, что «⋯» трека — в оформлении
   * темы, а не выпадающий список <select> (его WebKitGTK рисует белым GTK).
   */
  window.openPickMenu = (btn, items) => openMenu(btn, () => {
    menuReset();
    for (const it of items || []) {
      const b = menuItem(it.on ? Icons.svg("check") : "", String(it.label), it.fn);
      b.classList.toggle("on", !!it.on);
      b.setAttribute("aria-checked", it.on ? "true" : "false");
    }
    placeMenu();
  });
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
  // fallback — у какой кнопки спросить, если нажатую уже убрали (кнопка в подсказке, которая закрылась)
  const askConfirm = ({ title, text, ok } = {}, fallback = null) => new Promise((resolve) => {
    let btn = lastButton && lastButton.isConnected ? lastButton : document.activeElement;
    if ((!btn || btn === document.body) && fallback && fallback.isConnected) btn = fallback;
    if (!btn || btn === document.body) return resolve(false);
    // после текущего нажатия: оно ещё всплывает до document, а там нажатие мимо окошка его закрывает
    setTimeout(() => confirmClick(btn, () => resolve(true), [title, text].filter(Boolean).join(" "), ok || L("Удалить")), 0);
  });

  // ---------- «Темы комнаты» («Слушать вместе»): текущий стиль файлом и установка темы из файла ----------
  // Делиться можно только темой из файла: основы Whisper (neon, royal, steam, lolitta) есть у всех, а «Цвета»
  // пользователя (vkm_colors) в файл темы не входят.
  async function themeExport() {
    const th = S.theme;
    if (!th || !th.file) throw new Error(L("Поделиться можно темой из файла — основы Whisper и так есть у всех"));
    const bytes = new Uint8Array(await invoke("theme_file", { id: th.id }));
    const info = (S.themes || []).find((x) => x.id === th.id);
    return { id: th.id, name: th.name || (info && info.name) || th.id, author: (info && info.author) || "", bytes };
  }
  // true — поставлена и включена, false — не вышло (ошибка уже показана), null — не стали заменять свою тему с тем же id
  async function themeImport(bytes) {
    bytes = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    let id = "";
    try { id = String((await WMT.open(bytes)).manifest.id || ""); } catch {}
    const list = await invoke("themes_list").catch(() => S.themes || []);
    const have = id && list.find((x) => x.id === id && !x.builtin);
    if (have) {
      const old = new Uint8Array(await invoke("theme_file", { id }).catch(() => new ArrayBuffer(0)));
      const same = old.length === bytes.length && old.every((b, i) => b === bytes[i]);
      const ask = { text: L("У вас уже есть тема «{0}» — заменить её этой?", have.name), ok: L("Заменить") };
      if (!same && !(await askConfirm(ask, document.getElementById("togetherBtn")))) return null;
    }
    try {
      const info = await invoke("theme_install", bytes);
      await selectTheme(info.id);
      toast(L("Тема «{0}» установлена", info.name), "ok");
      return true;
    } catch (e) {
      toast(errMsg(e), "error");
      return false;
    }
  }

  W.ui.provide({
    toast: (text, ms, opts) => toast(text, opts && opts.kind === "err" ? "error" : opts && opts.kind === "ok" ? "ok" : null, opts && opts.action, ms),
    confirm: (o) => askConfirm(o),
    themeExport,
    themeImport,
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
    // «сердце» в строках очереди и истории (core/queue.js): то же, что в наших списках — трек в библиотеке
    likeButton: (x) => {
      const t = fromCore(x);
      return t && !t.room && !t.unavailable ? likeButton(t) : null;
    },
    settingsCard,
    accentColors,
    obsTheme: () => obsTheme(),
    // ключ архива лёг не в связку ключей, а в файл открытым текстом — предупредить
    secretSaved: () => invoke("secret_backend").then((b) => {
      if (b === "file") toast(L("Связки ключей в системе нет — ключ Whisper Archive сохранён в файле ~/.config/vkmusic (только для вас)"), "error");
    }).catch(() => {}),
  });
})();
