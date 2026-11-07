// Резервные копии (Rust: src-tauri/src/backup.rs; как backup.js у Whisper Yandex): карточка «Резервная копия»
// во вкладке «Хранилище» — сохранить копию в папку загрузок, восстановить из файла или из автокопии; автокопия —
// раз в день при запуске. Повреждённые настройки (lsguard.js → window.__lsBroken, config.json — app_info.config_broken)
// — подсказка «Восстановить» из последней автокопии, а новая автокопия в этот запуск не делается (иначе она
// сохранила бы уже сброшенные настройки).
"use strict";

(function () {
  /** Ключи localStorage, которые не входят в копию: снимок стиля и индекс архива пересобираются сами. */
  const SKIP = ["vkm_boot", "wa_index_v3"];
  let autos = null; // список автокопий (backup_list), загружается при открытии вкладки

  function collectStorage() {
    const out = {};
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k != null && !SKIP.includes(k)) out[k] = localStorage.getItem(k);
    }
    return out;
  }

  const fmtDate = (unix) => new Date(unix * 1000).toLocaleString(I18N.locale, { dateStyle: "medium", timeStyle: "short" });
  const fmtSize = (b) => (b >= 1048576 ? (b / 1048576).toFixed(1) + " " + L("МБ") : Math.max(1, Math.round(b / 1024)) + " " + L("КБ"));

  /** Восстановить открытую копию: настройки страницы — сразу, файлы — Rust при перезапуске. */
  async function apply(info) {
    try {
      await invoke("backup_apply", { storage: collectStorage() });
    } catch (e) {
      return toast(errMsg(e), "error");
    }
    // настройки страницы — ровно как в копии
    for (const k of Object.keys(collectStorage())) localStorage.removeItem(k);
    for (const [k, v] of Object.entries(info.storage || {})) {
      try { localStorage.setItem(k, v); } catch {}
    }
    toast(L("Копия восстановлена — перезапуск…"), "ok");
    setTimeout(() => invoke("app_restart").catch((e) => toast(errMsg(e), "error")), 400);
  }

  /** Открыть копию и спросить у кнопки, восстанавливать ли. */
  async function offer(btn, open) {
    let info;
    try {
      info = await open();
    } catch (e) {
      return toast(errMsg(e), "error");
    }
    const what = L("Копия от {0}: {1}, тем — {2}. Текущие настройки сохранятся отдельной копией, приложение перезапустится.",
      fmtDate(info.created), info.files + " " + plural(info.files, "файл настроек", "файла настроек", "файлов настроек"), info.themes);
    confirmClick(btn, () => apply(info), what, L("Восстановить"));
  }

  function chooseFile(btn) {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".vkmbak";
    input.addEventListener("change", async () => {
      const f = input.files && input.files[0];
      if (!f) return;
      const bytes = new Uint8Array(await f.arrayBuffer());
      offer(btn, () => invoke("backup_read", bytes));
    });
    input.click();
  }

  async function exportNow(btn) {
    btn.disabled = true;
    try {
      const path = await invoke("backup_export", { storage: collectStorage() });
      toast(L("Копия сохранена: {0}", path.replace(/^\/home\/[^/]+/, "~")), "ok",
        { label: L("Открыть папку"), fn: () => invoke("open_download_dir").catch((e) => toast(errMsg(e), "error")) }, 8000);
    } catch (e) {
      toast(errMsg(e), "error");
    } finally {
      btn.disabled = false;
    }
  }

  /** Карточка во вкладке «Хранилище» (renderSettings в app.js). */
  window.renderBackup = function renderBackup() {
    const c = setCard("out", L("Резервная копия"));
    c.append(el("p", "status", L("Настройки, библиотека сервисов, плейлисты, статистика, свои темы и оформление — одним файлом. Вход в сервисы в копию не входит.")));
    const row = el("div", "row bk-row");
    const save = el("button", "nav active", L("Сохранить копию"));
    save.type = "button";
    save.addEventListener("click", () => exportNow(save));
    const load = el("button", "nav", L("Восстановить из файла…"));
    load.type = "button";
    load.addEventListener("click", () => chooseFile(load));
    row.append(save, load);
    c.append(row);

    const list = el("div", "backup-list");
    c.append(el("h4", null, L("Автокопии")), list);
    const fill = () => {
      list.replaceChildren();
      const items = autos || [];
      if (!items.length) list.append(el("p", "status", autos ? L("Пока нет — первая появится вскоре после запуска.") : L("Загрузка…")));
      for (const a of items) {
        const r = el("div", "backup-item");
        const kind = a.name.startsWith("before-restore") ? L("перед восстановлением") : L("автокопия");
        r.append(el("span", "bk-date", fmtDate(a.created)), el("span", "bk-meta", `${kind} · ${fmtSize(a.size)}`));
        const b = el("button", "nav", L("Восстановить"));
        b.type = "button";
        b.addEventListener("click", () => offer(b, () => invoke("backup_read_auto", { name: a.name })));
        r.append(b);
        list.append(r);
      }
    };
    fill();
    if (autos == null) {
      invoke("backup_list")
        .then((l) => { autos = l; if (list.isConnected) fill(); })
        .catch(() => { autos = []; if (list.isConnected) fill(); });
    }
  };

  /** При запуске (boot в app.js): повреждённые настройки — предложить автокопию, иначе — автокопия раз в день. */
  window.backupBoot = function backupBoot(info) {
    const broken = window.__lsBroken || (info && info.config_broken);
    if (broken) {
      invoke("backup_list").then((l) => {
        const last = l.find((a) => a.name.startsWith("auto-"));
        if (!last) return toast(L("Часть настроек повреждена и сброшена, автокопий для восстановления нет."), "error", null, 10000);
        toast(L("Часть настроек повреждена. Восстановить из автокопии от {0}?", fmtDate(last.created)), "error", {
          label: L("Восстановить"),
          fn: () => invoke("backup_read_auto", { name: last.name }).then(apply).catch((e) => toast(errMsg(e), "error")),
        }, 0);
      }).catch(() => {});
      return;
    }
    // не сразу: запуск и так занят (тема, библиотека, синхронизация)
    setTimeout(() => {
      invoke("backup_auto", { storage: collectStorage() })
        .then((made) => { if (made) autos = null; })
        .catch((e) => console.warn("автокопия:", e));
    }, 30000);
  };
})();
