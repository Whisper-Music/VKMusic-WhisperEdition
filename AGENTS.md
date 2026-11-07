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
* Перед тем как считать работу готовой: `cargo build --release` без предупреждений, `cargo test`,
  `node --check` всех `src/*.js` и `node scripts/check-i18n.mjs` (все строки `L()` / `Lc()` / `plural()` и статичная
  разметка — в `en.js`, без повторов ключей; строки-константы вроде помощи cli.js и правил errinfo.js скрипт не видит —
  их переводы добавляются руками). То же проверяет CI — `.github/workflows/check.yml` (push / PR / раз в неделю:
  тесты на Linux с `-D warnings`, `cargo check` на Windows, `node --check`, переводы, `cargo audit`).
* Выпуски — `.github/workflows/release.yml` (как у Whisper Yandex): тег `vX.Y.Z` или запуск вручную → черновик в
  GitHub Releases: Linux x64 (`.deb` с зависимостями mpv/ffmpeg, `.AppImage`; ubuntu-22.04 ради старой glibc) и
  Windows x64 (NSIS `-setup.exe`, просьба владельца 2026-11-05). Автообновление — только Windows (просьба владельца,
  2026-11-05; `update.rs`, tauri-plugin-updater только в `cfg(windows)`): `latest.json` последнего выпуска, установщик
  подписан minisign — открытый ключ в `tauri.windows.conf.json` (`plugins.updater`), закрытый — секреты репозитория
  `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` (у владельца — `~/.tauri/vkmusic-whisper.key`; потерять ключ = выпущенные
  копии больше не обновятся). Интерфейс проверяет раз при запуске (`checkUpdate`), ставит — по кнопке в подсказке.
  Linux — пакеты/AUR или вручную. Версия — одинаковая в `src-tauri/tauri.conf.json` и `src-tauri/Cargo.toml` (проверяет `guard`).
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
src-tauri/src/main.rs      AppState, общие команды (app_info, save_prefs, окно), протокол vkm://, setup (mpv, трей, окно), выход
src-tauri/src/cmd/         команды интерфейса по разделам (до 2026-11-06 — всё в main.rs): player_cmd (плеер, устройство,
                           трей), vk_cmd (ВК, VK Микс, плейлисты), login (вход, куки WebKit/WebView2), sc_cmd, saved_cmd
                           (библиотека сервисов), local_cmd (файлы, yt-dlp), lyrics_cmd, storage_cmd (статистика, кэш,
                           резервные копии), themes_cmd (темы, библиотека тем). Всё общее — через `use crate::*`
src-tauri/src/winstate.rs  окно помнит размер, положение и развёрнутость (config.json → window); окно создаётся скрытым
src-tauri/src/tray.rs      значок в трее (меню; Linux — только если есть libayatana-appindicator3 / libappindicator3,
                           `tray::available`), «Закрывать в трей», мини-плеер — окно `mini` (mini.html); вид значка
                           (`Config::tray_icon`: цветной / светлый / тёмный / цвет темы — полоски логотипа без подложки, `tray::bars`)
