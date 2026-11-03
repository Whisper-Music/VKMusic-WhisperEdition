//! Статистика прослушивания: ~/.config/vkmusic/stats.json (просьба владельца, 2026-10-30).
//!
//! По ключу трека (как в порядке библиотеки: путь к файлу или `vk:`/`sc:` + id) — сколько раз трек дослушан
//! до конца, сколько секунд слушали всего, когда впервые и когда последний раз. Плюс журнал по дням
//! (местная дата `ГГГГ-ММ-ДД` от интерфейса): секунды по трекам и по часам суток — из него страница
//! «Статистика» строит итоги за период, топы, графики и серию дней. Секунды считает интерфейс по ходу
//! воспроизведения (без пауз и перемоток) и присылает порциями. Хранится только локально.
//!
//! Файл версии 2: `{"version":2,"tracks":{…},"days":{…}}`. Старый файл (просто ключ → Stat) читается.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

type Res<T> = Result<T, String>;

const MAX_KEYS: usize = 50_000;
/// одна порция секунд — не больше этого (интерфейс шлёт раз в несколько секунд)
const MAX_CHUNK: f64 = 600.0;
/// журнал по дням — не дольше этого (дни, старые выпадают)
const MAX_DAYS: usize = 1500;
/// треков в одном дне журнала — не больше этого
const MAX_DAY_TRACKS: usize = 5000;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Stat {
    /// сколько раз дослушан до конца
    #[serde(default)]
    pub plays: u64,
    /// сколько секунд слушали всего
    #[serde(default)]
    pub seconds: f64,
    /// впервые слушали (unix, с); 0 — неизвестно (статистика старой версии)
    #[serde(default)]
    pub first: u64,
    /// последний раз слушали (unix, с)
    #[serde(default)]
    pub last: u64,
}

/// Один день журнала: секунды по трекам и по часам суток (местное время).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Day {
    #[serde(default)]
    pub tracks: HashMap<String, f64>,
    #[serde(default)]
    pub hours: Vec<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub tracks: HashMap<String, Stat>,
    #[serde(default)]
    pub days: BTreeMap<String, Day>,
}

/// Статистика трека для правой панели: сама статистика и место в топе по времени.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct TrackStat {
    #[serde(flatten)]
    pub stat: Stat,
    /// место по времени прослушивания среди всех треков (1 — больше всех); 0 — ещё не слушали
    pub rank: usize,
    /// сколько треков в статистике
    pub total: usize,
}

pub struct StatsStore {
    data: Mutex<Data>,
    path: PathBuf,
}

pub fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 4096 && !k.chars().any(char::is_control)
}

