//! VK Music — музыкальный плеер: библиотека ~/Music/vk, загрузка через yt-dlp, темы .wmt,
//! прослушивание ВКонтакте и SoundCloud под своим аккаунтом, кэш треков библиотеки и загрузка их в ~/Downloads.

mod cache;
mod config;
mod deeplink;
// Rust-часть ядра Whisper (подмодуль src/core): Discord, виджет OBS, Whisper Archive и «Слушать вместе».
// Своё у приложения — whisper_host.rs (ключ, семафор, Origin, Discord/OBS), whisper_vk.rs (архив и треки VK Music)
#[path = "../../src/core/rust/discord.rs"]
mod discord;
mod fonts;
mod library;
mod lyrics;
#[path = "../../src/core/rust/obs.rs"]
mod obs;
mod order;
mod player;
mod playlists;
mod saved;
mod sc;
mod secret;
mod stats;
mod streams;
mod themelib;
mod themes;
mod vk;
#[path = "../../src/core/rust/whisper/mod.rs"]
mod whisper;
mod whisper_host;
mod whisper_vk;
mod wmt;
mod ytdlp;

use config::Config;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    cfg: Mutex<Config>,
    themes: themes::ThemeStore,
    http: reqwest::Client,
    vk: vk::Vk,
    sc: sc::Sc,
    saved: saved::SavedStore,
    /// ваши плейлисты ВК на диске (playlists.json): показываются сразу, сеть их обновляет
    playlists: playlists::PlaylistStore,
    /// порядок треков библиотеки (order.json)
    order: order::OrderStore,
    player: Mutex<Option<Arc<player::Player>>>,
    player_error: Mutex<Option<String>>,
    dl_seq: AtomicU64,
    /// ссылки на потоки уже прослушанных треков сервисов (в памяти, пока действуют)
    streams: streams::StreamCache,
    /// кэш треков библиотеки на диске (~/.cache/vkmusic/tracks)
    cache: Arc<cache::TrackCache>,
    /// статистика прослушивания (stats.json)
    stats: stats::StatsStore,
    /// тексты песен с Genius
    genius: lyrics::Genius,
}

type Res<T> = Result<T, String>;

impl AppState {
    fn player(&self) -> Res<Arc<player::Player>> {
        if let Some(p) = self.player.lock().unwrap().as_ref() {
            return Ok(p.clone());
        }
        Err(self
            .player_error
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| "Плеер не запущен".into()))
    }

    fn save(&self) -> Res<()> {
        self.cfg.lock().unwrap().save()
    }

    fn music_dir(&self) -> PathBuf {
        self.cfg.lock().unwrap().music_dir()
    }

    /// Сохранённый трек сервиса, если он в библиотеке.
    fn saved(&self, service: &str, id: &str) -> Option<saved::Saved> {
        self.saved.list().into_iter().find(|s| s.service == service && s.id == id)
    }
}

// ───────────────────────── общее ─────────────────────────

#[derive(Serialize)]
struct Info {
    music_dir: String,
    theme: String,
    volume: f64,
    overrides: std::collections::HashMap<String, f64>,
    shuffle: bool,
    repeat: String,
    normalize: bool,
    font: Option<String>,
    /// сервисы, в которых ищет общая строка поиска: "vk", "sc"
    search: Vec<String>,
    /// скрытые категории библиотеки тем
    hidden_theme_categories: Vec<String>,
    /// язык интерфейса: "ru" | "en"
    lang: String,
    lyrics_hide_parts: bool,
    lyrics_compact: bool,
    /// папка загрузок (развёрнутый путь)
    download_dir: String,
    player_error: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
fn app_info(st: State<'_, AppState>) -> Res<Info> {
    let c = st.cfg.lock().unwrap().clone();
    Ok(Info {
        music_dir: c.music_dir().display().to_string(),
        download_dir: c.download_dir().display().to_string(),
        theme: c.theme,
        volume: c.volume,
        overrides: c.overrides,
        shuffle: c.shuffle,
        repeat: c.repeat,
        normalize: c.normalize,
        font: c.font,
        search: c.search,
        hidden_theme_categories: c.hidden_theme_categories,
        lang: c.lang,
        lyrics_hide_parts: c.lyrics_hide_parts,
        lyrics_compact: c.lyrics_compact,
        player_error: st.player_error.lock().unwrap().clone(),
    })
}

#[tauri::command(rename_all = "snake_case")]
fn save_prefs(
    st: State<'_, AppState>,
    overrides: Option<std::collections::HashMap<String, f64>>,
    shuffle: Option<bool>,
    repeat: Option<String>,
    volume: Option<f64>,
    // Some("") — сбросить шрифт к шрифту темы
    font: Option<String>,
    search: Option<Vec<String>>,
    hidden_theme_categories: Option<Vec<String>>,
    lang: Option<String>,
    lyrics_hide_parts: Option<bool>,
    lyrics_compact: Option<bool>,
    // Some("") — папка загрузок по умолчанию (~/Downloads)
    download_dir: Option<String>,
) -> Res<()> {
    {
        let mut c = st.cfg.lock().unwrap();
        if let Some(o) = overrides {
            c.overrides = o;
        }
        if let Some(s) = shuffle {
            c.shuffle = s;
        }
        if let Some(r) = repeat.filter(|r| ["off", "all", "one"].contains(&r.as_str())) {
            c.repeat = r;
        }
        if let Some(l) = lang.filter(|l| l == "ru" || l == "en") {
            c.lang = l;
        }
        if let Some(b) = lyrics_hide_parts {
            c.lyrics_hide_parts = b;
        }
        if let Some(b) = lyrics_compact {
            c.lyrics_compact = b;
        }
        if let Some(list) = hidden_theme_categories {
            let mut l: Vec<String> = list.into_iter().filter(|x| wmt::valid_theme_id(x)).take(50).collect();
            l.sort();
            l.dedup();
            c.hidden_theme_categories = l;
        }
        if let Some(list) = search {
            let mut l: Vec<String> = list.into_iter().filter(|x| x == "vk" || x == "sc").collect();
            l.dedup();
            c.search = l;
        }
        if let Some(v) = volume {
            c.volume = v.clamp(0.0, 100.0);
        }
        if let Some(d) = download_dir {
            let d = d.trim().to_string();
            if d.is_empty() {
                c.download_dir = None;
            } else if config::valid_dir(&d) {
                c.download_dir = Some(d);
            } else {
                return Err("Укажите полный путь к папке (например, ~/Downloads)".into());
            }
        }
        if let Some(f) = font {
            let f = f.trim().to_string();
            if f.is_empty() {
                c.font = None;
            } else if fonts::valid(&f) {
                c.font = Some(f);
            } else {
                return Err("Недопустимое имя шрифта".into());
            }
        }
    }
    st.save()
}

/// Семейства шрифтов, установленных в системе (fontconfig).
#[tauri::command(rename_all = "snake_case")]
async fn fonts_list() -> Res<Vec<String>> {
    tauri::async_runtime::spawn_blocking(fonts::list)
        .await
        .map_err(|e| e.to_string())?
}

// ───────────────────────── плеер ─────────────────────────

/// Команды «играть» возвращают true, если трек начал кэшироваться (тогда придёт `cache://event`);
/// false — он уже в кэше или не из библиотеки, следующий в очереди можно кэшировать сразу.
#[tauri::command(rename_all = "snake_case")]
fn player_play(st: State<'_, AppState>, src: String, title: Option<String>) -> Res<bool> {
    let p = st.player()?;
    st.cache.begin();
    // играем только файлы из папки музыки
    if !library::inside(&st.music_dir(), Path::new(&src)) {
        return Err("Файл вне папки музыки".into());
    }
    let title = title.unwrap_or_default();
    let Some(job) = local_job(&src) else { return p.load(&src, &title).map(|_| false) };
    if let Some(c) = st.cache.get(&job.key) {
        return p.load(&c.to_string_lossy(), &title).map(|_| false);
    }
    p.load(&src, &title)?;
    st.cache.enqueue(job);
    Ok(true)
}

fn local_job(src: &str) -> Option<cache::Job> {
    Some(cache::Job {
        key: cache::local_key(Path::new(src))?,
        lib: src.to_string(),
        source: cache::Source::File(src.into()),
        artist: String::new(),
        title: String::new(),
    })
}

/// Следующее включение начать с секунды `at`; `hold` — загрузить на паузе («Слушать вместе», ядро Whisper).
#[tauri::command(rename_all = "snake_case")]
fn player_cue(st: State<'_, AppState>, at: f64, hold: bool) -> Res<()> {
    st.player()?.cue(if at.is_finite() { at } else { 0.0 }, hold);
    Ok(())
}

/// Скорость воспроизведения (0.25–4).
#[tauri::command(rename_all = "snake_case")]
fn player_rate(st: State<'_, AppState>, rate: f64) -> Res<()> {
    st.player()?.set_speed(if rate.is_finite() { rate } else { 1.0 })
}

#[tauri::command(rename_all = "snake_case")]
fn player_toggle(st: State<'_, AppState>) -> Res<()> {
    st.player()?.toggle()
}

#[tauri::command(rename_all = "snake_case")]
fn player_pause(st: State<'_, AppState>, paused: bool) -> Res<()> {
    st.player()?.set_pause(paused)
}

#[tauri::command(rename_all = "snake_case")]
fn player_seek(st: State<'_, AppState>, pos: f64) -> Res<()> {
    st.player()?.seek(pos)
}

#[tauri::command(rename_all = "snake_case")]
fn player_volume(st: State<'_, AppState>, volume: f64) -> Res<()> {
    st.cfg.lock().unwrap().volume = volume.clamp(0.0, 100.0);
    st.player()?.set_volume(volume)
}

/// «Постоянный уровень громкости»: выравнивание громкости в mpv (dynaudnorm).
#[tauri::command(rename_all = "snake_case")]
fn player_normalize(st: State<'_, AppState>, on: bool) -> Res<()> {
    st.player()?.set_normalize(on)?;
    st.cfg.lock().unwrap().normalize = on;
    st.save()
}

#[tauri::command(rename_all = "snake_case")]
fn player_stop(st: State<'_, AppState>) -> Res<()> {
    st.player()?.stop()
}

// ───────────────────────── ВКонтакте: только прослушивание ─────────────────────────

/// Сессия ВК из хранилища WebKit (общего для всех webview приложения). Как в NK-Player: вход есть, если
/// у vk.ru / vk.com есть кука remixsid или remixnsid. Вызывать не из главного потока.
fn vk_cookie(app: &AppHandle) -> Res<Option<vk::Session>> {
    let all = all_cookies(app)?;
    let Some(base) = vk::session_base(all.iter().map(|c| (c.name.as_str(), c.value.as_str(), c.domain.as_str()))) else {
        return Ok(None);
    };
    Ok(Some(vk::Session { base, cookies: vk::vk_cookies(all) }))
}

async fn vk_session(app: &AppHandle) -> Res<vk::Session> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || vk_cookie(&app))
        .await
        .map_err(|e| e.to_string())??
        .ok_or_else(|| "Вы не вошли во ВКонтакте".to_string())
}

