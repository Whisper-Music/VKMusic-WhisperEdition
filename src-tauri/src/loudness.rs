//! «Постоянный уровень громкости»: громкость трека по EBU R128 и постоянное усиление до общей цели.
//!
//! Как у Whisper Yandex (там громкость трека присылает Яндекс): каждый трек на диске (файл из папки музыки,
//! трек в кэше, файл архива) один раз замеряется ffmpeg (`ebur128`, интегральная громкость и истинный пик)
//! в фоне, по одному, с низким приоритетом; результат — в `~/.cache/vkmusic/loudness.json`.
//! Замеренный трек играет с постоянным усилением до `TARGET` (динамика трека не меняется), а если усиление
//! поднимает пики выше −1 dBTP — с ограничителем. Пока трек не замерен (поток, первое включение), mpv
//! выравнивает его на лету (`loudnorm` с той же целью).
//!
//! Прежний фильтр (`dynaudnorm` без целевого уровня) выравнивал по пикам — у современных треков они почти
//! всегда у 0 дБ, поэтому громкие и тихие треки (−6 и −11 LUFS в кэше владельца, 2026-11-06) звучали по-разному.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

/// Цель — −14 LUFS (как у Spotify и YouTube).
pub const TARGET: f64 = -14.0;
/// Потолок истинного пика после усиления, dBTP.
const CEILING: f64 = -1.0;
/// Сильнее не поднимаем: у тихих записей вместе с музыкой поднимается шум.
const MAX_BOOST: f64 = 10.0;
const MAX_ENTRIES: usize = 20_000;

/// Пока трек не замерен — на лету, с той же целью; LRA побольше, чтобы меньше сжимать динамику.
pub const LIVE_FILTER: &str = "lavfi=[loudnorm=I=-14:TP=-1.5:LRA=20]";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    /// интегральная громкость, LUFS
    pub i: f64,
    /// истинный пик, dBTP
    pub tp: f64,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    size: u64,
    #[serde(flatten)]
    m: Measure,
}

pub struct Loudness {
    file: PathBuf,
    map: Mutex<HashMap<String, Entry>>,
    queue: Mutex<(VecDeque<PathBuf>, HashSet<PathBuf>)>,
    wake: Condvar,
    /// не замерились (нет ffmpeg, битый файл) — до перезапуска не пробуем снова
    failed: Mutex<HashSet<PathBuf>>,
}

/// Фильтр mpv для замеренного трека: усиление до цели, ограничитель — если пики вышли бы за потолок.
pub fn filter_for(m: Measure) -> String {
    // тишина / не замерилось по-человечески — не трогаем
    if !m.i.is_finite() || m.i < -50.0 {
        return String::new();
    }
    let gain = (TARGET - m.i).clamp(-30.0, MAX_BOOST);
    let vol = format!("volume={gain:.2}dB");
    if m.tp.is_finite() && m.tp + gain > CEILING {
        // limit — в разах: 10^(−1/20) ≈ 0.891; level=disabled — без своего «автоуровня» ограничителя
        format!("lavfi=[{vol},alimiter=limit=0.891:level=disabled]")
    } else {
        format!("lavfi=[{vol}]")
    }
}

/// Итог `ebur128` из вывода ffmpeg: строки «I: -10.9 LUFS» и «Peak: 1.0 dBFS» в разделе Summary.
pub fn parse_summary(out: &str) -> Option<Measure> {
    let tail = &out[out.rfind("Summary:")?..];
    let num = |label: &str| -> Option<f64> {
        tail.lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix(label))
            .and_then(|r| r.split_whitespace().next())
            .and_then(|v| if v == "-inf" { Some(f64::NEG_INFINITY) } else { v.parse().ok() })
    };
    Some(Measure { i: num("I:")?, tp: num("Peak:").unwrap_or(f64::NAN) })
}

fn size_of(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len())
}

fn measure(path: &Path) -> Result<Measure, String> {
    let mut cmd = crate::os::tool("ffmpeg");
    cmd.args(["-hide_banner", "-nostats", "-nostdin", "-i"])
        .arg(path)
        .args(["-vn", "-sn", "-dn", "-af", "ebur128=peak=true:framelog=quiet", "-f", "null", "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    // замер — фоновая работа: не мешать интерфейсу и самому mpv
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, 15);
            Ok(())
        });
    }
    let out = cmd.output().map_err(|e| format!("ffmpeg: {e}"))?;
    let err = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(err.lines().last().unwrap_or("ffmpeg завершился с ошибкой").to_string());
    }
    parse_summary(&err).ok_or_else(|| "нет итога ebur128".into())
}

impl Loudness {
    pub fn new() -> Arc<Loudness> {
        let base = dirs::cache_dir().unwrap_or_else(|| crate::config::home().join(".cache"));
        Loudness::at(base.join("vkmusic").join("loudness.json"))
    }

