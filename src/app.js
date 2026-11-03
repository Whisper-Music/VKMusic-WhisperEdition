"use strict";
// VK Music — плеер: библиотека (файлы + сохранённые треки сервисов), поиск во ВКонтакте и SoundCloud.
// Всё общение с Rust — через invoke/события Tauri. Ядро Whisper (src/core) подключено раньше: о плеере и очереди
// ему сообщает Whisper.player.report(…) отсюда, а привязка к ядру (роли, провайдер, плеер, очередь) — в whisper-vk.js.

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
const listen = TAURI.event.listen;

const $ = (sel, root = document) => root.querySelector(sel);
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};

// названия страниц (русские — ключи перевода, переводятся при показе: L(PAGES[page]))
const PAGES = {
  library: "Библиотека",
  vk: "Миксы",
  "set-general": "Общее",
  "set-lyrics": "Текст песен",
  stats: "Статистика",
  "set-services": "Сервисы",
  "set-theme": "Оформление",
  "set-store": "Магазин тем",
  "set-storage": "Хранилище",
  "set-integrations": "Интеграции",
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

/** action — кнопка в подсказке { label, fn } (например, «Вернуть» у панели очереди ядра); ms — сколько показывать. */
function toast(msg, kind, action, ms) {
  const t = el("div", "toast" + (kind === "error" ? " error" : ""), String(msg));
  if (action && typeof action.fn === "function") {
    t.classList.add("has-action");
    const b = el("button", "toast-action", String(action.label || ""));
    b.addEventListener("click", () => (t.remove(), action.fn()));
    t.append(b);
  }
  $("#toasts").append(t);
  setTimeout(() => t.remove(), ms || (kind === "error" ? 6000 : action ? 5000 : 3500));
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
  const pop = el("div", "popup confirm-pop" + (cls ? " " + cls : ""));
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
    const input = el("input", "prompt-input");
    input.type = "text";
    input.maxLength = 128;
    input.value = value;
    input.spellcheck = false;
    const yes = el("button", "btn primary", ok);
    const no = el("button", "btn", L("Отмена"));
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
    const row = el("div", "confirm-actions");
    row.append(yes, no);
    pop.append(el("div", "confirm-text", label), input, row);
    setTimeout(() => (input.focus(), input.select()), 0);
  });
}