#[derive(Serialize)]
struct VkStatus {
    logged_in: bool,
    profile: Option<vk::Profile>,
    /// вход есть, но профиль не получен (нет сети, ограничение ВК)
    error: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
async fn vk_status(app: AppHandle, st: State<'_, AppState>) -> Res<VkStatus> {
    let Ok(session) = vk_session(&app).await else {
        st.vk.forget();
        return Ok(VkStatus { logged_in: false, profile: None, error: None });
    };
    match st.vk.profile(&session).await {
        Ok(p) => Ok(VkStatus { logged_in: true, profile: Some(p), error: None }),
        Err(e) => Ok(VkStatus { logged_in: true, profile: None, error: Some(e) }),
    }
}

// ───────────────────────── вход в сервисы (ВКонтакте, SoundCloud) ─────────────────────────

/// Высота полосы интерфейса над страницей входа (подсказка и «Отмена»), как в NK-Player.
const LOGIN_BAR: i32 = 56;
/// webview страницы входа (одна на все сервисы)
const LOGIN_LABEL: &str = "svc-login";

/// Страница входа внутри главного окна (как WebContentsView в NK-Player). В GTK оба webview лежат в
/// вертикальном боксе окна; на время входа переносим их в `gtk::Paned`: сверху полоса интерфейса
/// высотой `LOGIN_BAR` (подсказка и «Отмена»), снизу страница сервиса. После входа интерфейс возвращается в бокс.
/// (Упаковкой бокса не обойтись: GTK отдаёт WebKitWebView его естественную высоту — всё окно.)
fn login_layout(app: &AppHandle, login: bool) {
    let Some(w) = app.get_window("main") else { return };
    let _ = app.run_on_main_thread(move || {
        use gtk::prelude::*;
        let Ok(vbox) = w.default_vbox() else { return };
        let paned = vbox.children().into_iter().find_map(|c| c.downcast::<gtk::Paned>().ok());
        if login {
            if paned.is_some() {
                return;
            }
            let kids = vbox.children();
            let (Some(ui), Some(page)) = (kids.first(), kids.get(1)) else { return };
            let p = gtk::Paned::new(gtk::Orientation::Vertical);
            vbox.remove(ui);
            vbox.remove(page);
            p.pack1(ui, false, true);
            p.pack2(page, true, false);
            p.set_position(LOGIN_BAR);
            vbox.pack_start(&p, true, true, 0);
            p.show_all();
        } else if let Some(p) = paned {
            // все webview — обратно в бокс; страницу входа Tauri закроет следом
            let kids = p.children();
            for k in &kids {
                p.remove(k);
            }
            vbox.remove(&p);
            for (i, k) in kids.iter().enumerate() {
                vbox.pack_start(k, i == 0, true, 0);
            }
        }
    });
}

fn login_close(app: &AppHandle) {
    if let Some(v) = app.get_webview(LOGIN_LABEL) {
        let _ = v.close();
    }
    login_layout(app, false);
}

/// Хранилище кук WebKit (общее для всех webview приложения): `f` выполняется в главном потоке и
/// отвечает через канал. Сам вызов — не из главного потока (иначе ждали бы сами себя).
/// get_webview, а не get_webview_window: пока открыт вход, в окне main два webview.
/// Не `Webview::cookies` из wry: он отдаёт домен без ведущей точки («.vk.ru» → «vk.ru»), и кука сайта
/// становится кукой одного хоста — не уходит на login.vk.ru, где ВК после перезапуска обновляет сессию
/// (было: «Сессия ВКонтакте не распознана» до «Выйти»); и удалить такую куку через wry нельзя.
fn with_cookie_manager<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(webkit2gtk::CookieManager, std::sync::mpsc::Sender<Res<T>>) + Send + 'static,
) -> Res<T> {
    let w = app.get_webview("main").ok_or("Нет главного окна")?;
    let (tx, rx) = std::sync::mpsc::channel();
    let fail = tx.clone();
    w.with_webview(move |pw| {
        use webkit2gtk::{WebViewExt, WebsiteDataManagerExt};
        match pw.inner().website_data_manager().and_then(|m| m.cookie_manager()) {
            Some(cm) => f(cm, tx),
            None => {
                let _ = fail.send(Err("Куки недоступны".into()));
            }
        }
    })
    .map_err(|e| format!("Куки недоступны: {e}"))?;
    rx.recv_timeout(std::time::Duration::from_secs(10)).map_err(|_| "Куки недоступны: нет ответа WebKit".to_string())?
}

/// webkit_cookie_manager_get_all_cookies: в webkit2gtk 2.0 обёртки нет, у wry она закрытая — та же привязка.
mod wk {
    use gtk::gio;
    use gtk::glib::{self, translate::*};
    use webkit2gtk::ffi::WebKitCookieManager;

    unsafe extern "C" {
        fn webkit_cookie_manager_get_all_cookies(
            cm: *mut WebKitCookieManager,
            cancellable: *mut gio::ffi::GCancellable,
            callback: gio::ffi::GAsyncReadyCallback,
            data: glib::ffi::gpointer,
        );
        fn webkit_cookie_manager_get_all_cookies_finish(
            cm: *mut WebKitCookieManager,
            res: *mut gio::ffi::GAsyncResult,
            err: *mut *mut glib::ffi::GError,
        ) -> *mut glib::ffi::GList;
    }

    type Done = Box<dyn FnOnce(Result<Vec<soup::Cookie>, glib::Error>)>;

    unsafe extern "C" fn finish(src: *mut glib::gobject_ffi::GObject, res: *mut gio::ffi::GAsyncResult, data: glib::ffi::gpointer) {
        unsafe {
            let mut err = std::ptr::null_mut();
            let list = webkit_cookie_manager_get_all_cookies_finish(src as *mut _, res, &mut err);
            let r = if err.is_null() { Ok(FromGlibPtrContainer::from_glib_full(list)) } else { Err(from_glib_full(err)) };
            let done: Box<Done> = Box::from_raw(data as *mut Done);
            done(r);
        }
    }

    /// Все куки хранилища. Только из главного потока (колбэк — там же).
    pub fn all_cookies(cm: &webkit2gtk::CookieManager, done: impl FnOnce(Result<Vec<soup::Cookie>, glib::Error>) + 'static) {
        let data: Box<Done> = Box::new(Box::new(done));
        unsafe {
            webkit_cookie_manager_get_all_cookies(
                cm.to_glib_none().0,
                std::ptr::null_mut(),
                Some(finish),
                Box::into_raw(data) as glib::ffi::gpointer,
            );
        }
    }
}

fn plain_cookie(c: &mut soup::Cookie) -> vk::Cookie {
    vk::Cookie {
        name: c.name().map(|s| s.to_string()).unwrap_or_default(),
        value: c.value().map(|s| s.to_string()).unwrap_or_default(),
        domain: c.domain().map(|s| s.to_string()).unwrap_or_default(),
        path: c.path().map(|s| s.to_string()).unwrap_or_else(|| "/".into()),
    }
}

