//! Сохранённые треки сервисов (library.json), синхронизация, порядок библиотеки.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── сохранённые треки сервисов ─────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn saved_list(st: State<'_, AppState>) -> Vec<saved::Saved> {
    st.saved.list()
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn saved_add(st: State<'_, AppState>, mut track: saved::Saved) -> Res<saved::Saved> {
    // access_key трека из микса/плейлиста — из ответа API (в памяти Rust), не из интерфейса
    track.access_key = if track.service == "vk" && track.api { st.vk.access_key(&track.id) } else { None };
    let s = st.saved.add(track)?;
    st.order.seen([saved_key(&s)])?;
    Ok(s)
}

/// Синхронизация с аккаунтом: «Мои треки» ВК / лайки SoundCloud → библиотека. Без входа — ошибка
/// «не вошли» (без сетевых запросов). Возвращает библиотеку целиком.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn saved_sync(app: AppHandle, st: State<'_, AppState>, service: String) -> Res<Vec<saved::Saved>> {
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
pub(crate) async fn vk_delete(app: AppHandle, st: State<'_, AppState>, id: String) -> Res<()> {
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
pub(crate) fn saved_remove(st: State<'_, AppState>, service: String, id: String) -> Res<()> {
    st.saved.remove(&service, &id)?;
    st.cache.remove(&cache::service_key(&service, &id));
    let key = format!("{service}:{id}");
    st.order.retain(|k| k != key)
}
