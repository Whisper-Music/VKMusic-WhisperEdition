<div align="center">

<img src="src-tauri/icons/128x128.png" width="128" alt="Логотип VK Music Whisper Edition" />

# VK Music (Whisper Edition)

**Вся твоя музыка в одном плеере — и в твоём стиле.**
Неофициальный десктопный клиент: ВКонтакте, SoundCloud и свои файлы в одной библиотеке,
темы Whisper, тексты песен, статистика, Discord, OBS и друзья в одной комнате.

[![Release](https://img.shields.io/github/v/release/Whisper-Music/VKMusic-WhisperEdition?label=версия&color=8b5cf6)](../../releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Whisper-Music/VKMusic-WhisperEdition/total?label=скачиваний&color=ec4899)](../../releases)
![Linux](https://img.shields.io/badge/Linux-x64-f59e0b)
![Windows](https://img.shields.io/badge/Windows-x64-0078d4)
[![License](https://img.shields.io/badge/лицензия-PolyForm%20Noncommercial-64748b)](LICENSE)

[**Скачать**](../../releases/latest) · [Возможности](#-возможности) · [Темы](#-темы-и-магазин-тем) · [Установка](#-установка)

</div>

---

## ✨ Коротко

* **Одна библиотека на всё.** Файлы с диска, «Мои треки» ВКонтакте и лайки SoundCloud — одним списком
  в твоём порядке. Синхронизируется само.
* **Свой внешний вид до последней кнопки.** Интерфейс Whisper: четыре встроенных стиля, любые темы `.wmt`
  из магазина тем применяются целиком — вплоть до лампового телевизора с ручкой громкости.
* **Слушай сразу.** Включённый трек кэшируется и дальше играет с диска; следующий в очереди готовится заранее.
* **Скачивай без потерь.** Трек библиотеки — файлом в «Загрузки», ссылка YouTube / Bandcamp / SoundCloud —
  через yt-dlp в лучшем качестве, без перекодирования.
* **Музыка вместе и для стрима.** «Слушать вместе», статус в Discord, виджет «Сейчас играет» для OBS.
* **Лёгкий.** Tauri (Rust + системный WebView), звук — mpv.

---

## 🧩 Возможности

### 🎵 Библиотека и сервисы

* **ВКонтакте** — поиск, «Мои треки» в библиотеке, свои плейлисты (создать, переименовать, удалить, добавить трек),
  подборки с главной музыки ВК, **VK Микс** — бесконечный, догружается сам. Тексты песен самого ВКонтакте.
* **SoundCloud** — поиск, ссылки на треки и плейлисты, лайки в библиотеке. Поиск работает и без входа.
* **Свои файлы** из `~/Music/vk` — с тегами, обложками, форматом и битрейтом.
* **Вход — обычной страницей сервиса** прямо в окне (во ВКонтакте можно по QR-коду). Пароль приложение не видит.
* **Порядок библиотеки** — перетаскиванием мышью; новые треки встают первыми.
* **«Сердце»** — трек в библиотеке: у найденного трека, у трека из плейлиста или микса и у играющего.

### 🎧 Плеер

* **mpv в фоне:** очередь, перемешивание, повтор, перемотка, «постоянный уровень громкости».
* **Кэш треков:** включённый трек библиотеки сохраняется и дальше играет с диска, без запросов к сервису.
* **Текст песни на весь экран** — нажми на обложку: ВКонтакте или Genius, переводы Genius,
  пояснения к строкам по нажатию. Найденные тексты появляются сразу при следующем включении.
* **Очередь и история** — панель с «Далее» (убрать, перетащить) и историей по дням. Клавиша Q.
* **Статистика прослушиваний:** сегодня / неделя / месяц / всё время, графики по дням и часам, топ треков
  и исполнителей, «давно не слушали». Хранится только на твоём компьютере.
* **Клавиши:** Пробел — пауза, ←/→ — ±5 с (Shift — 30 с), Ctrl+F или `/` — поиск, Ctrl+N / Ctrl+P — следующий / предыдущий.

### ⬇️ Скачивание

* **Трек библиотеки — файлом** в папку загрузок (по умолчанию `~/Downloads`) кнопкой со стрелкой.
* **Любая ссылка, которую понимает yt-dlp** (YouTube, YouTube Music, SoundCloud, Bandcamp…) — в строке поиска,
  Enter: лучшая аудиодорожка **без перекодирования** — Opus остаётся Opus, FLAC — FLAC, с тегами и обложкой.
  Плейлист или альбом — целиком.

### 👥 Вместе — модули Whisper

* **«Слушать вместе».** Комната и код для друзей: у всех тот же трек с той же секунды, каждый слушает со своего
  аккаунта. Гости предлагают треки, ведущий ставит их следующими. Приглашение — ссылкой `whisper://room/КОД`.
* **[Whisper Archive](https://origin.whisper-archive.ru/)** — свои версии треков: если трек есть в архиве, он играет оттуда.
  Трек библиотеки можно сохранить в архив из меню «⋯».
* **Discord Rich Presence** — «Слушает…» в профиле: трек, исполнитель, обложка, время.
* **Виджет для OBS** — «Сейчас играет» для стрима, оформление — из темы.

### 🔒 Безопасность и бережность

* Сессии ВКонтакте и SoundCloud — куки самого WebView, «Выйти» их удаляет. Ключ Whisper Archive — в системной
  связке ключей (GNOME Keyring, KWallet, KeePassXC).
* К сервисам — только по твоему действию и не чаще запроса в секунду: без фоновых обходов и «прогрева».
* Темы — **только оформление**: CSS темы не может загрузить ничего из интернета и не выполняет код.

---

## 🎭 Темы и магазин тем

У VK Music **та же разметка и то же оформление, что у [Whisper Yandex](https://github.com/Whisper-Music/YandexMusic-WhisperEdition)** —
поэтому любая тема Whisper применяется целиком: цвета, шрифты, фон, свои панели, плеер, заголовок окна,
ручки и кнопки темы. Готовые темы ставятся прямо из приложения — **Настройки → Оформление → «Магазин тем»**,
все они лежат в [Whisper-Music/whisper-themes](https://github.com/Whisper-Music/whisper-themes):

| | | |
|:---:|:---:|:---:|
| <img src="https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/previews/whisper/chalk.jpg" width="260" alt="Мел"><br>**Мел** | <img src="https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/previews/whisper/retro-tv.jpg" width="260" alt="Телевизор"><br>**Телевизор** | <img src="https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/previews/whisper/windows-98.jpg" width="260" alt="Windows 98"><br>**Windows 98** |
| <img src="https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/previews/whisper/axiom.jpg" width="260" alt="Аксиома"><br>**Аксиома** | <img src="https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/previews/whisper/cartoon.jpg" width="260" alt="Cartoon"><br>**Cartoon** | <img src="https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/previews/whisper/terminal.jpg" width="260" alt="Терминал"><br>**Терминал** |

> Превью — из библиотеки тем (сняты в Whisper Yandex); в VK Music темы выглядят так же.

**Встроенные стили:** четыре основы Whisper — «Неон», «Королевский», «Стимпанк», «Lolitta» — и свои темы VK Music:

* **Osaka Jade** (по умолчанию) — палитра и обои *Glowing City* из темы Omarchy Osaka Jade;
* **Vantablack** — чистый чёрный и серые, обои *dot hands* из темы Omarchy Vantablack;
* **Ashen Inventory** — в духе экрана инвентаря мрачных RPG: камень, бронзовые рамки, слоты под обложки,
  тлеющий пепел. Всё нарисовано на CSS;
* **Грифель** — стеклянные панели поверх школьной доски со стёртым мелом; фон нарисован процедурно.

Свою тему можно перетащить прямо в окно. Как устроены темы — [docs/THEMES.md](docs/THEMES.md),
формат файла — `docs/WMT-FORMAT.md` в [whisper-core](https://github.com/Whisper-Music/whisper-core).

---

## 📦 Установка

**Linux x64** — скачай со страницы [Releases](../../releases/latest):

* `.deb` — Debian, Ubuntu, Mint: `sudo apt install ./VK.Music_*_amd64.deb` (mpv и ffmpeg поставятся сами);
* `.AppImage` — любой дистрибутив: `chmod +x VK*.AppImage && ./VK*.AppImage` (mpv и ffmpeg — из пакетов системы).

Для загрузок по ссылкам нужны `yt-dlp` и `python-mutagen` (обложки в файлах). Держи yt-dlp свежим — сайты часто меняются.
Для медиаклавиш поставь `mpv-mpris` — mpv подхватит его сам.

**Интерфейс в браузере** — `vkmusic --host` (или `--host 8080`): вместо окна интерфейс откроется во вкладке
браузера по умолчанию на `http://127.0.0.1:7380`. Музыка играет так же — через mpv на этом компьютере; окно
появится только для входа во ВКонтакте или SoundCloud. Адрес с ключом доступа печатается в консоль, работает одна
вкладка за раз (новая забирает плеер у старой), выход — Ctrl+C. История очереди и настройки модулей у браузера свои.

**Windows 10/11 x64** — `VK.Music_*_x64-setup.exe` со страницы [Releases](../../releases/latest). mpv, ffmpeg и
yt-dlp уже внутри установщика, ставить ничего не нужно. WebView2 есть в Windows 11; на Windows 10 без него
установщик скачает его сам. Установщик не подписан, поэтому SmartScreen может предупредить: «Подробнее → Выполнить
в любом случае». Настройки — в `%APPDATA%\vkmusic`, музыка — в `Музыка\vk`. Обновления — новым установщиком поверх
старого.

<details>
<summary><b>Сборка из исходников</b></summary>

Нужен Rust. Клонируй с подмодулем — ядро Whisper лежит в `src/core`
(если склонировал без ключа — `git submodule update --init`).

Arch / Manjaro:

```bash
sudo pacman -S --needed base-devel rust webkit2gtk-4.1 mpv yt-dlp ffmpeg python-mutagen
git clone --recurse-submodules https://github.com/Whisper-Music/VKMusic-WhisperEdition.git
cd VKMusic-WhisperEdition
cargo build --release && ./target/release/vkmusic
./packaging/install.sh      # в ~/.local/bin + ярлык в меню и ссылки whisper://
```

Debian / Ubuntu / Mint:

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libgtk-3-dev build-essential pkg-config libdbus-1-dev \
  mpv ffmpeg yt-dlp python3-mutagen
cargo build --release
```

Пакет `.deb` / `.AppImage`: `npx --yes @tauri-apps/cli@2 build --bundles deb,appimage`.

</details>

<details>
<summary><b>Где что лежит</b></summary>

| | |
|---|---|
| музыка | `~/Music/vk` (поле `music_dir` в config.json) |
| настройки | `~/.config/vkmusic/config.json` |
| свои темы | `~/.config/vkmusic/themes/*.wmt` |
| треки сервисов в библиотеке | `~/.config/vkmusic/library.json` |
| порядок, статистика, плейлисты ВК | `~/.config/vkmusic/{order,stats,playlists}.json` |
| кэш треков и текстов | `~/.cache/vkmusic/{tracks,lyrics}` |
| сессии ВК и SoundCloud | куки WebView в `~/.local/share/vkmusic-whisper/` |
| ключ доступа `--host` | `~/.config/vkmusic/host-key` |

</details>

<details>
<summary><b>Если что-то не так</b></summary>

* **Пустое окно на NVIDIA / Wayland** — запусти `WEBKIT_DISABLE_DMABUF_RENDERER=1 vkmusic` (со старым драйвером
  NVIDIA < 560 приложение делает это само). Если не помогло — `WEBKIT_DISABLE_COMPOSITING_MODE=1 vkmusic`.
* **Не скачивается по ссылке** — обнови yt-dlp.
* **Нет обложек в скачанных файлах** — поставь `python-mutagen`.

</details>

---

## ⚠️ Дисклеймер

**VK Music (Whisper Edition) — неофициальный сторонний проект.** Он не связан с ВКонтакте (VK) и SoundCloud,
не разрабатывается и не поддерживается ими. Для прослушивания нужен свой аккаунт. ВКонтакте не разрешает
сторонние клиенты для музыки — пользуйся на свой риск: аккаунт могут ограничить.

Названия «ВКонтакте», «VK», «SoundCloud», логотипы, музыкальные произведения, обложки, метаданные и другие
материалы сервисов принадлежат их правообладателям. Лицензия этого репозитория распространяется только на
оригинальный исходный код проекта.

## 🛠 Статус

Проект активно развивается: новые функции выходят регулярно, внешний вид и возможности могут меняться.
Нашёл ошибку или есть идея — открой [issue](../../issues).

## 📄 Лицензия

Исходный код распространяется под лицензией **PolyForm Noncommercial License** — подробнее в файле [LICENSE](LICENSE).
Основа оформления `src/whisper.css` — из [Whisper Yandex](https://github.com/Whisper-Music/YandexMusic-WhisperEdition),
ядро `src/core` — [whisper-core](https://github.com/Whisper-Music/whisper-core). Обои Osaka Jade и Vantablack — из тем Omarchy (MIT).