/// Все куки WebKit с настоящими доменами (с точкой — кука сайта и его поддоменов).
fn all_cookies(app: &AppHandle) -> Res<Vec<vk::Cookie>> {
    with_cookie_manager(app, |cm, tx| {
        wk::all_cookies(&cm, move |r| {
            let r = r.map(|mut v| v.iter_mut().map(plain_cookie).collect()).map_err(|e| format!("Куки недоступны: {e}"));
            let _ = tx.send(r);
        });
    })
}

/// Удалить куки, домен которых подходит под `own` (выход из сервиса).
fn delete_cookies(app: &AppHandle, own: fn(&str) -> bool) -> Res<()> {
    with_cookie_manager(app, move |cm, tx| {
        use webkit2gtk::CookieManagerExt;
        let mgr = cm.clone();
        wk::all_cookies(&cm, move |r| {
            let mut all = match r {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(format!("Куки недоступны: {e}")));
                    return;
                }
            };
            all.retain_mut(|c| c.domain().is_some_and(|d| own(d.as_str())));
            let left = std::rc::Rc::new(std::cell::Cell::new(all.len()));
            if all.is_empty() {
                let _ = tx.send(Ok(()));
                return;
            }
            for mut c in all {
                let (left, tx) = (left.clone(), tx.clone());
                mgr.delete_cookie(&mut c, None::<&gtk::gio::Cancellable>, move |r| {
                    if let Err(e) = r {
                        let _ = tx.send(Err(format!("Не удалось удалить куки: {e}")));
                    }
                    left.set(left.get() - 1);
                    if left.get() == 0 {
                        let _ = tx.send(Ok(()));
                    }
                });
            }
        });
    })
}

/// Вход выполнен, если у сервиса есть кука сессии.
fn logged_in(app: &AppHandle, service: &str) -> bool {
    match service {
        "vk" => matches!(vk_cookie(app), Ok(Some(_))),
        "sc" => matches!(sc_token(app), Ok(Some(_))),
        _ => false,
    }
}

/// Обычная страница входа сервиса (ВК — можно по QR-коду) прямо в главном окне.
/// Прав на команды у неё нет (capabilities — только main).
#[tauri::command(rename_all = "snake_case")]
async fn svc_login(app: AppHandle, service: String) -> Res<()> {
    let (url, title) = match service.as_str() {
        "vk" => (vk::LOGIN_URL, "ВКонтакте"),
        "sc" => (sc::LOGIN_URL, "SoundCloud"),
        _ => return Err("Неизвестный сервис".into()),
    };
    if app.get_webview(LOGIN_LABEL).is_some() {
        return Ok(());
    }
    let main = app.get_window("main").ok_or("Нет главного окна")?;
    let url = tauri::Url::parse(url).map_err(|e| e.to_string())?;
    let popups = app.clone();
    let seq = AtomicU64::new(0);
    let builder = tauri::webview::WebviewBuilder::new(LOGIN_LABEL, tauri::WebviewUrl::External(url))
        // как в NK-Player (Electron): со своим UA WebKitGTK страница vk.ru/login не догружается
        .user_agent(vk::USER_AGENT)
        // всплывающие окна (VK ID, вход через Google/Apple) открываются отдельными окнами
        .on_new_window(move |url, features| {
            let label = format!("svc-popup-{}", seq.fetch_add(1, Ordering::Relaxed));
            match tauri::WebviewWindowBuilder::new(&popups, label, tauri::WebviewUrl::External(url))
                .window_features(features)
                .title(title)
                .user_agent(vk::USER_AGENT)
                .build()
            {
                Ok(window) => tauri::webview::NewWindowResponse::Create { window },
                Err(_) => tauri::webview::NewWindowResponse::Deny,
            }
        });
    let size = main.inner_size().map_err(|e| e.to_string())?;
    let scale = main.scale_factor().unwrap_or(1.0);
    let size = size.to_logical::<f64>(scale);
    main.add_child(
        builder,
        tauri::LogicalPosition::new(0.0, LOGIN_BAR as f64),
        tauri::LogicalSize::new(size.width, (size.height - LOGIN_BAR as f64).max(100.0)),
    )
    .map_err(|e| format!("Не удалось открыть страницу входа: {e}"))?;
    login_layout(&app, true);
    let st = app.state::<AppState>();
    st.vk.forget();
    st.sc.forget();

    // как в NK-Player: опрашиваем куки, пока не появится сессия, затем убираем страницу входа
    let h = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        if h.get_webview(LOGIN_LABEL).is_none() {
            let _ = h.emit("svc://event", serde_json::json!({ "type": "login_closed", "service": service }));
            break;
        }
        if logged_in(&h, &service) {
            if service == "vk" {
                if let Ok(Some(session)) = vk_cookie(&h) {
                    // после входа ВК доводит сессию редиректами login.php?act=slogin, выставляя куки;
                    // даём странице входа пройти их, как браузеру, — тогда полные куки сохранятся в WebKit
                    vk_login_settle(&h, session.base);
                }
            }
            login_close(&h);
            let _ = h.emit("svc://event", serde_json::json!({ "type": "login", "service": service }));
            break;
        }
    });
    Ok(())
}

