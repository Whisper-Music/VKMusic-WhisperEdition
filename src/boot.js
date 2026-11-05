// Запуск без «мигания» стилей (как boot.js в Whisper Yandex).
// Стиль собирается по частям: сначала основа из разметки («Неон»), потом приходит тема из Rust (theme_render,
// theme_file, разбор .wmt). Было видно, как интерфейс рисуется «Неоном» и перекрашивается в выбранный стиль. Поэтому:
// * этот файл подключён в <head> без defer — работает до первой отрисовки: ставит на <html> стиль прошлого запуска
//   и прячет интерфейс (класс booting, правила — в whisper.css) под цветом фона этой темы (снимок vkm_boot
//   сохраняет app.js — saveBoot);
// * app.js, применив тему при запуске, зовёт bootDone — интерфейс показывается;
// * что-то сломалось или очень долго — через 4 с интерфейс показывается в любом случае.
(() => {
  const root = document.documentElement;
  root.classList.add("booting");
  try {
    const b = JSON.parse(localStorage.getItem("vkm_boot") || "null");
    if (b && typeof b === "object") {
      if (typeof b.style === "string" && /^[a-z0-9][a-z0-9-]{0,47}$/.test(b.style)) root.dataset.style = b.style;
      if (typeof b.base === "string" && /^[a-z]{1,16}$/.test(b.base)) root.dataset.base = b.base;
      // только цвет: #rgb / #rrggbb(aa) или rgb()/rgba() из getComputedStyle
      const color = (v) => typeof v === "string" && /^(#[0-9a-f]{3,8}|rgba?\([\d\s.,%]+\))$/i.test(v.trim());
      if (color(b.bg)) root.style.setProperty("--boot-bg", b.bg.trim());
      if (color(b.accent)) root.style.setProperty("--boot-accent", b.accent.trim()); // цвет индикатора загрузки
    }
  } catch {}

  let done = false;
  window.bootDone = () => {
    if (done) return;
    done = true;
    // шрифты темы ещё догружаются — подождать их, но не дольше 0,6 с
    const fonts = document.fonts?.ready || Promise.resolve();
    Promise.race([fonts, new Promise((r) => setTimeout(r, 600))]).then(() => {
      root.classList.remove("booting");
      try { window.saveBoot?.(); } catch {}
    });
  };
  setTimeout(() => window.bootDone(), 4000);
})();