src-tauri/src/backup.rs    резервные копии .vkmbak: настройки, library/order/playlists/stats, свои темы, localStorage;
                           автокопии раз в день (~/.config/vkmusic/backups, 5 шт.), восстановление — при перезапуске
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
src-tauri/src/loudness.rs  «Постоянный уровень громкости»: замер EBU R128 (ffmpeg ebur128) файлов на диске в фоне,
                           ~/.cache/vkmusic/loudness.json; фильтр mpv — усиление до −14 LUFS, незамеренный — loudnorm
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
src-tauri/src/hotkeys.rs   глобальные горячие клавиши (tauri-plugin-global-shortcut; Windows, Linux X11): hotkeys_set, событие hotkey
src-tauri/src/update.rs    автообновление (только Windows): update_check, update_install, `with_updater`
src-tauri/src/relay.rs     ретранслятор потоков треков ВК из API: 127.0.0.1/r/<токен> → CDN тем же клиентом, что и API
src-tauri/src/os.rs        Linux/Windows: права на файлы (`Private`), внешние программы (`tool`, `bin_dir`), `open`
src-tauri/src/host.rs      `--host [ПОРТ]`: интерфейс в браузере — HTTP-сервер на 127.0.0.1 (invoke, SSE-события, vkm/wa)
src/core/rust/…            Rust ядра через #[path]: whisper/ (архив, вход, загрузка, wa://, комнаты), discord.rs, obs.rs
src/                       index.html, app.css, app.js, icons.js — без сборщика и фреймворков
src/whisper.css            основа оформления Whisper (styles.css Whisper Yandex, PolyForm Noncommercial) — не править
src/boot.js                до первой отрисовки: стиль прошлого запуска (vkm_boot), интерфейс скрыт до применения темы
src/layout.js, layout.css  расположение (как у Whisper Yandex, без закреплённых): мини-режимы панелей, размер за край (vkm_layout)
src/themeguard.js          защита от ломающих тем (как у Whisper Yandex): закрыть/свернуть/перетащить окно, пауза, настройки
                           должны оставаться доступны — иначе аварийная полоса; Ctrl+Shift+Backspace — «Неон» (vkm_tguard)
src/colors.js              цвета темы: основной/второй и «Тонкая настройка…» по местам, для каждого стиля (vkm_colors), <style id="cc-css">
src/hotkeys.js             горячие клавиши: карточка в «Общем», окно назначения, сочетания — localStorage vkm_hotkeys
src/keynav.js              клавиатура внутри окна (как у Whisper Yandex): стрелки по спискам и меню, Enter, Menu, F1 — список
src/errinfo.js             понятные ошибки воспроизведения: где проблема (сервис, сеть, устройство, файл) и что делать
src/lsguard.js             до app.js: повреждённые JSON в localStorage → window.__lsBroken (backup.js предложит автокопию)
src/backup.js              карточка «Резервная копия» (Хранилище), автокопия при запуске
src/tray.js                что играет — трею и мини-плееру (mini_state / tray_state), карточка «Трей» в «Общем»
src/cli.js                 стиль-консоль («console» в settings темы, тема «Терминал»): команды VK Music, разметка #cli Whisper
src/mini.html, mini.js, mini.css  мини-плеер (окно `mini`): данные — только textContent, обложки — https и vkm /cover
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
`cache://event` (`cached` / `failed`, поле `key` — ключ трека в библиотеке) — после него кэшируется следующий трек,
`tray://ctl` (действие из меню значка или мини-плеера → `playerAction`, как событие `hotkey`; в `--host` трея нет),
`mini://state` (окну `mini`). Плюс `tauri://drag-*` для перетаскивания `.wmt`.
* Новая команда = функция в `src-tauri/src/cmd/<раздел>.rs` (`pub(crate)`, общее — `use crate::*`) + строка
  `cmd::<раздел>::<команда>` в `generate_handler![…]` (main.rs) + вызов в `src/app.js`. Помощник, который нужен
  другому разделу, — `pub(crate)`, а его раздел — в `use cmd::{…}` в main.rs.
  Проверяйте, что каждая команда из JS есть в обработчике (и наоборот, лишних нет). `vk_status` / `sc_status`
  вызываются как `invoke(service + "_status")` — простым grep их не найти.
* Файлы тем и обложки интерфейс получает по `vkm://localhost/…`: `/theme/<id>/<файл>`,
  `/themepreview/<id>`, `/cover?path=…`. Протокол отвечает только главному окну (`label == "main"`).