/// Открыть /audio на странице входа и дождаться, пока она туда дойдёт (до 15 с).
fn vk_login_settle(app: &AppHandle, base: &str) {
    let Some(v) = app.get_webview(LOGIN_LABEL) else { return };
    let Ok(target) = tauri::Url::parse(&format!("{base}/audio")) else { return };
    if v.navigate(target.clone()).is_err() {
        return;
    }
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(15) {
        std::thread::sleep(std::time::Duration::from_millis(500));
        match v.url() {
            Ok(u) if u.host_str() == target.host_str() && u.path() == "/audio" => {
                // страница загружена — даём ей выставить куки
                std::thread::sleep(std::time::Duration::from_millis(1500));
                return;
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
fn svc_login_cancel(app: AppHandle) {
    login_close(&app);
}

/// Выход: удалить куки сервиса из хранилища WebKit.
#[tauri::command(rename_all = "snake_case")]
async fn svc_logout(app: AppHandle, st: State<'_, AppState>, service: String) -> Res<()> {
    let own: fn(&str) -> bool = match service.as_str() {
        "vk" => vk::is_vk_domain,
        "sc" => sc::is_sc_domain,
        _ => return Err("Неизвестный сервис".into()),
    };
    st.vk.forget();
    st.sc.forget();
    // синхронизированные треки и ссылки на потоки уходят вместе с сессией
    st.saved.clear_synced(&service)?;
    prune_order(&st, &service)?;
    st.streams.clear_prefix(&format!("{service}:"));
    if service == "vk" {
        st.streams.clear_prefix("vkapi:");
        st.playlists.clear();
    }
    let h = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Res<()> {
        login_close(&h);
        delete_cookies(&h, own)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "snake_case")]
async fn vk_search(app: AppHandle, st: State<'_, AppState>, query: String, more: Option<vk::More>) -> Res<vk::Page> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err("Введите запрос до 200 символов".into());
    }
    if more.as_ref().is_some_and(|m| m.section_id.len() > 200 || m.start_from.len() > 200) {
        return Err("Неверное продолжение поиска".into());
    }
    let session = vk_session(&app).await?;
    st.vk.search(&session, query, more).await
}

/// Играть трек ВК: ссылку получает Rust (одну, по нажатию), интерфейс её не видит.
/// Ссылка уже прослушанного трека берётся из кэша; fresh — не брать из кэша (повтор, если не открылась).
#[tauri::command(rename_all = "snake_case")]
async fn vk_play(app: AppHandle, st: State<'_, AppState>, reload: String, title: Option<String>, fresh: Option<bool>) -> Res<bool> {
    let p = st.player()?;
    st.cache.begin();
    let key = format!("vk:{reload}");
    let title = title.unwrap_or_default();
    // трек библиотеки играет из кэша; не открылся (fresh) — кэш выбрасываем
    let saved = st.saved.list().into_iter().find(|s| s.service == "vk" && s.reload.as_deref() == Some(reload.as_str()));
    let ckey = saved.as_ref().map(|s| cache::service_key("vk", &s.id));
    if let Some(k) = &ckey {
        if fresh == Some(true) {
            st.cache.remove(k);
        } else if let Some(c) = st.cache.get(k) {
            return p.load(&c.to_string_lossy(), &title).map(|_| false);
        }
    }
    let s = match st.streams.get(&key) {
        Some(s) if fresh != Some(true) => s,
        _ => {
            st.streams.forget(&key);
            vk_stream(&app, &st, &reload).await?
        }
    };
    p.load_stream(&s.url, &title, vk::USER_AGENT, &s.referrer)?;
    if let (Some(k), Some(t)) = (ckey, saved) {
        st.cache.enqueue(cache_job(k, &s, &t));
        return Ok(true);
    }
    Ok(false)
}

/// Новая ссылка на поток ВК (запрос к сервису) — сразу в кэш ссылок.
async fn vk_stream(app: &AppHandle, st: &AppState, reload: &str) -> Res<streams::Stream> {
    let session = vk_session(app).await?;
    let url = st.vk.stream_url(&session, reload).await?;
    let s = streams::Stream { url, referrer: session.referrer() };
    st.streams.put(&format!("vk:{reload}"), s.clone());
    Ok(s)
}

fn cache_job(key: String, s: &streams::Stream, t: &saved::Saved) -> cache::Job {
    cache::Job {
        key,
        lib: saved_key(t),
        source: cache::Source::Stream { url: s.url.clone(), referrer: s.referrer.clone() },
        artist: t.artist.clone(),
        title: t.title.clone(),
    }
}

// ───────────────────────── ВК: VK Микс и плейлисты (API с web-токеном сайта) ─────────────────────────

/// Ссылки на потоки треков из API — в кэш ссылок (`vkapi:<id>`), интерфейсу — только описания треков.
fn keep_streams(st: &AppState, s: &vk::Session, tracks: Vec<(vk::Track, Option<String>)>) -> Vec<vk::Track> {
    tracks
        .into_iter()
        .map(|(t, url)| {
            if let Some(url) = url {
                st.streams.put(&format!("vkapi:{}", t.id), streams::Stream { url, referrer: s.referrer() });
            }
            t
        })
        .collect()
}

/// Раздел «Миксы»: VK Микс и подборки ВК с главной музыки.
#[tauri::command(rename_all = "snake_case")]
async fn vk_home(app: AppHandle, st: State<'_, AppState>) -> Res<vk::Home> {
    let session = vk_session(&app).await?;
    st.vk.home(&session).await
}

/// Ваши плейлисты ВК — в библиотеке над треками (при открытии библиотеки и по кнопке обновления).
/// Ответ сохраняется на диск (`playlists.json`) вместе с access_key плейлистов.
#[tauri::command(rename_all = "snake_case")]
async fn vk_my_playlists(app: AppHandle, st: State<'_, AppState>) -> Res<Vec<vk::ApiPlaylist>> {
    let session = vk_session(&app).await?;
    let list = st.vk.my_playlists(&session).await?;
    let keys: Vec<(String, String)> = list
        .iter()
        .filter_map(|pl| {
            let k = format!("pl:{}_{}", pl.owner_id, pl.id);
            st.vk.access_key(&k).map(|v| (k, v))
        })
        .collect();
    st.playlists.set_list(&list, keys);
    Ok(list)
}

/// Сохранённый список плейлистов — сразу, без сети; null — ещё не загружали.
#[tauri::command(rename_all = "snake_case")]
fn vk_my_playlists_cached(st: State<'_, AppState>) -> Option<Vec<vk::ApiPlaylist>> {
    st.playlists.list()
}

/// Треки плейлиста ВК (страница из 200). Ваши плейлисты сохраняются на диск.
#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, offset: Option<u64>) -> Res<vk::Page> {
    let session = vk_session(&app).await?;
    let offset = offset.unwrap_or(0).min(100_000);
    let (tracks, next) = st.vk.playlist(&session, owner_id, id, offset).await?;
    let tracks = keep_streams(&st, &session, tracks);
    let keys: Vec<(String, String)> = tracks.iter().filter_map(|t| st.vk.access_key(&t.id).map(|k| (t.id.clone(), k))).collect();
    st.playlists.set_tracks(owner_id, id, offset, &tracks, next, keys);
    Ok(vk::Page { tracks, next_offset: next, more: None })
}

/// Сохранённые треки вашего плейлиста — сразу, без сети; null — ещё не открывали.
#[tauri::command(rename_all = "snake_case")]
fn vk_playlist_cached(st: State<'_, AppState>, owner_id: i64, id: i64) -> Option<vk::Page> {
    st.playlists.tracks(owner_id, id).map(|s| vk::Page { tracks: s.tracks, next_offset: s.next, more: None })
}

/// Следующие треки микса (по включению микса и когда очередь микса подходит к концу).
#[tauri::command(rename_all = "snake_case")]
async fn vk_mix(app: AppHandle, st: State<'_, AppState>, mix_id: String) -> Res<Vec<vk::Track>> {
    if mix_id.is_empty() || mix_id.len() > 40 || !mix_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("Неизвестный микс".into());
    }
    let session = vk_session(&app).await?;
    let tracks = st.vk.mix(&session, &mix_id).await?;
    Ok(keep_streams(&st, &session, tracks))
}

// ── свои плейлисты ВК: создать, переименовать, удалить, треки в плейлист и из него (просьба владельца, 2026-11-02) ──

/// id трека ВК «owner_id_id».
fn valid_vk_id(id: &str) -> bool {
    let mut it = id.splitn(2, '_');
    id.len() <= 40
        && it.next().is_some_and(|o| o.trim_start_matches('-').parse::<u64>().is_ok())
        && it.next().is_some_and(|i| i.parse::<u64>().is_ok())
}

/// Название плейлиста: без управляющих символов, 1–128 знаков.
fn playlist_title(t: &str) -> Res<String> {
    let t: String = t.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if t.is_empty() || t.chars().count() > 128 {
        return Err("Название плейлиста — от 1 до 128 символов".into());
    }
    Ok(t)
}

#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist_create(app: AppHandle, st: State<'_, AppState>, title: String) -> Res<vk::ApiPlaylist> {
    let title = playlist_title(&title)?;
    let session = vk_session(&app).await?;
    let pl = st.vk.playlist_create(&session, &title).await?;
    st.playlists.added(&pl);
    Ok(pl)
}

#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist_rename(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, title: String) -> Res<()> {
    let title = playlist_title(&title)?;
    let session = vk_session(&app).await?;
    st.vk.playlist_rename(&session, owner_id, id, &title).await?;
    st.playlists.renamed(owner_id, id, &title);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist_delete(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64) -> Res<()> {
    let session = vk_session(&app).await?;
    st.vk.playlist_delete(&session, owner_id, id).await?;
    st.playlists.removed(owner_id, id);
    Ok(())
}

/// Трек ВК (из библиотеки, поиска, микса, плейлиста) — в свой плейлист.
#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist_add(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, track_id: String) -> Res<()> {
    if !valid_vk_id(&track_id) {
        return Err("Неверный id трека ВК".into());
    }
    // access_key трека из библиотеки — если в памяти его уже нет (после перезапуска)
    let key = st.saved("vk", &track_id).and_then(|s| s.access_key);
    let session = vk_session(&app).await?;
    st.vk.playlist_add(&session, owner_id, id, &track_id, key.as_deref()).await?;
    st.playlists.track_added(owner_id, id);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist_remove(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, track_id: String) -> Res<()> {
    if !valid_vk_id(&track_id) {
        return Err("Неверный id трека ВК".into());
    }
    let session = vk_session(&app).await?;
    st.vk.playlist_remove(&session, owner_id, id, &track_id).await?;
    st.playlists.track_removed(owner_id, id, &track_id);
    Ok(())
}

/// Чужой плейлист (подборка ВК, плейлист друга) — в свои.
#[tauri::command(rename_all = "snake_case")]
async fn vk_playlist_follow(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64) -> Res<()> {
    let session = vk_session(&app).await?;
    st.vk.playlist_follow(&session, owner_id, id).await
}

/// Играть трек из микса или плейлиста: ссылка из ответа API (в кэше ссылок), истекла — новая через audio.getById.
/// Такой трек, добавленный в библиотеку, играет из кэша треков и кэшируется, как остальные (вернёт true).
#[tauri::command(rename_all = "snake_case")]
async fn vk_play_api(app: AppHandle, st: State<'_, AppState>, id: String, title: Option<String>, fresh: Option<bool>) -> Res<bool> {
    let p = st.player()?;
    st.cache.begin();
    if !valid_vk_id(&id) {
        return Err("Неверный id трека ВК".into());
    }
    let title = title.unwrap_or_default();
    let saved = st.saved("vk", &id).filter(|s| s.api);
    let ckey = saved.as_ref().map(|s| cache::service_key("vk", &s.id));
    if let Some(k) = &ckey {
        if fresh == Some(true) {
            st.cache.remove(k);
        } else if let Some(c) = st.cache.get(k) {
            return p.load(&c.to_string_lossy(), &title).map(|_| false);
        }
    }
    let key = format!("vkapi:{id}");
    let s = match st.streams.get(&key) {
        Some(s) if fresh != Some(true) => s,
        _ => {
            st.streams.forget(&key);
            vk_api_stream(&app, &st, &id, saved.as_ref().and_then(|s| s.access_key.as_deref())).await?
        }
    };
    p.load_stream(&s.url, &title, vk::USER_AGENT, &s.referrer)?;
    if let (Some(k), Some(t)) = (ckey, saved) {
        st.cache.enqueue(cache_job(k, &s, &t));
        return Ok(true);
    }
    Ok(false)
}

/// Новая ссылка на трек из API (audio.getById) — сразу в кэш ссылок.
async fn vk_api_stream(app: &AppHandle, st: &AppState, id: &str, access_key: Option<&str>) -> Res<streams::Stream> {
    let session = vk_session(app).await?;
    let url = st.vk.api_stream(&session, id, access_key).await?;
    let s = streams::Stream { url, referrer: session.referrer() };
    st.streams.put(&format!("vkapi:{id}"), s.clone());
    Ok(s)
}

