//! Файлы из папки музыки и загрузки yt-dlp.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── локальные треки / yt-dlp ─────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn dl_start(app: AppHandle, st: State<'_, AppState>, input: String) -> Res<u64> {
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
pub(crate) async fn dl_caps() -> Res<ytdlp::Caps> {
    tauri::async_runtime::spawn_blocking(|| ytdlp::caps().clone())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn local_list(st: State<'_, AppState>) -> Res<Vec<library::LocalTrack>> {
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

pub(crate) fn saved_key(s: &saved::Saved) -> String {
    format!("{}:{}", s.service, s.id)
}

/// Убрать из порядка и из кэша треки сервиса, которых больше нет в библиотеке.
pub(crate) fn prune_order(st: &AppState, service: &str) -> Res<()> {
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
pub(crate) fn library_order(st: State<'_, AppState>) -> Vec<String> {
    st.order.list()
}

/// Перетаскивание трека в библиотеке: key встаёт перед before (None — в конец).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn library_move(st: State<'_, AppState>, key: String, before: Option<String>) -> Res<()> {
    let ok = |k: &str| !k.is_empty() && k.len() <= 4096 && !k.chars().any(char::is_control);
    if !ok(&key) || before.as_deref().is_some_and(|b| !ok(b)) {
        return Err("Неверный трек".into());
    }
    st.order.move_before(&key, before.as_deref())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn local_delete(st: State<'_, AppState>, path: String) -> Res<()> {
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

/// Обложка трека байтами — для динамического фона: картинку с CDN холст не прочитал бы (чужой адрес).
/// Только адреса CDN ВКонтакте / SoundCloud (`whisper_host::cover_ok`), до 10 МБ.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn cover_bytes(st: State<'_, AppState>, url: String) -> Res<tauri::ipc::Response> {
    if !whisper_host::cover_ok(&url) {
        return Err("Недопустимый адрес обложки".into());
    }
    whisper_vk::fetch_cover(&st.http, &url).await.map(tauri::ipc::Response::new)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn open_music_dir(st: State<'_, AppState>) -> Res<()> {
    let dir = st.music_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    os::open(&dir)
}