* CSP (с 2026-11-06, как у Whisper Yandex; до этого был выключен): `tauri.conf.json → app.security.csp` — скрипты только
  свои (`script-src 'self'`: ни инлайн-скриптов, ни `eval`, ни обработчиков в атрибутах), картинки — свои, `data:`,
  `blob:`, `vkm:` / `wa:` и CDN обложек (ВК, SoundCloud, Яндекс из комнат, превью whisper-themes, аватары Discord),
  `connect-src` — только IPC, `vkm:` / `wa:`, `blob:`. Стили — с `'unsafe-inline'` (тема — `<style>`, модули — style-атрибуты),
  поэтому `dangerousDisableAssetCspModification: ["style-src"]` (иначе Tauri добавил бы хэши и `'unsafe-inline'` перестал
  бы действовать). `--host` отдаёт ту же политику заголовком (`host::CSP`, тест `csp_matches_window`). Новый внешний хост
  картинок — в обе. CSP — второй рубеж: правила ниже по-прежнему обязательны.

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

* Порядок в `index.html` важен: в `<head>` без defer — `boot.js` (как у Whisper Yandex: стиль прошлого запуска из
  `vkm_boot` и класс `booting` — интерфейс скрыт под цветом фона, пока `boot()` не применит тему и не позовёт
  `bootDone`; снимок пишет `saveBoot` после `applyTheme`), `lsguard.js` (до всего, что пишет localStorage), затем
  `core/whisper-core.js` → i18n, `icons.js`, `errinfo.js`, `app.js` → `backup.js`, `tray.js`, `keynav.js`, `cli.js`,
  `hotkeys.js`, `colors.js`, `themeguard.js` → `whisper-vk.js` →
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
  `provider.store` (что история пишет в `vkm_history`) — трек `app.js` с `id` **ядра** (как у Whisper Yandex): по нему
  `core/queue.js` узнаёт повтор, иначе каждое прослушивание — новой строкой (было до 2026-11-06); свой id — в `rid`,
  `fromCore` (`unstore`) возвращает его. Старые записи переводятся при запуске, до загрузки `queue.js`.
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
  в пути имя пользователя; `coverUri` — всегда `null`: сервер комнат общий с Whisper Yandex и трек с обложкой не
  Яндекса отклоняет как «некорректный»), обратно — `fromRoom`: трек библиотеки по ключу или по исполнителю + названию +
  длительности (`libraryMatch`), SoundCloud — по id, иначе заготовка `room:…`: её найдёт архив ведущего или
  **один** поиск ВКонтакте при включении (`resolveRoomTrack`; это действие пользователя — он в комнате). У трека из
  комнаты `roomId` — id комнаты, по нему ядро узнаёт «тот же трек».
* «Мягкая связь» ядра (с 2026-11-06): гость сам выясняет, что у него значит трек комнаты — `provider.tracks([id])`
  (у нас без сети: результат `fromRoom`, запомненный в `roomSeen`, если это трек библиотеки или SoundCloud), затем
  `provider.search` (поиск ВКонтакте, `vkSearchOnce`: ответ и ошибка помнятся минуту — `resolveRoomTrack` того же
  трека второй раз не ищет; без входа во ВК — пусто), затем свой архив, иначе «нет на площадке» (`/missing`).
  Так в комнату можно с другим сервисом (Whisper Yandex ↔ VK Music). Ведущий так же заранее сопоставляет
  предложения гостей — один поиск на предложение, если трека нет в библиотеке. Ведущий делится треком, которого
  у гостя нет: `provider.roomCache` → `wa_room_cache_track` (файл — во временный кэш комнаты на сервере архива,
  только трек библиотеки, как «Сохранить в архив») или с галочкой «Сохранять их в мой архив» — `archiveSave`.
  Друзья и приглашения — команды ядра `wa_room_friends / friend_forget / invite / invites / invite_dismiss`
  (приглашения ждутся long-poll, пока есть друзья), мигание на панели задач — `requestUserAttention`
  (`core:window:allow-request-user-attention` в capabilities).