/// Местная дата от интерфейса: `ГГГГ-ММ-ДД`.
pub fn valid_day(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && d[5..7].parse::<u8>().is_ok_and(|m| (1..=12).contains(&m))
        && d[8..10].parse::<u8>().is_ok_and(|x| (1..=31).contains(&x))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse(bytes: &[u8]) -> Data {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else { return Data::default() };
    if v.get("version").and_then(serde_json::Value::as_u64).unwrap_or(0) >= 2 {
        return serde_json::from_value(v).unwrap_or_default();
    }
    // версия 1: ключ трека → Stat
    let tracks = serde_json::from_value::<HashMap<String, Stat>>(v).unwrap_or_default();
    Data { version: 2, tracks, days: BTreeMap::new() }
}

fn rank_of(tracks: &HashMap<String, Stat>, key: &str) -> TrackStat {
    let stat = tracks.get(key).copied().unwrap_or_default();
    let rank = if stat.seconds > 0.0 { 1 + tracks.values().filter(|s| s.seconds > stat.seconds).count() } else { 0 };
    TrackStat { stat, rank, total: tracks.len() }
}

impl StatsStore {
    pub fn new() -> StatsStore {
        StatsStore::at(crate::config::config_dir().join("stats.json"))
    }

    fn at(path: PathBuf) -> StatsStore {
        let data = std::fs::read(&path).map(|b| parse(&b)).unwrap_or_default();
        StatsStore { data: Mutex::new(data), path }
    }

    pub fn get(&self, key: &str) -> TrackStat {
        rank_of(&self.data.lock().unwrap().tracks, key)
    }

    /// Всё сразу — для страницы «Статистика» (итоги и топы считает интерфейс, он знает названия треков).
    pub fn all(&self) -> Data {
        self.data.lock().unwrap().clone()
    }

    /// Добавить прослушанные секунды (в день `day`, час `hour` по местному времени) и, если трек доигран до
    /// конца, один полный проигрыш. Возвращает новую статистику трека.
    pub fn add(&self, key: &str, seconds: f64, completed: bool, day: &str, hour: u8) -> Res<TrackStat> {
        if !valid_key(key) {
            return Err("Неверный трек".into());
        }
        if !valid_day(day) || hour > 23 {
            return Err("Неверная дата".into());
        }
        let seconds = if seconds.is_finite() { seconds.clamp(0.0, MAX_CHUNK) } else { 0.0 };
        let t = now();
        let mut data = self.data.lock().unwrap();
        data.version = 2;
        if !data.tracks.contains_key(key) && data.tracks.len() >= MAX_KEYS {
            return Err("Слишком много треков в статистике".into());
        }
        let s = data.tracks.entry(key.to_string()).or_default();
        s.seconds += seconds;
        if completed {
            s.plays += 1;
        }
        if s.first == 0 {
            s.first = t;
        }
        s.last = t;
        if seconds > 0.0 {
            let d = data.days.entry(day.to_string()).or_default();
            if d.tracks.contains_key(key) || d.tracks.len() < MAX_DAY_TRACKS {
                *d.tracks.entry(key.to_string()).or_default() += seconds;
            }
            d.hours.resize(24, 0.0);
            d.hours[hour as usize] += seconds;
            while data.days.len() > MAX_DAYS {
                data.days.pop_first();
            }
        }
        let out = rank_of(&data.tracks, key);
        self.save(&data)?;
        Ok(out)
    }

    fn save(&self, data: &Data) -> Res<()> {
        let dir = self.path.parent().ok_or("Нет папки настроек")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|e| format!("Не удалось сохранить статистику: {e}"))?;
            f.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vkmusic-stats-{name}-{}", std::process::id())).join("stats.json")
    }

    #[test]
    fn counts_days_and_persists() {
        let path = tmp("v2");
        let s = StatsStore::at(path.clone());
        s.add("vk:1_2", 12.5, false, "2026-10-30", 23).unwrap();
        s.add("vk:1_2", 1e9, true, "2026-10-30", 23).unwrap(); // порция обрезается
        s.add("vk:1_2", f64::NAN, true, "2026-10-31", 0).unwrap();
        let t = s.add("sc:9", 5.0, false, "2026-10-31", 1).unwrap();
        assert_eq!((t.rank, t.total), (2, 2));
        let g = s.get("vk:1_2");
        assert_eq!((g.stat.plays, g.stat.seconds, g.rank), (2, 612.5, 1));
        assert!(g.stat.first > 0 && g.stat.last >= g.stat.first);
        assert_eq!(s.get("nope").rank, 0);
        let all = s.all();
        assert_eq!(all.days["2026-10-30"].tracks["vk:1_2"], 612.5);
        assert_eq!(all.days["2026-10-30"].hours[23], 612.5);
        assert!(!all.days.contains_key("2026-10-31") || !all.days["2026-10-31"].tracks.contains_key("vk:1_2"));
        assert!(s.add("", 1.0, false, "2026-10-30", 0).is_err());
        assert!(s.add("a\nb", 1.0, false, "2026-10-30", 0).is_err());
        assert!(s.add("k", 1.0, false, "2026-13-01", 0).is_err());
        assert!(s.add("k", 1.0, false, "2026-10-30", 24).is_err());
        // после перезапуска — то же самое
        assert_eq!(StatsStore::at(path.clone()).get("vk:1_2").stat.plays, 2);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reads_version1() {
        let path = tmp("v1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"vk:1_2":{"plays":3,"seconds":100.0}}"#).unwrap();
        let s = StatsStore::at(path.clone());
        assert_eq!(s.get("vk:1_2").stat.plays, 3);
        assert_eq!(s.get("vk:1_2").stat.first, 0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn days_valid() {
        assert!(valid_day("2026-10-30"));
        for bad in ["2026-1-30", "2026-00-10", "2026-10-32", "20261030xx", "2026/10/30", "../../etc"] {
            assert!(!valid_day(bad), "{bad}");
        }
    }
}
