//! SoundCloud — поиск и прослушивание под своим аккаунтом.
//!
//! Как и ВКонтакте, это неофициальный клиент: внутренний api-v2 сайта с client_id из его скриптов
//! (так же делают yt-dlp и scdl). Рамки те же:
//! * вход — обычная страница soundcloud.com/signin в главном окне; пароль приложение не видит,
//!   сессия — кука `oauth_token` в хранилище WebKit; выход удаляет куки SoundCloud;
//! * запросы — только по действию пользователя, не чаще раза в секунду, без фоновой подгрузки;
//! * ссылка на поток запрашивается при нажатии «играть»; поток на диск не пишется
//!   (скачать трек можно, как и раньше, через yt-dlp — по ссылке на страницу трека).

use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const LOGIN_URL: &str = "https://soundcloud.com/signin";
const SITE: &str = "https://soundcloud.com";
const API: &str = "https://api-v2.soundcloud.com";
const GAP: Duration = Duration::from_millis(1000);
const MAX_BODY: usize = 8 * 1024 * 1024;
const PAGE: u64 = 30;
/// треков плейлиста за раз (ссылка на плейлист)
const MAX_PLAYLIST: usize = 200;

type Res<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Track {
    /// числовой id трека
    pub id: String,
    pub artist: String,
    pub title: String,
    pub duration: f64,
    pub cover: Option<String>,
    /// страница трека (для yt-dlp и «открыть в браузере»)
    pub url: Option<String>,
    /// доступен только 30-секундный фрагмент (Go+ без подписки)
    pub snippet: bool,
    /// SoundCloud не даёт слушать трек в этой стране/вообще
    pub blocked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub tracks: Vec<Track>,
    pub next_offset: Option<u64>,
}

pub struct Sc {
    http: reqwest::Client,
    client_id: std::sync::Mutex<Option<String>>,
    last: tokio::sync::Mutex<Option<Instant>>,
    profile: std::sync::Mutex<Option<Profile>>,
}

// ───────────────────────── сессия ─────────────────────────

/// Токен сессии — кука oauth_token у soundcloud.com.
pub fn token_from<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Option<String> {
    cookies
        .into_iter()
        .find(|(name, value, domain)| *name == "oauth_token" && is_sc_domain(domain) && valid_token(value))
        .map(|(_, v, _)| v.to_string())
}