* Кнопки плеера у гостя: ядро глушит «вперёд/назад» и `.modebtn` (роль `player.mode`) у любого гостя — даже
  отошедшего. `whisper-vk.js` (`GUEST_OWN`) ловит эти нажатия раньше, на window в фазе захвата, и выполняет сам:
  «Текст песни» и «Постоянный уровень громкости» (у нас тоже `.modebtn`) — всегда, «вперёд/назад», перемешивание
  и повтор — когда гость отошёл (`follow = false`). Функции кнопок — `toggleShuffle`, `cycleRepeat`, `toggleLyrics`,
  `toggleNormalize` в app.js.
* Гость, который следует за ведущим (`roomGuest()` в app.js), свою очередь не двигает: доигравший или не открывшийся
  трек ждёт следующего от ведущего (иначе ядро считало включение своего трека уходом — «вылеты» из комнаты),
  без предзагрузки следующего и догрузки микса. Ошибка трека — `media().error` (`S.playError`): по ней ядро говорит
  ведущему «нет у меня». При смене трека прошлый сразу останавливается (`player_stop` в `playAt`) — раньше гость
  слышал старый трек, пока грузился новый.
  «Играть» от ядра, когда в плеере нет файла (`player.bind.play`): трек включается заново с места, куда ядро перед
  этим перемотало (`seek` без файла запоминает его в `S.idleSeek`) — так работает вход в комнату с тем же треком,
  что стоит у гостя. Гостю не включается трек, который у него доиграл (`S.roomEnded`) или не открылся
  (`S.playError`); вход в комнату и «Вернуться» (событие ядра `room`) снимают `S.playError` и забывают неудачные
  поиски `vkSearchOnce`.
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
* «Темы комнаты» (ядро, 2026-11-06): `kit.themeExport` — текущий стиль файлом (`theme_file`; только тема из файла:
  основы Whisper есть у всех, «Цвета» в файл не входят), `kit.themeImport` — `theme_install` + `selectTheme`; своя
  тема с тем же id и другим содержимым заменяется только после подтверждения (`askConfirm`, null — отказались).
  Команды ядра `wa_room_theme_share` (сырое тело + заголовки `x-room-code`, `x-theme-name`, `x-theme-author`),
  `wa_room_theme_get`, `wa_room_theme_remove`. В `--host` заголовки `x-*` передаёт `host.js` (`invoke(cmd, байты,
  { headers })`) и `host::invoke` — только они, не куки и не Host. Метку приложения у друзей («VK») ядро шлёт само
  (`wa_room_invites(since, app)`, id провайдера `vk`).
* Строки очереди и истории ядра — с «сердцем» (`kit.likeButton` → наш `likeButton`). `fromCore` и `fromRoom` ищут трек
  по ключу через `libraryByKey` (индекс `libIndex`), а не перебором: их зовут на каждую строку.
* Своё в `localStorage` (не модули): `vkm_boot` (снимок стиля для boot.js), `vkm_dyn` (фон по обложке),
  `vkm_style_view` (компактный вид стилей), `vkm_search_hist` (история поиска, 30), `vkm_hotkeys`, `vkm_layout`,
  `vkm_colors` (цвета пользователя по стилям: в CSS попадают только `#rrggbb(aa)`), `vkm_cli_hist` (история команд
  консоли), `vkm_cli_prev_style` (к какому стилю вернёт `gui`). В резервную копию входит весь localStorage, кроме
  `vkm_boot` и `wa_index_v3` (`backup::storage_key_ok`).
* Кэш треков: предел — `Config::cache_limit_mb` (0 — без ограничения, «Хранилище»); сверх него `TrackCache::prune`
  удаляет давно не игравшие (по времени изменения файла; включение из кэша — `touch`), кроме только что докачанного.
* Фон по обложке (как у Whisper Yandex; CSS — `.dynbg` в whisper.css, разметка `#dynBg` внутри `#bg`): обложку
  с CDN холст не прочитал бы — её отдаёт `cover_bytes` (только `whisper_host::cover_ok`, до 10 МБ), обложка файла —
  `fetch` по `vkm://` (там `Access-Control-Allow-Origin`).
