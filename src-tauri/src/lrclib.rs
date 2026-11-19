//! Тексты песен из LRCLIB (https://lrclib.net; как у Whisper Yandex, просьба владельца 2026-11-17) — открытая база,
//! в том числе синхронизированных текстов (LRC). Порядок поиска (`lyrics_get`): текст ВК → синхронный LRCLIB →
//! Genius → простой текст LRCLIB. Запросы — только на lrclib.net (свой клиент, перенаправления — только туда же),
//! по одному и не чаще раза в секунду, как к остальным сервисам; один повтор при временном сбое (429, 5xx, сеть).

use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

const HOST: &str = "lrclib.net";
const MAX_BODY: usize = 2 * 1024 * 1024;
const GAP: Duration = Duration::from_millis(1000);

/// Найденный текст: строки и (у синхронного) начало каждой строки, мс.
#[derive(Debug, PartialEq, Default)]
pub struct Found {
    pub text: String,
    pub times: Vec<u64>,
    pub instrumental: bool,
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::custom(|a| {
                if a.previous().len() < 3 && a.url().scheme() == "https" && a.url().host_str() == Some(HOST) {
                    a.follow()
                } else {
                    a.error("перенаправление за пределы lrclib.net")
                }
            }))
            // LRCLIB просит называть приложение, версию и ссылку
            .user_agent(concat!("whisper-vkmusic/", env!("CARGO_PKG_VERSION"), " (https://github.com/Whisper-Music/whisper-vkmusic)"))
            .build()
            .expect("HTTP-клиент LRCLIB")
    })
}

enum Fail {
    Retry(String, Duration),
    Final(String),
}

async fn get_once(url: &reqwest::Url) -> Result<Option<Value>, Fail> {
    static LAST: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
    let mut last = LAST.lock().await;
    if let Some(t) = *last {
        if t.elapsed() < GAP {
            tokio::time::sleep(GAP - t.elapsed()).await;
        }
    }
    *last = Some(Instant::now());
    let res = client().get(url.clone()).send().await;
    drop(last);
    let resp = match res {
        Ok(r) => r,
        Err(e) if e.is_timeout() => return Err(Fail::Retry(format!("LRCLIB долго не отвечает ({e})"), Duration::from_millis(300))),
        Err(e) => return Err(Fail::Retry(format!("LRCLIB недоступен ({e})"), Duration::from_millis(500))),
    };
    let st = resp.status();
    if st.as_u16() == 404 {
        return Ok(None);
    }
    if st.as_u16() == 429 {
        let wait = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2)
            .min(5);
        return Err(Fail::Retry("LRCLIB просит подождать — слишком много запросов".into(), Duration::from_secs(wait)));
    }
    if st.is_server_error() {
        return Err(Fail::Retry(format!("LRCLIB: ошибка сервера {}", st.as_u16()), Duration::from_millis(800)));
    }
    if !st.is_success() {
        return Err(Fail::Final(format!("LRCLIB: HTTP {}", st.as_u16())));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_BODY) {
        return Err(Fail::Final("LRCLIB: слишком большой ответ".into()));
    }
    let b = resp.bytes().await.map_err(|e| Fail::Retry(format!("ответ LRCLIB оборвался ({e})"), Duration::from_millis(500)))?;
    if b.len() > MAX_BODY {
        return Err(Fail::Final("LRCLIB: слишком большой ответ".into()));
    }
    serde_json::from_slice(&b).map(Some).map_err(|_| Fail::Final("LRCLIB: непонятный ответ".into()))
}

async fn get_json(url: reqwest::Url) -> Res<Option<Value>> {
    match get_once(&url).await {
        Ok(v) => Ok(v),
        Err(Fail::Final(e)) => Err(e),
        Err(Fail::Retry(_, wait)) => {
            tokio::time::sleep(wait).await;
            get_once(&url).await.map_err(|f| match f {
                Fail::Retry(e, _) | Fail::Final(e) => e,
            })
        }
    }
}

fn url(path: &str, q: &[(&str, &str)]) -> reqwest::Url {
    let mut u = reqwest::Url::parse(&format!("https://{HOST}{path}")).expect("адрес LRCLIB");
    u.query_pairs_mut().extend_pairs(q.iter().filter(|(_, v)| !v.is_empty()));
    u
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// LRC → строки и время начала каждой (мс). Строка с несколькими метками повторяется для каждой; метки
/// `[ar:…]` и прочие теги пропускаются. Время не по порядку — сортируем; пустые строки по краям убираются.
pub fn parse_lrc(lrc: &str) -> (String, Vec<u64>) {
    let mut rows: Vec<(u64, String)> = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some(end) = r.find(']') else { break };
            let tag = &r[..end];
            let Some((m, s)) = tag.split_once(':') else { break };
            let (Ok(m), Ok(s)) = (m.trim().parse::<u64>(), s.trim().parse::<f64>()) else { break };
            if !(0.0..60.0).contains(&s) {
                break;
            }
            stamps.push(m * 60_000 + (s * 1000.0).round() as u64);
            rest = r[end + 1..].trim_start();
        }
        let line: String = rest.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
        for t in stamps {
            rows.push((t, line.clone()));
        }
    }
    rows.sort_by_key(|r| r.0);
    while rows.first().is_some_and(|r| r.1.is_empty()) {
        rows.remove(0);
    }
    while rows.last().is_some_and(|r| r.1.is_empty()) {
        rows.pop();
    }
    let text = rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>().join("\n");
    (text, rows.into_iter().map(|r| r.0).collect())
}

