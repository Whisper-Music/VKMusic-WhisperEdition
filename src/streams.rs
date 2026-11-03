//! Кэш ссылок на потоки треков сервисов — только в памяти, пока ссылка действует.
//!
//! Задержка при включении трека ВК/SoundCloud — это в основном получение ссылки (1–2 запроса с паузой
//! между ними). Уже прослушанный трек берёт ссылку отсюда и включается сразу, без запросов. Звук на диск
//! здесь не пишется (треки библиотеки кэширует `cache`); после перезапуска приложения кэш ссылок пуст.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// если в ссылке нет срока — столько её и храним
const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);
/// не дольше этого, даже если ссылка обещает больше
const MAX_TTL: Duration = Duration::from_secs(60 * 60);
/// запас до истечения срока
const MARGIN: u64 = 120;
const MAX_ITEMS: usize = 2000;

#[derive(Clone)]
pub struct Stream {
    pub url: String,
    pub referrer: String,
}

pub struct StreamCache {
    items: Mutex<HashMap<String, (Stream, Instant)>>,
}

/// Срок жизни ссылки по её параметру expires (unix, с), если он есть.
pub fn ttl_for(url: &str, now_unix: u64) -> Option<Duration> {
    let u = reqwest::Url::parse(url).ok()?;
    let from_query = u
        .query_pairs()
        .find(|(k, _)| k.eq_ignore_ascii_case("expires"))
        .and_then(|(_, v)| v.parse::<u64>().ok());
    let from_path = || {
        let segs: Vec<&str> = u.path_segments()?.collect();
        let i = segs.iter().position(|s| *s == "expires")?;
        segs.get(i + 1)?.parse::<u64>().ok()
    };
    let exp = from_query.or_else(from_path)?;
    Some(Duration::from_secs(exp.saturating_sub(now_unix + MARGIN)))
}

impl StreamCache {
    pub fn new() -> StreamCache {
        StreamCache { items: Mutex::new(HashMap::new()) }
    }

    pub fn get(&self, key: &str) -> Option<Stream> {
        let mut items = self.items.lock().unwrap();
        match items.get(key) {
            Some((s, until)) if Instant::now() < *until => Some(s.clone()),
            Some(_) => {
                items.remove(key);
                None
            }
            None => None,
        }
    }

    pub fn put(&self, key: &str, s: Stream) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let ttl = ttl_for(&s.url, now).unwrap_or(DEFAULT_TTL).min(MAX_TTL);
        if ttl.is_zero() {
            return;
        }
        let mut items = self.items.lock().unwrap();
        if items.len() >= MAX_ITEMS {
            let t = Instant::now();
            items.retain(|_, (_, until)| *until > t);
            if items.len() >= MAX_ITEMS {
                items.clear();
            }
        }
        items.insert(key.to_string(), (s, Instant::now() + ttl));
    }

    pub fn forget(&self, key: &str) {
        self.items.lock().unwrap().remove(key);
    }

    /// выход из сервиса: ссылки его сессии больше не нужны
    pub fn clear_prefix(&self, prefix: &str) {
        self.items.lock().unwrap().retain(|k, _| !k.starts_with(prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_from_expires() {
        let now = 1_000_000;
        assert_eq!(ttl_for("https://cs1.okcdn.ru/a.mp3?expires=1000720&x=1", now), Some(Duration::from_secs(600)));
        assert_eq!(ttl_for("https://cs1.vkuseraudio.net/s/expires/1000500/a.m3u8", now), Some(Duration::from_secs(380)));
        assert_eq!(ttl_for("https://cf-media.sndcdn.com/a.mp3?Policy=x", now), None);
        // уже истекла
        assert_eq!(ttl_for("https://x/a?expires=999000", now), Some(Duration::ZERO));
    }

    #[test]
    fn cache_put_get() {
        let c = StreamCache::new();
        let s = |u: &str| Stream { url: u.into(), referrer: "https://vk.ru/".into() };
        c.put("vk:1", s("https://cf/a.mp3"));
        assert_eq!(c.get("vk:1").unwrap().url, "https://cf/a.mp3");
        // истёкшую не кладём
        c.put("vk:2", s("https://x/a?expires=1"));
        assert!(c.get("vk:2").is_none());
        c.clear_prefix("vk:");
        assert!(c.get("vk:1").is_none());
        c.put("sc:1", s("https://cf/b.mp3"));
        c.forget("sc:1");
        assert!(c.get("sc:1").is_none());
    }
}