* Хранилище модулей в `localStorage`: `vkm_history`, `vkm_discord`, `vkm_obs` (приставка `vkm_`), архив — `wa_cfg`,
  `wa_index_v3`, «Ставить в очередь» комнаты — `ym_room_autoq`, «Делиться треками, которых нет у гостей» —
  `ym_room_share` и `ym_room_share_save` (имена ключей задаёт ядро).
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
  скриптов `a-v2.sndcdn.com` (`sc::find_client_id`), обновляется один раз при 401/403; помнится между запусками
  в `~/.cache/vkmusic/sc-client-id` (`Sc::with_cid_file`; он общий, не секрет) — иначе первый поиск ждал ~1,5 с. Адреса транскодингов —
  только `api-v2.soundcloud.com` (`pick_transcoding`); зашифрованные HLS (DRM) не играем. Поиск работает без входа.
* ВКонтакте: вход определяется, как в NK-Player, — кука `remixsid`/`remixnsid` у vk.ru или vk.com
  (`vk::session_base`), запросы идут на тот же сайт.
* Сессии — куки WebKit, не config.json; логин/пароль приложение не принимает; выход (`svc_logout`) удаляет
  куки сервиса;
* ссылки: сначала открытые (`audio_streams`, `[2]`), иначе — расшифровка `audio_api_unavailable` как в
  NK-Player (`vk::decode_url`, по просьбе владельца 2026-10-30). Это порт JS побайтно; меняя его, сверяйтесь
  с векторами теста `decode_matches_nk_player` (сгенерированы оригинальным `vk-audio.js`, коммит a4509c8);
* запросы — по действию пользователя, не чаще раза в секунду (`vk::GAP`, `sc::GAP`: по одному, начало следующего —
  не раньше чем через секунду после начала предыдущего; до 2026-11-06 отсчёт шёл от ответа, и поиск ВК, который
  отвечает ~1,8 с, ждал лишнюю секунду), без повторов, предзагрузки,
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
  чтобы не слать запрос на каждую букву. Запрос из строки поиска — `live: true` (`vk_search`, `sc_search`): пока он ждёт
  очереди, более новый такой же его отменяет (`ERR_SUPERSEDED`, в сервис он не уходит; интерфейс это ошибкой не
  показывает) — итоговый запрос не ждёт промежуточных. Поиск для «Слушать вместе» — без `live`. Сколько идёт поиск:
  `VKM_COOKIES=… cargo test live_search -- --ignored --nocapture` (2026-11-06: ВК ~1,8 с — это сам ВК, ответ ~480 КБ),
  `cargo test live_sc_search -- --ignored --nocapture` (SoundCloud ~0,9 с);
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
  Синхронный текст ВК (2026-11-06): `lyrics.timestamps[{begin, end, line}]` (мс) → `Lyrics.times` (начало каждой строки
  `text`; `vk::parse_lyrics`), интерфейс рисует его разметкой Whisper Yandex (`.flyrics.synced > .ln`, текущая — `.ln.on`,
  `syncLyricsLine`; нажатие на строку — перемотка; руками листают — 4 с не прокручиваем сами). Записи ВК до версии 4
  (`VK_V`) сохранены без времени — `lyrics_cached` их не отдаёт, `lyrics_get` спрашивает ВК заново (не вышло — прежний
  текст, `old_vk`, а не Genius). Перевод Genius показывается простым текстом.
  Подбор Genius «как в VK MP3 Mod» (один запрос, совпадение слов) пробовали 2026-11-02 и убрали по решению владельца:
  на 40 треках — 28 (3 не та версия) против 31 (1) у нашего. Мод ходит в мобильный `api.genius.com` с токеном
  официального приложения Genius — такой токен не используем (выдавать себя за чужое приложение нельзя). Текст ВК одного трека:
  `VKM_COOKIES=… VKM_VK_ID=… cargo test live_vk_lyrics -- --ignored --nocapture`. Скрытие строк частей песни и
  пустых строк (`lyrics_hide_parts`, `lyrics_compact`) — только в интерфейсе (`tidyLyrics`), кэш не меняется.
