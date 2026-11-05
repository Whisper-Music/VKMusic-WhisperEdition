//! Кэш треков библиотеки: ~/.cache/vkmusic/tracks (просьба владельца, 2026-10-30).
//!
//! Кэшируются только треки библиотеки — файлы из папки музыки и сохранённые треки сервисов. Трек попадает
//! в кэш, когда его включили: файл копируется, поток сервиса один раз выкачивается ffmpeg без перекодирования
//! (`-c copy`, затем перекладывается в «родной» контейнер по кодеку: mp3, m4a, opus…). Дальше трек играет
//! из кэша, без запросов к сервису. Одновременно идёт одна задача; из ожидающих остаётся только последняя —
//! пролистанные треки не выкачиваются. Когда текущий трек в кэше, интерфейс просит закэшировать следующий
//! в очереди (`prefetch`, просьба владельца 2026-10-30): он ждёт после текущего, а новое включение трека
//! его отменяет (`begin` меняет поколение).
//!
//! Имена файлов — ключ + расширение: `vk-<owner>_<id>`, `sc-<id>`, `local-<хэш пути, размера и времени>`.

use crate::library::AUDIO_EXT;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

type Res<T> = Result<T, String>;

pub enum Source {
    /// файл из папки музыки
    File(PathBuf),
    /// поток сервиса по https
    Stream { url: String, referrer: String },
}

pub struct Job {
    pub key: String,
    /// ключ трека в библиотеке (путь файла | vk:/sc:+id) — для события «закэширован»
    pub lib: String,
    pub source: Source,
    pub artist: String,
    pub title: String,
}

pub struct TrackCache {
    dir: PathBuf,
    /// одна выкачка/копия за раз
    gate: Mutex<()>,
    queue: Mutex<Queue>,
    /// вызывается после каждой фоновой задачи: (ключ библиотеки, получилось ли)
    notify: OnceLock<Box<dyn Fn(&str, bool) + Send + Sync>>,
    /// предел размера, байт; 0 — без ограничения
    limit: std::sync::atomic::AtomicU64,
}

#[derive(Default)]
struct Queue {
    /// включённый трек — первым
    play: Option<Job>,
    /// следующий в очереди — после него
    next: Option<Job>,
    /// фоновый поток уже работает
    running: bool,
    /// растёт при каждом включении трека: устаревшая предзагрузка отбрасывается
    gen: u64,
}

/// Ключ трека сервиса: `vk-<owner>_<id>` / `sc-<id>`.
pub fn service_key(service: &str, id: &str) -> String {
    format!("{service}-{id}")
}

/// Ключ файла из папки музыки: путь + размер + время изменения — изменённый файл кэшируется заново.
pub fn local_key(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let mut h = fnv(path.as_os_str().as_encoded_bytes(), 0xcbf2_9ce4_8422_2325);
    h = fnv(&m.len().to_le_bytes(), h);
    h = fnv(&mtime.to_le_bytes(), h);
    Some(format!("local-{h:016x}"))
}

/// FNV-1a: стабилен между версиями Rust (в отличие от DefaultHasher) — имена в кэше не «сбегают».
fn fnv(data: &[u8], mut h: u64) -> u64 {
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 80 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Контейнер по кодеку: (расширение, формат ffmpeg). Незнакомое — в Matroska.
fn container_for(codec: &str) -> (&'static str, &'static str) {
    match codec {
        "mp3" => ("mp3", "mp3"),
        "aac" | "alac" => ("m4a", "ipod"),
        "opus" => ("opus", "opus"),
        "vorbis" => ("ogg", "ogg"),
        "flac" => ("flac", "flac"),
        _ => ("mka", "matroska"),
    }
}

/// Имя файла для папки загрузок: «Исполнитель - Название», без `/`, управляющих символов и точки в начале.
pub fn file_name(artist: &str, title: &str) -> String {
    let raw = if artist.trim().is_empty() { title.to_string() } else { format!("{} - {}", artist.trim(), title.trim()) };
    let s: String = raw
        .chars()
        .map(|c| if c == '/' || c.is_control() { '_' } else { c })
        .take(150)
        .collect();
    let s = s.trim().trim_start_matches('.').trim().to_string();
    if s.is_empty() {
        "track".into()
    } else {
        s
    }
}

/// Свободное имя в папке: «имя.ext», «имя (2).ext»…
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

fn run(cmd: &mut Command) -> Res<String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Для кэша и загрузки треков сервисов нужен ffmpeg (sudo pacman -S ffmpeg): {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ошибка ffmpeg");
        Err(format!("Не удалось получить трек: {}", last.chars().take(300).collect::<String>()))
    }
}

