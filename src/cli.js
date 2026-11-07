// Консоль: интерфейс VK Music в виде командной строки (как cli.js у Whisper Yandex; тема «Терминал» из библиотеки).
// Включается темой: в .wmt — «@@ settings whisper-music» → { "console": true } или
// { "console": { "prompt": "user@whisper:~$", "banner": "текст приветствия" } } (applyHooks в app.js → cliMount).
// Тема задаёт только вид (CSS для #cli — разметка Whisper Yandex: #cli.cli > .cli-out > .cl(.cmd|.err|.dim|.ok|.hl|.hd|
// .banner), .cli-status, form.cli-in > .cli-ps + input); сами команды — здесь: тема не может добавить свои или
// выполнить что-то, чего нет в списке. Всё из данных (названия, ошибки) — только textContent.
// Команды — те же действия, что у кнопок: запросы к сервисам идут через те же команды Rust и те же правила (GAP).
"use strict";

(() => {
  const HIST_KEY = "vkm_cli_hist", PREV_KEY = "vkm_cli_prev_style";
  const PROMPT_DEF = "user@vkmusic:~$";
  let root = null, out, inp, ps, status, mounted = false;
  let hist = [], hpos = -1, draft = "";
  let lastList = []; // последний нумерованный список: [{ kind: "track" | "playlist", data, all }]
  try { hist = JSON.parse(localStorage.getItem(HIST_KEY) || "[]").filter((x) => typeof x === "string").slice(-200); } catch {}

  // ---------- вывод ----------
  function line(text = "", cls = "") {
    const d = document.createElement("div");
    d.className = "cl" + (cls ? " " + cls : "");
    d.textContent = text;
    out.append(d);
    while (out.childElementCount > 1500) out.firstElementChild.remove();
    out.scrollTop = out.scrollHeight;
    return d;
  }
  const err = (t) => line(t, "err");
  const dim = (t) => line(t, "dim");
  const ok = (t) => line(t, "ok");
  const hd = (t) => line(t, "hd");
  function table(rows) {
    const w = [];
    for (const r of rows) r.forEach((c, i) => { w[i] = Math.max(w[i] || 0, String(c).length); });
    for (const r of rows) line(r.map((c, i) => (i === r.length - 1 ? String(c) : String(c).padEnd(w[i] + 2))).join(""));
  }
  const cut = (s, n) => { s = String(s || ""); return s.length > n ? s.slice(0, n - 1) + "…" : s; };
  const svc = (t) => (t.service === "vk" ? "vk" : t.service === "sc" ? "sc" : "file");
  const trackRow = (i, t) => [String(i + 1).padStart(3), cut(t.title, 44), cut(t.artist, 30), svc(t), fmt(t.duration || 0)];
  function listTracks(tracks, from = 0) {
    lastList = tracks.map((t) => ({ kind: "track", data: t, all: tracks }));
    if (!tracks.length) return dim(L("(пусто)"));
    table(tracks.slice(from, from + 50).map((t, i) => trackRow(from + i, t)));
    if (tracks.length > from + 50) dim(L("… и ещё {0}. play <номер> — включить.", tracks.length - from - 50));
    else dim(L("play <номер> — включить, add <номер> — следующим."));
  }

  // ---------- разбор ----------
  const onOff = (a) => (/^(on|1|вкл|да|true)$/i.test(a || "") ? true : /^(off|0|выкл|нет|false)$/i.test(a || "") ? false : null);
  function parseTime(s) {
    const m = /^(\d+):(\d{1,2})$/.exec(s || "");
    if (m) return +m[1] * 60 + +m[2];
    return s !== "" && isFinite(+s) ? +s : NaN;
  }
  const pick = (a) => {
    const n = parseInt(a, 10) - 1;
    return Number.isInteger(n) && n >= 0 ? lastList[n] : null;
  };
  function setOverride(key, v, min, max, scale) {
    const n = Number(v);
    if (!isFinite(n)) return NaN;
    const val = Math.max(min, Math.min(max, n)) / scale;
    S.overrides = { ...(S.overrides || {}), [key]: val };
    applyStyleVars();
    invoke("save_prefs", { overrides: S.overrides }).catch(() => {});
    return Math.round(val * scale);
  }

  // ---------- команды ----------
  // { names, args, help, group, fn }
  const CMDS = [];
  const def = (names, args, help, group, fn) => CMDS.push({ names: names.split(" "), args, help, group, fn });
  const need = () => { if (!S.current) throw new Error(L("Ничего не играет.")); };

  // Воспроизведение
  def("play p", "[номер]", "без номера — пауза/пуск; с номером — пункт из последнего списка", "Воспроизведение", async (a) => {
    if (!a[0]) return togglePlay();
    const it = pick(a[0]);
    if (!it) return err(L("Нет пункта {0} в последнем списке.", a[0]));
    if (it.kind === "playlist") return openPlaylist(it.data);
    const all = it.all.filter((t) => !t.unavailable);
    const i = all.indexOf(it.data);
    if (i < 0) return err(L("Этот трек недоступен."));
    playList(all, i);
    ok(L("▶ {0} — {1}", it.data.artist, it.data.title));
  });
  def("pause", "", "пауза", "Воспроизведение", () => { if (S.current && !S.pl.paused && !S.pl.idle) togglePlay(); });
  def("resume", "", "продолжить", "Воспроизведение", () => { if (!S.current || S.pl.paused || S.pl.idle) togglePlay(); });
  def("next n", "", "следующий трек", "Воспроизведение", () => { need(); next(false); });
  def("prev b", "", "предыдущий трек", "Воспроизведение", () => { need(); prev(); });
  def("seek", "<м:сс | сек | +сек | -сек>", "перемотать", "Воспроизведение", (a) => {
    need();
    const s = a[0] || "";
    const rel = /^[+-]/.test(s);
    const v = rel ? Number(s) : parseTime(s);
    if (!isFinite(v)) return err(L("Пример: seek 1:30, seek +15"));
    seekTo(Math.max(0, rel ? S.pl.pos + v : v));
  });
  def("vol volume", "[0–100 | +N | -N]", "громкость", "Воспроизведение", (a) => {
    if (!a[0]) return dim(L("Громкость: {0}%", S.volume));
    const v = /^[+-]/.test(a[0]) ? S.volume + Number(a[0]) : Number(a[0]);
    if (!isFinite(v)) return err(L("Пример: vol 30, vol +5"));
    setVolume(v);
    dim(L("Громкость: {0}%", S.volume));
  });
  def("mute", "", "выключить/включить звук", "Воспроизведение", () => $("#volBtn").click());
  def("speed", "[0.25–4]", "скорость воспроизведения", "Воспроизведение", (a) => {
    if (a[0]) {
      const v = Number(String(a[0]).replace(",", "."));
      if (!isFinite(v)) return err(L("Пример: speed 1.25"));
      setRate(v);
    }
    dim(L("Скорость: {0}", (S.rate || 1) + "×"));
  });
  def("shuffle", "[on|off]", "перемешивание", "Воспроизведение", (a) => {
    const want = onOff(a[0]);
    if (want === null || want !== S.shuffle) toggleShuffle();
    dim(L("Перемешивание: {0}", S.shuffle ? "on" : "off"));
  });
  def("repeat", "[off|all|one]", "повтор", "Воспроизведение", (a) => {
    const want = (a[0] || "").toLowerCase();
    if (want && !["off", "all", "one"].includes(want)) return err(L("Пример: repeat all"));
    if (!want) cycleRepeat();
    else for (let i = 0; i < 3 && S.repeat !== want; i++) cycleRepeat();
    dim(L("Повтор: {0}", S.repeat));
  });
  def("norm", "[on|off]", "постоянный уровень громкости", "Воспроизведение", (a) => {
    const want = onOff(a[0]);
    if (want === null || want !== S.normalize) toggleNormalize();
    dim(L("Постоянный уровень громкости: {0}", S.normalize ? "on" : "off"));
  });
  def("like", "", "в библиотеку (текущий трек)", "Воспроизведение", () => { need(); $("#likeBtn").click(); });
  def("now np", "", "что играет", "Воспроизведение", () => {
    need();
    const t = S.current;
    line(`${t.artist} — ${t.title}`, "hl");
    dim(`${svc(t)} · ${fmt(S.pl.pos)} / ${fmt(S.pl.duration || t.duration || 0)} · ${S.pl.paused ? "⏸" : "▶"}`);
  });
  def("queue q", "", "что дальше в очереди", "Воспроизведение", () => {
    const rest = S.queue.slice(S.qi + 1, S.qi + 21);
    if (!rest.length) return dim(L("Дальше в очереди ничего нет."));
    hd(L("Дальше"));
    listTracks(rest);
  });
  def("add", "<номер>", "поставить трек из последнего списка следующим", "Воспроизведение", (a) => {
    const it = pick(a[0]);
    if (!it || it.kind !== "track") return err(L("Нет трека {0} в последнем списке.", a[0] || ""));
    window.playNext(it.data);
  });
  def("lyrics", "", "текст песни (Esc — назад)", "Воспроизведение", () => { need(); openFull("lyrics"); });

  // Музыка
  def("search s", "<запрос>", "поиск во ВКонтакте и SoundCloud", "Музыка", async (a) => {
    const q = a.join(" ").trim();
    if (q.length < 2) return err(L("Пример: search daft punk"));
    dim(L("Ищу «{0}»…", q));
    const [vk, sc] = await Promise.allSettled([
      invoke("vk_search", { query: q, more: null, live: false }).then((p) => p.tracks.map((x) => svcTrack("vk", x))),
      invoke("sc_search", { query: q, offset: 0, live: false }).then((p) => p.tracks.map((x) => svcTrack("sc", x))),
    ]);
    const list = [];
    for (const [name, r] of [["ВКонтакте", vk], ["SoundCloud", sc]]) {
      if (r.status === "fulfilled") list.push(...r.value);
      else dim(`${name}: ${errMsg(r.reason)}`);
    }
    listTracks(list.filter((t) => !t.unavailable));
  });
  def("lib library", "[страница]", "библиотека (по 50)", "Музыка", (a) => {
    const page = Math.max(1, parseInt(a[0], 10) || 1);
    const all = visibleTracks();
    hd(L("Библиотека: {0}", all.length));
    listTracks(all, (page - 1) * 50);
  });
  def("find f", "<текст>", "найти в библиотеке", "Музыка", (a) => {
    const q = a.join(" ").toLowerCase().trim();
    if (!q) return err(L("Пример: find cure"));
    listTracks(visibleTracks().filter((t) => matches(t, q)));
  });
  def("mix", "", "VK Микс", "Музыка", async () => {
    await playMix("common");
    if (S.current) ok(L("▶ VK Микс: {0} — {1}", S.current.artist, S.current.title));
  });
  def("playlists pl", "[номер]", "ваши плейлисты ВК; с номером — треки плейлиста", "Музыка", async (a) => {
    if (a[0]) {
      const it = pick(a[0]);
      if (!it || it.kind !== "playlist") return err(L("Нет плейлиста {0} в последнем списке.", a[0]));
      return openPlaylist(it.data);
    }
    await loadMyPlaylists();
    const list = (S.myPl && S.myPl.list) || [];
    if (S.myPl && S.myPl.error) return err(errMsg(S.myPl.error));
    lastList = list.map((p) => ({ kind: "playlist", data: p }));
    if (!list.length) return dim(L("(пусто)"));
    table(list.map((p, i) => [String(i + 1).padStart(3), cut(p.title, 50), String(p.count ?? "")]));
    dim(L("pl <номер> — треки плейлиста."));
  });
  async function openPlaylist(p) {
    dim(L("Открываю «{0}»…", p.title));
    const page = await invoke("vk_playlist", { owner_id: p.owner_id, id: p.id, offset: 0 });
    hd(p.title);
    listTracks(page.tracks.map((x) => svcTrack("vk", x)).filter((t) => !t.unavailable));
  }
  def("dl download", "", "скачать текущий трек в папку загрузок", "Музыка", async () => {
    need();
    if (!libraryMatch(S.current)) return err(L("Скачать можно только трек из библиотеки."));
    await downloadTrack(libraryMatch(S.current));
  });

  // Оформление
  def("style theme", "[id]", "список стилей или переключить стиль", "Оформление", async (a) => {
    if (!a[0]) {
      const list = S.themes || [];
      table(list.map((t) => [(S.theme && S.theme.id === t.id ? "*" : " ") + " " + t.id, t.name || ""]));
      return dim(L("style <id> — переключить. gui — обычный интерфейс."));
    }
    if (!(S.themes || []).some((t) => t.id === a[0])) return err(L("Нет стиля «{0}».", a[0]));
    await selectTheme(a[0]);
  });
  def("gui exit", "", "вернуться к обычному интерфейсу (прошлый стиль)", "Оформление", async () => {
    let prev = "";
    try { prev = localStorage.getItem(PREV_KEY) || ""; } catch {}
    if (!(S.themes || []).some((t) => t.id === prev)) prev = "neon";
    await selectTheme(prev);
  });
  def("dim", "<0–85>", "затемнение фона", "Оформление", (a) => {
    const v = setOverride("dim", a[0], 0, 85, 100);
    isFinite(v) ? dim(L("Затемнение: {0}%", v)) : err(L("Пример: dim 40"));
  });
  def("blur", "<0–40>", "размытие фона", "Оформление", (a) => {
    const v = setOverride("blur", a[0], 0, 40, 1);
    isFinite(v) ? dim(L("Размытие: {0} px", v)) : err(L("Пример: blur 8"));
  });
  def("panel", "<20–100>", "непрозрачность консоли", "Оформление", (a) => {
    const v = setOverride("panel", a[0], 20, 100, 100);
    isFinite(v) ? dim(L("Непрозрачность: {0}%", v)) : err(L("Пример: panel 80"));
  });

  // Консоль
  def("help h ?", "[команда]", "список команд или справка по одной", "Консоль", (a) => {
    if (a[0]) {
      const c = find(a[0]);
      if (!c) return err(L("{0}: команда не найдена.", a[0]));
      line(`${c.names.join(", ")} ${c.args}`, "hl");
      return dim(L(c.help));
    }
    let group = "";
    for (const c of CMDS) {
      if (c.group !== group) { group = c.group; hd(L(group)); }
      line(`  ${(c.names[0] + " " + c.args).padEnd(34)}${L(c.help)}`);
    }
    dim(L("Tab — дополнить команду, ↑/↓ — история, Ctrl+L — очистить."));
  });
  def("clear cls", "", "очистить экран", "Консоль", () => { out.textContent = ""; });
  def("history", "", "история команд", "Консоль", () => hist.slice(-30).forEach((h, i, arr) => line(`${String(hist.length - arr.length + i + 1).padStart(4)}  ${h}`)));
  def("about", "", "о программе", "Консоль", () => {
    line("VK Music (Whisper Edition) — ВКонтакте, SoundCloud и свои файлы.", "hl");
    dim(L("Консоль — режим стиля «Терминал». gui — обычный интерфейс."));
  });

  const find = (name) => CMDS.find((c) => c.names.includes(String(name).toLowerCase()));
  function splitArgs(s) {
    const res = [];
    const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
    let m;
    while ((m = re.exec(s))) res.push(m[1] ?? m[2] ?? m[3]);
    return res;
  }
  async function run(raw) {
    const text = raw.trim();
    line(`${ps.textContent} ${raw}`, "cmd");
    if (!text) return;
    if (hist[hist.length - 1] !== text) {
      hist.push(text);
      hist = hist.slice(-200);
      try { localStorage.setItem(HIST_KEY, JSON.stringify(hist)); } catch {}
    }
    const [name, ...args] = splitArgs(text);
    const c = find(name);
    if (!c) return err(L("{0}: команда не найдена. help — список команд.", name));
    try {
      await c.fn(args);
    } catch (e) {
      err(L("Ошибка: {0}", e && e.message ? e.message : errMsg(e)));
    }
  }

  // ---------- строка состояния ----------
  const bar = (p, w = 24) => { const n = Math.round(Math.max(0, Math.min(1, p)) * w); return "[" + "#".repeat(n) + "-".repeat(w - n) + "]"; };
  function paintStatus() {
    if (!mounted) return;
    const t = S.current;
    if (!t) { status.textContent = L("■ ничего не играет · help — команды"); return; }
    const cur = S.pl.pos || 0, dur = S.pl.duration || t.duration || 0;
    const flags = [S.shuffle ? "shuffle" : "", S.repeat !== "off" ? "repeat:" + S.repeat : "", (S.rate || 1) !== 1 ? S.rate + "×" : "",
      libraryMatch(t) ? "♥" : "", S.normalize ? "norm" : ""].filter(Boolean).join(" ");
    status.textContent = `${!S.pl.paused && !S.pl.idle ? "▶" : "⏸"} ${cut(t.title, 40)} — ${cut(t.artist, 28)}  ${bar(dur ? cur / dur : 0)} ${fmt(cur)}/${fmt(dur)}  vol ${S.muted ? 0 : S.volume}%${flags ? "  " + flags : ""}`;
  }
  setInterval(paintStatus, 500);

  // ---------- сборка ----------
  function build() {
    root = document.createElement("div");
    root.id = "cli";
    root.className = "cli";
    out = document.createElement("div");
    out.className = "cli-out";
    out.id = "cliOut";
    out.setAttribute("role", "log");
    out.setAttribute("aria-live", "polite");
    status = document.createElement("div");
    status.className = "cli-status";
    status.id = "cliStatus";
    const form = document.createElement("form");
    form.className = "cli-in";
    form.autocomplete = "off";
    ps = document.createElement("span");
    ps.className = "cli-ps";
    inp = document.createElement("input");
    inp.id = "cliIn";
    inp.spellcheck = false;
    inp.autocomplete = "off";
    inp.setAttribute("aria-label", L("Команда"));
    form.append(ps, inp);
    root.append(out, status, form);
    document.body.append(root);
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const v = inp.value;
      inp.value = "";
      hpos = -1;
      run(v);
    });
    inp.addEventListener("keydown", (e) => {
      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        e.preventDefault();
        e.stopPropagation();
        if (!hist.length) return;
        if (hpos === -1) { draft = inp.value; hpos = hist.length; }
        hpos = Math.max(0, Math.min(hist.length, hpos + (e.key === "ArrowUp" ? -1 : 1)));
        inp.value = hpos === hist.length ? draft : hist[hpos];
        if (hpos === hist.length) hpos = -1;
      } else if (e.key === "Tab") {
        e.preventDefault();
        const w = inp.value.trimStart();
        if (w.includes(" ")) return;
        const hits = [...new Set(CMDS.flatMap((c) => c.names).filter((n) => n.startsWith(w.toLowerCase())))];
        if (hits.length === 1) inp.value = hits[0] + " ";
        else if (hits.length > 1) dim(hits.join("  "));
      } else if (e.key === "l" && e.ctrlKey) {
        e.preventDefault();
        out.textContent = "";
      } else if (e.key === "c" && e.ctrlKey && inp.selectionStart === inp.selectionEnd) {
        e.preventDefault();
        line(`${ps.textContent} ${inp.value}^C`, "cmd");
        inp.value = "";
        hpos = -1;
      }
    });
    root.addEventListener("mouseup", () => { if (!getSelection().toString()) inp.focus(); });
  }

  /** Включить / выключить консоль (applyHooks в app.js): o — settings.console темы (true или { prompt, banner }). */
  window.cliMount = (o) => {
    const want = !!o;
    if (!want) {
      if (mounted) {
        mounted = false;
        document.body.classList.remove("cli-on");
      }
      // последний обычный стиль — к нему вернёт gui
      try { if (S.theme && S.theme.id) localStorage.setItem(PREV_KEY, S.theme.id); } catch {}
      return;
    }
    if (!root) build();
    const opts = typeof o === "object" ? o : {};
    ps.textContent = String(typeof opts.prompt === "string" && opts.prompt ? opts.prompt : PROMPT_DEF).slice(0, 40);
    if (mounted) return;
    mounted = true;
    document.body.classList.add("cli-on");
    if (!out.childElementCount) {
      const banner = typeof opts.banner === "string" && opts.banner ? opts.banner : "VK Music · терминал";
      for (const l of banner.split("\n").slice(0, 20)) line(l.slice(0, 200), "banner");
      dim(L("help — список команд. gui — обычный интерфейс."));
    }
    paintStatus();
    setTimeout(() => inp.focus(), 50);
  };
})();