* VK Микс и плейлисты (просьба владельца, 2026-11-01; раздел «Плейлисты» — до 2026-11-06 «Миксы и плейлисты»;
  VK Микс — только своей страницей, карточки в разделе у него нет): через API ВК с **web-токеном самого сайта** —
  `login.vk.ru/?act=web_token` (`version=1`, `app_id=6287487`, куки сессии, как браузер), методы
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
  (`L(PAGES[page])`). Один русский текст с разными переводами — `Lc("уточнение", "Текст")`, в словаре ключ
  `"уточнение|Текст"` («Свернуть» окно — Minimize, боковую панель — `"панель|Свернуть"`: Collapse). Ошибки Rust остаются русскими; для показа — `errMsg(e)` (перевод по точному совпадению), для
  проверок текста ошибки — `errText(e)`. Переменная `t` в `app.js` — трек, поэтому функция перевода называется `L`.
* Никаких `<select>` и других родных выпадающих списков: в WebKitGTK их рисует GTK — белым поверх любой темы
  (просьба владельца, 2026-11-06). Выбор из списка — своё меню (`window.openPickMenu(btn, [{ label, on, fn }])`,
  то же `.tmenu`, что «⋯» трека) или свой список, как шрифты (`renderFontPicker`: `#fontCard`, `#fontFind`,
  `.fontlist > .fontitem` — разметка Whisper Yandex, оформление в whisper.css). Системных диалогов выбора файла
  для сохранения тоже нет: «Сохранить стиль в файл…» пишет в папку загрузок.
* Подтверждение опасных действий — маленькое окошко у кнопки (`confirmClick(btn, fn, вопрос, кнопка)`,
  `.confirm-pop`), не `confirm()` и не двойное нажатие (по просьбе владельца, 2026-10-30).
  Исключение по просьбе владельца: установка/удаление в библиотеке тем — одним нажатием.
* Разметка — контракт с темами Whisper: классы и id те же, что у Whisper Yandex (по ним написана библиотека тем).
  Не переименовывать и не перестраивать `.app`, `.side`, `.main`, `.head` (`#title`, `#sub`, `#headImg`, `#back`),
  `ul.list > li` (`img`, `div > .t/.a`, `.likebtn`, `.morebtn`), `.player` (`.pinfo` `#cover` `.meta` `#pTitle` `#pArtist`
  `#likeBtn`, `.pcenter` `.seekrow` `#seek` `.ctrls` `#play` `#prev` `#next` `.modebtn`, `.pright` `#lyrBtn` (своя кнопка «Текст песни») `#volWrap` `#volBtn`
  `.volpop` `#vol` `#pMore`), `#settingsPage.settings` (`.settabs .chip`, `.card[data-tab]`), `.simcard`, `.lastAlbum`,
  `#wavePage` (`.waveorbits`, `#waveBtn.wavebtn`), `#full`, `.titlebar`, `.tmenu`, `.toast`, `body.is-playing`.
  Свои элементы — дополнительными классами и внутри этих (`.lq`, `.lacts`, `.ldur` в строке). `whisper.css` не правится:
  своё — в `app.css`. Возможности тем сверх CSS (`settings whisper-music`): `features.animations`, `controls.volume:
  "knob"` (`data-vol-knob`, `--vol`), `buttons` (`#thBtns .thbtn`, действия — только `TH_ACTIONS`), `layout: "fixed"`
  (`Layout.lock`: действует расположение по умолчанию, своё не теряется; `#layoutCard.lylock`), `console` (`true` или
  `{ prompt, banner }` — стиль-консоль `cli.js`: `body.cli-on`, `.app` скрыт; команды — только свои, тема задаёт вид;
  themeguard не требует от неё кнопок пуска и настроек).

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
* «Сохранить стиль в файл…» (как у Whisper Yandex, 2026-11-06): `buildStyleFile` в app.js собирает текущий стиль
  со всем, что пользователь поменял (основной/второй цвет и «Тонкая настройка» → `settings.colorOverrides`,
  свечение, прозрачность, затемнение и размытие → `tokens.effects`, свой шрифт → `tokens.font`), `theme_save`
  (сырое тело, проверка `wmt::open`) пишет `<название>.wmt` в папку загрузок. Основа Whisper → копия `<основа>-copy`
  с правилами стиля из CSS приложения (`WhisperThemeCss.extractFrom` по всем `<link rel=stylesheet>`), встроенная
  тема VK Music → `<id>-copy`, своя тема — тот же id (загрузка обратно обновляет её). `colorOverrides` из файла темы
  `colors.js` читает как «Тонкую настройку» по умолчанию (`themeCc`; своя главнее).
