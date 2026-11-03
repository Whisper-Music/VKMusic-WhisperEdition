# Темы VK Music (`.wmt`)

VK Music читает темы в формате **Whisper Music Theme** — том же, что у
[YandexMusic-WhisperEdition](https://github.com/Whisper-Music/YandexMusic-WhisperEdition)
(описание формата — [`docs/WMT-FORMAT.md`](https://github.com/Whisper-Music/YandexMusic-WhisperEdition/blob/main/docs/WMT-FORMAT.md) там).
Файл читает `src-tauri/src/wmt.rs` (своя реализация, проверена на примерах тем WhisperEdition), а CSS темы
к окну применяет **движок тем ядра Whisper** — `src/core/theme-css.js` из
[whisper-core](https://github.com/Whisper-Music/whisper-core) (строгое описание формата — `docs/WMT-FORMAT.md` там же).

* **id приложения** для секций: `whisper-vk` → `@@ css whisper-vk`, `@@ settings whisper-vk` (как «Whisper VK» в ядре).
  Прежнее имя `vkmusic` (`@@ css vkmusic`) по-прежнему читается — если секций `whisper-vk` в теме нет.
* **`@@ css whisper-core`** — общая секция тем Whisper: правила по **ролям** интерфейса (`[data-w="player.play"]`,
  таблица ниже), а не по нашим классам. Такая тема одинаково ложится на все приложения Whisper. Порядок:
  `whisper-core` → `whisper-vk` (своя секция главнее) → переменные из `tokens`.
* **`tokens`** понимаются целиком — поэтому любая тема WhisperEdition сразу перекрашивает VK Music
  (её `@@ css whisper-music` пропускается: там другая разметка).
* Тема с фоном — zip, переименованный в `.wmt`: `theme.wmt` + `background.<jpg|png|webp|gif|avif|bmp|mp4|webm|m4v|mov>` + любые файлы
  (шрифты, картинки), доступные как `url(asset:имя)`.
* **Темы Whisper Music** (в том числе из «Оформление → Магазин тем», репозиторий whisper-themes) работают
  так: `tokens` и фон применяются целиком, из `@@ css whisper-music` берутся только правила `@font-face`
  (чтобы подключились шрифты из архива темы), остальная разметка пропускается.
* Библиотека скачивает только `index.json`, `themes/<категория>/<id>.wmt` и `previews/<категория>/<id>.(jpg|png|webp)` с
  `raw.githubusercontent.com/Whisper-Music/whisper-themes/main/`; файл проверяется так же, как загруженный вручную,
  и `manifest.id` в нём должен совпадать с записью в `index.json`. Обновление предлагается, когда
  `version` в библиотеке отличается от `manifest.version` установленной темы.
* Свои темы: «Оформление → Загрузить .wmt» или перетащить файл в окно. Хранятся в `~/.config/vkmusic/themes/<id>.wmt`.
  Повторная загрузка с тем же `manifest.id` обновляет тему. id встроенных тем (`osaka-jade`, `vantablack`, `ashen-inventory`, `slate`) заняты.

## Собрать тему из папки

```sh
cp -r themes/osaka-jade themes/my-theme        # theme.wmt, background.webp, preview.jpg
$EDITOR themes/my-theme/theme.wmt                # поменять manifest.id и name, цвета, CSS
scripts/pack-theme.sh themes/my-theme            # → themes/my-theme.wmt
```

## tokens → CSS-переменные

| tokens | переменная |
|---|---|
| `color.accent` / `color.accent2` (`#rrggbb`) | `--accent`, `--accent-rgb` / `--accent2`, `--accent2-rgb` |
| `color.text`, `color.muted`, `color.background` | `--text`, `--muted`, `--bg` (+ `--bg-rgb`, если `#rrggbb`) |
| `effects.glow` (0–48) | `--glow` (px) |
| `effects.panelOpacity` (0.2–1) | `--panel-opacity` |
| `effects.dim` (0–0.85) | `--dim` |
| `effects.backgroundBlur` | `--bg-blur` (px) |
| `font.body`, `font.display` | `--font-body`, `--font-display` |

Переменные из `tokens` подключаются **после** CSS темы и главнее его. Ползунки «Тонкая настройка»
(затемнение, размытие, непрозрачность панелей) и выбранный «Шрифт интерфейса» (`--font-body`, `--font-display`)
главнее и того, и другого.

Дополнительные переменные, которые тема может задать в `:root` своего CSS:
`--heading`, `--text-soft`, `--selection`, `--border`, `--border-active`, `--hover`, `--danger`, `--ok`, `--warn`,
`--radius`, `--radius-sm`, `--gap`, `--panel-blur`, `--side-width`.

## Разметка

```
body(.settings-mode)(.login-mode)(.side-collapsed)(.np-off)(.np-over)(.drag-sorting)
                                             .settings-mode — строка поиска .gsearch скрыта (visibility: hidden);
                                             .side-collapsed — левая панель свёрнута до значков (нажатие на .brand);
                                             .np-off — правая панель спрятана кнопкой .np-toggle; .np-over — на узком
                                             окне правая панель на месте списка; .drag-sorting — тащат трек
                                             html[lang=ru|en] — язык интерфейса
├── #bg.bg > .bg-media (img | video)       фон темы
├── .bg-dim                                затемнение (--dim)
├── #app.app                                сетка окна
│   ├── aside.side.panel
│   │   ├── .brand > .brand-mark, span > b
│   │   ├── nav.nav#mainNav > button.nav-item[data-page=library|vk](.active) > .nav-badge
│   │   ├── nav.nav.settings-nav#setNav    в настройках вместо #mainNav:
│   │   │     > button.nav-item.nav-back, .nav-caption, button.nav-item[data-page=set-general|set-lyrics|set-services|set-theme|set-storage]
│   │   ├── .queue-mini
│   │   ├── button.side-foot#statsBtn(.active)      страница «Статистика» (повторное нажатие — библиотека)
│   │   └── button.side-foot#settingsBtn(.active)   настройки (повторное нажатие — назад)
│   ├── main.main.panel
│   │   ├── header.topbar                   сетка: заголовок | общий поиск (по центру) | справа
│   │   │   ├── .topbar-title > button.icon-btn.page-back#pageBack, h1.page-title   стрелка «назад» — в открытом плейлисте и в магазине тем
│   │   │   ├── .gsearch > form.search#searchBox > .ic, input, button.svc-btn(.off) > .ic…   значки сервисов
│   │   │   │            > .popup.svc-menu > .popup-title, label.popup-item.check > input, .ic, span
│   │   │   │            > .search-drop#searchDrop   результаты поиска поверх библиотеки:
│   │   │   │                > .result-section(.in-library) > .section-title.with-icon, .tracks, .drop-note(.error),
│   │   │   │                  .spinner.small, .btn.more; .yt-fallback
│   │   │   └── .topbar-right > .topbar-actions, button.icon-btn.np-toggle(.on)   правая панель
│   │   └── section.content
│   │       ├── .section-title(.with-icon), .hint, .empty, .spinner
│   │       ├── .lib-bar > .icon-btn, .sync-status                                     (Библиотека)
│   │       ├── .lib-playlists#libPlaylists > .section-title, .vk-row(.grab|.grabbing|.more-l|.more-r — за краем есть ещё карточки) > button.vk-pl, button.vk-pl.vk-pl-new («Новый плейлист»)
│   │       │     (ваши плейлисты ВК; строка прокручивается перетаскиванием, полосы прокрутки нет)
│   │       ├── section.content(.page-in) — анимация смены страницы
│   │       ├── .tracks > .track(.playing|.unavailable|.dragging)
│   │       │     > .t-num, .t-cover > img, .t-meta > .t-title(> .t-badge), .t-artist,
│   │       │       .t-quality(> .t-service > .ic), .t-actions > .icon-btn(.busy), .t-dur
│   │       ├── .downloads > .dl-head, .dl(.done|.failed) > .dl-name, .dl-status, .dl-bar > i
│   │       ├── .svc-cards > .svc-card > .svc-head > .ic, .svc-name, .svc-status(.on|.warn);
│   │       │     .svc-actions; .caps > .cap(.ok|.bad); .btn.open-dir                    (Сервисы)
│   │       ├── form.lib-head.storage-row > .storage-label, .input, .btn.primary;
│   │       │     .cache-size > .cache-size-value, .cache-size-dir; .btn.danger.open-dir             (Хранилище)
│   │       ├── .themes > .theme-card(.active) > .theme-prev > .theme-swatch, .theme-info, .theme-desc
│   │       ├── .lang-switch > .btn(.on)                                                  (Общее: язык)
│   │       ├── label.check-row > input, span
│   │       │                                                                            (Текст песен)
│   │       ├── .vk-mix > .ic, .vk-mix-text > .vk-mix-title, .vk-mix-desc; .btn.primary   (Миксы)
│   │       │   .vk-grid > button.vk-pl > .vk-pl-cover > img, .vk-pl-title, .vk-pl-sub
│   │       │   .vk-pl-head > .vk-pl-cover, .vk-pl-meta > .vk-pl-name, .vk-pl-sub, .vk-pl-actions > .btn.primary («Слушать»), .btn («К себе») | button.icon-btn.vk-pl-more («⋯»); .btn.more
│   │       ├── .stats-page > .stats-now (только уже 1400px), .stats-periods > .btn(.on), .stats-tiles > .stats-tile >
│   │       │     .stats-tile-value, .stats-tile-label; .chart > .chart-max, .chart-bars > .chart-col(.empty) > .chart-bar,
│   │       │     .chart-axis > span, .chart-tip; .stats-cols > .stats-list > .stats-row(.clickable|.no-cover) >
│   │       │     .stats-rank, .t-cover, .stats-name > div, .stats-meter; .stats-value                   (Статистика)
│   │       └── #themeLib > .lib-head; .lib-groups#themeLibGrid > section.lib-cat > .section-title.lib-cat-head
│   │             > span, .icon-btn (скрыть); .hint, .lib-artist, .themes.lib-grid > .theme-card.lib-card > .lib-meta > .lib-tag, .lib-size; .lib-actions;
│   │             .lib-hidden > span, .btn (скрытые категории — нажатие показывает)
│   ├── aside.np-side.panel#npSide          справа от главной панели (нет в настройках и в окне уже 1400px):
│   │     > .np-head > .np-big-cover > .ic, img; .np-big-meta > .np-big-title, .np-big-artist, .np-big-info;
│   │       .np-stats > .np-stat > .np-stat-value, .np-stat-label; .np-more > div > b;
│   │       .np-lyrics > .np-lyrics-head > span, label.np-lyrics-notes > input, span; select.np-lyrics-lang
│   │         (язык: оригинал первым), button.np-lyrics-src;
│   │         .np-lyrics-text(.note) > .lyr-tag, .lyr-ann(.open) (фрагмент с пояснением); .lyr-note(.below|.loading)
│   └── footer.player.panel
│       ├── .np > .np-cover > img, .np-meta > .np-title, .np-artist; button.icon-btn.np-add#npAdd «в библиотеку», button.icon-btn.np-add.np-topl#npToPl «в плейлист»
│       ├── .center > .controls > #shuffleBtn, #prevBtn, button.play#playBtn, #nextBtn, #repeatBtn (.on)
│       │           > .progress > .time, .bar#bar > .bar-fill, .bar-knob
│       └── .vol > #normBtn(.on) «постоянный уровень громкости», button.qtoggle#qBtn(.on) (очередь, ядро),
│                  .vol-box > #volBtn, input#vol
├── .login-bar#loginBar > span, .btn          полоса над страницей входа сервиса (окно сжато до неё)
├── .popup.confirm-pop#confirmPop > .confirm-text, .confirm-actions > .btn.danger, .btn   подтверждение
│   (.prompt-pop — с input.prompt-input и .btn.primary: название плейлиста; .pl-menu > .popup-title, .popup-item — выбор плейлиста; меню «⋯» плейлиста — .pl-menu > .popup-item(.danger))
├── aside.panel.qpanel#qPanel              «Очередь и история» (core/queue.js, классы — его):
│     > .qhead > b, .qclose; .qbody > section.qsec > h4, ul.list(.qnow|.qnext) > li(.playing|.dragging) > .grip, img,
│       div > .t, .a; .qx, .qmore, .qempty; .qhist > .qday, ul.list.qhlist > li > .qtime; .qclear
└── .toasts > .toast(.error)(.has-action > button.toast-action)
```

Общие классы: `.panel`, `.btn`, `.btn.primary`, `.icon-btn(.on)`, `.input`, `.ic` (значок SVG, цвет — `currentColor`),
`.popup` / `.popup-item` (всплывающие меню).

`.search` — общий поиск сверху.

## Роли (`@@ css whisper-core`)

Роль в селекторе — `[data-w="имя"]`; движок ядра заменяет её на наши элементы. Разновидность — через `:`
(`[data-w="track"]` совпадает и с `track:playing`), часть — через `.` (`player` ≠ `player.play`). Роль, которой здесь
нет (например, `titlebar` — у окна системная рамка), выбрасывает селектор, остальные из того же списка остаются.

| роль | у нас |
|---|---|
| `sidebar` | `.side` |
| `nav`, `nav:active` | `.side .nav-item`, `.side-foot` (`.active`) |
| `search:global`, `search:settings` | `#searchBox`; поле поиска в «Магазине тем» |
| `main`, `page.title` | `.main`, `#pageTitle` |
| `button`, `button:active`, `button:danger` | `.btn`, `.btn.primary`, `.btn.danger` |
| `tabs`, `tab`, `tab:active` | `.tabs`, `.tab`, `.tab.active` |
| `tracklist`, `track`, `track:playing` | `.tracks`, `.track`, `.track.playing` |
| `track.cover`, `track.title`, `track.artist` | `.t-cover`, `.t-title`, `.t-artist` |
| `card`, `tile` | `.svc-card`, `.theme-card`, `.stats-tile`; `.vk-pl`, `.vk-mix` |
| `player`, `player.info`, `player.cover`, `player.title`, `player.artist` | `.player`, `.np-meta`, `.np-cover`, `#npTitle`, `#npArtist` |
| `player.controls`, `player.play`, `player.prev`, `player.next` | `.controls`, `#playBtn`, `#prevBtn`, `#nextBtn` |
| `player.mode`, `player.mode:on` | `#shuffleBtn`, `#repeatBtn` (`.on`) |
| `player.seek`, `player.time` | `#bar`, `.progress .time` |
| `player.actions`, `player.volume`, `player.volume.button`, `player.volume.slider` | `.vol`, `.vol-box`, `#volBtn`, `#vol` |
| `radio` | `.vk-mix` (VK Микс) |
| `menu`, `menu.item`, `toast` | `.popup`, `.popup-item`, `.toast` |
| `queue`, `lyrics` | `#qPanel` (панель очереди ядра), `#npLyrics` |

Полный словарь ролей Whisper — `VOCAB` в `src/core/whisper-core.js`.

## Что делает движок тем

* Каждое правило темы привязывается к её стилю: `.track { … }` → `:root[data-style="<id>"] .track { … }`
  (`:root` / `html` в начале селектора — к самому `<html>`). Поэтому правила темы сильнее таких же правил приложения.
* `@keyframes`, `@font-face` и `@layer` темы получают префикс `wmt-<id>-` (ссылки на них внутри темы переименовываются
  тоже): тема не может подменить анимации и шрифты приложения, свои — сколько угодно.
* Сдвиги, повороты и увеличение (`transform`, `translate`, `scale`, `rotate`) в правилах с `:hover` / `:active`
  действуют только при `html[data-anim="on"]` (сейчас — всегда).
* `@import`, `@property`, `@page` и прочие @-правила, кроме `@media`, `@supports`, `@container`, `@layer`, `@scope`,
  `@starting-style`, `@keyframes`, `@font-face`, не поддерживаются.

Ограничения CSS темы: `@import` вырезается, `url()` работает только с `asset:` (файлы темы), `data:` и `#якорями` —
объявление с внешним адресом выбрасывается целиком; `image-set()`, `src()` и подобные — тоже.

Анимации интерфейса — короткие, с замедлением в конце: переменные `--ease-out` и `--anim`, ключевые кадры
`np-in` (правая панель), `page-in` (страница), `pop-in` (всплывашки), `lyr-in` (текст песни). Тема может их
выключить (`animation: none`) или назначить свои (`@keyframes` темы получают префикс, поэтому одноимённые
кадры приложения тема не подменяет — задайте `animation` своим кадрам). При «уменьшении движения» в системе анимаций нет.
