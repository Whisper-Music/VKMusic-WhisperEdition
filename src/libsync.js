// libsync.js — синхронизация библиотеки через Whisper Archive (просьба владельца, 2026-11-17; LIBRARY-SYNC.md архива,
// площадка `vk`): файлы папки музыки (скачанные по ссылке и свои) — те же треки на тех же местах на телефоне и
// втором компьютере. Треки ВКонтакте и SoundCloud синхронизирует сам сервис.
//
// Пути сюда не попадают: Rust (libsync.rs) отдаёт файлы с их key и сам находит файл по key. Состояние —
// localStorage `vkm_libsync` (в резервную копию не входит: на другом компьютере оно сочлось бы удалением всех треков):
//   on — выключатель, rev — последний увиденный номер изменений, account — чей ключ архива (вошли в другой — с нуля),
//   sent[key] = { s: подпись отправленных полей, f: файл на сервере (true) | время неудачной загрузки, l: время лайка }.
(() => {
  const { invoke } = window.__TAURI__.core;
  const STORE = "vkm_libsync";
  const SERVICE = "vk";
  const KEY_RE = /^[a-z0-9][a-z0-9_-]{0,63}$/;
  const BATCH = 100;
  const UPLOADS_PER_RUN = 20;           // сервер: 60 файлов за 10 минут
  const RETRY_UPLOAD = 6 * 3600 * 1000;  // неудачный файл — снова не раньше чем через 6 часов
  const EVERY = 5 * 60 * 1000;

  const read = () => {
    try {
      const v = JSON.parse(localStorage.getItem(STORE) || "null");
      if (v && typeof v === "object" && !Array.isArray(v)) return v;
    } catch {}
    return {};
  };
  let st = { on: false, rev: 0, account: "", sent: {}, ...read() };
  if (!st.sent || typeof st.sent !== "object") st.sent = {};
  const save = () => {
    try {
      localStorage.setItem(STORE, JSON.stringify(st));
    } catch {}
  };

  let running = null, soon = null, status = "", lastAt = 0;
  const setStatus = (s) => {
    status = s;
    const p = document.getElementById("libsyncStatus");
    if (p) p.textContent = s;
  };

  async function api(cmd, args) {
    const r = await invoke(cmd, args);
    let d = null;
    try {
      d = r.body ? JSON.parse(r.body) : null;
    } catch {}
    if (r.status >= 200 && r.status < 300) return d;
    const e = new Error((d && d.message) || `Whisper Archive ответил HTTP ${r.status}`);
    e.type = d && d.type;
    throw e;
  }

  const clip = (s, n) => String(s || "").replace(/[\u0000-\u001f]/g, "").trim().slice(0, n);
  /** Запись сервера из файла папки музыки. */
  function recOf(l) {
    const was = st.sent[l.key];
    return {
      key: l.key,
      kind: "import",
      app: "whisper-vk",
      title: clip(l.title, 200) || "Без названия",
      artists: l.artist ? [clip(l.artist, 120)] : [],
      durationMs: Math.max(0, Math.round(l.duration_ms || 0)),
      source: { site: "file" },
      liked: true,
      likedAt: (was && was.l) || l.added_ms || Date.now(),
    };
  }
  const sig = (r) => JSON.stringify([r.title, r.artists, r.durationMs, r.likedAt]);

  /** Восстановленные треки — на место по времени лайка среди остальных треков библиотеки (новые сверху). */
  async function place(restored, local) {
    if (!restored.length) return;
    await loadLibrary();
    const liked = new Map(local.map((l) => [l.lib, (st.sent[l.key] && st.sent[l.key].l) || l.added_ms]));
    const time = (t) => (t.path ? liked.get(t.key) || t.added * 1000 : (t.added || 0) * 1000);
    for (const r of restored) {
      const before = visibleTracks().find((t) => t.key !== r.lib && time(t) < r.liked);
      await invoke("library_move", { key: r.lib, before: before ? before.key : null }).catch(() => {});
      await loadOrder();
    }
    if (S.page === "library") renderLibrary();
  }

  async function run() {
    if (!st.on) return;
    if (!(await invoke("wa_token_exists").catch(() => false))) {
      setStatus(L("Нужен вход в Whisper Archive: Настройки → Интеграции."));
      return;
    }
    setStatus(L("Синхронизация…"));
    const me = await api("wa_me").catch(() => null);
    const acc = String((me && (me.id || (me.user && me.user.id))) || "");
    if (acc && st.account !== acc) {
      st = { on: true, rev: 0, account: acc, sent: {} };
      save();
    }
    let local = await invoke("libsync_local");
    let mine = new Map(local.map((l) => [l.key, l]));

    // 1. забрать: изменения других устройств
    const got = await api("wa_lib_list", { service: SERVICE, since: st.rev || 0 });
    const restored = [];
    let removed = 0, waiting = 0;
    for (const it of Array.isArray(got && got.items) ? got.items : []) {
      const key = String((it && it.key) || "");
      if (!KEY_RE.test(key)) continue;
      const was = st.sent[key], have = mine.get(key);
      if (it.deleted) {
        // удалили на другом устройстве: здесь удаляем, только если трек не меняли после прошлой синхронизации
        if (have && was && was.s === sig(recOf(have))) {
          await invoke("libsync_delete", { key });
          mine.delete(key);
          removed++;
        }
        delete st.sent[key];
        continue;
      }
      // свои файлы вместо треков площадки (replace) и обрезки (cut) — у VK Music их нет
      if (it.kind && it.kind !== "import") continue;
      const liked = Number(it.likedAt) || 0;
      if (have) {
        st.sent[key] = { ...(was || {}), f: it.file ? true : was && was.f, l: liked || (was && was.l) };
        continue;
      }
      // трека здесь нет. Был и пропал — это удаление здесь, его отправит шаг 2
      if (was) continue;
      if (!it.file) {
        waiting++; // файл ещё загружает другое устройство
        continue;
      }
      try {
        const artist = Array.isArray(it.artists) && it.artists.length ? String(it.artists[0]) : "";
        const lib = await invoke("libsync_restore", { key, artist, title: String(it.title || "") });
        st.sent[key] = { s: null, f: true, l: liked };
        restored.push({ key, lib, liked });
      } catch (e) {
        console.warn("libsync restore", key, e);
      }
    }
    if (got && Number(got.revision) >= 0) st.rev = Number(got.revision);
    save();
    if (restored.length || removed) {
      local = await invoke("libsync_local");
      mine = new Map(local.map((l) => [l.key, l]));
      // восстановленный файл отправлять обратно незачем: его подпись — как есть у нас
      for (const r of restored) if (mine.has(r.key)) st.sent[r.key].s = sig(recOf(mine.get(r.key)));
      save();
      await place(restored, local);
      if (removed) await loadLibrary();
    }

    // 2. отправить: новое и изменённое у нас, пропавшее у нас — удаления
    const items = [];
    for (const l of local) {
      const r = recOf(l), was = st.sent[l.key];
      if (!was || was.s !== sig(r)) items.push(r);
    }
    let gone = Object.keys(st.sent).filter((k) => !mine.has(k));
    // с диска разом пропала большая часть (не тот диск, папка не подключена) — не удаляем, а забираем заново
    if (gone.length > 10 && gone.length > Object.keys(st.sent).length / 2) {
      for (const k of gone) delete st.sent[k];
      st.rev = 0;
      gone = [];
      save();
      setTimeout(() => sync(), 2000);
    }
    let full = false;
    while (items.length || gone.length) {
      const part = items.splice(0, BATCH), del = gone.splice(0, BATCH);
      const body = {};
      if (part.length) body.items = part;
      if (del.length) body.deleted = del;
      let res;
      try {
        res = await api("wa_lib_changes", { service: SERVICE, changes: body });
      } catch (e) {
        if (e.type === "library_full") {
          full = true;
          break;
        }
        throw e;
      }
      const back = new Map((Array.isArray(res && res.items) ? res.items : []).map((x) => [String(x.key), x]));
      for (const r of part) {
        const was = st.sent[r.key] || {};
        const b = back.get(r.key);
        st.sent[r.key] = { s: sig(r), f: b ? (b.file ? true : was.f === true ? false : was.f) : was.f, l: r.likedAt };
      }
      for (const k of del) delete st.sent[k];
      save();
    }

    // 3. файлы, которых на сервере нет, — по одному
    let up = 0, upErr = "";
    for (const l of local) {
      const was = st.sent[l.key];
      if (!was || was.f === true) continue;
      if (typeof was.f === "number" && Date.now() - was.f < RETRY_UPLOAD) continue;
      if (up >= UPLOADS_PER_RUN) break;
      up++;
      try {
        const r = await invoke("libsync_upload", { key: l.key });
        if (r.status >= 200 && r.status < 300) was.f = true;
        else {
          let d = null;
          try {
            d = JSON.parse(r.body);
          } catch {}
          was.f = Date.now();
          upErr = (d && d.message) || `HTTP ${r.status}`;
          if (r.status === 429 || (d && d.type === "storage_quota_exceeded")) break;
        }
      } catch (e) {
        was.f = Date.now();
        upErr = errMsg(e);
      }
      save();
    }

    lastAt = Date.now();
    const total = local.length;
    const parts = [L("Синхронизировано: {0}", `${total} ${plural(total, "трек", "трека", "треков")}`)];
    if (restored.length) parts.push(L("получено: {0}", restored.length));
    if (removed) parts.push(L("удалено: {0}", removed));
    if (waiting) parts.push(L("ждут файла с другого устройства: {0}", waiting));
    if (full) parts.push(L("в архиве больше 1000 треков — новые не отправлены"));
    if (upErr) parts.push(L("файл не отправился: {0}", upErr));
    setStatus(parts.join(" · ") + ".");
  }

  function sync() {
    if (!st.on) return Promise.resolve();
    if (running) return running;
    running = run()
      .catch((e) => setStatus(L("Синхронизация не удалась: {0}", errMsg(e))))
      .finally(() => {
        running = null;
      });
    return running;
  }

  window.LibSync = {
    /** Библиотека изменилась (скачали, удалили файл) — синхронизировать через несколько секунд, пачкой. */
    changed() {
      if (!st.on || running) return;
      clearTimeout(soon);
      soon = setTimeout(sync, 8000);
    },
    sync,
  };

  /** Карточка «Синхронизация библиотеки» во вкладке «Хранилище» (renderSettings в app.js). */
  window.renderLibSyncCard = function renderLibSyncCard() {
    const c = setCard("out", L("Синхронизация библиотеки"));
    c.append(el("p", "status", L("Треки из папки музыки (скачанные по ссылке и свои файлы) — через Whisper Archive на телефоне и другом компьютере, на тех же местах. Треки ВКонтакте и SoundCloud синхронизирует сам сервис.")));
    c.append(switchRow(L("Синхронизировать через Whisper Archive"), st.on, (on) => {
      st.on = on;
      save();
      if (on) sync();
      else setStatus("");
      renderSettings();
    }));
    if (!st.on) return;
    const p = el("p", "status", status || (lastAt ? "" : L("Ещё не синхронизировалось.")));
    p.id = "libsyncStatus";
    const now = el("button", "nav", L("Синхронизировать сейчас"));
    now.type = "button";
    now.addEventListener("click", async () => {
      now.disabled = true;
      await sync();
      now.disabled = false;
    });
    const row = el("div", "row");
    row.append(now);
    c.append(p, row);
  };

  // при запуске — когда библиотека и вход в архив уже готовы; дальше — раз в 5 минут
  setTimeout(sync, 12000);
  setInterval(() => {
    if (!document.hidden || Date.now() - lastAt > 4 * EVERY) sync();
  }, EVERY);
})();
