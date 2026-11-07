//! Тексты песен: ВКонтакте и Genius.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── тексты песен (Genius) ─────────────────────────

/// Текст песни для играющего трека: из кэша или поиск на Genius по исполнителю и названию.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn lyrics_get(app: AppHandle, st: State<'_, AppState>, artist: String, title: String, vk_id: Option<String>) -> Res<lyrics::Lyrics> {
    if title.trim().is_empty() || artist.len() > 500 || title.len() > 500 {
        return Err("Нет названия трека".into());
    }
    if let Some(l) = st.genius.lyrics_cached(&artist, &title) {
        return Ok(l);
    }
    // у трека ВК — сначала текст самого ВК (нет входа или текста — дальше к Genius)
    if let Some(id) = vk_id.filter(|id| valid_vk_id(id)) {
        if let Ok(session) = vk_session(&app).await {
            if let Ok(Some(vl)) = st.vk.lyrics(&session, &id).await {
                let l = lyrics::Lyrics {
                    status: "found".into(),
                    text: vl.text,
                    times: vl.times,
                    source: "vk".into(),
                    credits: vl.credits,
                    meta: true,
                    ..lyrics::Lyrics::empty()
                };
                st.genius.store_for(&artist, &title, &l);
                return Ok(l);
            }
        }
    }
    if let Some(l) = st.genius.old_vk(&artist, &title) {
        return Ok(l);
    }
    st.genius.lyrics(&artist, &title).await
}


/// Текст из кэша на диске (без сети) — показывается сразу при включении трека; нет — null.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn lyrics_cached(st: State<'_, AppState>, artist: String, title: String) -> Res<Option<lyrics::Lyrics>> {
    if title.trim().is_empty() || artist.len() > 500 || title.len() > 500 {
        return Ok(None);
    }
    Ok(st.genius.lyrics_cached(&artist, &title))
}

/// Перевод из кэша на диске (без сети); нет — null.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn lyrics_translation_cached(st: State<'_, AppState>, url: String) -> Res<Option<String>> {
    Ok(st.genius.translation_cached(&url))
}

/// Переводы и язык оригинала найденного текста — вторым шагом, после показа текста.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn lyrics_meta(st: State<'_, AppState>, artist: String, title: String) -> Res<lyrics::Lyrics> {
    st.genius.meta(&artist, &title).await
}

/// Текст перевода песни: url — из `translations` найденного текста.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn lyrics_translation(st: State<'_, AppState>, url: String) -> Res<String> {
    st.genius.translation(&url).await
}

/// Пояснение Genius к фрагменту текста (по нажатию на фрагмент).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn lyrics_note(st: State<'_, AppState>, id: u64) -> Res<String> {
    if id == 0 {
        return Err("Нет пояснения".into());
    }
    st.genius.annotation(id).await
}

/// Открыть страницу песни на Genius в браузере — только адреса genius.com.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn lyrics_open(url: String) -> Res<()> {
    let ok = lyrics::genius_page(&url);
    if !ok {
        return Err("Недопустимая ссылка".into());
    }
    os::open(&url)
}
