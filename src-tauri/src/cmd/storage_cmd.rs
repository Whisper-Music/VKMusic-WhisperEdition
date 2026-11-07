//! Статистика, кэш треков и загрузки, резервные копии.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── статистика прослушивания ─────────────────────────

/// Статистика трека: сколько раз дослушан, сколько секунд слушали, когда, место в топе. key — ключ трека.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn stats_get(st: State<'_, AppState>, key: String) -> stats::TrackStat {
    st.stats.get(&key)
}

/// Добавить прослушанные секунды (порцию) и, если трек доигран до конца, один полный проигрыш.
/// day/hour — местные дата (ГГГГ-ММ-ДД) и час: по ним журнал по дням и часам.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn stats_add(st: State<'_, AppState>, key: String, seconds: f64, completed: bool, day: String, hour: u8) -> Res<stats::TrackStat> {
    st.stats.add(&key, seconds, completed, &day, hour)
}

/// Вся статистика — для страницы «Статистика».
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn stats_all(st: State<'_, AppState>) -> Res<stats::Data> {
    Ok(st.stats.all())
}


// ───────────────────────── хранилище: кэш треков и загрузки ─────────────────────────

/// Скачать трек библиотеки в папку загрузок (по умолчанию ~/Downloads). key — как в порядке библиотеки:
/// путь к файлу или `vk:`/`sc:` + id. Трек сервиса берётся из кэша, а если его там нет — сначала кэшируется.
/// Возвращает путь сохранённого файла.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn track_download(app: AppHandle, st: State<'_, AppState>, key: String) -> Res<String> {
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
pub(crate) async fn library_file(app: &AppHandle, st: &AppState, key: &str) -> Res<(PathBuf, String)> {
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
pub(crate) async fn service_job(app: &AppHandle, st: &AppState, t: &saved::Saved) -> Res<cache::Job> {
    let s = match t.service.as_str() {
        "vk" if t.api => via_relay(match st.streams.get(&format!("vkapi:{}", t.id)) {
            Some(s) => s,
            None => vk_api_stream(app, st, &t.id, t.access_key.as_deref()).await?,
        }),
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
pub(crate) async fn cache_prefetch(app: AppHandle, st: State<'_, AppState>, key: String) -> Res<()> {
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
pub(crate) struct CacheInfo {
    dir: String,
    count: usize,
    bytes: u64,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn cache_info(st: State<'_, AppState>) -> Res<CacheInfo> {
    let cache = st.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (count, bytes) = cache.stats();
        CacheInfo { dir: cache.dir().display().to_string(), count, bytes }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn cache_clear(st: State<'_, AppState>) -> Res<()> {
    st.genius.clear();
    let cache = st.cache.clone();
    tauri::async_runtime::spawn_blocking(move || cache.clear())
        .await
        .map_err(|e| e.to_string())?
}


// ───────────────────────── резервные копии (backup.rs) ─────────────────────────

/// «Сохранить копию»: файл .vkmbak в папку загрузок; storage — настройки страницы (localStorage).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn backup_export(st: State<'_, AppState>, storage: std::collections::BTreeMap<String, String>) -> Res<String> {
    let dest = st.cfg.lock().unwrap().download_dir();
    let _ = st.save(); // в копию — настройки как сейчас
    tauri::async_runtime::spawn_blocking(move || backup::write_to(&dest, "vkmusic-backup", &backup::collect(storage)))
        .await
        .map_err(|e| e.to_string())?
        .map(|p| p.display().to_string())
}

/// Автокопия (раз в день, при запуске): true — сделана сейчас.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn backup_auto(st: State<'_, AppState>, storage: std::collections::BTreeMap<String, String>) -> Res<bool> {
    let _ = st.save();
    tauri::async_runtime::spawn_blocking(move || backup::auto(storage))
        .await
        .map_err(|e| e.to_string())?
        .map(|p| p.is_some())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn backup_list() -> Vec<backup::Entry> {
    backup::list()
}

#[derive(Serialize)]
pub(crate) struct BackupInfo {
    created: u64,
    version: String,
    files: usize,
    themes: usize,
    storage: std::collections::BTreeMap<String, String>,
}

pub(crate) fn open_backup(st: &AppState, bytes: &[u8]) -> Res<BackupInfo> {
    let b = backup::parse(bytes)?;
    let (created, version, files, themes) = b.summary();
    let info = BackupInfo { created, version, files, themes, storage: b.storage.clone() };
    *st.restore.lock().unwrap() = Some(b);
    Ok(info)
}

/// Открыть файл копии для восстановления (сырое тело): что в ней и настройки страницы.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn backup_read(st: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Res<BackupInfo> {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err("Нет файла копии".into());
    };
    open_backup(&st, body)
}

/// Открыть автокопию по имени (из backup_list).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn backup_read_auto(st: State<'_, AppState>, name: String) -> Res<BackupInfo> {
    let bytes = backup::read_auto(&name)?;
    open_backup(&st, &bytes)
}

/// Восстановить открытую копию: текущее — в копию «before-restore», файлы — при следующем запуске.
/// Настройки страницы записывает интерфейс, затем зовёт app_restart.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn backup_apply(st: State<'_, AppState>, storage: std::collections::BTreeMap<String, String>) -> Res<()> {
    let Some(b) = st.restore.lock().unwrap().take() else {
        return Err("Сначала откройте копию".into());
    };
    let _ = st.save();
    tauri::async_runtime::spawn_blocking(move || backup::stage(&b, storage)).await.map_err(|e| e.to_string())?
}

/// Перезапуск приложения (после восстановления копии).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn app_restart(app: AppHandle) {
    app.restart();
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn open_download_dir(st: State<'_, AppState>) -> Res<()> {
    let dir = st.cfg.lock().unwrap().download_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    os::open(&dir)
}

/// Тема перетащена в окно — приходит путь к файлу.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn theme_install_path(st: State<'_, AppState>, path: String) -> Res<themes::ThemeInfo> {
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() as usize > wmt::MAX_FILE {
        return Err("файл темы больше 64 МБ".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    st.themes.install(&bytes)
}