fn valid_token(t: &str) -> bool {
    !t.is_empty() && t.len() <= 256 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub fn is_sc_domain(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == "soundcloud.com" || d.ends_with(".soundcloud.com")
}

/// Ссылка на страницу SoundCloud (для resolve и yt-dlp).
pub fn is_sc_url(s: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(s.trim()) else { return false };
    u.scheme() == "https"
        && u.host_str().is_some_and(|h| ["soundcloud.com", "m.soundcloud.com", "www.soundcloud.com", "on.soundcloud.com"].contains(&h))
        && u.path().len() > 1
}

// ───────────────────────── разбор ответов ─────────────────────────

fn num_id(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn https(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| s.starts_with("https://") && s.len() < 2048 && !s.chars().any(char::is_whitespace))
        .map(str::to_string)
}

fn transcodings(v: &Value) -> Vec<&Value> {
    v.pointer("/media/transcodings").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

pub fn parse_track(v: &Value) -> Option<Track> {
    if v.get("kind").and_then(Value::as_str).is_some_and(|k| k != "track") {
        return None;
    }
    let id = num_id(v.get("id"))?;
    let title = v.get("title").and_then(Value::as_str)?.trim().to_string();
    let artist = v
        .pointer("/publisher_metadata/artist")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| v.pointer("/user/username").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    // обложка покрупнее: -large (100px) → -t500x500 (панель «Сейчас играет»)
    let cover = https(v.get("artwork_url"))
        .or_else(|| https(v.pointer("/user/avatar_url")))
        .map(|c| c.replace("-large.", "-t500x500."));
    let tc = transcodings(v);
    let policy = v.get("policy").and_then(Value::as_str).unwrap_or("");
    let snippet = policy == "SNIP" || (!tc.is_empty() && tc.iter().all(|t| t.get("snipped").and_then(Value::as_bool) == Some(true)));
    Some(Track {
        id: id.to_string(),
        artist,
        title,
        duration: v.get("duration").and_then(Value::as_f64).unwrap_or(0.0) / 1000.0,
        cover,
        url: https(v.get("permalink_url")).filter(|u| u.starts_with("https://soundcloud.com/")),
        snippet,
        blocked: policy == "BLOCK" || tc.is_empty(),
    })
}

/// Транскодинг для прослушивания: целый трек важнее фрагмента, progressive (mp3) — HLS;
/// зашифрованные HLS (DRM) mpv не сыграет — пропускаем. Адрес — только api-v2.
pub fn pick_transcoding(v: &Value) -> Option<String> {
    let tc = transcodings(v);
    let ok = |t: &&Value, protocol: &str, snipped: bool| {
        t.pointer("/format/protocol").and_then(Value::as_str) == Some(protocol)
            && t.get("snipped").and_then(Value::as_bool).unwrap_or(false) == snipped
            && t.get("url").and_then(Value::as_str).is_some_and(|u| u.starts_with(&format!("{API}/")))
    };
    for snipped in [false, true] {
        for protocol in ["progressive", "hls"] {
            if let Some(t) = tc.iter().find(|t| ok(t, protocol, snipped)) {
                return t.get("url").and_then(Value::as_str).map(str::to_string);
            }
        }
    }
    None
}

/// next_href → путь и параметры запроса к api-v2 (без client_id — его подставит `api`).
fn api_path(href: &str) -> Option<(String, Vec<(String, String)>)> {
    let u = reqwest::Url::parse(href).ok()?;
    if u.scheme() != "https" || u.host_str() != Some("api-v2.soundcloud.com") {
        return None;
    }
    let params = u.query_pairs().filter(|(k, _)| k != "client_id").map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Some((u.path().to_string(), params))
}

fn offset_of(next_href: Option<&Value>) -> Option<u64> {
    let u = reqwest::Url::parse(next_href?.as_str()?).ok()?;
    let off = u.query_pairs().find(|(k, _)| k == "offset")?.1.parse().ok()?;
    Some(off)
}

/// client_id из скрипта сайта.
pub fn find_client_id(js: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"client_id\s*[:=]\s*"?([A-Za-z0-9]{32})\b"#).unwrap());
    re.captures(js).map(|c| c[1].to_string())
}

/// Скрипты сайта, в которых может лежать client_id (последние — вероятнее).
pub fn script_urls(html: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"<script[^>]+src="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)""#).unwrap());
    re.captures_iter(html).map(|c| c[1].to_string()).collect()
}

// ───────────────────────── запросы ─────────────────────────

impl Sc {
    pub fn new() -> Sc {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(crate::vk::USER_AGENT)
            .build()
            .unwrap_or_default();
        Sc {
            http,
            client_id: std::sync::Mutex::new(None),
            last: tokio::sync::Mutex::new(None),
            profile: std::sync::Mutex::new(None),
        }
    }

    pub fn forget(&self) {
        *self.profile.lock().unwrap() = None;
    }

    /// GET с очередью (не чаще раза в секунду); тело — текст.
    async fn get(&self, url: reqwest::Url, token: Option<&str>) -> Res<(u16, String)> {
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < GAP {
                tokio::time::sleep(GAP - t.elapsed()).await;
            }
        }
        let mut req = self.http.get(url).header("Origin", SITE).header("Referer", format!("{SITE}/"));
        if let Some(t) = token {
            req = req.header("Authorization", format!("OAuth {t}"));
        }
        let res = req.send().await;
        *last = Some(Instant::now());
        drop(last);
        let res = res.map_err(|e| format!("Нет связи с SoundCloud: {e}"))?;
        let status = res.status().as_u16();
        if res.content_length().is_some_and(|n| n as usize > MAX_BODY) {
            return Err("Слишком большой ответ SoundCloud".into());
        }
        let text = res.text().await.map_err(|e| format!("Ответ SoundCloud не прочитан: {e}"))?;
        if text.len() > MAX_BODY {
            return Err("Слишком большой ответ SoundCloud".into());
        }
        Ok((status, text))
    }

    async fn client_id(&self, fresh: bool) -> Res<String> {
        if !fresh {
            if let Some(id) = self.client_id.lock().unwrap().clone() {
                return Ok(id);
            }
        }
        let (status, html) = self.get(reqwest::Url::parse(SITE).unwrap(), None).await?;
        if status != 200 {
            return Err(format!("SoundCloud ответил HTTP {status}"));
        }
        for src in script_urls(&html).iter().rev().take(10) {
            let Ok(u) = reqwest::Url::parse(src) else { continue };
            let Ok((200, js)) = self.get(u, None).await else { continue };
            if let Some(id) = find_client_id(&js) {
                *self.client_id.lock().unwrap() = Some(id.clone());
                return Ok(id);
            }
        }
        Err("Не удалось подключиться к SoundCloud — возможно, сайт изменился.".into())
    }

    /// Запрос к api-v2 с client_id; при 401/403 один раз обновляет client_id.
    async fn api(&self, path: &str, params: &[(&str, String)], token: Option<&str>) -> Res<Value> {
        for fresh in [false, true] {
            let cid = self.client_id(fresh).await?;
            let mut u = reqwest::Url::parse(&format!("{API}{path}")).map_err(|e| e.to_string())?;
            u.query_pairs_mut().extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str()))).append_pair("client_id", &cid);
            let (status, text) = self.get(u, token).await?;
            match status {
                200 => return serde_json::from_str(&text).map_err(|_| "SoundCloud ответил не данными".to_string()),
                401 | 403 if !fresh => continue,
                401 if token.is_some() => return Err("Сессия SoundCloud истекла — войдите снова.".into()),
                404 => return Err("SoundCloud: не найдено".into()),
                429 => return Err("SoundCloud ограничил число запросов — попробуйте позже.".into()),
                s => return Err(format!("SoundCloud ответил HTTP {s}")),
            }
        }
        Err("SoundCloud не принял запрос".into())
    }

    pub async fn profile(&self, token: &str) -> Res<Profile> {
        if let Some(p) = self.profile.lock().unwrap().clone() {
            return Ok(p);
        }
        let v = self.api("/me", &[], Some(token)).await?;
        let id = num_id(v.get("id")).ok_or("SoundCloud не отдал профиль")?;
        let name = v
            .get("username")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("SoundCloud")
            .to_string();
        let p = Profile { id, name };
        *self.profile.lock().unwrap() = Some(p.clone());
        Ok(p)
    }

    pub async fn search(&self, token: Option<&str>, query: &str, offset: u64) -> Res<Page> {
        let v = self
            .api(
                "/search/tracks",
                &[("q", query.into()), ("limit", PAGE.to_string()), ("offset", offset.to_string()), ("linked_partitioning", "1".into())],
                token,
            )
            .await?;
        let tracks = v
            .get("collection")
            .and_then(Value::as_array)
            .map(|c| c.iter().filter_map(parse_track).collect())
            .unwrap_or_default();
        Ok(Page { tracks, next_offset: offset_of(v.get("next_href")).filter(|&n| n > offset) })
    }

    /// Лайки аккаунта (для синхронизации с библиотекой), новые сначала; до `max` треков.
    pub async fn likes(&self, token: &str, max: usize) -> Res<Vec<Track>> {
        let uid = self.profile(token).await?.id;
        let mut out = Vec::new();
        let mut v = self
            .api(&format!("/users/{uid}/track_likes"), &[("limit", "200".into()), ("linked_partitioning", "1".into())], Some(token))
            .await?;
        for _ in 0..50 {
            for item in v.get("collection").and_then(Value::as_array).into_iter().flatten() {
                if let Some(t) = item.get("track").and_then(parse_track) {
                    out.push(t);
                }
            }
            if out.len() >= max {
                out.truncate(max);
                break;
            }
            // следующая страница — по next_href, но только на api-v2
            let Some((path, params)) = v.get("next_href").and_then(Value::as_str).and_then(api_path) else { break };
            let params: Vec<(&str, String)> = params.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            v = self.api(&path, &params, Some(token)).await?;
        }
        Ok(out)
    }

    /// Ссылка на трек или плейлист SoundCloud → треки.
    pub async fn resolve(&self, token: Option<&str>, url: &str) -> Res<Vec<Track>> {
        if !is_sc_url(url) {
            return Err("Это не ссылка на SoundCloud".into());
        }
        let v = self.api("/resolve", &[("url", url.trim().into())], token).await?;
        match v.get("kind").and_then(Value::as_str) {
            Some("track") => Ok(parse_track(&v).into_iter().collect()),
            Some("playlist" | "system-playlist") => {
                let items: Vec<&Value> = v.get("tracks").and_then(Value::as_array).map(|a| a.iter().take(MAX_PLAYLIST).collect()).unwrap_or_default();
                // в плейлисте полные данные только у первых треков, у остальных — один id
                let missing: Vec<String> = items.iter().filter(|t| parse_track(t).is_none()).filter_map(|t| num_id(t.get("id"))).map(|i| i.to_string()).collect();
                let mut full = std::collections::HashMap::new();
                for chunk in missing.chunks(50) {
                    let got = self.api("/tracks", &[("ids", chunk.join(","))], token).await?;
                    for t in got.as_array().into_iter().flatten().filter_map(parse_track) {
                        full.insert(t.id.clone(), t);
                    }
                }
                Ok(items
                    .iter()
                    .filter_map(|t| parse_track(t).or_else(|| full.get(&num_id(t.get("id"))?.to_string()).cloned()))
                    .collect())
            }
            _ => Err("По этой ссылке нет трека или плейлиста".into()),
        }
    }

    /// Ссылка на поток одного трека — по нажатию «играть».
    pub async fn stream_url(&self, token: Option<&str>, id: &str) -> Res<String> {
        if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Неверный id трека".into());
        }
        let v = self.api(&format!("/tracks/{id}"), &[], token).await?;
        let tc = pick_transcoding(&v).ok_or("SoundCloud не даёт слушать этот трек")?;
        let path = tc.strip_prefix(API).ok_or("Неверная ссылка SoundCloud")?.to_string();
        let mut params = Vec::new();
        if let Some(a) = v.get("track_authorization").and_then(Value::as_str) {
            params.push(("track_authorization", a.to_string()));
        }
        // путь транскодинга уже содержит свои параметры — разбираем их
        let (path, query) = path.split_once('?').unwrap_or((&path, ""));
        let mut all: Vec<(&str, String)> = Vec::new();
        let parsed: Vec<(String, String)> = reqwest::Url::parse(&format!("{API}/?{query}"))
            .map(|u| u.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect())
            .unwrap_or_default();
        for (k, v) in &parsed {
            if k != "client_id" {
                all.push((k.as_str(), v.clone()));
            }
        }
        all.extend(params);
        let r = self.api(path, &all, token).await?;
        https(r.get("url")).ok_or_else(|| "SoundCloud не дал ссылку на поток".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tr(policy: &str, tcs: Value) -> Value {
        json!({
            "kind": "track", "id": 123456, "title": " Song ", "duration": 215000,
            "artwork_url": "https://i1.sndcdn.com/artworks-abc-large.jpg",
            "permalink_url": "https://soundcloud.com/artist/song",
            "policy": policy, "user": {"username": "Artist", "avatar_url": "https://i1.sndcdn.com/avatars-x-large.jpg"},
            "media": {"transcodings": tcs}, "track_authorization": "auth"
        })
    }

    fn tc(protocol: &str, snipped: bool, path: &str) -> Value {
        json!({"url": format!("https://api-v2.soundcloud.com/media/soundcloud:tracks:123456/{path}"), "format": {"protocol": protocol, "mime_type": "audio/mpeg"}, "snipped": snipped})
    }

    #[test]
    fn track_parsed() {
        let t = parse_track(&tr("ALLOW", json!([tc("hls", false, "a/stream/hls"), tc("progressive", false, "b/stream/progressive")]))).unwrap();
        assert_eq!(t.id, "123456");
        assert_eq!(t.title, "Song");
        assert_eq!(t.artist, "Artist");
        assert_eq!(t.duration, 215.0);
        assert_eq!(t.cover.as_deref(), Some("https://i1.sndcdn.com/artworks-abc-t500x500.jpg"));
        assert_eq!(t.url.as_deref(), Some("https://soundcloud.com/artist/song"));
        assert!(!t.snippet && !t.blocked);
        assert!(parse_track(&json!({"kind": "playlist", "id": 1, "title": "x"})).is_none());
        assert!(parse_track(&tr("SNIP", json!([tc("hls", true, "a")]))).unwrap().snippet);
        assert!(parse_track(&tr("BLOCK", json!([]))).unwrap().blocked);
    }

    #[test]
    fn transcoding_choice() {
        let v = tr("ALLOW", json!([tc("hls", true, "s"), tc("hls", false, "h"), tc("progressive", false, "p")]));
        assert!(pick_transcoding(&v).unwrap().ends_with("/p"));
        let v = tr("ALLOW", json!([tc("hls", true, "s"), tc("hls", false, "h"), tc("ctr-encrypted-hls", false, "e")]));
        assert!(pick_transcoding(&v).unwrap().ends_with("/h"));
        let v = tr("SNIP", json!([tc("hls", true, "s")]));
        assert!(pick_transcoding(&v).unwrap().ends_with("/s"));
        // чужой адрес не принимаем
        let v = tr("ALLOW", json!([{"url": "https://evil.example/x", "format": {"protocol": "progressive"}, "snipped": false}]));
        assert_eq!(pick_transcoding(&v), None);
    }

    #[test]
    fn session_and_urls() {
        assert_eq!(token_from([("oauth_token", "2-29-abc_DEF", ".soundcloud.com")]).as_deref(), Some("2-29-abc_DEF"));
        assert_eq!(token_from([("oauth_token", "bad token", ".soundcloud.com")]), None);
        assert_eq!(token_from([("oauth_token", "x", "soundcloud.com.evil")]), None);
        assert!(is_sc_url("https://soundcloud.com/artist/song"));
        assert!(is_sc_url("https://on.soundcloud.com/AbCd"));
        assert!(!is_sc_url("https://soundcloud.com/"));
        assert!(!is_sc_url("http://soundcloud.com/a"));
        assert!(!is_sc_url("https://soundcloud.com.evil/a"));
    }

    #[test]
    fn client_id_and_paging() {
        let html = r#"<script crossorigin src="https://a-v2.sndcdn.com/assets/0-abc.js"></script><script crossorigin src="https://a-v2.sndcdn.com/assets/55-4f29.js"></script>"#;
        assert_eq!(script_urls(html).len(), 2);
        assert_eq!(find_client_id(r#"x={client_id:"ZeISTkUY1Q4so1FsHJKZaJo898iWk0d3",y:1}"#).as_deref(), Some("ZeISTkUY1Q4so1FsHJKZaJo898iWk0d3"));
        assert_eq!(find_client_id("client_id=short"), None);
        assert_eq!(offset_of(Some(&json!("https://api-v2.soundcloud.com/search/tracks?q=a&offset=30&limit=30"))), Some(30));
        assert_eq!(offset_of(None), None);
        let (path, params) = api_path("https://api-v2.soundcloud.com/users/1/track_likes?offset=2026-10-30T10%3A00%3A00.000Z%2Cuser-track-likes%2C1&limit=200&client_id=x").unwrap();
        assert_eq!(path, "/users/1/track_likes");
        assert!(params.iter().any(|(k, v)| k == "offset" && v.starts_with("2026-10-30T10:00")));
        assert!(!params.iter().any(|(k, _)| k == "client_id"));
        assert_eq!(api_path("https://evil.example/users/1"), None);
    }
}
