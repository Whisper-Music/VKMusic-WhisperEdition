//! Whisper Archive и VK Music: что архив делает с треками именно этого приложения. Общая часть (ключ, вход,
//! загрузка, комнаты) — в whisper/ ядра, она про ВКонтакте и mpv ничего не знает.
//!
//! * `wa_save_track` — «Сохранить в архив»: файл трека библиотеки берёт Rust сам по ключу (файл из папки музыки или
//!   трек сервиса из кэша — library_file), адрес файла интерфейс не передаёт. Обложка — embedded из файла,
//!   с CDN ВКонтакте / SoundCloud (только `whisper_host::cover_ok`) или нарисованная интерфейсом.
//! * `wa_play` — играть трек из архива: звук играет mpv, а не <audio>, поэтому протокол wa:// ему не годится —
//!   файл скачивается целиком в ~/.cache/vkmusic/archive (ключ в mpv не попадает) и играет оттуда.

use crate::whisper::{self, upload, WaResponse};
use crate::{library, AppState};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

type Res<T> = Result<T, String>;

/// Сколько треков архива держать на диске (самые старые удаляются).
const ARCHIVE_KEEP: usize = 30;

/// Сохранить трек библиотеки в архив. `album_id` — в этот альбом (владельца ключа), иначе новый приватный
/// альбом `album_title` с обложкой.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn wa_save_track(
    app: AppHandle,
    st: State<'_, AppState>,
    key: String,
    file_name: String,
    album_id: Option<String>,
    album_title: Option<String>,
    cover_url: Option<String>,
    cover_bytes: Option<Vec<u8>>,
) -> Res<WaResponse> {
    // ключ, id альбома и бюджет загрузок — до выкачивания трека: без них качать незачем
    let up = upload::begin(album_id.as_deref(), cover_bytes.as_ref().map(|b| b.len()))?;
    let (path, _) = crate::library_file(&app, &st, &key).await?;
    let src = path.clone();
    let audio = tauri::async_runtime::spawn_blocking(move || accepted_audio(&src)).await.map_err(|e| e.to_string())??;
    if audio.len() > upload::MAX_TRACK_UPLOAD {
        return Err("Трек больше 100 МБ — архив такие не принимает".into());
    }
    let track = upload::TrackFile::new(&file_name, &audio).ok_or("Архив не принимает формат этого трека")?;
    if let Some(id) = album_id {
        return up.to_album(&id, track).await;
    }
    let title = upload::album_title(album_title.as_deref())?;
    let cover = match (cover_url.filter(|u| crate::whisper_host::cover_ok(u)), cover_bytes) {
        (Some(u), _) => fetch_cover(&st.http, &u).await?,
        (None, Some(b)) => b,
        (None, None) => {
            let p = path.clone();
            tauri::async_runtime::spawn_blocking(move || library::cover(&p).map(|c| c.0))
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Для нового альбома нужна обложка")?
        }
    };
    up.new_album(&title, &cover, track).await
}

/// Файл в формате, который принимает архив (mp3, flac, ogg, wav, m4a). Остальное (opus в webm, aac в ts и т. п.)
/// перекладывается ffmpeg без перекодирования (`-c copy`): opus/vorbis — в ogg, остальное — в m4a.
fn accepted_audio(src: &Path) -> Res<Vec<u8>> {
    let bytes = std::fs::read(src).map_err(|e| format!("Не удалось прочитать трек: {e}"))?;
    if upload::audio_kind(&bytes).is_some() {
        return Ok(bytes);
    }
    let tmp = std::env::temp_dir().join(format!("vkmusic-wa-{}", std::process::id()));
    for (fmt, ext) in [("ogg", "ogg"), ("ipod", "m4a")] {
        let out = tmp.with_extension(ext);
        let ok = crate::os::tool("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(src)
            .args(["-map", "0:a:0", "-c", "copy", "-f", fmt])
            .arg(&out)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let data = std::fs::read(&out).ok();
        let _ = std::fs::remove_file(&out);
        if let Some(d) = data.filter(|d| ok && upload::audio_kind(d).is_some()) {
            return Ok(d);
        }
    }
    Err("Архив не принимает формат этого трека, а переложить его без перекодирования не вышло (нужен ffmpeg)".into())
}

pub(crate) async fn fetch_cover(http: &reqwest::Client, url: &str) -> Res<Vec<u8>> {
    let resp = http.get(url).send().await.map_err(|e| format!("Не удалось скачать обложку: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Не удалось скачать обложку: HTTP {}", resp.status().as_u16()));
    }
    if resp.content_length().is_some_and(|n| n as usize > upload::MAX_COVER_UPLOAD) {
        return Err("Обложка слишком большая".into());
    }
    let b = resp.bytes().await.map_err(|e| format!("Не удалось скачать обложку: {e}"))?;
    if b.len() > upload::MAX_COVER_UPLOAD {
        return Err("Обложка слишком большая".into());
    }
    Ok(b.to_vec())
}

fn archive_dir(st: &AppState) -> PathBuf {
    st.cache.dir().parent().map(|p| p.join("archive")).unwrap_or_else(|| std::env::temp_dir().join("vkmusic-archive"))
}

fn uuid_ok(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { *c == b'-' } else { c.is_ascii_hexdigit() })
}

/// Играть трек из архива (id — из Whisper.sources ядра). Уже скачанный играет сразу с диска.
#[tauri::command(rename_all = "snake_case")]
pub async fn wa_play(st: State<'_, AppState>, id: String, title: Option<String>) -> Res<()> {
    if !uuid_ok(&id) {
        return Err("Некорректный id трека".into());
    }
    let p = st.player()?;
    st.cache.begin(); // новое включение: ожидающая задача кэша библиотеки больше не нужна
    let dir = archive_dir(&st);
    let id = id.to_ascii_lowercase();
    let have = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(id.as_str()))
    });
    let path = match have {
        Some(p) => p,
        None => {
            let bytes = whisper::wa_track_bytes(&id).await?;
            let (ext, _) = upload::audio_kind(&bytes).unwrap_or(("bin", ""));
            let d = dir.clone();
            let file = d.join(format!("{id}.{ext}"));
            let (f, id) = (file.clone(), id.clone());
            tauri::async_runtime::spawn_blocking(move || -> Res<()> {
                std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                let tmp = d.join(format!("{id}.{ext}.part")); // имя не совпадает с готовым файлом по file_stem
                std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &f)).map_err(|e| format!("Не удалось сохранить трек: {e}"))?;
                prune(&d, ARCHIVE_KEEP);
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())??;
            file
        }
    };
    p.load(&path.to_string_lossy(), &title.unwrap_or_default())
}

/// Оставить в папке `keep` самых новых файлов.
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::uuid_ok;

    #[test]
    fn uuids() {
        assert!(uuid_ok("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(!uuid_ok("0f8fad5b-d9cb-469f-a165-70867728950"));
        assert!(!uuid_ok("../../../../etc/passwd-aaaaaaaaaaaaaaaa"));
        assert!(!uuid_ok("0f8fad5bxd9cb-469f-a165-70867728950e"));
    }
}
