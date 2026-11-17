//! Синхронизация библиотеки через Whisper Archive (просьба владельца, 2026-11-17; сервер — `/api/library/vk`,
//! описание — LIBRARY-SYNC.md архива, Rust запросов — `whisper::library` ядра). Синхронизируются файлы из папки
//! музыки (скачанные по ссылке и свои): треки ВКонтакте и SoundCloud и так лежат в аккаунтах сервисов.
//!
//! Какой файл — какая запись: `key` — `f-` + 32 знака sha256 содержимого (одинаковый файл на двух устройствах —
//! одна запись). Восстановленный из архива файл получает key записи (его содержимое может отличаться: формат,
//! который архив не берёт, перед загрузкой перекладывается в ogg/m4a), поэтому соответствие «путь → key» хранится
//! в `~/.config/vkmusic/libsync.json` (не в кэше: без него восстановленные треки стали бы новыми записями).
//!
//! Решает, что отправить и что забрать, интерфейс (`libsync.js`); пути он не видит — только key. Rust сам находит
//! файл по key, читает и пишет только внутри папки музыки (`library::inside`).

use crate::os::Private;
use crate::whisper::{self, upload, WaResponse};
use crate::{cache, library, AppState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

type Res<T> = Result<T, String>;

/// Площадка библиотеки VK Music на сервере архива.
const SERVICE: &str = "vk";

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
struct Entry {
    size: u64,
    mtime: u64,
    key: String,
}

/// путь файла → (размер, время изменения, key); неизменённые файлы второй раз не читаются
pub struct Index {
    map: Mutex<Option<HashMap<String, Entry>>>,
    path: PathBuf,
}

impl Index {
    pub fn new() -> Index {
        Index { map: Mutex::new(None), path: crate::config::config_dir().join("libsync.json") }
    }

    fn load(&self) -> HashMap<String, Entry> {
        let mut g = self.map.lock().unwrap();
        if g.is_none() {
            *g = Some(std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
        }
        g.clone().unwrap_or_default()
    }

    fn store(&self, m: HashMap<String, Entry>) -> Res<()> {
        let data = serde_json::to_vec(&m).map_err(|e| e.to_string())?;
        *self.map.lock().unwrap() = Some(m);
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .private()
                .open(&tmp)
                .map_err(|e| format!("Не удалось сохранить состояние синхронизации: {e}"))?;
            f.write_all(&data).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// Файл записи `key` (только существующий и внутри папки музыки).
    fn path_of(&self, dir: &Path, key: &str) -> Option<PathBuf> {
        self.load()
            .into_iter()
            .find(|(_, e)| e.key == key)
            .map(|(p, _)| PathBuf::from(p))
            .filter(|p| p.is_file() && library::inside(dir, p))
    }
}

fn stat(p: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(p).ok()?;
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((m.len(), mtime))
}

/// key по содержимому: `f-` + первые 32 знака sha256
pub fn content_key(p: &Path) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let hex: String = h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect();
    Some(format!("f-{hex}"))
}

#[derive(Serialize)]
pub struct LibLocal {
    key: String,
    /// ключ трека в библиотеке интерфейса (путь файла) — чтобы поставить восстановленный трек на место
    lib: String,
    title: String,
    artist: String,
    duration_ms: u64,
    /// когда файл появился (мс): время лайка записи, если своего ещё нет
    added_ms: u64,
}

/// Файлы папки музыки с их key. Читает только новые и изменённые файлы (остальное — из libsync.json).
#[tauri::command(rename_all = "snake_case")]
pub async fn libsync_local(st: State<'_, AppState>) -> Res<Vec<LibLocal>> {
    let dir = st.music_dir();
    let index = st.libsync.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !dir.is_dir() {
            return Err("Папки музыки нет — синхронизация подождёт".to_string());
        }
        let old = index.load();
        let mut map = HashMap::new();
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in library::scan(&dir) {
            let p = PathBuf::from(&t.path);
            let Some((size, mtime)) = stat(&p) else { continue };
            let key = match old.get(&t.path) {
                Some(e) if e.size == size && e.mtime == mtime => e.key.clone(),
                _ => match content_key(&p) {
                    Some(k) => k,
                    None => continue,
                },
            };
            map.insert(t.path.clone(), Entry { size, mtime, key: key.clone() });
            // два одинаковых файла — одна запись
            if !seen.insert(key.clone()) {
                continue;
            }
            out.push(LibLocal {
                key,
                lib: t.path,
                title: t.title,
                artist: t.artist,
                duration_ms: t.duration * 1000,
                added_ms: t.added * 1000,
            });
        }
        if map != old {
            index.store(map)?;
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Отправить файл записи в архив (запись уже должна быть на сервере). Формат, который архив не берёт, —
/// перекладывается без перекодирования (как «Сохранить в архив»).
#[tauri::command(rename_all = "snake_case")]
pub async fn libsync_upload(st: State<'_, AppState>, key: String) -> Res<WaResponse> {
    if !whisper::library::key_ok(&key) {
        return Err("Некорректный номер трека библиотеки".into());
    }
    let dir = st.music_dir();
    let path = st.libsync.path_of(&dir, &key).ok_or("Файла этого трека больше нет в папке музыки")?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("track").to_string();
    let bytes = tauri::async_runtime::spawn_blocking(move || crate::whisper_vk::accepted_audio(&path))
        .await
        .map_err(|e| e.to_string())??;
    whisper::library::upload_file(SERVICE, &key, &stem, &bytes).await
}

/// Забрать файл записи из архива в папку музыки (имя — «Исполнитель - Название»). Уже есть — вернёт его.
/// Возвращает ключ трека в библиотеке интерфейса (путь файла).
#[tauri::command(rename_all = "snake_case")]
pub async fn libsync_restore(st: State<'_, AppState>, key: String, artist: String, title: String) -> Res<String> {
    if !whisper::library::key_ok(&key) {
        return Err("Некорректный номер трека библиотеки".into());
    }
    let dir = st.music_dir();
    if let Some(p) = st.libsync.path_of(&dir, &key) {
        return Ok(p.to_string_lossy().into_owned());
    }
    let bytes = whisper::library::download_file(SERVICE, &key).await?;
    let (ext, _) = upload::audio_kind(&bytes).ok_or("Архив отдал файл в незнакомом формате")?;
    let index = st.libsync.clone();
    let clip = |s: &str| s.chars().filter(|c| !c.is_control()).take(200).collect::<String>();
    let stem = cache::file_name(&clip(&artist), &clip(&title));
    tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку музыки: {e}"))?;
        let path = cache::free_path(&dir, &stem, ext);
        let tmp = dir.join(format!(".{key}.part"));
        std::fs::write(&tmp, &bytes)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("Не удалось сохранить трек: {e}")
            })?;
        let (size, mtime) = stat(&path).ok_or("Не удалось сохранить трек")?;
        let p = path.to_string_lossy().into_owned();
        let mut m = index.load();
        m.insert(p.clone(), Entry { size, mtime, key });
        index.store(m)?;
        Ok(p)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Запись удалили на другом устройстве — удалить её файл из папки музыки (интерфейс проверил, что трек здесь
/// не меняли после прошлой синхронизации). Файла уже нет — ничего не делает.
#[tauri::command(rename_all = "snake_case")]
pub fn libsync_delete(st: State<'_, AppState>, key: String) -> Res<()> {
    if !whisper::library::key_ok(&key) {
        return Err("Некорректный номер трека библиотеки".into());
    }
    let dir = st.music_dir();
    let Some(path) = st.libsync.path_of(&dir, &key) else { return Ok(()) };
    let ckey = cache::local_key(&path);
    std::fs::remove_file(&path).map_err(|e| format!("Не удалось удалить {}: {e}", path.display()))?;
    let p = path.to_string_lossy().into_owned();
    let mut m = st.libsync.load();
    m.remove(&p);
    st.libsync.store(m)?;
    st.order.retain(|k| k != p)?;
    if let Some(k) = ckey {
        st.cache.remove(&k);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_content() {
        let dir = std::env::temp_dir().join(format!("vkmusic-libsync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.mp3"), dir.join("b.mp3"), dir.join("c.mp3"));
        std::fs::write(&a, b"ID3 one").unwrap();
        std::fs::write(&b, b"ID3 one").unwrap();
        std::fs::write(&c, b"ID3 two").unwrap();
        let ka = content_key(&a).unwrap();
        assert!(whisper::library::key_ok(&ka) && ka.len() == 34);
        assert_eq!(ka, content_key(&b).unwrap());
        assert_ne!(ka, content_key(&c).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
}
