// Проверка настроек страницы при запуске (как lsguard.js у Whisper Yandex) — в <head> сразу после boot.js,
// до app.js и модулей ядра.
//
// Настройки страницы лежат в localStorage. Если запись повреждена (сбой диска, аварийное выключение посреди записи),
// модули молча берут значения по умолчанию и тут же сохраняют их поверх — испорченное пропадает, а вместе с ним
// история очереди, цвета, раскладка. Поэтому повреждение замечаем ДО того, как кто-то что-то запишет: backup.js
// потом предложит восстановить всё из автокопии и не будет делать новую автокопию из «сброшенных» настроек.
// Здесь ничего не исправляется и не удаляется — только запоминается в window.__lsBroken (ключ → испорченное значение).
"use strict";

(function () {
  // эти ключи всегда JSON (если есть); остальные проверяются, только если похожи на JSON
  const JSON_KEYS = ["vkm_history", "vkm_layout", "vkm_colors", "vkm_hotkeys", "vkm_discord", "vkm_obs", "vkm_tguard",
    "vkm_search_hist", "vkm_dyn", "wa_cfg"];
  const broken = {};
  let ls;
  try {
    ls = window.localStorage;
  } catch {
    return;
  }
  for (let i = 0; i < ls.length; i++) {
    const k = ls.key(i);
    if (k == null || k === "vkm_boot") continue; // снимок стиля boot.js проверяет сам
    const v = ls.getItem(k);
    if (v == null) continue;
    const looks = /^\s*[[{"]/.test(v);
    if (!looks && !JSON_KEYS.includes(k)) continue;
    try {
      JSON.parse(v);
    } catch {
      broken[k] = v;
    }
  }
  if (Object.keys(broken).length) {
    window.__lsBroken = broken;
    console.warn("lsguard: повреждены настройки", Object.keys(broken));
  }
})();