* Защита от ломающих тем — `themeguard.js` (только темы из файлов, кроме встроенных): обязательный минимум —
  закрыть/свернуть/перетащить окно (кроме `--host`), «Пуск/пауза», настройки — родной кнопкой или кнопкой темы
  (`settings → buttons`). Нет два раза подряд — аварийная полоса (popover, инлайн-стили с !important); тема спрятала
  и её — «Неон». Ctrl+Shift+Backspace — «Неон» всегда. «Не показывать» для темы — `vkm_tguard`.
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
* «Постоянный уровень громкости» — по громкости EBU R128, как у Whisper Yandex (2026-11-06): `dynaudnorm` без
  целевого уровня выравнивал по пикам, а они у современных треков почти всегда у 0 дБ — громкие (−6 LUFS) и тихие
  (−11) звучали по-разному. Теперь файл на диске (папка музыки, кэш — `TrackCache::on_file`, архив) один раз
  замеряется (`loudness::Loudness`, по одному, nice 15) и играет с постоянным усилением до `TARGET` (ограничитель —
  только если пик выйдет за −1 dBTP); `af` ставится перед каждым `loadfile` (`Player::norm_filter`). Поток и ещё
  не замеренный файл — `loudnorm` на лету (`LIVE_FILTER`); посреди трека фильтр не меняется.
* Главное окно создаётся скрытым (`"visible": false`): показывает его `winstate::restore` в setup, уже с прежним размером.
  Новый путь запуска окна (не через setup) — не забыть `show()`. С `--host` окно так и остаётся скрытым.
* Трей на Linux: tray-icon сам грузит libayatana-appindicator3 / libappindicator3 (dlopen) и **падает**, если её нет, —
  поэтому `tray::init` только после `tray::available()`. В `.deb` она — в recommends. Мини-плеер — окно фиксированного
  размера (min = max): Hyprland и другие тайлинговые композиторы делают такое плавающим.
* Устройство вывода: список и текущее mpv присылает сам (`observe_property audio-device-list / audio-device`,
  `Player::devices`); выбранное — `Config::audio_device`, ставится, когда список пришёл (`restore_device`), и только
  из списка (`set_device`). mpv перечисляет каждое устройство в каждой звуковой системе и плагины ALSA (у владельца —
  ~35 строк), поэтому меню показывает `player::shown_devices`: «Системное», устройства первой системы mpv (pipewire /
  wasapi) по картам и выбранное; остальное — «Все устройства». Скорость — «⋯» плеера (`setRate`, не сохраняется между запусками).
