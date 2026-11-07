//! ВКонтакте: прослушивание, VK Микс и плейлисты (API с web-токеном сайта).
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── ВКонтакте: только прослушивание ─────────────────────────

/// Сессия ВК из хранилища WebKit (общего для всех webview приложения). Как в NK-Player: вход есть, если
/// у vk.ru / vk.com есть кука remixsid или remixnsid. Вызывать не из главного потока.
pub(crate) fn vk_cookie(app: &AppHandle) -> Res<Option<vk::Session>> {
    let all = all_cookies(app)?;
    let Some(base) = vk::session_base(all.iter().map(|c| (c.name.as_str(), c.value.as_str(), c.domain.as_str()))) else {
        return Ok(None);
    };
    Ok(Some(vk::Session { base, cookies: vk::vk_cookies(all) }))
}

pub(crate) async fn vk_session(app: &AppHandle) -> Res<vk::Session> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || vk_cookie(&app))
        .await
        .map_err(|e| e.to_string())??
        .ok_or_else(|| "Вы не вошли во ВКонтакте".to_string())
}

#[derive(Serialize)]
pub(crate) struct VkStatus {
    logged_in: bool,
    profile: Option<vk::Profile>,
    /// вход есть, но профиль не получен (нет сети, ограничение ВК)
    error: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_status(app: AppHandle, st: State<'_, AppState>) -> Res<VkStatus> {
    let Ok(session) = vk_session(&app).await else {
        st.vk.forget();
        return Ok(VkStatus { logged_in: false, profile: None, error: None });
    };
    match st.vk.profile(&session).await {
        Ok(p) => Ok(VkStatus { logged_in: true, profile: Some(p), error: None }),
        Err(e) => Ok(VkStatus { logged_in: true, profile: None, error: Some(e) }),
    }
}


// ───────────────────────── ВК: VK Микс и плейлисты (API с web-токеном сайта) ─────────────────────────

/// Ссылки на потоки треков из API — в кэш ссылок (`vkapi:<id>`), интерфейсу — только описания треков.
pub(crate) fn keep_streams(st: &AppState, s: &vk::Session, tracks: Vec<(vk::Track, Option<String>)>) -> Vec<vk::Track> {
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
pub(crate) async fn vk_home(app: AppHandle, st: State<'_, AppState>) -> Res<vk::Home> {
    let session = vk_session(&app).await?;
    st.vk.home(&session).await
}

/// Ваши плейлисты ВК — в библиотеке над треками (при открытии библиотеки и по кнопке обновления).
/// Ответ сохраняется на диск (`playlists.json`) вместе с access_key плейлистов.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_my_playlists(app: AppHandle, st: State<'_, AppState>) -> Res<Vec<vk::ApiPlaylist>> {
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
pub(crate) fn vk_my_playlists_cached(st: State<'_, AppState>) -> Option<Vec<vk::ApiPlaylist>> {
    st.playlists.list()
}

/// Треки плейлиста ВК (страница из 200). Ваши плейлисты сохраняются на диск.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_playlist(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, offset: Option<u64>) -> Res<vk::Page> {
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
pub(crate) fn vk_playlist_cached(st: State<'_, AppState>, owner_id: i64, id: i64) -> Option<vk::Page> {
    st.playlists.tracks(owner_id, id).map(|s| vk::Page { tracks: s.tracks, next_offset: s.next, more: None })
}

/// Следующие треки микса (по включению микса и когда очередь микса подходит к концу).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_mix(app: AppHandle, st: State<'_, AppState>, mix_id: String) -> Res<Vec<vk::Track>> {
    if mix_id.is_empty() || mix_id.len() > 40 || !mix_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("Неизвестный микс".into());
    }
    let session = vk_session(&app).await?;
    let tracks = st.vk.mix(&session, &mix_id).await?;
    Ok(keep_streams(&st, &session, tracks))
}

