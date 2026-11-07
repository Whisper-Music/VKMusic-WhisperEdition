//! Темы: установка, выбор, файлы; библиотека тем whisper-themes.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── темы ─────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn themes_list(st: State<'_, AppState>) -> Vec<themes::ThemeInfo> {
    st.themes.list()
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn theme_render(st: State<'_, AppState>, id: Option<String>) -> Res<themes::ThemeRender> {
    let id = id.unwrap_or_else(|| st.cfg.lock().unwrap().theme.clone());
    match st.themes.render(&id) {
        Ok(r) => Ok(r),
        // выбранная тема пропала — возвращаемся к встроенной
        Err(_) if id != themes::DEFAULT_THEME => st.themes.render(themes::DEFAULT_THEME),
        Err(e) => Err(e),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn theme_select(st: State<'_, AppState>, id: String) -> Res<themes::ThemeRender> {
    let r = st.themes.render(&id)?;
    st.cfg.lock().unwrap().theme = id;
    st.save()?;
    Ok(r)
}

/// Тема приходит одним двоичным куском (invoke('theme_install', Uint8Array)).
#[tauri::command]
pub(crate) fn theme_install(st: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Res<themes::ThemeInfo> {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err("Нет данных темы".into());
    };
    st.themes.install(body)
}

/// Файл темы целиком (ArrayBuffer в JS) — для оформления виджета OBS (ядро Whisper разбирает его само).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn theme_file(st: State<'_, AppState>, id: String) -> Res<tauri::ipc::Response> {
    st.themes.raw(&id).map(tauri::ipc::Response::new)
}

/// «Сохранить стиль в файл…»: тема, собранная интерфейсом (текущий стиль с цветами и эффектами пользователя),
/// сырым телом → папка загрузок, «<название>.wmt» (свободное имя). Без системного диалога: он белый (GTK) и
/// выбивается из оформления. Файл должен быть темой — иначе отказ. Возвращает путь.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn theme_save(st: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Res<String> {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err("Нет данных темы".into());
    };
    if body.len() > wmt::MAX_FILE {
        return Err("файл темы больше 64 МБ".into());
    }
    let bytes = body.clone();
    let dest = st.cfg.lock().unwrap().download_dir();
    tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        let theme = wmt::open(&bytes).map_err(|e| format!("Тема не собралась: {e}"))?;
        let ext = "wmt";
        std::fs::create_dir_all(&dest).map_err(|e| format!("Не удалось создать {}: {e}", dest.display()))?;
        let out = cache::free_path(&dest, &cache::file_name("", theme.name()), ext);
        std::fs::write(&out, &bytes).map_err(|e| format!("Не удалось сохранить тему: {e}"))?;
        Ok(out.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}


// ───────────────────────── библиотека тем (whisper-themes) ─────────────────────────

#[derive(Serialize)]
pub(crate) struct LibItem {
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
pub(crate) struct LibList {
    categories: Vec<themelib::Category>,
    themes: Vec<LibItem>,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn themelib_list(st: State<'_, AppState>, fresh: Option<bool>) -> Res<LibList> {
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
pub(crate) async fn themelib_install(st: State<'_, AppState>, id: String) -> Res<themes::ThemeRender> {
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
pub(crate) fn theme_remove(st: State<'_, AppState>, id: String) -> Res<()> {
    st.themes.remove(&id)?;
    let mut c = st.cfg.lock().unwrap();
    if c.theme == id {
        c.theme = themes::DEFAULT_THEME.into();
        let _ = c.save();
    }
    Ok(())
}