// ───────────────────────── SoundCloud ─────────────────────────

/// Токен сессии SoundCloud (кука oauth_token) из WebKit. Вызывать не из главного потока.
fn sc_token(app: &AppHandle) -> Res<Option<String>> {
    let all = all_cookies(app)?;
    Ok(sc::token_from(all.iter().map(|c| (c.name.as_str(), c.value.as_str(), c.domain.as_str()))))
}

/// Токен, если вход выполнен (поиск и прослушивание работают и без входа).
async fn sc_session(app: &AppHandle) -> Option<String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || sc_token(&app).ok().flatten()).await.ok().flatten()
}

#[derive(Serialize)]
struct ScStatus {
    logged_in: bool,
    profile: Option<sc::Profile>,
    error: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
async fn sc_status(app: AppHandle, st: State<'_, AppState>) -> Res<ScStatus> {
    let Some(token) = sc_session(&app).await else {
        st.sc.forget();
        return Ok(ScStatus { logged_in: false, profile: None, error: None });
    };
    match st.sc.profile(&token).await {
        Ok(p) => Ok(ScStatus { logged_in: true, profile: Some(p), error: None }),
        Err(e) => Ok(ScStatus { logged_in: true, profile: None, error: Some(e) }),
    }
}

#[tauri::command(rename_all = "snake_case")]
async fn sc_search(app: AppHandle, st: State<'_, AppState>, query: String, offset: Option<u64>) -> Res<sc::Page> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err("Введите запрос до 200 символов".into());
    }
    let token = sc_session(&app).await;
    st.sc.search(token.as_deref(), query, offset.unwrap_or(0).min(10_000)).await
}

/// Ссылка на трек/плейлист SoundCloud → треки.
#[tauri::command(rename_all = "snake_case")]
async fn sc_resolve(app: AppHandle, st: State<'_, AppState>, url: String) -> Res<Vec<sc::Track>> {
    if url.len() > 2048 {
        return Err("Слишком длинная ссылка".into());
    }
    let token = sc_session(&app).await;
    st.sc.resolve(token.as_deref(), &url).await
}

/// Играть трек SoundCloud: ссылку на поток получает Rust (по нажатию), интерфейс её не видит.
#[tauri::command(rename_all = "snake_case")]
async fn sc_play(app: AppHandle, st: State<'_, AppState>, id: String, title: Option<String>, fresh: Option<bool>) -> Res<bool> {
    let p = st.player()?;
    st.cache.begin();
    let key = format!("sc:{id}");
    let title = title.unwrap_or_default();
    let saved = st.saved("sc", &id);
    let ckey = cache::service_key("sc", &id);
    if saved.is_some() {
        if fresh == Some(true) {
            st.cache.remove(&ckey);
        } else if let Some(c) = st.cache.get(&ckey) {
            return p.load(&c.to_string_lossy(), &title).map(|_| false);
        }
    }
    let s = match st.streams.get(&key) {
        Some(s) if fresh != Some(true) => s,
        _ => {
            st.streams.forget(&key);
            sc_stream(&app, &st, &id).await?
        }
    };
    p.load_stream(&s.url, &title, vk::USER_AGENT, &s.referrer)?;
    if let Some(t) = saved {
        st.cache.enqueue(cache_job(ckey, &s, &t));
        return Ok(true);
    }
    Ok(false)
}

/// Новая ссылка на поток SoundCloud (запрос к сервису) — сразу в кэш ссылок.
async fn sc_stream(app: &AppHandle, st: &AppState, id: &str) -> Res<streams::Stream> {
    const REFERRER: &str = "https://soundcloud.com/";
    let token = sc_session(app).await;
    let url = st.sc.stream_url(token.as_deref(), id).await?;
    let s = streams::Stream { url, referrer: REFERRER.into() };
    st.streams.put(&format!("sc:{id}"), s.clone());
    Ok(s)
}

// ───────────────────────── сохранённые треки сервисов ─────────────────────────

#[tauri::command(rename_all = "snake_case")]
fn saved_list(st: State<'_, AppState>) -> Vec<saved::Saved> {
    st.saved.list()
}

#[tauri::command(rename_all = "snake_case")]
fn saved_add(st: State<'_, AppState>, mut track: saved::Saved) -> Res<saved::Saved> {
    // access_key трека из микса/плейлиста — из ответа API (в памяти Rust), не из интерфейса
    track.access_key = if track.service == "vk" && track.api { st.vk.access_key(&track.id) } else { None };
    let s = st.saved.add(track)?;
    st.order.seen([saved_key(&s)])?;
    Ok(s)
}

/// Синхронизация с аккаунтом: «Мои треки» ВК / лайки SoundCloud → библиотека. Без входа — ошибка
/// «не вошли» (без сетевых запросов). Возвращает библиотеку целиком.
#[tauri::command(rename_all = "snake_case")]
async fn saved_sync(app: AppHandle, st: State<'_, AppState>, service: String) -> Res<Vec<saved::Saved>> {
    const MAX: usize = 10_000;
    let fresh: Vec<saved::Saved> = match service.as_str() {
        "vk" => {
            let session = vk_session(&app).await?;
            let mut tracks = Vec::new();
            let mut offset = 0;
            for _ in 0..100 {
                let page = st.vk.my_tracks(&session, offset).await?;
                tracks.extend(page.tracks);
                match page.next_offset {
                    Some(n) if tracks.len() < MAX => offset = n,
                    _ => break,
                }
            }
            tracks
                .into_iter()
                .filter(|t| t.reload.is_some())
                .map(|t| saved::Saved {
                    service: "vk".into(),
                    id: t.id,
                    reload: t.reload,
                    url: None,
                    artist: t.artist,
                    title: t.title,
                    duration: t.duration,
                    cover: t.cover,
                    added: 0,
                    sync: true,
                    del: t.del,
                    api: false,
                    access_key: None,
                })
                .collect()
        }
        "sc" => {
            let token = sc_session(&app).await.ok_or("Вы не вошли в SoundCloud")?;
            st.sc
                .likes(&token, MAX)
                .await?
                .into_iter()
                .filter(|t| !t.blocked)
                .map(|t| saved::Saved {
                    service: "sc".into(),
                    id: t.id,
                    reload: None,
                    url: t.url,
                    artist: t.artist,
                    title: t.title,
                    duration: t.duration,
                    cover: t.cover,
                    added: 0,
                    sync: true,
                    del: None,
                    api: false,
                    access_key: None,
                })
                .collect()
        }
        _ => return Err("Неизвестный сервис".into()),
    };
    // ключи в порядке сервиса (новые сначала) — новые для библиотеки встанут первыми
    let keys: Vec<String> = fresh.iter().map(saved_key).collect();
    st.saved.replace_synced(&service, fresh)?;
    st.order.seen(keys)?;
    prune_order(&st, &service)?;
    Ok(st.saved.list())
}

