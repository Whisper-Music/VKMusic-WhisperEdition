# AGENTS.md

Руководство для ИИ-агентов и разработчиков, которые меняют этот репозиторий.
Пользовательское описание — в [README.md](README.md), формат тем — в [docs/THEMES.md](docs/THEMES.md).

## Что это

**VK Music** — локальный музыкальный плеер для Linux (Arch) и Windows, Rust + Tauri 2.
Библиотека — папка `~/Music/vk`; треки добавляются через yt-dlp; воспроизведение — mpv; оформление — темы `.wmt`
(формат Whisper Music Theme из [YandexMusic-WhisperEdition](https://github.com/Whisper-Music/YandexMusic-WhisperEdition)).
Приложение построено на **ядре Whisper** — [whisper-core](https://github.com/Whisper-Music/whisper-core), подмодуль
в `src/core` (решение владельца, 2026-11-03; в ядре оно называется «Whisper VK», id `whisper-vk`): шина событий,
плеер и очередь для общих модулей, роли интерфейса и движок CSS тем, и все общие модули (просьба владельца,
2026-11-04, по образцу YandexMusic-WhisperEdition): «Очередь и история», Whisper Archive, «Слушать вместе», Discord,
виджет OBS — вместе с их Rust-частью.
**Интерфейс — разметка Whisper Yandex** (просьба владельца, 2026-11-04: «темы Whisper должны применяться полностью»):
те же классы и id (`.app`, `.side`, `.main > .head`, `ul.list > li`, `.player > .pinfo/.pcenter/.pright`, `#settingsPage`
с вкладками, `#full`, `.titlebar` …), основа оформления — `src/whisper.css` (styles.css Whisper Yandex без изменений),
тема применяется, как там: `@@ css whisper-music` целиком.
ВКонтакте и SoundCloud — поиск и прослушивание под своим аккаунтом (см. правило «Сервисы»). Библиотека —
файлы из папки музыки плюс сохранённые треки сервисов (`library.json`, описание трека). Звук треков
библиотеки — в кэше `~/.cache/vkmusic/tracks`, скачать трек можно в папку загрузок (`~/Downloads`).
Папка `~/Music/vk` историческая: треки ВК туда не попадают.

## Сборка и проверка

```sh
sudo pacman -S --needed base-devel rust webkit2gtk-4.1 mpv yt-dlp ffmpeg python-mutagen
cargo build --release        # интерфейс вшит в бинарник (tauri feature custom-protocol), tauri-cli не нужен
./target/release/vkmusic
cargo test                   # тесты Rust (модули src-tauri/src/*.rs)
./packaging/install.sh       # установка в ~/.local/bin + ярлык
```

* Клонировать — с подмодулем: `git clone --recurse-submodules …`; в уже склонированном — `git submodule update --init`.
  Без `src/core` интерфейс не запустится (ядро — первый скрипт `index.html`).
* Раскладка как у Tauri и приложений Whisper: интерфейс — `src/` (`frontendDist: "../src"`), приложение — `src-tauri/`.
  Корневой `Cargo.toml` — рабочее пространство (`members = ["src-tauri"]`, там же `[profile.release]`), поэтому
  `cargo build` / `cargo test` — из корня, бинарник — в `target/`.
* Сборка и тесты требуют `webkit2gtk-4.1` — крейт один (бинарный), тесты собирают и Tauri.
* `themes::tests::library_preview_saved_and_removed` создаёт и сразу удаляет тему `zz-preview-test`
  в настоящей `~/.config/vkmusic/themes`.
* Интерфейс без Rust можно проверить в браузере (Playwright), подменив `window.__TAURI__` заглушкой с теми же
  командами и событиями — так проверялись все экраны.
* Перед тем как считать работу готовой: `cargo build --release` без предупреждений, `cargo test`, и
  `node --check src/app.js src/whisper-vk.js src/host.js`.
* Выпуски — `.github/workflows/release.yml` (как у Whisper Yandex): тег `vX.Y.Z` или запуск вручную → черновик в
  GitHub Releases: Linux x64 (`.deb` с зависимостями mpv/ffmpeg, `.AppImage`; ubuntu-22.04 ради старой glibc) и
  Windows x64 (NSIS `-setup.exe`, просьба владельца 2026-11-05). Автообновления (tauri-plugin-updater) нет: AUR
  или новым установщиком. Версия — одинаковая в `src-tauri/tauri.conf.json` и `src-tauri/Cargo.toml` (проверяет `guard`).
  CLI Tauri — `npx @tauri-apps/cli@2` (своего package.json нет); локально: `npx --yes @tauri-apps/cli@2 build --bundles deb`.
  Ярлык в `.deb` — `packaging/vkmusic.desktop` (`bundle.linux.deb.desktopTemplate`: нужен `%u` для ссылок whisper://).
* Windows (`src-tauri/tauri.windows.conf.json` — Tauri сам подмешивает его при сборке на Windows): установщик NSIS
  для текущего пользователя; `bin/*` → папка `bin` рядом с `vkmusic.exe` — mpv (shinchiro), ffmpeg/ffprobe с DLL
  (BtbN, LGPL shared), yt-dlp.exe: их качает `release.yml` в `src-tauri/bin` (в git не входит). Без этой папки
  сборка под Windows не пройдёт (tauri-build проверяет ресурсы). Всё платформенное — в `os.rs` (права на файлы —
  `Private`, программы — `os::tool`: на Windows — из `bin` и без консольного окна, папки/ссылки — `os::open`) и под
  `cfg`: mpv — именованный канал `\\.\pipe\vkmusic-mpv-<pid>` через tokio (синхронный канал не даёт писать,
  пока поток событий ждёт чтения) и Job Object вместо `PR_SET_PDEATHSIG`; куки — `ICoreWebView2CookieManager`
  (`wv2` в `main.rs`, не `Webview::cookies` из wry — по той же причине, что на Linux); страница входа —
  дочерний webview с `auto_resize` (GTK-раскладки нет); ключ архива — диспетчер учётных данных (без запасного файла);
  шрифты — `InstalledFontCollection` через PowerShell; `vkm://` в WebView2 — `http://vkm.localhost/`
  (`host::vkm_base`). Тесты на Windows не гоняются (только `cargo check` в CI).
* Идентификатор — `vkmusic-whisper` (`IDENT` в main.rs = `identifier` в tauri.conf.json, тест `ident_matches_config`):
  папки WebKit `~/.local/share/vkmusic-whisper` (куки входа, localStorage), запись ключа архива в связке ключей.
  Прежний `ru.bobrik.vkmusic` переносится при запуске (`migrate_ident`, `secret::from_old`). Настройки приложения —
  по-прежнему `~/.config/vkmusic`, кэш — `~/.cache/vkmusic` (не зависят от идентификатора).
* Лицензия — PolyForm Noncommercial 1.0.0 (`LICENSE`, как у Whisper Yandex и ядра); строку `Required Notice` не трогать.
* Репозиторий — https://github.com/Whisper-Music/VKMusic-WhisperEdition. В нём только приложение: ядро — подмодуль
  (`src/core`, своё — в whisper-core), темы библиотеки — в whisper-themes (здесь — только 4 встроенные, `themes/`).

## Устройство

```
src-tauri/src/main.rs      AppState, все #[tauri::command], протокол vkm://, запуск mpv в setup, выход
src-tauri/src/config.rs    ~/.config/vkmusic/config.json (serde(default) — старые/лишние поля не ломают загрузку)
src-tauri/src/player.rs    mpv в фоне: --idle, JSON IPC через unix-сокет в $XDG_RUNTIME_DIR, PR_SET_PDEATHSIG
src-tauri/src/ytdlp.rs     загрузки yt-dlp: аргументы, разбор вывода по маркерам VKMPROG/VKMTITLE/VKMFILE, события
src-tauri/src/library.rs   сканирование ~/Music/vk (глубина 3), теги/формат/битрейт/обложки — lofty; теги неизменённых файлов
                           (путь, размер, время изменения) при повторном сканировании берутся из памяти (`TAGS`)
src-tauri/src/fonts.rs     системные шрифты: `fc-list --format "%{family[0]}\n" :`
src-tauri/src/wmt.rs       чтение .wmt: текст (#WMT 1, секции @@) и zip (свой ридер: stored/deflate, без zip64)
src-tauri/src/themes.rs    встроенные темы (include_bytes!), ~/.config/vkmusic/themes, tokens → CSS-переменные,
                           основы Whisper (neon/royal/steam/lolitta) как стили без файла, theme_file (сам файл для ядра), превью
src-tauri/src/themelib.rs  библиотека тем: whisper-themes на raw.githubusercontent.com (reqwest)
src-tauri/src/vk.rs        ВКонтакте: al_audio.php с куками сессии, разбор ответов, ссылка на поток одного трека;
                           API с web-токеном сайта (VK Микс, плейлисты): web_token, api, home, my_playlists, playlist, mix
src-tauri/src/sc.rs        SoundCloud: api-v2 с client_id из скриптов сайта, oauth_token из кук, поиск/resolve/поток
src-tauri/src/saved.rs     сохранённые треки сервисов: ~/.config/vkmusic/library.json, Saved::validate
src-tauri/src/playlists.rs ваши плейлисты ВК на диске: ~/.config/vkmusic/playlists.json — список, треки открывавшихся, access_key
src-tauri/src/lyrics.rs    тексты песен с Genius: чистка названий, поиск api/search/song, оценка похожести, текст со страницы,
                           кэш ~/.cache/vkmusic/lyrics
src-tauri/src/streams.rs   кэш ссылок на потоки треков сервисов: в памяти, срок — из expires ссылки (до часа)
src-tauri/src/cache.rs     кэш треков библиотеки ~/.cache/vkmusic/tracks: копия файла / поток через ffmpeg -c copy,
                           одна задача за раз: включённый трек, затем следующий в очереди; имена для папки загрузок
src-tauri/src/stats.rs     статистика прослушивания: ~/.config/vkmusic/stats.json (версия 2) — по ключу трека дослушан раз,
                           секунд, первое/последнее прослушивание; журнал по местным дням (от интерфейса): секунды по трекам и часам
src-tauri/src/order.rs     порядок библиотеки: ~/.config/vkmusic/order.json, ключи (путь файла | vk:/sc:+id), новые сверху
src-tauri/src/whisper_host.rs  что приложение даёт Rust ядра: ключ архива (secret.rs), heavy_slot, app_origin, Discord, OBS
src-tauri/src/whisper_vk.rs    архив для VK Music: wa_save_track (файл трека библиотеки по ключу), wa_play (трек архива → mpv)
src-tauri/src/secret.rs    ключ Whisper Archive: Secret Service (keyring), без связки ключей — файл 0600 (secret_backend)
src-tauri/src/deeplink.rs  ссылки whisper://room/КОД: deep-link + single-instance, room_link_take, событие room-link
src-tauri/src/os.rs        Linux/Windows: права на файлы (`Private`), внешние программы (`tool`, `bin_dir`), `open`
src-tauri/src/host.rs      `--host [ПОРТ]`: интерфейс в браузере — HTTP-сервер на 127.0.0.1 (invoke, SSE-события, vkm/wa)
src/core/rust/…            Rust ядра через #[path]: whisper/ (архив, вход, загрузка, wa://, комнаты), discord.rs, obs.rs
src/                       index.html, app.css, app.js, icons.js — без сборщика и фреймворков
src/whisper.css            основа оформления Whisper (styles.css Whisper Yandex, PolyForm Noncommercial) — не править
src/host.js                заглушка window.__TAURI__ для браузера (--host): сервер вставляет её первым скриптом index.html
src/whisper-vk.js          привязка к ядру Whisper: setApp, роли (ui.define), провайдер, player.bind, queue.bind, ui.provide
src/i18n/                  локализация: i18n.js (L, plural, applyStatic, setLang), ru.js, en.js
src/core/                  ядро Whisper — git-подмодуль whisper-core (здесь не правится, см. «Ядро Whisper»)
themes/<id>/               исходники встроенной темы: theme.wmt, background.*, preview.jpg
themes/<id>.wmt            собранная тема — именно она вшивается в бинарник
docs/THEMES.md             темы для VK Music: что от Whisper, что своё (id, секция whisper-vk, роли, ограничения)
```

### Связь интерфейса и Rust

* Команды: `invoke(имя, { аргументы })`; все команды объявлены с `rename_all = "snake_case"`, поэтому аргументы
  в JS — тоже snake_case. Ошибки — `Result<T, String>` с понятным пользователю текстом **на русском**.
* Исключение — `theme_install`: файл темы приходит сырым телом (`invoke("theme_install", Uint8Array)`,
  `tauri::ipc::InvokeBody::Raw`).
* События Rust → JS: `player://event` (`state` / `ended` / `error`), `dl://event`
  (`title` / `progress` / `saved` / `done` / `failed`), `svc://event` (`login` / `login_closed`, поле `service`: `vk` | `sc`) — после них интерфейс прячет полосу входа,
`cache://event` (`cached` / `failed`, поле `key` — ключ трека в библиотеке) — после него кэшируется следующий трек. Плюс `tauri://drag-*` для перетаскивания `.wmt`.
* Новая команда = функция в `main.rs` + строка в `generate_handler![…]` + вызов в `src/app.js`.
  Проверяйте, что каждая команда из JS есть в обработчике (и наоборот, лишних нет). `vk_status` / `sc_status`
  вызываются как `invoke(service + "_status")` — простым grep их не найти.
* Файлы тем и обложки интерфейс получает по `vkm://localhost/…`: `/theme/<id>/<файл>`,
  `/themepreview/<id>`, `/cover?path=…`. Протокол отвечает только главному окну (`label == "main"`).
* CSP выключен (`"csp": null`) — поэтому защита держится на правилах ниже, не нарушайте их.

### Интерфейс в браузере (`--host`, просьба владельца 2026-11-05)

* `vkmusic --host [ПОРТ]` (по умолчанию 7380 — постоянный: от адреса зависит localStorage браузера): то же приложение
  без окна. Главное окно `main` создаётся скрытым с `about:blank` (`context` в `main.rs`) — оно держит куки WebKit и через
  него идут команды; интерфейс в нём не запускается. Сервер (`host.rs`, свой HTTP/1.1 на std, поток на соединение):
  файлы интерфейса — `asset_resolver`, `index.html` — с `host.js` первым скриптом; `POST /invoke/<команда>` →
  `Webview::on_message` главного окна (тело JSON или сырое — `theme_install`; ответ JSON или байты); события — SSE
  `/events` (список `host::EVENTS`: **новое событие Rust → JS — добавить и туда**); `vkm://` → `/p/vkm/…`
  (`crate::serve`), `wa://` → `/p/wa/…` (`serve_stream`). Адреса `vkm://` в интерфейсе — только через
  `convertFileSrc("", "vkm")` (`VKM` в `app.js`) или `host::vkm_base()` в Rust, не строкой.
* Защита: только 127.0.0.1; `Host` — `127.0.0.1:ПОРТ` / `localhost:ПОРТ`; каждый запрос — с кукой `vkm_key_<порт>`
  (HttpOnly, SameSite=Strict), её ставит `/?key=…` (ключ — `~/.config/vkmusic/host-key`, 0600); POST с чужим `Origin`
  отклоняется; `frame-ancestors 'none'`. Не ослаблять: через сервер доступны все команды приложения.
* Отличия от окна: `win_*` ничего не делают (кнопки окна скрыты — `.host-mode .tbctl`); вход в сервис — отдельным окном
  `svc-login` (`login_layout` не нужен); перетаскивание `.wmt` — событиями браузера (`installThemeBytes`); работает
  одна вкладка — новый поток событий шлёт прежним `host://taken`; повторный запуск открывает вкладку (`open_browser`),
  ссылка whisper:// — событием `room-link` во вкладку; выход — SIGINT/SIGTERM → `app.exit(0)` (mpv и настройки — как
  при закрытии окна). Графический сеанс нужен и с `--host`: скрытое окно GTK, окно входа в сервисы.

### Ядро Whisper (`src/core`)

* Порядок в `index.html` важен: `core/whisper-core.js` → i18n, `icons.js`, `app.js` → `whisper-vk.js` →
  `core/wmt.js`, `core/theme-css.js` → модули `archive`, `queue`, `discord`, `obs`, `together` (порядок модулей =
  порядок карточек в «Интеграциях»). CSS: `core/base.css` → `whisper.css` → `core/together.css` → `app.css` →
  `core/archive.css`, `queue.css`, `discord.css`, `obs.css` → `<style id="theme-css">`. `app.js` запускается по
  `DOMContentLoaded` (`boot`), когда роли и движок тем уже подключены.
* Ядру о плеере сообщает `app.js` — `Whisper.player.report(…)`: смена трека (`playAt`: `track`, `started` — кроме
  повтора со свежей ссылкой, `source`, `queue`), пуск/пауза (`player://event`), перемотка (`seekTo`), громкость
  (`syncVolume`), режимы (`syncModes`), догрузка микса. Новое место, где меняется что-то из этого, — тоже с отчётом.
* Привязка — `whisper-vk.js`: id приложения `whisper-vk` (секция тем), приставка хранилища модулей `vkm_`
  (история очереди — `vkm_history`; сменить = потерять историю), провайдер `vk` (трек ядра: `id` — ключ библиотеки,
  длиннее 100 символов — хэш; `provider` — `local` | `vk` | `sc`; `raw` — трек `app.js`). Очередь для ядра — `S.queue`;
  «что дальше» считает `order()` так же, как `upcoming()` в `app.js` (при перемешивании известен только следующий).
  Трек от модуля ядра превращается обратно в трек библиотеки по ключу (`fromCore`) — данные из хранилища модулей
  (история) не доверенные так же, как всё из интерфейса: Rust проверяет пути и ключи сам.
* Роли (`Whisper.ui.define` в `whisper-vk.js`) — публичный контракт с темами, как классы: меняются вместе с
  `docs/THEMES.md`. Словарь ролей — `VOCAB` в `core/whisper-core.js`.
* Набор интерфейса (`ui.provide`): подсказка с кнопкой (`toast(…, action, ms)`), подтверждение — наше окошко у кнопки
  (`confirmClick` у последней нажатой кнопки), не `window.confirm`; меню трека `menu` / `trackMenu` — своё меню «⋯»
  (`openTrackMenu` в `whisper-vk.js`, `.popup.tmenu`: «Играть следующим», пункты модулей — `trackMenu.build`, слот
  `top` — «Слушать вместе»); `settingsCard` — вкладка «Интеграции» (`data-tab="int"`) постоянной `#settingsPage` —
  модули ищут свои элементы по id, карточки не должны выпадать из документа (свои карточки `.own` перерисовываются);
  `accentColors`, `obsTheme` (секции `whisper-obs` из файла темы — `theme_file`, `WMT.open`, `obsCss`; только когда
  спросил включённый виджет), `secretSaved` (ключ лёг в файл — предупредить). Роли — те же, что у Whisper Yandex.
* Плеер для модулей (`player.bind`): `playTrack(t, { at, hold })` ставит трек сразу за играющим и включает его;
  `at` / `hold` — через `player_cue` (опции `start` и `pause` mpv до `loadfile`, действует 30 с, `player.rs`);
  `setRate` — `player_rate` (`speed` mpv); `media()` — `loading` (от включения до первой длительности от mpv),
  `startAt`; `report({ audible })` — первый `state` без паузы после загрузки. Загружали «без звука», а ведущий уже играет
  (`together.wantsPlay()`) — снимаем паузу. Перед следующим треком `next()` зовёт `together.take()`.
* Источники звука: `playAt` сначала спрашивает `Whisper.sources.resolve` (Whisper Archive: свой трек или архив ведущего
  комнаты, `waShared`). Нашёлся — `wa_play`: mpv не играет `wa://`, поэтому файл целиком (`whisper::wa_track_bytes`)
  кладётся в `~/.cache/vkmusic/archive/<uuid>.<ext>` (последние 30) и играет оттуда; ключ архива в mpv не попадает.
  Не открылся — играет сам трек.
* «Слушать вместе»: трек в комнату — `toRoom` (id — ключ библиотеки; у файла из папки музыки — **хэш, не путь**:
  в пути имя пользователя), обратно — `fromRoom`: трек библиотеки по ключу или по исполнителю + названию +
  длительности (`libraryMatch`), SoundCloud — по id, иначе заготовка `room:…`: её найдёт архив ведущего или
  **один** поиск ВКонтакте при включении (`resolveRoomTrack`; это действие пользователя — он в комнате). У трека из
  комнаты `roomId` — id комнаты, по нему ядро узнаёт «тот же трек». Комнаты — один сервис: VK Music ↔ VK Music.
* «Сохранить в архив» — `provider.archiveSave` → `wa_save_track` с **ключом** трека библиотеки: файл Rust берёт сам
  (`library_file`: папка музыки или кэш, нет в кэше — выкачивает), формат, который архив не принимает (opus в webm,
  aac в ts), перекладывается ffmpeg без перекодирования (`-c copy` в ogg/m4a). Обложка — с CDN ВКонтакте/SoundCloud
  (только `whisper_host::cover_ok`), из тегов файла или нарисованная модулем. Треки вне библиотеки — нет.
* Rust ядра: команды — в `generate_handler!` (`whisper::…`, `whisper::auth::…`, `whisper::stream::…`,
  `whisper::rooms::…`, `discord::…`, `obs::…`); `build.rs` и `capabilities` у нас без списка команд приложения, поэтому
  отдельно разрешать их не нужно. Протокол `wa` — только главному окну. Зависимости ядра: `tauri-plugin-http` (только
  реэкспорт reqwest, плагин не подключается), `getrandom`, `libc`, `tokio`; для ключа — `keyring` (Secret Service,
  vendored dbus); ссылки — `tauri-plugin-deep-link` (`plugins.deep-link.desktop.schemes` в `tauri.conf.json`) и
  `tauri-plugin-single-instance` (второй запуск отдаёт ссылку первому). Схему регистрирует `install.sh` (`xdg-mime`,
  только если у `x-scheme-handler/whisper` ещё нет хозяина) и ярлык (`MimeType`, `Exec=vkmusic %u`), не приложение.
* Discord: `DISCORD_APP_ID` — общее приложение «Whisper» семейства (как у Whisper Yandex, ID публичный); своё —
  поменять константу в `whisper_host.rs`. Виджет OBS: обложки — только `https://*.userapi.com`, `*.vkuserphoto.ru`,
  `*.vk.me`, `*.sndcdn.com` (`OBS_IMG_SRC`, `cover_ok`); у файлов из папки музыки обложки в виджете нет (она `vkm://`).
* Хранилище модулей в `localStorage`: `vkm_history`, `vkm_discord`, `vkm_obs` (приставка `vkm_`), архив — `wa_cfg`,
  `wa_index_mine`, «Ставить в очередь» комнаты — `ym_room_autoq` (имя ключа задаёт ядро).
* Код ядра здесь не правится: репозиторий whisper-core общий с Whisper Yandex и не наш (права — только чтение).
  Нужно что-то в ядре — предложить владельцу whisper-core; своё — в `whisper-vk.js` / `app.js`.
  Новая версия ядра: `git submodule update --remote src/core`, проверить (`cargo build`, Playwright), закоммитить `src/core`.
  `rust/check` ядра внутри `src/core` не запускать: Tauri встроил бы его `target/` в бинарник.
* Модули ядра — по-русски, без нашего `L()`: перевод — только в самом ядре (в `en.js` — лишь постоянная разметка
  панели очереди, её переводит `applyStatic`). Тексты ядра местами про Whisper Yandex (например, путь к входу в архив
  «Настройки → Для разработчиков» — у нас «Настройки → Интеграции»): поправить можно только в whisper-core.

## Правила

**Безопасность**
* Пути от интерфейса не доверенные: играть, удалять и отдавать обложки можно только для файлов внутри папки
  музыки (`library::inside`, сравнение после `canonicalize`).
* Данные (названия треков, имена тем, ошибки) вставляются в DOM только через `textContent` / `el()`.
  Никакого `innerHTML` с данными; `innerHTML` используется только в `icons.js` для своих SVG (и в модулях ядра —
  для их постоянной разметки и значков; данные там тоже идут через `textContent`).
* CSS тем — только через движок тем ядра, как у Whisper Yandex (`applyTheme`: `WMT.open` файла из `theme_file` →
  `WhisperThemeCss.themeCss(th, id, "whisper-music", { core, mutedIn, extra: whisper-vk })`): правила привязываются к
  стилю `:root[data-style="<id>"]`, роли `[data-w]` → наши селекторы, из `url()` остаются только ресурсы самой темы
  (`blob:`), `data:` и `#якоря` (проверка после раскрытия `\`-экранирования), `@import`, `image-set()` и подобные
  выбрасываются, `@keyframes` / `@font-face` / `@layer` темы получают префикс `wmt-<id>-`. Новые места, куда попадает
  CSS темы, — тоже через `themeCss` / `scopeCss`. Цвета и эффекты из `tokens` — переменные основы (`applyStyleVars`:
  только `#rrggbb` и числа в диапазонах), шрифт пользователя — прямо на `body` (главнее шрифта темы). Rust CSS тем не
  трогает; установка темы без `whisper-music` / `whisper-core` / `whisper-vk` и без `tokens.color` отклоняется.
* Библиотека тем качает только `index.json`, `themes/[<категория>/]<id>.wmt`,
  `previews/[<категория>/]<id>.(jpg|jpeg|png|webp)` с фиксированного `BASE` и с лимитами размера
  (`themelib::limit_for`; категория — один уровень папок, имя как у id темы); путь темы и превью в записи каталога
  должен совпадать с её `category` и `id`; `manifest.id` скачанного файла обязан совпадать с записью каталога.
  `index.json` — формат 2 (папки-категории, `categories`, `artist`; с 2026-10-29), формат 1 тоже читается.
  Не расширяйте список путей и не делайте адрес настраиваемым без причины. Каталог (`index.json`) держится в памяти
  10 минут (`themelib::INDEX_TTL`); «Обновить список» — `themelib_list { fresh: true }`.
* Имя шрифта перед записью в конфиг и CSS проверяется `fonts::valid`.

**Сервисы (ВКонтакте — 2026-10-29, SoundCloud — 2026-10-30, решения владельца).** Оба — неофициальные
клиенты сайтов (нарушают их правила), поэтому держимся рамок:
* вход — обычная страница сервиса (`vk.ru/login`, `soundcloud.com/signin`) вторым webview `svc-login` прямо
  в главном окне (`Window::add_child`, feature `unstable`; в GTK интерфейс сжимается до полосы `LOGIN_BAR` —
  `login_layout`), без прав на команды; попапы — отдельные окна `svc-popup-N`. Один `svc_login(service)` на все.
* SoundCloud: сессия — кука `oauth_token` (`sc::token_from`), заголовок `Authorization: OAuth`; client_id — из
  скриптов `a-v2.sndcdn.com` (`sc::find_client_id`), обновляется один раз при 401/403. Адреса транскодингов —
  только `api-v2.soundcloud.com` (`pick_transcoding`); зашифрованные HLS (DRM) не играем. Поиск работает без входа.
* ВКонтакте: вход определяется, как в NK-Player, — кука `remixsid`/`remixnsid` у vk.ru или vk.com
  (`vk::session_base`), запросы идут на тот же сайт.
* Сессии — куки WebKit, не config.json; логин/пароль приложение не принимает; выход (`svc_logout`) удаляет
  куки сервиса;
* ссылки: сначала открытые (`audio_streams`, `[2]`), иначе — расшифровка `audio_api_unavailable` как в
  NK-Player (`vk::decode_url`, по просьбе владельца 2026-10-30). Это порт JS побайтно; меняя его, сверяйтесь
  с векторами теста `decode_matches_nk_player` (сгенерированы оригинальным `vk-audio.js`, коммит a4509c8);
* запросы — по действию пользователя, не чаще раза в секунду (`vk::GAP`, `sc::GAP`), без повторов, предзагрузки,
  «прогрева» CDN и обхода ограничений; ограничение сервиса — показать ошибку и ждать. Исключение (просьба
  владельца, 2026-10-30): синхронизация «Мои треки» ВК / лайки SoundCloud → библиотека (`saved_sync`) при
  запуске, после входа и по кнопке обновления — постранично через ту же очередь, до 10 000 треков;
* кэш треков и загрузка (просьба владельца, 2026-10-30): трек **библиотеки** (файл или сохранённый трек
  сервиса), который включили, кладётся в `cache::TrackCache` и дальше играет из кэша без запросов к сервису.
  Поток выкачивается ffmpeg тем же адресом, UA и Referer, что у mpv, без перекодирования (`-c copy`), по одному
  треку; ожидающая задача заменяется следующей (`enqueue`) — пролистанные треки не выкачиваются.
  Предзагрузка (просьба владельца, 2026-10-30): когда включённый трек уже в кэше (`*_play` вернули `false`)
  или докачался (`cache://event` с его ключом), интерфейс зовёт `cache_prefetch` для **одного** следующего
  трека очереди (`upcoming()`; при перемешивании он выбирается заранее — `S.shuffleNext`). Ссылка на поток
  запрашивается в этот момент, через ту же очередь `GAP`; задача ждёт после включённого трека, а новое
  включение (`TrackCache::begin`) её отменяет. Дальше одного трека вперёд не кэшировать.
  `fresh: true` выбрасывает трек из кэша. Убран из библиотеки (`saved_remove`, `vk_delete`,
  синхронизация, выход, удаление/изменение файла) — убирается и из кэша. `track_download` копирует трек
  библиотеки в `Config::download_dir` (по умолчанию `~/Downloads`), трек сервиса — через кэш; ключ — как в
  порядке библиотеки, данные трека берутся из `library.json`, не из интерфейса. Треки вне библиотеки
  (результаты поиска) не кэшируются и не скачиваются;
* кэш ссылок: ссылки на потоки уже прослушанных треков — в памяти (`streams::StreamCache`, ключ `vk:<reload>` /
  `sc:<id>`), пока действуют; не открылась — интерфейс один раз повторяет `vk_play`/`sc_play` с `fresh: true`.
  Результаты общего поиска кэшируются в интерфейсе (`searchCache`) до очистки строки; поиск — с задержкой `SEARCH_DELAY`,
  чтобы не слать запрос на каждую букву;
* порядок библиотеки — `order::OrderStore` (просьба владельца, 2026-10-30): впервые появившийся трек встаёт
  первым (`seen`: `local_list` — новые файлы, `saved_add`, `saved_sync` — пачкой в порядке сервиса), пропавшие
  убираются (`retain`, `prune_order`). Без файла порядок один раз собирается из прежних дат (`init` в `local_list`);
  до этого `seen` ничего не делает — иначе синхронизация, закончившаяся раньше сканирования, поставила бы все
  файлы выше. Интерфейс сортирует по `library_order`, даты — только для треков вне порядка;
* удаление из ВК (просьба владельца, 2026-10-30): `vk_delete` — `act=delete_audio` с хэшем удаления
  (`[13]` → `h[3]`, `Saved.del`), только для синхронизированных «Моих треков»; id и хэш берутся из
  `library.json`, не из интерфейса; результат — `vk::al_ok`. Подтверждение — окошко. Другие изменения
  аккаунтов (добавление в ВК, плейлисты, лайки SC) — только по явной просьбе владельца;
* синхронизированные треки — `Saved.sync = true`: список сервиса целиком заменяется (`replace_synced`), при
  выходе убирается (`clear_synced`); добавленные вручную («+») не трогаются, кнопки «убрать» у синхронизированных нет;
* User-Agent — как у Electron в NK-Player (`vk::USER_AGENT`, Chromium): с UA WebKitGTK страница входа
  не догружается (проверено 2026-10-29). Тот же UA — у запросов и у mpv; Referer — сайт сессии. Больше
  ничего не подделывать.
* Запросы к ВК — только через клиент с хранилищем кук (`Vk::client`, `jar_for`), не голым заголовком `Cookie`:
  после входа ВК доводит сессию редиректами `login.php?act=slogin`, выставляя куки по ходу; без хранилища —
  «error following redirect». Перед закрытием страница входа сама открывает `/audio` (`vk_login_settle`).
* Пока открыт вход, в окне `main` два webview: `get_webview_window("main")` возвращает None — для кук
  берите `get_webview("main")`, для раскладки и `add_child` — `get_window("main")`.
* Куки WebKit читаются и удаляются только через `with_cookie_manager` / `all_cookies` / `delete_cookies` (`main.rs`),
  не `Webview::cookies` / `delete_cookie` из wry: wry отдаёт домен без ведущей точки (крейт `cookie`), кука `.vk.ru`
  становилась кукой одного хоста и не уходила на `login.vk.ru`, где ВК после перезапуска обновляет сессию
  (`role=fast` → `login.php?act=slogin`) — «Сессия ВКонтакте не распознана» до «Выйти» (2026-11-02); и удалить
  такую куку через wry нельзя (выход её не удалял).
* Тексты песен — Genius (просьба владельца, 2026-10-30, «как в VK MP3 Mod»): без ключа API, `genius.com/api/search/song`
  (до трёх поисков на трек) и страница песни (`lyrics::extract`, блоки `data-lyrics-container`). При включении трека
  интерфейс сначала берёт текст из кэша без сети (`lyrics_cached`, `lyrics_translation_cached`) и показывает сразу —
  вместе с последним выбранным языком (`lyricsPrefLang`, localStorage; перевода нет в кэше — оригинал, перевод
  подгружается и подставляется). В сеть — только если в кэше нет: по включению трека, после задержки `LYRICS_DELAY`
  и только когда текст на экране (`#full` в виде «Текст»: нажатие на обложку или карточку «Сейчас играет»); не чаще раза в секунду
  (`lyrics::GAP`), результат — в кэш `~/.cache/vkmusic/lyrics` («не нашли» — на 3 дня). Подбор (`clean_title`,
  `clean_artist`, `score`) проверяйте на библиотеке владельца: `VKM_LYRICS_SAMPLE=40 cargo test live_library -- --ignored
  --nocapture` (2026-10-30: 31/40 и 30/40, промахи — инструменталы и OST). Ссылку открывает только `lyrics_open` (genius.com).
  Переводы — `translation_songs` из `/api/songs/<id>` (отдельным шагом `lyrics_meta` после показа текста — текст на
  запрос раньше), текст перевода — по выбору
  (`lyrics_translation`, адрес — только `genius_page`); пояснения — по нажатию на фрагмент (`lyrics_note`,
  `/api/referents/<id>`, кэш на диске `lyrics/notes/<id>.txt`), фрагменты в тексте помечены U+E000 id U+E001 … U+E002. Одна песня целиком:
  `VKM_ARTIST=… VKM_TITLE=… cargo test live_one -- --ignored --nocapture`.
  Текст ВК (просьба владельца, 2026-11-02): у трека ВК сначала текст самого ВК (`Vk::lyrics`, `audio.getLyrics`,
  `audio_id`, через очередь `GAP`; нет текста — ошибка 104 → Genius), кэшируется там же (`store_for`, `source: "vk"`).
  Подбор Genius «как в VK MP3 Mod» (один запрос, совпадение слов) пробовали 2026-11-02 и убрали по решению владельца:
  на 40 треках — 28 (3 не та версия) против 31 (1) у нашего. Мод ходит в мобильный `api.genius.com` с токеном
  официального приложения Genius — такой токен не используем (выдавать себя за чужое приложение нельзя). Текст ВК одного трека:
  `VKM_COOKIES=… VKM_VK_ID=… cargo test live_vk_lyrics -- --ignored --nocapture`. Скрытие строк частей песни и
  пустых строк (`lyrics_hide_parts`, `lyrics_compact`) — только в интерфейсе (`tidyLyrics`), кэш не меняется.
* VK Микс и плейлисты (просьба владельца, 2026-11-01; раздел «Миксы»): через API ВК с **web-токеном
  самого сайта** — `login.vk.ru/?act=web_token` (`version=1`, `app_id=6287487`, куки сессии, как браузер), методы
  `catalog.getAudio` (главная: миксы и подборки «Собрано алгоритмами / редакцией», «Слушайте друг друга»),
  `audio.getPlaylists` (свои — разделом «Плейлисты» в библиотеке над треками, `vk_my_playlists`: при открытии
  библиотеки раз за запуск и по кнопке обновления; без входа и без сохранённых — не показываются), `audio.get` (треки плейлиста, по 200), `audio.getStreamMixAudios` (`mix_id=common`,
  каждый вызов — новые 2–3 трека), `audio.getById` (свежая ссылка). Версия API — `5.289`, как у сайта (`vk::API_V`).
  Токен — только в памяти (`Vk::token`), до срока; при ошибке 5 — один раз новый. Запросы — в той же очереди `GAP`
  (`Vk::post_json`), только по действию: открыть раздел/плейлист, «Показать ещё», включить микс; микс догружается,
  когда до конца очереди 2 трека (`extendMix`). Свои плейлисты хранятся локально (просьба владельца, 2026-11-02):
  `playlists::PlaylistStore`, `~/.config/vkmusic/playlists.json` — список (`vk_my_playlists` записывает,
  `vk_my_playlists_cached` отдаёт без сети) и треки открывавшихся плейлистов из списка (`vk_playlist` записывает,
  с offset 0 — заменяет; `vk_playlist_cached`), плюс их access_key (при запуске — в `Vk::keys`, интерфейсу не
  отдаются). Интерфейс показывает сохранённое сразу, затем один запрос обновляет его (список — раз за запуск и по
  кнопке обновления; открытый плейлист — первая страница при каждом открытии, `refreshVkPlaylist`). Изменения
  через `vk_playlist_*` правят и файл; выход из ВК его удаляет. Чужие плейлисты (подборки) не сохраняются. Ссылки из ответов — в кэше ссылок (`vkapi:<id>`), интерфейсу не
  отдаются; access_key — в `Vk::keys`. Треки из API (`Track.api`) играют `vk_play_api`. В библиотеку добавляются
  «сердцем» (просьба владельца, 2026-11-02; в строке — `.likebtn`, у играющего трека — `#likeBtn`): `Saved.api = true` без
  reload, `access_key` кладёт `saved_add` из `Vk::keys` (не из интерфейса); такой трек играет `vk_play_api`, ссылка —
  `audio.getById` (`vk_api_stream`), кэшируется и скачивается, как остальные треки библиотеки. Вне библиотеки треки
  из API не кэшируются и не скачиваются. Проверка на настоящем ВК:
  `VKM_COOKIES=~/.local/share/vkmusic-whisper/cookies cargo test live_api -- --ignored --nocapture`.
  Управление своими плейлистами (просьба владельца, 2026-11-02): `audio.createPlaylist`, `editPlaylist` (название),
  `deletePlaylist`, `addToPlaylist`, `removeFromPlaylist`, `followPlaylist` — команды `vk_playlist_*`, только по
  нажатию (удаление трека/плейлиста — с подтверждением `confirmClick`). Менять можно только плейлист с owner_id = id
  из web-токена (`Vk::own`); `ApiPlaylist.mine` / `editable` (у добавленного чужого есть `original`). Имена методов
  проверены без изменений в аккаунте: `cargo test live_playlist_methods -- --ignored --nocapture` (на чужом плейлисте
  owner_id=1; «Unknown method» — имя неверное). «сердце» горит (трек уже в библиотеке), если он есть в ней по ключу или по
  исполнителю + названию + длительности ±3 с (`libraryMatch` в app.js): «Мои треки» — копии с вашим owner_id.
  Другие методы API (лайки, «Мои треки» через API, порядок треков в плейлисте) — только по явной просьбе владельца.
* Скачивание треков ВК вне библиотеки, несколько аккаунтов, другие новые сервисы — только по явной просьбе владельца.

**Качество загрузок** — не перекодировать: `-f bestaudio/best --extract-audio --audio-format best`.
Перекодирование в mp3 намеренно убрано (lossy → lossy). Обложка встраивается только если есть python-mutagen
(`ytdlp::caps`), иначе yt-dlp падает на opus/m4a. Если меняете аргументы — прогоните их на настоящем yt-dlp
(например, на локальном файле через `python3 -m http.server`) и обновите тесты в `ytdlp.rs`.

**Интерфейс и тексты**
* Язык исходников, сообщений об ошибках, комментариев и документации — русский. Интерфейс — русский и английский
  (просьба владельца, 2026-10-30; «Настройки → Общее», `Config::lang`): строки в `src/app.js` — через `L("Русский текст {0}", x)`
  (ключ — русский текст), множественное число — `plural(n, "трек", "трека", "треков")`, словари — `src/i18n/<язык>.js`
  (`en.js`: ключ → перевод, для `plural` — `[одна, много]`). Новую строку интерфейса — сразу в `en.js`; проверка, что
  переведены все ключи, — скриптом из истории или вручную. Статичная разметка `index.html` переводится сама (`applyStatic`).
  Не оборачивайте в `L()` строки-данные (сравнения, ключи) и константы уровня модуля — их переводят при показе
  (`L(PAGES[page])`). Ошибки Rust остаются русскими; для показа — `errMsg(e)` (перевод по точному совпадению), для
  проверок текста ошибки — `errText(e)`. Переменная `t` в `app.js` — трек, поэтому функция перевода называется `L`.
* Подтверждение опасных действий — маленькое окошко у кнопки (`confirmClick(btn, fn, вопрос, кнопка)`,
  `.confirm-pop`), не `confirm()` и не двойное нажатие (по просьбе владельца, 2026-10-30).
  Исключение по просьбе владельца: установка/удаление в библиотеке тем — одним нажатием.
* Разметка — контракт с темами Whisper: классы и id те же, что у Whisper Yandex (по ним написана библиотека тем).
  Не переименовывать и не перестраивать `.app`, `.side`, `.main`, `.head` (`#title`, `#sub`, `#headImg`, `#back`),
  `ul.list > li` (`img`, `div > .t/.a`, `.likebtn`, `.morebtn`), `.player` (`.pinfo` `#cover` `.meta` `#pTitle` `#pArtist`
  `#likeBtn`, `.pcenter` `.seekrow` `#seek` `.ctrls` `#play` `#prev` `#next` `.modebtn`, `.pright` `#volWrap` `#volBtn`
  `.volpop` `#vol` `#pMore`), `#settingsPage.settings` (`.settabs .chip`, `.card[data-tab]`), `.simcard`, `.lastAlbum`,
  `#wavePage` (`.waveorbits`, `#waveBtn.wavebtn`), `#full`, `.titlebar`, `.tmenu`, `.toast`, `body.is-playing`.
  Свои элементы — дополнительными классами и внутри этих (`.lq`, `.lacts`, `.ldur` в строке). `whisper.css` не правится:
  своё — в `app.css`. Возможности тем сверх CSS (`settings whisper-music`): `features.animations`, `controls.volume:
  "knob"` (`data-vol-knob`, `--vol`), `buttons` (`#thBtns .thbtn`, действия — только `TH_ACTIONS`).

**Темы**
* Встроенная тема: правьте `themes/<id>/theme.wmt`, затем пересоберите архив
  `scripts/pack-theme.sh themes/<id>` (или тем же составом файлов через zip) и проверьте
  `cargo test` (`builtin_parses`). Новая встроенная тема — ещё и строка в `BUILTIN` в `themes.rs`.
  Картинки в собранных архивах `osaka-jade`, `vantablack`, `ashen-inventory` не совпадают с файлами в папках
  (2026-11-03): чтобы поменять только CSS, обновляйте в архиве один файл — `cd themes/<id> && zip -X ../<id>.wmt theme.wmt`.
* id встроенных тем (`osaka-jade`, `vantablack`, `ashen-inventory`, `slate`) и основ Whisper (`neon`, `royal`, `steam`,
  `lolitta` — стили без файла, CSS в `whisper.css`) заняты — свои темы с ними не ставятся.
* Секции, как у Whisper Yandex: `@@ css whisper-core` (по ролям) → `@@ css whisper-music` (разметка Whisper — она же
  наша) → `@@ css whisper-vk` (своё для VK Music, необязательно) → `tokens`; `@@ settings whisper-music` — возможности
  темы. Встроенные темы пишутся в `whisper-music`. Старые секции `vkmusic` (прежняя разметка) не читаются.
* Ассеты чужих игр, программ и брендов (арт, логотипы, шрифты без свободной лицензии) в темы не класть.
  Ashen Inventory «в духе» Dark Souls нарисована целиком на CSS, фон — процедурный.
  Обои Osaka Jade и Vantablack — из тем Omarchy (MIT). Фон «Грифеля» (`slate`) — процедурный (numpy + Pillow),
  по мотивам доски из темы «Мел» Whisper Music, но без её картинки и надписей.

## Чего не делать

* Не расширять ВКонтакте за рамки правила выше: никаких токенов Kate Mobile и других приложений, чтения кук из
  браузеров, ввода пароля в приложении. Единственный токен API — web-токен самого сайта для микса и плейлистов
  (решение владельца 2026-11-01), только в памяти.
* Не добавлять Omarchy-специфичных функций (у владельца не Omarchy): темы «как в Omarchy» — да,
  чтение `~/.local/state/omarchy` — нет.
* Не переписывать файлы, которые владелец мог править в редакторе, «вслепую»: если файл мог измениться,
  сначала сравните его с последней известной версией.
* Не трогать `~/Music/vk`, `~/.config/vkmusic` и `~/.cache/vkmusic` пользователя за пределами того, что делает само приложение.

## Известные ловушки

* `MutexGuard` во временном значении `if let Some(x) = st.lock().unwrap().take()` живёт до конца блока —
  берите значение в отдельную переменную (`let player = …take();`).
* `tauri::webview::Cookie`, а не `tauri::Cookie`; `WebviewWindowBuilder::new(app, …)` принимает `&AppHandle`
  — не передавайте `&&AppHandle`.
* В байтовых строках `b"…"` / `br#"…"#` нельзя кириллицу — используйте `"…".as_bytes()`.
* WebKitGTK + NVIDIA/Wayland: `WEBKIT_DISABLE_DMABUF_RENDERER=1` приложение ставит только со старым драйвером
  NVIDIA (< 560, `old_nvidia`). Этот обход копирует каждый кадр через процессор: на 2560×1440 интерфейс падал
  до ~10 кадров/с (с DMA-BUF — 60, NVIDIA 615, Hyprland, замер 2026-10-30). Не включайте его безусловно.
  Замерять удобно `MiniBrowser` из webkit2gtk с копией `src/` и заглушкой `__TAURI__`: на DMA-BUF стеклянные
  эффекты (`backdrop-filter`, размытие фона) на FPS не влияли.
* Выше 60 кадров/с WebKitGTK 2.54 интерфейс не рисует (замер 2026-10-30, монитор 165 Гц, NVIDIA 615). Две причины:
  драйвер NVIDIA отвечает `EOPNOTSUPP` на `DRM_IOCTL_WAIT_VBLANK`, и WebKit берёт таймер на 60 Гц
  (`FullSpeedFramesPerSecond`); но и с подменой ioctl на 165 Гц `requestAnimationFrame` упирается в 60
  (за vblank ниже 60 идёт, выше — нет), фича `PreferPageRenderingUpdatesNear60FPS` не влияет, webkitgtk-6.0 даёт ~48.
  Это ограничение движка, а не приложения: без пересборки WebKitGTK не обойти.
* У части тем Whisper Music нет картинки в файле (Терминал, Windows 98): превью для карточки берётся из
  библиотеки и хранится как `~/.config/vkmusic/themes/<id>.preview.<ext>`.
* mpv читает `mpv.conf` пользователя: если там включено сохранение позиции, трек продолжал бы играть с места,
  где его переключили. Поэтому mpv запускается с `--resume-playback=no --save-position-on-quit=no`.
  `af` плеер задаёт сам («постоянный уровень громкости»), перекрывая `af` из конфига пользователя.
