//! «Студия» (как у Whisper Yandex, просьба владельца 2026-11-17): свои версии треков библиотеки — обрезки.
//! Волну рисует и отрезок даёт послушать интерфейс (`studio.js`, Web Audio — отдельно от плеера) по файлу трека
//! (`studio_source`); темп, тональность и громкость он же считает по звуку (`studioinfo.js`), о файле (формат, битрейт,
//! частота) — `studio_info`. Сохранение — ffmpeg без перекодирования (`-c copy`: формат и качество как у исходника),
//! файл — в папку музыки: дальше это обычный трек библиотеки («сердце», скачать, архив). Файл исходника Rust берёт сам
//! по ключу трека библиотеки (`library_file`), как «Сохранить в архив».
//!
//! «Мои версии» — `~/.config/vkmusic/studio.json`: номер версии `studio-<32 hex>` (он же key записи синхронизации
//! библиотеки — libsync.rs, `kind: cut` у обрезки трека ВК с `replaces` = id трека), откуда вырезана, границы,
//! название, локальное «любимое». Версия без файла (удалили в папке) из списка выпадает.

use crate::os::Private;
use crate::{cache, library, library_file, AppState};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, State};

type Res<T> = Result<T, String>;

/// Файл больше — в студию не берём (весь файл декодируется в памяти страницы).
const MAX_SOURCE: u64 = 200 * 1024 * 1024;
/// Самый короткий отрезок, мс.
const MIN_LEN_MS: u64 = 1000;
const MAX_ITEMS: usize = 500;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Version {
    /// 32 hex; key синхронизации — `studio-<id>`
    pub id: String,
    /// файл версии (он же ключ трека в библиотеке)
    pub path: String,
    /// ключ исходного трека в библиотеке (пусто — версию прислало другое устройство)
    #[serde(default)]
    pub src: String,
    /// id трека ВК (owner_id_id), если вырезана из него: в синхронизации это обрезка трека площадки (`kind: cut`)
    #[serde(default)]
    pub replaces: Option<String>,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub created: u64,
    #[serde(default)]
    pub liked: bool,
    /// обложка исходника (https CDN ВК / SoundCloud) — для альбома архива при «Поделиться»
    #[serde(default)]
    pub cover: Option<String>,
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

fn hex_ok(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn new_id() -> Res<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// id трека ВК из ключа библиотеки `vk:<owner>_<id>`.
fn vk_id(src: &str) -> Option<String> {
    src.strip_prefix("vk:").filter(|id| crate::valid_vk_id(id)).map(str::to_string)
}

pub struct Store {
    items: Mutex<Vec<Version>>,
    path: PathBuf,
}

impl Store {
    pub fn new() -> Store {
        let path = crate::config::config_dir().join("studio.json");
        let items = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<Version>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|v| hex_ok(&v.id))
            .collect();
        Store { items: Mutex::new(items), path }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Store {
        Store { items: Mutex::new(Vec::new()), path }
    }

    fn save(&self, items: &[Version]) -> Res<()> {
        let dir = self.path.parent().ok_or("Нет папки настроек")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(items).map_err(|e| e.to_string())?;
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .private()
                .open(&tmp)
                .map_err(|e| format!("Не удалось сохранить версии студии: {e}"))?;
            f.write_all(&data).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// Версии, чей файл ещё есть (новые сверху); пропавшие из списка убираются.
    pub fn list(&self) -> Vec<Version> {
        let mut g = self.items.lock().unwrap();
        let before = g.len();
        g.retain(|v| Path::new(&v.path).is_file());
        if g.len() != before {
            let _ = self.save(&g);
        }
        let mut out = g.clone();
        out.sort_by(|a, b| b.created.cmp(&a.created));
        out
    }

    fn get(&self, id: &str) -> Option<Version> {
        self.items.lock().unwrap().iter().find(|v| v.id == id).cloned()
    }

    /// Новая или пришедшая с другого устройства версия (тот же id — заменяется).
    pub fn put(&self, v: Version) -> Res<()> {
        let mut g = self.items.lock().unwrap();
        g.retain(|x| x.id != v.id);
        if g.len() >= MAX_ITEMS {
            return Err("В студии слишком много версий".into());
        }
        g.push(v);
        self.save(&g)
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Version)) -> Res<Version> {
        let mut g = self.items.lock().unwrap();
        let v = g.iter_mut().find(|v| v.id == id).ok_or("Такой версии нет")?;
        f(v);
        let out = v.clone();
        self.save(&g)?;
        Ok(out)
    }

    pub fn remove_path(&self, path: &str) -> Res<()> {
        let mut g = self.items.lock().unwrap();
        let before = g.len();
        g.retain(|v| v.path != path);
        if g.len() != before {
            self.save(&g)?;
        }
        Ok(())
    }
}

/// Версия, пришедшая синхронизацией (libsync_restore): номер — из key `studio-<id>`.
pub fn adopt(st: &AppState, key: &str, path: &str, title: &str, artist: &str, cut: Option<&CutMeta>) -> Res<()> {
    let Some(id) = key.strip_prefix("studio-").filter(|id| hex_ok(id)) else { return Ok(()) };
    let (start_ms, end_ms, replaces) = cut.map(|c| (c.start_ms, c.end_ms, c.replaces.clone())).unwrap_or((0, 0, None));
    st.studio.put(Version {
        id: id.to_string(),
        path: path.to_string(),
        src: replaces.as_ref().map(|r| format!("vk:{r}")).unwrap_or_default(),
        replaces: replaces.filter(|r| crate::valid_vk_id(r)),
        title: clean(title, 200),
        artist: clean(artist, 200),
        start_ms,
        end_ms,
        created: now(),
        liked: false,
        cover: None,
    })
}

/// Границы и источник обрезки (из записи синхронизации: `replaces`, `meta.cut`).
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct CutMeta {
    #[serde(default)]
    pub replaces: Option<String>,
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: u64,
}

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

#[derive(Serialize)]
pub struct FileInfo {
    /// MP3, OPUS, M4A… (по расширению файла)
    format: String,
    /// кодек по разбору lofty: Mpeg, Opus, Mp4, Flac…
    codec: String,
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    channels: Option<u8>,
    bit_depth: Option<u8>,
    size: u64,
    duration_ms: u64,
    /// файл папки музыки (а не кэш трека сервиса)
    local: bool,
}

/// О файле трека: формат, кодек, битрейт, частота, каналы, размер (lofty, без декодирования).
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_info(app: AppHandle, st: State<'_, AppState>, key: String) -> Res<FileInfo> {
    let (path, _) = library_file(&app, &st, &key).await?;
    let local = key.starts_with('/');
    tauri::async_runtime::spawn_blocking(move || -> Res<FileInfo> {
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        let format = path.extension().map(|x| x.to_string_lossy().to_ascii_uppercase()).unwrap_or_default();
        let mut out = FileInfo { format, codec: String::new(), bitrate: None, sample_rate: None, channels: None, bit_depth: None, size, duration_ms: 0, local };
        if let Ok(f) = lofty::read_from_path(&path) {
            use lofty::file::{AudioFile, TaggedFileExt};
            let p = f.properties();
            out.codec = format!("{:?}", f.file_type());
            out.bitrate = p.audio_bitrate().filter(|b| *b > 0);
            out.sample_rate = p.sample_rate();
            out.channels = p.channels();
            out.bit_depth = p.bit_depth();
            out.duration_ms = p.duration().as_millis() as u64;
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn mmss(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}.{:02}", s / 60, s % 60)
}

/// Аргументы ffmpeg для отрезка [start, end) мс без перекодирования. `title` / `artist` — теги новой версии.
fn cut_args(src: &Path, out: &Path, start_ms: u64, end_ms: u64, title: &str, artist: &str) -> Vec<std::ffi::OsString> {
    let sec = |ms: u64| format!("{}.{:03}", ms / 1000, ms % 1000);
    let mut a: Vec<std::ffi::OsString> = ["-v", "error", "-y", "-i"].iter().map(Into::into).collect();
    a.push(src.into());
    for x in ["-ss".to_string(), sec(start_ms), "-to".into(), sec(end_ms), "-map".into(), "0:a:0".into(), "-map_metadata".into(), "0".into(), "-c".into(), "copy".into()] {
        a.push(x.into());
    }
    tags(&mut a, title, artist);
    a.push(out.into());
    a
}

fn tags(a: &mut Vec<std::ffi::OsString>, title: &str, artist: &str) {
    if !title.is_empty() {
        a.push("-metadata".into());
        a.push(format!("title={title}").into());
    }
    if !artist.is_empty() {
        a.push("-metadata".into());
        a.push(format!("artist={artist}").into());
    }
}

/// ffmpeg во временный файл рядом и замена — без половинчатых файлов при ошибке.
fn ffmpeg_to(dir: &Path, out: &Path, ext: &str, args: impl FnOnce(&Path) -> Vec<std::ffi::OsString>) -> Res<()> {
    let tmp = dir.join(format!(".studio-{}-{}.{ext}", std::process::id(), now()));
    let res = crate::os::tool("ffmpeg")
        .args(args(&tmp))
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Для студии нужен ffmpeg: {e}"))?;
    if !res.status.success() || std::fs::metadata(&tmp).map(|m| m.len() == 0).unwrap_or(true) {
        let _ = std::fs::remove_file(&tmp);
        let why = String::from_utf8_lossy(&res.stderr).lines().last().unwrap_or("").chars().take(200).collect::<String>();
        return Err(format!("ffmpeg: {why}"));
    }
    std::fs::rename(&tmp, out).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Не удалось сохранить версию: {e}")
    })
}

fn audio_ext(p: &Path) -> Res<String> {
    p.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| matches!(e.as_str(), "mp3" | "m4a" | "aac" | "ogg" | "opus" | "oga" | "webm" | "flac" | "wav" | "mka"))
        .ok_or_else(|| "Формат этого трека студия не обрезает".to_string())
}

/// Сохранить отрезок трека [start_ms, end_ms) новой версией в папку музыки. `title` — название версии (пусто —
/// «<трек> (обрезка м.сс–м.сс)»), `artist` / `track_title` / `cover` — как трек показан в интерфейсе (теги, имя
/// файла и обложка альбома при «Поделиться»; обложка — только https).
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn studio_save(
    app: AppHandle,
    st: State<'_, AppState>,
    key: String,
    start_ms: u64,
    end_ms: u64,
    title: String,
    track_title: String,
    artist: String,
    cover: Option<String>,
) -> Res<Version> {
    if end_ms < start_ms + MIN_LEN_MS || end_ms > 24 * 3600 * 1000 {
        return Err("Отрезок — не короче секунды".into());
    }
    let (src, _) = library_file(&app, &st, &key).await?;
    let ext = audio_ext(&src)?;
    let (title, track, artist) = (clean(&title, 200), clean(&track_title, 200), clean(&artist, 200));
    let vtitle = if title.is_empty() {
        format!("{} (обрезка {}–{})", if track.is_empty() { "Трек" } else { &track }, mmss(start_ms), mmss(end_ms))
    } else {
        title
    };
    let dir = st.music_dir();
    let stem = cache::file_name(&artist, &vtitle);
    let (t2, a2) = (vtitle.clone(), artist.clone());
    let path = tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку музыки: {e}"))?;
        let out = cache::free_path(&dir, &stem, &ext);
        ffmpeg_to(&dir, &out, &ext, |tmp| cut_args(&src, tmp, start_ms, end_ms, &t2, &a2)).map_err(|e| format!("Не обрезалось — {e}"))?;
        Ok(out.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())??;
    let v = Version {
        id: new_id()?,
        path,
        replaces: vk_id(&key),
        src: key,
        title: vtitle,
        artist,
        start_ms,
        end_ms,
        created: now(),
        liked: false,
        cover: cover.filter(|u| crate::whisper_host::cover_ok(u)),
    };
    st.studio.put(v.clone())?;
    Ok(v)
}

/// «Мои версии» (новые сверху).
#[tauri::command(rename_all = "snake_case")]
pub fn studio_list(st: State<'_, AppState>) -> Vec<Version> {
    st.studio.list()
}

/// «Любимая» (локально) и название (тег title файла меняется тоже — ffmpeg без перекодирования).
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_update(st: State<'_, AppState>, id: String, liked: Option<bool>, title: Option<String>) -> Res<Version> {
    let v = st.studio.get(&id).ok_or("Такой версии нет")?;
    let title = title.map(|t| clean(&t, 200)).filter(|t| !t.is_empty() && *t != v.title);
    if let Some(t) = &title {
        let path = PathBuf::from(&v.path);
        if !library::inside(&st.music_dir(), &path) {
            return Err("Файл версии вне папки музыки".into());
        }
        let ext = audio_ext(&path)?;
        let (t2, a2) = (t.clone(), v.artist.clone());
        tauri::async_runtime::spawn_blocking(move || {
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            ffmpeg_to(&dir, &path, &ext, |tmp| {
                let mut a: Vec<std::ffi::OsString> = ["-v", "error", "-y", "-i"].iter().map(Into::into).collect();
                a.push(path.clone().into());
                for x in ["-map", "0", "-map_metadata", "0", "-c", "copy"] {
                    a.push(x.into());
                }
                tags(&mut a, &t2, &a2);
                a.push(tmp.into());
                a
            })
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Не переименовалось — {e}"))?;
    }
    st.studio.update(&id, |v| {
        if let Some(l) = liked {
            v.liked = l;
        }
        if let Some(t) = title {
            v.title = t;
        }
    })
}

/// Удалить версию — и её файл из папки музыки.
#[tauri::command(rename_all = "snake_case")]
pub fn studio_delete(st: State<'_, AppState>, id: String) -> Res<()> {
    let v = st.studio.get(&id).ok_or("Такой версии нет")?;
    let path = PathBuf::from(&v.path);
    if path.exists() {
        if !library::inside(&st.music_dir(), &path) {
            return Err("Файл версии вне папки музыки".into());
        }
        let ckey = cache::local_key(&path);
        std::fs::remove_file(&path).map_err(|e| format!("Не удалось удалить файл версии: {e}"))?;
        if let Some(k) = ckey {
            st.cache.remove(&k);
        }
    }
    st.studio.remove_path(&v.path)?;
    st.order.retain(|k| k != v.path)
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
        assert_eq!(vk_id("vk:-2001_5").as_deref(), Some("-2001_5"));
        assert_eq!(vk_id("sc:5"), None);
        assert!(hex_ok(&new_id().unwrap()));
    }

    #[test]
    fn store() {
        let dir = std::env::temp_dir().join(format!("vkmusic-studio-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("v.mp3");
        std::fs::write(&f, b"ID3").unwrap();
        let s = Store::at(dir.join("studio.json"));
        let v = Version {
            id: new_id().unwrap(),
            path: f.to_string_lossy().into_owned(),
            src: "vk:1_2".into(),
            replaces: Some("1_2".into()),
            title: "T".into(),
            artist: String::new(),
            start_ms: 0,
            end_ms: 2000,
            created: 1,
            liked: false,
            cover: None,
        };
        s.put(v.clone()).unwrap();
        assert_eq!(s.list()[0].id, v.id);
        assert!(s.update(&v.id, |x| x.liked = true).unwrap().liked);
        std::fs::remove_file(&f).unwrap();
        assert!(s.list().is_empty()); // файла нет — версии в списке нет
        let _ = std::fs::remove_dir_all(dir);
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
            ffmpeg_to(&dir, &out, ext, |tmp| cut_args(&src, tmp, 1000, 3500, "Т (обрезка)", "А")).unwrap();
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