impl TrackCache {
    pub fn new() -> TrackCache {
        let base = dirs::cache_dir().unwrap_or_else(|| crate::config::home().join(".cache"));
        TrackCache::at(base.join("vkmusic").join("tracks"))
    }

    fn at(dir: PathBuf) -> TrackCache {
        TrackCache { dir, gate: Mutex::new(()), queue: Mutex::new(Queue::default()), notify: OnceLock::new(), limit: Default::default() }
    }

    pub fn on_done(&self, f: impl Fn(&str, bool) + Send + Sync + 'static) {
        let _ = self.notify.set(Box::new(f));
    }

    /// Включён трек: ожидающая предзагрузка прежнего «следующего» больше не нужна.
    pub fn begin(&self) -> u64 {
        let mut q = self.queue.lock().unwrap();
        q.gen += 1;
        q.next = None;
        q.gen
    }

    pub fn generation(&self) -> u64 {
        self.queue.lock().unwrap().gen
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Предел размера кэша (байт, 0 — без ограничения); сразу убирает лишнее.
    pub fn set_limit(&self, bytes: u64) {
        self.limit.store(bytes, std::sync::atomic::Ordering::Relaxed);
        self.prune("");
    }

    /// Кэш больше предела — удалить давно не игравшие (по времени изменения файла: включение из кэша его
    /// обновляет, `touch`), кроме `keep` (только что докачанный / играющий).
    pub fn prune(&self, keep: &str) {
        let limit = self.limit.load(std::sync::atomic::Ordering::Relaxed);
        if limit == 0 {
            return;
        }
        let mut e: Vec<_> = self
            .entries()
            .into_iter()
            .map(|(k, p, size)| {
                let t = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                (k, p, size, t)
            })
            .collect();
        let mut total: u64 = e.iter().map(|x| x.2).sum();
        e.sort_by_key(|x| x.3);
        for (k, p, size, _) in e {
            if total <= limit {
                break;
            }
            if k == keep {
                continue;
            }
            if std::fs::remove_file(&p).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    /// Трек включили из кэша — он «свежий» для `prune`.
    pub fn touch(&self, path: &Path) {
        if let Ok(f) = std::fs::File::options().write(true).open(path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
    }

    /// Файл трека в кэше, если он есть.
    pub fn get(&self, key: &str) -> Option<PathBuf> {
        if !valid_key(key) {
            return None;
        }
        AUDIO_EXT.iter().map(|x| self.dir.join(format!("{key}.{x}"))).find(|p| p.is_file())
    }

    pub fn remove(&self, key: &str) {
        while let Some(p) = self.get(key) {
            if std::fs::remove_file(p).is_err() {
                break;
            }
        }
    }

    /// Ключи готовых файлов кэша (без недокачанных).
    fn entries(&self) -> Vec<(String, PathBuf, u64)> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        rd.flatten()
            .filter_map(|e| {
                let p = e.path();
                let (stem, ext) = (p.file_stem()?.to_str()?.to_string(), p.extension()?.to_str()?);
                (valid_key(&stem) && AUDIO_EXT.contains(&ext)).then(|| {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    (stem, p.clone(), size)
                })
            })
            .collect()
    }

    /// Убрать треки, которых больше нет в библиотеке: keep(ключ) == false — удалить.
    pub fn retain(&self, keep: impl Fn(&str) -> bool) {
        for (key, path, _) in self.entries() {
            if !keep(&key) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// (число треков, байт)
    pub fn stats(&self) -> (usize, u64) {
        let e = self.entries();
        (e.len(), e.iter().map(|x| x.2).sum())
    }

    /// Удалить всё содержимое кэша (и ожидающую задачу).
    pub fn clear(&self) -> Res<()> {
        {
            let mut q = self.queue.lock().unwrap();
            q.play = None;
            q.next = None;
        }
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return Ok(()) };
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_file()) {
                std::fs::remove_file(e.path()).map_err(|err| format!("Не удалось очистить кэш: {err}"))?;
            }
        }
        Ok(())
    }

    /// Закэшировать включённый трек в фоне. Ждёт только последний — предыдущий ожидающий заменяется.
    pub fn enqueue(self: &Arc<Self>, job: Job) {
        if self.get(&job.key).is_some() {
            return;
        }
        let mut q = self.queue.lock().unwrap();
        q.play = Some(job);
        self.start(q);
    }

    /// Закэшировать следующий трек очереди — после включённого. gen — поколение на момент запроса:
    /// если с тех пор включили другой трек, задача устарела.
    pub fn prefetch(self: &Arc<Self>, job: Job, gen: u64) {
        if self.get(&job.key).is_some() {
            return;
        }
        let mut q = self.queue.lock().unwrap();
        if q.gen != gen {
            return;
        }
        q.next = Some(job);
        self.start(q);
    }

    fn start(self: &Arc<Self>, mut q: std::sync::MutexGuard<'_, Queue>) {
        if q.running {
            return;
        }
        q.running = true;
        drop(q);
        let me = self.clone();
        std::thread::spawn(move || loop {
            let job = {
                let mut q = me.queue.lock().unwrap();
                match q.play.take().or_else(|| q.next.take()) {
                    Some(j) => j,
                    None => {
                        q.running = false;
                        break;
                    }
                }
            };
            let res = me.fetch(&job);
            if let Err(e) = &res {
                eprintln!("vkmusic: кэш {}: {e}", job.key);
            } else {
                me.prune(&job.key);
            }
            if let Some(f) = me.notify.get() {
                f(&job.lib, res.is_ok());
            }
        });
    }

    /// Положить трек в кэш (если его там ещё нет) и вернуть путь. Блокирующий вызов.
    pub fn fetch(&self, job: &Job) -> Res<PathBuf> {
        if !valid_key(&job.key) {
            return Err("Неверный ключ кэша".into());
        }
        let _gate = self.gate.lock().unwrap();
        if let Some(p) = self.get(&job.key) {
            return Ok(p);
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("Не удалось создать {}: {e}", self.dir.display()))?;
        let key = &job.key;
        match &job.source {
            Source::File(src) => {
                let ext = src
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(str::to_ascii_lowercase)
                    .filter(|x| AUDIO_EXT.contains(&x.as_str()))
                    .ok_or("Неизвестный формат файла")?;
                let tmp = self.dir.join(format!("{key}.part"));
                let dst = self.dir.join(format!("{key}.{ext}"));
                std::fs::copy(src, &tmp).map_err(|e| format!("Не удалось скопировать в кэш: {e}"))?;
                std::fs::rename(&tmp, &dst).map_err(|e| e.to_string())?;
                Ok(dst)
            }
            Source::Stream { url, referrer } => {
                if !url.starts_with("https://") && !crate::relay::is_relay(url) {
                    return Err("Неверная ссылка на поток".into());
                }
                // 1) поток (mp3 или HLS) как есть — в Matroska, она примет любой кодек
                let raw = self.dir.join(format!("{key}.part1"));
                let res = run(crate::os::tool("ffmpeg")
                    .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"])
                    .args(["-rw_timeout", "30000000", "-user_agent", crate::vk::USER_AGENT, "-referer", referrer])
                    .args(["-i", url, "-map", "0:a:0", "-c", "copy", "-f", "matroska"])
                    .arg(&raw))
                // 2) кодек — и в «родной» контейнер с тегами (локально, без сети)
                .and_then(|_| {
                    run(crate::os::tool("ffprobe")
                        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name"])
                        .args(["-of", "default=nw=1:nk=1"])
                        .arg(&raw))
                })
                .and_then(|codec| {
                    let (ext, format) = container_for(codec.trim());
                    let tmp = self.dir.join(format!("{key}.part2"));
                    run(crate::os::tool("ffmpeg")
                        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
                        .arg(&raw)
                        .args(["-map", "0:a:0", "-c", "copy"])
                        .arg("-metadata")
                        .arg(format!("artist={}", job.artist))
                        .arg("-metadata")
                        .arg(format!("title={}", job.title))
                        .args(["-f", format])
                        .arg(&tmp))?;
                    let dst = self.dir.join(format!("{key}.{ext}"));
                    std::fs::rename(&tmp, &dst).map_err(|e| e.to_string())?;
                    Ok(dst)
                });
                let _ = std::fs::remove_file(&raw);
                let _ = std::fs::remove_file(self.dir.join(format!("{key}.part2")));
                res
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_names() {
        assert!(valid_key(&service_key("vk", "-1_2")));
        assert!(!valid_key("../x"));
        assert!(!valid_key("a.b"));
        assert_eq!(file_name(" A/C ", "T\u{7}"), "A_C - T_");
        assert_eq!(file_name("", "..hidden"), "hidden");
        assert_eq!(file_name("", "  "), "track");
        assert_eq!(container_for("aac"), ("m4a", "ipod"));
        assert_eq!(container_for("pcm_s16le"), ("mka", "matroska"));
    }

    #[test]
    fn limit_drops_oldest_but_keeps_current() {
        let dir = std::env::temp_dir().join(format!("vkmusic-cache-limit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = TrackCache::at(dir.clone());
        let now = std::time::SystemTime::now();
        for (i, k) in ["vk-1_1", "vk-1_2", "vk-1_3"].iter().enumerate() {
            let p = dir.join(format!("{k}.mp3"));
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(now - std::time::Duration::from_secs(100 - i as u64 * 10)).unwrap();
        }
        // старейший — vk-1_1, но он «текущий»: удаляются следующие по возрасту
        c.limit.store(150, std::sync::atomic::Ordering::Relaxed);
        c.prune("vk-1_1");
        assert!(c.get("vk-1_1").is_some());
        assert!(c.get("vk-1_2").is_none());
        assert!(c.get("vk-1_3").is_none() || c.stats().1 <= 150);
        c.set_limit(0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn local_copy_cached_and_pruned() {
        let base = std::env::temp_dir().join(format!("vkmusic-cache-test-{}", std::process::id()));
        let src = base.join("music").join("song.MP3");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"id3").unwrap();
        let c = TrackCache::at(base.join("cache"));
        let key = local_key(&src).unwrap();
        assert!(c.get(&key).is_none());
        let job = Job { key: key.clone(), lib: String::new(), source: Source::File(src.clone()), artist: String::new(), title: String::new() };
        let p = c.fetch(&job).unwrap();
        assert_eq!(p.extension().unwrap(), "mp3");
        assert_eq!(c.get(&key), Some(p));
        assert_eq!(c.stats(), (1, 3));
        // имя занято — следующее свободное
        assert_eq!(free_path(&base.join("cache"), &key, "mp3").file_name().unwrap().to_str().unwrap(), format!("{key} (2).mp3"));
        c.retain(|k| k != key);
        assert!(c.get(&key).is_none());
        c.fetch(&job).unwrap();
        c.clear().unwrap();
        assert_eq!(c.stats(), (0, 0));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn prefetch_after_current_and_stale_dropped() {
        let base = std::env::temp_dir().join(format!("vkmusic-cache-queue-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let file = |n: &str| {
            let p = base.join(format!("{n}.mp3"));
            std::fs::write(&p, n.as_bytes()).unwrap();
            p
        };
        let job = |p: &PathBuf| Job {
            key: local_key(p).unwrap(),
            lib: p.display().to_string(),
            source: Source::File(p.clone()),
            artist: String::new(),
            title: String::new(),
        };
        let (a, b, old) = (file("a"), file("b"), file("old"));
        let c = Arc::new(TrackCache::at(base.join("cache")));
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let tx = Mutex::new(tx);
        c.on_done(move |k, ok| {
            assert!(ok);
            let _ = tx.lock().unwrap().send(k.to_string());
        });
        let stale = c.begin();
        let gen = c.begin(); // включили другой трек — прежняя предзагрузка устарела
        c.prefetch(job(&old), stale);
        c.enqueue(job(&a));
        c.prefetch(job(&b), gen);
        let t = std::time::Duration::from_secs(5);
        assert_eq!(rx.recv_timeout(t).unwrap(), a.display().to_string());
        assert_eq!(rx.recv_timeout(t).unwrap(), b.display().to_string());
        assert!(rx.recv_timeout(std::time::Duration::from_millis(200)).is_err());
        assert!(c.get(&local_key(&old).unwrap()).is_none());
        let _ = std::fs::remove_dir_all(base);
    }
}
