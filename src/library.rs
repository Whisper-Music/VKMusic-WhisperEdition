//! Локальная музыка: треки в ~/Music/vk (теги читаются lofty).

use lofty::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const AUDIO_EXT: &[&str] = &["mp3", "m4a", "aac", "opus", "ogg", "oga", "flac", "wav", "webm", "mka"];

#[derive(Debug, Clone, Serialize)]
pub struct LocalTrack {
    pub path: String,
    pub artist: String,
    pub title: String,
    pub duration: u64,
    pub has_cover: bool,
    pub added: u64,
    /// формат по расширению: OPUS, M4A, FLAC, MP3…
    pub format: String,
    /// средний битрейт аудио, кбит/с (если lofty смог определить)
    pub bitrate: Option<u32>,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, out);
            }
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| AUDIO_EXT.contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(p);
        }
    }
}

/// «Исполнитель - Название.mp3» → (исполнитель, название)
fn from_filename(p: &Path) -> (String, String) {
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), stem),
    }
}

/// Теги уже прочитанных файлов: путь → (размер, время изменения, трек). Повторное сканирование папки
/// (обновление библиотеки) не разбирает неизменённые файлы заново.
static TAGS: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, (u64, u128, LocalTrack)>>> = std::sync::Mutex::new(None);

pub fn scan(dir: &Path) -> Vec<LocalTrack> {
    let mut files = Vec::new();
    walk(dir, 3, &mut files);
    let mut cache = TAGS.lock().unwrap().take().unwrap_or_default();
    let mut fresh = std::collections::HashMap::with_capacity(files.len());
    let mut out: Vec<LocalTrack> = files
        .into_iter()
        .map(|p| {
            let meta = std::fs::metadata(&p).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            if let Some((s, m, t)) = cache.remove(&p) {
                if s == size && m == mtime && mtime != 0 {
                    fresh.insert(p, (s, m, t.clone()));
                    return t;
                }
            }
            let t = read_track(&p);
            fresh.insert(p, (size, mtime, t.clone()));
            t
        })
        .collect();
    // пропавшие файлы из кэша уходят сами: в нём только найденные сейчас
    *TAGS.lock().unwrap() = Some(fresh);
    out.sort_by(|a, b| b.added.cmp(&a.added));
    out
}

/// Описание файла: исполнитель, название, длительность, обложка, формат, битрейт (теги — lofty).
fn read_track(p: &Path) -> LocalTrack {
    let p = p.to_path_buf();
    let (fa, ft) = from_filename(&p);
    let added = std::fs::metadata(&p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut t = LocalTrack {
        path: p.display().to_string(),
        artist: fa,
        title: ft,
        duration: 0,
        has_cover: false,
        added,
        format: p
            .extension()
            .map(|x| x.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default(),
        bitrate: None,
    };
    if let Ok(tagged) = lofty::read_from_path(&p) {
        let props = tagged.properties();
        t.duration = props.duration().as_secs();
        t.bitrate = props.audio_bitrate().filter(|b| *b > 0);
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(a) = tag.artist().filter(|a| !a.trim().is_empty()) {
                t.artist = a.to_string();
            }
            if let Some(x) = tag.title().filter(|x| !x.trim().is_empty()) {
                t.title = x.to_string();
            }
            t.has_cover = !tag.pictures().is_empty();
        }
    }
    t
}

/// Обложка из тегов файла: (данные, mime).
pub fn cover(path: &Path) -> Option<(Vec<u8>, &'static str)> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pic = tag.pictures().first()?;
    let data = pic.data().to_vec();
    let mime = if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if data.starts_with(b"RIFF") {
        "image/webp"
    } else {
        "image/jpeg"
    };
    Some((data, mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescan_uses_cached_tags() {
        let dir = std::env::temp_dir().join(format!("vkmusic-lib-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Artist - Song.mp3");
        std::fs::write(&f, b"not really mp3").unwrap();
        let a = scan(&dir);
        assert_eq!((a[0].artist.as_str(), a[0].title.as_str()), ("Artist", "Song"));
        // тот же файл — из кэша, тот же результат
        let b = scan(&dir);
        assert_eq!(b[0].path, a[0].path);
        assert!(TAGS.lock().unwrap().as_ref().unwrap().contains_key(&f));
        // файл пропал — пропал и из кэша
        std::fs::remove_file(&f).unwrap();
        assert!(scan(&dir).is_empty());
        assert!(TAGS.lock().unwrap().as_ref().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Путь лежит внутри папки музыки (после разворачивания ссылок)?
pub fn inside(dir: &Path, path: &Path) -> bool {
    match (dir.canonicalize(), path.canonicalize()) {
        (Ok(d), Ok(p)) => p.starts_with(d),
        _ => false,
    }
}