* Полоса трека (`#seek`, `#fSeek`) — как у ВКонтакте (просьба владельца, 2026-11-07): родной кружок скрыт (`app.css`,
  `::-webkit-slider-thumb` нулевой с `!important`), указатель обрабатывает `app.js` сам (`preventDefault`): время —
  из координаты по ширине полосы, перемотка — при отпускании, каждое нажатие перематывает, над полосой — `.seektip`.
  Кружок — свой `.seekdot` (`placeSeekDot`: на конце заливки, `pointer-events: none`; оформление основ — в `app.css`,
  встроенных тем — в их `theme.wmt`), показ — «Расположение» (`vkm_layout.dot`, `data-ly-dot`).
  Родной ползунок не годился: нажатие на кружок не давало `change`, а кружок стоял со сдвигом на свою ширину.
  Родные `input` / `change` — только клавиатуре.
* Громкость у mpv кубическая (усиление = (v/100)³): ползунок на 14% давал −51 дБ, линейная шкала оказалась слишком
  громкой. Шкала интерфейса — квадратичная (`player::GAIN_POW`, «обычная громкость — около 20%», просьба владельца
  2026-11-05); в mpv — через `player::to_mpv`, обратно — `from_mpv`. Не передавайте в mpv громкость ползунка напрямую.
* Ползунок громкости (2026-11-06): интерфейс обновляется раз за кадр (`requestAnimationFrame`), в mpv — не чаще раза
  в 50 мс; `--vol` на `<html>` — только у темы с ручкой (`syncVolumeVar`): переменная на корне пересчитывала стили
  всего документа на каждый шаг. Изменение громкости в mpv не шлёт срочное состояние (`player.rs`).
* Перетаскивание края панели (`layout.js`) меняет размер на каждом шаге. CSS-переменная в `style` у `<html>` пересчитывала
  стили всего документа (~120 мс на шаг с 3000 строк, 2026-11-07), поэтому `--ly-sw` / `--ly-ph` стоят на `.app` и
  вместе с `--sw` объявлены ненаследуемыми (`@property` в layout.css), а `apply()` трогает только изменившееся.
  Пока тянут боковую панель, строки списка вне экрана не раскладываются (`html.lyfast .list li { content-visibility:
  auto }`), кроме списков со счётчиком CSS (номера «Стимпанка»: `content-visibility` замкнул бы счётчик в строке).
* Смена трека: после `loadfile` mpv ещё присылает позицию и длительность прошлого файла. Поэтому у состояния есть
  номер файла (`PlayerState.file`, растёт на `file-loaded`), а между `start-file` и `file-loaded` позиция — 0;
  интерфейс не верит состояниям с прежним номером (`S.fileAtPlay`) — иначе «Слушать вместе» отдавало гостям новый
  трек с позицией старого. Пауза — явным `player_pause` (не `cycle`).
* `Player::load` перед `loadfile` ставит mpv `pause = hold` (обычно `false`): пауза, нажатая, пока трек включается
  (ссылка на поток, архив — секунды), снималась сама — «не паузится, даже с нескольких нажатий» (2026-11-06). Поэтому
  `togglePlay` при `S.pendingPlay` кладёт её и в `player_cue`, а `settlePlay` после загрузки повторяет и обнуляет cue.
* «loading failed» от mpv ничего не говорит: плеер подписан на журнал mpv (`request_log_messages warn`) и дописывает
  к ошибке последнюю строку (`log_reason`, ссылки вырезаются — в них подписи). Новые ссылки ВК на официальные треки
  (с альбомом и обложкой) — CDN `*.okcdn.ru`, без расширения, подписаны под IP (`srcIp`) того, кто запросил API.
  На Windows API ходит через системный прокси / VPN, mpv — напрямую: «Failed to open». Поэтому потоки треков из API
  (`vk_play_api`, кэш через `service_job`) идут через ретранслятор `relay.rs` (127.0.0.1, токен, только ссылки,
  зарегистрированные Rust; `via_relay`). HLS через него не пускать: относительные адреса сегментов не откроются.
  Проверка: `VKM_COOKIES=… cargo test live_relay -- --ignored --nocapture`.
