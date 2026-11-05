"use strict";
// VK Music — плеер: библиотека (файлы + сохранённые треки сервисов), поиск во ВКонтакте и SoundCloud.
// Всё общение с Rust — через invoke/события Tauri. Ядро Whisper (src/core) подключено раньше: о плеере и очереди
// ему сообщает Whisper.player.report(…) отсюда, а привязка к ядру (роли, провайдер, плеер, очередь) — в whisper-vk.js.

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
const listen = TAURI.event.listen;
/** Интерфейс в браузере (vkmusic --host): __TAURI__ подменяет src/host.js. */
const HOST = !!TAURI.__host;
/** Начало адресов протокола vkm:// (обложки файлов): vkm://localhost/ в окне, /p/vkm/ в браузере. */
const VKM = TAURI.core.convertFileSrc("", "vkm");

const $ = (sel, root = document) => root.querySelector(sel);
/** Прокручивается средняя панель целиком (как у Whisper): .main. */
const scroller = () => document.querySelector(".main");
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};

// названия страниц (русские — ключи перевода, переводятся при показе: L(PAGES[page]))
const PAGES = {
  library: "Библиотека",
  wave: "VK Микс",
  vk: "Миксы и плейлисты",
  search: "Поиск",
  stats: "Статистика",
  services: "Сервисы",
  settings: "Настройки",
  store: "Магазин тем",
};

const SERVICES = {
  // название — на языке интерфейса (VK по-английски)
  vk: { get name() { return L("ВКонтакте"); }, icon: "vk" },
  sc: { name: "SoundCloud", icon: "sc" },
};


const S = {
  page: "library",
  lastPage: "library", // куда вернуться из настроек
  musicDir: "",
  downloadDir: "",     // куда кнопка «Скачать» кладёт треки
  cacheInfo: null,     // { dir, count, bytes }; null — ещё не спрашивали
  saving: new Set(),   // ключи треков, которые сейчас скачиваются в папку загрузок
  local: [],
  saved: [],
  localLoaded: false,
  syncing: new Set(),  // сервисы, которые сейчас синхронизируются
  order: new Map(),    // порядок библиотеки (order.json): ключ → место, 0 — самый новый
  normalize: false,
  downloads: new Map(),
  caps: null,
  queue: [],
  qi: -1,
  current: null,
  skips: 0,
  pl: { pos: 0, duration: 0, paused: true, idle: true },
  // для ядра Whisper («Слушать вместе», Discord): трек грузится (от включения до первой длительности от mpv),
  // с какой секунды он начнёт, ждём ли первого звука (report audible), загружен ли «без звука» (hold); скорость
  loading: false,
  startAt: 0,
  needAudible: false,
  holdPending: false,
  playSeq: 0,
  rate: 1,
  seeking: false,
  shuffle: false,
  repeat: "off",
  volume: 70,
  muted: false,
  overrides: {},
  font: null,
  fonts: null,
  theme: null,
  themes: [],
  lib: null,          // каталог библиотеки тем (null — ещё не загружен)
  libCats: [],        // категории библиотеки (папки репозитория): { id, name, description }
  libHidden: [],      // id скрытых категорий (config.json)
  statsData: null,    // вся статистика (stats_all) для страницы «Статистика»; null — не загружена
  vkHome: null,       // раздел «Миксы»: { mixes, groups, mine } | { error }; null — не загружен
  vkPl: null,         // открытый плейлист ВК: { pl, tracks, next, busy, error, from: страница, где открыт }
  myPl: null,         // ваши плейлисты ВК для библиотеки: { list } | { loading } | { error }; null — не загружены
  queueMix: null,     // очередь играет VK Микс (id) — по мере игры догружается
  mixBusy: false,
  statsPeriod: "30",  // период страницы: "1" | "7" | "30" | "all"
  libError: null,
  libBusy: new Set(), // id тем, которые сейчас ставятся/удаляются
  libFilter: "",
  history: [],
  shuffleNext: null,  // при перемешивании — заранее выбранный следующий трек (его и кэшируем)
  search: ["vk", "sc"], // где ищет общая строка поиска
  svc: { vk: null, sc: null }, // статус входа: { logged_in, profile, error }; null — не спрашивали
  sr: null,           // результаты общего поиска
};

// ───────────────────────── утилиты ─────────────────────────

