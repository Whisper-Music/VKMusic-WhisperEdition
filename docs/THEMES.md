# Темы VK Music (`.wmt`)

Темы VK Music — **темы Whisper** (Whisper Music Theme). Формат файла — `src/core/docs/WMT-FORMAT.md` из
[whisper-core](https://github.com/Whisper-Music/whisper-core); библиотека тем — [whisper-themes](https://github.com/Whisper-Music/whisper-themes).
У VK Music **та же разметка, что у Whisper Yandex** ([YandexMusic-WhisperEdition](https://github.com/Whisper-Music/YandexMusic-WhisperEdition)):
те же классы и id, та же основа оформления (`src/whisper.css` — их `styles.css` без изменений). Поэтому любая тема
Whisper применяется целиком: цвета, шрифты, фон, своя разметка панелей, кнопок, плеера и полосы заголовка.

## Как применяется тема

Так же, как в Whisper Yandex: файл разбирает ядро (`WMT.open`), CSS собирает движок тем ядра
(`WhisperThemeCss.themeCss(th, id, "whisper-music")`) в таком порядке:

1. `@@ css whisper-core` — общие правила по **ролям** (`[data-w="player.play"]`), одинаковые во всех приложениях Whisper;
2. `@@ css whisper-music` — правила по разметке Whisper (у VK Music она та же);
3. `@@ css whisper-vk` — необязательная секция только для VK Music (поверх, главнее);
4. `tokens`: `color.text`, `color.muted`, шрифты (`font.body` на `body`, `font.display` — `--font-display`), фон окна.

Каждое правило привязывается к стилю темы: `.panel {…}` → `:root[data-style="<id>"] .panel {…}`. Основа темы
(`manifest.base`: `neon` по умолчанию, `royal`, `steam`, `lolitta`) — атрибут `data-base` на `<html>`.

Из `tokens` в переменные основы: `color.accent` / `color.accent2` (`#rrggbb`) → `--accent`, `--accent2`;
`effects.glow` (0–48) → `--glow`; `effects.panelOpacity` (0.2–1) → `--panel-a`; `effects.dim` (0–0.85) → `--dim`;
`effects.backgroundBlur` → `--bgblur`. Фон темы (`background.*` в архиве) — `--bg-image` на `#bg` (картинка) или
`#bgVideo` внутри `#bg` (видео). «Фон и панели» в настройках (затемнение, размытие, непрозрачность) и выбранный шрифт
интерфейса главнее темы.

Возможности сверх CSS — `@@ settings whisper-music`, как у Whisper:

* `features.animations: false` — анимации выключены (`html[data-anim="off"]`);
* `controls.volume: "knob"` — громкость ручкой: `html[data-vol-knob]`, `#volBtn` тянется мышью, громкость числом — `--vol` (0…1, на `<html>`, только у такой темы);
* `buttons: { "имя": "действие" }` — кнопки темы `#thBtns > button.thbtn[data-th="имя"]` (где стоят и как выглядят —
  решает CSS темы; `.on` — действие сейчас включено). Действия: `wave` (VK Микс), `play`, `next`, `prev`, `like`
  (в библиотеку), `mute`, `shuffle`, `repeat`, `full` (обложка и текст на весь экран), `more`, `page:wave`, `page:likes`
  (библиотека), `page:stats`, `page:settings`, `page:profile` (сервисы), `vol-up`, `vol-down`, `close`, `minimize`,
  `maximize`, `drag`;
* `layout: "fixed"` — расположение задаёт тема: настройки «Расположение» (сторона и вид панелей, их размер) не действуют,
  пока выбрана тема (`#layoutCard.lylock`), — интерфейс в раскладке по умолчанию;
* `console: true` или `{ "prompt": "user@vkmusic:~$", "banner": "текст" }` — стиль-консоль, как у Whisper Yandex
  (тема «Терминал»): вместо окна — командная строка. Разметка та же: `body.cli-on`, `#cli.cli` > `.cli-out` > `.cl`
  (`.cmd`, `.err`, `.dim`, `.ok`, `.hl`, `.hd`, `.banner`), `.cli-status`, `form.cli-in` > `.cli-ps` + `input`. Команды —
  свои у VK Music (`help`: play, next, seek, vol, speed, search, lib, mix, playlists, lyrics, style, gui…); тема их не
  меняет. Скрытых команд-пасхалок Whisper Yandex (visual, piano…) нет;
* синхронный текст песни (треки ВК, у которых он есть) — разметка Whisper Yandex: `.flyrics.synced > .ln`, текущая
  строка — `.ln.on`.

## Чем VK Music отличается от Whisper Yandex

Разметка та же, страницы — свои. Где у Whisper Yandex «Моя волна», «Лайки», «Профиль», у VK Music:

| Whisper Yandex | VK Music |
|---|---|
| `#wave` «Моя волна», `#wavePage` с `#waveBtn` | «VK Микс»: та же страница и кнопка (`.wavebtn`, `.waveorbits`) |
| `#likes` «Лайки» | «Библиотека»: файлы из `~/Music/vk` и треки ВКонтакте / SoundCloud |
| `#profile` «Профиль» | «Сервисы»: вход во ВКонтакте и SoundCloud, yt-dlp (карточки `.card`) |
| «Похожие альбомы» (`.similar` > `.simgrid` > `.simcard`) | «Плейлисты» в библиотеке, раздел «Плейлисты» |
| `.lastAlbum` у артиста | карточка «Сейчас играет» рядом со списком библиотеки (`.lastAlbum.nowcard`) |
| `#full` с текстом Яндекса / LRCLIB | `#full` с текстом ВКонтакте / Genius (`#fSrc`: «Текст», язык, «Пояснения», «Genius») |
| вкладки настроек `look ui play out app int` | `look` Оформление, `ui` Общее, `play` Текст песен, `out` Хранилище, `int` Интеграции |

Свои элементы VK Music (их у Whisper нет — темы их не знают, оформление по умолчанию в `app.css`):

* в строке трека `ul.list > li.track`: `.lq` (формат и битрейт / значок сервиса), `.lacts` (скачать, удалить, в плейлист —
  `.icon-btn`, видны при наведении), `.ldur` (длительность);
* в шапке страницы `.head`: `.headbtns` — кнопки справа («Обновить»);
* в плеере `.pright`: `#normBtn.modebtn` — «постоянный уровень громкости»;
* на полосе трека (`.seekrow > #seek`, `.fseek > #fSeek`): кружок — **`.seekdot`**, не родной кружок ползунка.
  Перемотка — как у ВКонтакте (время — прямо из места нажатия, каждое нажатие перематывает), поэтому
  `#seek::-webkit-slider-thumb` и `#fSeek::-webkit-slider-thumb` скрыты, а `.seekdot` (указатель не ловит) стоит на конце
  заливки. Оформляйте его вместо `::-webkit-slider-thumb`: размер, фон, тень, `transform` (центровка — через `translate`,
  её не трогайте). Пользователь может спрятать кружок — `html[data-ly-dot="off"]`;
* подсказка со временем под курсором над полосой трека — `.seektip`;
* окошки подтверждения и выбора плейлиста — `.popup.tmenu.confirm-pop` (оформлены как меню `.tmenu`);
* `.login-bar` — полоса над страницей входа сервиса, `.drop-hint` — «отпустите файл .wmt».

Модули ядра Whisper (очередь `.qpanel`, Whisper Archive, «Слушать вместе», Discord, OBS) — те же, что в Whisper Yandex.

## Роли (`@@ css whisper-core`)

Роли — те же, что у Whisper Yandex (`src/whisper-vk.js`, `Whisper.ui.define`; словарь — `VOCAB` в
`src/core/whisper-core.js`). Например: `titlebar`, `sidebar`, `nav` / `nav:active`, `search:global`, `main`,
`page.title`, `button` / `button:active` / `button:danger`, `chip` / `chip:active`, `tracklist`, `track` / `track:playing`,
`track.cover`, `track.title`, `track.artist`, `track.like`, `track.more`, `card`, `tile`, `player`, `player.cover`,
`player.info`, `player.title`, `player.artist`, `player.like`, `player.controls`, `player.play`, `player.prev`,
`player.next`, `player.mode` / `player.mode:on`, `player.seek`, `player.time`, `player.actions`, `player.extra`,
`player.volume`, `player.more`, `radio`, `menu`, `menu.item`, `toast`, `dialog`, `queue`, `fullplayer`, `lyrics`.
`player.extra` у VK Music — `#togetherBtn`, `#lyrBtn` (текст песни), `#normBtn`.
Роли, которой нет (`radio.stations`, `player.dislike`), — селектор с ней выбрасывается, остальные из списка остаются.

## Ограничения CSS темы

Как в Whisper: `@import` вырезается; `url()` — только ресурсы самой темы (`url(asset:имя)`), `data:` и `#якоря`
(объявление с внешним адресом выбрасывается целиком); `image-set()`, `src()`, `expression()`, `javascript:` — тоже.
`@keyframes`, `@font-face`, `@layer` темы получают префикс `wmt-<id>-` (ссылки на них внутри темы — тоже): тема не
подменяет анимации и шрифты приложения. Сдвиги и увеличение при `:hover` / `:active` — только при `html[data-anim="on"]`.

## Встроенные темы и свои

* Встроенные: **Osaka Jade**, **Vantablack**, **Ashen Inventory** (основа «Королевский»), **Грифель**; плюс четыре
  основы Whisper без файла — «Неон», «Королевский», «Стимпанк», «Lolitta». Их id заняты.
* Свои: «Настройки → Оформление → Загрузить .wmt», перетащить файл в окно или «Магазин тем». Хранятся в
  `~/.config/vkmusic/themes/<id>.wmt`; повторная загрузка с тем же `manifest.id` обновляет тему.
* Тема без `@@ css whisper-music` / `whisper-core` / `whisper-vk` и без цветов в `tokens` не ставится — применять нечего.
* Старые темы VK Music с секцией `@@ css vkmusic` (прежняя разметка) больше не читаются: перепишите их под разметку
  Whisper (секция `whisper-music`), как встроенные.
* «Экспорт стиля» (как у Whisper Yandex) кладёт текущий стиль в папку загрузок: с вашими цветами
  (`tokens.color`, «Тонкая настройка» — `settings whisper-music → colorOverrides`), эффектами (`tokens.effects`) и
  шрифтом. Основа Whisper сохраняется копией `<основа>-copy` с правилами стиля, встроенная тема — `<id>-copy`, своя —
  под своим id. Поправьте файл и загрузите обратно. `colorOverrides` из файла темы VK Music читает так же, как
  Whisper Yandex.
* Тема не должна прятать минимум: закрыть, свернуть и перетащить окно, «Пуск/пауза» и настройки — родными кнопками
  или своими (`settings → buttons`: `close`, `minimize`, `drag`, `play`, `page:settings`). Иначе появится аварийная
  полоса с этими кнопками и «Стандартный стиль». Ctrl+Shift+Backspace всегда включает «Неон».

## Собрать тему из папки

```sh
cp -r themes/osaka-jade themes/my-theme        # theme.wmt, background.webp, preview.jpg
$EDITOR themes/my-theme/theme.wmt                # поменять manifest.id и name, цвета, CSS в @@ css whisper-music
scripts/pack-theme.sh themes/my-theme            # → themes/my-theme.wmt
```

Проверить тему без приложения можно в браузере: `src/index.html` с заглушкой `window.__TAURI__` (так проверялись
темы библиотеки: «Windows 98», «Телевизор», ASCII, «Мел»).
