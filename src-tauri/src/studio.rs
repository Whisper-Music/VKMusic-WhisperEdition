//! «Студия» (как у Whisper Yandex, просьба владельца 2026-11-17): своя версия трека библиотеки — пока обрезка.
//! Волну рисует и отрезок даёт послушать интерфейс (`studio.js`, Web Audio — отдельно от плеера) по файлу трека
//! (`studio_source`). Сохранение — ffmpeg без перекодирования (`-c copy`: формат и качество как у исходника), новый
//! файл — в папку музыки, рядом с остальными: дальше это обычный трек библиотеки («сердце», скачать, архив,
//! синхронизация). Файл берёт Rust сам по ключу трека библиотеки (`library_file`), как «Сохранить в архив».
//! На телефоне студии нет: там нет ffmpeg.

use crate::{cache, library_file, AppState};
use std::path::Path;
use tauri::{AppHandle, State};

type Res<T> = Result<T, String>;

/// Файл больше — в студию не берём (весь файл декодируется в памяти страницы).
const MAX_SOURCE: u64 = 200 * 1024 * 1024;
/// Самый короткий отрезок, мс.
const MIN_LEN_MS: u64 = 1000;

/// Файл трека библиотеки «сырыми» байтами (в JS — ArrayBuffer): по нему рисуется волна и слушается отрезок.
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_source(app: AppHandle, st: State<'_, AppState>, key: String) -> Res<tauri::ipc::Response> {
    let (path, _) = library_file(&app, &st, &key).await?;
    let bytes = tauri::async_runtime::spawn_blocking(move || -> Res<Vec<u8>> {
        let len = std::fs::metadata(&path).map_err(|e| format!("Не удалось открыть трек: {e}"))?.len();
        if len > MAX_SOURCE {
            return Err("Трек больше 200 МБ — студия такие не открывает".into());
        }
        std::fs::read(&path).map_err(|e| format!("Не удалось прочитать трек: {e}"))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

fn mmss(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}.{:02}", s / 60, s % 60)
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// Аргументы ffmpeg для отрезка [start, end) мс без перекодирования. `title` / `artist` — теги новой версии.
fn cut_args(src: &Path, out: &Path, start_ms: u64, end_ms: u64, title: &str, artist: &str) -> Vec<std::ffi::OsString> {
    let sec = |ms: u64| format!("{}.{:03}", ms / 1000, ms % 1000);
    let mut a: Vec<std::ffi::OsString> = ["-v", "error", "-y", "-i"].iter().map(Into::into).collect();
    a.push(src.into());
    for x in ["-ss".to_string(), sec(start_ms), "-to".into(), sec(end_ms), "-map".into(), "0:a:0".into(), "-map_metadata".into(), "0".into(), "-c".into(), "copy".into()] {
        a.push(x.into());
    }
    if !title.is_empty() {
        a.push("-metadata".into());
        a.push(format!("title={title}").into());
    }
    if !artist.is_empty() {
        a.push("-metadata".into());
        a.push(format!("artist={artist}").into());
    }
    a.push(out.into());
    a
}

/// Сохранить отрезок трека [start_ms, end_ms) новой версией в папку музыки. `title` / `artist` — как трек
/// показан в интерфейсе (только для тегов и имени файла). Возвращает ключ новой версии в библиотеке (путь файла).
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_save(
    app: AppHandle,
    st: State<'_, AppState>,
    key: String,
    start_ms: u64,
    end_ms: u64,
    title: String,
    artist: String,
) -> Res<String> {
    if end_ms < start_ms + MIN_LEN_MS || end_ms > 24 * 3600 * 1000 {
        return Err("Отрезок — не короче секунды".into());
    }
    let (src, _) = library_file(&app, &st, &key).await?;
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| matches!(e.as_str(), "mp3" | "m4a" | "aac" | "ogg" | "opus" | "oga" | "webm" | "flac" | "wav" | "mka"))
        .ok_or("Формат этого трека студия не обрезает")?;
    let (title, artist) = (clean(&title, 200), clean(&artist, 200));
    let vtitle = format!("{} (обрезка {}–{})", if title.is_empty() { "Трек" } else { &title }, mmss(start_ms), mmss(end_ms));
    let dir = st.music_dir();
    let stem = cache::file_name(&artist, &vtitle);
    tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку музыки: {e}"))?;
        let out = cache::free_path(&dir, &stem, &ext);
        let tmp = dir.join(format!(".studio-{}.{ext}", std::process::id()));
        let res = crate::os::tool("ffmpeg")
            .args(cut_args(&src, &tmp, start_ms, end_ms, &vtitle, &artist))
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("Для студии нужен ffmpeg: {e}"))?;
        if !res.status.success() || std::fs::metadata(&tmp).map(|m| m.len() == 0).unwrap_or(true) {
            let _ = std::fs::remove_file(&tmp);
            let why = String::from_utf8_lossy(&res.stderr).lines().last().unwrap_or("").chars().take(200).collect::<String>();
            return Err(format!("ffmpeg не обрезал трек: {why}"));
        }
        std::fs::rename(&tmp, &out).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("Не удалось сохранить версию: {e}")
        })?;
        Ok(out.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args() {
        let a = cut_args(Path::new("/m/a.opus"), Path::new("/m/b.opus"), 12_345, 75_000, "Т (обрезка)", "");
        let s: Vec<String> = a.iter().map(|x| x.to_string_lossy().into_owned()).collect();
        assert!(s.windows(2).any(|w| w == ["-ss", "12.345"]));
        assert!(s.windows(2).any(|w| w == ["-to", "75.000"]));
        assert!(s.windows(2).any(|w| w == ["-c", "copy"]));
        assert!(s.contains(&"title=Т (обрезка)".to_string()));
        assert!(!s.iter().any(|x| x.starts_with("artist=")));
        assert_eq!(s.last().unwrap(), "/m/b.opus");
        assert_eq!(mmss(75_000), "1.15");
    }

    /// Настоящий ffmpeg (если есть): 6 с синуса в opus и mp3 → отрезок 1–3,5 с без перекодирования.
    #[test]
    fn real_cut() {
        if crate::os::tool("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("vkmusic-studio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for ext in ["opus", "mp3"] {
            let src = dir.join(format!("src.{ext}"));
            let ok = crate::os::tool("ffmpeg")
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=6"])
                .arg(&src)
                .status()
                .unwrap()
                .success();
            assert!(ok);
            let out = dir.join(format!("out.{ext}"));
            let ok = crate::os::tool("ffmpeg").args(cut_args(&src, &out, 1000, 3500, "Т (обрезка)", "А")).status().unwrap().success();
            assert!(ok, "{ext}");
            let probe = crate::os::tool("ffprobe")
                .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
                .arg(&out)
                .output()
                .unwrap();
            let d: f64 = String::from_utf8_lossy(&probe.stdout).trim().parse().unwrap();
            assert!((d - 2.5).abs() < 0.2, "{ext}: {d}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