    fn at(file: PathBuf) -> Arc<Loudness> {
        let map = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let me = Arc::new(Loudness {
            file,
            map: Mutex::new(map),
            queue: Mutex::new(Default::default()),
            wake: Condvar::new(),
            failed: Mutex::new(HashSet::new()),
        });
        let w = me.clone();
        let _ = std::thread::Builder::new().name("loudness".into()).spawn(move || w.work());
        me
    }

    /// Замер файла, если он есть и файл с тех пор не менялся (размер тот же).
    pub fn get(&self, path: &Path) -> Option<Measure> {
        let size = size_of(path)?;
        let map = self.map.lock().unwrap();
        map.get(path.to_string_lossy().as_ref()).filter(|e| e.size == size).map(|e| e.m)
    }

    /// Замерить в фоне (если ещё не замерен и не стоит в очереди).
    pub fn request(&self, path: &Path) {
        if self.get(path).is_some() || self.failed.lock().unwrap().contains(path) || size_of(path).is_none() {
            return;
        }
        let mut q = self.queue.lock().unwrap();
        if q.1.insert(path.to_path_buf()) {
            q.0.push_back(path.to_path_buf());
            self.wake.notify_one();
        }
    }

    /// Фильтр mpv для файла: замеренный — постоянное усиление, иначе — на лету (и замер в очередь).
    pub fn filter(&self, path: &Path) -> String {
        match self.get(path) {
            Some(m) => filter_for(m),
            None => {
                self.request(path);
                LIVE_FILTER.into()
            }
        }
    }

    fn work(&self) {
        loop {
            let path = {
                let mut q = self.queue.lock().unwrap();
                loop {
                    if let Some(p) = q.0.pop_front() {
                        break p;
                    }
                    q = self.wake.wait(q).unwrap();
                }
            };
            let res = match size_of(&path) {
                Some(size) => measure(&path).map(|m| (size, m)),
                None => Err("файла нет".into()),
            };
            self.queue.lock().unwrap().1.remove(&path);
            match res {
                Ok((size, m)) => self.store(&path, size, m),
                Err(e) => {
                    eprintln!("vkmusic: громкость {}: {e}", path.display());
                    self.failed.lock().unwrap().insert(path);
                }
            }
        }
    }

    fn store(&self, path: &Path, size: u64, m: Measure) {
        let json = {
            let mut map = self.map.lock().unwrap();
            map.insert(path.to_string_lossy().into_owned(), Entry { size, m });
            if map.len() > MAX_ENTRIES {
                map.retain(|k, _| Path::new(k).is_file());
            }
            serde_json::to_vec(&*map)
        };
        let Ok(json) = json else { return };
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.file.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "[Parsed_ebur128_0 @ 0x1] Summary:\n\n  Integrated loudness:\n    I:         -10.9 LUFS\n    \
                       Threshold: -21.0 LUFS\n\n  Loudness range:\n    LRA:         3.7 LU\n\n  True peak:\n    Peak:        1.0 dBFS\n";

    #[test]
    fn parses_ffmpeg_summary() {
        assert_eq!(parse_summary(OUT), Some(Measure { i: -10.9, tp: 1.0 }));
        assert_eq!(parse_summary("I: -5 LUFS"), None, "без Summary — не итог (это строки по ходу)");
        let silent = parse_summary("Summary:\n    I:         -70.0 LUFS\n    Peak:       -inf dBFS\n").unwrap();
        assert_eq!(silent.i, -70.0);
        assert!(silent.tp.is_infinite());
    }

    #[test]
    fn gain_to_target_with_limiter_only_when_needed() {
        // громкий трек: −3.1 дБ, пик 1 − 3.1 < −1 — без ограничителя
        assert_eq!(filter_for(Measure { i: -10.9, tp: 1.0 }), "lavfi=[volume=-3.10dB]");
        // тихий: +6 дБ поднимут пик −3 до +3 — нужен ограничитель
        assert_eq!(
            filter_for(Measure { i: -20.0, tp: -3.0 }),
            "lavfi=[volume=6.00dB,alimiter=limit=0.891:level=disabled]"
        );
        // очень тихий — не больше MAX_BOOST
        assert!(filter_for(Measure { i: -40.0, tp: -20.0 }).starts_with("lavfi=[volume=10.00dB"));
        // тишина — без фильтра
        assert_eq!(filter_for(Measure { i: -70.0, tp: f64::NEG_INFINITY }), "");
    }

    #[test]
    fn stored_measure_needs_same_size() {
        let dir = std::env::temp_dir().join(format!("vkm-loud-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let track = dir.join("a.mp3");
        std::fs::write(&track, b"1234").unwrap();
        let l = Loudness::at(dir.join("loudness.json"));
        l.store(&track, 4, Measure { i: -9.0, tp: 0.5 });
        assert_eq!(l.get(&track).map(|m| m.i), Some(-9.0));
        // файл перезаписан — замер больше не годится
        std::fs::write(&track, b"123456").unwrap();
        assert!(l.get(&track).is_none());
        // и переживает перезапуск
        std::fs::write(&track, b"1234").unwrap();
        let again = Loudness::at(dir.join("loudness.json"));
        assert_eq!(again.get(&track).map(|m| m.tp), Some(0.5));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