function fmt(sec) {
  sec = Math.max(0, Math.floor(sec || 0));
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  const ss = String(s).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** Длительность списка: «42 мин», «7 ч 17 мин», «2 ч» (минуты округляются до целых, без «1 ч 60 мин»). */
function fmtTotal(sec) {
  const all = Math.round((sec || 0) / 60), h = Math.floor(all / 60), m = all % 60;
  if (!h) return L("{0} мин", m);
  return m ? L("{0} ч {1} мин", h, m) : L("{0} ч", h);
}

/** Сколько слушали: «45 с», «3 мин 20 с», «50 мин», «2 ч 5 мин», «2 ч». */
function fmtListened(sec) {
  sec = Math.floor(sec || 0);
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  if (h) return m ? L("{0} ч {1} мин", h, m) : L("{0} ч", h);
  if (m) return s ? L("{0} мин {1} с", m, s) : L("{0} мин", m);
  return L("{0} с", s);
}

/**
 * Подсказка внизу по центру — одна, как у Whisper (.toast: .tico, текст, .tact). kind: "error" | "ok" | пусто;
 * action — кнопка { label, fn } (например, «Вернуть» у панели очереди ядра); ms — сколько показывать (0 — пока не сменят).
 */
let toastTimer = 0;
function toast(msg, kind, action, ms) {
  const t = $("#toast");
  const k = kind === "error" ? "err" : kind === "ok" ? "ok" : "";
  t.replaceChildren();
  t.className = "toast" + (k ? " " + k : "");
  if (k) t.append(el("i", "tico", k === "ok" ? "✓" : "✕"));
  t.append(el("span", null, String(msg)));
  if (action && typeof action.fn === "function") {
    const b = el("button", "tact", String(action.label || ""));
    b.addEventListener("click", () => ((t.hidden = true), action.fn()));
    t.append(b);
  }
  t.hidden = false;
  clearTimeout(toastTimer);
  if (ms !== 0) toastTimer = setTimeout(() => (t.hidden = true), ms || (k === "err" ? 6000 : action ? 5000 : 3500));
}

const errText = (e) => (typeof e === "string" ? e : e && e.message ? e.message : JSON.stringify(e));
/** Ошибка для показа: известные сообщения Rust (русские) переводятся по словарю. Для проверок — errText. */
const errMsg = (e) => L(errText(e));

/**
 * Подтверждение опасного действия — маленькое окошко у кнопки («Удалить файл с диска?» [Удалить] [Отмена]),
 * не confirm(). Закрывается «Отменой», Esc или нажатием мимо.
 */
/** Окошко у кнопки (подтверждение, ввод названия, выбор плейлиста): одно на экране, закрывается кликом мимо. */
function popAt(btn, cls, fill) {
  closeConfirm();
  const pop = el("div", "popup tmenu confirm-pop" + (cls ? " " + cls : ""));
  pop.id = "confirmPop";
  pop.setAttribute("role", "dialog");
  fill(pop);
  pop.addEventListener("click", (ev) => ev.stopPropagation());
  document.body.append(pop);
  // под кнопкой, не вылезая за окно (если внизу нет места — над ней)
  const r = btn.getBoundingClientRect();
  const w = pop.offsetWidth, h = pop.offsetHeight;
  const left = Math.min(Math.max(8, r.right - w), window.innerWidth - w - 8);
  const top = r.bottom + 6 + h > window.innerHeight - 8 ? r.top - h - 6 : r.bottom + 6;
  pop.style.left = left + "px";
  pop.style.top = Math.max(8, top) + "px";
  btn.classList.add("armed");
  pop._btn = btn;
  return pop;
}

/** Ввести название (новый плейлист, переименование): fn(текст) по «ОК» или Enter. */
function promptClick(btn, fn, label, ok, value = "") {
  popAt(btn, "prompt-pop", (pop) => {
    const input = el("input", "field prompt-input");
    input.type = "text";
    input.maxLength = 128;
    input.value = value;
    input.spellcheck = false;
    const yes = el("button", "nav active", ok);
    const no = el("button", "nav", L("Отмена"));
    const submit = () => {
      const v = input.value.trim();
      if (!v) return input.focus();
      closeConfirm();
      fn(v);
    };
    yes.addEventListener("click", submit);
    no.addEventListener("click", closeConfirm);
    input.addEventListener("keydown", (ev) => {
      ev.stopPropagation(); // не горячие клавиши плеера
      if (ev.key === "Enter") submit();
      if (ev.key === "Escape") closeConfirm();
    });
    const row = el("div", "row confirm-actions");
    row.append(yes, no);
    pop.append(el("div", "confirm-text", label), input, row);
    setTimeout(() => (input.focus(), input.select()), 0);
  });
}

function confirmClick(btn, fn, label = L("Точно?"), ok = L("Удалить")) {
  closeConfirm();
  const pop = el("div", "popup tmenu confirm-pop");
  pop.id = "confirmPop";
  pop.setAttribute("role", "dialog");
  const yes = el("button", "nav danger", ok);
  const no = el("button", "nav", L("Отмена"));
  yes.addEventListener("click", (ev) => {
    ev.stopPropagation();
    closeConfirm();
    fn();
  });
  no.addEventListener("click", (ev) => {
    ev.stopPropagation();
    closeConfirm();
  });
  const row = el("div", "row confirm-actions");
  row.append(yes, no);
  pop.append(el("div", "confirm-text", label), row);
  pop.addEventListener("click", (ev) => ev.stopPropagation());
  document.body.append(pop);
  // под кнопкой, не вылезая за окно (если внизу нет места — над ней)
  const r = btn.getBoundingClientRect();
  const w = pop.offsetWidth, h = pop.offsetHeight;
  const left = Math.min(Math.max(8, r.right - w), window.innerWidth - w - 8);
  const top = r.bottom + 6 + h > window.innerHeight - 8 ? r.top - h - 6 : r.bottom + 6;
  pop.style.left = left + "px";
  pop.style.top = Math.max(8, top) + "px";
  btn.classList.add("armed");
  pop._btn = btn;
  no.focus();
}

function closeConfirm() {
  const pop = $("#confirmPop");
  if (!pop) return;
  if (pop._btn) pop._btn.classList.remove("armed");
  pop.remove();
}

function emptyState(icon, text, action) {
  const d = el("div", "empty");
  d.append(Icons.icon(icon), el("div", null, text));
  if (action) d.append(action);
  return d;
}

function iconButton(icon, title, onClick) {
  const b = el("button", "icon-btn");
  b.title = title;
  b.append(Icons.icon(icon));
  b.addEventListener("click", (ev) => {
    ev.stopPropagation();
    onClick(b, ev);
  });
  return b;
}

/** Кнопка в стиле Whisper (.nav): cls "primary" — главная (.active), "danger" — опасная; остальное — как есть. */
function button(icon, text, cls, onClick) {
  const extra = String(cls || "").split(/\s+/).filter(Boolean).map((c) => (c === "primary" ? "active" : c));
  const b = el("button", ["nav", "btn", ...extra].join(" "));
  if (icon) b.append(Icons.icon(icon));
  b.append(el("span", null, text));
  b.addEventListener("click", (ev) => {
    ev.stopPropagation();
    onClick(b, ev);
  });
  return b;
}

// ───────────────────────── треки: файлы, сохранённые, найденные ─────────────────────────

function localTrack(t) {
  const file = t.path.split("/").pop();
  return {
    key: t.path,
    path: t.path,
    file,
    artist: t.artist || L("Неизвестный исполнитель"),
    noArtist: !t.artist,
    title: t.title || file,
    duration: t.duration,
    added: t.added,
    format: t.format,
    bitrate: t.bitrate,
    cover: t.has_cover ? VKM + "cover?path=" + encodeURIComponent(t.path) : null,
  };
}

/** Трек сервиса: из поиска (vk.Track / sc.Track) или сохранённый (saved.Saved). */
function svcTrack(service, t) {
  const id = String(t.id);
  return {
    key: service + ":" + id,
    service,
    id,
    reload: service === "vk" ? t.reload || null : null,
    url: service === "sc" ? t.url || null : null,
    artist: t.artist || L("Неизвестный исполнитель"),
    title: t.title || L("Без названия"),
    duration: t.duration || 0,
    cover: t.cover || null,
    added: t.added || 0,
    snippet: !!t.snippet,
    sync: !!t.sync,
    // ваша аудиозапись ВК — можно удалить в самом ВК
    deletable: service === "vk" && !!t.sync && !!t.del,
    // трек из микса или плейлиста ВК (API): играет по ссылке из API (audio.getById); в библиотеку добавляется
    api: service === "vk" && !!t.api,
    // ВК без reload и не из API и заблокированный SoundCloud слушать нельзя
    unavailable: service === "vk" ? !t.reload && !t.api : !!t.blocked,
  };
}

/** Нормализованная строка для сравнения: регистр, ё/е, знаки препинания. */
const normText = (x) => String(x || "").toLowerCase().replace(/ё/g, "е").replace(/[^\p{L}\p{N}]+/gu, " ").trim();
let libIndex = { saved: null, local: null, byKey: new Map(), byText: new Map() };

/**
 * Трек уже в библиотеке? Тот же ключ — или тот же исполнитель и название с длительностью ±3 с: трек из плейлиста
 * ВК — оригинал (owner_id автора), а в «Моих треках» лежит его копия с вашим owner_id, ключи не совпадают.
 */
function libraryMatch(t) {
  if (!t) return null;
  if (t.path) return t;
  if (libIndex.saved !== S.saved || libIndex.local !== S.local) {
    libIndex = { saved: S.saved, local: S.local, byKey: new Map(), byText: new Map() };
    for (const x of S.local.concat(S.saved)) {
      libIndex.byKey.set(x.key, x);
      if (x.noArtist) continue;
      const k = normText(x.artist) + "\n" + normText(x.title);
      if (!libIndex.byText.has(k)) libIndex.byText.set(k, []);
      libIndex.byText.get(k).push(x);
    }
  }
  const exact = libIndex.byKey.get(t.key);
  if (exact) return exact;
  const same = libIndex.byText.get(normText(t.artist) + "\n" + normText(t.title)) || [];
  return same.find((x) => !x.duration || !t.duration || Math.abs(x.duration - t.duration) <= 3) || null;
}

function allTracks() {
  return S.local.concat(S.saved);
}

/** Склеить страницы без повторов. */
function concatUnique(a, b) {
  const seen = new Set(a.map((t) => t.key));
  return a.concat(b.filter((t) => !seen.has(t.key) && seen.add(t.key)));
}

function matches(t, q) {
  return (t.artist + " " + t.title + " " + (t.file || "")).toLowerCase().includes(q);
}

async function saveTrack(t) {
  try {
    const s = await invoke("saved_add", {
      track: {
        service: t.service, id: t.id, reload: t.reload, url: t.url, api: t.api && !t.reload,
        artist: t.artist, title: t.title, duration: t.duration, cover: t.cover, added: 0,
      },
    });
    S.saved = S.saved.filter((x) => x.key !== t.key).concat([svcTrack(s.service, s)]);
    await loadOrder();
    toast(L("Добавлено в библиотеку: {0}", t.title));
    const keep = scroller().scrollTop;
    rerender();
    scroller().scrollTop = keep;
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

/** Удалить трек из «Моих треков» ВК — и в самом ВК, и из библиотеки. */
async function deleteFromVk(t) {
  try {
    await invoke("vk_delete", { id: t.id });
    S.saved = S.saved.filter((x) => x.key !== t.key);
    toast(L("Удалено из ВКонтакте: {0}", t.title));
    rerender();
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

/** Скачать трек библиотеки в папку загрузок (трек сервиса — через кэш). */
async function downloadTrack(t, b) {
  if (S.saving.has(t.key)) return;
  S.saving.add(t.key);
  b.classList.add("busy");
  b.disabled = true;
  try {
    const path = await invoke("track_download", { key: t.key });
    toast(L("Сохранено: {0}", tilde(path)));
    S.cacheInfo = null;
  } catch (e) {
    toast(errMsg(e), "error");
  }
  S.saving.delete(t.key);
  b.classList.remove("busy");
  b.disabled = false;
}

function downloadButton(t) {
  const b = iconButton("download", L("Скачать в {0}", tilde(S.downloadDir)), (btn) => downloadTrack(t, btn));
  if (S.saving.has(t.key)) {
    b.classList.add("busy");
    b.disabled = true;
  }
  return b;
}

async function unsaveTrack(t) {
  try {
    await invoke("saved_remove", { service: t.service, id: t.id });
    S.saved = S.saved.filter((x) => x.key !== t.key);
    rerender();
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

// ───────────────────────── синхронизация с аккаунтами ─────────────────────────

function renderSyncStatus() {
  const s = $("#syncStatus");
  if (!s) return;
  const names = [...S.syncing].map((id) => SERVICES[id].name);
  s.textContent = names.length ? L("Синхронизация: {0}…", names.join(", ")) : "";
}

/** «Мои треки» ВК / лайки SoundCloud → библиотека. Без входа сервис молча пропускается. */
async function syncService(id, quiet) {
  if (S.syncing.has(id)) return;
  S.syncing.add(id);
  renderSyncStatus();
  try {
    const list = await invoke("saved_sync", { service: id });
    S.saved = list.map((s) => svcTrack(s.service, s));
    syncNpAdd();
    await loadOrder();
    if (S.page === "library") renderLibraryList();
  } catch (e) {
    const msg = errText(e);
    if (!(quiet && /не вошли/i.test(msg))) toast(L("Синхронизация {0}: {1}", SERVICES[id].name, L(msg)), "error");
  }
  S.syncing.delete(id);
  renderSyncStatus();
}

function syncAll() {
  for (const id of Object.keys(SERVICES)) syncService(id, true);
}

function quality(t) {
  const parts = [];
  if (t.format) parts.push(t.format);
  if (t.bitrate) parts.push(L("{0} кбит/с", t.bitrate));
  return parts.join(" · ");
}

/**
 * Список треков. Для файлов — формат и «удалить файл»; для треков сервисов — значок сервиса,
 * «в библиотеку» / «убрать из библиотеки» и (SoundCloud) «скачать через yt-dlp».
 */
/** Значок трека для строки списка: обложка или заглушка (у Whisper — img прямо в li). */
/**
 * Заглушка обложки: трек без обложки (или она не загрузилась) — нота на приглушённом фоне, а не пустое место.
 * Своя постоянная SVG (без данных), цвета полупрозрачные — ложится на фон любой темы.
 */
const NO_COVER = "data:image/svg+xml," + encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect width="48" height="48" fill="rgba(255,255,255,0.07)"/>' +
  '<g fill="none" stroke="rgba(255,255,255,0.45)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M20 31V15l12-2.5v15.5"/><circle cx="17" cy="31" r="3"/><circle cx="29" cy="28" r="3"/></g></svg>');

/** Обложка или заглушка; не загрузилась — заглушка. */
function setCover(img, src) {
  img.onerror = src ? () => ((img.onerror = null), (img.src = NO_COVER)) : null;
  img.src = src || NO_COVER;
  img.classList.toggle("nocover", !src);
}

function trackImg(src) {
  const img = el("img");
  img.alt = "";
  img.loading = "lazy";
  setCover(img, src);
  return img;
}

/** «Сердце» — трек в библиотеке (у Whisper — лайк). У трека сервиса не из библиотеки — добавить. */
function likeButton(t) {
  const b = el("button", "likebtn");
  const lib = libraryMatch(t);
  setHeart(b, !!lib);
  b.title = lib ? L("В библиотеке") : L("В библиотеку");
  b.addEventListener("click", (ev) => {
    ev.stopPropagation();
    if (!libraryMatch(t)) saveTrack(t);
    else toast(L("Трек уже в библиотеке"));
  });
  return b;
}
function setHeart(b, on) {
  b.classList.toggle("on", on);
  b.replaceChildren(Icons.icon(on ? "heartfill" : "heart"));
}

/**
 * Список треков — как у Whisper: ul.list > li > img, div > .t / .a, кнопки. Свои элементы VK Music — после
 * названия: качество / сервис (.lq), действия (.lacts), длительность (.ldur).
 */
function renderTracks(items, reorder, opts = {}) {
  const wrap = el("ul", "list tracks");
  items.forEach((t) => {
    const row = el("li", "track");
    row.dataset.key = t.key;
    if (S.current && S.current.key === t.key) row.classList.add("playing");
    if (t.unavailable) row.classList.add("unavailable");

    const body = el("div");
    const title = el("span", "t", t.title);
    if (t.snippet) {
      const b = el("span", "t-badge", L("30 с"));
      b.title = L("SoundCloud даёт только фрагмент (трек Go+)");
      title.append(b);
    }
    body.append(title, el("span", "a", t.artist));

    const q = el("span", "lq");
    const actions = el("span", "lacts");
    if (t.path) {
      q.textContent = quality(t);
      q.title = t.file;
      actions.append(
        downloadButton(t),
        iconButton("trash", L("Удалить файл"), (b) =>
          confirmClick(b, async () => {
            try {
              await invoke("local_delete", { path: t.path });
              S.local = S.local.filter((x) => x.key !== t.key);
              rerender();
            } catch (e) {
              toast(errMsg(e), "error");
            }
          }, L("Удалить файл с диска?"))
        )
      );
    } else {
      const svc = SERVICES[t.service];
      q.append(Icons.icon(svc.icon));
      q.classList.add("lsvc");
      q.title = svc.name;
      const saved = S.saved.find((x) => x.key === t.key);
      // открыт ваш плейлист — «убрать из плейлиста», иначе — «в плейлист»
      if (opts.playlist) actions.append(removeFromPlaylistButton(opts.playlist, t));
      else if (canAddToPlaylist(t)) actions.append(iconButton("listadd", L("В плейлист"), (b) => addToPlaylistMenu(b, t)));
      // трек библиотеки — в папку загрузок (через кэш); найденный трек SoundCloud — в папку музыки через yt-dlp
      if (saved) actions.append(downloadButton(t));
      else if (t.service === "sc" && t.url) {
        actions.append(iconButton("download", L("Скачать файл через yt-dlp"), () => startDownload(t.url, `${t.artist} — ${t.title}`)));
      }
      if (saved && saved.deletable) {
        actions.append(
          iconButton("trash", L("Удалить из ВКонтакте"), (b) =>
            confirmClick(b, () => deleteFromVk(saved), L("Удалить трек из ваших аудиозаписей ВКонтакте? Он удалится и в самом ВК."), L("Удалить"))
          )
        );
        q.title += L(" · из вашего аккаунта");
      } else if (saved && saved.sync) {
        q.title += L(" · из вашего аккаунта");
      } else if (saved) {
        actions.append(
          iconButton("trash", L("Убрать из библиотеки"), (b) => confirmClick(b, () => unsaveTrack(t), L("Убрать трек из библиотеки?"), L("Убрать")))
        );
      } else if (libraryMatch(t)) {
        q.title += L(" · уже в библиотеке");
      }
    }

    row.append(trackImg(t.cover), body, q, actions, el("span", "ldur", fmt(t.duration)));
    // «сердце» (в библиотеке) и «⋯» — как у Whisper, в конце строки
    if (t.service && !t.unavailable) row.append(likeButton(t));
    if (!t.unavailable) {
      const mb = el("button", "morebtn");
      mb.title = L("Ещё");
      mb.append(Icons.icon("dots"));
      mb.addEventListener("click", (ev) => {
        ev.stopPropagation();
        openTrackMenu(t, mb);
      });
      row.append(mb);
    }
    row.addEventListener("click", () => {
      // только что перетаскивали — это не нажатие
      if (dragJustEnded) return;
      if (t.unavailable) return toast(L("{0} не даёт слушать этот трек", SERVICES[t.service].name), "error");
      if (S.current && S.current.key === t.key) togglePlay();
      else playList(items.filter((x) => !x.unavailable), items.filter((x) => !x.unavailable).indexOf(t));
    });
    if (reorder) row.addEventListener("pointerdown", (ev) => startDrag(ev, row, wrap));
    wrap.append(row);
  });
  return wrap;
}

function markPlaying() {
  document.querySelectorAll(".list li.track").forEach((r) => r.classList.toggle("playing", !!S.current && r.dataset.key === S.current.key));
}

// ───────────────────────── навигация ─────────────────────────
//
// Страницы — в средней панели (.main), как у Whisper: шапка .head (#title, #sub, кнопки справа), под ней #chips и
// содержимое. «VK Микс» — своя страница (#wavePage), настройки — постоянная #settingsPage (там живут карточки
// модулей ядра), остальное рисуется в #content.

/** Шапка страницы: заголовок, строка под ним (текст или узлы), картинка (обложка плейлиста). */
function setHead(title, sub, img) {
  $("#title").textContent = title || "";
  const s = $("#sub");
  if (sub == null || sub === "") {
    s.hidden = true;
    s.replaceChildren();
  } else {
    s.hidden = false;
    s.replaceChildren(...(Array.isArray(sub) ? sub : [sub]));
  }
  const h = $("#headImg");
  if (img) {
    h.src = img;
    h.hidden = false;
  } else {
    h.hidden = true;
    h.removeAttribute("src");
  }
}
/** Кнопки страницы справа в шапке (заменяют прежние: страница перерисовывается целиком). */
const headBtns = (...b) => $("#headBtns").replaceChildren(...b);
/** Строка чипов под шапкой (сервисы поиска, вкладки). */
function setChips(items) {
  const c = $("#chips");
  c.replaceChildren(...items);
  c.hidden = !items.length;
}
function chip(text, active, onClick) {
  const b = el("button", "chip" + (active ? " active" : ""), text);
  b.addEventListener("click", onClick);
  return b;
}

function go(page, keepPopups) {
  // повторное нажатие пункта меню внутри открытого плейлиста — обратно к списку
  if (page === S.page && S.vkPl && S.vkPl.from === page) S.vkPl = null;
  if (page !== "search" && page !== "store") S.lastPage = page;
  S.page = page;
  if (page === "stats") S.statsData = null;
  if (page === "settings" && S.setTab === "out") S.cacheInfo = null; // кэш растёт, пока играет музыка
  const navPage = page === "store" ? "settings" : page;
  document.querySelectorAll(".side .nav[data-page]").forEach((n) => n.classList.toggle("active", n.dataset.page === navPage));
  if (!keepPopups) closePopups();
  scroller().scrollTop = 0;
  scroller().classList.toggle("wavemode", page === "wave");
  $("#wavePage").hidden = page !== "wave";
  $("#waveInfo").hidden = page !== "wave";
  $("#settingsPage").hidden = page !== "settings";
  $("#content").hidden = page === "wave" || page === "settings";
  rerender();
  // новая страница проявляется (анимация перезапускается)
  const c = page === "settings" ? $("#settingsPage") : $("#content");
  c.classList.remove("page-in");
  void c.offsetWidth;
  c.classList.add("page-in");
}

/** Стрелка «назад» в шапке: из открытого плейлиста, из поиска, из магазина тем. */
function syncBack() {
  const b = $("#back");
  const store = S.page === "store", search = S.page === "search";
  b.hidden = !(store || search || (S.vkPl && S.vkPl.from === S.page));
  b.title = store ? L("Назад: {0}", L("Настройки")) : search ? L("Закрыть поиск") : L("Назад: {0}", L(PAGES[S.page] || ""));
}

$("#back").addEventListener("click", () => {
  if (S.page === "store") {
    S.setTab = "look";
    return go("settings");
  }
  if (S.page === "search") {
    $("#search").value = "";
    endSearch();
    return go(S.lastPage || "library");
  }
  S.vkPl = null;
  scroller().scrollTop = 0;
  rerender();
});

/** Перерисовать текущую страницу (после изменений данных). */
function rerender() {
  const c = $("#content");
  c.className = "content page-" + S.page + (S.page === "stats" ? " stats" : S.page === "services" ? " settings svcpage" : "");
  $("#headBtns").replaceChildren();
  setChips([]);
  setHead(L(PAGES[S.page] || ""));
  syncBack();
  syncNpAdd();
  if (S.page === "library") renderLibrary();
  else if (S.page === "wave") renderWave();
  else if (S.page === "stats") renderStatsPage();
  else if (S.page === "vk") renderVk();
  else if (S.page === "search") renderSearch();
  else if (S.page === "services") renderServices();
  else if (S.page === "settings") renderSettings();
  else if (S.page === "store") renderStore();
}

// всплывающие окошки (подтверждение, выбор плейлиста)
function closePopups() {
  document.querySelectorAll(".popup").forEach((p) => (p.hidden = true));
  closeConfirm();
}

document.addEventListener("click", (ev) => {
  if (!ev.target.closest(".popup")) closePopups();
});
// не перетаскивать выделенный текст (и не вставлять его перетаскиванием): файлы .wmt принимает сам Tauri
document.addEventListener("dragstart", (ev) => ev.preventDefault());
document.addEventListener("dragover", (ev) => ev.preventDefault());
document.addEventListener("drop", (ev) => ev.preventDefault());
// окошко подтверждения не должно «уезжать» от своей кнопки
document.addEventListener("scroll", closeConfirm, true);
window.addEventListener("resize", closeConfirm);

// ───────────────────────── библиотека: файлы + сохранённые ─────────────────────────

/** Сохранённый порядок библиотеки (Rust ведёт его в order.json: новые треки — первыми). */
// ───────────────────────── перетаскивание треков: порядок библиотеки ─────────────────────────

/** Сдвиг мыши, после которого нажатие становится перетаскиванием. */
const DRAG_START = 6;
/** Длительность «расступания» соседних строк, мс. */
const DRAG_SHIFT_MS = 180;
let dragJustEnded = false;

/**
 * Перетаскивание строки библиотеки мышью (не HTML5 drag-and-drop: его на Linux перехватывает Tauri ради
 * перетаскивания .wmt). Строка едет за курсором, соседние плавно расступаются (FLIP), у краёв список
 * прокручивается. Отпустили — Rust ставит трек перед следующим (library_move).
 */
function startDrag(ev, row, wrap) {
  if (ev.button !== 0 || ev.target.closest("button, input, a, .lacts")) return;
  const box = scroller();
  const startY = ev.clientY;
  let dragging = false, grab = 0, lastY = ev.clientY, raf = 0;

  const naturalTop = () => {
    const prev = row.style.transform;
    row.style.transform = "";
    const top = row.getBoundingClientRect().top;
    row.style.transform = prev;
    return top;
  };
  const follow = () => (row.style.transform = `translateY(${lastY - grab - naturalTop()}px)`);

  // соседи плавно встают на новые места: запомнили — переставили — анимировали разницу
  const reorderTo = (before) => {
    if (before === row.nextSibling || before === row) return;
    const others = [...wrap.children].filter((x) => x !== row);
    const was = new Map(others.map((x) => [x, x.getBoundingClientRect().top]));
    wrap.insertBefore(row, before);
    for (const x of others) {
      const d = was.get(x) - x.getBoundingClientRect().top;
      if (!d) continue;
      x.animate([{ transform: `translateY(${d}px)` }, { transform: "translateY(0)" }], { duration: DRAG_SHIFT_MS, easing: "cubic-bezier(.22, .8, .24, 1)" });
    }
  };

  const update = () => {
    follow();
    // куда встать: перед первой строкой, чья середина ниже курсора
    const others = [...wrap.children].filter((x) => x !== row);
    const target = others.find((x) => {
      const r = x.getBoundingClientRect();
      return lastY < r.top + r.height / 2;
    });
    reorderTo(target || null);
    follow();
  };

  // у краёв списка — прокрутка
  const tick = () => {
    const r = box.getBoundingClientRect();
    const edge = 48;
    const v = lastY < r.top + edge ? -(r.top + edge - lastY) / 3 : lastY > r.bottom - edge ? (lastY - (r.bottom - edge)) / 3 : 0;
    if (v) {
      box.scrollTop += Math.max(-24, Math.min(24, v));
      update();
    }
    raf = requestAnimationFrame(tick);
  };

  const move = (e) => {
    lastY = e.clientY;
    if (!dragging) {
      if (Math.abs(lastY - startY) < DRAG_START) return;
      dragging = true;
      grab = startY - row.getBoundingClientRect().top;
      row.classList.add("dragging");
      document.body.classList.add("drag-sorting");
      row.setPointerCapture(e.pointerId);
      raf = requestAnimationFrame(tick);
    }
    update();
  };

  const up = async () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", up);
    if (!dragging) return;
    cancelAnimationFrame(raf);
    // клик, который придёт следом за отпусканием, — не «включить трек»
    dragJustEnded = true;
    setTimeout(() => (dragJustEnded = false), 0);
    // строка плавно садится на место
    const off = row.style.transform;
    row.style.transform = "";
    row.animate([{ transform: off || "translateY(0)" }, { transform: "translateY(0)" }], { duration: DRAG_SHIFT_MS, easing: "cubic-bezier(.22, .8, .24, 1)" });
    row.classList.remove("dragging");
    document.body.classList.remove("drag-sorting");
    const next = row.nextElementSibling;
    const key = row.dataset.key, before = next ? next.dataset.key : null;
    try {
      await invoke("library_move", { key, before });
      await loadOrder();
    } catch (e) {
      toast(errMsg(e), "error");
      await loadOrder();
      renderLibraryList();
    }
  };

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", up);
}

async function loadOrder() {
  try {
    const keys = await invoke("library_order");
    S.order = new Map(keys.map((k, i) => [k, i]));
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

async function loadLibrary() {
  try {
    const [local, saved] = await Promise.all([invoke("local_list"), invoke("saved_list")]);
    S.local = local.map(localTrack);
    S.saved = saved.map((s) => svcTrack(s.service, s));
    syncNpAdd();
    // после local_list: он заносит новые файлы в порядок
    await loadOrder();
  } catch (e) {
    toast(errMsg(e), "error");
  }
  S.localLoaded = true;
  if (S.page === "library") renderLibrary();
}

let reloadTimer = null;
function reloadLocalSoon() {
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(loadLibrary, 800);
}

/** Библиотека в сохранённом порядке; трека ещё нет в порядке — он сверху (новые, ещё не учтённые). */
function visibleTracks() {
  const pos = (t) => (S.order.has(t.key) ? S.order.get(t.key) : -1);
  return allTracks().sort((a, b) => pos(a) - pos(b) || b.added - a.added);
}

function renderLibrary() {
  // открыт плейлист из раздела «Плейлисты»
  if (S.vkPl && S.vkPl.from === "library") return renderVkPlaylist();
  const c = $("#content");
  c.replaceChildren();
  headBtns(
    button("refresh", L("Обновить"), "pin", () => {
      S.localLoaded = false;
      if (S.myPl) S.myPl.fresh = false; // сохранённый список остаётся на экране, пока не придёт свежий
      renderLibrary();
      syncAll();
    })
  );
  libSub();
  const dl = el("div", "downloads");
  dl.id = "downloads";
  c.append(dl);
  renderDownloads();
  // ваши плейлисты ВК — над треками
  const pls = el("section", "similar libpl");
  pls.id = "libPlaylists";
  c.append(pls);
  renderMyPlaylists();
  // треки и рядом карточка «Сейчас играет» (как «Последний релиз» у Whisper)
  const cols = el("div", "cols");
  const list = el("div", "libcol");
  list.id = "libList";
  const now = el("aside", "lastAlbum nowcard");
  now.id = "lastAlbum";
  now.hidden = true;
  now.title = L("Текст и обложка на весь экран");
  now.addEventListener("click", () => openFull("lyrics"));
  cols.append(list, now);
  c.append(cols);
  renderNowCard();
  if (!S.localLoaded) {
    list.append(el("div", "spinner"));
    loadLibrary();
    return;
  }
  renderLibraryList();
}

/** Под заголовком библиотеки: сколько треков, сколько звучат, идёт ли синхронизация. */
function libSub() {
  if (S.page !== "library" || (S.vkPl && S.vkPl.from === "library")) return;
  const items = S.localLoaded ? allTracks() : [];
  const total = items.reduce((a, t) => a + (t.duration || 0), 0);
  const parts = [];
  if (items.length) parts.push(el("span", null, `${items.length} ${plural(items.length, "трек", "трека", "треков")} · ${fmtTotal(total)}`));
  const st = el("span", "sync-status");
  st.id = "syncStatus";
  parts.push(st);
  setHead(L(PAGES.library), parts);
  renderSyncStatus();
}

function renderLibraryList() {
  const box = $("#libList");
  if (!box) return;
  const keep = scroller().scrollTop;
  box.replaceChildren();
  libSub();
  const all = allTracks();
  if (!all.length) {
    box.append(
      emptyState(
        "note",
        L("Библиотека пуста. Войдите в сервисы в настройках — ваши треки ВКонтакте и лайки SoundCloud появятся здесь сами. " +
          "Ещё можно найти трек в строке поиска сверху и добавить его кнопкой «+» или вставить ссылку — файл скачается в {0}.", S.musicDir)
      )
    );
    return;
  }
  box.append(renderTracks(visibleTracks(), true));
  scroller().scrollTop = keep;
}

/** Карточка «Сейчас играет» рядом со списком: обложка, название, сколько слушали; нажатие — текст на весь экран. */
function renderNowCard() {
  const box = $("#lastAlbum");
  if (!box) return;
  const t = S.current;
  box.hidden = !t;
  if (!t) return;
  if (box.dataset.key !== t.key) {
    box.dataset.key = t.key;
    const meta = el("div", "nowstat");
    meta.id = "nowStat";
    box.replaceChildren(trackImg(t.cover), el("small", null, L("Сейчас играет")), el("b", null, t.title), el("span", null, t.artist), meta);
  }
  renderNpStats();
}

// ───────────────────────── общий поиск: строка в боковой панели, результаты — страницей ─────────────────────────
//
// Ищет сам, пока вы печатаете (с небольшой задержкой). Результаты — страница «Поиск» в средней панели (как у Whisper),
// где искать — чипы под шапкой. Пока поиск идёт (строка не пуста), результаты каждого сервиса по каждому запросу
// кэшируются: вернулись к прежнему запросу или снова включили сервис — результаты появляются сразу, без запросов.

const SEARCH_DELAY = 450;
const SEARCH_MIN = 2;

const isUrl = (s) => /^https?:\/\/\S+$/i.test(s);
const isScUrl = (s) => /^https:\/\/(www\.|m\.|on\.)?soundcloud\.com\/\S+/i.test(s);

/** Кэш результатов на время поиска: "vk|text|запрос" → секция. Чистится, когда строку очистили. */
const searchCache = new Map();

/** Где искать — чипы сервисов под шапкой страницы поиска. */
function searchChips() {
  if (!S.sr || S.sr.kind !== "text") return setChips([]);
  setChips(Object.entries(SERVICES).map(([id, svc]) => {
    const b = chip(svc.name, S.search.includes(id), () => {
      const on = !S.search.includes(id);
      S.search = Object.keys(SERVICES).filter((k) => (k === id ? on : S.search.includes(k)));
      invoke("save_prefs", { search: S.search }).catch((e) => toast(errMsg(e), "error"));
      renderSearch();
    });
    b.prepend(Icons.icon(svc.icon));
    return b;
  }));
}

// ── история поиска (как у Whisper Yandex): пустое поле в фокусе — список прошлых запросов. Попадают запросы,
// подтверждённые Enter, и те, по которым что-то включили из результатов. Только в localStorage этого компьютера.
const SEARCH_HIST_KEY = "vkm_search_hist", SEARCH_HIST_MAX = 30;
const searchHist = (() => {
  try {
    const v = JSON.parse(localStorage.getItem(SEARCH_HIST_KEY) || "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string" && x.trim()).map((x) => x.trim().slice(0, 200)).slice(0, SEARCH_HIST_MAX) : [];
  } catch {
    return [];
  }
})();
function saveSearchHist() {
  try { localStorage.setItem(SEARCH_HIST_KEY, JSON.stringify(searchHist)); } catch {}
}
function pushSearchHist(q) {
  q = String(q || "").trim().slice(0, 200);
  if (!q || isUrl(q)) return;
  const i = searchHist.findIndex((x) => x.toLowerCase() === q.toLowerCase());
  if (i >= 0) searchHist.splice(i, 1);
  searchHist.unshift(q);
  if (searchHist.length > SEARCH_HIST_MAX) searchHist.length = SEARCH_HIST_MAX;
  saveSearchHist();
}
/** Пустое поле поиска — страница истории. */
function showSearchHistory() {
  S.sr = { kind: "history", q: "", lib: [] };
  if (S.page !== "search") go("search");
  else rerender();
}
function renderSearchHist(c) {
  if (!searchHist.length) {
    c.append(el("p", "status", L("История поиска пуста.")));
    return;
  }
  const list = el("ul", "list");
  const CLOCK = '<svg viewBox="0 0 24 24" width="18" height="18"><circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M12 7v5.2l3.4 2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>';
  for (const q of searchHist) {
    const li = el("li", "histitem");
    const ico = el("span", "hico");
    ico.innerHTML = CLOCK; // свой постоянный значок, данных в разметке нет
    const del = el("button", "hdel", "✕");
    del.title = L("Удалить из истории");
    del.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const i = searchHist.indexOf(q);
      if (i >= 0) searchHist.splice(i, 1);
      saveSearchHist();
      rerender();
    });
    li.append(ico, el("span", "t", q), del);
    li.addEventListener("click", () => {
      $("#search").value = q;
      pushSearchHist(q);
      runSearch(q, true);
    });
    list.append(li);
  }
  const clr = el("li", "histclear");
  clr.append(button(null, L("Очистить историю"), "nav", () => {
    searchHist.length = 0;
    saveSearchHist();
    rerender();
  }));
  list.append(clr);
  c.append(list);
}
$("#search").addEventListener("focus", () => {
  if (!$("#search").value.trim()) showSearchHistory();
});
// включили трек из результатов — запрос в историю
$("#content").addEventListener("click", (ev) => {
  if (S.page === "search" && S.sr && S.sr.kind === "text" && ev.target.closest("ul.list > li")) pushSearchHist(S.sr.q);
}, true);

let searchTimer = null;
$("#search").addEventListener("input", () => {
  clearTimeout(searchTimer);
  const q = $("#search").value.trim();
  if (!q) {
    endSearch();
    // поле в фокусе — история, иначе обратно
    if (document.activeElement === $("#search")) showSearchHistory();
    else if (S.page === "search") go(S.lastPage || "library");
    return;
  }
  searchTimer = setTimeout(() => runSearch(q), SEARCH_DELAY);
});

$("#search").addEventListener("keydown", (ev) => {
  if (ev.key === "Enter") {
    clearTimeout(searchTimer);
    const q = $("#search").value.trim();
    if (!q) return;
    // Enter по ссылке (не SoundCloud) — скачать через yt-dlp
    if (isUrl(q) && !isScUrl(q)) return downloadLink(q);
    pushSearchHist(q);
    runSearch(q, true);
  } else if (ev.key === "Escape") {
    ev.currentTarget.value = "";
    endSearch();
    ev.currentTarget.blur();
    if (S.page === "search") go(S.lastPage || "library");
  }
});

/** Строку очистили — поиск окончен: кэш результатов забывается. */
function endSearch() {
  clearTimeout(searchTimer);
  S.sr = null;
  searchCache.clear();
}

function downloadLink(q) {
  startDownload(q, q);
  $("#search").value = "";
  endSearch();
  toast(L("Скачиваю по ссылке — прогресс в библиотеке"));
  go("library");
}

function newSection() {
  return { tracks: [], busy: true, error: null, login: false, next: null };
}

/** Секция сервиса для текущего запроса: из кэша или новым запросом. */
function section(id) {
  const sr = S.sr;
  const key = `${id}|${sr.kind}|${sr.q}`;
  let sec = searchCache.get(key);
  if (!sec) {
    sec = newSection();
    searchCache.set(key, sec);
    fetchSection(id, sec, false);
  }
  return sec;
}

function fetchSection(id, sec, more) {
  const sr = S.sr;
  sec.busy = true;
  let call;
  if (id === "vk") call = invoke("vk_search", { query: sr.q, more: more ? sec.next : null }).then((p) => [p.tracks, p.more]);
  else if (sr.kind === "url") call = invoke("sc_resolve", { url: sr.q }).then((list) => [list, null]);
  else call = invoke("sc_search", { query: sr.q, offset: more ? sec.next : 0 }).then((p) => [p.tracks, p.next_offset]);
  const key = `${id}|${sr.kind}|${sr.q}`;
  call
    .then(([list, next]) => {
      const tracks = list.map((t) => svcTrack(id, t));
      Object.assign(sec, { tracks: more ? concatUnique(sec.tracks, tracks) : tracks, next, busy: false, error: null, login: false });
    })
    .catch((e) => {
      const msg = errText(e);
      Object.assign(sec, { busy: false, error: msg, login: /не вошли/i.test(msg) });
      // ошибки не кэшируем — следующий раз спросим снова (например, после входа)
      if (!more) searchCache.delete(key);
    })
    .finally(() => S.page === "search" && renderSearch());
}

/** Поиск по строке: текст — в отмеченных сервисах; ссылка SoundCloud — открыть трек/плейлист. */
function runSearch(q, force) {
  if (!force && q.length < SEARCH_MIN && !isUrl(q)) return;
  const kind = isScUrl(q) ? "url" : isUrl(q) ? "link" : "text";
  const lq = q.toLowerCase();
  S.sr = {
    q,
    kind,
    // уже есть в библиотеке — отдельно выше
    lib: kind === "url"
      ? S.saved.filter((t) => t.url && t.url.replace(/\/$/, "") === q.replace(/[?#].*$/, "").replace(/\/$/, ""))
      : kind === "text" ? allTracks().filter((t) => matches(t, lq)) : [],
  };
  if (S.page !== "search") go("search");
  else {
    scroller().scrollTop = 0;
    rerender();
  }
}

function renderSection(id, sec) {
  const box = el("section", "similar result-section");
  const svc = SERVICES[id];
  const head = el("h3", "section-title with-icon");
  head.append(Icons.icon(svc.icon), el("span", null, svc.name));
  box.append(head);
  if (sec.tracks.length) box.append(renderTracks(sec.tracks));
  if (sec.busy) {
    box.append(el("div", "spinner small"));
  } else if (sec.login) {
    const row = el("p", "status drop-note");
    row.append(el("span", null, L("Чтобы искать во {0}, войдите в аккаунт.", svc.name) + " "), button("link", L("Войти"), "", () => go("services")));
    box.append(row);
  } else if (sec.error) {
    const row = el("p", "status drop-note err");
    row.append(el("span", null, L(sec.error) + " "), button(null, L("Повторить"), "", () => fetchSection(id, sec, sec.tracks.length > 0)));
    box.append(row);
  } else if (sec.next != null && sec.next !== false) {
    box.append(button(null, L("Показать ещё"), "more", () => {
      fetchSection(id, sec, true);
      renderSearch();
    }));
  } else if (!sec.tracks.length) {
    box.append(el("p", "status", L("Ничего не нашлось.")));
  }
  return box;
}

function renderSearch() {
  const c = $("#content");
  const sr = S.sr;
  if (!sr) return go(S.lastPage || "library");
  const keep = scroller().scrollTop;
  c.replaceChildren();
  if (sr.kind === "history") {
    setHead(L("Поиск"), "");
    syncBack();
    setChips([]);
    return renderSearchHist(c);
  }
  setHead(L("Поиск"), `«${sr.q}»`);
  syncBack();
  searchChips();
  if (sr.kind === "link") {
    c.append(el("p", "status", L("Ссылка скачается файлом в лучшем качестве; плейлист или альбом — целиком.")));
    c.append(button("download", L("Скачать по ссылке через yt-dlp (Enter)"), "primary", () => downloadLink(sr.q)));
    return;
  }
  if (sr.lib.length) {
    const box = el("section", "similar result-section in-library");
    box.append(el("h3", "section-title", L("Уже в библиотеке")), renderTracks(sr.lib));
    c.append(box);
  }
  const ids = sr.kind === "url" ? ["sc"] : S.search.filter((s) => SERVICES[s]);
  for (const id of ids) c.append(renderSection(id, section(id)));
  if (sr.kind === "text") {
    if (!ids.length) c.append(el("p", "status", L("Поиск в сервисах выключен — включите ВКонтакте или SoundCloud кнопкой над результатами.")));
    // запасной путь: первый результат YouTube через yt-dlp
    const yt = button("download", L("Скачать «{0}» с YouTube (yt-dlp, первый результат)", sr.q), "yt-fallback", () => startDownload(sr.q, L("Поиск: ") + sr.q));
    c.append(yt);
  }
  scroller().scrollTop = keep;
}

// ───────────────────────── настройки: сервисы ─────────────────────────

function loginBar(service) {
  const on = !!service;
  $("#loginBar").hidden = !on;
  document.body.classList.toggle("login-mode", on);
  if (on) {
    // в браузере (--host) страница входа — отдельным окном приложения
    $("#loginBarText").textContent = HOST
      ? service === "vk"
        ? L("Войдите во ВКонтакте в открывшемся окне VK Music. Пароль приложение не видит.")
        : L("Войдите в SoundCloud в открывшемся окне VK Music. Пароль приложение не видит.")
      : service === "vk"
        ? L("Войдите во ВКонтакте: отсканируйте QR-код в приложении VK на телефоне или войдите как обычно. Пароль приложение не видит.")
        : L("Войдите в SoundCloud как обычно. Пароль приложение не видит.");
  }
}

async function login(service) {
  loginBar(service);
  try {
    await invoke("svc_login", { service });
  } catch (e) {
    loginBar(null);
    toast(errMsg(e), "error");
  }
}

$("#loginCancel").addEventListener("click", () => {
  loginBar(null);
  invoke("svc_login_cancel").catch(() => {});
});

async function refreshStatus(service) {
  S.svc[service] = "loading";
  if (S.page === "services") renderServices();
  try {
    S.svc[service] = await invoke(service + "_status");
  } catch (e) {
    S.svc[service] = { logged_in: false, profile: null, error: errMsg(e) };
  }
  if (S.page === "services") renderServices();
}

listen("svc://event", (ev) => {
  const e = ev.payload;
  loginBar(null);
  if (e.type === "login") toast(L("Вы вошли: {0}", SERVICES[e.service] ? SERVICES[e.service].name : e.service));
  if (SERVICES[e.service]) {
    refreshStatus(e.service);
    if (e.type === "login") syncService(e.service);
    // вошли во ВКонтакте — миксы и плейлисты заново (до входа были пустыми или с ошибкой) и сразу на экран:
    // раньше они появлялись только после перехода на другую страницу и обратно
    if (e.type === "login" && e.service === "vk") {
      S.myPl = null;
      S.vkHome = null;
      if (S.page === "vk") loadVkHome(true);
      renderMyPlaylists();
    }
  }
});

const NOTES = {
  vk: "«Мои треки» синхронизируются с библиотекой. ВКонтакте не разрешает сторонние клиенты — пользуйтесь на свой риск.",
  sc: "Лайки синхронизируются с библиотекой. Поиск работает и без входа.",
};

function renderServiceCard(id) {
  const svc = SERVICES[id];
  const st = S.svc[id];
  const card = el("div", "card svc-card");
  const head = el("h3", "svc-head");
  head.append(Icons.icon(svc.icon), el("span", "svc-name", svc.name));
  const status = el("p", "status svc-status");
  const actions = el("div", "row svc-actions");
  if (st === null || st === "loading") {
    status.textContent = L("Проверяю вход…");
    if (st === null) refreshStatus(id);
  } else if (!st.logged_in) {
    status.textContent = L("Вход не выполнен");
    actions.append(button("link", L("Войти"), "primary", () => login(id)));
  } else {
    status.textContent = st.profile ? L("Вы вошли как {0}", st.profile.name) : L("Вход выполнен");
    status.classList.add("on");
    if (st.error) {
      status.textContent += ` — ${L(st.error)}`;
      status.classList.add("err");
      actions.append(button("refresh", L("Проверить"), "", () => refreshStatus(id)));
    }
    actions.append(
      button("logout", L("Выйти"), "danger", (b) =>
        confirmClick(b, async () => {
          try {
            await invoke("svc_logout", { service: id });
            S.saved = S.saved.filter((t) => !(t.service === id && t.sync));
            // плейлисты ВК (и сохранённые на диске) уходят вместе с сессией
            if (id === "vk") {
              S.myPl = null;
              S.vkPl = null;
            }
            if (S.current && S.current.service === id) invoke("player_stop").catch(() => {});
            toast(L("Вы вышли: {0}", svc.name));
          } catch (e) {
            toast(errMsg(e), "error");
          }
          refreshStatus(id);
        }, L("Выйти из {0}?", svc.name), L("Выйти"))
      )
    );
  }
  card.append(head, status);
  if (actions.children.length) card.append(actions);
  card.append(el("p", "status", L(NOTES[id])));
  return card;
}

function renderCaps() {
  const box = el("div", "caps");
  const cap = S.caps;
  if (!cap) return box;
  const item = (ok, text, fix) => {
    const r = el("div", "cap " + (ok ? "ok" : "bad"));
    r.append(Icons.icon(ok ? "check" : "x"), el("span", null, text));
    if (!ok && fix) r.append(el("code", null, fix));
    return r;
  };
  box.append(
    item(!!cap.ytdlp, cap.ytdlp ? `yt-dlp ${cap.ytdlp}` : L("yt-dlp не найден"), "sudo pacman -S yt-dlp"),
    item(cap.ffmpeg, cap.ffmpeg ? "ffmpeg" : L("ffmpeg не найден"), "sudo pacman -S ffmpeg"),
    item(cap.mutagen, cap.mutagen ? L("обложки (python-mutagen)") : L("без обложек: нет python-mutagen"), "sudo pacman -S python-mutagen")
  );
  return box;
}

/** «Сервисы»: вход во ВКонтакте и SoundCloud, загрузка файлов через yt-dlp — карточками, как настройки Whisper. */
function renderServices() {
  const c = $("#content");
  c.replaceChildren();
  for (const id of Object.keys(SERVICES)) c.append(renderServiceCard(id));
  const yd = el("div", "card colbreak");
  yd.append(
    el("h3", null, L("Загрузка файлов (yt-dlp)")),
    el(
      "p",
      "status",
      L("Ссылка в строке поиска (YouTube, YouTube Music, SoundCloud, Bandcamp — всё, что понимает yt-dlp) скачивается в {0} " +
        "лучшей аудиодорожкой без перекодирования — исходный Opus, AAC или FLAC, с тегами{1}. ", S.musicDir, S.caps && S.caps.mutagen ? L(" и обложкой") : "") +
        L("Ссылка на плейлист или альбом скачивается целиком. Ссылка на SoundCloud сначала показывается в поиске — скачать можно кнопкой со стрелкой.")
    ),
    renderCaps()
  );
  const row = el("div", "row");
  row.append(button("folder", L("Открыть папку с музыкой ({0})", S.musicDir.replace(/^\/home\/[^/]+/, "~")), "open-dir", () =>
    invoke("open_music_dir").catch((e) => toast(errMsg(e), "error"))
  ));
  yd.append(row);
  c.append(yd);
}

// ───────────────────────── «Миксы»: VK Микс и плейлисты ВК ─────────────────────────

/** Загрузить раздел (VK Микс, ваши плейлисты, подборки ВК). force — заново («Обновить»). */
async function loadVkHome(force) {
  if (S.vkHome && !force) return;
  S.vkHome = { loading: true };
  if (S.page === "vk") renderVk();
  try {
    S.vkHome = await invoke("vk_home");
  } catch (e) {
    S.vkHome = { error: errText(e) };
  }
  if (S.page === "vk") renderVk();
}

/** Ваши плейлисты ВК для библиотеки (один раз за запуск и по кнопке обновления). Без входа — тихо пропускаем. */
/**
 * Ваши плейлисты: сначала сохранённые на диске (сразу, без сети), затем свежий список из ВК — раз за запуск
 * и по кнопке обновления (`fresh: false`). Не загрузились — остаётся сохранённое.
 */
let myPlBusy = false;
async function loadMyPlaylists() {
  if (myPlBusy || (S.myPl && S.myPl.fresh)) return;
  myPlBusy = true;
  if (!S.myPl) {
    const saved = await invoke("vk_my_playlists_cached").catch(() => null);
    if (saved && !S.myPl) {
      S.myPl = { list: saved, fresh: false };
      renderMyPlaylists();
    }
  }
  try {
    S.myPl = { list: await invoke("vk_my_playlists"), fresh: true };
  } catch (e) {
    if (S.myPl && S.myPl.list) S.myPl.fresh = true;
    else S.myPl = { error: errText(e), fresh: true };
  }
  myPlBusy = false;
  renderMyPlaylists();
}

function renderMyPlaylists() {
  const box = $("#libPlaylists");
  if (!box) return;
  if (!S.myPl || !S.myPl.fresh) loadMyPlaylists();
  // свежий список заменяет сохранённый — строка остаётся прокрученной, где была
  const was = box.querySelector(".simgrid");
  const keepLeft = was ? was.scrollLeft : 0;
  box.replaceChildren();
  box.hidden = !(S.myPl && S.myPl.list);
  // без входа во ВКонтакте раздела нет; со входом — даже пустой, с карточкой «Новый плейлист»
  if (!S.myPl || !S.myPl.list) return;
  box.append(el("h3", null, L("Плейлисты")));
  const row = el("div", "simgrid plrow");
  const add = el("div", "simcard plnew");
  const cover = el("div", "plnewcover");
  cover.append(Icons.icon("plus"));
  add.append(cover, el("b", null, L("Новый плейлист")));
  add.addEventListener("click", (ev) => {
    ev.stopPropagation(); // иначе тот же клик закроет окошко (closePopups на document)
    // новый плейлист сразу открывается — там и добавляются треки
    promptClick(cover, (title) => createPlaylist(title, (pl) => {
      S.vkPl = null;
      openVkPlaylist(pl);
    }), L("Название плейлиста"), L("Создать"));
  });
  row.append(add);
  for (const pl of S.myPl.list) row.append(vkPlaylistCard(pl));
  grabScroll(row);
  box.append(row);
  row.scrollLeft = keepLeft;
}

// ── свои плейлисты ВК: создать, переименовать, удалить, треки в плейлист и из него ──

/** Ваши плейлисты, куда можно добавлять треки (свои, не добавленные чужие). */
const editablePlaylists = () => ((S.myPl && S.myPl.list) || []).filter((pl) => pl.editable);

/** Трек ВК можно положить в свой плейлист (вход есть — список плейлистов загружен). */
function canAddToPlaylist(t) {
  return !!t && t.service === "vk" && !t.unavailable && /^-?\d+_\d+$/.test(t.id) && !!(S.myPl && S.myPl.list);
}

async function createPlaylist(title, then) {
  try {
    const pl = await invoke("vk_playlist_create", { title });
    if (S.myPl && S.myPl.list) S.myPl.list.unshift(pl);
    toast(L("Плейлист создан: {0}", pl.title));
    renderMyPlaylists();
    if (then) await then(pl);
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

/** Меню у кнопки: в какой плейлист положить трек (и «Новый плейлист…»). */
function addToPlaylistMenu(btn, t) {
  popAt(btn, "pl-menu", (pop) => {
    pop.append(el("div", "popup-title", L("В плейлист")));
    for (const pl of editablePlaylists()) {
      const item = el("button", "popup-item");
      item.append(Icons.icon("note"), el("span", null, pl.title || L("Без названия")));
      item.addEventListener("click", () => {
        closeConfirm();
        addToPlaylist(pl, t);
      });
      pop.append(item);
    }
    const add = el("button", "popup-item");
    add.append(Icons.icon("plus"), el("span", null, L("Новый плейлист…")));
    add.addEventListener("click", () =>
      promptClick(btn, (title) => createPlaylist(title, (pl) => addToPlaylist(pl, t)), L("Название плейлиста"), L("Создать"))
    );
    pop.append(add);
  });
}

async function addToPlaylist(pl, t) {
  try {
    await invoke("vk_playlist_add", { owner_id: pl.owner_id, id: pl.id, track_id: t.id });
    pl.count = (pl.count || 0) + 1;
    toast(L("Добавлено в «{0}»: {1}", pl.title, t.title));
    renderMyPlaylists();
    // этот плейлист открыт — перечитать его треки
    const v = S.vkPl;
    if (v && v.pl.owner_id === pl.owner_id && v.pl.id === pl.id) {
      Object.assign(v, { tracks: [], next: 0, error: null });
      loadVkPlaylistMore();
    }
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

function removeFromPlaylistButton(pl, t) {
  return iconButton("listremove", L("Убрать из плейлиста"), (b) =>
    confirmClick(b, async () => {
      try {
        await invoke("vk_playlist_remove", { owner_id: pl.owner_id, id: pl.id, track_id: t.id });
        pl.count = Math.max(0, (pl.count || 1) - 1);
        if (S.vkPl && samePl(S.vkPl.pl, pl)) S.vkPl.tracks = S.vkPl.tracks.filter((x) => x.key !== t.key);
        const keep = scroller().scrollTop;
        rerender();
        scroller().scrollTop = keep;
      } catch (e) {
        toast(errMsg(e), "error");
      }
    }, L("Убрать трек из плейлиста «{0}»?", pl.title), L("Убрать"))
  );
}

/**
 * Кнопки открытого плейлиста — справа от «Слушать»: чужой — «К себе»; свой — «⋯» с «Переименовать» и «Удалить»,
 * добавленный чужой — «⋯» с «Убрать из моих плейлистов». Кнопки живут в шапке плейлиста и уходят вместе с ним.
 */
function playlistActions(v) {
  const pl = v.pl;
  if (!pl.mine) {
    if (!(S.myPl && S.myPl.list)) return [];
    return [
      button("plus", L("К себе"), "", async (b) => {
        b.disabled = true;
        try {
          await invoke("vk_playlist_follow", { owner_id: pl.owner_id, id: pl.id });
          toast(L("Плейлист добавлен к вашим: {0}", pl.title));
          if (S.myPl) S.myPl.fresh = false; // перечитать «Плейлисты» в библиотеке
          loadMyPlaylists();
        } catch (e) {
          toast(errMsg(e), "error");
        }
        b.disabled = false;
      }),
    ];
  }
  const rename = (btn) =>
    promptClick(btn, async (title) => {
      try {
        await invoke("vk_playlist_rename", { owner_id: pl.owner_id, id: pl.id, title });
        pl.title = title;
        rerender();
      } catch (e) {
        toast(errMsg(e), "error");
      }
    }, L("Название плейлиста"), L("Сохранить"), pl.title);
  const remove = (btn) =>
    confirmClick(btn, async () => {
      try {
        await invoke("vk_playlist_delete", { owner_id: pl.owner_id, id: pl.id });
        if (S.myPl && S.myPl.list) S.myPl.list = S.myPl.list.filter((x) => !samePl(x, pl));
        toast(pl.editable ? L("Плейлист удалён: {0}", pl.title) : L("Плейлист убран: {0}", pl.title));
        S.vkPl = null;
        rerender();
      } catch (e) {
        toast(errMsg(e), "error");
      }
    }, pl.editable ? L("Удалить плейлист «{0}» во ВКонтакте?", pl.title) : L("Убрать плейлист «{0}» из ваших?", pl.title),
    pl.editable ? L("Удалить") : L("Убрать"));
  const more = iconButton("dots", L("Ещё"), (b) => {
    if ($("#confirmPop") && $("#confirmPop")._btn === b) return closeConfirm();
    popAt(b, "pl-menu", (pop) => {
      const item = (icon, text, fn, cls) => {
        const it = el("button", "popup-item" + (cls ? " " + cls : ""));
        it.append(Icons.icon(icon), el("span", null, text));
        // окошко ввода / подтверждения встаёт на место меню, у той же кнопки
        it.addEventListener("click", () => fn(b));
        pop.append(it);
      };
      if (pl.editable) item("edit", L("Переименовать"), rename);
      item("trash", pl.editable ? L("Удалить плейлист") : L("Убрать из моих плейлистов"), remove, "danger");
    });
  });
  more.classList.add("vk-pl-more");
  return [more];
}

/**
 * Прокрутка строки перетаскиванием мышью (полосы прокрутки нет). Сдвиг больше GRAB_START — это прокрутка, и
 * нажатие на карточку не срабатывает; отпустили на ходу — строка докатывается с замедлением.
 */
const GRAB_START = 5;
function grabScroll(row) {
  row.classList.add("grab");
  // край, за которым есть ещё карточки, плавно гаснет — видно, что строку можно прокрутить
  const edges = () => {
    const max = row.scrollWidth - row.clientWidth;
    row.classList.toggle("more-l", row.scrollLeft > 1);
    row.classList.toggle("more-r", row.scrollLeft < max - 1);
  };
  row.addEventListener("scroll", edges, { passive: true });
  new ResizeObserver(edges).observe(row);
  row.addEventListener("pointerdown", (ev) => {
    if (ev.button !== 0 || ev.pointerType === "touch") return;
    const x0 = ev.clientX, left0 = row.scrollLeft;
    let moved = false, lastX = ev.clientX, lastT = performance.now(), v = 0;
    const move = (e) => {
      const dx = e.clientX - x0;
      if (!moved && Math.abs(dx) < GRAB_START) return;
      if (!moved) {
        moved = true;
        row.classList.add("grabbing");
        row.setPointerCapture(e.pointerId);
      }
      row.scrollLeft = left0 - dx;
      const now = performance.now();
      // скорость, px/мс — не больше 2.5 (иначе один резкий рывок улетает в конец строки)
      v = Math.max(-2.5, Math.min(2.5, (lastX - e.clientX) / Math.max(1, now - lastT)));
      lastX = e.clientX;
      lastT = now;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (!moved) return;
      row.classList.remove("grabbing");
      // клик, который придёт после отпускания, — не «открыть плейлист»
      const stop = (e) => {
        e.stopPropagation();
        e.preventDefault();
      };
      row.addEventListener("click", stop, { capture: true, once: true });
      setTimeout(() => row.removeEventListener("click", stop, { capture: true }), 0);
      // инерция: скорость гаснет плавно (ease-out)
      let t = performance.now();
      const glide = (now) => {
        const dt = now - t;
        t = now;
        v *= Math.pow(0.9, dt / 16);
        if (Math.abs(v) < 0.02) return;
        const before = row.scrollLeft;
        row.scrollLeft += v * dt;
        if (row.scrollLeft !== before) requestAnimationFrame(glide);
      };
      if (performance.now() - lastT < 80) requestAnimationFrame(glide);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  });
}

/** Карточка плейлиста — как «похожий альбом» у Whisper: .simcard > img, b, span. */
function vkPlaylistCard(pl) {
  const card = el("div", "simcard");
  card.append(trackImg(pl.cover), el("b", null, pl.title || L("Без названия")));
  // «Слушайте друг друга»: чей плейлист и насколько совпадает со вкусом (как на сайте ВК)
  if (pl.owner_name || pl.match_pct != null) {
    card.classList.add("friendpl");
    if (pl.owner_name) card.append(el("span", "plowner", pl.owner_name));
    if (pl.match_pct != null) card.append(el("span", "plmatch", L("Совпадение {0}%", pl.match_pct)));
  } else {
    const sub = pl.count ? `${pl.count} ${plural(pl.count, "трек", "трека", "треков")}` : pl.subtitle || "";
    card.append(el("span", null, sub));
  }
  card.title = pl.title || "";
  card.addEventListener("click", () => openVkPlaylist(pl));
  return card;
}

function renderVk() {
  const c = $("#content");
  if (S.vkPl && S.vkPl.from === "vk") return renderVkPlaylist();
  c.replaceChildren();
  headBtns(button("refresh", L("Обновить"), "pin", () => loadVkHome(true)));
  const h = S.vkHome;
  if (!h) return loadVkHome();
  if (h.loading) return c.append(el("div", "spinner"));
  if (h.error) {
    if (/не вошли/i.test(h.error)) {
      const b = button("link", L("Войти"), "primary", () => login("vk"));
      return c.append(emptyState("vk", L("Войдите во ВКонтакте, чтобы слушать VK Микс и плейлисты."), b));
    }
    const retry = button(null, L("Повторить"), "", () => loadVkHome(true));
    return c.append(emptyState("x", L(h.error), retry));
  }
  // миксы ВК — карточками с кнопкой «Слушать»; главный (VK Микс) — ещё и отдельной страницей с большой кнопкой
  const mixes = h.mixes || [];
  if (mixes.length) {
    const sec = el("section", "similar mixsec");
    sec.append(el("h3", null, L("Миксы")));
    const g = el("div", "mixgrid");
    for (const m of mixes) {
      const card = el("div", "card vk-mix" + (S.queueMix === m.id ? " on" : ""));
      card.append(
        el("h3", null, m.id === "common" ? L("VK Микс") : m.title),
        el("p", "status", m.id === "common" ? L("Бесконечная подборка по вашему вкусу") : m.description || "")
      );
      const row = el("div", "row");
      row.append(button("play", S.queueMix === m.id ? L("Играет") : L("Слушать"), "primary", (b) => playMix(m.id, b)));
      card.append(row);
      g.append(card);
    }
    sec.append(g);
    c.append(sec);
  }
  for (const grp of h.groups || []) {
    if (!grp.playlists || !grp.playlists.length) continue;
    const sec = el("section", "similar");
    sec.append(el("h3", null, L(grp.title)));
    const g = el("div", "simgrid");
    for (const pl of grp.playlists) g.append(vkPlaylistCard(pl));
    sec.append(g);
    c.append(sec);
  }
  if (!mixes.length && !(h.groups || []).length) c.append(emptyState("mix", L("ВКонтакте не прислал подборок.")));
}

const samePl = (a, b) => a.owner_id === b.owner_id && a.id === b.id;

/** Открыть плейлист. Ваш, уже открывавшийся, — сразу с диска; первая страница из ВК потом заменяет его. */
async function openVkPlaylist(pl) {
  const v = { pl, tracks: [], next: 0, busy: false, error: null, from: S.page };
  S.vkPl = v;
  scroller().scrollTop = 0;
  const saved = pl.mine ? await invoke("vk_playlist_cached", { owner_id: pl.owner_id, id: pl.id }).catch(() => null) : null;
  if (S.vkPl !== v) return;
  if (!saved) return loadVkPlaylistMore();
  v.tracks = concatUnique([], saved.tracks.map((x) => svcTrack("vk", x)));
  v.next = saved.next_offset ?? null;
  rerender();
  refreshVkPlaylist(v);
}

/** Сохранённый плейлист на экране — свежая первая страница из ВК без спиннера, прокрутка на месте. */
async function refreshVkPlaylist(v) {
  v.refreshing = true;
  try {
    const p = await invoke("vk_playlist", { owner_id: v.pl.owner_id, id: v.pl.id, offset: 0 });
    if (S.vkPl !== v) return;
    v.tracks = concatUnique([], p.tracks.map((x) => svcTrack("vk", x)));
    v.next = p.next_offset ?? null;
  } catch (e) {
    // не обновился — остаётся сохранённый
  } finally {
    v.refreshing = false;
  }
  if (S.vkPl === v && S.page === v.from) {
    const keep = scroller().scrollTop;
    rerender();
    scroller().scrollTop = keep;
  }
}

async function loadVkPlaylistMore() {
  const v = S.vkPl;
  if (!v || v.busy || v.refreshing || v.next == null) return;
  v.busy = true;
  rerender();
  try {
    const p = await invoke("vk_playlist", { owner_id: v.pl.owner_id, id: v.pl.id, offset: v.next });
    if (S.vkPl !== v) return;
    v.tracks = concatUnique(v.tracks, p.tracks.map((x) => svcTrack("vk", x)));
    v.next = p.next_offset ?? null;
  } catch (e) {
    v.error = errText(e);
  }
  v.busy = false;
  if (S.vkPl === v && S.page === v.from) {
    const keep = scroller().scrollTop;
    rerender();
    scroller().scrollTop = keep;
  }
}

function renderVkPlaylist() {
  const c = $("#content");
  const v = S.vkPl;
  c.replaceChildren();
  // шапка — как у альбома Whisper: обложка, название, сколько треков; кнопки — строкой .plbar под шапкой
  const sub = v.pl.count ? `${v.pl.count} ${plural(v.pl.count, "трек", "трека", "треков")}` : v.pl.subtitle || "";
  setHead(v.pl.title || L("Без названия"), sub, v.pl.cover || null);
  syncBack();
  const playable = () => v.tracks.filter((x) => !x.unavailable);
  const bar = el("div", "plbar");
  const play = button(null, "▶ " + L("Слушать"), "primary", () => playable().length && playList(playable(), 0));
  play.disabled = !playable().length;
  bar.append(play, ...playlistActions(v));
  // свой плейлист: «Добавить треки» — список из библиотеки с поиском (у пустого открыт сразу)
  const empty = !v.tracks.length && !v.busy && !v.error && v.next == null;
  if (v.pl.editable) {
    if (v.adding == null && empty) v.adding = true;
    const add = button("plus", L("Добавить треки"), v.adding ? "on" : "", () => ((v.adding = !v.adding), rerender()));
    bar.append(add);
  }
  c.append(bar);
  if (v.tracks.length) c.append(renderTracks(v.tracks, false, { playlist: v.pl.editable ? v.pl : null }));
  if (v.busy) c.append(el("div", "spinner"));
  else if (v.error) c.append(emptyState("x", L(v.error), button(null, L("Повторить"), "", () => ((v.error = null), loadVkPlaylistMore()))));
  else if (v.next != null) c.append(button(null, L("Показать ещё"), "more", () => loadVkPlaylistMore()));
  else if (!v.tracks.length)
    c.append(el("div", "status", v.pl.editable
      ? L("В плейлисте пока нет треков. Добавьте их из библиотеки ниже или у любого трека ВКонтакте: «⋯» → «В плейлист».")
      : L("В плейлисте нет треков, которые можно слушать.")));
  if (v.pl.editable && v.adding) c.append(playlistAdder(v));
}

/**
 * «Добавить треки» в свой плейлист: треки ВКонтакте из библиотеки, которых в нём ещё нет, с поиском
 * по названию и исполнителю (как рекомендации с «+» на пустом плейлисте у Whisper Yandex). Добавляет addToPlaylist.
 */
function playlistAdder(v) {
  const box = el("section", "similar pladd");
  box.append(el("h3", null, L("Добавить из библиотеки")));
  const q = el("input", "field pladd-q");
  q.type = "search";
  q.placeholder = L("Поиск в библиотеке");
  q.value = v.addQuery || "";
  const list = el("ul", "list pladd-list");
  const fill = () => {
    const have = new Set(v.tracks.map((x) => x.id));
    const words = normText(q.value).split(" ").filter(Boolean);
    const pool = allTracks().filter((t) => canAddToPlaylist(t) && !have.has(t.id)
      && words.every((w) => normText(t.artist + " " + t.title).includes(w)));
    list.replaceChildren();
    for (const t of pool.slice(0, 60)) {
      const li = el("li");
      const meta = el("div");
      meta.append(el("div", "t", t.title), el("div", "a", t.artist));
      const b = iconButton("plus", L("Добавить в этот плейлист"), async (btn) => {
        btn.disabled = true;
        li.classList.add("added");
        await addToPlaylist(v.pl, t);
      });
      b.classList.add("recadd");
      li.append(trackImg(t.cover), meta, b);
      list.append(li);
    }
    if (!pool.length) list.append(el("li", "status", words.length ? L("Ничего не нашлось") : L("В библиотеке нет треков ВКонтакте, которых нет в этом плейлисте.")));
  };
  q.addEventListener("input", () => ((v.addQuery = q.value), fill()));
  fill();
  box.append(q, list);
  return box;
}

// ───────────────────────── страница «VK Микс»: большая кнопка, как «Моя волна» у Whisper ─────────────────────────

const WAVE_ICON = '<svg viewBox="0 0 24 24" width="54" height="54" aria-hidden="true"><path fill="currentColor" d="M8 5.5v13l10.5-6.5z"/></svg>';
const WAVE_PAUSE = '<svg viewBox="0 0 24 24" width="54" height="54" aria-hidden="true"><path fill="currentColor" d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z"/></svg>';

function renderWave() {
  setHead(L("VK Микс"));
  syncWave();
}

/** Кнопка и подпись страницы «VK Микс»: играет ли микс, что сейчас звучит. */
function syncWave() {
  const on = S.queueMix === "common" && !!S.current;
  const playing = on && !S.pl.paused && !S.pl.idle;
  const b = $("#waveBtn");
  b.innerHTML = playing ? WAVE_PAUSE : WAVE_ICON;      // свои постоянные значки, данных в разметке нет
  b.classList.toggle("playing", playing);
  b.title = playing ? L("Пауза") : L("VK Микс");
  $("#waveHint").hidden = on;
  $("#waveHint").textContent = S.svc.vk && S.svc.vk.logged_in === false ? L("Войдите во ВКонтакте, чтобы слушать VK Микс") : L("Нажми, чтобы включить VK Микс");
  $("#nowplay").hidden = !on;
  if (on) {
    $("#waveTitle").textContent = S.current.title;
    $("#waveArtist").textContent = S.current.artist;
  }
}

$("#waveBtn").addEventListener("click", () => {
  if (S.queueMix === "common" && S.current) return togglePlay();
  playMix("common");
});

/** Включить VK Микс: первые треки — и играть; дальше очередь догружается сама (extendMix). */
async function playMix(id, btn) {
  if (btn) btn.disabled = true;
  try {
    const list = (await invoke("vk_mix", { mix_id: id })).map((x) => svcTrack("vk", x)).filter((x) => !x.unavailable);
    if (!list.length) throw L("ВКонтакте не прислал треков для микса. Попробуйте ещё раз.");
    playList(list, 0);
    S.queueMix = id;
    // сразу ещё порцию — микс отдаёт по 2–3 трека
    extendMix();
  } catch (e) {
    toast(errMsg(e), "error");
  }
  if (btn) btn.disabled = false;
  if (S.page === "vk" && !(S.vkPl && S.vkPl.from === "vk")) renderVk();
  syncWave();
}

async function extendMix() {
  const id = S.queueMix;
  if (!id || S.mixBusy) return;
  S.mixBusy = true;
  try {
    const more = (await invoke("vk_mix", { mix_id: id })).map((x) => svcTrack("vk", x)).filter((x) => !x.unavailable);
    if (S.queueMix === id) {
      const have = new Set(S.queue.map((x) => x.key));
      S.queue.push(...more.filter((x) => !have.has(x.key)));
      // только строка «Очередь» — без перерисовки панели (не закрывать пояснение, не сбрасывать текст)
      updateQueueInfo();
      Whisper.player.report({ queue: true });
    }
  } catch (e) {
    // не догрузили — доиграет то, что есть
  }
  S.mixBusy = false;
}

// ───────────────────────── страница «Статистика» ─────────────────────────

const PERIODS = [["1", "Сегодня"], ["7", "7 дней"], ["30", "30 дней"], ["all", "Всё время"]];
let statsTimer = null;

/** Статистика обновилась (порция секунд) — перечитать страницу, но не чаще раза в 5 с. */
function reloadStatsSoon() {
  if (statsTimer) return;
  statsTimer = setTimeout(async () => {
    statsTimer = null;
    if (S.page !== "stats") return;
    try {
      S.statsData = await invoke("stats_all");
      const keep = scroller().scrollTop;
      renderStatsPage();
      scroller().scrollTop = keep;
    } catch (e) {}
  }, 5000);
}

/** Даты периода (местные, ГГГГ-ММ-ДД), от старой к новой; для «всё время» — null. */
function periodDays(period) {
  if (period === "all") return null;
  const n = Number(period), out = [], d = new Date();
  for (let i = n - 1; i >= 0; i--) {
    const x = new Date(d);
    x.setDate(d.getDate() - i);
    out.push(localDay(x));
  }
  return out;
}

/** Дней подряд с музыкой (не меньше 30 с), считая сегодня или — если сегодня ещё не слушали — вчера. */
function streak(days) {
  const has = (d) => days[d] && Object.values(days[d].tracks || {}).reduce((a, b) => a + b, 0) >= 30;
  const d = new Date();
  if (!has(localDay(d))) d.setDate(d.getDate() - 1);
  let n = 0;
  while (has(localDay(d))) {
    n++;
    d.setDate(d.getDate() - 1);
  }
  return n;
}

/**
 * Столбчатый график одной серии: подписи под столбиками — label(i) (пусто — без подписи),
 * подсказка при наведении — tip(i). Высота — доля от максимума.
 */
function barChart(values, label, tip) {
  const box = el("div", "chart");
  const max = Math.max(...values, 0);
  box.append(el("div", "chart-max", max ? L("максимум — {0}", fmtListened(max)) : L("пока пусто")));
  const bars = el("div", "chart-bars");
  const axis = el("div", "chart-axis");
  const tipEl = el("div", "chart-tip");
  tipEl.hidden = true;
  values.forEach((v, i) => {
    const col = el("div", "chart-col" + (v ? "" : " empty"));
    const bar = el("div", "chart-bar");
    bar.style.height = max ? `${(v / max) * 100}%` : "0";
    col.append(bar);
    // область наведения — весь столбец, а не только видимая часть
    col.addEventListener("mouseenter", () => {
      tipEl.replaceChildren(el("b", null, tip(i)), document.createTextNode(" · " + (v ? fmtListened(v) : L("не слушали"))));
      tipEl.hidden = false;
      const r = col.getBoundingClientRect(), b = box.getBoundingClientRect();
      tipEl.style.left = `${Math.min(Math.max(r.left - b.left + r.width / 2, 60), b.width - 60)}px`;
      tipEl.style.top = `${bars.offsetTop + bars.offsetHeight * (1 - (max ? v / max : 0)) - 6}px`;
    });
    col.addEventListener("mouseleave", () => (tipEl.hidden = true));
    bars.append(col);
    axis.append(el("span", null, label(i)));
  });
  box.append(bars, axis, tipEl);
  return box;
}

/** Строка топа: место, обложка, название и подпись, полоска доли, значение. */
function statsRow(rank, t, title, sub, value, share, onClick) {
  const row = el("div", "stats-row" + (t ? "" : " no-cover") + (onClick ? " clickable" : ""));
  row.append(el("div", "stats-rank", String(rank)));
  if (t) {
    const cover = el("div", "t-cover");
    if (t.cover) {
      const img = el("img");
      img.loading = "lazy";
      img.alt = "";
      img.src = t.cover;
      img.onerror = () => img.replaceWith(Icons.icon("note"));
      cover.append(img);
    } else cover.append(Icons.icon("note"));
    row.append(cover);
  }
  const name = el("div", "stats-name");
  name.append(el("div", null, title));
  if (sub) name.append(el("div", null, sub));
  if (share > 0) {
    const meter = el("div", "stats-meter");
    meter.style.width = `${Math.max(2, share * 100)}%`;
    name.append(meter);
  }
  row.append(name, el("div", "stats-value", value));
  if (onClick) row.addEventListener("click", onClick);
  return row;
}

async function renderStatsPage() {
  const c = $("#content");
  c.replaceChildren();
  const page = el("div", "stats-page");
  c.append(page);

  const periods = el("div", "chips stats-periods");
  for (const [id, name] of PERIODS) {
    const b = el("button", "chip" + (S.statsPeriod === id ? " active" : ""), L(name));
    b.addEventListener("click", () => {
      S.statsPeriod = id;
      renderStatsPage();
    });
    periods.append(b);
  }
  page.append(periods);

  if (!S.statsData) {
    page.append(el("div", "spinner"));
    try {
      // пустой ответ — пустая статистика (иначе страница перезапрашивала бы данные без конца)
      S.statsData = (await invoke("stats_all")) || { tracks: {}, days: {} };
    } catch (e) {
      page.lastChild.replaceWith(emptyState("x", L("Статистика недоступна: ") + errMsg(e)));
      return;
    }
    if (S.page === "stats") renderStatsPage();
    return;
  }

  const data = S.statsData;
  const days = data.days || {};
  const tracks = data.tracks || {};
  const span = periodDays(S.statsPeriod);
  const inPeriod = span ? span.filter((d) => days[d]) : Object.keys(days);
  // секунды по трекам и по часам за период
  const perTrack = new Map();
  const hours = new Array(24).fill(0);
  for (const d of inPeriod) {
    for (const [k, v] of Object.entries(days[d].tracks || {})) perTrack.set(k, (perTrack.get(k) || 0) + v);
    (days[d].hours || []).forEach((v, h) => (hours[h] += v || 0));
  }
  const total = [...perTrack.values()].reduce((a, b) => a + b, 0);
  const activeDays = inPeriod.filter((d) => Object.values(days[d].tracks || {}).some((v) => v > 0)).length;
  const byKey = new Map(allTracks().map((t) => [t.key, t]));

  // итоги
  const tiles = el("div", "stats-tiles");
  const tile = (value, label) => {
    const x = el("div", "stats-tile statcard card");
    x.append(el("div", "stats-tile-value", value), el("div", "stats-tile-label", label));
    tiles.append(x);
  };
  tile(fmtListened(total), L("слушали"));
  tile(String(perTrack.size), plural(perTrack.size, "трек", "трека", "треков"));
  if (S.statsPeriod !== "1") tile(String(activeDays), plural(activeDays, "день с музыкой", "дня с музыкой", "дней с музыкой"));
  const st = streak(days);
  tile(`${st} ${plural(st, "день", "дня", "дней")}`, L("подряд с музыкой"));
  page.append(tiles);

  // по дням (за «всё время» — по месяцам)
  if (S.statsPeriod !== "1") {
    let values, label, tip, title;
    const month = (d) => new Date(d + "T12:00:00");
    if (span) {
      title = L("По дням");
      values = span.map((d) => Object.values((days[d] || {}).tracks || {}).reduce((a, b) => a + b, 0));
      const every = span.length > 10 ? 5 : 1;
      label = (i) => ((span.length - 1 - i) % every === 0 ? String(month(span[i]).getDate()) : "");
      tip = (i) => fmtWhen(month(span[i]).getTime() / 1000, true);
    } else {
      title = L("По месяцам");
      const months = new Map();
      for (const d of Object.keys(days).sort()) {
        const m = d.slice(0, 7);
        months.set(m, (months.get(m) || 0) + Object.values(days[d].tracks || {}).reduce((a, b) => a + b, 0));
      }
      const keys = [...months.keys()];
      values = keys.map((m) => months.get(m));
      const name = (m) => month(m + "-01").toLocaleDateString(I18N.locale, { month: "short", year: "2-digit" });
      label = (i) => name(keys[i]);
      tip = (i) => month(keys[i] + "-01").toLocaleDateString(I18N.locale, { month: "long", year: "numeric" }).replace(/ г\.$/, "");
    }
    page.append(el("h3", "section-title", title));
    page.append(values.length ? barChart(values, label, tip) : el("p", "hint", L("Пока пусто — статистика копится, пока играет музыка.")));
  }

  page.append(el("h3", "section-title", L("По часам суток")));
  page.append(barChart(hours, (h) => (h % 3 === 0 ? String(h) : ""), (h) => `${h}:00–${h + 1}:00`));

  // топы
  const cols = el("div", "stats-cols");
  const top = [...perTrack.entries()].filter(([k]) => byKey.has(k)).sort((a, b) => b[1] - a[1]).slice(0, 10);
  const topBox = el("div", "card stbox");
  topBox.append(el("h3", "section-title", L("Топ треков")));
  const topList = el("div", "stats-list");
  if (!top.length) topList.append(el("p", "hint", L("За этот период треков из библиотеки не слушали.")));
  const topTracks = top.map(([k]) => byKey.get(k));
  top.forEach(([k, v], i) => {
    const t = byKey.get(k);
    const plays = (tracks[k] || {}).plays || 0;
    const sub = t.artist + (plays ? " · " + L("дослушан {0} {1}", plays, plural(plays, "раз", "раза", "раз")) : "");
    topList.append(statsRow(i + 1, t, t.title, sub, fmtListened(v), v / top[0][1], () => playList(topTracks, i)));
  });
  topBox.append(topList);

  const artists = new Map();
  for (const [k, v] of perTrack) {
    const t = byKey.get(k);
    if (t) artists.set(t.artist, (artists.get(t.artist) || 0) + v);
  }
  const topArtists = [...artists.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10);
  const artBox = el("div", "card stbox");
  artBox.append(el("h3", "section-title", L("Топ исполнителей")));
  const artList = el("div", "stats-list");
  if (!topArtists.length) artList.append(el("p", "hint", L("Пока пусто.")));
  topArtists.forEach(([name, v], i) => artList.append(statsRow(i + 1, null, name, "", fmtListened(v), v / topArtists[0][1])));
  artBox.append(artList);
  cols.append(topBox, artBox);
  page.append(cols);

  // давно не слушали: любимые (по времени за всё время), которые не играли больше 30 дней
  const monthAgo = Date.now() / 1000 - 30 * 86400;
  const forgotten = Object.entries(tracks)
    .filter(([k, s]) => byKey.has(k) && s.last && s.last < monthAgo && s.seconds >= 60)
    .sort((a, b) => b[1].seconds - a[1].seconds)
    .slice(0, 10);
  const never = allTracks().filter((t) => !t.unavailable && !(tracks[t.key] && tracks[t.key].seconds > 0));
  const cols2 = el("div", "stats-cols");
  const fBox = el("div", "card stbox");
  fBox.append(el("h3", "section-title", L("Давно не слушали")));
  const fList = el("div", "stats-list");
  if (!forgotten.length) fList.append(el("p", "hint", L("Здесь появятся любимые треки, которые не играли больше месяца.")));
  const fTracks = forgotten.map(([k]) => byKey.get(k));
  forgotten.forEach(([k, s], i) => {
    const t = byKey.get(k);
    fList.append(statsRow(i + 1, t, t.title, t.artist + " · " + L("последний раз {0}", fmtWhen(s.last, true)), fmtListened(s.seconds), s.seconds / forgotten[0][1].seconds, () => playList(fTracks, i)));
  });
  fBox.append(fList);
  const nBox = el("div", "card stbox");
  nBox.append(el("h3", "section-title", L("Ни разу не слушали — {0}", never.length)));
  const nList = el("div", "stats-list");
  if (!never.length) nList.append(el("p", "hint", L("Вы слушали всю библиотеку.")));
  const nShow = never.slice(0, 10);
  nShow.forEach((t, i) => nList.append(statsRow(i + 1, t, t.title, t.artist, t.duration ? fmt(t.duration) : "", 0, () => playList(never, i))));
  nBox.append(nList);
  cols2.append(fBox, nBox);
  page.append(cols2);
}

// ───────────────────────── настройки: вкладки и карточки, как у Whisper ─────────────────────────
//
// #settingsPage — постоянная секция: карточки модулей ядра (вкладка «Интеграции») вставляются в неё при запуске и
// остаются. Свои карточки (.own) рисуются заново при каждом показе. Какая вкладка видна — data-tab на секции,
// прячет остальные CSS Whisper (.settings[data-tab=…] > .card:not([data-tab=…])).

function setCard(tab, title, cls) {
  const card = el("div", "card own" + (cls ? " " + cls : ""));
  card.dataset.tab = tab;
  if (title) card.append(el("h3", null, title));
  $("#settingsPage").append(card);
  return card;
}

/** Переключатель Whisper: label.switch > input + i + span. */
function switchRow(text, on, onChange) {
  const row = el("label", "switch");
  const box = el("input");
  box.type = "checkbox";
  box.checked = !!on;
  box.addEventListener("change", () => onChange(box.checked));
  row.append(box, el("i"), el("span", null, text));
  return row;
}

function renderSettings() {
  const page = $("#settingsPage");
  // «Текст песен» теперь — карточка вкладки «Общее» (прежняя вкладка "play" — туда же)
  const tab = !S.setTab || S.setTab === "play" ? (S.setTab ? "ui" : "look") : S.setTab;
  page.dataset.tab = tab;
  page.querySelectorAll("#setTabs .chip").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab));
  page.querySelectorAll(":scope > .card.own").forEach((x) => x.remove());
  // карточки модулей ядра — после своих (они вставлены раньше): свои — перед первой карточкой модуля
  const mods = [...page.querySelectorAll(":scope > .card:not(.own)")];
  renderThemes();
  renderGeneral();
  renderLyricsSettings();
  renderStorage();
  for (const m of mods) page.append(m);
}

document.querySelectorAll("#setTabs .chip").forEach((b) =>
  b.addEventListener("click", () => {
    S.setTab = b.dataset.tab;
    if (S.setTab === "out") S.cacheInfo = null;
    renderSettings();
  })
);

function renderLyricsSettings() {
  const c = setCard("ui", L("Текст песен"));
  const toggle = (label, key, pref) =>
    c.append(switchRow(L(label), S[key], async (on) => {
      S[key] = on;
      renderLyrics();
      await invoke("save_prefs", { [pref]: on }).catch((e) => toast(errMsg(e), "error"));
    }));
  toggle("Скрывать строки частей песни: [Куплет], [Припев], [Бридж]…", "lyrHideParts", "lyrics_hide_parts");
  toggle("Убирать пустые строки между частями", "lyrCompact", "lyrics_compact");
  c.append(el("p", "status", L("Текст — кнопкой в плеере или нажатием на обложку.")));
}

// ───────────────────────── настройки: общее (язык) ─────────────────────────

const LANGS = [["ru", "Русский"], ["en", "English"]];

function renderGeneral() {
  const c = setCard("ui", L("Язык"));
  const row = el("div", "lyseg lang-switch");
  for (const [code, name] of LANGS) {
    const b = el("button", "chip" + (I18N.lang === code ? " active" : ""), name);
    b.addEventListener("click", () => switchLang(code));
    row.append(b);
  }
  c.append(row);
  if (window.Layout) window.Layout.card(setCard("ui", L("Расположение")));
  if (window.Hotkeys) window.Hotkeys.card(setCard("ui", L("Горячие клавиши")));
}

/** Сменить язык на лету: статичная разметка, текущая страница, плеер, панели. */
async function switchLang(code) {
  if (code === I18N.lang) return;
  try {
    await invoke("save_prefs", { lang: code });
  } catch (e) {
    return toast(errMsg(e), "error");
  }
  setLang(code);
  // всё, что нарисовано с текстом, — заново
  rerender();
  window.Layout?.refresh();
  updateNowPlaying();
  syncModes();
  syncVolume();
  syncNormalize();
  renderSyncStatus();
  updatePlayButton();
}

// ───────────────────────── хранилище: папка загрузок и кэш треков ─────────────────────────

const tilde = (p) => p.replace(/^\/home\/[^/]+/, "~");

function renderStorage() {
  const c = setCard("out", L("Загрузки"));
  c.append(el("p", "status", L("Куда кнопка со стрелкой сохраняет треки. Пусто — ~/Downloads.")));
  const row = el("form", "row storage-row");
  const input = el("input", "field");
  input.type = "text";
  input.spellcheck = false;
  input.placeholder = "~/Downloads";
  input.value = tilde(S.downloadDir);
  input.setAttribute("aria-label", L("Путь загрузки"));
  const save = el("button", "nav active", L("Сохранить"));
  save.type = "submit";
  row.append(input, save);
  row.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    try {
      await invoke("save_prefs", { download_dir: input.value.trim() });
      const info = await invoke("app_info");
      S.downloadDir = info.download_dir;
      input.value = tilde(S.downloadDir);
      toast(L("Папка загрузок: {0}", tilde(S.downloadDir)));
    } catch (e) {
      toast(errMsg(e), "error");
    }
  });
  c.append(row);

  const k = setCard("out", L("Кэш треков"), "colbreak");
  const ci = S.cacheInfo;
  let text = L("Считаю размер кэша…");
  if (ci === null) loadCacheInfo();
  else if (ci.error) text = ci.error;
  else if (!ci.count) text = L("Кэш пуст");
  else text = `${fmtSize(ci.bytes)} · ${ci.count} ${plural(ci.count, "трек", "трека", "треков")}`;
  k.append(
    el(
      "p",
      "status",
      L("Включённые треки библиотеки сохраняются сюда и дальше играют без запросов к сервису.")
    )
  );
  // размер — отдельной заметной строкой между описанием и кнопкой
  const size = el("div", "dlpath cache-size");
  size.append(el("small", null, ci && ci.dir ? tilde(ci.dir) : L("Кэш треков")), el("span", null, text));
  k.append(size);
  // предел размера: сверх него удаляются давно не игравшие треки (Rust, TrackCache::prune)
  const lim = el("div", "lyrow cache-limit");
  const seg = el("div", "lyseg");
  for (const [mb, name] of [[0, L("Без ограничения")], [1024, "1 ГБ"], [2048, "2 ГБ"], [5120, "5 ГБ"], [10240, "10 ГБ"]]) {
    const b = el("button", "chip" + ((S.cacheLimit || 0) === mb ? " active" : ""), L(name));
    b.type = "button";
    b.addEventListener("click", async () => {
      try {
        await invoke("save_prefs", { cache_limit_mb: mb });
        S.cacheLimit = mb;
        S.cacheInfo = null;
        renderSettings();
      } catch (e) {
        toast(errMsg(e), "error");
      }
    });
    seg.append(b);
  }
  lim.append(el("span", null, L("Размер кэша")), seg);
  k.append(lim);
  const clear = button("trash", L("Очистить кэш"), "danger", (b) =>
    confirmClick(b, async () => {
      try {
        await invoke("cache_clear");
        toast(L("Кэш очищен"));
      } catch (e) {
        toast(errMsg(e), "error");
      }
      S.cacheInfo = null;
      if (S.page === "settings") renderSettings();
    }, L("Удалить все треки из кэша?"), L("Очистить"))
  );
  clear.disabled = !!ci && !ci.error && !ci.count;
  const r2 = el("div", "row");
  r2.append(clear);
  k.append(r2);
}

async function loadCacheInfo() {
  try {
    // пустой ответ — пустой кэш (иначе страница перезапрашивала бы размер без конца)
    S.cacheInfo = (await invoke("cache_info")) || { dir: "", count: 0, bytes: 0 };
  } catch (e) {
    S.cacheInfo = { error: errMsg(e) };
  }
  if (S.page === "settings") renderSettings();
}

// ───────────────────────── загрузки через yt-dlp ─────────────────────────

function renderDownload(d) {
  const row = el("div", "dl");
  row.dataset.id = d.id;
  row.classList.toggle("done", d.status === "done");
  row.classList.toggle("failed", d.status === "failed");
  const files = d.count ? ` · ${d.count} ${plural(d.count, "файл", "файла", "файлов")}` : "";
  const st =
    d.status === "done" ? L("Готово") + files :
    d.status === "failed" ? L("Ошибка: ") + d.error + files :
    d.stage === "convert" ? L("Обработка…") + files :
    d.percent ? `${d.percent.toFixed(1)}%` + files : L("Запуск…");
  const bar = el("div", "dl-bar");
  const fill = el("i");
  fill.style.width = (d.status === "done" ? 100 : d.percent || 0) + "%";
  bar.append(fill);
  const name = el("div", "dl-name", d.title || d.name);
  name.title = d.name;
  row.append(name, el("div", "dl-status", st), bar);
  return row;
}

/** Загрузки — над списком библиотеки; готовые можно убрать. */
function renderDownloads() {
  const box = $("#downloads");
  if (!box) return;
  box.replaceChildren();
  box.classList.toggle("card", S.downloads.size > 0);
  if (!S.downloads.size) return;
  const head = el("h3", "dl-head");
  head.append(el("span", null, L("Загрузки")));
  if ([...S.downloads.values()].some((d) => d.status !== "run")) {
    const clear = el("button", "nav link-btn", L("Убрать готовые"));
    clear.addEventListener("click", () => {
      for (const [id, d] of S.downloads) if (d.status !== "run") S.downloads.delete(id);
      renderDownloads();
    });
    head.append(clear);
  }
  box.append(head);
  for (const d of [...S.downloads.values()].reverse()) box.append(renderDownload(d));
}

function updateDownload(d) {
  const old = document.querySelector(`.dl[data-id="${d.id}"]`);
  if (old) old.replaceWith(renderDownload(d));
  else renderDownloads();
  updateBadge();
}

function updateBadge() {
  const n = [...S.downloads.values()].filter((d) => d.status === "run").length;
  const b = $("#dlBadge");
  b.hidden = !n;
  b.textContent = n ? String(n) : "";
  b.title = n ? L("Скачивается: {0}", n) : "";
}

async function startDownload(input, name) {
  try {
    const id = await invoke("dl_start", { input });
    S.downloads.set(id, { id, name, title: "", percent: 0, stage: "download", status: "run", count: 0 });
    updateBadge();
    renderDownloads();
    if (S.page !== "library") toast(L("Скачиваю: {0}", name));
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

listen("dl://event", (ev) => {
  const e = ev.payload;
  const d = S.downloads.get(e.id);
  if (!d) return;
  if (e.type === "title") {
    d.title = e.title;
    d.percent = 0;
    d.stage = "download";
  } else if (e.type === "progress") {
    d.percent = e.percent;
    d.stage = e.stage;
  } else if (e.type === "saved") {
    d.count = e.count;
    reloadLocalSoon();
  } else if (e.type === "done") {
    d.status = "done";
    d.count = e.count;
    toast(e.count > 1 ? L("Скачано {0} {1}", e.count, plural(e.count, "трек", "трека", "треков")) : L("Скачано: {0}", d.title || d.name));
  } else if (e.type === "failed") {
    d.status = "failed";
    d.error = e.error;
    d.count = e.count;
    toast(L("Не скачалось «{0}»: {1}", d.title || d.name, L(e.error)), "error");
  }
  updateDownload(d);
});

// ───────────────────────── темы: как у Whisper ─────────────────────────
//
// Стиль — одна из основ Whisper (neon, royal, steam, lolitta: их CSS — в whisper.css) или тема .wmt. Тему разбирает
// ядро (WMT.open из файла, который отдаёт Rust — theme_file), CSS собирает движок тем ядра: @@ css whisper-core →
// @@ css whisper-music → @@ css whisper-vk (своё для VK Music, необязательно) → tokens, привязка к стилю
// :root[data-style="<id>"], фильтр безопасности, ресурсы темы — blob:. Цвета и эффекты из tokens — переменные основы.

const BASES = ["neon", "royal", "steam", "lolitta"];
/** Значения основ, как в Whisper: цвета, свечение, непрозрачность панелей, затемнение фона. */
const STYLE_DEF = {
  neon: { accent: "#a66bff", accent2: "#e060ff", glow: 22, panel: 0.7, dim: 0.1 },
  royal: { accent: "#c9a24b", accent2: "#8b1e2d", glow: 6, panel: 0.92, dim: 0.1 },
  steam: { accent: "#c58a4b", accent2: "#8c4f24", glow: 4, panel: 0.9, dim: 0.1 },
  lolitta: { accent: "#e2aab4", accent2: "#8c4a5c", glow: 4, panel: 0.94, dim: 0.1 },
};
/** Блоки, где основа осветляет второстепенный текст под тёмные стили: цвет темы там тоже главнее (как у Whisper). */
const MUTED_IN = ":is(.settings > .card, .stats .card, .main .list, .main .lastAlbum, .main .similar, .main .sub, .main > .status)";

const hexRgb = (h) => {
  const m = /^#?([0-9a-f]{6})$/i.exec(h || "");
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const relLum = (rgb) => rgb.map((v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)).reduce((a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i], 0);
const btnHi = (h) => {
  const c = hexRgb(h);
  return c ? "#" + c.map((v) => Math.round(v * 0.7 + 255 * 0.3).toString(16).padStart(2, "0")).join("") : h;
};
/** Цвет текста на кнопках, залитых вторым цветом (тёмный или белый — что читается лучше). */
function onAccent(a, b) {
  const DARK = "#0b0618", ca = hexRgb(a), cb = hexRgb(b);
  if (!ca || !cb) return { color: DARK, shadow: "none" };
  const spots = [ca, cb, ca.map((v, i) => Math.round((v + cb[i]) / 2))].map(relLum);
  const cr = (l1, l2) => (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
  const dark = Math.min(...spots.map((l) => cr(l, relLum(hexRgb(DARK))))), white = Math.min(...spots.map((l) => cr(l, 1)));
  const best = Math.max(dark, white);
  const shadow = best >= 4.5 ? "none" : dark >= white ? "0 0 5px rgba(255,255,255,.55)" : "0 1px 4px rgba(0,0,0,.75)";
  return { color: dark >= white ? DARK : "#ffffff", shadow };
}
const num = (v, lo, hi) => (typeof v === "number" && isFinite(v) ? Math.min(hi, Math.max(lo, v)) : null);

let themeSeq = 0;
/** r — ответ theme_render / theme_select: { id, name, base, file, background, obs }. */
async function applyTheme(r) {
  const seq = ++themeSeq;
  const base = BASES.includes(r.base) ? r.base : "neon";
  let css = "", urls = [], tokens = {}, settings = {};
  if (r.file) {
    try {
      const th = await WMT.open(new Uint8Array(await invoke("theme_file", { id: r.id })));
      const out = WhisperThemeCss.themeCss(th, r.id, "whisper-music", { core: "whisper-core", mutedIn: MUTED_IN, extra: "\n" + (th.css("whisper-vk") || "") });
      ({ css, urls } = out);
      tokens = th.tokens || {};
      settings = th.settings("whisper-music") || {};
    } catch (e) {
      toast(L("Тема не загрузилась: ") + errMsg(e), "error");
    }
  }
  if (seq !== themeSeq) return urls.forEach(URL.revokeObjectURL);
  (S.themeUrls || []).forEach(URL.revokeObjectURL);
  S.themeUrls = urls;
  S.theme = r;
  S.themeTokens = tokens;
  S.themeSettings = settings;
  $("#theme-css").textContent = css;
  const root = document.documentElement;
  // своя тема — свой стиль (её правила привязаны к нему); основа — для правил whisper.css, которые зависят от неё
  root.dataset.style = r.file ? r.id : base;
  root.dataset.base = base;
  root.dataset.theme = r.id;
  // фон: картинка — переменной --bg-image на #bg (так его рисуют темы), видео — #bgVideo внутри
  const v = $("#bgVideo");
  if (r.background && r.background.video) {
    if (v.getAttribute("src") !== r.background.url) v.src = r.background.url;
    v.hidden = false;
    if (!dynOn) v.play().catch(() => {}); // при фоне по обложке видео не видно
  } else {
    v.pause();
    v.removeAttribute("src");
    v.hidden = true;
  }
  applyStyleVars();
  saveBoot();
}

/** Снимок для boot.js: стиль, основа, цвет фона и акцент — следующий запуск сразу рисуется в них. */
let bootSaveT = 0;
function saveBoot() {
  clearTimeout(bootSaveT);
  bootSaveT = setTimeout(() => {
    const root = document.documentElement;
    if (root.classList.contains("booting")) return;
    let bg = getComputedStyle(document.body).backgroundColor;
    if (!bg || /^rgba\([^)]*,\s*0\)$/.test(bg) || bg === "transparent") bg = "";
    const accent = getComputedStyle(root).getPropertyValue("--accent").trim();
    try {
      localStorage.setItem("vkm_boot", JSON.stringify({ style: root.dataset.style || "", base: root.dataset.base || "", bg, accent }));
    } catch {}
  }, 300);
}
window.saveBoot = saveBoot;

/** Переменные основы из tokens темы (или значения основы), поверх — «тонкая настройка» пользователя. */
function applyStyleVars() {
  const r = document.documentElement.style, th = S.theme || {}, base = document.documentElement.dataset.base || "neon";
  const def = STYLE_DEF[base], c = (S.themeTokens && S.themeTokens.color) || {}, fx = (S.themeTokens && S.themeTokens.effects) || {};
  const hex = (x) => (/^#[0-9a-f]{6}$/i.test(x || "") ? x : null);
  const accent = hex(c.accent) || def.accent, accent2 = hex(c.accent2) || def.accent2;
  r.setProperty("--accent", accent);
  r.setProperty("--accent2", accent2);
  // старые переменные VK Music (rgb для rgba()) — их ещё зовут свои блоки app.css
  r.setProperty("--accent-rgb", hexRgb(accent).join(", "));
  r.setProperty("--accent2-rgb", hexRgb(accent2).join(", "));
  const oa = onAccent(btnHi(accent2), accent2);
  r.setProperty("--on-accent", oa.color);
  r.setProperty("--on-accent-shadow", base === "neon" ? oa.shadow : "none");
  const o = S.overrides || {};
  const glow = num(fx.glow, 0, 48) ?? def.glow;
  r.setProperty("--glow", (th.file || base === "neon" ? glow : def.glow) + "px");
  r.setProperty("--panel-a", String(o.panel ?? num(fx.panelOpacity, 0.2, 1) ?? def.panel));
  r.setProperty("--dim", String(o.dim ?? num(fx.dim, 0, 0.85) ?? def.dim));
  r.setProperty("--bgblur", (o.blur ?? num(fx.backgroundBlur, 0, 64) ?? 6) + "px");
  const bg = th.background && !th.background.video ? `url("${th.background.url}")` : "none";
  r.setProperty("--bg-image", bg);
  // тема без анимаций (settings whisper-music → features.animations = false) — как у Whisper
  const f = (S.themeSettings && S.themeSettings.features) || {};
  document.documentElement.dataset.anim = f.animations === false ? "off" : "on";
  applyFont();
  applyHooks();
}

// ───────────────────────── возможности тем сверх CSS (settings whisper-music), как у Whisper ─────────────────────────
// "controls": { "volume": "knob" } — громкость ручкой; "buttons": { "power": "wave", … } — кнопки-действия темы
// (#thBtns > button.thbtn[data-th]: где стоят и как выглядят — решает CSS темы; действие — только из списка ниже:
// тема может нажать кнопку приложения, но не больше).

const TH_ACTIONS = {
  wave: ["#waveBtn", "VK Микс"], play: ["#play", "Пуск/пауза"], next: ["#next", "Следующий"], prev: ["#prev", "Предыдущий"],
  like: ["#likeBtn", "В библиотеку"], mute: ["#volBtn", "Выключить звук"], shuffle: ["#shuffleBtn", "Перемешать"],
  repeat: ["#repeatBtn", "Повтор"], full: ["#cover", "Обложка и текст на весь экран"], more: ["#pMore", "Ещё"],
  "page:wave": ["#wave", "VK Микс"], "page:likes": ["#likes", "Библиотека"], "page:stats": ["#statsBtn", "Статистика"],
  "page:settings": ["#settingsBtn", "Настройки"], "page:profile": ["#profile", "Сервисы"],
  "vol-up": [null, "Громче"], "vol-down": [null, "Тише"],
  close: ["#wClose", "Закрыть"], minimize: ["#wMin", "Свернуть"], maximize: ["#wMax", "Развернуть"], drag: [null, "Перетащить окно"],
};
const TH_STATE = {
  wave: () => S.queueMix === "common" && !S.pl.paused && !S.pl.idle,
  play: () => !S.pl.paused && !S.pl.idle,
  like: () => !!S.current && !!libraryMatch(S.current),
  mute: () => S.muted || S.volume === 0,
  shuffle: () => S.shuffle,
  repeat: () => S.repeat !== "off",
};
const volKnob = () => !!(S.themeSettings && S.themeSettings.controls && S.themeSettings.controls.volume === "knob");
let thKey = "";
function applyHooks() {
  const de = document.documentElement, vb = $("#volBtn");
  de.toggleAttribute("data-vol-knob", volKnob());
  if (volKnob()) {
    vb.setAttribute("role", "slider");
    vb.setAttribute("aria-valuemin", "0");
    vb.setAttribute("aria-valuemax", "100");
    vb.setAttribute("aria-valuenow", String(S.volume));
  } else ["role", "aria-valuemin", "aria-valuemax", "aria-valuenow"].forEach((a) => vb.removeAttribute(a));
  const b = S.themeSettings && S.themeSettings.buttons;
  const list = b && typeof b === "object" && !Array.isArray(b)
    ? Object.entries(b).filter(([k, v]) => /^[a-z][a-z0-9-]{0,23}$/.test(k) && typeof v === "string" && Object.hasOwn(TH_ACTIONS, v)).slice(0, 16)
    : [];
  const key = JSON.stringify(list);
  if (key === thKey) return thSync();
  thKey = key;
  let box = $("#thBtns");
  if (!list.length) return box && box.remove();
  if (!box) {
    box = el("div", "thbtns");
    box.id = "thBtns";
    document.body.append(box);
  }
  box.replaceChildren(...list.map(([name, act]) => {
    const t = el("button", "thbtn");
    t.type = "button";
    t.dataset.th = name;
    t.dataset.act = act;
    t.title = L(TH_ACTIONS[act][1]);
    t.setAttribute("aria-label", t.title);
    if (act === "drag") t.addEventListener("mousedown", (e) => e.button === 0 && invoke("win_drag").catch(() => {}));
    t.addEventListener("click", () => {
      if (act === "drag") return;
      if (act === "vol-up" || act === "vol-down") return setVolume(S.volume + (act === "vol-up" ? 5 : -5));
      const target = $(TH_ACTIONS[act][0]);
      if (target && !target.disabled) target.click();
    });
    return t;
  }));
  thSync();
}
/** Состояние кнопок темы (.on) — по состоянию приложения. */
function thSync() {
  const box = $("#thBtns");
  if (!box) return;
  for (const t of box.children) {
    const act = t.dataset.act, src = TH_ACTIONS[act][0] && $(TH_ACTIONS[act][0]);
    t.classList.toggle("on", act.startsWith("page:") ? !!(src && src.classList.contains("active")) : !!(TH_STATE[act] && TH_STATE[act]()));
    t.disabled = !!(src && src.disabled);
  }
}

/** Громкость 0–100 (ручка темы, колесо над громкостью, кнопки темы «громче / тише»). */
let volSave = null;
function setVolume(v) {
  S.volume = Math.round(Math.min(100, Math.max(0, v)));
  S.muted = false;
  $("#vol").value = S.volume;
  syncVolume();
  invoke("player_volume", { volume: S.volume }).catch(() => {});
  clearTimeout(volSave);
  volSave = setTimeout(() => invoke("save_prefs", { volume: S.volume }).catch(() => {}), 500);
}

const OVERRIDES = [
  { key: "dim", label: "Затемнение фона", min: 0, max: 0.85, step: 0.01, fmt: (v) => Math.round(v * 100) + "%" },
  { key: "blur", label: "Размытие фона", min: 0, max: 40, step: 1, fmt: (v) => v + " px" },
  { key: "panel", label: "Непрозрачность панелей", min: 0.2, max: 1, step: 0.01, fmt: (v) => Math.round(v * 100) + "%" },
];
const applyOverrides = () => applyStyleVars();

/** Шрифт интерфейса из системы — поверх шрифтов темы (и для текста, и для заголовков). */
function applyFont() {
  const st = document.documentElement.style;
  // шрифт темы задаёт body (tokens.font.body) — выбранный пользователем главнее: прямо на body
  if (S.font) {
    const fam = `"${S.font.replace(/["\\]/g, "")}", system-ui, sans-serif`;
    st.setProperty("--font-display", fam);
    document.body.style.fontFamily = fam;
  } else {
    st.removeProperty("--font-display");
    document.body.style.removeProperty("font-family");
  }
}

async function setFont(name) {
  S.font = name || null;
  applyFont();
  try {
    await invoke("save_prefs", { font: name || "" });
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

function renderFontPicker() {
  const box = el("div", "font-picker");
  const sel = el("select", "field");
  const def = el("option", null, L("Как в теме"));
  def.value = "";
  sel.append(def);
  const preview = el("div", "font-preview", L("Съешь же ещё этих мягких французских булок, да выпей чаю — 0123456789"));
  const fill = () => {
    const list = S.fonts || [];
    if (S.font && !list.includes(S.font)) list.unshift(S.font);
    for (const f of list) {
      const o = el("option", null, f);
      o.value = f;
      o.style.fontFamily = `"${f}"`;
      if (f === S.font) o.selected = true;
      sel.append(o);
    }
  };
  if (S.fonts) fill();
  else {
    const wait = el("option", null, L("Загружаю список шрифтов…"));
    wait.disabled = true;
    sel.append(wait);
    invoke("fonts_list")
      .then((list) => {
        S.fonts = list;
        wait.remove();
        fill();
      })
      .catch((e) => {
        wait.textContent = L("Не удалось получить шрифты");
        toast(errMsg(e), "error");
      });
  }
  sel.addEventListener("change", () => setFont(sel.value));
  box.append(sel, preview);
  return box;
}

async function selectTheme(id) {
  // выбранная карточка подсвечивается сразу (тему из файла ещё разбирать), настройки — после применения:
  // раньше они перерисовывались до того, как applyTheme сменит S.theme, и подсветка оставалась на прошлом стиле
  for (const c of document.querySelectorAll(".stylecard")) c.classList.toggle("on", c.dataset.style === id);
  try {
    await applyTheme(await invoke("theme_select", { id }));
    if (S.lib) for (const t of S.lib) t.active = t.id === id;
    if (S.page === "settings") renderSettings();
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

async function installThemeBytes(bytes) {
  try {
    const info = await invoke("theme_install", bytes);
    toast(L("Тема «{0}» установлена", info.name));
    await selectTheme(info.id);
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

async function installThemePath(path) {
  try {
    const info = await invoke("theme_install_path", { path });
    toast(L("Тема «{0}» установлена", info.name));
    await selectTheme(info.id);
  } catch (e) {
    toast(errMsg(e), "error");
  }
}

// ───────────────────────── оформление: карточки вкладки «Оформление» ─────────────────────────

/** Основы Whisper — как стили на выбор (названия и подписи — как в Whisper Yandex). */
const BASE_CARDS = [
  ["neon", "Неон", "Свечение, стекло, прозрачные панели"],
  ["royal", "Королевский", "Золото, готика, знамёна"],
  ["steam", "Стимпанк", "Латунь, заклёпки, шестерёнки"],
  ["lolitta", "Lolitta", "Пудровый розовый, банты, жемчуг, камея"],
];

function styleCard(id, name, desc, author, preview, colors, removable) {
  const b = el("button", "stylecard" + (removable ? " custom" : ""));
  b.dataset.style = id;
  if (S.theme && S.theme.id === id) b.classList.add("on");
  const pv = el("span", "sprev " + (BASES.includes(id) ? "sp-" + id : "sp-custom"));
  if (preview) pv.style.backgroundImage = `url("${preview}")`;
  else if (colors.length) pv.style.background = `linear-gradient(135deg, ${colors.join(", ")})`;
  const t = el("b");
  t.append(el("span", "scauth", author || "Whisper Music"), el("span", "scname", name));
  b.append(pv, t);
  if (desc) b.append(el("small", null, desc));
  b.addEventListener("click", () => selectTheme(id));
  if (removable) {
    const x = el("span", "scdel", "✕");
    x.title = L("Удалить тему");
    x.setAttribute("role", "button");
    x.addEventListener("click", (ev) => {
      ev.stopPropagation();
      confirmClick(x, async () => {
        try {
          await invoke("theme_remove", { id });
          if (S.theme && S.theme.id === id) await applyTheme(await invoke("theme_render", { id: null }));
          renderSettings();
        } catch (e) {
          toast(errMsg(e), "error");
        }
      }, L("Удалить тему?"));
    });
    b.append(x);
  }
  return b;
}

function renderThemes() {
  const c = setCard("look", null);
  const head = el("div", "stylehead");
  // компактный вид — стили кружочками (как у Whisper Yandex): выбор запоминается, пока не выбирали — сам, когда стилей много
  const view = el("button", "stview");
  view.type = "button";
  view.innerHTML = '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><circle cx="6" cy="6" r="3" fill="currentColor"/><circle cx="18" cy="6" r="3" fill="currentColor"/><circle cx="6" cy="18" r="3" fill="currentColor"/><circle cx="18" cy="18" r="3" fill="currentColor"/></svg>';
  head.append(el("h3", null, L("Стиль оформления")), view);
  c.append(head);
  const pick = el("div", "stylepick");
  for (const [id, name, desc] of BASE_CARDS) pick.append(styleCard(id, L(name), L(desc), "Whisper Music", null, []));
  for (const t of S.themes || []) {
    pick.append(styleCard(t.id, t.name, t.description || "", t.author || "", t.preview, [t.accent, t.accent2].filter(Boolean), !t.builtin));
  }
  c.append(pick);
  const applyView = () => {
    let saved = null;
    try { saved = localStorage.getItem("vkm_style_view"); } catch {}
    const compact = saved ? saved === "compact" : pick.children.length >= 6;
    pick.classList.toggle("compact", compact);
    view.classList.toggle("on", compact);
    view.setAttribute("aria-pressed", String(compact));
    view.title = compact ? L("Обычный вид: карточки с описанием") : L("Компактный вид: стили кружочками");
    // в компактном виде описание прячется — остаётся во всплывающей подсказке
    for (const card of pick.children) if (!card.title) card.title = card.querySelector("small")?.textContent || "";
  };
  view.addEventListener("click", () => {
    try { localStorage.setItem("vkm_style_view", pick.classList.contains("compact") ? "full" : "compact"); } catch {}
    applyView();
  });
  applyView();
  const input = el("input");
  input.type = "file";
  input.accept = ".wmt,.zip";
  input.hidden = true;
  input.addEventListener("change", async () => {
    const f = input.files && input.files[0];
    if (f) await installThemeBytes(new Uint8Array(await f.arrayBuffer()));
    input.value = "";
  });
  const row = el("div", "row");
  row.append(button("download", L("Магазин тем"), "primary", () => go("store")), button("upload", L("Загрузить .wmt"), "", () => input.click()), input);
  c.append(row, el("p", "status stylecap", L("Файл темы .wmt можно перетащить прямо в окно.")));
  // список тем — с диска (свои могли появиться), карточка перерисуется
  invoke("themes_list").then((list) => {
    const same = JSON.stringify(list) === JSON.stringify(S.themes);
    S.themes = list;
    if (!same && S.page === "settings") renderSettings();
  }).catch((e) => toast(errMsg(e), "error"));

  const fc = setCard("look", L("Шрифт"));
  fc.append(renderFontPicker());

  const bc = setCard("look", L("Фон и панели"), "colbreak");
  bc.append(switchRow(L("Фон по обложке трека"), dynOn, (on) => {
    dynOn = on;
    try { localStorage.setItem(DYN_KEY, on ? "1" : "0"); } catch {}
    dynUpdate(S.current);
  }));
  for (const o of OVERRIDES) {
    const lab = el("label", "slide");
    const r = el("input");
    r.type = "range";
    r.min = o.min;
    r.max = o.max;
    r.step = o.step;
    const cs = getComputedStyle(document.documentElement);
    const cur = o.key in S.overrides ? S.overrides[o.key]
      : o.key === "blur" ? parseFloat(cs.getPropertyValue("--bgblur")) || 0
      : parseFloat(cs.getPropertyValue(o.key === "dim" ? "--dim" : "--panel-a")) || 0;
    r.value = cur;
    const out = el("span", null, `${L(o.label)} · ${o.fmt(Number(cur))}`);
    const fill = () => r.style.setProperty("--p", ((r.value - r.min) / (r.max - r.min)) * 100 + "%");
    fill();
    r.addEventListener("input", () => {
      S.overrides[o.key] = Number(r.value);
      out.textContent = `${L(o.label)} · ${o.fmt(Number(r.value))}`;
      fill();
      applyOverrides();
    });
    r.addEventListener("change", () => invoke("save_prefs", { overrides: S.overrides }).catch(() => {}));
    lab.append(out, r);
    bc.append(lab);
  }
  const rr = el("div", "row");
  rr.append(button(null, L("Как в теме"), "", async () => {
    S.overrides = {};
    applyOverrides();
    await invoke("save_prefs", { overrides: {} }).catch(() => {});
    renderSettings();
  }));
  bc.append(rr);
}

// ───────────────────────── библиотека тем (github.com/Whisper-Music/whisper-themes) ─────────────────────────

/** «Магазин тем» — отдельная страница, чтобы каталог не отодвигал настройки текущей темы. */
function renderStore() {
  // «назад» — стрелка в шапке (syncBack)
  const c = $("#content");
  c.replaceChildren();
  const libBox = el("div", "lib");
  libBox.id = "themeLib";
  c.append(libBox);
  renderThemeLib();
}

async function loadThemeLib() {
  S.libError = null;
  try {
    const list = await invoke("themelib_list", { fresh: !!S.libFresh });
    S.libFresh = false;
    S.lib = list.themes;
    S.libCats = list.categories || [];
  } catch (e) {
    S.lib = null;
    S.libError = errMsg(e);
  }
  renderThemeLib();
}

function fmtSize(n) {
  if (!n) return "";
  if (n >= 1073741824) return L("{0} ГБ", (n / 1073741824).toFixed(2));
  return n >= 1048576 ? L("{0} МБ", (n / 1048576).toFixed(1)) : L("{0} КБ", Math.max(1, Math.round(n / 1024)));
}

function renderThemeLib() {
  const box = $("#themeLib");
  if (!box) return;
  box.replaceChildren();

  const head = el("div", "lib-head");
  const filter = el("input", "search tlfind");
  filter.type = "search";
  filter.placeholder = L("Поиск по названию, автору, тегам…");
  filter.value = S.libFilter;
  filter.addEventListener("input", () => {
    S.libFilter = filter.value;
    renderLibGrid();
  });
  const refresh = iconButton("refresh", L("Обновить список"), () => {
    S.lib = null;
    S.libFresh = true; // мимо кэша каталога в Rust
    renderThemeLib();
  });
  head.append(filter, refresh);
  box.append(head);
  box.append(el("p", "status", L("Темы из репозитория whisper-themes: у VK Music та же разметка, что у Whisper, поэтому тема применяется целиком — цвета, шрифты, фон и оформление.")))

  const grid = el("div", "lib-groups");
  grid.id = "themeLibGrid";
  box.append(grid);

  if (S.lib === null && !S.libError) {
    grid.append(el("div", "spinner"));
    loadThemeLib();
    return;
  }
  renderLibGrid();
}

function renderLibGrid() {
  const grid = $("#themeLibGrid");
  if (!grid) return;
  grid.replaceChildren();
  if (S.libError) {
    const retry = el("button", "btn", L("Повторить"));
    retry.addEventListener("click", () => {
      S.libError = null;
      renderThemeLib();
    });
    grid.append(emptyState("x", L("Библиотека недоступна: ") + S.libError, retry));
    return;
  }
  const q = S.libFilter.trim().toLowerCase();
  const catName = (id) => (S.libCats.find((c) => c.id === id) || {}).name || "";
  const items = (S.lib || []).filter((t) =>
    !q || [t.name, t.author, t.artist, catName(t.category), t.description, t.id, ...(t.tags || [])]
      .join(" ").toLowerCase().includes(q)
  );
  if (!items.length) {
    grid.append(emptyState("search", q ? L("Ничего не нашлось.") : L("В библиотеке пока нет тем.")));
    return;
  }
  // по папкам репозитория в их порядке; темы без папки (старый формат каталога) — одной группой в конце
  const groups = S.libCats.map((c) => ({ cat: c, items: items.filter((t) => t.category === c.id) }));
  groups.push({ cat: null, items: items.filter((t) => !t.category || !S.libCats.some((c) => c.id === t.category)) });
  const shown = groups.filter((g) => g.items.length && !(g.cat && S.libHidden.includes(g.cat.id)));
  for (const g of shown) {
    const sec = el("section", "lib-cat");
    if (g.cat) {
      const head = el("div", "section-title with-icon lib-cat-head");
      head.append(el("span", null, g.cat.name), iconButton("hide", L("Скрыть категорию"), () => setLibHidden(g.cat.id, true)));
      sec.append(head);
      if (g.cat.description) sec.append(el("p", "hint", g.cat.description));
    }
    // темы одного артиста — вместе, под его именем
    const byArtist = new Map();
    for (const t of g.items) {
      const k = t.artist || "";
      if (!byArtist.has(k)) byArtist.set(k, []);
      byArtist.get(k).push(t);
    }
    for (const [artist, list] of byArtist) {
      if (artist) sec.append(el("div", "lib-artist", artist));
      const cards = el("div", "themes lib-grid");
      for (const t of list) cards.append(renderLibCard(t));
      sec.append(cards);
    }
    grid.append(sec);
  }
  if (!shown.length) grid.append(emptyState("hide", L("Все категории с такими темами скрыты.")));
  // скрытые категории — строкой внизу, нажатие возвращает
  const hidden = S.libCats.filter((c) => S.libHidden.includes(c.id));
  if (hidden.length) {
    const row = el("div", "lib-hidden");
    row.append(el("span", null, L("Скрыто:")));
    for (const c of hidden) {
      const b = button("hide", c.name, "lib-hidden-cat", () => setLibHidden(c.id, false));
      b.title = L("Показать категорию");
      row.append(b);
    }
    grid.append(row);
  }
}

/** Скрыть или показать категорию библиотеки тем (запоминается в config.json). */
function setLibHidden(id, hide) {
  S.libHidden = S.libHidden.filter((x) => x !== id).concat(hide ? [id] : []);
  invoke("save_prefs", { hidden_theme_categories: S.libHidden }).catch((e) => toast(errMsg(e), "error"));
  renderLibGrid();
}

function renderLibCard(t) {
  const card = el("div", "theme-card lib-card");
  if (t.active) card.classList.add("active");
  const prev = el("div", "theme-prev");
  if (t.preview_url) {
    const img = el("img");
    img.loading = "lazy";
    img.alt = "";
    img.src = t.preview_url;
    img.onerror = () => img.remove();
    prev.append(img);
  }
  const info = el("div", "theme-info");
  const name = el("div", "theme-name");
  if (t.author) name.append(el("span", "theme-author", t.author + " · "));
  name.append(document.createTextNode(t.name));
  info.append(name);
  card.append(prev, info);
  if (t.description) card.append(el("div", "theme-desc", t.description));

  const meta = el("div", "lib-meta");
  for (const tag of t.tags || []) meta.append(el("span", "lib-tag", tag));
  const ver = t.update && t.installed_version
    ? `v${t.installed_version} → v${t.version}`
    : t.version ? `v${t.version}` : "";
  const extra = [ver, fmtSize(t.size)].filter(Boolean).join(" · ");
  if (extra) meta.append(el("span", "lib-size", extra));
  card.append(meta);

  const actions = el("div", "lib-actions");
  const busy = S.libBusy.has(t.id);
  if (t.builtin_conflict) {
    actions.append(el("span", "lib-note", L("id занят встроенной темой")));
  } else if (!t.installed) {
    const b = el("button", "btn primary");
    b.append(Icons.icon("download"), el("span", null, busy ? L("Устанавливаю…") : L("Установить")));
    b.disabled = busy;
    b.addEventListener("click", () => libInstall(t));
    actions.append(b);
  } else {
    if (t.update) {
      const up = el("button", "btn primary");
      up.append(Icons.icon("refresh"), el("span", null, busy ? L("Обновляю…") : L("Обновить")));
      up.title = L("Обновить до v{0}", t.version);
      up.disabled = busy;
      up.addEventListener("click", () => libInstall(t));
      actions.append(up);
    } else if (t.active) {
      actions.append(el("span", "lib-note on", L("Включена")));
    } else {
      const use = el("button", "btn");
      use.textContent = L("Применить");
      use.disabled = busy;
      use.addEventListener("click", async () => {
        await selectTheme(t.id);
      });
      actions.append(use);
    }
    const del = el("button", "btn danger");
    del.append(Icons.icon("trash"), el("span", null, L("Удалить")));
    del.disabled = busy;
    del.addEventListener("click", () => libRemove(t));
    actions.append(del);
  }
  card.append(actions);
  return card;
}

async function libInstall(t) {
  S.libBusy.add(t.id);
  renderLibGrid();
  try {
    applyTheme(await invoke("themelib_install", { id: t.id }));
    toast(t.installed ? L("Тема «{0}» обновлена", t.name) : L("Тема «{0}» установлена", t.name));
  } catch (e) {
    toast(errMsg(e), "error");
  }
  S.libBusy.delete(t.id);
  if (S.page === "store") {
    S.lib = null;
    const keep = scroller().scrollTop;
    renderStore();
    scroller().scrollTop = keep;
  }
}

async function libRemove(t) {
  S.libBusy.add(t.id);
  renderLibGrid();
  try {
    await invoke("theme_remove", { id: t.id });
    if (S.theme && S.theme.id === t.id) applyTheme(await invoke("theme_render", { id: null }));
    toast(L("Тема «{0}» удалена", t.name));
  } catch (e) {
    toast(errMsg(e), "error");
  }
  S.libBusy.delete(t.id);
  if (S.page === "store") {
    S.lib = null;
    const keep = scroller().scrollTop;
    renderStore();
    scroller().scrollTop = keep;
  }
}

// перетаскивание .wmt в окно
listen("tauri://drag-enter", () => ($("#dropHint").hidden = false));
listen("tauri://drag-leave", () => ($("#dropHint").hidden = true));
listen("tauri://drag-drop", (ev) => {
  $("#dropHint").hidden = true;
  const paths = (ev.payload && ev.payload.paths) || [];
  const theme = paths.find((p) => /\.(wmt|zip)$/i.test(p));
  if (theme) installThemePath(theme);
  else if (paths.length) toast(L("Перетащите файл темы .wmt"), "error");
});
// в браузере (--host) событий Tauri о перетаскивании нет — файл темы читается сам и уходит сырым телом
if (HOST) {
  const files = (e) => e.dataTransfer && [...e.dataTransfer.types].includes("Files");
  let depth = 0;
  document.addEventListener("dragenter", (e) => {
    if (!files(e)) return;
    depth++;
    $("#dropHint").hidden = false;
  });
  document.addEventListener("dragleave", (e) => {
    if (files(e) && --depth <= 0) {
      depth = 0;
      $("#dropHint").hidden = true;
    }
  });
  document.addEventListener("dragover", (e) => files(e) && e.preventDefault());
  document.addEventListener("drop", async (e) => {
    if (!files(e)) return;
    e.preventDefault();
    depth = 0;
    $("#dropHint").hidden = true;
    const f = [...e.dataTransfer.files].find((x) => /\.(wmt|zip)$/i.test(x.name));
    if (f) installThemeBytes(new Uint8Array(await f.arrayBuffer()));
    else if (e.dataTransfer.files.length) toast(L("Перетащите файл темы .wmt"), "error");
  });
}

// ───────────────────────── плеер ─────────────────────────

function playList(items, i) {
  S.queue = items.slice();
  S.history = [];
  S.shuffleNext = null;
  S.queueMix = null;
  playAt(i);
}

/**
 * Индекс трека, который заиграет следующим (−1 — никакой). При перемешивании он выбирается заранее и
 * запоминается — тогда next() включит именно тот трек, что уже кэшируется.
 */
function upcoming() {
  const n = S.queue.length;
  if (!n || S.repeat === "one") return -1;
  if (S.shuffle && n > 1) return shufflePick();
  if (S.qi + 1 < n) return S.qi + 1;
  return S.repeat === "all" ? 0 : -1;
}

/** Случайный следующий трек (не текущий) — выбирается один раз до переключения. */
function shufflePick() {
  const n = S.queue.length;
  if (S.shuffleNext == null || S.shuffleNext === S.qi || S.shuffleNext >= n) {
    do S.shuffleNext = Math.floor(Math.random() * n);
    while (S.shuffleNext === S.qi);
  }
  return S.shuffleNext;
}

/** Текущий трек уже в кэше — кэшируем следующий в очереди (только трек библиотеки; проверяет Rust). */
function prefetchNext() {
  const t = S.queue[upcoming()];
  if (!t || t.unavailable || t === S.current) return;
  invoke("cache_prefetch", { key: t.key }).catch(() => {});
}

/**
 * fresh — ссылку на поток запросить заново, а не из кэша (кэшированная не открылась).
 * opts (ядро Whisper, «Слушать вместе»): { at — с какой секунды начать, hold — загрузить на паузе }.
 * Сначала — источники ядра (Whisper Archive: свой трек или архив ведущего комнаты), потом сам трек.
 */
async function playAt(i, fresh, opts) {
  let t = S.queue[i];
  if (!t) return;
  const at = (opts && opts.at) || 0, hold = !!(opts && opts.hold);
  const seq = ++S.playSeq;
  S.qi = i;
  S.current = t;
  S.retried = !!fresh;
  // состояния mpv о прошлом файле (номер file не вырос) больше не в счёт: ни позиция, ни «загрузился»
  Object.assign(S, { loading: true, startAt: at, needAudible: true, holdPending: hold, fileAtPlay: S.pl.file || 0 });
  Object.assign(S.pl, { pos: 0, duration: 0 });
  if (!t.room) startListen(t);
  updateNowPlaying();
  markPlaying();
  // ядру Whisper: новый трек (повтор со свежей ссылкой — не новое включение, в историю не идёт)
  Whisper.player.report({ track: t, queue: true, ...(fresh ? {} : { started: t }) });
  try {
    const title = `${t.artist} — ${t.title}`;
    if (at > 0 || hold) await invoke("player_cue", { at, hold });
    const nx = S.queue[upcoming()];
    const hit = await Whisper.sources.resolve(t, nx && nx !== t ? nx : null).catch(() => null);
    if (seq !== S.playSeq) return;
    if (hit) {
      try {
        await invoke("wa_play", { id: hit.id, title });
        if (t.room) startListen(t);
        Whisper.player.report({ source: hit.source });
        return;
      } catch (e) {
        if (seq !== S.playSeq) return;
        // архив не отдал файл — играет сам трек
      }
    }
    // трек комнаты «Слушать вместе», которого нет в библиотеке: один поиск ВКонтакте
    if (t.room) {
      const r = await window.resolveRoomTrack(t).catch(() => null);
      if (seq !== S.playSeq) return;
      if (!r) throw L("Трека «{0}» нет ни в вашей библиотеке, ни во ВКонтакте", t.title);
      S.queue[i] = S.current = t = r;
      startListen(t);
      updateNowPlaying();
    }
    Whisper.player.report({ source: t.path ? "local" : "provider" });
    let caching;
    if (t.service === "vk" && t.api && !t.reload) caching = await invoke("vk_play_api", { id: t.id, title, fresh: !!fresh });
    else if (t.service === "vk") caching = await invoke("vk_play", { reload: t.reload, title, fresh: !!fresh });
    else if (t.service === "sc") caching = await invoke("sc_play", { id: t.id, title, fresh: !!fresh });
    else caching = await invoke("player_play", { src: t.path, title });
    // текущий уже в кэше (или не из библиотеки) — следующий сразу, иначе — после cache://event
    if (!caching && S.current === t) prefetchNext();
    // VK Микс бесконечный: очередь подходит к концу — догружаем следующие треки
    if (S.queueMix && S.qi >= S.queue.length - 2) extendMix();
  } catch (e) {
    if (seq === S.playSeq) S.loading = false;
    toast(errMsg(e), "error");
  }
}

/** Скорость воспроизведения (ядро Whisper: «Слушать вместе» выравнивает её по ведущему). */
function setRate(r) {
  S.rate = Math.min(4, Math.max(0.25, Number(r) || 1));
  invoke("player_rate", { rate: S.rate }).catch(() => {});
  Whisper.player.report({ rate: S.rate });
}

function next(auto) {
  // «Слушать вместе»: режим «Ставить в очередь» успевает поставить предложенный трек следующим
  const room = Whisper.modules.together;
  if (room && room.take) room.take();
  if (!S.queue.length) return;
  if (auto && S.repeat === "one") return playAt(S.qi);
  let i;
  if (S.shuffle && S.queue.length > 1) {
    S.history.push(S.qi);
    i = shufflePick();
    S.shuffleNext = null;
  } else {
    i = S.qi + 1;
    if (i >= S.queue.length) {
      if (S.repeat === "all") i = 0;
      else if (auto) {
        S.pl.paused = true;
        updatePlayButton();
        return;
      } else return;
    }
  }
  playAt(i);
}

function prev() {
  if (S.pl.pos > 3) return seekTo(0);
  if (S.shuffle && S.history.length) return playAt(S.history.pop());
  if (S.qi > 0) playAt(S.qi - 1);
  else seekTo(0);
}

/** Перемотка (и отчёт ядру: «Слушать вместе», Discord и т. п. узнают о скачке позиции). */
function seekTo(pos) {
  invoke("player_seek", { pos }).catch(() => {});
  Whisper.player.report({ seek: pos });
}

function togglePlay() {
  if (!S.current) {
    const items = visibleTracks().filter((t) => !t.unavailable);
    if (items.length) playList(items, 0);
    return;
  }
  if (S.pl.idle && !S.loading) return playAt(S.qi);
  // явное состояние, а не «переключить»: кнопка делает то, что показывает (cycle pause иногда не срабатывал
  // с первого раза — нажатие во время загрузки или после запоздавшего события)
  const paused = !S.pl.paused;
  S.pl.paused = paused;
  updatePlayButton();
  invoke("player_pause", { paused }).catch((e) => toast(errMsg(e), "error"));
}

// ───────────────────────── правая панель и статистика прослушивания ─────────────────────────

/** Прослушивание текущего трека: накопленные и ещё не отправленные в Rust секунды. */
const heard = { key: null, acc: 0, lastPos: null, stat: null, prevLast: 0 };
/** Отправлять секунды порциями — не реже раза в столько секунд прослушивания. */
const STATS_CHUNK = 10;

/** Играющий трек на весь экран (#full, как у Whisper) и карточка «Сейчас играет» в библиотеке. */
function updateNpSide() {
  const t = S.current;
  $("#fTitle").textContent = t ? t.title : L("Ничего не играет");
  $("#fArtist").textContent = t ? t.artist : "";
  const src = !t ? "" : t.service ? SERVICES[t.service].name : quality(t);
  $("#fInfo").textContent = t ? [src, t.duration ? fmt(t.duration) : ""].filter(Boolean).join(" · ") : "";
  const img = $("#fCover");
  if (t) setCover(img, t.cover);
  else img.removeAttribute("src");
  $("#fBg").style.backgroundImage = t && t.cover ? `url("${t.cover}")` : "none";
  $("#fLyrBtn").disabled = !t;
  if (!t && !$("#full").hidden) closeFull();
  renderNowCard();
  renderNpStats();
  updateLyrics();
}

/** Обложка и текст на весь экран: mode "cover" | "lyrics". Текст ищется, только пока он на экране. */
function openFull(mode) {
  if (!S.current) return;
  const f = $("#full");
  f.hidden = false;
  f.dataset.mode = mode === "lyrics" ? "lyrics" : "cover";
  $("#fLyrBtn").classList.toggle("active", f.dataset.mode === "lyrics");
  closeTrackMenuSafe();
  updateLyrics();
}
function closeFull() {
  $("#full").hidden = true;
  closeNote();
}
const closeTrackMenuSafe = () => window.Whisper && Whisper.ui.kit.closeMenu && Whisper.ui.kit.closeMenu();

// ───────────────────────── текст песни (Genius) ─────────────────────────

/**
 * Текст играющего трека: для какого трека, ответ Rust (null — ещё ищем), таймер задержки поиска,
 * выбранный перевод (null — оригинал) и загруженные тексты переводов.
 */
const lyr = { key: null, data: null, error: null, timer: null, lang: null, trans: new Map(), transError: null };
/** Искать текст через столько после включения трека — пролистанные не ищем. */
const LYRICS_DELAY = 1500;
/** Пометки фрагментов с пояснениями в тексте от Rust: U+E000 id U+E001 фрагмент U+E002. */
const ANN_RE = /(\d+)([\s\S]*?)/g;
/** Пояснения, уже загруженные в этом запуске: id → текст. */
const notes = new Map();

// ───────────────────────── левая и правая панели ─────────────────────────

/** Левую панель свернула кнопка правой панели (а не пользователь) — спрятали правую, разворачиваем обратно. */
/** Текст песни сейчас на экране (полноэкранный плеер в виде «Текст»): только тогда — в сеть. */
function npShown() {
  return !$("#full").hidden && $("#full").dataset.mode === "lyrics";
}

/** Название языка перевода по-русски: «английский», «романизация». */
function langName(code) {
  if (code === "romanization") return L("Романизация");
  try {
    const n = new Intl.DisplayNames([I18N.lang], { type: "language" }).of(code === "iw" ? "he" : code);
    return n ? n[0].toUpperCase() + n.slice(1) : code;
  } catch (e) {
    return code;
  }
}

/** Последний выбранный язык текста ("" — оригинал): на следующих треках выставляется сразу. */
let lyricsPrefLang = localStorage.getItem("lyricsLang") || "";

function updateLyrics() {
  const t = S.current;
  if (!t) return;
  if (lyr.key !== t.key) {
    clearTimeout(lyr.timer);
    closeNote();
    Object.assign(lyr, { key: t.key, data: null, error: null, timer: null, checking: false, lang: null, trans: new Map(), transError: null, metaPending: false });
  }
  if (!lyr.data && !lyr.error && !lyr.timer && !lyr.checking && npShown()) {
    const key = t.key;
    // у файла без тега исполнителя — «Неизвестный исполнитель»: ищем только по названию
    const artist = t.noArtist ? "" : t.artist, title = t.title;
    lyr.checking = true;
    // сначала — кэш на диске: мгновенно и без сети
    invoke("lyrics_cached", { artist, title })
      .catch(() => null)
      .then(async (d) => {
        if (lyr.key !== key) return;
        lyr.checking = false;
        if (d) {
          lyr.data = d;
          await applyPrefLang(key);
          if (lyr.key !== key) return;
          renderLyrics();
          fetchLyricsMeta(key, artist, title);
          return;
        }
        // в кэше нет — к ВК / Genius, после задержки (пролистанные треки не ищем)
        const vkId = t.service === "vk" ? t.id : null;
        lyr.timer = setTimeout(() => fetchLyrics(key, artist, title, vkId), LYRICS_DELAY);
        renderLyrics();
      });
  }
  renderLyrics();
}

/** Текст с Genius (не было в кэше). */
async function fetchLyrics(key, artist, title, vkId) {
  try {
    // vk_id — для трека ВК: при поиске «как в VK MP3 Mod» сначала спрашивается текст самого ВК
    const data = await invoke("lyrics_get", { artist, title, vk_id: vkId || null });
    if (lyr.key === key) lyr.data = data;
  } catch (e) {
    if (lyr.key === key) lyr.error = errMsg(e);
  }
  if (lyr.key !== key) return;
  if (lyr.data) await applyPrefLang(key);
  if (lyr.key !== key) return;
  renderLyrics();
  fetchLyricsMeta(key, artist, title);
}

/** Переводы и язык оригинала — следующим запросом, если их ещё нет в кэше (текст уже на экране). */
async function fetchLyricsMeta(key, artist, title) {
  const d = lyr.data;
  if (!d || d.status !== "found" || d.meta) return;
  try {
    const full = await invoke("lyrics_meta", { artist, title });
    if (lyr.key === key && lyr.data === d && (full.translations || []).length + (full.language ? 1 : 0)) {
      Object.assign(d, { translations: full.translations, language: full.language, meta: true });
      await applyPrefLang(key);
      if (lyr.key !== key) return;
      if (!noteFor) renderLyrics();
      else lyr.metaPending = true;
    }
  } catch (e) {
    // без переводов — текст уже показан
  }
}

/**
 * Последний выбранный язык: перевод в кэше — выставляем сразу (до отрисовки, без мелькания оригинала);
 * нет в кэше — показываем оригинал, а перевод подгружаем и подставляем, если язык за это время не меняли.
 */
async function applyPrefLang(key) {
  const d = lyr.data;
  const code = lyricsPrefLang;
  if (!code || lyr.lang || !d || d.status !== "found") return;
  const tr = (d.translations || []).find((x) => x.language === code);
  if (!tr) return;
  if (lyr.trans.has(code)) {
    lyr.lang = code;
    return;
  }
  const cached = await invoke("lyrics_translation_cached", { url: tr.url }).catch(() => null);
  if (lyr.key !== key) return;
  if (cached) {
    lyr.trans.set(code, cached);
    lyr.lang = code;
    return;
  }
  invoke("lyrics_translation", { url: tr.url })
    .then((text) => {
      if (lyr.key !== key) return;
      lyr.trans.set(code, text);
      // пока грузилось, язык не выбирали руками — подставляем
      if (!lyr.lang && lyricsPrefLang === code) {
        lyr.lang = code;
        if (!noteFor) renderLyrics();
        else lyr.metaPending = true;
      }
    })
    .catch(() => {});
}

/** Выбрать перевод (null — оригинал); текст перевода грузится один раз. */
async function selectLyricsLang(code) {
  const d = lyr.data;
  closeNote();
  // запоминаем: на следующих треках этот язык выставится сам
  lyricsPrefLang = code || "";
  localStorage.setItem("lyricsLang", lyricsPrefLang);
  // текст ещё не пришёл (или это не тот трек) — выбирать нечего
  if (code && !(d && (d.translations || []).some((x) => x.language === code))) {
    lyr.lang = null;
    return renderLyrics();
  }
  lyr.lang = code;
  lyr.transError = null;
  const tr = code && d ? (d.translations || []).find((x) => x.language === code) : null;
  if (tr && !lyr.trans.has(code)) {
    const key = lyr.key;
    renderLyrics();
    try {
      const text = await invoke("lyrics_translation", { url: tr.url });
      if (lyr.key === key) lyr.trans.set(code, text);
    } catch (e) {
      if (lyr.key === key) lyr.transError = errMsg(e);
    }
    if (lyr.key !== key) return;
  }
  renderLyrics();
}

/**
 * Настройки «Текст песен»: без строк частей песни ([Куплет], [Припев]…) и/или без пустых строк между частями.
 * Пометки пояснений (U+E000…U+E002) на это не влияют: строка-заголовок проверяется без них.
 */
function tidyLyrics(text) {
  if (!S.lyrHideParts && !S.lyrCompact) return text;
  const bare = (line) => line.replace(/\uE000\d+\uE001|\uE002/g, "").trim();
  return text
    .split("\n")
    .filter((line) => !(S.lyrHideParts && /^\[.*\]$/.test(bare(line))) && !(S.lyrCompact && !bare(line)))
    .join("\n")
    .replace(/^\n+/, "");
}

/** Показывать пояснения Genius (галочка возле выбора языка, запоминается; по умолчанию — да). */
let lyricsNotes = localStorage.getItem("lyricsNotes") !== "0";

/** Текст с пометками → узлы: [Куплет] приглушённо, фрагменты с пояснением — кликабельные (если включены). */
function lyricsNodes(text, box) {
  const plain = (str) => {
    str.split("\n").forEach((line, i) => {
      if (i) box.append("\n");
      if (/^\[.*\]$/.test(line.trim())) box.append(el("span", "lyr-tag", line));
      else if (line) box.append(line);
    });
  };
  let last = 0;
  for (const m of text.matchAll(ANN_RE)) {
    plain(text.slice(last, m.index));
    if (!lyricsNotes) {
      plain(m[2]);
      last = m.index + m[0].length;
      continue;
    }
    const span = el("span", "lyr-ann", m[2]);
    span.dataset.id = m[1];
    span.title = L("Пояснение Genius");
    span.addEventListener("click", (ev) => {
      ev.stopPropagation();
      openNote(span);
    });
    box.append(span);
    last = m.index + m[0].length;
  }
  plain(text.slice(last));
}

function renderLyrics() {
  closeNote(); // фрагменты пересоздаются — всплывашка от старого фрагмента не нужна
  const text = $("#fLyrics");
  const src = $("#fGenius");
  const sel = $("#fLang");
  const d = lyr.data;
  text.replaceChildren();
  // выбор перевода — только если переводы есть
  const trans = d && d.status === "found" ? d.translations || [] : [];
  sel.hidden = !trans.length;
  if (trans.length) {
    // оригинал — первым, подписан своим языком
    sel.replaceChildren(el("option", null, d.language ? langName(d.language) : L("Оригинал")));
    sel.firstChild.value = "";
    for (const t of trans) {
      const o = el("option", null, langName(t.language));
      o.value = t.language;
      sel.append(o);
    }
    sel.value = lyr.lang || "";
  }
  src.hidden = !(d && (d.url || d.source === "vk"));
  src.disabled = false;
  if (d && d.source === "vk") {
    // текст от самого ВК: ссылки нет — только подпись (и правообладатель, если ВК его назвал)
    src.textContent = L("ВКонтакте");
    src.title = d.credits ? L("Текст предоставлен {0}", d.credits) : L("Текст из ВКонтакте");
    src.onclick = null;
    src.disabled = true;
  } else if (d && d.url) {
    const tr = lyr.lang && trans.find((x) => x.language === lyr.lang);
    const url = tr ? tr.url : d.url;
    src.textContent = "Genius ↗";
    src.title = L("{0}: открыть на Genius", tr ? tr.title : `${d.song_artist} — ${d.song_title}`);
    src.onclick = () => invoke("lyrics_open", { url }).catch((e) => toast(errMsg(e), "error"));
  }
  let body = null;
  if (lyr.error) body = L("Текст недоступен: ") + lyr.error;
  else if (!d) body = lyr.checking || !npShown() ? "" : L("Ищу текст на Genius…");
  else if (d.status === "instrumental") body = L("Инструментал — без слов.");
  else if (d.status !== "found") body = L("Текст не нашёлся на Genius.");
  else if (lyr.lang && lyr.transError) body = L("Перевод недоступен: ") + lyr.transError;
  else if (lyr.lang && !lyr.trans.has(lyr.lang)) body = L("Загружаю перевод…");
  text.classList.toggle("note", body != null);
  text.classList.add("plain");
  const shown = body == null ? tidyLyrics(lyr.lang ? lyr.trans.get(lyr.lang) : d.text) : "";
  // галочка пояснений — только если в показанном тексте они есть
  $("#fNotesBox").hidden = !/\uE000/.test(shown);
  $("#fNotes").checked = lyricsNotes;
  if (body != null) text.textContent = body;
  else lyricsNodes(shown, text);
  const view = `${lyr.key}|${lyr.lang || ""}|${body != null}`;
  if (lyr.renderedView !== view) {
    text.scrollTop = 0;
    lyr.renderedView = view;
    // новый текст (трек, перевод) — проявляется
    text.classList.remove("lyr-in");
    void text.offsetWidth;
    text.classList.add("lyr-in");
  }
}

// ───────────────────────── пояснения Genius: всплывашка над фрагментом ─────────────────────────

let noteFor = null;

function closeNote() {
  const pop = $("#lyrNote");
  if (pop) pop.hidden = true;
  if (noteFor) noteFor.classList.remove("open");
  noteFor = null;
  // пока было открыто пояснение, пришли переводы — дорисовываем выбор языка сейчас
  if (lyr.metaPending) {
    lyr.metaPending = false;
    renderLyrics();
  }
}

/** Показать пояснение над фрагментом (под ним — если сверху не хватает места). */
async function openNote(span) {
  if (noteFor === span) return closeNote();
  closeNote();
  noteFor = span;
  span.classList.add("open");
  const pop = $("#lyrNote");
  const id = Number(span.dataset.id);
  const show = (txt, cls) => {
    if (noteFor !== span) return;
    pop.className = "lyr-note" + (cls ? " " + cls : "");
    pop.textContent = txt;
    pop.hidden = false;
    placeNote(span, pop);
  };
  if (notes.has(id)) return show(notes.get(id));
  show(L("Загружаю пояснение…"), "loading");
  try {
    const txt = await invoke("lyrics_note", { id });
    notes.set(id, txt);
    show(txt);
  } catch (e) {
    show(errMsg(e), "loading");
  }
}

function placeNote(span, pop) {
  const box = $("#lyrNote").offsetParent.getBoundingClientRect();
  const r = (span.getClientRects()[0] || span.getBoundingClientRect());
  const w = pop.offsetWidth, h = pop.offsetHeight;
  let left = r.left - box.left + r.width / 2 - w / 2;
  left = Math.max(0, Math.min(left, box.width - w));
  let top = r.top - box.top - h - 8;
  pop.classList.toggle("below", top < 0);
  if (top < 0) {
    const last = [...span.getClientRects()].pop() || r;
    top = last.bottom - box.top + 8;
  }
  pop.style.left = `${left}px`;
  pop.style.top = `${top}px`;
}

/** Сколько слушали играющий трек — в карточке «Сейчас играет» и на весь экран. */
function renderNpStats() {
  const box = $("#nowStat");
  const st = heard.stat;
  const ok = !!S.current && !!st && heard.key === S.current.key;
  const lines = [];
  if (ok) {
    lines.push(`${st.plays} ${plural(st.plays, "раз дослушан", "раза дослушан", "раз дослушан")} · ${L("слушали {0}", fmtListened(st.seconds + heard.acc))}`);
    if (st.rank) lines.push(L("#{0} из {1} по времени прослушивания", st.rank, st.total));
    // «последний раз» — до этого включения: сейчас трек играет, его last уже «сейчас»
    if (heard.prevLast) lines.push(L("Последний раз — {0}", fmtWhen(heard.prevLast)));
    else if (!st.first && !st.seconds) lines.push(L("Слушаете впервые"));
  }
  if (box) box.replaceChildren(...lines.map((x) => el("small", null, x)));
}

/** Когда: «сегодня в 10:05», «вчера в 23:10», «12 марта», «12 марта 2025» (dateOnly — без времени). */
function fmtWhen(unix, dateOnly) {
  const d = new Date(unix * 1000), now = new Date();
  const day = (x) => localDay(x);
  const yest = new Date(now);
  yest.setDate(now.getDate() - 1);
  const hm = d.toLocaleTimeString(I18N.locale, { hour: "2-digit", minute: "2-digit" });
  if (day(d) === day(now)) return dateOnly ? L("сегодня") : L("сегодня в {0}", hm);
  if (day(d) === day(yest)) return dateOnly ? L("вчера") : L("вчера в {0}", hm);
  const opts = { day: "numeric", month: "long" };
  if (d.getFullYear() !== now.getFullYear()) opts.year = "numeric";
  return d.toLocaleDateString(I18N.locale, opts).replace(/ г\.$/, "");
}

/** Местная дата ГГГГ-ММ-ДД. */
function localDay(d) {
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Отправить накопленные секунды (completed — трек доигран до конца). */
async function flushListen(completed) {
  const key = heard.key;
  if (!key || (!completed && heard.acc < 1)) return;
  const seconds = heard.acc;
  heard.acc = 0;
  try {
    const now = new Date();
    const st = await invoke("stats_add", { key, seconds, completed: !!completed, day: localDay(now), hour: now.getHours() });
    if (heard.key === key) heard.stat = st;
    if (S.page === "stats") reloadStatsSoon();
  } catch (e) {
    if (heard.key === key) heard.acc += seconds;
  }
  renderNpStats();
}

/** Включён трек: прежний — дописать в статистику, у нового — загрузить статистику. */
function startListen(t) {
  if (heard.key === t.key) return; // повтор того же трека (fresh) — счёт продолжается
  flushListen(false);
  Object.assign(heard, { key: t.key, acc: 0, lastPos: null, stat: null, prevLast: 0 });
  invoke("stats_get", { key: t.key })
    .then((st) => {
      if (heard.key === t.key) Object.assign(heard, { stat: st, prevLast: st.last });
      renderNpStats();
    })
    .catch(() => {});
}

/** Событие плеера state: прибавить прослушанное время (без пауз и перемоток). */
function trackListen(e) {
  if (!S.current || e.idle || e.paused) {
    heard.lastPos = null;
    return;
  }
  if (heard.lastPos != null) {
    const d = e.pos - heard.lastPos;
    if (d > 0 && d < 1.5) heard.acc += d;
  }
  heard.lastPos = e.pos;
  if (heard.acc >= STATS_CHUNK) flushListen(false);
  else renderNpStats();
}

// ───────────────────────── динамический фон по обложке (как у Whisper Yandex) ─────────────────────────
// Обложка превращается в blob: — так её можно и показать, и разобрать на цвета (картинку с CDN холст прочитать
// не даёт: её отдаёт Rust — cover_bytes, только адреса CDN ВКонтакте / SoundCloud; обложку файла — vkm://, там
// Access-Control-Allow-Origin). Слоёв обложки два: новая проявляется поверх старой; над ними — аура из цветов
// обложки, зерно и виньетка (whisper.css, .dynbg). Включается в «Оформление → Фон и панели» (vkm_dyn).
const DYN_KEY = "vkm_dyn";
let dynOn = (() => { try { return localStorage.getItem(DYN_KEY) === "1"; } catch { return false; } })();
let dynKey = "", dynSeq = 0, dynFront = 0;
const dynCache = new Map(); // адрес обложки → { url, soft, echo, pal }

// Размытая и подкрашенная копия — один раз на маленьком холсте (коробочное размытие: у холста WebKitGTK нет ctx.filter)
function boxBlur(d, w, h, r) {
  const t = new Float32Array(d.length), n = 2 * r + 1;
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let c = 0; c < 4; c++) {
    let sum = 0;
    for (let k = -r; k <= r; k++) sum += d[(y * w + Math.min(w - 1, Math.max(0, x + k))) * 4 + c];
    t[(y * w + x) * 4 + c] = sum / n;
  }
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let c = 0; c < 4; c++) {
    let sum = 0;
    for (let k = -r; k <= r; k++) sum += t[(Math.min(h - 1, Math.max(0, y + k)) * w + x) * 4 + c];
    d[(y * w + x) * 4 + c] = sum / n;
  }
}
async function dynSoft(img, side, out, r, sat, bri) {
  const a = document.createElement("canvas");
  a.width = a.height = side;
  const g = a.getContext("2d", { willReadFrequently: true });
  g.drawImage(img, 0, 0, side, side);
  const id = g.getImageData(0, 0, side, side), d = id.data;
  for (let i = 0; i < d.length; i += 4) {
    const lum = 0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2];
    for (let c = 0; c < 3; c++) d[i + c] = Math.min(255, Math.max(0, (lum + (d[i + c] - lum) * sat) * bri));
  }
  boxBlur(d, side, side, r);
  boxBlur(d, side, side, r);
  g.putImageData(id, 0, 0);
  const b = document.createElement("canvas");
  b.width = b.height = out;
  const h = b.getContext("2d", { willReadFrequently: true });
  h.drawImage(a, 0, 0, out, out);
  const id2 = h.getImageData(0, 0, out, out);
  boxBlur(id2.data, out, out, Math.max(1, Math.round(out / side)));
  h.putImageData(id2, 0, 0);
  const blob = await new Promise((res) => b.toBlob(res, "image/png"));
  return blob ? URL.createObjectURL(blob) : "";
}
function rgb2hsl(r, g, b) {
  r /= 255; g /= 255; b /= 255;
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b), l = (mx + mn) / 2;
  let h = 0, sat = 0;
  if (mx !== mn) {
    const d = mx - mn;
    sat = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn);
    h = (mx === r ? (g - b) / d + (g < b ? 6 : 0) : mx === g ? (b - r) / d + 2 : (r - g) / d + 4) * 60;
  }
  return [h, sat, l];
}
function hsl2hex(h, sat, l) {
  sat = Math.min(1, Math.max(0, sat)); l = Math.min(1, Math.max(0, l));
  const a = sat * Math.min(l, 1 - l);
  const f = (n) => { const k = (n + h / 30) % 12; return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)))).toString(16).padStart(2, "0"); };
  return "#" + f(0) + f(8) + f(4);
}
/** Два главных цвета обложки (по оттенкам, с весом насыщенности). */
function paletteOf(img) {
  const N = 64, c = document.createElement("canvas");
  c.width = c.height = N;
  const g = c.getContext("2d", { willReadFrequently: true });
  g.drawImage(img, 0, 0, N, N);
  const d = g.getImageData(0, 0, N, N).data;
  const bins = Array.from({ length: 24 }, (_, i) => ({ h: i * 15 + 7.5, w: 0, s: 0 }));
  let sumS = 0, n = 0;
  for (let i = 0; i < d.length; i += 4) {
    if (d[i + 3] < 128) continue;
    const [h, sat, l] = rgb2hsl(d[i], d[i + 1], d[i + 2]);
    sumS += sat; n++;
    const w = sat * (1 - Math.abs(2 * l - 1)), b = bins[Math.floor(h / 15) % 24];
    b.w += w; b.s += sat * w;
  }
  if (!n || sumS / n < 0.1) return { a: "#e8e8f4", b: "#9a9ab4" };
  const sorted = bins.filter((b) => b.w > 0).sort((x, y) => y.w - x.w), first = sorted[0];
  const dist = (x, y) => { const v = Math.abs(x - y) % 360; return Math.min(v, 360 - v); };
  const second = sorted.find((b) => b !== first && dist(b.h, first.h) >= 45 && b.w >= first.w * 0.25);
  const satOf = (b) => Math.min(0.95, Math.max(0.6, b.s / b.w));
  return { a: hsl2hex(first.h, satOf(first), 0.64), b: second ? hsl2hex(second.h, satOf(second), 0.62) : hsl2hex((first.h + 45) % 360, 0.8, 0.66) };
}
async function dynArt(src) {
  let it = dynCache.get(src);
  if (it) return it;
  const bytes = /^https:\/\//.test(src) ? await invoke("cover_bytes", { url: src }) : await (await fetch(src)).arrayBuffer();
  const url = URL.createObjectURL(new Blob([bytes]));
  const img = new Image();
  img.src = url;
  await img.decode();
  let pal = null, soft = "", echo = "";
  try { pal = paletteOf(img); } catch {}
  try {
    soft = await dynSoft(img, 40, 160, 2, 1.5, 0.72); // большие «дышащие» слои — сильное размытие
    echo = await dynSoft(img, 72, 216, 1, 1.4, 1);    // «эхо» обложки — помягче
  } catch {}
  it = { url, soft: soft || url, echo: echo || url, pal };
  dynCache.set(src, it);
  while (dynCache.size > 16) {
    const [k, v] = dynCache.entries().next().value;
    if (k === dynKey) break;
    for (const u of [v.url, v.soft, v.echo]) if (u) URL.revokeObjectURL(u);
    dynCache.delete(k);
  }
  return it;
}
function dynShow(it) {
  const box = $("#dynBg"), ls = box.querySelectorAll(".dyn-l");
  if (ls[dynFront].dataset.url !== it.url) {
    const next = ls[1 - dynFront];
    next.style.backgroundImage = `url("${it.soft}")`;
    next.dataset.url = it.url;
    next.classList.remove("on");
    void next.offsetWidth;
    next.classList.add("on");
    ls[dynFront].classList.remove("on");
    dynFront = 1 - dynFront;
    const [art, echo] = box.querySelectorAll(".dyn-art i");
    art.style.backgroundImage = `url("${it.url}")`;
    echo.style.backgroundImage = `url("${it.echo}")`;
  }
  if (it.pal) {
    box.style.setProperty("--dyn-a", it.pal.a);
    box.style.setProperty("--dyn-b", it.pal.b);
  }
  box.classList.add("ready");
}
/**
 * Заглушка фона для трека без обложки: градиент в цветах темы и крупная нота (SVG без данных трека).
 * Цвета — только проверенные #rrggbb темы, иначе цвета Неона.
 */
function dynPlaceholder() {
  const cs = getComputedStyle(document.documentElement);
  // --accent зарегистрирован как <color> (@property в whisper.css) — значение приходит как rgb(…)
  const hex = (v, d) => {
    v = (v || "").trim();
    if (/^#[0-9a-f]{6}$/i.test(v)) return v;
    const m = /^rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(v);
    return m ? "#" + [m[1], m[2], m[3]].map((x) => Math.min(255, +x).toString(16).padStart(2, "0")).join("") : d;
  };
  const a = hex(cs.getPropertyValue("--accent"), "#a66bff"), b = hex(cs.getPropertyValue("--accent2"), "#e060ff");
  const svg = (note) =>
    "data:image/svg+xml," + encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400"><defs><radialGradient id="g" cx="30%" cy="25%" r="90%">` +
      `<stop offset="0" stop-color="${a}" stop-opacity=".55"/><stop offset=".45" stop-color="${b}" stop-opacity=".25"/><stop offset=".9" stop-color="#0b0618" stop-opacity="0"/></radialGradient></defs>` +
      `<rect width="400" height="400" fill="#0b0618"/><rect width="400" height="400" fill="url(#g)"/>` +
      (note ? '<g fill="none" stroke="rgba(255,255,255,0.55)" stroke-width="14" stroke-linecap="round" stroke-linejoin="round"><path d="M160 270V120l120-25v145"/><circle cx="130" cy="270" r="30"/><circle cx="250" cy="240" r="30"/></g>' : "") +
      "</svg>");
  return { url: svg(true), soft: svg(false), echo: svg(false), pal: { a, b } };
}

/** Фон под играющий трек. Нет обложки — заглушка в цветах темы. */
async function dynUpdate(t) {
  const my = ++dynSeq;
  document.body.classList.toggle("dyn-on", dynOn);
  const v = $("#bgVideo");
  if (!dynOn) {
    $("#dynBg").classList.remove("ready");
    dynKey = "";
    if (!v.hidden && v.paused) v.play().catch(() => {});
    return;
  }
  if (!v.paused) v.pause(); // видеообои темы под фоном по обложке не видны — не тратим на них кадры
  const src = t && t.cover;
  if (!src) {
    if (!t || dynKey === "-") return;
    dynKey = "-";
    return dynShow(dynPlaceholder());
  }
  if (src === dynKey) return;
  try {
    const it = await dynArt(src);
    if (my !== dynSeq) return;
    dynKey = src;
    dynShow(it);
  } catch {}
}

function updateNowPlaying() {
  const t = S.current;
  $("#pTitle").textContent = t ? t.title : L("Ничего не играет");
  $("#pArtist").textContent = t ? t.artist : "";
  const img = $("#cover");
  if (t) setCover(img, t.cover);
  else img.removeAttribute("src");
  document.title = t ? `${t.artist} — ${t.title}` : "VK Music";
  dynUpdate(t);
  syncNpAdd();
  updateNpSide();
  updateQueueInfo();
  syncWave();
}

/** Очередь изменилась без смены трека (панель очереди ядра: «играть следующим», убрать, переставить). */
function updateQueueInfo() {
  $("#queueInfo").textContent = S.queue.length ? L("Очередь: {0} из {1}", S.qi + 1, S.queue.length) : "";
}

/** «Сердце» у играющего трека: в библиотеке ли он (у трека сервиса не из библиотеки — добавить); «⋯». */
function syncNpAdd() {
  const t = S.current;
  $("#pMore").disabled = !t;
  for (const b of [$("#likeBtn"), $("#fLike")]) {
    b.hidden = !t || !t.service || t.unavailable;
    if (!b.hidden) {
      const on = !!libraryMatch(t);
      setHeart(b, on);
      b.title = on ? L("В библиотеке") : L("В библиотеку");
    }
  }
}

function updatePlayButton() {
  const playing = !S.pl.paused && !S.pl.idle;
  // как у Whisper: темы показывают «играет / пауза» по классу body (54 правила в библиотеке тем)
  document.body.classList.toggle("is-playing", playing);
  thSync();
  for (const b of [$("#play"), $("#fPlay")]) {
    b.replaceChildren(Icons.icon(playing ? "pause" : "play"));
  }
  syncWave();
}

function updateProgress() {
  const d = S.pl.duration || (S.current && S.current.duration) || 0;
  for (const [c, du] of [["#tCur", "#tDur"], ["#fTCur", "#fTDur"]]) {
    $(c).textContent = fmt(S.pl.pos);
    $(du).textContent = fmt(d);
  }
  if (!S.seeking) setBar(d ? S.pl.pos / d : 0);
}

/** Полоса воспроизведения — ползунок Whisper: значение 0–1000, заливка — --p. */
function setBar(f) {
  f = Math.min(1, Math.max(0, f));
  for (const b of [$("#seek"), $("#fSeek")]) {
    b.value = Math.round(f * 1000);
    b.style.setProperty("--p", f * 100 + "%");
  }
}

function initPlayer() {
  $("#play").addEventListener("click", togglePlay);
  $("#fPlay").addEventListener("click", togglePlay);
  for (const id of ["#likeBtn", "#fLike"]) {
    $(id).addEventListener("click", () => {
      if (!S.current) return;
      if (libraryMatch(S.current)) toast(L("Трек уже в библиотеке"));
      else saveTrack(S.current);
    });
  }
  $("#pMore").addEventListener("click", (ev) => {
    ev.stopPropagation();
    if (S.current) openTrackMenu(S.current, $("#pMore"));
  });
  $("#pMore").append(Icons.icon("dots"));
  $("#next").addEventListener("click", () => next(false));
  $("#prev").addEventListener("click", prev);
  $("#fNext").addEventListener("click", () => next(false));
  $("#fPrev").addEventListener("click", prev);
  $("#shuffleBtn").addEventListener("click", () => {
    S.shuffle = !S.shuffle;
    S.shuffleNext = null;
    S.history = [];
    syncModes();
    invoke("save_prefs", { shuffle: S.shuffle }).catch(() => {});
  });
  $("#repeatBtn").addEventListener("click", () => {
    S.repeat = { off: "all", all: "one", one: "off" }[S.repeat];
    syncModes();
    invoke("save_prefs", { repeat: S.repeat }).catch(() => {});
  });
  // обложка — текст и обложка на весь экран (как у Whisper)
  $("#cover").addEventListener("click", () => S.current && openFull("lyrics"));
  $("#fClose").addEventListener("click", closeFull);
  $("#fLyrBtn").addEventListener("click", () => openFull($("#full").dataset.mode === "lyrics" ? "cover" : "lyrics"));

  const vol = $("#vol");
  vol.value = S.volume;
  let volTimer = null;
  vol.addEventListener("input", () => {
    S.volume = Number(vol.value);
    S.muted = false;
    syncVolume();
    clearTimeout(volTimer);
    volTimer = setTimeout(() => invoke("player_volume", { volume: S.volume }).catch(() => {}), 40);
  });
  vol.addEventListener("change", () => invoke("save_prefs", { volume: S.volume }).catch(() => {}));
  // текст песни — своей кнопкой в плеере (раньше — только нажатием на обложку, его не находили); повторное — закрыть
  $("#lyrBtn").append(Icons.icon("lyrics"));
  $("#lyrBtn").addEventListener("click", () => {
    if (!$("#full").hidden && $("#full").dataset.mode === "lyrics") return closeFull();
    if (!S.current) return toast(L("Сначала включите трек"));
    openFull("lyrics");
  });
  $("#normBtn").addEventListener("click", () => {
    S.normalize = !S.normalize;
    syncNormalize();
    invoke("player_normalize", { on: S.normalize }).catch((e) => toast(errMsg(e), "error"));
  });
  $("#volBtn").addEventListener("click", () => {
    if (knob.moved) return (knob.moved = false);
    S.muted = !S.muted;
    syncVolume();
    invoke("player_volume", { volume: S.muted ? 0 : S.volume }).catch(() => {});
  });
  // громкость ручкой (тема с controls.volume = "knob"): тянуть вверх/вправо — громче; щелчок без движения — без звука
  const knob = { y: 0, x: 0, v: 0, id: null, moved: false };
  const vb = $("#volBtn");
  vb.addEventListener("pointerdown", (e) => {
    if (!volKnob() || e.button !== 0) return;
    Object.assign(knob, { y: e.clientY, x: e.clientX, v: S.volume, id: e.pointerId, moved: false });
    vb.setPointerCapture(e.pointerId);
  });
  vb.addEventListener("pointermove", (e) => {
    if (knob.id !== e.pointerId) return;
    const d = knob.y - e.clientY + (e.clientX - knob.x);
    if (!knob.moved && Math.abs(d) < 4) return;
    knob.moved = true;
    setVolume(knob.v + d / 1.6);
  });
  vb.addEventListener("pointerup", (e) => knob.id === e.pointerId && (knob.id = null));
  vb.addEventListener("pointercancel", (e) => ((knob.id = null), (knob.moved = false)));
  vb.addEventListener("keydown", (e) => {
    if (!volKnob()) return;
    const d = { ArrowUp: 5, ArrowRight: 5, ArrowDown: -5, ArrowLeft: -5 }[e.key];
    if (d) {
      e.preventDefault();
      e.stopPropagation();
      setVolume(S.volume + d);
    }
  });
  $("#volWrap").addEventListener("wheel", (e) => {
    e.preventDefault();
    setVolume(S.volume + (e.deltaY < 0 ? 5 : -5));
  }, { passive: false });

  // перемотка: ползунки Whisper (#seek в плеере, #fSeek на весь экран), 0–1000; mpv перематывает на отпускании
  for (const id of ["#seek", "#fSeek"]) {
    const bar = $(id);
    bar.addEventListener("input", () => {
      if (!S.current) return;
      S.seeking = true;
      const f = bar.value / 1000, d = S.pl.duration || S.current.duration || 0;
      setBar(f);
      for (const c of ["#tCur", "#fTCur"]) $(c).textContent = fmt(f * d);
    });
    bar.addEventListener("change", () => {
      if (!S.current) return;
      S.seeking = false;
      const d = S.pl.duration || S.current.duration || 0, pos = (bar.value / 1000) * d;
      S.pl.pos = pos;
      seekTo(pos);
      updateProgress();
    });
  }

  // фоновое кэширование закончилось: если это включённый трек — кэшируем следующий
  listen("cache://event", (ev) => {
    const e = ev.payload;
    if (S.current && e.key === S.current.key) prefetchNext();
  });

  listen("player://event", (ev) => {
    const e = ev.payload;
    if (e.type === "state") {
      // пока новый трек не загрузился, позиция и длительность — от прошлого: не показываем и не отдаём ядру
      const stale = S.loading && !(e.file > (S.fileAtPlay || 0));
      Object.assign(S.pl, { pos: stale ? 0 : e.pos, duration: stale ? 0 : e.duration, paused: e.paused, idle: e.idle, file: e.file });
      // трек загрузился: загружали «без звука», а ведущий комнаты уже играет — не держать
      if (S.loading && !stale && !e.idle && e.duration > 0) {
        S.loading = false;
        const room = Whisper.modules.together;
        if (S.holdPending && room && room.wantsPlay && room.wantsPlay()) invoke("player_pause", { paused: false }).catch(() => {});
        S.holdPending = false;
      }
      Whisper.player.report({ playing: !e.paused && !e.idle });
      // звук реально пошёл (после загрузки) — ядру: «Слушать вместе» догоняет ведущего по этому моменту
      if (S.needAudible && !S.loading && !e.paused && !e.idle) {
        S.needAudible = false;
        Whisper.player.report({ audible: true });
      }
      if (e.pos > 1) S.skips = 0;
      trackListen(e);
      updatePlayButton();
      updateProgress();
    } else if (e.type === "ended") {
      flushListen(true);
      // повтор того же трека — новый проигрыш считается заново
      if (S.repeat === "one") heard.key = null;
      next(true);
    } else if (e.type === "error") {
      onPlayError(e.message);
    }
  });

  syncModes();
  syncVolume();
  syncNormalize();
}

function syncNormalize() {
  const b = $("#normBtn");
  if (!b.firstChild) b.append(Icons.icon("levels"));
  b.classList.toggle("on", S.normalize);
  b.title = S.normalize ? L("Постоянный уровень громкости: включён") : L("Постоянный уровень громкости: выключен");
}

function onPlayError(message) {
  const t = S.current;
  if (/mpv остановился/.test(message)) return toast(message, "error");
  // ссылка из кэша могла протухнуть раньше срока — один раз пробуем свежую
  if (t && t.service && !S.retried) return playAt(S.qi, true);
  toast(L("Не удалось воспроизвести «{0}»: {1}", t ? t.title : "", L(message)), "error");
  if (++S.skips < 5) setTimeout(() => next(true), 800);
}

function syncModes() {
  Whisper.player.report({ modes: { available: true, shuffle: S.shuffle, repeat: { off: 0, all: 1, one: 2 }[S.repeat] } });
  thSync();
  const sb = $("#shuffleBtn");
  sb.classList.toggle("on", S.shuffle);
  sb.replaceChildren(Icons.icon("shuffle"));
  const rb = $("#repeatBtn");
  rb.classList.toggle("on", S.repeat !== "off");
  rb.replaceChildren(Icons.icon(S.repeat === "one" ? "repeat1" : "repeat"));
  rb.title = { off: L("Повтор выключен"), all: L("Повторять очередь"), one: L("Повторять трек") }[S.repeat];
}

function syncVolume() {
  Whisper.player.report({ volume: S.muted ? 0 : S.volume / 100 });
  $("#volBtn").replaceChildren(Icons.icon(S.muted || S.volume === 0 ? "mute" : "volume"));
  $("#volVal").textContent = S.muted ? "0" : String(S.volume);
  const v = $("#vol");
  v.style.setProperty("--p", (S.muted ? 0 : S.volume) + "%");
  // громкость числом 0…1 — для CSS тем (ручка громкости)
  document.documentElement.style.setProperty("--vol", String(S.muted ? 0 : S.volume / 100));
  if (volKnob()) $("#volBtn").setAttribute("aria-valuenow", String(S.muted ? 0 : S.volume));
  thSync();
}

// глобальные горячие клавиши (hotkeys.rs → событие hotkey; сочетания — hotkeys.js)
function volumeStep(d) {
  S.volume = Math.max(0, Math.min(100, Math.round(S.volume + d)));
  S.muted = false;
  $("#vol").value = S.volume;
  syncVolume();
  invoke("player_volume", { volume: S.volume }).catch(() => {});
  invoke("save_prefs", { volume: S.volume }).catch(() => {});
}
listen("hotkey", (ev) => {
  const click = (sel) => { const b = $(sel); if (b && !b.hidden && !b.disabled) b.click(); };
  switch (ev.payload) {
    case "toggle": return togglePlay();
    case "next": return S.current && next(false);
    case "prev": return S.current && prev();
    case "vol_up": return volumeStep(5);
    case "vol_down": return volumeStep(-5);
    case "mute": return click("#volBtn");
    case "seek_fwd": return S.current && seekTo(Math.min((S.pl.duration || 0) || Infinity, S.pl.pos + 10));
    case "seek_back": return S.current && seekTo(Math.max(0, S.pl.pos - 10));
    case "like": return click("#likeBtn");
    case "repeat": return click("#repeatBtn");
    case "shuffle": return click("#shuffleBtn");
  }
});

// клавиатура
document.addEventListener("keydown", (ev) => {
  const a = document.activeElement;
  // ползунки (перемотка, громкость) — не поле ввода: Пробел и стрелки после них работают
  const typing = !!a && (/^(TEXTAREA|SELECT)$/.test(a.tagName) || (a.tagName === "INPUT" && !/^(range|checkbox|button)$/.test(a.type)) || a.isContentEditable);
  if (ev.key === "Escape") {
    closePopups();
    closeNote();
    if (!$("#full").hidden) closeFull();
  }
  // Ctrl+F или / — общий поиск
  if ((ev.ctrlKey && ev.key.toLowerCase() === "f") || (!typing && ev.key === "/")) {
    ev.preventDefault();
    $("#search").focus();
    $("#search").select();
    return;
  }
  if (typing) return;
  if (ev.code === "Space") {
    ev.preventDefault();
    togglePlay();
  } else if (ev.key === "ArrowRight" && S.current) {
    seekTo(S.pl.pos + (ev.shiftKey ? 30 : 5));
  } else if (ev.key === "ArrowLeft" && S.current) {
    seekTo(Math.max(0, S.pl.pos - (ev.shiftKey ? 30 : 5)));
  } else if (ev.key === "n" && ev.ctrlKey) {
    next(false);
  } else if (ev.key === "p" && ev.ctrlKey) {
    prev();
  }
});

// ───────────────────────── запуск ─────────────────────────

async function boot() {
  // исходные (русские) тексты разметки — до того, как интерфейс начнёт их менять
  collectStatic();
  Icons.hydrate();
  // боковая панель: страницы; «Статистика» и «Настройки» повторным нажатием — обратно в библиотеку
  document.querySelectorAll(".side .nav[data-page]").forEach((n) =>
    n.addEventListener("click", () => {
      const p = n.dataset.page;
      if ((p === "stats" || p === "settings") && S.page === p) return go("library");
      if (p !== "search" && S.page === "search") {
        $("#search").value = "";
        endSearch();
      }
      go(p);
    })
  );
  // полоса заголовка окна: перетаскивание окна, кнопки, двойной щелчок — развернуть
  const tb = $("#titlebar");
  tb.addEventListener("mousedown", (ev) => {
    if (ev.button === 0 && !ev.target.closest("button") && ev.detail === 1) invoke("win_drag").catch(() => {});
  });
  tb.addEventListener("dblclick", (ev) => !ev.target.closest("button") && invoke("win_toggle_max").catch(() => {}));
  $("#wMin").addEventListener("click", () => invoke("win_minimize").catch(() => {}));
  $("#wMax").addEventListener("click", () => invoke("win_toggle_max").catch(() => {}));
  $("#wClose").addEventListener("click", () => invoke("win_close").catch(() => {}));
  // перевод текста, пояснения, закрытие пояснения кликом мимо и прокруткой
  $("#fLang").addEventListener("change", (ev) => selectLyricsLang(ev.target.value || null));
  $("#fNotes").addEventListener("change", (ev) => {
    lyricsNotes = ev.target.checked;
    localStorage.setItem("lyricsNotes", lyricsNotes ? "1" : "0");
    renderLyrics();
  });
  $("#fLyrics").addEventListener("scroll", closeNote);
  document.addEventListener("click", (ev) => {
    if (noteFor && !ev.target.closest("#lyrNote")) closeNote();
  });

  let info;
  try {
    info = await invoke("app_info");
  } catch (e) {
    info = { theme: "osaka-jade", volume: 70, overrides: {}, shuffle: false, repeat: "off", search: ["vk", "sc"] };
    toast(errMsg(e), "error");
  }
  // язык интерфейса — до первой отрисовки
  setLang(info.lang || "ru");
  window.Layout?.refresh(); // подписи кнопки «Свернуть» — на языке интерфейса
  S.musicDir = info.music_dir || "~/Music/vk";
  S.downloadDir = info.download_dir || "~/Downloads";
  S.cacheLimit = info.cache_limit_mb || 0;
  S.volume = info.volume;
  S.overrides = info.overrides || {};
  S.shuffle = !!info.shuffle;
  S.repeat = info.repeat || "off";
  S.font = info.font || null;
  S.search = Array.isArray(info.search) ? info.search : ["vk", "sc"];
  S.libHidden = Array.isArray(info.hidden_theme_categories) ? info.hidden_theme_categories : [];
  S.lyrHideParts = !!info.lyrics_hide_parts;
  S.lyrCompact = !!info.lyrics_compact;
  S.normalize = !!info.normalize;
  try {
    await applyTheme(await invoke("theme_render", { id: null }));
  } catch (e) {
    toast(L("Тема не загрузилась: ") + errMsg(e), "error");
  }
  // тема применена — интерфейс можно показывать (boot.js прятал его под цветом фона прошлого стиля)
  window.bootDone?.();
  initPlayer();
  if (info.player_error) toast(L(info.player_error), "error");
  go("library");
  syncAll();

  invoke("dl_caps")
    .then((c) => {
      S.caps = c;
      if (S.page === "services") renderServices();
      if (!c.ytdlp || !c.ffmpeg) toast(L("Для загрузки по ссылкам нужны yt-dlp и ffmpeg: sudo pacman -S yt-dlp ffmpeg"), "error");
    })
    .catch(() => {});

  // автообновление — только Windows (update.rs; на Linux update_check всегда отвечает «нет»): раз при запуске,
  // установка — по кнопке в подсказке
  setTimeout(checkUpdate, 4000);
}

async function checkUpdate() {
  let u = null;
  try { u = await invoke("update_check"); } catch { return; }
  if (!u || !u.version) return;
  toast(L("Доступна новая версия {0}", u.version), "", {
    label: L("Обновить"),
    fn: () => {
      toast(L("Скачивается обновление…"), "", null, 0);
      invoke("update_install").catch((e) => toast(errMsg(e), "error"));
    },
  }, 0);
}

// после всех скриптов: привязка к ядру (whisper-vk.js) и движок тем подключаются после этого файла,
// а тема при запуске уже должна знать роли
document.addEventListener("DOMContentLoaded", boot);