/// Удалить трек из «Моих треков» ВК (и из библиотеки). Только ваши синхронизированные аудиозаписи —
/// у них есть хэш удаления; id и хэш берутся из библиотеки, а не из интерфейса.
#[tauri::command(rename_all = "snake_case")]
async fn vk_delete(app: AppHandle, st: State<'_, AppState>, id: String) -> Res<()> {
    let item = st
        .saved
        .list()
        .into_iter()
        .find(|s| s.service == "vk" && s.id == id && s.sync)
        .ok_or("Этого трека нет среди ваших аудиозаписей")?;
    let hash = item.del.ok_or("ВКонтакте не разрешает удалить этот трек")?;
    let session = vk_session(&app).await?;
    st.vk.delete(&session, &id, &hash).await?;
    st.saved.remove("vk", &id)?;
    st.cache.remove(&cache::service_key("vk", &id));
    let key = format!("vk:{id}");
    st.order.retain(|k| k != key)?;
    if let Some(r) = item.reload {
        st.streams.forget(&format!("vk:{r}"));
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
fn saved_remove(st: State<'_, AppState>, service: String, id: String) -> Res<()> {
    st.saved.remove(&service, &id)?;
    st.cache.remove(&cache::service_key(&service, &id));
    let key = format!("{service}:{id}");
    st.order.retain(|k| k != key)
}

// ───────────────────────── локальные треки / yt-dlp ─────────────────────────

#[tauri::command(rename_all = "snake_case")]
fn dl_start(app: AppHandle, st: State<'_, AppState>, input: String) -> Res<u64> {
    let id = st.dl_seq.fetch_add(1, Ordering::Relaxed) + 1;
    let req = ytdlp::DlRequest { input };
    let dir = st.music_dir();
    let h = app.clone();
    ytdlp::spawn(id, req, dir, move |ev| {
        let _ = h.emit("dl://event", &ev);
    })?;
    Ok(id)
}

/// Что установлено для загрузок: yt-dlp, ffmpeg, python-mutagen (обложки).
#[tauri::command(rename_all = "snake_case")]
async fn dl_caps() -> Res<ytdlp::Caps> {
    tauri::async_runtime::spawn_blocking(|| ytdlp::caps().clone())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
async fn local_list(st: State<'_, AppState>) -> Res<Vec<library::LocalTrack>> {
    let dir = st.music_dir();
    let scan_dir = dir.clone();
    let tracks = tauri::async_runtime::spawn_blocking(move || library::scan(&scan_dir))
        .await
        .map_err(|e| e.to_string())?;
    if !st.order.initialized() {
        // первый запуск с порядком: собираем его из прежних дат (файлы — по времени изменения)
        let mut all: Vec<(String, u64)> = tracks.iter().map(|t| (t.path.clone(), t.added)).collect();
        all.extend(st.saved.list().into_iter().map(|s| (saved_key(&s), s.added)));
        all.sort_by(|a, b| b.1.cmp(&a.1));
        st.order.init(all.into_iter().map(|x| x.0).collect())?;
    } else {
        // новые файлы — первыми (среди себя — по времени изменения, scan уже так отсортирован)
        st.order.seen(tracks.iter().map(|t| t.path.clone()))?;
        // удалённые файлы — из порядка; если папки нет (диск не подключён), порядок не трогаем
        if dir.is_dir() {
            let present: std::collections::HashSet<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
            st.order.retain(|k| !k.starts_with('/') || present.contains(k))?;
            // кэш — только для файлов, которые есть сейчас и не менялись
            let cache = st.cache.clone();
            let paths: Vec<String> = present.iter().map(|p| p.to_string()).collect();
            tauri::async_runtime::spawn_blocking(move || {
                let keys: std::collections::HashSet<String> =
                    paths.iter().filter_map(|p| cache::local_key(Path::new(p))).collect();
                cache.retain(|k| !k.starts_with("local-") || keys.contains(k));
            });
        }
    }
    Ok(tracks)
}

fn saved_key(s: &saved::Saved) -> String {
    format!("{}:{}", s.service, s.id)
}

/// Убрать из порядка и из кэша треки сервиса, которых больше нет в библиотеке.
fn prune_order(st: &AppState, service: &str) -> Res<()> {
    let list = st.saved.list();
    let cached: std::collections::HashSet<String> = list.iter().map(|s| cache::service_key(&s.service, &s.id)).collect();
    let cprefix = format!("{service}-");
    st.cache.retain(|k| !k.starts_with(&cprefix) || cached.contains(k));
    let present: std::collections::HashSet<String> = list.iter().map(saved_key).collect();
    let prefix = format!("{service}:");
    st.order.retain(|k| !k.starts_with(&prefix) || present.contains(k))
}

/// Порядок треков библиотеки: ключи (путь к файлу или `vk:`/`sc:` + id), новые сначала.
#[tauri::command(rename_all = "snake_case")]
fn library_order(st: State<'_, AppState>) -> Vec<String> {
    st.order.list()
}

/// Перетаскивание трека в библиотеке: key встаёт перед before (None — в конец).
#[tauri::command(rename_all = "snake_case")]
fn library_move(st: State<'_, AppState>, key: String, before: Option<String>) -> Res<()> {
    let ok = |k: &str| !k.is_empty() && k.len() <= 4096 && !k.chars().any(char::is_control);
    if !ok(&key) || before.as_deref().is_some_and(|b| !ok(b)) {
        return Err("Неверный трек".into());
    }
    st.order.move_before(&key, before.as_deref())
}

#[tauri::command(rename_all = "snake_case")]
fn local_delete(st: State<'_, AppState>, path: String) -> Res<()> {
    let dir = st.music_dir();
    let p = Path::new(&path);
    if !library::inside(&dir, p) {
        return Err("Файл вне папки музыки".into());
    }
    let ckey = cache::local_key(p);
    std::fs::remove_file(p).map_err(|e| format!("Не удалось удалить: {e}"))?;
    if let Some(k) = ckey {
        st.cache.remove(&k);
    }
    st.order.retain(|k| k != path)
}

#[tauri::command(rename_all = "snake_case")]
fn open_music_dir(st: State<'_, AppState>) -> Res<()> {
    let dir = st.music_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xdg-open: {e}"))
}

// ───────────────────────── тексты песен (Genius) ─────────────────────────

/// Текст песни для играющего трека: из кэша или поиск на Genius по исполнителю и названию.
#[tauri::command(rename_all = "snake_case")]
async fn lyrics_get(app: AppHandle, st: State<'_, AppState>, artist: String, title: String, vk_id: Option<String>) -> Res<lyrics::Lyrics> {
    if title.trim().is_empty() || artist.len() > 500 || title.len() > 500 {
        return Err("Нет названия трека".into());
    }
    if let Some(l) = st.genius.lyrics_cached(&artist, &title) {
        return Ok(l);
    }
    // у трека ВК — сначала текст самого ВК (нет входа или текста — дальше к Genius)
    if let Some(id) = vk_id.filter(|id| valid_vk_id(id)) {
        if let Ok(session) = vk_session(&app).await {
            if let Ok(Some((text, credits))) = st.vk.lyrics(&session, &id).await {
                let l = lyrics::Lyrics { status: "found".into(), text, source: "vk".into(), credits, meta: true, ..lyrics::Lyrics::empty() };
                st.genius.store_for(&artist, &title, &l);
                return Ok(l);
            }
        }
    }
    st.genius.lyrics(&artist, &title).await
}


/// Текст из кэша на диске (без сети) — показывается сразу при включении трека; нет — null.
#[tauri::command(rename_all = "snake_case")]
async fn lyrics_cached(st: State<'_, AppState>, artist: String, title: String) -> Res<Option<lyrics::Lyrics>> {
    if title.trim().is_empty() || artist.len() > 500 || title.len() > 500 {
        return Ok(None);
    }
    Ok(st.genius.lyrics_cached(&artist, &title))
}

/// Перевод из кэша на диске (без сети); нет — null.
#[tauri::command(rename_all = "snake_case")]
async fn lyrics_translation_cached(st: State<'_, AppState>, url: String) -> Res<Option<String>> {
    Ok(st.genius.translation_cached(&url))
}

/// Переводы и язык оригинала найденного текста — вторым шагом, после показа текста.
#[tauri::command(rename_all = "snake_case")]
async fn lyrics_meta(st: State<'_, AppState>, artist: String, title: String) -> Res<lyrics::Lyrics> {
    st.genius.meta(&artist, &title).await
}

/// Текст перевода песни: url — из `translations` найденного текста.
#[tauri::command(rename_all = "snake_case")]
async fn lyrics_translation(st: State<'_, AppState>, url: String) -> Res<String> {
    st.genius.translation(&url).await
}

/// Пояснение Genius к фрагменту текста (по нажатию на фрагмент).
#[tauri::command(rename_all = "snake_case")]
async fn lyrics_note(st: State<'_, AppState>, id: u64) -> Res<String> {
    if id == 0 {
        return Err("Нет пояснения".into());
    }
    st.genius.annotation(id).await
}

/// Открыть страницу песни на Genius в браузере — только адреса genius.com.
#[tauri::command(rename_all = "snake_case")]
fn lyrics_open(url: String) -> Res<()> {
    let ok = lyrics::genius_page(&url);
    if !ok {
        return Err("Недопустимая ссылка".into());
    }
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xdg-open: {e}"))
}

// ───────────────────────── статистика прослушивания ─────────────────────────

/// Статистика трека: сколько раз дослушан, сколько секунд слушали, когда, место в топе. key — ключ трека.
#[tauri::command(rename_all = "snake_case")]
fn stats_get(st: State<'_, AppState>, key: String) -> stats::TrackStat {
    st.stats.get(&key)
}

/// Добавить прослушанные секунды (порцию) и, если трек доигран до конца, один полный проигрыш.
/// day/hour — местные дата (ГГГГ-ММ-ДД) и час: по ним журнал по дням и часам.
#[tauri::command(rename_all = "snake_case")]
fn stats_add(st: State<'_, AppState>, key: String, seconds: f64, completed: bool, day: String, hour: u8) -> Res<stats::TrackStat> {
    st.stats.add(&key, seconds, completed, &day, hour)
}

/// Вся статистика — для страницы «Статистика».
#[tauri::command(rename_all = "snake_case")]
async fn stats_all(st: State<'_, AppState>) -> Res<stats::Data> {
    Ok(st.stats.all())
}

// ───────────────────────── хранилище: кэш треков и загрузки ─────────────────────────

/// Скачать трек библиотеки в папку загрузок (по умолчанию ~/Downloads). key — как в порядке библиотеки:
/// путь к файлу или `vk:`/`sc:` + id. Трек сервиса берётся из кэша, а если его там нет — сначала кэшируется.
/// Возвращает путь сохранённого файла.
#[tauri::command(rename_all = "snake_case")]
async fn track_download(app: AppHandle, st: State<'_, AppState>, key: String) -> Res<String> {
    let dest = st.cfg.lock().unwrap().download_dir();
    let (src, stem) = library_file(&app, &st, &key).await?;
    tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        let ext = src.extension().and_then(|x| x.to_str()).unwrap_or("mka").to_ascii_lowercase();
        std::fs::create_dir_all(&dest).map_err(|e| format!("Не удалось создать {}: {e}", dest.display()))?;
        let out = cache::free_path(&dest, &stem, &ext);
        std::fs::copy(&src, &out).map_err(|e| format!("Не удалось сохранить: {e}"))?;
        Ok(out.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Файл трека библиотеки по ключу (как в порядке библиотеки) и имя для него «Исполнитель - Название».
/// Файл из папки музыки — как есть; трек сервиса — из кэша, а нет там — выкачивается в кэш сейчас.
/// Данные трека сервиса берутся из library.json, не из интерфейса; треки вне библиотеки не отдаются.
async fn library_file(app: &AppHandle, st: &AppState, key: &str) -> Res<(PathBuf, String)> {
    if key.starts_with('/') {
        let src = PathBuf::from(key);
        if !library::inside(&st.music_dir(), &src) {
            return Err("Файл вне папки музыки".into());
        }
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        return Ok((src, cache::file_name("", &stem)));
    }
    let (service, id) = key.split_once(':').ok_or("Неверный трек")?;
    let t = st.saved(service, id).ok_or("Скачать можно только трек из библиотеки")?;
    let name = cache::file_name(&t.artist, &t.title);
    if let Some(p) = st.cache.get(&cache::service_key(service, id)) {
        return Ok((p, name));
    }
    let job = service_job(app, st, &t).await?;
    let cache = st.cache.clone();
    let p = tauri::async_runtime::spawn_blocking(move || cache.fetch(&job)).await.map_err(|e| e.to_string())??;
    Ok((p, name))
}

/// Задача кэша для трека сервиса из библиотеки. Ссылка на поток — из кэша ссылок или новый запрос к сервису.
async fn service_job(app: &AppHandle, st: &AppState, t: &saved::Saved) -> Res<cache::Job> {
    let s = match t.service.as_str() {
        "vk" if t.api => match st.streams.get(&format!("vkapi:{}", t.id)) {
            Some(s) => s,
            None => vk_api_stream(app, st, &t.id, t.access_key.as_deref()).await?,
        },
        "vk" => {
            let reload = t.reload.as_deref().ok_or("Этот трек ВК недоступен")?;
            match st.streams.get(&format!("vk:{reload}")) {
                Some(s) => s,
                None => vk_stream(app, st, reload).await?,
            }
        }
        _ => match st.streams.get(&format!("sc:{}", t.id)) {
            Some(s) => s,
            None => sc_stream(app, st, &t.id).await?,
        },
    };
    Ok(cache_job(cache::service_key(&t.service, &t.id), &s, t))
}

/// Закэшировать следующий трек очереди (интерфейс зовёт, когда текущий уже в кэше). key — как в порядке
/// библиотеки; трек не из библиотеки или уже закэшированный пропускается. Ссылку на поток сервиса
/// запрашиваем сейчас, перед самой выкачкой, — чтобы она не успела протухнуть.
#[tauri::command(rename_all = "snake_case")]
async fn cache_prefetch(app: AppHandle, st: State<'_, AppState>, key: String) -> Res<()> {
    let gen = st.cache.generation();
    let job = if key.starts_with('/') {
        if !library::inside(&st.music_dir(), Path::new(&key)) {
            return Err("Файл вне папки музыки".into());
        }
        let Some(job) = local_job(&key) else { return Ok(()) };
        job
    } else {
        let Some((service, id)) = key.split_once(':') else { return Ok(()) };
        let Some(t) = st.saved(service, id) else { return Ok(()) };
        if st.cache.get(&cache::service_key(service, id)).is_some() {
            return Ok(());
        }
        service_job(&app, &st, &t).await?
    };
    st.cache.prefetch(job, gen);
    Ok(())
}

#[derive(Serialize)]
struct CacheInfo {
    dir: String,
    count: usize,
    bytes: u64,
}

#[tauri::command(rename_all = "snake_case")]
async fn cache_info(st: State<'_, AppState>) -> Res<CacheInfo> {
    let cache = st.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (count, bytes) = cache.stats();
        CacheInfo { dir: cache.dir().display().to_string(), count, bytes }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
async fn cache_clear(st: State<'_, AppState>) -> Res<()> {
    st.genius.clear();
    let cache = st.cache.clone();
    tauri::async_runtime::spawn_blocking(move || cache.clear())
        .await
        .map_err(|e| e.to_string())?
}

// ───────────────────────── темы ─────────────────────────

#[tauri::command(rename_all = "snake_case")]
fn themes_list(st: State<'_, AppState>) -> Vec<themes::ThemeInfo> {
    st.themes.list()
}

#[tauri::command(rename_all = "snake_case")]
fn theme_render(st: State<'_, AppState>, id: Option<String>) -> Res<themes::ThemeRender> {
    let id = id.unwrap_or_else(|| st.cfg.lock().unwrap().theme.clone());
    match st.themes.render(&id) {
        Ok(r) => Ok(r),
        // выбранная тема пропала — возвращаемся к встроенной
        Err(_) if id != themes::DEFAULT_THEME => st.themes.render(themes::DEFAULT_THEME),
        Err(e) => Err(e),
    }
}

#[tauri::command(rename_all = "snake_case")]
fn theme_select(st: State<'_, AppState>, id: String) -> Res<themes::ThemeRender> {
    let r = st.themes.render(&id)?;
    st.cfg.lock().unwrap().theme = id;
    st.save()?;
    Ok(r)
}

/// Тема приходит одним двоичным куском (invoke('theme_install', Uint8Array)).
#[tauri::command]
fn theme_install(st: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Res<themes::ThemeInfo> {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err("Нет данных темы".into());
    };
    st.themes.install(body)
}

/// Файл темы целиком (ArrayBuffer в JS) — для оформления виджета OBS (ядро Whisper разбирает его само).
#[tauri::command(rename_all = "snake_case")]
fn theme_file(st: State<'_, AppState>, id: String) -> Res<tauri::ipc::Response> {
    st.themes.raw(&id).map(tauri::ipc::Response::new)
}

/// Тема перетащена в окно — приходит путь к файлу.
#[tauri::command(rename_all = "snake_case")]
fn theme_install_path(st: State<'_, AppState>, path: String) -> Res<themes::ThemeInfo> {
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() as usize > wmt::MAX_FILE {
        return Err("файл темы больше 64 МБ".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    st.themes.install(&bytes)
}

// ───────────────────────── библиотека тем (whisper-themes) ─────────────────────────

#[derive(Serialize)]
struct LibItem {
    #[serde(flatten)]
    entry: themelib::Entry,
    preview_url: Option<String>,
    installed: bool,
    installed_version: Option<String>,
    /// установлена, но в библиотеке версия новее
    update: bool,
    active: bool,
    /// id занят встроенной темой — поставить нельзя
    builtin_conflict: bool,
}

#[derive(Serialize)]
struct LibList {
    categories: Vec<themelib::Category>,
    themes: Vec<LibItem>,
}

#[tauri::command(rename_all = "snake_case")]
async fn themelib_list(st: State<'_, AppState>, fresh: Option<bool>) -> Res<LibList> {
    let catalog = themelib::index(&st.http, fresh == Some(true)).await?;
    let local: std::collections::HashMap<String, themes::ThemeInfo> = st
        .themes
        .list()
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
    let active = st.cfg.lock().unwrap().theme.clone();
    let themes = catalog
        .themes
        .into_iter()
        .map(|e| {
            let mine = local.get(&e.id).filter(|t| !t.builtin);
            let installed_version = mine.and_then(|t| t.version.clone());
            let lib_version = e.version.clone().filter(|v| !v.is_empty());
            LibItem {
                preview_url: e.preview.as_deref().and_then(themelib::url_for),
                installed: mine.is_some(),
                update: mine.is_some() && lib_version.is_some() && lib_version != installed_version,
                active: active == e.id,
                builtin_conflict: themes::ThemeStore::is_builtin(&e.id),
                installed_version,
                entry: e,
            }
        })
        .collect();
    Ok(LibList { categories: catalog.categories, themes })
}

/// Установить (или обновить) тему из библиотеки и сразу включить её.
#[tauri::command(rename_all = "snake_case")]
async fn themelib_install(st: State<'_, AppState>, id: String) -> Res<themes::ThemeRender> {
    let e = themelib::index(&st.http, false)
        .await?
        .themes
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("Темы «{id}» нет в библиотеке"))?;
    if e.size.is_some_and(|n| n as usize > wmt::MAX_FILE) {
        return Err("Тема больше 64 МБ".into());
    }
    let bytes = themelib::fetch(&st.http, &e.file).await?;
    // в файле должна быть та самая тема, что в списке
    let parsed = wmt::open(&bytes).map_err(|err| format!("Тема не загружена: {err}"))?;
    if parsed.id() != e.id {
        return Err(format!("В файле тема «{}», а ожидалась «{}»", parsed.id(), e.id));
    }
    let info = st.themes.install(&bytes)?;
    let r = st.themes.render(&info.id)?;
    st.cfg.lock().unwrap().theme = info.id;
    st.save()?;
    Ok(r)
}

#[tauri::command(rename_all = "snake_case")]
fn theme_remove(st: State<'_, AppState>, id: String) -> Res<()> {
    st.themes.remove(&id)?;
    let mut c = st.cfg.lock().unwrap();
    if c.theme == id {
        c.theme = themes::DEFAULT_THEME.into();
        let _ = c.save();
    }
    Ok(())
}

// ───────────────────────── vkm:// — файлы тем и обложки ─────────────────────────

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "max-age=3600")
        .body(body)
        .unwrap_or_default()
}

fn serve(app: &AppHandle, uri: &str) -> Response<Vec<u8>> {
    let not_found = || respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec());
    let Ok(url) = tauri::Url::parse(uri) else { return not_found() };
    let st = app.state::<AppState>();
    let segs: Vec<String> = url
        .path_segments()
        .map(|s| s.map(str::to_string).collect())
        .unwrap_or_default();
    match segs.first().map(String::as_str) {
        // /theme/<id>/<file>
        Some("theme") if segs.len() == 3 => {
            let Ok(theme) = st.themes.get(&segs[1]) else { return not_found() };
            match theme.asset(&segs[2]) {
                Some(data) => respond(StatusCode::OK, wmt::mime_of(&segs[2]), data.to_vec()),
                None => not_found(),
            }
        }
        // /cover?path=<путь к файлу>
        Some("cover") => {
            let Some(path) = url.query_pairs().find(|(k, _)| k == "path").map(|(_, v)| v.to_string()) else {
                return not_found();
            };
            let dir = st.music_dir();
            if !library::inside(&dir, Path::new(&path)) {
                return respond(StatusCode::FORBIDDEN, "text/plain", Vec::new());
            }
            match library::cover(Path::new(&path)) {
                Some((data, mime)) => respond(StatusCode::OK, mime, data),
                None => not_found(),
            }
        }
        _ => not_found(),
    }
}

/// Старый драйвер NVIDIA: с ним WebKitGTK через DMA-BUF может рисовать пустое окно.
/// Версия — из /sys/module/nvidia/version («615.78.08»).
fn old_nvidia(version: Option<&str>) -> bool {
    const FIXED: u32 = 560;
    version
        .and_then(|v| v.trim().split('.').next()?.parse::<u32>().ok())
        .is_some_and(|major| major < FIXED)
}

/// Окно — наверх (ссылка whisper:// пришла в уже открытое приложение, второй запуск).
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    // Обход WEBKIT_DISABLE_DMABUF_RENDERER дорог: кадр копируется через процессор, и чем больше окно, тем
    // медленнее (2560×1440: ~10 кадров/с против 60 с DMA-BUF на NVIDIA 615, проверено 2026-10-30). Поэтому
    // включаем его только со старым драйвером NVIDIA; переменная, заданная пользователем, главнее.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
        && old_nvidia(std::fs::read_to_string("/sys/module/nvidia/version").ok().as_deref())
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let cfg = Config::load();
    // от старых версий в config.json мог остаться токен ВКонтакте — перезаписываем без него
    if config::has_legacy_secrets() {
        let _ = cfg.save();
    }
    // сохранённые плейлисты играют и после перезапуска: их access_key — сразу в память клиента ВК
    let playlists = playlists::PlaylistStore::new();
    let vk = vk::Vk::new();
    vk.remember_keys(playlists.keys());
    let state = AppState {
        themes: themes::ThemeStore::new(),
        http: themelib::client(),
        vk,
        sc: sc::Sc::new(),
        saved: saved::SavedStore::new(),
        playlists,
        order: order::OrderStore::new(),
        cfg: Mutex::new(cfg),
        player: Mutex::new(None),
        player_error: Mutex::new(None),
        dl_seq: AtomicU64::new(0),
        streams: streams::StreamCache::new(),
        cache: Arc::new(cache::TrackCache::new()),
        stats: stats::StatsStore::new(),
        genius: lyrics::Genius::new(),
    };

    let app = tauri::Builder::default()
        // второй запуск — окно уже открытого; ссылку whisper:// из его аргументов плагин отдаёт deep-link
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_deep_link::init())
        .manage(state)
        // wa:// — потоки Whisper Archive (ядро: whisper::stream), только главному окну
        .register_asynchronous_uri_scheme_protocol("wa", |ctx, request, responder| {
            if ctx.webview_label() != "main" {
                responder.respond(respond(StatusCode::FORBIDDEN, "text/plain", Vec::new()));
                return;
            }
            tauri::async_runtime::spawn(async move {
                responder.respond(whisper::stream::serve_stream(request).await);
            });
        })
        .register_asynchronous_uri_scheme_protocol("vkm", |ctx, request, responder| {
            if ctx.webview_label() != "main" {
                responder.respond(respond(StatusCode::FORBIDDEN, "text/plain", Vec::new()));
                return;
            }
            let app = ctx.app_handle().clone();
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(serve(&app, &uri));
            });
        })
        .setup(|app| {
            deeplink::init(app.handle()); // ссылки whisper://room/КОД — приглашения в комнату
            let handle = app.handle().clone();
            let st = app.state::<AppState>();
            let h = handle.clone();
            st.cache.on_done(move |key, ok| {
                let kind = if ok { "cached" } else { "failed" };
                let _ = h.emit("cache://event", serde_json::json!({ "type": kind, "key": key }));
            });
            let (volume, normalize) = {
                let c = st.cfg.lock().unwrap();
                (c.volume, c.normalize)
            };
            // mpv запускается из главного потока — PR_SET_PDEATHSIG привязан к нему
            match player::Player::start(volume, move |ev| {
                let _ = handle.emit("player://event", &ev);
            }) {
                Ok(p) => {
                    if normalize {
                        let _ = p.set_normalize(true);
                    }
                    *st.player.lock().unwrap() = Some(Arc::new(p));
                }
                Err(e) => {
                    eprintln!("vkmusic: {e}");
                    *st.player_error.lock().unwrap() = Some(e);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            save_prefs,
            fonts_list,
            player_play,
            player_cue,
            player_rate,
            player_toggle,
            player_pause,
            player_seek,
            player_volume,
            player_stop,
            player_normalize,
            dl_start,
            dl_caps,
            local_list,
            local_delete,
            open_music_dir,
            themes_list,
            theme_render,
            theme_select,
            theme_install,
            theme_install_path,
            theme_file,
            theme_remove,
            themelib_list,
            themelib_install,
            svc_login,
            svc_login_cancel,
            svc_logout,
            vk_status,
            vk_search,
            vk_play,
            vk_delete,
            vk_home,
            vk_my_playlists,
            vk_my_playlists_cached,
            vk_playlist,
            vk_playlist_cached,
            vk_mix,
            vk_play_api,
            vk_playlist_create,
            vk_playlist_rename,
            vk_playlist_delete,
            vk_playlist_add,
            vk_playlist_remove,
            vk_playlist_follow,
            sc_status,
            sc_search,
            sc_resolve,
            sc_play,
            saved_list,
            saved_add,
            saved_sync,
            saved_remove,
            library_order,
            library_move,
            track_download,
            cache_info,
            cache_clear,
            cache_prefetch,
            stats_get,
            stats_add,
            stats_all,
            lyrics_get,
            lyrics_open,
            lyrics_meta,
            lyrics_cached,
            lyrics_translation_cached,
            lyrics_translation,
            lyrics_note,
            // ядро Whisper: Whisper Archive
            secret::secret_backend,
            whisper::wa_token_exists,
            whisper::wa_token_set,
            whisper::wa_token_clear,
            whisper::wa_albums_mine,
            whisper::wa_album,
            whisper::wa_me,
            whisper::wa_stream,
            whisper::auth::wa_auth_request_code,
            whisper::auth::wa_auth_issue,
            whisper::stream::wa_prefetch,
            whisper_vk::wa_save_track,
            whisper_vk::wa_play,
            // «Слушать вместе»
            whisper::rooms::wa_room_create,
            whisper::rooms::wa_room_get,
            whisper::rooms::wa_room_join,
            whisper::rooms::wa_room_leave,
            whisper::rooms::wa_room_state,
            whisper::rooms::wa_room_suggest,
            whisper::rooms::wa_room_dismiss,
            whisper::rooms::wa_room_transfer,
            whisper::rooms::wa_room_ping,
            deeplink::room_link_take,
            // Discord и виджет OBS
            discord::discord_config,
            discord::discord_status,
            discord::discord_retry,
            discord::discord_update,
            obs::obs_config,
            obs::obs_state,
            obs::obs_theme,
        ])
        .build(tauri::generate_context!())
        .expect("не удалось запустить приложение");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            let st = handle.state::<AppState>();
            let _ = st.save();
            let player = st.player.lock().unwrap().take();
            if let Some(p) = player {
                p.quit();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::old_nvidia;

    #[test]
    fn dmabuf_workaround_only_for_old_nvidia() {
        assert!(old_nvidia(Some("550.120\n")));
        assert!(!old_nvidia(Some("615.78.08\n")));
        assert!(!old_nvidia(Some("560.35.03")));
        // нет NVIDIA (Intel/AMD) или версия непонятна — DMA-BUF
        assert!(!old_nvidia(None));
        assert!(!old_nvidia(Some("abc")));
    }
}