// ── свои плейлисты ВК: создать, переименовать, удалить, треки в плейлист и из него (просьба владельца, 2026-11-02) ──

/// id трека ВК «owner_id_id».
pub(crate) fn valid_vk_id(id: &str) -> bool {
    let mut it = id.splitn(2, '_');
    id.len() <= 40
        && it.next().is_some_and(|o| o.trim_start_matches('-').parse::<u64>().is_ok())
        && it.next().is_some_and(|i| i.parse::<u64>().is_ok())
}

/// Название плейлиста: без управляющих символов, 1–128 знаков.
pub(crate) fn playlist_title(t: &str) -> Res<String> {
    let t: String = t.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if t.is_empty() || t.chars().count() > 128 {
        return Err("Название плейлиста — от 1 до 128 символов".into());
    }
    Ok(t)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_playlist_create(app: AppHandle, st: State<'_, AppState>, title: String) -> Res<vk::ApiPlaylist> {
    let title = playlist_title(&title)?;
    let session = vk_session(&app).await?;
    let pl = st.vk.playlist_create(&session, &title).await?;
    st.playlists.added(&pl);
    Ok(pl)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_playlist_rename(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, title: String) -> Res<()> {
    let title = playlist_title(&title)?;
    let session = vk_session(&app).await?;
    st.vk.playlist_rename(&session, owner_id, id, &title).await?;
    st.playlists.renamed(owner_id, id, &title);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_playlist_delete(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64) -> Res<()> {
    let session = vk_session(&app).await?;
    st.vk.playlist_delete(&session, owner_id, id).await?;
    st.playlists.removed(owner_id, id);
    Ok(())
}

/// Трек ВК (из библиотеки, поиска, микса, плейлиста) — в свой плейлист.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_playlist_add(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, track_id: String) -> Res<()> {
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
pub(crate) async fn vk_playlist_remove(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64, track_id: String) -> Res<()> {
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
pub(crate) async fn vk_playlist_follow(app: AppHandle, st: State<'_, AppState>, owner_id: i64, id: i64) -> Res<()> {
    let session = vk_session(&app).await?;
    st.vk.playlist_follow(&session, owner_id, id).await
}

/// Играть трек из микса или плейлиста: ссылка из ответа API (в кэше ссылок), истекла — новая через audio.getById.
/// Такой трек, добавленный в библиотеку, играет из кэша треков и кэшируется, как остальные (вернёт true).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_play_api(app: AppHandle, st: State<'_, AppState>, id: String, title: Option<String>, fresh: Option<bool>) -> Res<bool> {
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
            {
                st.cache.touch(&c);
                return p.load(&c.to_string_lossy(), &title).map(|_| false);
            }
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
    // поток — через ретранслятор (relay.rs): ссылка CDN подписана под IP запроса к API
    let s = via_relay(s);
    p.load_stream(&s.url, &title, vk::USER_AGENT, &s.referrer)?;
    if let (Some(k), Some(t)) = (ckey, saved) {
        st.cache.enqueue(cache_job(k, &s, &t));
        return Ok(true);
    }
    Ok(false)
}

/// Ссылка на поток трека из API — локальным адресом ретранслятора (mp3 целиком, не HLS: относительные адреса
/// сегментов через него не открылись бы — поэтому только для `vkapi:`).
pub(crate) fn via_relay(s: streams::Stream) -> streams::Stream {
    streams::Stream { url: relay::url_for(&s.url, &s.referrer), referrer: s.referrer }
}

/// Новая ссылка на трек из API (audio.getById) — сразу в кэш ссылок.
pub(crate) async fn vk_api_stream(app: &AppHandle, st: &AppState, id: &str, access_key: Option<&str>) -> Res<streams::Stream> {
    let session = vk_session(app).await?;
    let url = st.vk.api_stream(&session, id, access_key).await?;
    let s = streams::Stream { url, referrer: session.referrer() };
    st.streams.put(&format!("vkapi:{id}"), s.clone());
    Ok(s)
}