function confirmClick(btn, fn, label = L("Точно?"), ok = L("Удалить")) {
  closeConfirm();
  const pop = el("div", "popup confirm-pop");
  pop.id = "confirmPop";
  pop.setAttribute("role", "dialog");
  const yes = el("button", "btn danger", ok);
  const no = el("button", "btn", L("Отмена"));
  yes.addEventListener("click", (ev) => {
    ev.stopPropagation();
    closeConfirm();
    fn();
  });
  no.addEventListener("click", (ev) => {
    ev.stopPropagation();
    closeConfirm();
  });
  const row = el("div", "confirm-actions");
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

function button(icon, text, cls, onClick) {
  const b = el("button", "btn" + (cls ? " " + cls : ""));
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
    cover: t.has_cover ? "vkm://localhost/cover?path=" + encodeURIComponent(t.path) : null,
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
    const keep = $("#content").scrollTop;
    rerender();
    $("#content").scrollTop = keep;
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
function renderTracks(items, reorder, opts = {}) {
  const wrap = el("div", "tracks");
  items.forEach((t, i) => {
    const row = el("div", "track");
    row.dataset.key = t.key;
    if (S.current && S.current.key === t.key) row.classList.add("playing");
    if (t.unavailable) row.classList.add("unavailable");

    const cover = el("div", "t-cover");
    if (t.cover) {
      const img = el("img");
      img.loading = "lazy";
      img.alt = "";
      img.src = t.cover;
      img.onerror = () => img.replaceWith(Icons.icon("note"));
      cover.append(img);
    } else cover.append(Icons.icon("note"));

    const meta = el("div", "t-meta");
    const title = el("div", "t-title", t.title);
    if (t.snippet) {
      const b = el("span", "t-badge", L("30 с"));
      b.title = L("SoundCloud даёт только фрагмент (трек Go+)");
      title.append(b);
    }
    meta.append(title, el("div", "t-artist", t.artist));

    const q = el("div", "t-quality");
    const actions = el("div", "t-actions");
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
      const badge = el("span", "t-service");
      badge.append(Icons.icon(svc.icon));
      badge.title = svc.name;
      q.append(badge);
      const saved = S.saved.find((s) => s.key === t.key);
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
        badge.title += L(" · из вашего аккаунта");
      } else if (saved && saved.sync) {
        // синхронизирован из аккаунта — убирается вместе с ним в самом сервисе
        badge.title += L(" · из вашего аккаунта");
      } else if (saved) {
        actions.append(
          iconButton("trash", L("Убрать из библиотеки"), (b) => confirmClick(b, () => unsaveTrack(t), L("Убрать трек из библиотеки?"), L("Убрать")))
        );
      } else if (libraryMatch(t)) {
        // тот же трек уже есть в библиотеке (например, копией в «Моих треках») — не добавляем второй раз
        badge.title += L(" · уже в библиотеке");
      } else if (!t.unavailable) {
        actions.append(iconButton("plus", L("В библиотеку"), () => saveTrack(t)));
      }
    }

    // «⋯»: «Играть следующим» и пункты модулей ядра Whisper («Слушать вместе», «Сохранить в архив»)
    if (!t.unavailable) actions.append(iconButton("dots", L("Ещё"), (b) => openTrackMenu(t, b)));
    row.append(el("div", "t-num", String(i + 1)), cover, meta, q, actions, el("div", "t-dur", fmt(t.duration)));
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
  document.querySelectorAll(".track").forEach((r) => r.classList.toggle("playing", !!S.current && r.dataset.key === S.current.key));
}

// ───────────────────────── навигация ─────────────────────────

const inSettings = () => S.page.startsWith("set-");

function go(page, keepPopups) {
  // повторное нажатие пункта меню внутри открытого плейлиста — обратно к списку
  if (page === S.page && S.vkPl && S.vkPl.from === page) S.vkPl = null;
  if (!inSettings() && page.startsWith("set-")) S.lastPage = S.page;
  S.page = page;
  if (page === "set-storage") S.cacheInfo = null; // кэш растёт, пока играет музыка
  const settings = inSettings();
  $("#mainNav").hidden = settings;
  $("#setNav").hidden = !settings;
  document.body.classList.toggle("settings-mode", settings);
  $("#settingsBtn").classList.toggle("active", settings);
  $("#statsBtn").classList.toggle("active", page === "stats");
  // переход на другую страницу — список снова на месте (на узком окне панель могла его закрывать)
  document.body.classList.remove("np-over");
  syncNpToggle();
  if (page === "stats") S.statsData = null;
  // магазин тем — часть «Оформления»
  const navPage = page === "set-store" ? "set-theme" : page;
  document.querySelectorAll(".nav-item[data-page]").forEach((n) => n.classList.toggle("active", n.dataset.page === navPage));
  $("#pageTitle").textContent = L(page === "set-store" ? PAGES[page] : settings ? "Настройки" : PAGES[page]);
  $("#topActions").replaceChildren();
  if (!keepPopups) closePopups();
  $("#content").scrollTop = 0;
  // «Интеграции» — постоянная страница #intPage: карточки модулей ядра Whisper живут в документе всё время
  $("#content").hidden = page === "set-integrations";
  $("#intPage").hidden = page !== "set-integrations";
  rerender();
  // новая страница проявляется (анимация перезапускается)
  const c = $("#content");
  c.classList.remove("page-in");
  void c.offsetWidth;
  c.classList.add("page-in");
}

/** Перерисовать текущую страницу (после изменений данных). */
/** Стрелка «назад» слева от названия страницы — пока на странице открыт плейлист ВК и в магазине тем. */
function syncBack() {
  const b = $("#pageBack");
  const store = S.page === "set-store";
  b.hidden = !(store || (S.vkPl && S.vkPl.from === S.page));
  b.title = L("Назад: {0}", L(PAGES[store ? "set-theme" : S.page] || ""));
}

$("#pageBack").addEventListener("click", () => {
  if (S.page === "set-store") return go("set-theme");
  S.vkPl = null;
  $("#content").scrollTop = 0;
  rerender();
});

function rerender() {
  syncBack();
  syncNpAdd();
  if (S.page === "library") renderLibrary();
  else if (S.page === "stats") renderStatsPage();
  else if (S.page === "vk") renderVk();
  else if (S.page === "set-general") renderGeneral();
  else if (S.page === "set-lyrics") renderLyricsSettings();
  else if (S.page === "set-services") renderServices();
  else if (S.page === "set-theme") renderThemes();
  else if (S.page === "set-store") renderStore();
  else if (S.page === "set-storage") renderStorage();
  if (dropOpen()) renderDrop();
}

// всплывающие меню: настройки и выбор сервисов поиска
function closePopups() {
  document.querySelectorAll(".popup").forEach((p) => (p.hidden = true));
  closeConfirm();
}

function togglePopup(popup) {
  const open = popup.hidden;
  closePopups();
  popup.hidden = !open;
}

document.addEventListener("click", (ev) => {
  if (!ev.target.closest(".popup, #svcBtn")) closePopups();
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
  if (ev.button !== 0 || ev.target.closest("button, input, a, .t-actions")) return;
  const scroller = $("#content");
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
    const r = scroller.getBoundingClientRect();
    const edge = 48;
    const v = lastY < r.top + edge ? -(r.top + edge - lastY) / 3 : lastY > r.bottom - edge ? (lastY - (r.bottom - edge)) / 3 : 0;
    if (v) {
      scroller.scrollTop += Math.max(-24, Math.min(24, v));
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
    // номера строк — по новому порядку
    [...wrap.children].forEach((x, i) => {
      const n = x.querySelector(".t-num");
      if (n) n.textContent = String(i + 1);
    });
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

/** Строка над списком: обновить (папка + синхронизация с сервисами) и состояние синхронизации. */
function renderLibraryBar() {
  const bar = el("div", "lib-bar");
  const refresh = iconButton("refresh", L("Перечитать папку и синхронизировать с сервисами"), () => {
    S.localLoaded = false;
    if (S.myPl) S.myPl.fresh = false; // сохранённый список остаётся на экране, пока не придёт свежий
    renderLibrary();
    syncAll();
  });
  const status = el("span", "sync-status");
  status.id = "syncStatus";
  bar.append(refresh, status);
  return bar;
}

function renderLibrary() {
  syncBack();
  // открыт плейлист из строки «Плейлисты»
  if (S.vkPl && S.vkPl.from === "library") return renderVkPlaylist();
  const c = $("#content");
  c.replaceChildren();
  c.append(renderLibraryBar());
  renderSyncStatus();
  const dl = el("div", "downloads");
  dl.id = "downloads";
  c.append(dl);
  renderDownloads();
  // ваши плейлисты ВК — над треками
  const pls = el("div", "lib-playlists");
  pls.id = "libPlaylists";
  c.append(pls);
  renderMyPlaylists();
  const list = el("div");
  list.id = "libList";
  c.append(list);
  if (!S.localLoaded) {
    list.append(el("div", "spinner"));
    loadLibrary();
    return;
  }
  renderLibraryList();
}

function renderLibraryList() {
  const box = $("#libList");
  if (!box) return;
  const keep = $("#content").scrollTop;
  box.replaceChildren();
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
  const items = visibleTracks();
  const total = items.reduce((s, t) => s + (t.duration || 0), 0);
  box.append(el("div", "section-title", `${items.length} ${plural(items.length, "трек", "трека", "треков")} · ${fmtTotal(total)}`));
  box.append(renderTracks(items, true));
  $("#content").scrollTop = keep;
}

// ───────────────────────── общий поиск: выпадающий список под строкой ─────────────────────────
//
// Ищет сам, пока вы печатаете (с небольшой задержкой). Результаты — в выпадающем списке, библиотека под ним
// остаётся. Список закрывается нажатием в любом месте вне его (строка поиска и меню сервисов — не в счёт).
// Пока поиск идёт (строка не пуста), результаты каждого сервиса по каждому запросу кэшируются: вернулись к
// прежнему запросу или снова включили сервис — результаты появляются сразу, без запросов.

const SEARCH_DELAY = 450;
const SEARCH_MIN = 2;

const isUrl = (s) => /^https?:\/\/\S+$/i.test(s);
const isScUrl = (s) => /^https:\/\/(www\.|m\.|on\.)?soundcloud\.com\/\S+/i.test(s);

/** Кэш результатов на время поиска: "vk|text|запрос" → секция. Чистится, когда строку очистили. */
const searchCache = new Map();

function renderSvcButton() {
  const b = $("#svcBtn");
  b.replaceChildren();
  const on = S.search.filter((s) => SERVICES[s]);
  for (const s of on) b.append(Icons.icon(SERVICES[s].icon));
  if (!on.length) b.append(el("span", "svc-none", "—"));
  b.title = on.length ? L("Ищет: ") + on.map((s) => SERVICES[s].name).join(", ") : L("Поиск в сервисах выключен");
  b.classList.toggle("off", !on.length);
}

function renderSvcMenu() {
  const m = $("#svcMenu");
  m.replaceChildren(el("div", "popup-title", L("Где искать")));
  for (const [id, svc] of Object.entries(SERVICES)) {
    const label = el("label", "popup-item check");
    const cb = el("input");
    cb.type = "checkbox";
    cb.checked = S.search.includes(id);
    cb.addEventListener("change", () => {
      S.search = Object.keys(SERVICES).filter((k) => (k === id ? cb.checked : S.search.includes(k)));
      renderSvcButton();
      invoke("save_prefs", { search: S.search }).catch((e) => toast(errMsg(e), "error"));
      refreshResults(id, cb.checked);
    });
    label.append(cb, Icons.icon(svc.icon), el("span", null, svc.name));
    m.append(label);
  }
}

$("#svcBtn").addEventListener("click", (ev) => {
  ev.stopPropagation();
  const m = $("#svcMenu");
  const open = m.hidden;
  closeConfirm();
  m.hidden = !open;
  if (open) renderSvcMenu();
});

const dropOpen = () => !$("#searchDrop").hidden;

function openDrop() {
  $("#searchDrop").hidden = false;
  renderDrop();
}

function closeDrop() {
  $("#searchDrop").hidden = true;
}

let searchTimer = null;
$("#searchInput").addEventListener("input", () => {
  clearTimeout(searchTimer);
  const q = $("#searchInput").value.trim();
  if (!q) return endSearch();
  searchTimer = setTimeout(() => runSearch(q), SEARCH_DELAY);
});

$("#searchInput").addEventListener("focus", () => {
  if (S.sr && $("#searchInput").value.trim()) openDrop();
});

$("#searchBox").addEventListener("submit", (ev) => {
  ev.preventDefault();
  clearTimeout(searchTimer);
  const q = $("#searchInput").value.trim();
  if (!q) return;
  // Enter по ссылке (не SoundCloud) — скачать через yt-dlp
  if (isUrl(q) && !isScUrl(q)) return downloadLink(q);
  runSearch(q, true);
});

$("#searchInput").addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") {
    if (dropOpen()) closeDrop();
    else {
      ev.currentTarget.value = "";
      endSearch();
      ev.currentTarget.blur();
    }
  }
});

/** Строку очистили — поиск окончен: список закрывается, кэш результатов забывается. */
function endSearch() {
  clearTimeout(searchTimer);
  S.sr = null;
  searchCache.clear();
  closeDrop();
}

function downloadLink(q) {
  startDownload(q, q);
  $("#searchInput").value = "";
  endSearch();
  toast(L("Скачиваю по ссылке — прогресс в библиотеке"));
  if (S.page !== "library") go("library");
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
    .finally(() => dropOpen() && renderDrop());
}

/** Поиск по строке: текст — в отмеченных сервисах; ссылка SoundCloud — открыть трек/плейлист. */
function runSearch(q, force) {
  if (!force && q.length < SEARCH_MIN && !isUrl(q)) return closeDrop();
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
  openDrop();
}

/** Сервис включили/выключили: результаты обновляются сразу, выключенный остаётся в кэше. */
function refreshResults() {
  const q = $("#searchInput").value.trim();
  if (!q || isUrl(q)) return;
  if (!S.sr || S.sr.q !== q) return runSearch(q, true);
  openDrop();
}

function renderSection(id, sec) {
  const box = el("div", "result-section");
  const svc = SERVICES[id];
  const head = el("div", "section-title with-icon");
  head.append(Icons.icon(svc.icon), el("span", null, svc.name));
  box.append(head);
  if (sec.tracks.length) box.append(renderTracks(sec.tracks));
  if (sec.busy) {
    box.append(el("div", "spinner small"));
  } else if (sec.login) {
    const b = button("link", L("Войти"), "", () => {
      closeDrop();
      go("set-services");
    });
    const row = el("div", "drop-note");
    row.append(el("span", null, L("Чтобы искать во {0}, войдите в аккаунт.", svc.name)), b);
    box.append(row);
  } else if (sec.error) {
    const row = el("div", "drop-note error");
    row.append(el("span", null, L(sec.error)), button(null, L("Повторить"), "", () => fetchSection(id, sec, sec.tracks.length > 0)));
    box.append(row);
  } else if (sec.next != null && sec.next !== false) {
    box.append(button(null, L("Показать ещё"), "more", () => {
      fetchSection(id, sec, true);
      renderDrop();
    }));
  } else if (!sec.tracks.length) {
    box.append(el("div", "hint", L("Ничего не нашлось.")));
  }
  return box;
}

function renderDrop() {
  const d = $("#searchDrop");
  const sr = S.sr;
  if (!sr || d.hidden) return;
  const keep = d.scrollTop;
  d.replaceChildren();
  if (sr.kind === "link") {
    const b = button("download", L("Скачать по ссылке через yt-dlp (Enter)"), "primary", () => downloadLink(sr.q));
    d.append(el("div", "hint", L("Ссылка скачается файлом в лучшем качестве; плейлист или альбом — целиком.")), b);
    return;
  }
  if (sr.lib.length) {
    const box = el("div", "result-section in-library");
    box.append(el("div", "section-title", L("Уже в библиотеке")), renderTracks(sr.lib));
    d.append(box);
  }
  const ids = sr.kind === "url" ? ["sc"] : S.search.filter((s) => SERVICES[s]);
  for (const id of ids) d.append(renderSection(id, section(id)));
  if (sr.kind === "text") {
    if (!ids.length) d.append(el("p", "hint", L("Поиск в сервисах выключен — включите ВКонтакте или SoundCloud кнопкой справа от строки.")));
    // запасной путь: первый результат YouTube через yt-dlp
    const yt = button("download", L("Скачать «{0}» с YouTube (yt-dlp, первый результат)", sr.q), "", () => {
      startDownload(sr.q, L("Поиск: ") + sr.q);
      closeDrop();
    });
    yt.classList.add("yt-fallback");
    d.append(yt);
  }
  d.scrollTop = keep;
}

// список закрывается нажатием в любом месте вне его (строка поиска, меню сервисов и окошко подтверждения — не в счёт)
document.addEventListener("mousedown", (ev) => {
  if (dropOpen() && !ev.target.closest("#searchDrop, #searchBox, #svcMenu, #confirmPop")) closeDrop();
});

// ───────────────────────── настройки: сервисы ─────────────────────────

function loginBar(service) {
  const on = !!service;
  $("#loginBar").hidden = !on;
  document.body.classList.toggle("login-mode", on);
  if (on) {
    $("#loginBarText").textContent = service === "vk"
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
  if (S.page === "set-services") renderServices();
  try {
    S.svc[service] = await invoke(service + "_status");
  } catch (e) {
    S.svc[service] = { logged_in: false, profile: null, error: errMsg(e) };
  }
  if (S.page === "set-services") renderServices();
}

listen("svc://event", (ev) => {
  const e = ev.payload;
  loginBar(null);
  if (e.type === "login") toast(L("Вы вошли: {0}", SERVICES[e.service] ? SERVICES[e.service].name : e.service));
  if (SERVICES[e.service]) {
    refreshStatus(e.service);
    if (e.type === "login") syncService(e.service);
    // вошли во ВКонтакте — список плейлистов заново (до входа был пустым или с ошибкой)
    if (e.type === "login" && e.service === "vk") S.myPl = null;
  }
});

const NOTES = {
  vk:
    "«Мои треки» ВКонтакте синхронизируются с библиотекой: при запуске, после входа и по кнопке обновления; после выхода они из библиотеки убираются. " +
    "Треки ВК из библиотеки кэшируются при прослушивании и скачиваются кнопкой со стрелкой. " +
    "Ссылка запрашивается при нажатии «играть». ВКонтакте не разрешает сторонние клиенты для музыки — пользуйтесь на свой риск: аккаунт могут ограничить.",
  sc:
    "Ваши лайки SoundCloud синхронизируются с библиотекой: при запуске, после входа и по кнопке обновления. " +
    "Поиск и прослушивание работают и без входа; со входом SoundCloud узнаёт ваш аккаунт (например, полные треки Go+ при подписке). " +
    "Трек SoundCloud можно скачать файлом через yt-dlp кнопкой со стрелкой. Это неофициальный клиент сайта.",
};

function renderServiceCard(id) {
  const svc = SERVICES[id];
  const st = S.svc[id];
  const card = el("div", "svc-card");
  const head = el("div", "svc-head");
  head.append(Icons.icon(svc.icon), el("div", "svc-name", svc.name));
  const status = el("div", "svc-status");
  const actions = el("div", "svc-actions");
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
      status.classList.add("warn");
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
  head.append(status);
  card.append(head, actions, el("p", "hint", L(NOTES[id])));
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

function renderServices() {
  const c = $("#content");
  c.replaceChildren();
  c.append(el("div", "section-title", L("Авторизация")));
  const cards = el("div", "svc-cards");
  for (const id of Object.keys(SERVICES)) cards.append(renderServiceCard(id));
  c.append(cards);

  c.append(el("div", "section-title", L("Загрузка файлов (yt-dlp)")));
  c.append(
    el(
      "p",
      "hint",
      L("Ссылка в строке поиска (YouTube, YouTube Music, SoundCloud, Bandcamp — всё, что понимает yt-dlp) скачивается в {0} " +
        "лучшей аудиодорожкой без перекодирования — исходный Opus, AAC или FLAC, с тегами{1}. ", S.musicDir, S.caps && S.caps.mutagen ? L(" и обложкой") : "") +
        L("Ссылка на плейлист или альбом скачивается целиком. Ссылка на SoundCloud сначала показывается в поиске — скачать можно кнопкой со стрелкой.")
    )
  );
  c.append(renderCaps());
  c.append(button("folder", L("Открыть папку с музыкой ({0})", S.musicDir.replace(/^\/home\/[^/]+/, "~")), "open-dir", () =>
    invoke("open_music_dir").catch((e) => toast(errMsg(e), "error"))
  ));
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
  const was = box.querySelector(".vk-row");
  const keepLeft = was ? was.scrollLeft : 0;
  box.replaceChildren();
  // без входа во ВКонтакте строки нет; со входом — даже пустая, с карточкой «Новый плейлист»
  if (!S.myPl || !S.myPl.list) return;
  const list = S.myPl.list;
  box.append(el("div", "section-title", L("Плейлисты")));
  const row = el("div", "vk-row");
  const add = el("button", "vk-pl vk-pl-new");
  const cover = el("div", "vk-pl-cover");
  cover.append(Icons.icon("plus"));
  add.append(cover, el("div", "vk-pl-title", L("Новый плейлист")));
  add.addEventListener("click", (ev) => {
    ev.stopPropagation(); // иначе тот же клик закроет окошко (closePopups на document)
    promptClick(cover, (title) => createPlaylist(title), L("Название плейлиста"), L("Создать"));
  });
  row.append(add);
  for (const pl of list) row.append(vkPlaylistCard(pl));
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
        const keep = $("#content").scrollTop;
        rerender();
        $("#content").scrollTop = keep;
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

/** Карточка плейлиста: обложка, название, подпись. */
function vkPlaylistCard(pl) {
  const card = el("button", "vk-pl");
  const cover = el("div", "vk-pl-cover");
  if (pl.cover) {
    const img = el("img");
    img.loading = "lazy";
    img.alt = "";
    img.src = pl.cover;
    img.onerror = () => img.replaceWith(Icons.icon("note"));
    cover.append(img);
  } else cover.append(Icons.icon("note"));
  card.append(cover, el("div", "vk-pl-title", pl.title || L("Без названия")));
  const sub = pl.count ? `${pl.count} ${plural(pl.count, "трек", "трека", "треков")}` : pl.subtitle || "";
  if (sub) card.append(el("div", "vk-pl-sub", sub));
  card.addEventListener("click", () => openVkPlaylist(pl));
  return card;
}

function renderVk() {
  syncBack();
  const c = $("#content");
  if (S.vkPl && S.vkPl.from === "vk") return renderVkPlaylist();
  c.replaceChildren();
  const refresh = iconButton("refresh", L("Обновить"), () => loadVkHome(true));
  $("#topActions").replaceChildren(refresh);
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
  // VK Микс
  for (const m of h.mixes || []) {
    const card = el("div", "vk-mix");
    const text = el("div", "vk-mix-text");
    text.append(el("div", "vk-mix-title", m.id === "common" ? L("VK Микс") : m.title), el("div", "vk-mix-desc", m.id === "common" ? L("Бесконечная подборка по вашему вкусу") : m.description));
    const play = button("play", L("Слушать"), "primary", (b) => playMix(m.id, b));
    if (S.queueMix === m.id) play.classList.add("on");
    card.append(Icons.icon("mix"), text, play);
    c.append(card);
  }
  const grid = (title, list) => {
    if (!list || !list.length) return;
    c.append(el("div", "section-title", L(title)));
    const g = el("div", "vk-grid");
    for (const pl of list) g.append(vkPlaylistCard(pl));
    c.append(g);
  };
  for (const g of h.groups || []) grid(g.title, g.playlists);
  if (!(h.mixes || []).length && !(h.groups || []).length) c.append(emptyState("mix", L("ВКонтакте не прислал подборок.")));
}

const samePl = (a, b) => a.owner_id === b.owner_id && a.id === b.id;

/** Открыть плейлист. Ваш, уже открывавшийся, — сразу с диска; первая страница из ВК потом заменяет его. */
async function openVkPlaylist(pl) {
  const v = { pl, tracks: [], next: 0, busy: false, error: null, from: S.page };
  S.vkPl = v;
  $("#content").scrollTop = 0;
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
    const keep = $("#content").scrollTop;
    rerender();
    $("#content").scrollTop = keep;
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
    const keep = $("#content").scrollTop;
    rerender();
    $("#content").scrollTop = keep;
  }
}

function renderVkPlaylist() {
  const c = $("#content");
  const v = S.vkPl;
  c.replaceChildren();
  $("#topActions").replaceChildren();
  const head = el("div", "vk-pl-head");
  const cover = el("div", "vk-pl-cover");
  if (v.pl.cover) {
    const img = el("img");
    img.alt = "";
    img.src = v.pl.cover;
    cover.append(img);
  } else cover.append(Icons.icon("note"));
  const meta = el("div", "vk-pl-meta");
  meta.append(el("div", "vk-pl-name", v.pl.title || L("Без названия")));
  if (v.pl.count) meta.append(el("div", "vk-pl-sub", `${v.pl.count} ${plural(v.pl.count, "трек", "трека", "треков")}`));
  const playable = () => v.tracks.filter((x) => !x.unavailable);
  const play = button("play", L("Слушать"), "primary", () => playable().length && playList(playable(), 0));
  play.disabled = !playable().length;
  const acts = el("div", "vk-pl-actions");
  acts.append(play, ...playlistActions(v));
  meta.append(acts);
  head.append(cover, meta);
  c.append(head);
  if (v.tracks.length) c.append(renderTracks(v.tracks, false, { playlist: v.pl.editable ? v.pl : null }));
  if (v.busy) c.append(el("div", "spinner"));
  else if (v.error) c.append(emptyState("x", L(v.error), button(null, L("Повторить"), "", () => ((v.error = null), loadVkPlaylistMore()))));
  else if (v.next != null) c.append(button(null, L("Показать ещё"), "more", () => loadVkPlaylistMore()));
  else if (!v.tracks.length) c.append(emptyState("note", L("В плейлисте нет треков, которые можно слушать.")));
}

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
      const keep = $("#content").scrollTop;
      renderStatsPage();
      $("#content").scrollTop = keep;
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

/** Сводка по играющему треку — на узком окне, где правой панели нет. */
function renderStatsNow() {
  const box = $("#statsNow");
  if (!box) return;
  const t = S.current;
  box.hidden = !t;
  if (!t) return;
  // обложку и название строим один раз на трек, дальше (4 раза в секунду) меняется только строка цифр
  if (box.dataset.key !== t.key) {
    box.dataset.key = t.key;
    box.replaceChildren();
    if (t.cover) {
      const img = el("img");
      img.alt = "";
      img.src = t.cover;
      img.onerror = () => img.replaceWith(Icons.icon("note"));
      box.append(img);
    } else box.append(Icons.icon("note"));
    const meta = el("div", "stats-now-meta");
    meta.append(el("div", "stats-now-title", `${t.artist} — ${t.title}`), el("div", "stats-now-sub"));
    box.append(meta);
  }
  const subEl = box.querySelector(".stats-now-sub");
  subEl.textContent = "";
  const st = heard.key === t.key ? heard.stat : null;
  if (st) {
    const parts = [
      L("дослушан {0} {1}", st.plays, plural(st.plays, "раз", "раза", "раз")),
      L("слушали {0}", fmtListened(st.seconds + heard.acc)),
    ];
    if (st.rank) parts.push(L("#{0} из {1}", st.rank, st.total));
    if (heard.prevLast) parts.push(L("последний раз {0}", fmtWhen(heard.prevLast)));
    subEl.textContent = parts.join(" · ");
  }
}

async function renderStatsPage() {
  const c = $("#content");
  c.replaceChildren();
  const page = el("div", "stats-page");
  c.append(page);
  const now = el("div", "stats-now");
  now.id = "statsNow";
  page.append(now);
  renderStatsNow();

  const periods = el("div", "stats-periods");
  for (const [id, name] of PERIODS) {
    const b = el("button", "btn" + (S.statsPeriod === id ? " on" : ""), L(name));
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
    const x = el("div", "stats-tile");
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
    page.append(el("div", "section-title", title));
    page.append(values.length ? barChart(values, label, tip) : el("p", "hint", L("Пока пусто — статистика копится, пока играет музыка.")));
  }

  page.append(el("div", "section-title", L("По часам суток")));
  page.append(barChart(hours, (h) => (h % 3 === 0 ? String(h) : ""), (h) => `${h}:00–${h + 1}:00`));

  // топы
  const cols = el("div", "stats-cols");
  const top = [...perTrack.entries()].filter(([k]) => byKey.has(k)).sort((a, b) => b[1] - a[1]).slice(0, 10);
  const topBox = el("div");
  topBox.append(el("div", "section-title", L("Топ треков")));
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
  const artBox = el("div");
  artBox.append(el("div", "section-title", L("Топ исполнителей")));
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
  const fBox = el("div");
  fBox.append(el("div", "section-title", L("Давно не слушали")));
  const fList = el("div", "stats-list");
  if (!forgotten.length) fList.append(el("p", "hint", L("Здесь появятся любимые треки, которые не играли больше месяца.")));
  const fTracks = forgotten.map(([k]) => byKey.get(k));
  forgotten.forEach(([k, s], i) => {
    const t = byKey.get(k);
    fList.append(statsRow(i + 1, t, t.title, t.artist + " · " + L("последний раз {0}", fmtWhen(s.last, true)), fmtListened(s.seconds), s.seconds / forgotten[0][1].seconds, () => playList(fTracks, i)));
  });
  fBox.append(fList);
  const nBox = el("div");
  nBox.append(el("div", "section-title", L("Ни разу не слушали — {0}", never.length)));
  const nList = el("div", "stats-list");
  if (!never.length) nList.append(el("p", "hint", L("Вы слушали всю библиотеку.")));
  const nShow = never.slice(0, 10);
  nShow.forEach((t, i) => nList.append(statsRow(i + 1, t, t.title, t.artist, t.duration ? fmt(t.duration) : "", 0, () => playList(never, i))));
  nBox.append(nList);
  cols2.append(fBox, nBox);
  page.append(cols2);
}

// ───────────────────────── настройки: текст песен ─────────────────────────

function renderLyricsSettings() {
  const c = $("#content");
  c.replaceChildren();
  c.append(el("div", "section-title", L("Отображение")));
  const toggle = (label, key, pref) => {
    const row = el("label", "check-row");
    const box = el("input");
    box.type = "checkbox";
    box.checked = !!S[key];
    box.addEventListener("change", async () => {
      S[key] = box.checked;
      renderLyrics();
      await invoke("save_prefs", { [pref]: box.checked }).catch((e) => toast(errMsg(e), "error"));
    });
    row.append(box, el("span", null, L(label)));
    c.append(row);
  };
  toggle("Скрывать строки частей песни: [Куплет], [Припев], [Бридж]…", "lyrHideParts", "lyrics_hide_parts");
  toggle("Убирать пустые строки между частями", "lyrCompact", "lyrics_compact");
}

// ───────────────────────── настройки: общее (язык) ─────────────────────────

const LANGS = [["ru", "Русский"], ["en", "English"]];

function renderGeneral() {
  const c = $("#content");
  c.replaceChildren();
  c.append(el("div", "section-title", L("Язык")));
  c.append(el("p", "hint", L("Язык интерфейса. Сообщения сервисов и названия треков остаются как есть.")));
  const row = el("div", "lang-switch");
  for (const [code, name] of LANGS) {
    const b = el("button", "btn" + (I18N.lang === code ? " on" : ""), name);
    b.addEventListener("click", () => switchLang(code));
    row.append(b);
  }
  c.append(row);
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
  $("#pageTitle").textContent = L(inSettings() ? "Настройки" : PAGES[S.page]);
  rerender();
  updateNowPlaying();
  syncModes();
  syncVolume();
  syncNormalize();
  setCollapsed(document.body.classList.contains("side-collapsed"));
  renderSvcButton();
  renderSyncStatus();
  updatePlayButton();
}

// ───────────────────────── хранилище: папка загрузок и кэш треков ─────────────────────────

const tilde = (p) => p.replace(/^\/home\/[^/]+/, "~");

function renderStorage() {
  const c = $("#content");
  c.replaceChildren();

  c.append(el("div", "section-title", L("Загрузки")));
  c.append(el("p", "hint", L("Кнопка со стрелкой у трека библиотеки сохраняет его файлом в эту папку. Пустое поле — ~/Downloads.")));
  const row = el("form", "lib-head storage-row");
  const input = el("input", "input");
  input.type = "text";
  input.spellcheck = false;
  input.placeholder = "~/Downloads";
  input.value = tilde(S.downloadDir);
  input.setAttribute("aria-label", L("Путь загрузки"));
  const save = el("button", "btn primary", L("Сохранить"));
  save.type = "submit";
  row.append(el("label", "storage-label", L("Путь загрузки")), input, save);
  row.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    try {
      await invoke("save_prefs", { download_dir: input.value.trim() });
      const info = await invoke("app_info");
      S.downloadDir = info.download_dir;
      input.value = tilde(S.downloadDir);
      toast(L("Папка загрузок: {0}", tilde(S.downloadDir)));
      if (S.page === "library") renderLibraryList();
    } catch (e) {
      toast(errMsg(e), "error");
    }
  });
  c.append(row);

  c.append(el("div", "section-title", L("Кэш треков")));
  const ci = S.cacheInfo;
  let text = L("Считаю размер кэша…");
  if (ci === null) loadCacheInfo();
  else if (ci.error) text = ci.error;
  else if (!ci.count) text = L("Кэш пуст");
  else text = `${fmtSize(ci.bytes)} · ${ci.count} ${plural(ci.count, "трек", "трека", "треков")}`;
  c.append(
    el(
      "p",
      "hint",
      L("Включённый трек библиотеки — файл или трек сервиса — сохраняется в кэш и дальше играет оттуда, без запросов к сервису. Следом кэшируется следующий трек очереди. ") +
        L("Трек, убранный из библиотеки, удаляется из кэша.")
    )
  );
  // размер — отдельной заметной строкой между описанием и кнопкой
  const size = el("div", "cache-size");
  size.append(el("div", "cache-size-value", text));
  if (ci && ci.dir) size.append(el("div", "cache-size-dir", tilde(ci.dir)));
  c.append(size);
  const clear = button("trash", L("Очистить кэш"), "danger open-dir", (b) =>
    confirmClick(b, async () => {
      try {
        await invoke("cache_clear");
        toast(L("Кэш очищен"));
      } catch (e) {
        toast(errMsg(e), "error");
      }
      S.cacheInfo = null;
      if (S.page === "set-storage") renderStorage();
    }, L("Удалить все треки из кэша?"), L("Очистить"))
  );
  clear.disabled = !!ci && !ci.error && !ci.count;
  c.append(clear);
}

async function loadCacheInfo() {
  try {
    // пустой ответ — пустой кэш (иначе страница перезапрашивала бы размер без конца)
    S.cacheInfo = (await invoke("cache_info")) || { dir: "", count: 0, bytes: 0 };
  } catch (e) {
    S.cacheInfo = { error: errMsg(e) };
  }
  if (S.page === "set-storage") renderStorage();
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
  if (!S.downloads.size) return;
  const head = el("div", "section-title dl-head");
  head.append(el("span", null, L("Загрузки")));
  if ([...S.downloads.values()].some((d) => d.status !== "run")) {
    const clear = el("button", "link-btn", L("Убрать готовые"));
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

// ───────────────────────── темы ─────────────────────────

function applyTheme(r) {
  // Движок тем ядра Whisper: правила привязываются к стилю (:root[data-style="<id>"] …), роли [data-w="…"]
  // становятся нашими селекторами (whisper-vk.js), из url() остаются только файлы этой темы (r.assets), data: и #якоря.
  // Переменные из tokens идут в том же листе последними — главнее тех же переменных в CSS темы.
  const css = (r.css || "") + "\n" + (r.vars || "");
  $("#theme-css").textContent = WhisperThemeCss.scopeCss(css, r.id, new Set(r.assets || []));
  const root = document.documentElement;
  root.dataset.style = r.id;
  root.dataset.theme = r.id;
  root.dataset.base = r.base || "";
  const bg = $("#bg");
  bg.replaceChildren();
  if (r.background) {
    let m;
    if (r.background.video) {
      m = document.createElement("video");
      m.autoplay = true;
      m.muted = true;
      m.loop = true;
      m.playsInline = true;
    } else {
      m = document.createElement("img");
      m.alt = "";
    }
    m.className = "bg-media";
    m.src = r.background.url;
    bg.append(m);
  }
  S.theme = r;
  applyOverrides();
}

const OVERRIDES = [
  { key: "dim", css: "--dim", unit: "", label: "Затемнение фона", min: 0, max: 0.85, step: 0.01 },
  { key: "blur", css: "--bg-blur", unit: "px", label: "Размытие фона", min: 0, max: 40, step: 1 },
  { key: "panel", css: "--panel-opacity", unit: "", label: "Непрозрачность панелей", min: 0.2, max: 1, step: 0.01 },
];

function applyOverrides() {
  const st = document.documentElement.style;
  for (const o of OVERRIDES) {
    if (o.key in S.overrides) st.setProperty(o.css, S.overrides[o.key] + o.unit);
    else st.removeProperty(o.css);
  }
  applyFont();
}

/** Шрифт интерфейса из системы — поверх шрифтов темы (и для текста, и для заголовков). */
function applyFont() {
  const st = document.documentElement.style;
  if (S.font) {
    const fam = `"${S.font.replace(/["\\]/g, "")}", system-ui, sans-serif`;
    st.setProperty("--font-body", fam);
    st.setProperty("--font-display", fam);
  } else {
    st.removeProperty("--font-body");
    st.removeProperty("--font-display");
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
  const sel = el("select", "input");
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
  try {
    applyTheme(await invoke("theme_select", { id }));
    if (S.lib) for (const t of S.lib) t.active = t.id === id;
    if (S.page === "set-theme") renderThemes();
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

// ───────────────────────── оформление ─────────────────────────

async function renderThemes() {
  const c = $("#content");
  const input = el("input");
  input.type = "file";
  input.accept = ".wmt,.zip";
  input.hidden = true;
  input.addEventListener("change", async () => {
    const f = input.files && input.files[0];
    if (f) await installThemeBytes(new Uint8Array(await f.arrayBuffer()));
    input.value = "";
  });
  const load = el("button", "btn");
  load.append(Icons.icon("upload"), el("span", null, L("Загрузить .wmt")));
  load.addEventListener("click", () => input.click());
  const store = el("button", "btn primary");
  store.append(Icons.icon("download"), el("span", null, L("Магазин тем")));
  store.addEventListener("click", () => go("set-store"));
  $("#topActions").replaceChildren(store, load, input);

  try {
    S.themes = await invoke("themes_list");
  } catch (e) {
    toast(errMsg(e), "error");
  }
  if (S.page !== "set-theme") return;
  // перерисовка той же страницы (выбор темы, «Как в теме») — без прыжка наверх; при переходе go() уже обнулил прокрутку
  const keep = c.scrollTop;
  c.replaceChildren();
  c.append(el("div", "section-title", L("Темы")));
  const grid = el("div", "themes");
  for (const t of S.themes) {
    const card = el("div", "theme-card");
    card.tabIndex = 0;
    if (S.theme && S.theme.id === t.id) card.classList.add("active");
    const prev = el("div", "theme-prev");
    if (t.preview) prev.style.backgroundImage = `url("${t.preview}")`;
    if (t.background_color) prev.style.backgroundColor = t.background_color;
    const sw = el("div", "theme-swatch");
    for (const col of [t.accent, t.accent2]) if (col) {
      const i = el("i");
      i.style.background = col;
      sw.append(i);
    }
    prev.append(sw);
    const info = el("div", "theme-info");
    const name = el("div", "theme-name");
    if (t.author) name.append(el("span", "theme-author", t.author + " · "));
    name.append(document.createTextNode(t.name));
    info.append(name);
    if (!t.builtin) {
      info.append(
        iconButton("trash", L("Удалить тему"), (b) =>
          confirmClick(b, async () => {
            try {
              await invoke("theme_remove", { id: t.id });
              if (S.theme && S.theme.id === t.id) applyTheme(await invoke("theme_render", { id: null }));
              renderThemes();
            } catch (e) {
              toast(errMsg(e), "error");
            }
          }, L("Удалить тему?"))
        )
      );
    }
    card.append(prev, info);
    if (t.description) card.append(el("div", "theme-desc", t.description));
    card.addEventListener("click", () => selectTheme(t.id));
    card.addEventListener("keydown", (ev) => ev.key === "Enter" && selectTheme(t.id));
    grid.append(card);
  }
  c.append(grid);
  c.append(el("p", "hint", L("Темы в формате .wmt (Whisper Music Theme): файл можно перетащить прямо в окно. Свои темы лежат в ~/.config/vkmusic/themes.")));

  c.append(el("div", "section-title", L("Шрифт интерфейса")));
  c.append(renderFontPicker());

  c.append(el("div", "section-title", L("Тонкая настройка")));
  const sl = el("div", "sliders");
  const cs = getComputedStyle(document.documentElement);
  for (const o of OVERRIDES) {
    const r = el("input");
    r.type = "range";
    r.min = o.min;
    r.max = o.max;
    r.step = o.step;
    const cur = o.key in S.overrides ? S.overrides[o.key] : parseFloat(cs.getPropertyValue(o.css)) || 0;
    r.value = cur;
    const out = el("output", null, o.unit ? `${cur}${o.unit}` : Number(cur).toFixed(2));
    r.addEventListener("input", () => {
      S.overrides[o.key] = Number(r.value);
      out.textContent = o.unit ? `${r.value}${o.unit}` : Number(r.value).toFixed(2);
      applyOverrides();
    });
    r.addEventListener("change", () => invoke("save_prefs", { overrides: S.overrides }).catch(() => {}));
    sl.append(el("label", null, L(o.label)), r, out);
  }
  c.append(sl);
  const reset = el("button", "btn", L("Как в теме"));
  reset.style.margin = "8px";
  reset.addEventListener("click", async () => {
    S.overrides = {};
    applyOverrides();
    await invoke("save_prefs", { overrides: {} }).catch(() => {});
    renderThemes();
  });
  c.append(reset);
  c.scrollTop = keep;
}

// ───────────────────────── библиотека тем (github.com/Whisper-Music/whisper-themes) ─────────────────────────

/** «Магазин тем» — отдельная страница, чтобы каталог не отодвигал настройки текущей темы. */
function renderStore() {
  // «назад» — та же стрелка слева от названия, что у открытого плейлиста (syncBack)
  $("#topActions").replaceChildren();
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
  const filter = el("input", "input");
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
  box.append(
    el(
      "p",
      "hint",
      L("Темы из репозитория whisper-themes сделаны для Whisper Music: здесь применяются их цвета, шрифты и фон, а особая разметка (как у «Терминала» или «Windows 98») не повторяется — у VK Music другой интерфейс.")
    )
  );

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
  if (S.page === "set-store") {
    S.lib = null;
    const keep = $("#content").scrollTop;
    renderStore();
    $("#content").scrollTop = keep;
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
  if (S.page === "set-store") {
    S.lib = null;
    const keep = $("#content").scrollTop;
    renderStore();
    $("#content").scrollTop = keep;
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
  Object.assign(S, { loading: true, startAt: at, needAudible: true, holdPending: hold });
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
  if (S.pl.idle) return playAt(S.qi);
  invoke("player_toggle").catch((e) => toast(errMsg(e), "error"));
}

// ───────────────────────── правая панель и статистика прослушивания ─────────────────────────

/** Прослушивание текущего трека: накопленные и ещё не отправленные в Rust секунды. */
const heard = { key: null, acc: 0, lastPos: null, stat: null, prevLast: 0 };
/** Отправлять секунды порциями — не реже раза в столько секунд прослушивания. */
const STATS_CHUNK = 10;

function updateNpSide() {
  const t = S.current;
  $("#npBigTitle").textContent = t ? t.title : L("Ничего не играет");
  $("#npBigArtist").textContent = t ? t.artist : "";
  const src = !t ? "" : t.service ? SERVICES[t.service].name : quality(t);
  $("#npBigInfo").textContent = t ? [src, t.duration ? fmt(t.duration) : ""].filter(Boolean).join(" · ") : "";
  const img = $("#npBigImg");
  if (t && t.cover) {
    if (img.getAttribute("src") !== t.cover) {
      img.hidden = true;
      img.onload = () => (img.hidden = false);
      img.onerror = () => (img.hidden = true);
      img.src = t.cover;
    }
  } else {
    img.hidden = true;
    img.removeAttribute("src");
  }
  renderNpStats();
  updateLyrics();
}

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
let sideAutoCollapsed = false;

function setCollapsed(on) {
  document.body.classList.toggle("side-collapsed", on);
  $("#brand").title = on ? L("Развернуть панель") : L("Свернуть панель");
  localStorage.setItem("sideCollapsed", on ? "1" : "");
  closeNote();
  updateLyrics();
  syncNpToggle();
}

function syncNpToggle() {
  const on = npShown();
  const b = $("#npToggle");
  b.classList.toggle("on", on);
  b.title = on ? L("Спрятать правую панель") : L("Показать правую панель: обложка, статистика, текст");
}

/**
 * Кнопка справа вверху: спрятать правую панель или выдвинуть её. Если окно узкое, сначала сворачиваем
 * левую панель до значков; если и так не помещается — правая панель встаёт на место списка.
 */
function toggleNp() {
  const b = document.body.classList;
  if (npShown()) {
    b.add("np-off");
    b.remove("np-over");
    if (sideAutoCollapsed) {
      sideAutoCollapsed = false;
      setCollapsed(false);
    }
  } else {
    b.remove("np-off");
    if (!npShown() && window.innerWidth >= 1100 && !b.contains("side-collapsed")) {
      sideAutoCollapsed = true;
      setCollapsed(true);
    }
    if (!npShown()) b.add("np-over");
  }
  localStorage.setItem("npOff", b.contains("np-off") ? "1" : "");
  closeNote();
  updateLyrics();
  syncNpToggle();
}

/** Правая панель сейчас видна (зависит от ширины окна, свёрнутой левой панели и настроек). */
function npShown() {
  return getComputedStyle($("#npSide")).display !== "none";
}

let npResizeTimer = null;
window.addEventListener("resize", () => {
  clearTimeout(npResizeTimer);
  npResizeTimer = setTimeout(() => {
    // окно стало шире — «панель вместо списка» больше не нужна
    if (window.innerWidth >= 1100) document.body.classList.remove("np-over");
    closeNote();
    updateLyrics();
    syncNpToggle();
  }, 200);
});

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
  const box = $("#npLyrics");
  box.hidden = !t;
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
  const text = $("#npLyricsText");
  const src = $("#npLyricsSrc");
  const sel = $("#npLyricsLang");
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
  const shown = body == null ? tidyLyrics(lyr.lang ? lyr.trans.get(lyr.lang) : d.text) : "";
  // галочка пояснений — только если в показанном тексте они есть
  $("#npLyricsNotesBox").hidden = !/\uE000/.test(shown);
  $("#npLyricsNotes").checked = lyricsNotes;
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
  const box = $("#npLyrics").getBoundingClientRect();
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

function renderNpStats() {
  const box = $("#npStats");
  const st = heard.stat;
  box.hidden = !S.current || !st || heard.key !== S.current.key;
  if (box.hidden) return;
  $("#npPlays").textContent = String(st.plays);
  $("#npPlaysLabel").textContent = plural(st.plays, "раз дослушан", "раза дослушан", "раз дослушан");
  $("#npListened").textContent = fmtListened(st.seconds + heard.acc);
  const more = $("#npMore");
  more.replaceChildren();
  const line = (...parts) => {
    const d = el("div");
    for (const x of parts) d.append(typeof x === "string" ? document.createTextNode(x) : x);
    more.append(d);
  };
  if (st.rank) line(el("b", null, `#${st.rank}`), " " + L("из {0} по времени прослушивания", st.total));
  // «последний раз» — до этого включения: сейчас трек играет, его last уже «сейчас»
  if (heard.prevLast) line(L("Последний раз — "), el("b", null, fmtWhen(heard.prevLast)));
  else if (!st.first && !st.seconds) line(L("Слушаете впервые"));
  if (st.first) line(L("Впервые — "), el("b", null, fmtWhen(st.first, true)));
  if (S.page === "stats") renderStatsNow();
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

function updateNowPlaying() {
  const t = S.current;
  $("#npTitle").textContent = t ? t.title : L("Ничего не играет");
  $("#npArtist").textContent = t ? t.artist : "";
  const img = $("#npCover");
  if (t && t.cover) img.src = t.cover;
  else img.removeAttribute("src");
  document.title = t ? `${t.artist} — ${t.title}` : "VK Music";
  syncNpAdd();
  updateNpSide();
  updateQueueInfo();
}

/** Очередь изменилась без смены трека (панель очереди ядра: «играть следующим», убрать, переставить). */
function updateQueueInfo() {
  $("#queueInfo").textContent = S.queue.length ? L("Очередь: {0} из {1}", S.qi + 1, S.queue.length) : "";
}

/** «+» у играющего трека: трек сервиса (микс, плейлист, поиск), которого ещё нет в библиотеке. */
function syncNpAdd() {
  const t = S.current;
  $("#npMenu").hidden = !t;
  $("#npAdd").hidden = !t || !t.service || t.unavailable || !!libraryMatch(t);
  $("#npToPl").hidden = !canAddToPlaylist(t);
}

function updatePlayButton() {
  Icons.setIcon($("#playBtn"), !S.pl.paused && !S.pl.idle ? "pause" : "play");
}

function updateProgress() {
  const d = S.pl.duration || (S.current && S.current.duration) || 0;
  $("#curTime").textContent = fmt(S.pl.pos);
  $("#durTime").textContent = fmt(d);
  if (!S.seeking) setBar(d ? S.pl.pos / d : 0);
}

function setBar(f) {
  f = Math.min(1, Math.max(0, f));
  $("#barFill").style.width = f * 100 + "%";
  $("#barKnob").style.left = f * 100 + "%";
}

function initPlayer() {
  $("#playBtn").addEventListener("click", togglePlay);
  $("#npAdd").addEventListener("click", () => S.current && saveTrack(S.current));
  $("#npMenu").addEventListener("click", (ev) => {
    ev.stopPropagation();
    if (S.current) openTrackMenu(S.current, $("#npMenu"));
  });
  $("#npToPl").addEventListener("click", (ev) => {
    ev.stopPropagation();
    if (S.current) addToPlaylistMenu($("#npToPl"), S.current);
  });
  $("#nextBtn").addEventListener("click", () => next(false));
  $("#prevBtn").addEventListener("click", prev);
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
  $("#normBtn").addEventListener("click", () => {
    S.normalize = !S.normalize;
    syncNormalize();
    invoke("player_normalize", { on: S.normalize }).catch((e) => toast(errMsg(e), "error"));
  });
  $("#volBtn").addEventListener("click", () => {
    S.muted = !S.muted;
    syncVolume();
    invoke("player_volume", { volume: S.muted ? 0 : S.volume }).catch(() => {});
  });

  // перемотка
  const bar = $("#bar");
  const frac = (ev) => {
    const r = bar.getBoundingClientRect();
    return (ev.clientX - r.left) / r.width;
  };
  bar.addEventListener("pointerdown", (ev) => {
    if (!S.current) return;
    S.seeking = true;
    bar.classList.add("drag");
    bar.setPointerCapture(ev.pointerId);
    setBar(frac(ev));
  });
  bar.addEventListener("pointermove", (ev) => {
    if (S.seeking) {
      setBar(frac(ev));
      const d = S.pl.duration || S.current.duration;
      $("#curTime").textContent = fmt(Math.min(1, Math.max(0, frac(ev))) * d);
    }
  });
  bar.addEventListener("pointerup", (ev) => {
    if (!S.seeking) return;
    S.seeking = false;
    bar.classList.remove("drag");
    const d = S.pl.duration || (S.current && S.current.duration) || 0;
    const pos = Math.min(1, Math.max(0, frac(ev))) * d;
    S.pl.pos = pos;
    seekTo(pos);
    updateProgress();
  });

  // фоновое кэширование закончилось: если это включённый трек — кэшируем следующий
  listen("cache://event", (ev) => {
    const e = ev.payload;
    if (S.current && e.key === S.current.key) prefetchNext();
  });

  listen("player://event", (ev) => {
    const e = ev.payload;
    if (e.type === "state") {
      Object.assign(S.pl, { pos: e.pos, duration: e.duration, paused: e.paused, idle: e.idle });
      // трек загрузился: загружали «без звука», а ведущий комнаты уже играет — не держать
      if (S.loading && !e.idle && e.duration > 0) {
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
  $("#shuffleBtn").classList.toggle("on", S.shuffle);
  const rb = $("#repeatBtn");
  rb.classList.toggle("on", S.repeat !== "off");
  Icons.setIcon(rb, S.repeat === "one" ? "repeat1" : "repeat");
  rb.title = { off: L("Повтор выключен"), all: L("Повторять очередь"), one: L("Повторять трек") }[S.repeat];
}

function syncVolume() {
  Whisper.player.report({ volume: S.muted ? 0 : S.volume / 100 });
  Icons.setIcon($("#volBtn"), S.muted || S.volume === 0 ? "mute" : "volume");
}

// клавиатура
document.addEventListener("keydown", (ev) => {
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement && document.activeElement.tagName);
  if (ev.key === "Escape") {
    closePopups();
    closeNote();
  }
  // Ctrl+F или / — общий поиск
  if ((ev.ctrlKey && ev.key.toLowerCase() === "f") || (!typing && ev.key === "/")) {
    ev.preventDefault();
    $("#searchInput").focus();
    $("#searchInput").select();
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
  document.querySelectorAll(".nav-item[data-page]").forEach((n) => n.addEventListener("click", () => go(n.dataset.page)));
  $("#setBack").addEventListener("click", () => go("library"));
  // логотип сворачивает левую панель до значков (запоминается): в узком окне так помещается панель с текстом
  const brand = $("#brand");
  setCollapsed(localStorage.getItem("sideCollapsed") === "1");
  document.body.classList.toggle("np-off", localStorage.getItem("npOff") === "1");
  syncNpToggle();
  brand.addEventListener("click", () => {
    sideAutoCollapsed = false;
    setCollapsed(!document.body.classList.contains("side-collapsed"));
  });
  $("#npToggle").addEventListener("click", toggleNp);
  brand.addEventListener("keydown", (ev) => (ev.key === "Enter" || ev.key === " ") && (ev.preventDefault(), brand.click()));
  // перевод текста, закрытие пояснения кликом мимо и прокруткой
  $("#npLyricsLang").addEventListener("change", (ev) => selectLyricsLang(ev.target.value || null));
  $("#npLyricsNotes").addEventListener("change", (ev) => {
    lyricsNotes = ev.target.checked;
    localStorage.setItem("lyricsNotes", lyricsNotes ? "1" : "0");
    renderLyrics();
  });
  $("#npLyricsText").addEventListener("scroll", closeNote);
  document.addEventListener("click", (ev) => {
    if (noteFor && !ev.target.closest("#lyrNote")) closeNote();
  });
  // «Статистика» — страница; повторное нажатие — обратно в библиотеку
  $("#statsBtn").addEventListener("click", () => go(S.page === "stats" ? "library" : "stats"));
  // настройки открываются сразу; повторное нажатие — обратно
  $("#settingsBtn").addEventListener("click", () => {
    if (inSettings()) $("#setBack").click();
    else go("set-general");
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
  setCollapsed(document.body.classList.contains("side-collapsed"));
  S.musicDir = info.music_dir || "~/Music/vk";
  S.downloadDir = info.download_dir || "~/Downloads";
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
  renderSvcButton();
  try {
    applyTheme(await invoke("theme_render", { id: null }));
  } catch (e) {
    toast(L("Тема не загрузилась: ") + errMsg(e), "error");
  }
  initPlayer();
  if (info.player_error) toast(L(info.player_error), "error");
  go("library");
  syncAll();

  invoke("dl_caps")
    .then((c) => {
      S.caps = c;
      if (S.page === "set-services") renderServices();
      if (!c.ytdlp || !c.ffmpeg) toast(L("Для загрузки по ссылкам нужны yt-dlp и ffmpeg: sudo pacman -S yt-dlp ffmpeg"), "error");
    })
    .catch(() => {});
}

// после всех скриптов: привязка к ядру (whisper-vk.js) и движок тем подключаются после этого файла,
// а тема при запуске уже должна знать роли
document.addEventListener("DOMContentLoaded", boot);