fn pick(v: &Value, want_synced: bool) -> Option<Found> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty());
    if v.get("instrumental").and_then(Value::as_bool).unwrap_or(false) {
        return Some(Found { instrumental: true, ..Found::default() });
    }
    if let Some((text, times)) = s("syncedLyrics").map(parse_lrc).filter(|(t, _)| !t.is_empty()) {
        return Some(Found { text, times, instrumental: false });
    }
    if want_synced {
        return None;
    }
    s("plainLyrics").map(|p| Found { text: p.lines().map(str::trim_end).collect::<Vec<_>>().join("\n"), times: Vec::new(), instrumental: false })
}

/// Текст трека: точный поиск (исполнитель + название + длительность), не нашлось — обычный, вариант с ближайшей
/// длительностью (±8 с), синхронный — выше. `synced_only` — только синхронный (до Genius).
pub async fn find(artist: &str, title: &str, duration: Option<f64>, synced_only: bool) -> Res<Option<Found>> {
    let artist = clean(artist, 300);
    let title = clean(title, 300);
    if title.is_empty() {
        return Ok(None);
    }
    let dur = duration.filter(|d| d.is_finite() && *d > 0.0 && *d < 6.0 * 3600.0);
    let dur_s = dur.map(|d| (d.round() as u64).to_string()).unwrap_or_default();
    let mut first_err = None;
    if !artist.is_empty() && !dur_s.is_empty() {
        let q = [("artist_name", artist.as_str()), ("track_name", title.as_str()), ("duration", dur_s.as_str())];
        match get_json(url("/api/get", &q)).await {
            Ok(Some(v)) => {
                if let Some(f) = pick(&v, synced_only) {
                    return Ok(Some(f));
                }
            }
            Ok(None) => {}
            Err(e) if e.contains("подождать") => return Err(e),
            Err(e) => first_err = Some(e),
        }
    }
    let q = [("track_name", title.as_str()), ("artist_name", artist.as_str())];
    let items = match get_json(url("/api/search", &q)).await {
        Ok(Some(Value::Array(items))) => items,
        Ok(_) => return Ok(None),
        Err(e) => return Err(first_err.unwrap_or(e)),
    };
    let off = |v: &Value| dur.map(|x| (x - v.get("duration").and_then(Value::as_f64).unwrap_or(0.0)).abs()).unwrap_or(0.0);
    let synced = |v: &Value| v.get("syncedLyrics").and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty());
    let mut good: Vec<&Value> = items.iter().filter(|v| off(v) <= 8.0).collect();
    good.sort_by(|a, b| synced(b).cmp(&synced(a)).then(off(a).partial_cmp(&off(b)).unwrap_or(std::cmp::Ordering::Equal)));
    Ok(good.into_iter().find_map(|v| pick(v, synced_only)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc() {
        let (t, times) = parse_lrc("[ar: Кино]\n[00:01.00]\n[00:12.50]Группа крови\n[00:15.20][01:02.00]На рукаве\n[00:20.00]\n[00:21.00]Мой порядковый номер\n[00:30.00]");
        // пустая строка на 0:30 — не крайняя: после неё повтор с 1:02
        assert_eq!(t, "Группа крови\nНа рукаве\n\nМой порядковый номер\n\nНа рукаве");
        assert_eq!(times, vec![12500, 15200, 20000, 21000, 30000, 62000]);
        assert_eq!(parse_lrc("просто текст"), (String::new(), vec![]));
    }

    #[test]
    fn picks() {
        let v = serde_json::json!({ "syncedLyrics": "[00:01.00]раз\n[00:02.00]два", "plainLyrics": "раз\nдва" });
        assert_eq!(pick(&v, true).unwrap().times, vec![1000, 2000]);
        let p = serde_json::json!({ "plainLyrics": "раз  \nдва" });
        assert!(pick(&p, true).is_none());
        assert_eq!(pick(&p, false).unwrap().text, "раз\nдва");
        assert!(pick(&serde_json::json!({ "instrumental": true }), true).unwrap().instrumental);
    }

    /// cargo test live_lrclib -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_lrclib() {
        let f = tauri::async_runtime::block_on(find("Кино", "Группа крови", Some(287.0), true)).unwrap();
        let f = f.expect("не нашлось");
        println!("{} строк, синхронный: {}", f.text.lines().count(), !f.times.is_empty());
        println!("{}", f.text.lines().take(4).collect::<Vec<_>>().join(" / "));
    }
}
