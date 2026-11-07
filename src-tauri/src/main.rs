// Windows: без консольного окна (приложение с окном; в отладочной сборке консоль остаётся — видны сообщения)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! VK Music — музыкальный плеер: библиотека ~/Music/vk, загрузка через yt-dlp, темы .wmt,
//! прослушивание ВКонтакте и SoundCloud под своим аккаунтом, кэш треков библиотеки и загрузка их в ~/Downloads.

mod backup;
mod cache;
mod cmd;
mod config;
mod deeplink;
// Rust-часть ядра Whisper (подмодуль src/core): Discord, виджет OBS, Whisper Archive и «Слушать вместе».
// Своё у приложения — whisper_host.rs (ключ, семафор, Origin, Discord/OBS), whisper_vk.rs (архив и треки VK Music)
#[path = "../../src/core/rust/discord.rs"]
mod discord;
mod fonts;
mod host;
mod hotkeys;
mod library;
mod loudness;
mod lyrics;
#[path = "../../src/core/rust/obs.rs"]
mod obs;
mod order;
mod os;
mod player;
mod playlists;
mod relay;
mod saved;
mod sc;
mod secret;
mod stats;
mod streams;
mod themelib;
mod themes;
mod tray;
mod update;
mod vk;
#[path = "../../src/core/rust/whisper/mod.rs"]
mod whisper;
mod whisper_host;
mod whisper_vk;
mod winstate;
mod wmt;
mod ytdlp;

use config::Config;
// общие помощники разделов (vk_session, local_job, …) — видны всем модулям через `use crate::*`
use cmd::{local_cmd::*, login::*, player_cmd::*, sc_cmd::*, storage_cmd::*, vk_cmd::*};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};
use update::WithUpdater;

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
    /// громкость треков на диске (EBU R128) для «Постоянного уровня громкости»
    loud: Arc<loudness::Loudness>,
    /// открытая для восстановления копия (backup_read → backup_apply)
    restore: Mutex<Option<backup::Backup>>,
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
    /// предел кэша треков, МБ (0 — без ограничения)
    cache_limit_mb: u64,
    player_error: Option<String>,
    /// config.json при запуске не разобрался (повреждён) — интерфейс предложит автокопию
    config_broken: bool,
    /// значок в трее возможен (Linux: есть AppIndicator; не --host) / включён / закрывать в трей
    tray_available: bool,
    tray: bool,
    close_to_tray: bool,
    tray_icon: String,
}

