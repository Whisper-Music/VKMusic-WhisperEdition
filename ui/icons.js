// Простые значки (свои, stroke = currentColor). <i data-icon="имя"> → <svg>.
(function () {
  const P = {
    heart: '<path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z"/>',
    search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>',
    spark: '<path d="M12 3v4M12 17v4M3 12h4M17 12h4M6.3 6.3l2.5 2.5M15.2 15.2l2.5 2.5M6.3 17.7l2.5-2.5M15.2 8.8l2.5-2.5"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    palette: '<path d="M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.8 1.6-1.6 0-1.2-1-1.4-1-2.4 0-.9.7-1.5 1.6-1.5H16a5 5 0 0 0 5-5C21 6.6 17 3 12 3z"/><circle cx="7.5" cy="11" r="1"/><circle cx="10" cy="7.3" r="1"/><circle cx="14.6" cy="7.3" r="1"/>',
    logout: '<path d="M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3"/><path d="M10 16l-4-4 4-4M6 12h10"/>',
    play: '<path d="M8 5.5v13l10.5-6.5z" fill="currentColor" stroke="none"/>',
    pause: '<path d="M8 5h3v14H8zM13.5 5h3v14h-3z" fill="currentColor" stroke="none"/>',
    prev: '<path d="M7 5v14"/><path d="M18 5.5v13L9 12z" fill="currentColor" stroke="none"/>',
    next: '<path d="M17 5v14"/><path d="M6 5.5v13L15 12z" fill="currentColor" stroke="none"/>',
    shuffle: '<path d="M4 7h3c4 0 6 10 10 10h3M4 17h3c1.6 0 2.8-1.5 3.8-3.4M14 9.4C15 7.6 16 7 17 7h3"/><path d="M18 5l2 2-2 2M18 15l2 2-2 2"/>',
    repeat: '<path d="M4 11V9a3 3 0 0 1 3-3h12l-3-3M20 13v2a3 3 0 0 1-3 3H5l3 3"/>',
    repeat1: '<path d="M4 11V9a3 3 0 0 1 3-3h12l-3-3M20 13v2a3 3 0 0 1-3 3H5l3 3"/><path d="M11.5 10.5l1.5-1v5.5" stroke-width="1.6"/>',
    volume: '<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/>',
    mute: '<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    listadd: '<path d="M4 6h12M4 11h12M4 16h7"/><path d="M17.5 13v7M14 16.5h7"/>',
    listremove: '<path d="M4 6h12M4 11h12M4 16h7"/><path d="M14 16.5h7"/>',
    edit: '<path d="M4.5 19.5h4l10-10-4-4-10 10z"/><path d="M13 7l4 4"/>',
    check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
    trash: '<path d="M4.5 7h15M10 4h4M6.5 7l1 13h9l1-13"/>',
    download: '<path d="M12 4v11M7 10.5l5 5 5-5M5 20h14"/>',
    note: '<path d="M9 18V5.5l10-2V16"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>',
    disk: '<rect x="3.5" y="13" width="17" height="6.5" rx="2"/><path d="M5 13l2.5-7.5h9L19 13"/><circle cx="16.5" cy="16.3" r=".6" fill="currentColor"/>',
    mix: '<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="2.5"/><path d="M12 3.5a8.5 8.5 0 0 1 8.5 8.5M12 7a5 5 0 0 1 5 5"/>',
    lyrics: '<path d="M4 6h11M4 10h11M4 14h7M4 18h7"/><path d="M19 13v6.5"/><circle cx="17" cy="19.5" r="2"/><path d="M19 13l2 1"/>',
    globe: '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.4 2.3 3.6 5.1 3.6 8.5s-1.2 6.2-3.6 8.5c-2.4-2.3-3.6-5.1-3.6-8.5s1.2-6.2 3.6-8.5z"/>',
    panel: '<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M14.5 4.5v15"/>',
    chart: '<path d="M4 20h16"/><path d="M7 16v-5M12 16V6M17 16v-8"/>',
    hide: '<path d="M3 3l18 18"/><path d="M10.6 6.1A9.8 9.8 0 0 1 12 6c5 0 8.5 4.2 9.5 6-.4.8-1.3 2-2.5 3.2M6.6 7.6C4.6 9 3.2 10.9 2.5 12c1 1.8 4.5 6 9.5 6 1.6 0 3-.4 4.3-1.1"/><path d="M9.9 10a3 3 0 0 0 4.1 4.1"/>',
    upload: '<path d="M12 20V9M7 13.5l5-5 5 5M5 4h14"/>',
    open: '<path d="M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4"/>',
    refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6"/>',
    dots: '<circle cx="5.5" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.4" fill="currentColor" stroke="none"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
    gear: '<circle cx="12" cy="12" r="3"/><path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7"/><circle cx="12" cy="12" r="6.5"/>',
    // «постоянный уровень громкости»: столбики одной высоты
    levels: '<path d="M5 9v6M9.5 7v10M14.5 7v10M19 9v6"/><path d="M3 12h18" stroke-dasharray="1.5 2.5"/>',
    back: '<path d="M14.5 5.5L8 12l6.5 6.5"/>',
    link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
    // значки сервисов — свои, не логотипы
    vk: '<rect x="2.5" y="4.5" width="19" height="15" rx="4"/><text x="12" y="15.6" text-anchor="middle" font-size="8.5" font-weight="700" font-family="system-ui, sans-serif" fill="currentColor" stroke="none">VK</text>',
    sc: '<path d="M7 17.5h10.5a3.5 3.5 0 0 0 .4-7 5 5 0 0 0-9.6-1.3A4 4 0 0 0 7 17.5z"/><path d="M4 12v5.5M2 13.5v3"/>',
  };
  function svg(name) {
    return '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + (P[name] || "") + "</svg>";
  }
  function hydrate(root) {
    (root || document).querySelectorAll("i[data-icon]").forEach((el) => {
      el.innerHTML = svg(el.dataset.icon);
      el.classList.add("ic");
    });
  }
  function icon(name) {
    const i = document.createElement("i");
    i.className = "ic";
    i.dataset.icon = name;
    i.innerHTML = svg(name);
    return i;
  }
  function setIcon(el, name) {
    const i = el.querySelector("i[data-icon]") || el;
    i.dataset.icon = name;
    i.innerHTML = svg(name);
  }
  window.Icons = { hydrate, icon, setIcon };
})();
