//! SoundCloud: поиск, resolve, прослушивание.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── SoundCloud ─────────────────────────

/// Токен сессии SoundCloud (кука oauth_token) из WebKit. Вызывать не из главного потока.
pub(crate) fn sc_token(app: &AppHandle) -> Res<Option<String>> {
    let all = all_cookies(app)?;
    Ok(sc::token_from(all.iter().map(|c| (c.name.as_str(), c.value.as_str(), c.domain.as_str()))))
}

/// Токен, если вход выполнен (поиск и прослушивание работают и без входа).
pub(crate) async fn sc_session(app: &AppHandle) -> Option<String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || sc_token(&app).ok().flatten()).await.ok().flatten()
}

#[derive(Serialize)]
pub(crate) struct ScStatus {
    logged_in: bool,
    profile: Option<sc::Profile>,
    error: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn sc_status(app: AppHandle, st: State<'_, AppState>) -> Res<ScStatus> {
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
pub(crate) async fn sc_search(app: AppHandle, st: State<'_, AppState>, query: String, offset: Option<u64>, live: Option<bool>) -> Res<sc::Page> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err("Введите запрос до 200 символов".into());
    }
    let token = sc_session(&app).await;
    st.sc.search(token.as_deref(), query, offset.unwrap_or(0).min(10_000), live == Some(true)).await
}

/// Ссылка на трек/плейлист SoundCloud → треки.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn sc_resolve(app: AppHandle, st: State<'_, AppState>, url: String) -> Res<Vec<sc::Track>> {
    if url.len() > 2048 {
        return Err("Слишком длинная ссылка".into());
    }
    let token = sc_session(&app).await;
    st.sc.resolve(token.as_deref(), &url).await
}

/// Играть трек SoundCloud: ссылку на поток получает Rust (по нажатию), интерфейс её не видит.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn sc_play(app: AppHandle, st: State<'_, AppState>, id: String, title: Option<String>, fresh: Option<bool>) -> Res<bool> {
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
            {
                st.cache.touch(&c);
                return p.load(&c.to_string_lossy(), &title).map(|_| false);
            }
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
pub(crate) async fn sc_stream(app: &AppHandle, st: &AppState, id: &str) -> Res<streams::Stream> {
    const REFERRER: &str = "https://soundcloud.com/";
    let token = sc_session(app).await;
    let url = st.sc.stream_url(token.as_deref(), id).await?;
    let s = streams::Stream { url, referrer: REFERRER.into() };
    st.streams.put(&format!("sc:{id}"), s.clone());
    Ok(s)
}