#[tauri::command(rename_all = "snake_case")]
fn app_info(st: State<'_, AppState>) -> Res<Info> {
    let c = st.cfg.lock().unwrap().clone();
    Ok(Info {
        music_dir: c.music_dir().display().to_string(),
        download_dir: c.download_dir().display().to_string(),
        cache_limit_mb: c.cache_limit_mb,
        config_broken: config::BROKEN.load(std::sync::atomic::Ordering::Relaxed),
        tray_available: !host::active() && tray::available(),
        tray: c.tray,
        close_to_tray: c.close_to_tray,
        tray_icon: c.tray_icon,
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
    // предел кэша треков, МБ (0 — без ограничения)
    cache_limit_mb: Option<u64>,
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
        if let Some(mb) = cache_limit_mb {
            c.cache_limit_mb = mb.min(1024 * 1024);
            st.cache.set_limit(c.cache_limit_mb * 1024 * 1024);
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

pub(crate) fn serve(app: &AppHandle, uri: &str) -> Response<Vec<u8>> {
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

// ───────────────────────── своя полоса заголовка окна (окно без рамки системы, как у Whisper) ─────────────────────────

#[tauri::command]
fn win_minimize(window: tauri::Window) {
    let _ = window.minimize();
}
#[tauri::command]
fn win_toggle_max(window: tauri::Window) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}
#[tauri::command]
fn win_close(window: tauri::Window) {
    let _ = window.close();
}
#[tauri::command]
fn win_drag(window: tauri::Window) {
    let _ = window.start_dragging();
}

/// Окно — наверх (ссылка whisper:// пришла в уже открытое приложение, второй запуск).
/// С --host окна нет: интерфейс — во вкладке браузера, событие room-link уходит туда.
fn show_main(app: &AppHandle) {
    if host::active() {
        return;
    }
    if let Some(w) = app.get_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Идентификатор приложения (tauri.conf.json → identifier): по нему WebKit хранит куки входа и localStorage
/// (~/.local/share/<id>), а связка ключей — ключ Whisper Archive (secret.rs).
pub const IDENT: &str = "vkmusic-whisper";
/// Прежний идентификатор (до выхода в Whisper-Music, 2026-11-04).
pub const OLD_IDENT: &str = "ru.bobrik.vkmusic";

/// Данные WebKit со старого идентификатора — на новое место (один раз, до создания окна): иначе после обновления
/// пришлось бы заново входить во ВКонтакте и SoundCloud и пропали бы история очереди и настройки модулей.
fn migrate_ident() {
    for base in [dirs::data_dir(), dirs::config_dir(), dirs::cache_dir()].into_iter().flatten() {
        let (old, new) = (base.join(OLD_IDENT), base.join(IDENT));
        if old.is_dir() && !new.exists() {
            if let Err(e) = std::fs::rename(&old, &new) {
                eprintln!("vkmusic: не удалось перенести {}: {e}", old.display());
            }
        }
    }
}

/// Контекст Tauri. С --host главное окно скрытое и пустое: оно держит куки WebKit (вход в сервисы) и принимает
/// команды от браузера, а интерфейс в нём не запускается (иначе он играл бы и считал статистику вторым экземпляром).
fn context(host: bool) -> tauri::Context<tauri::Wry> {
    let mut ctx = tauri::generate_context!();
    if host {
        if let Some(w) = ctx.config_mut().app.windows.iter_mut().find(|w| w.label == "main") {
            w.visible = false;
            w.url = tauri::WebviewUrl::External("about:blank".parse().expect("about:blank"));
        }
    }
    ctx
}

fn main() {
    // --host [ПОРТ]: интерфейс в браузере на http://127.0.0.1:ПОРТ вместо окна (host.rs)
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host_port = host::port_from_args(&args).unwrap_or_else(|e| {
        eprintln!("vkmusic: {e}");
        std::process::exit(2);
    });
    migrate_ident();
    // восстановление из резервной копии — до того, как что-либо загрузит настройки
    backup::apply_pending();
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
    let cache_limit = cfg.cache_limit_mb * 1024 * 1024;
    let playlists = playlists::PlaylistStore::new();
    let vk = vk::Vk::new();
    vk.remember_keys(playlists.keys());
    let loud = loudness::Loudness::new();
    let state = AppState {
        themes: themes::ThemeStore::new(),
        http: themelib::client(),
        vk,
        sc: sc::Sc::new().with_cid_file(
            dirs::cache_dir().unwrap_or_else(|| config::home().join(".cache")).join("vkmusic").join("sc-client-id"),
        ),
        saved: saved::SavedStore::new(),
        playlists,
        order: order::OrderStore::new(),
        cfg: Mutex::new(cfg),
        player: Mutex::new(None),
        player_error: Mutex::new(None),
        dl_seq: AtomicU64::new(0),
        streams: streams::StreamCache::new(),
        cache: {
            let c = Arc::new(cache::TrackCache::new());
            c.set_limit(cache_limit);
            let l = loud.clone();
            c.on_file(move |p| l.request(p));
            c
        },
        loud,
        restore: Mutex::new(None),
        stats: stats::StatsStore::new(),
        genius: lyrics::Genius::new(),
    };

    let app = tauri::Builder::default()
        // второй запуск — окно уже открытого; ссылку whisper:// из его аргументов плагин отдаёт deep-link
        // (с --host — вкладка браузера, если запустили не по ссылке whisper://: ссылку получит открытая вкладка)
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if host::active() {
                if !args.iter().any(|a| a.starts_with("whisper:")) {
                    host::open_browser();
                }
            } else {
                show_main(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        // автообновление — только Windows (update.rs)
        .with_updater()
        // глобальные горячие клавиши (hotkeys.rs)
        .plugin(hotkeys::plugin())
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
            // мини-плееру — только обложки (файлы из папки музыки)
            let mini_cover = ctx.webview_label() == "mini" && request.uri().path() == "/cover";
            if ctx.webview_label() != "main" && !mini_cover {
                responder.respond(respond(StatusCode::FORBIDDEN, "text/plain", Vec::new()));
                return;
            }
            let app = ctx.app_handle().clone();
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(serve(&app, &uri));
            });
        })
        .setup(move |app| {
            // --host: сервер поднимается до mpv — занятый порт останавливает запуск сразу
            if let Some(port) = host_port {
                match host::start(app.handle(), port) {
                    Ok(url) => {
                        eprintln!("VK Music: интерфейс — {url}\nVK Music: выход — Ctrl+C");
                        host::exit_on_signal(app.handle());
                        host::open_browser();
                    }
                    Err(e) => {
                        eprintln!("vkmusic: {e}");
                        std::process::exit(2);
                    }
                }
            }
            deeplink::init(app.handle()); // ссылки whisper://room/КОД — приглашения в комнату
            let handle = app.handle().clone();
            let st = app.state::<AppState>();
            // окно — прежнего размера и там же, где было (с --host окно скрыто и не запоминается)
            app.manage(tray::Mini::default());
            let c = st.cfg.lock().unwrap().clone();
            if host_port.is_none() && c.tray {
                if let Err(e) = tray::init(app.handle(), &c.tray_icon, &c.tray_accent) {
                    eprintln!("vkmusic: значок в трее: {e}");
                }
            }
            if host_port.is_none() {
                if let Some(win) = winstate::main_window(app.handle()) {
                    // крестик — в трей (если значок есть и так настроено): музыка играет дальше
                    let h = handle.clone();
                    let w2 = win.clone();
                    win.on_window_event(move |e| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                            if tray::active() && h.state::<AppState>().cfg.lock().unwrap().close_to_tray {
                                api.prevent_close();
                                let _ = w2.hide();
                            }
                        }
                    });
                    let saved = st.cfg.lock().unwrap().window;
                    winstate::restore(&win, saved);
                    let h = handle.clone();
                    winstate::track(&win, move |next| {
                        let st = h.state::<AppState>();
                        let mut c = st.cfg.lock().unwrap();
                        c.window = next(c.window);
                    });
                }
            }
            let h = handle.clone();
            st.cache.on_done(move |key, ok| {
                let kind = if ok { "cached" } else { "failed" };
                let _ = h.emit("cache://event", serde_json::json!({ "type": kind, "key": key }));
            });
            let (volume, normalize, device) = {
                let c = st.cfg.lock().unwrap();
                (c.volume, c.normalize, c.audio_device.clone().unwrap_or_default())
            };
            // mpv запускается из главного потока — PR_SET_PDEATHSIG привязан к нему
            match player::Player::start(volume, st.loud.clone(), move |ev| {
                let _ = handle.emit("player://event", &ev);
            }) {
                Ok(p) => {
                    if normalize {
                        let _ = p.set_normalize(true);
                    }
                    let p = Arc::new(p);
                    p.restore_device(device);
                    *st.player.lock().unwrap() = Some(p);
                }
                Err(e) => {
                    eprintln!("vkmusic: {e}");
                    *st.player_error.lock().unwrap() = Some(e);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            update::update_check,
            hotkeys::hotkeys_set,
            cmd::local_cmd::cover_bytes,
            update::update_install,
            app_info,
            save_prefs,
            fonts_list,
            win_minimize,
            win_toggle_max,
            win_close,
            win_drag,
            cmd::player_cmd::player_play,
            cmd::player_cmd::player_cue,
            cmd::player_cmd::player_rate,
            cmd::player_cmd::player_pause,
            cmd::player_cmd::player_seek,
            cmd::player_cmd::player_volume,
            cmd::player_cmd::player_stop,
            cmd::player_cmd::player_normalize,
            cmd::player_cmd::player_devices,
            cmd::player_cmd::player_device,
            cmd::player_cmd::tray_prefs,
            tray::tray_state,
            tray::mini_state,
            tray::mini_state_get,
            tray::mini_ctl,
            tray::mini_toggle,
            cmd::storage_cmd::backup_export,
            cmd::storage_cmd::backup_auto,
            cmd::storage_cmd::backup_list,
            cmd::storage_cmd::backup_read,
            cmd::storage_cmd::backup_read_auto,
            cmd::storage_cmd::backup_apply,
            cmd::storage_cmd::app_restart,
            cmd::local_cmd::dl_start,
            cmd::local_cmd::dl_caps,
            cmd::local_cmd::local_list,
            cmd::local_cmd::local_delete,
            cmd::local_cmd::open_music_dir,
            cmd::themes_cmd::themes_list,
            cmd::themes_cmd::theme_render,
            cmd::themes_cmd::theme_select,
            cmd::themes_cmd::theme_install,
            cmd::storage_cmd::theme_install_path,
            cmd::themes_cmd::theme_save,
            cmd::storage_cmd::open_download_dir,
            cmd::themes_cmd::theme_file,
            cmd::themes_cmd::theme_remove,
            cmd::themes_cmd::themelib_list,
            cmd::themes_cmd::themelib_install,
            cmd::login::svc_login,
            cmd::login::svc_login_cancel,
            cmd::login::svc_logout,
            cmd::vk_cmd::vk_status,
            cmd::login::vk_search,
            cmd::login::vk_play,
            cmd::saved_cmd::vk_delete,
            cmd::vk_cmd::vk_home,
            cmd::vk_cmd::vk_my_playlists,
            cmd::vk_cmd::vk_my_playlists_cached,
            cmd::vk_cmd::vk_playlist,
            cmd::vk_cmd::vk_playlist_cached,
            cmd::vk_cmd::vk_mix,
            cmd::vk_cmd::vk_play_api,
            cmd::vk_cmd::vk_playlist_create,
            cmd::vk_cmd::vk_playlist_rename,
            cmd::vk_cmd::vk_playlist_delete,
            cmd::vk_cmd::vk_playlist_add,
            cmd::vk_cmd::vk_playlist_remove,
            cmd::vk_cmd::vk_playlist_follow,
            cmd::sc_cmd::sc_status,
            cmd::sc_cmd::sc_search,
            cmd::sc_cmd::sc_resolve,
            cmd::sc_cmd::sc_play,
            cmd::saved_cmd::saved_list,
            cmd::saved_cmd::saved_add,
            cmd::saved_cmd::saved_sync,
            cmd::saved_cmd::saved_remove,
            cmd::local_cmd::library_order,
            cmd::local_cmd::library_move,
            cmd::storage_cmd::track_download,
            cmd::storage_cmd::cache_info,
            cmd::storage_cmd::cache_clear,
            cmd::storage_cmd::cache_prefetch,
            cmd::storage_cmd::stats_get,
            cmd::storage_cmd::stats_add,
            cmd::storage_cmd::stats_all,
            cmd::lyrics_cmd::lyrics_get,
            cmd::lyrics_cmd::lyrics_open,
            cmd::lyrics_cmd::lyrics_meta,
            cmd::lyrics_cmd::lyrics_cached,
            cmd::lyrics_cmd::lyrics_translation_cached,
            cmd::lyrics_cmd::lyrics_translation,
            cmd::lyrics_cmd::lyrics_note,
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
            whisper::rooms::wa_room_missing,
            whisper::rooms::wa_room_friends,
            whisper::rooms::wa_room_friend_forget,
            whisper::rooms::wa_room_invite,
            whisper::rooms::wa_room_invites,
            whisper::rooms::wa_room_invite_dismiss,
            whisper::rooms::wa_room_theme_share,
            whisper::rooms::wa_room_theme_get,
            whisper::rooms::wa_room_theme_remove,
            whisper_vk::wa_room_cache_track,
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
        .build(context(host_port.is_some()))
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
    fn ident_matches_config() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], super::IDENT, "IDENT и identifier в tauri.conf.json должны совпадать");
    }

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
