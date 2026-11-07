// Понятные ошибки воспроизведения (как errinfo.js у Whisper Yandex): где именно проблема — во ВКонтакте или
// SoundCloud, в сети, в устройстве вывода, в самом файле, в Whisper Archive или в mpv — и что с этим делать.
// Вместо «Не удалось воспроизвести: loading failed (HTTP error 403 Forbidden)» — «Проблема в сети или в ссылке:
// сервер не отдал трек (403) … Если включён VPN или прокси — …». Исходный текст ошибки — по кнопке «Подробнее».
//
// Классы ошибок — по тексту (ошибки Rust и журнала mpv, см. player.rs → log_reason): порядок правил важен,
// первое совпадение побеждает. Тексты — через L() (i18n), исходная ошибка не переводится.
"use strict";

(function () {
  // [где, образец, что случилось, что делать]
  const RULES = [
    ["mpv", /mpv не найден|mpv остановился|Нет связи с mpv|mpv завершился/i,
      "Плеер mpv не работает", "Установите mpv и перезапустите приложение."],
    ["service", /Сессия ВКонтакте|Войдите во ВКонтакте|не вошли во ВКонтакте|authorization failed|error 5\b/i,
      "ВКонтакте не узнал вход", "Выйдите из ВКонтакте и войдите заново."],
    ["service", /ВКонтакте не дал ссылку|недоступен в вашей стране|Access denied|copyright|правообладател/i,
      "ВКонтакте не отдаёт этот трек", "Трек убрали или он недоступен в вашей стране — попробуйте другую версию через поиск."],
    ["service", /ВКонтакте.*(ограничил|Too many|flood|captcha)|error (6|9|14)\b/i,
      "ВКонтакте ограничил запросы", "Подождите минуту и попробуйте снова."],
    ["service", /SoundCloud ограничил|SoundCloud.*429/i,
      "SoundCloud ограничил запросы", "Подождите минуту и попробуйте снова."],
    ["service", /DRM|зашифрован|SoundCloud: не найдено|SoundCloud не (принял|дал)/i,
      "SoundCloud не отдаёт этот трек", "Трек защищён, удалён или доступен только по подписке Go+."],
    ["service", /ВКонтакте ответил HTTP 5|SoundCloud ответил HTTP 5/i,
      "Сервис сейчас не отвечает", "Это сбой на стороне сервиса — попробуйте позже."],
    ["link", /HTTP error 40[13]|403 Forbidden|401 Unauthorized/i,
      "Сервер не отдал трек (ссылка устарела или привязана к другому адресу)",
      "Если включён VPN или прокси — выключите его или включите для всех программ, затем включите трек снова."],
    ["link", /HTTP error 404|404 Not Found|HTTP error 410/i,
      "Трека больше нет по этой ссылке", "Скорее всего, его удалили из сервиса."],
    ["network", /error sending request|dns|resolve|Name or service not known|timed out|timeout|Connection (refused|reset)|Network is unreachable|No route to host|tls|ssl|certificate|Failed to open|HTTP error 5\d\d/i,
      "Нет связи с сервером", "Проверьте интернет, VPN или прокси."],
    ["device", /audio (output|device)|ao[/ ]|Could not open\/initialize audio|Failed to initialize audio|PulseAudio|PipeWire|ALSA|WASAPI|No such device/i,
      "Устройство вывода звука недоступно", "Выберите другое устройство: «⋯» в плеере → «Устройство вывода»."],
    ["archive", /Whisper Archive|архив/i,
      "Whisper Archive не отдал трек", "Проверьте вход в архив: Настройки → Интеграции."],
    ["file", /Файл вне папки музыки|No such file|не найден|Нет файла/i,
      "Файла нет на диске", "Его переместили или удалили — обновите библиотеку."],
    ["file", /unrecognized file format|Failed to recognize file format|Invalid data|no audio|decod|codec|demux|format/i,
      "Файл повреждён или его формат не поддерживается", "Попробуйте скачать трек заново."],
  ];
  const WHERE = {
    mpv: "в плеере",
    service: "в сервисе",
    link: "в сети или в ссылке",
    network: "в сети",
    device: "в устройстве вывода",
    archive: "в Whisper Archive",
    file: "в файле",
  };

  /** Разобрать ошибку: { where, what, hint, raw } или null, если она незнакомая. */
  function explain(e) {
    const raw = typeof e === "string" ? e : e && e.message ? e.message : String(e);
    for (const [where, re, what, hint] of RULES) {
      if (re.test(raw)) return { where, what, hint, raw };
    }
    return null;
  }

  /**
   * Показать ошибку включения трека: «Не удалось воспроизвести «Название» — проблема в сети: …» и совет.
   * Незнакомая ошибка — как раньше, текстом. Исходный текст — по «Подробнее».
   */
  function playErrorToast(title, e) {
    const info = explain(e);
    const raw = info ? info.raw : typeof e === "string" ? e : e && e.message ? e.message : String(e);
    if (!info) return toast(L("Не удалось воспроизвести «{0}»: {1}", title || "", L(raw)), "error");
    const head = L("Не удалось воспроизвести «{0}» — проблема {1}", title || "", L(WHERE[info.where]));
    const msg = `${head}: ${L(info.what)}. ${L(info.hint)}`;
    toast(msg, "error", { label: L("Подробнее"), fn: () => toast(raw, "error", null, 12000) }, 9000);
  }

  window.ErrInfo = { explain, playErrorToast };
})();
