//! ВКонтакте — только прослушивание под своим аккаунтом.
//!
//! Как это устроено и чего здесь намеренно нет:
//! * вход — как в NK-Player: обычная страница `vk.ru/login` прямо в главном окне (можно по QR-коду);
//!   пароль приложение не видит, сессия — куки в хранилище WebKit (не в config.json); выход удаляет эти куки;
//! * данные — тот же `al_audio.php`, что использует сайт: свои треки и поиск, по странице за раз,
//!   не чаще одного запроса в секунду, без повторов и фоновой подгрузки;
//! * ссылка на трек запрашивается только при нажатии «играть» и только на один трек;
//! * сначала — открытые ссылки, которые ВК отдаёт сам (`audio_streams`, mp3/aac/HLS); если их нет —
//!   ссылка `audio_api_unavailable` расшифровывается тем же способом, что в NK-Player (`decode_url`,
//!   по просьбе владельца, 2026-10-30);
//! * треки библиотеки кэшируются на диск и скачиваются (`cache`, просьба владельца 2026-10-30);
//! * VK Микс и плейлисты (свои и подборки ВК с главной музыки) — через API ВК с web-токеном самого сайта
//!   (`login.vk.ru/?act=web_token`, как браузер; просьба владельца 2026-11-01). Токен — только в памяти, запросы —
//!   в той же очереди `GAP`, только по действию пользователя (открыть раздел, плейлист, включить микс).

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const LOGIN_URL: &str = "https://vk.ru/login";
/// User-Agent как у Electron в NK-Player (Chromium без «Electron/…»). С UA WebKitGTK страница входа
/// vk.ru не догружается; тот же UA — у запросов к ВК и у mpv, чтобы сессия видела один браузер.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
/// не чаще одного запроса к ВК в секунду
const GAP: Duration = Duration::from_millis(1000);
const MAX_BODY: usize = 8 * 1024 * 1024;

const ERR_LIMIT: &str = "ВК ответил страницей вместо данных — похоже на временное ограничение. Попробуйте позже.";
const ERR_SESSION: &str = "Сессия ВКонтакте не распознана — войдите снова.";

type Res<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    /// owner_id_id
    pub id: String,
    /// id для reload_audio; None — ВК не дал трек для прослушивания
    pub reload: Option<String>,
    pub artist: String,
    pub title: String,
    pub duration: f64,
    pub cover: Option<String>,
    /// хэш удаления (есть у ваших аудиозаписей): удалить трек из «Моих треков» ВК
    pub del: Option<String>,
    /// трек из API (микс, плейлист): играет по ссылке из API (`vk_play_api`), а не через reload_audio
    #[serde(default)]
    pub api: bool,
}

/// Хэш из поля [13] ВК: только буквы и цифры, разумной длины.
pub fn valid_hash(h: &str) -> bool {
    !h.is_empty() && h.len() <= 64 && h.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Продолжение поиска (load_catalog_section).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct More {
    pub section_id: String,
    pub start_from: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub tracks: Vec<Track>,
    /// следующая страница «Моих треков»
    pub next_offset: Option<u64>,
    /// следующая страница поиска
    pub more: Option<More>,
}

pub struct Vk {
    /// клиент с хранилищем кук для текущей сессии: (отпечаток кук WebKit, клиент)
    http: std::sync::Mutex<Option<(String, reqwest::Client)>>,
    /// web-токен сайта для API (микс, плейлисты): (отпечаток кук, токен, id пользователя, до какого времени)
    token: std::sync::Mutex<Option<(String, String, i64, Instant)>>,
    /// access_key треков и плейлистов из API: «owner_id_id» / «pl:owner_id_id» → ключ
    keys: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// когда начался последний запрос; заодно выстраивает запросы в очередь (по одному, пока не придёт ответ)
    last: tokio::sync::Mutex<Option<Instant>>,
    profile: std::sync::Mutex<Option<Profile>>,
    /// номер последнего поиска из строки поиска: устаревший (вы печатаете дальше) в очереди не ждёт и не уходит в ВК
    live_search: std::sync::atomic::AtomicU64,
}

/// Поиск не выполнен: пока он ждал очереди, из строки поиска пришёл более новый запрос.
pub const ERR_SUPERSEDED: &str = "Поиск заменён более новым запросом";

// ───────────────────────── сессия ─────────────────────────

/// Кука из хранилища WebKit.
#[derive(Debug, Clone, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

/// Сессия: сайт, на котором выполнен вход, и куки ВК из WebKit.
#[derive(Debug, Clone)]
pub struct Session {
    /// https://vk.ru или https://vk.com
    pub base: &'static str,
    pub cookies: Vec<Cookie>,
}

impl Session {
    /// Отпечаток кук: изменились (новый вход/выход) — нужен новый клиент.
    fn fingerprint(&self) -> String {
        let mut v: Vec<String> = self.cookies.iter().map(|c| format!("{}|{}|{}", c.domain, c.name, c.value)).collect();
        v.sort();
        format!("{}\n{}", self.base, v.join("\n"))
    }

    /// Referer, с которым CDN ВК отдаёт аудио (как у плеера на сайте)
    pub fn referrer(&self) -> String {
        format!("{}/", self.base)
    }
}

fn is_session_cookie(name: &str, value: &str) -> bool {
    (name == "remixsid" || name == "remixnsid") && !value.is_empty()
}

/// Как в NK-Player: вход выполнен, если среди кук vk.ru / vk.com есть remixsid или remixnsid.
/// Возвращает сайт, для которого есть сессия (vk.ru важнее).
pub fn session_base<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Option<&'static str> {
    let mut found = None;
    for (name, value, domain) in cookies {
        if !is_session_cookie(name, value) {
            continue;
        }
        let d = domain.trim_start_matches('.').to_ascii_lowercase();
        if d == "vk.ru" || d.ends_with(".vk.ru") {
            return Some("https://vk.ru");
        }
        if d == "vk.com" || d.ends_with(".vk.com") {
            found = Some("https://vk.com");
        }
    }
    found
}

/// Куки ВК, пригодные для хранилища reqwest: только домены ВК, без управляющих символов.
pub fn vk_cookies(all: impl IntoIterator<Item = Cookie>) -> Vec<Cookie> {
    let bad = |s: &str| s.contains([';', '\r', '\n']);
    all.into_iter()
        .filter(|c| is_vk_domain(&c.domain))
        .filter(|c| !c.name.is_empty() && !c.name.contains('=') && !bad(&c.name) && !bad(&c.value))
        .filter(|c| !bad(&c.domain) && !bad(&c.path) && !c.domain.contains(['/', ' ', ',']))
        .collect()
}

/// Хранилище кук для запросов: заполняется куками WebKit, дальше само принимает Set-Cookie —
/// как браузерная сессия Electron в NK-Player (после входа ВК гоняет через login.php?act=slogin, выставляя куки).
fn jar_for(s: &Session) -> std::sync::Arc<reqwest::cookie::Jar> {
    let jar = reqwest::cookie::Jar::default();
    for c in &s.cookies {
        let host = c.domain.trim_start_matches('.');
        let Ok(url) = format!("https://{host}/").parse::<reqwest::Url>() else { continue };
        let path = if c.path.starts_with('/') { c.path.as_str() } else { "/" };
        // домен с точкой — кука и для поддоменов; без точки — только для этого хоста
        let domain = if c.domain.starts_with('.') { format!("; Domain={host}") } else { String::new() };
        jar.add_cookie_str(&format!("{}={}{domain}; Path={path}; Secure", c.name, c.value), &url);
    }
    std::sync::Arc::new(jar)
}

/// Куки этого домена принадлежат ВК (их удаляем при выходе).
pub fn is_vk_domain(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    ["vk.ru", "vk.com", "vk.id", "vkontakte.ru"]
        .iter()
        .any(|s| d == *s || d.ends_with(&format!(".{s}")))
}

/// id для reload_audio: owner_id, id и два хэша.
pub fn valid_reload_id(s: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    s.len() <= 128 && RE.get_or_init(|| Regex::new(r"^-?\d+_\d+_[0-9A-Za-z]+_[0-9A-Za-z]+$").unwrap()).is_match(s)
}

// ───────────────────────── разбор ответов ─────────────────────────

fn unescape_html(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"&(#\d+|#x[0-9a-fA-F]+|amp|quot|apos|lt|gt|nbsp);").unwrap());
    re.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let ch = match e {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "nbsp" => Some(' '),
            _ if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            _ => e[1..].parse().ok().and_then(char::from_u32),
        };
        ch.map(String::from).unwrap_or_else(|| c[0].to_string())
    })
    .trim()
    .to_string()
}

fn num(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Трек в ответах ВК — массив: [id, owner_id, url, title, artist, duration, …, flags(12), hashes(13), covers(14), …]
pub fn parse_track(v: &Value) -> Option<Track> {
    let a = v.as_array()?;
    let id = num(a.first())?;
    let owner = num(a.get(1))?;
    let h: Vec<&str> = a.get(13).and_then(Value::as_str).unwrap_or("").split('/').collect();
    let reload = match (h.get(2), h.get(5)) {
        (Some(x), Some(y)) if !x.is_empty() && !y.is_empty() => Some(format!("{owner}_{id}_{x}_{y}")),
        _ => None,
    }
    .filter(|r| valid_reload_id(r));
    // [13] = add/edit/action/delete/replace/url — хэши действий, как на сайте
    let del = h.get(3).filter(|x| valid_hash(x)).map(|x| x.to_string());
    let cover = a
        .get(14)
        .and_then(Value::as_str)
        // несколько размеров через запятую, от меньшего к большему — берём самую крупную (панель «Сейчас играет»)
        .and_then(|s| s.split(',').map(str::trim).filter(|x| !x.is_empty()).last())
        .map(unescape_html)
        .filter(|s| s.starts_with("https://") && s.len() < 2048);
    let text = |i: usize| a.get(i).and_then(Value::as_str).map(unescape_html).unwrap_or_default();
    Some(Track {
        id: format!("{owner}_{id}"),
        reload,
        title: text(3),
        artist: text(4),
        duration: a.get(5).and_then(Value::as_f64).unwrap_or(0.0),
        cover,
        del,
        api: false,
    })
}

fn parse_list(list: Option<&Value>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    list.and_then(Value::as_array)
        .map(|l| l.iter().filter_map(parse_track).filter(|t| seen.insert(t.id.clone())).collect())
        .unwrap_or_default()
}

/// Открытая ссылка на поток, которую ВК отдал сам (без расшифровки).
pub fn pick_stream(v: &Value, now: u64) -> Option<String> {
    static EXP: OnceLock<Regex> = OnceLock::new();
    let exp = EXP.get_or_init(|| Regex::new(r"[?&/]expires[=/](\d+)").unwrap());
    let a = v.as_array()?;
    let ok = |u: &str| u.starts_with("https://") && u.len() < 4096 && !u.chars().any(char::is_whitespace);
    let flags = match a.get(12) {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
        Some(o @ Value::Object(_)) => o.clone(),
        _ => Value::Null,
    };
    if let Some(streams) = flags.get("audio_streams").and_then(Value::as_array) {
        for kind in ["mp3", "aac", "hls_ts", "hls"] {
            let Some(url) = streams
                .iter()
                .filter(|s| s.get("type").and_then(Value::as_str) == Some(kind))
                .find_map(|s| s.get("url").and_then(Value::as_str))
            else {
                continue;
            };
            if !ok(url) {
                continue;
            }
            // протухшая (или протухающая через минуту) ссылка бесполезна
            let expires = exp.captures(url).and_then(|c| c[1].parse::<u64>().ok());
            if expires.is_some_and(|e| e < now + 60) {
                continue;
            }
            return Some(url.to_string());
        }
    }
    a.get(2)
        .and_then(Value::as_str)
        .filter(|u| ok(u) && !u.contains("audio_api_unavailable"))
        .map(str::to_string)
}

// ───────────────────────── расшифровка ссылок (как в NK-Player) ─────────────────────────
//
// Порт decodeVkAudioUrl из NK-Player (vk-audio.js) — побайтно, включая особенности JS: числа из строк
// (Number), ToInt32 у ^, остаток со знаком делимого, отрицательные индексы substr/splice. Проверено на
// векторах, полученных из оригинального JS (тест decode_matches_nk_player).

const VK_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN0PQRSTUVWXYZO123456789+/=";

/// JS ToInt32
fn to_i32(f: f64) -> i32 {
    if !f.is_finite() {
        return 0;
    }
    (f.trunc().rem_euclid(4294967296.0) as u64 as u32) as i32
}

/// JS Number(строка) для аргументов операций; None (нет аргумента) — undefined → NaN
fn js_num(arg: Option<&[u8]>) -> f64 {
    let Some(a) = arg else { return f64::NAN };
    let s = String::from_utf8_lossy(a);
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    if s.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
        return f64::NAN;
    }
    s.parse().unwrap_or(f64::NAN)
}

fn vk_b64(e: &[u8]) -> Option<Vec<u8>> {
    if e.is_empty() || e.len() % 4 == 1 {
        return None;
    }
    let mut r = Vec::with_capacity(e.len());
    let (mut t, mut o) = (0i64, 0i64);
    for &ch in e {
        let Some(i) = VK_ALPHABET.iter().position(|&c| c == ch) else { continue };
        t = if o % 4 != 0 { 64 * t + i as i64 } else { i as i64 };
        let cond = o % 4;
        o += 1;
        if cond != 0 {
            r.push((255 & (to_i32(t as f64) >> ((-2 * o) & 6))) as u8);
        }
    }
    Some(r)
}

fn shuffle_indexes(n: usize, t: f64) -> Vec<f64> {
    let mut res = vec![0.0; n];
    let mut t = t.abs();
    let nf = n as f64;
    for o in (0..n).rev() {
        let x = to_i32(nf * (o as f64 + 1.0)) ^ to_i32(t + o as f64);
        t = (x as f64) % nf;
        res[o] = t;
    }
    res
}

fn op_s(e: Vec<u8>, t: f64) -> Vec<u8> {
    let n = e.len();
    if n == 0 {
        return e;
    }
    let idx = shuffle_indexes(n, t);
    let mut a = e;
    for o in 1..n {
        // arr.splice(k, 1, arr[o]) с отрицательным k — от конца
        let k = idx[n - 1 - o];
        let k = if k < 0.0 { (n as f64 + k).max(0.0) } else { k } as usize;
        if k < n {
            a.swap(o, k);
        }
    }
    a
}

fn op_r(e: Vec<u8>, t: f64) -> Vec<u8> {
    let len = VK_ALPHABET.len() * 2;
    e.into_iter()
        .filter_map(|ch| {
            let Some(i) = VK_ALPHABET.iter().position(|&c| c == ch) else { return Some(ch) };
            // (ALPHABET + ALPHABET).substr(i - t, 1)
            let start = i as f64 - t;
            let start = if start.is_nan() { 0.0 } else { start.trunc() };
            let start = if start < 0.0 { (len as f64 + start).max(0.0) } else { start };
            (start < len as f64).then(|| VK_ALPHABET[start as usize % VK_ALPHABET.len()])
        })
        .collect()
}

/// Сырой результат расшифровки (как decodeVkAudioUrl без проверки на http).
fn decode_raw(url: &str, user_id: i64) -> Option<Vec<u8>> {
    let extra = url.split("?extra=").nth(1).filter(|e| !e.is_empty())?;
    let mut parts = extra.split('#');
    let p0 = parts.next()?;
    let ops = match parts.next() {
        Some(p) if !p.is_empty() => vk_b64(p.as_bytes())?,
        _ => Vec::new(),
    };
    let mut t = vk_b64(p0.as_bytes()).filter(|t| !t.is_empty())?;
    let list: Vec<&[u8]> = if ops.is_empty() { Vec::new() } else { ops.split(|&b| b == 9).collect() };
    for op in list.iter().rev() {
        let mut args = op.split(|&b| b == 11);
        let name = args.next().unwrap_or(&[]);
        let arg = args.next();
        t = match name {
            b"v" => {
                t.reverse();
                t
            }
            b"r" => op_r(t, js_num(arg)),
            b"s" => op_s(t, js_num(arg)),
            b"i" => op_s(t, (to_i32(js_num(arg)) ^ to_i32(user_id as f64)) as f64),
            b"x" => {
                let code = *arg?.first()?;
                t.into_iter().map(|b| b ^ code).collect()
            }
            _ => return None,
        };
    }
    Some(t)
}

/// Расшифровать ссылку `audio_api_unavailable.mp3?extra=…`; нужен id пользователя.
pub fn decode_url(url: &str, user_id: i64) -> Option<String> {
    if !url.contains("audio_api_unavailable") {
        return None;
    }
    let s = String::from_utf8(decode_raw(url, user_id)?).ok()?;
    (s.starts_with("https://") && s.len() < 4096 && !s.chars().any(char::is_whitespace)).then_some(s)
}

/// Из блоков поиска — «Все треки», иначе самый длинный, кроме «Мои треки».
fn pick_search_block(data: &Value) -> Option<&Value> {
    let blocks: Vec<&Value> = data
        .get("playlists")
        .and_then(Value::as_array)
        .map(|b| {
            b.iter()
                .filter(|p| p.get("list").and_then(Value::as_array).is_some_and(|l| !l.is_empty()))
                .collect()
        })
        .unwrap_or_default();
    let title = |p: &Value| p.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let len = |p: &Value| p.get("list").and_then(Value::as_array).map_or(0, Vec::len);
    blocks
        .iter()
        .find(|p| title(p) == "Все треки")
        .or_else(|| blocks.iter().filter(|p| title(p) != "Мои треки").max_by_key(|p| len(p)))
        .copied()
        .or_else(|| data.get("playlist").filter(|p| p.is_object()))
}

fn search_page(block: Option<&Value>) -> Page {
    let Some(b) = block else {
        return Page { tracks: Vec::new(), next_offset: None, more: None };
    };
    let s = |v: Option<&Value>| match v {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    let has_more = b.get("hasMore").is_some_and(|v| v.as_bool() == Some(true) || v.as_i64() == Some(1));
    let more = match (s(b.get("blockId")).or_else(|| s(b.get("id"))), s(b.get("nextOffset"))) {
        (Some(section_id), Some(start_from)) if has_more => Some(More { section_id, start_from }),
        _ => None,
    };
    Page { tracks: parse_list(b.get("list")), next_offset: None, more }
}

/// Ответ al_audio.php: JSON, иногда с префиксом `<!--`.
pub fn parse_al(text: &str) -> Res<Value> {
    serde_json::from_str(text.trim_start().trim_start_matches("<!--")).map_err(|_| ERR_LIMIT.to_string())
}

/// Ответ на действие: payload[0] = 0 — успех; иначе ВК вернул ошибку (текст — в payload[1][0]).
pub fn al_ok(v: &Value) -> Res<()> {
    let code = v.pointer("/payload/0");
    let ok = match code {
        Some(Value::Number(n)) => n.as_i64() == Some(0),
        Some(Value::String(s)) => s == "0",
        _ => false,
    };
    if ok {
        return Ok(());
    }
    let msg = v
        .pointer("/payload/1/0")
        .and_then(Value::as_str)
        .map(|m| unescape_html(&m.replace(['"', '\\'], "")))
        .filter(|m| !m.is_empty() && m.len() < 300 && !m.contains('<'));
    Err(match msg {
        Some(m) => format!("ВКонтакте не удалил трек: {m}"),
        None => "ВКонтакте не удалил трек".into(),
    })
}

fn payload(v: &Value, i: usize) -> Option<&Value> {
    v.get("payload")?.get(1)?.get(i)
}

/// id и имя со страницы /audio.
pub fn parse_profile(html: &str) -> Option<Profile> {
    static UID: OnceLock<Regex> = OnceLock::new();
    let uid = UID.get_or_init(|| Regex::new(r#""uid":"?(\d+)|window\.vk\s*=\s*\{"id":(\d+)"#).unwrap());
    let c = uid.captures(html)?;
    let id: i64 = c.get(1).or_else(|| c.get(2))?.as_str().parse().ok()?;
    let field = |name: &str| {
        let re = Regex::new(&format!(r#""{name}":("(?:[^"\\]|\\.)*")"#)).ok()?;
        serde_json::from_str::<String>(&re.captures(html)?[1]).ok()
    };
    let name = [field("first_name"), field("last_name")]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Some(Profile { id, name: if name.is_empty() { format!("id{id}") } else { name } })
}

// ───────────────────────── запросы ─────────────────────────

impl Vk {
    pub fn new() -> Vk {
        Vk {
            http: std::sync::Mutex::new(None),
            token: std::sync::Mutex::new(None),
            keys: std::sync::Mutex::new(std::collections::HashMap::new()),
            last: tokio::sync::Mutex::new(None),
            profile: std::sync::Mutex::new(None),
            live_search: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Забыть профиль (выход / новый вход).
    pub fn forget(&self) {
        *self.profile.lock().unwrap() = None;
        *self.http.lock().unwrap() = None;
        *self.token.lock().unwrap() = None;
    }

    /// Клиент для сессии; пока куки WebKit те же — тот же клиент (и накопленные им куки).
    fn client(&self, s: &Session) -> Res<reqwest::Client> {
        let fp = s.fingerprint();
        let mut g = self.http.lock().unwrap();
        if let Some((f, c)) = g.as_ref() {
            if *f == fp {
                return Ok(c.clone());
            }
        }
        let c = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .cookie_provider(jar_for(s))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| e.to_string())?;
        *g = Some((fp, c.clone()));
        Ok(c)
    }

    async fn send(
        &self,
        s: &Session,
        build: impl FnOnce(&reqwest::Client) -> reqwest::RequestBuilder,
    ) -> Res<String> {
        self.send_if(s, build, || true).await
    }

    /// `send`, но запрос уходит, только если `wanted()` всё ещё правда, когда до него дошла очередь.
    async fn send_if(
        &self,
        s: &Session,
        build: impl FnOnce(&reqwest::Client) -> reqwest::RequestBuilder,
        wanted: impl Fn() -> bool,
    ) -> Res<String> {
        let req = build(&self.client(s)?);
        // очередь: запросы по одному, следующий начинается не раньше чем через GAP после начала предыдущего
        // (раньше — после его ответа: поиск ВК отвечает ~2 с, и к каждому следующему запросу прибавлялась секунда)
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < GAP {
                tokio::time::sleep(GAP - t.elapsed()).await;
            }
        }
        if !wanted() {
            return Err(ERR_SUPERSEDED.into());
        }
        let started = Instant::now();
        let res = req.send().await;
        *last = Some(started);
        drop(last);
        let res = res.map_err(|e| format!("Нет связи с ВКонтакте: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("ВКонтакте ответил HTTP {}", res.status().as_u16()));
        }
        // увели на страницу входа — сессии нет
        let u = res.url();
        if u.path().starts_with("/login") || u.host_str().is_some_and(|h| h.starts_with("id.") || h.starts_with("login.")) {
            return Err(ERR_SESSION.into());
        }
        if res.content_length().is_some_and(|n| n as usize > MAX_BODY) {
            return Err("Слишком большой ответ ВКонтакте".into());
        }
        let text = res.text().await.map_err(|e| format!("Ответ ВКонтакте не прочитан: {e}"))?;
        if text.len() > MAX_BODY {
            return Err("Слишком большой ответ ВКонтакте".into());
        }
        Ok(text)
    }

    async fn al_audio(&self, s: &Session, form: &[(&str, String)]) -> Res<Value> {
        self.al_audio_if(s, form, || true).await
    }

    async fn al_audio_if(&self, s: &Session, form: &[(&str, String)], wanted: impl Fn() -> bool) -> Res<Value> {
        let act = form.iter().find(|(k, _)| *k == "act").map(|(_, v)| v.as_str()).unwrap_or("");
        let text = self
            .send_if(
                s,
                |c| {
                    c.post(format!("{}/al_audio.php?act={act}", s.base))
                        .header("X-Requested-With", "XMLHttpRequest")
                        .header("Referer", format!("{}/audio", s.base))
                        .form(form)
                },
                wanted,
            )
            .await?;
        parse_al(&text)
    }

    pub async fn profile(&self, s: &Session) -> Res<Profile> {
        let cached = self.profile.lock().unwrap().clone();
        if let Some(p) = cached {
            return Ok(p);
        }
        let html = self.send(s, |c| c.get(format!("{}/audio", s.base))).await?;
        let p = parse_profile(&html).ok_or(ERR_SESSION)?;
        *self.profile.lock().unwrap() = Some(p.clone());
        Ok(p)
    }

    /// «Мои треки», одна страница начиная с offset.
    pub async fn my_tracks(&self, s: &Session, offset: u64) -> Res<Page> {
        let uid = self.profile(s).await?.id.to_string();
        let v = self
            .al_audio(
                s,
                &[
                    ("act", "load_section".into()),
                    ("al", "1".into()),
                    ("claim", "0".into()),
                    ("context", "my".into()),
                    ("from_id", uid.clone()),
                    ("is_loading_all", "0".into()),
                    ("is_preload", "0".into()),
                    ("offset", offset.to_string()),
                    ("owner_id", uid),
                    ("playlist_id", "-1".into()),
                    ("type", "playlist".into()),
                    ("access_hash", String::new()),
                ],
            )
            .await?;
        let pl = payload(&v, 0).filter(|p| p.get("list").is_some_and(Value::is_array)).ok_or_else(|| {
            if offset == 0 { "Не удалось загрузить ваши треки — возможно, доступ к ним закрыт.".to_string() } else { ERR_LIMIT.to_string() }
        })?;
        let tracks = parse_list(pl.get("list"));
        let has_more = pl.get("hasMore").is_some_and(|v| v.as_bool() == Some(true) || v.as_i64() == Some(1));
        let next = num(pl.get("nextOffset")).and_then(|n| u64::try_from(n).ok()).filter(|&n| n > offset);
        Ok(Page { tracks, next_offset: next.filter(|_| has_more), more: None })
    }

    /// `live` — запрос из строки поиска: более новый такой же запрос отменяет этот, пока он ждёт очереди
    /// (вы печатаете дальше — промежуточные «lin», «linkin» не задерживают итоговый запрос).
    pub async fn search(&self, s: &Session, query: &str, more: Option<More>, live: bool) -> Res<Page> {
        use std::sync::atomic::Ordering::SeqCst;
        let my = if live { self.live_search.fetch_add(1, SeqCst) + 1 } else { 0 };
        let wanted = || my == 0 || self.live_search.load(SeqCst) == my;
        let v = match more {
            Some(m) => {
                self.al_audio_if(
                    s,
                    &[
                        ("act", "load_catalog_section".into()),
                        ("al", "1".into()),
                        ("section_id", m.section_id),
                        ("start_from", m.start_from),
                    ],
                    &wanted,
                )
                .await?
            }
            None => {
                let uid = self.profile(s).await?.id.to_string();
                self.al_audio_if(
                    s,
                    &[
                        ("act", "section".into()),
                        ("al", "1".into()),
                        ("claim", "0".into()),
                        ("is_layer", "0".into()),
                        ("owner_id", uid),
                        ("section", "search".into()),
                        ("q", query.into()),
                    ],
                    &wanted,
                )
                .await?
            }
        };
        let data = payload(&v, 1).cloned().unwrap_or(Value::Null);
        let block = pick_search_block(&data)
            .or_else(|| data.get("playlists").and_then(|p| p.get(0)));
        Ok(search_page(block))
    }

    /// Удалить трек из «Моих треков» ВК — как кнопка удаления на сайте (act=delete_audio с хэшем удаления).
    pub async fn delete(&self, s: &Session, id: &str, hash: &str) -> Res<()> {
        let (owner, aid) = id.split_once('_').ok_or("Неверный id трека")?;
        if owner.trim_start_matches('-').parse::<u64>().is_err() || aid.parse::<u64>().is_err() || !valid_hash(hash) {
            return Err("Неверный id трека".into());
        }
        let v = self
            .al_audio(
                s,
                &[
                    ("act", "delete_audio".into()),
                    ("al", "1".into()),
                    ("oid", owner.into()),
                    ("aid", aid.into()),
                    ("hash", hash.into()),
                    ("restore", "1".into()),
                ],
            )
            .await?;
        al_ok(&v)
    }

    /// Ссылка на поток одного трека — по нажатию «играть».
    pub async fn stream_url(&self, s: &Session, reload: &str) -> Res<String> {
        if !valid_reload_id(reload) {
            return Err("Неверный id трека".into());
        }
        let v = self
            .al_audio(s, &[("act", "reload_audio".into()), ("al", "1".into()), ("ids", reload.into())])
            .await?;
        let list = payload(&v, 0).and_then(Value::as_array).ok_or(
            "ВКонтакте сейчас не выдаёт ссылки (ограничение на число запросов). Попробуйте через несколько минут.",
        )?;
        let first = list.first().ok_or("ВКонтакте не отдал этот трек — возможно, он недоступен.")?;
        if let Some(url) = pick_stream(first, now_secs()) {
            return Ok(url);
        }
        // как в NK-Player: расшифровать ссылку из [2]
        let enc = first.get(2).and_then(Value::as_str).unwrap_or("");
        if enc.is_empty() {
            return Err("ВКонтакте не дал ссылку на этот трек — возможно, он недоступен в вашей стране или удалён.".into());
        }
        let uid = self.profile(s).await?.id;
        decode_url(enc, uid).ok_or_else(|| "Не удалось расшифровать ссылку на трек — возможно, ВК изменил формат.".into())
    }
}

// ───────────────────────── API ВК: VK Микс и плейлисты (web-токен сайта) ─────────────────────────

/// Приложение веб-версии ВК и версия API — как у самого сайта (vk.ru, 2026-11).
const API_APP: &str = "6287487";
const API_V: &str = "5.289";

/// Плейлист из API: свой или подборка ВК. access_key остаётся в Rust (`Vk::keys`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiPlaylist {
    pub owner_id: i64,
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    pub count: u64,
    pub cover: Option<String>,
    /// ваш плейлист (owner_id — вы): можно удалить (добавленный чужой — убрать из своих)
    #[serde(default)]
    pub mine: bool,
    /// ваш собственный, не добавленный чужой: можно переименовать, добавлять и убирать треки
    #[serde(default)]
    pub editable: bool,
    /// «Слушайте друг друга»: чей плейлист (имя друга из `profiles` / сообщества из `groups`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    /// «Слушайте друг друга»: совпадение с вашим вкусом, % (`percentage` 0…1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_pct: Option<u8>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Mix {
    pub id: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlaylistGroup {
    pub title: String,
    pub playlists: Vec<ApiPlaylist>,
}

/// Главная музыки: миксы и подборки плейлистов (как на сайте, без блоков «Мои треки», жанров и т. п.).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Home {
    pub mixes: Vec<Mix>,
    pub groups: Vec<PlaylistGroup>,
}

/// Трек из API → (трек, ссылка на поток, access_key). Без ссылки — ВК не даёт слушать (api = false).
pub fn api_track(a: &Value) -> Option<(Track, Option<String>, Option<String>)> {
    let owner = a.get("owner_id")?.as_i64()?;
    let id = a.get("id")?.as_i64()?;
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let url = Some(s("url")).filter(|u| u.starts_with("https://") && u.len() < 4096);
    let cover = ["photo_600", "photo_300", "photo_270", "photo_135"]
        .iter()
        .find_map(|k| a.pointer(&format!("/album/thumb/{k}")).and_then(Value::as_str))
        .filter(|u| u.starts_with("https://") && u.len() < 2048)
        .map(str::to_string);
    let key = Some(s("access_key")).filter(|k| valid_hash(k));
    let t = Track {
        id: format!("{owner}_{id}"),
        reload: None,
        artist: s("artist"),
        title: s("title"),
        duration: a.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
        cover,
        del: None,
        api: url.is_some(),
    };
    Some((t, url, key))
}

fn api_playlist(p: &Value) -> Option<(ApiPlaylist, Option<String>)> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let cover = ["photo_600", "photo_300", "photo_270", "photo_135"]
        .iter()
        .find_map(|k| p.pointer(&format!("/photo/{k}")).or_else(|| p.pointer(&format!("/thumbs/0/{k}"))).and_then(Value::as_str))
        .or_else(|| p.get("cover").and_then(Value::as_str))
        .filter(|u| u.starts_with("https://") && u.len() < 2048)
        .map(str::to_string);
    let pl = ApiPlaylist {
        owner_id: p.get("owner_id")?.as_i64()?,
        id: p.get("id")?.as_i64()?,
        title: s("title").chars().take(200).collect(),
        subtitle: if s("subtitle").is_empty() { s("description") } else { s("subtitle") }.chars().take(200).collect(),
        count: p.get("count").and_then(Value::as_u64).unwrap_or(0),
        cover,
        mine: false,
        // добавленный к себе чужой плейлист — копия с полем original
        editable: p.get("original").is_none_or(Value::is_null),
        owner_name: None,
        match_pct: None,
    };
    Some((pl, Some(s("access_key")).filter(|k| valid_hash(k))))
}

/// Главная из ответа `catalog.getAudio`: миксы и блоки плейлистов первого раздела, по заголовкам блоков.
pub fn parse_home(r: &Value) -> (Home, Vec<(String, String)>) {
    let mut keys = Vec::new();
    let mut by_id = std::collections::HashMap::new();
    for p in r.get("playlists").and_then(Value::as_array).into_iter().flatten() {
        if let Some((mut pl, key)) = api_playlist(p) {
            pl.editable = false;
            if let Some(k) = key {
                keys.push((format!("pl:{}_{}", pl.owner_id, pl.id), k));
            }
            by_id.insert(format!("{}_{}", pl.owner_id, pl.id), pl);
        }
    }
    // имена владельцев: друзья — profiles (id > 0), сообщества — groups (owner_id = −id)
    let mut names = std::collections::HashMap::new();
    for u in r.get("profiles").and_then(Value::as_array).into_iter().flatten() {
        let first = u.get("first_name").and_then(Value::as_str).unwrap_or("").trim();
        let last = u.get("last_name").and_then(Value::as_str).unwrap_or("").trim();
        if let Some(id) = u.get("id").and_then(Value::as_i64) {
            names.insert(id, format!("{first} {last}").trim().to_string());
        }
    }
    for g in r.get("groups").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(id), Some(n)) = (g.get("id").and_then(Value::as_i64), g.get("name").and_then(Value::as_str)) {
            names.insert(-id, n.trim().to_string());
        }
    }
    // плейлисты друзей («Слушайте друг друга») — обложка, автор и совпадение отдельно, название — из общего списка
    for p in r.get("recommended_playlists").and_then(Value::as_array).into_iter().flatten() {
        let owner = p.get("owner_id").and_then(Value::as_i64).unwrap_or(0);
        let id = format!("{owner}_{}", p.get("id").and_then(Value::as_i64).unwrap_or(0));
        let Some(pl) = by_id.get_mut(&id) else { continue };
        if let Some(c) = p.get("cover").and_then(Value::as_str) {
            if pl.cover.is_none() && c.starts_with("https://") && c.len() < 2048 {
                pl.cover = Some(c.to_string());
            }
        }
        pl.owner_name = names.get(&owner).filter(|n| !n.is_empty()).map(|n| n.chars().take(100).collect());
        pl.match_pct = p.get("percentage").and_then(Value::as_f64).filter(|x| (0.0..=1.0).contains(x)).map(|x| (x * 100.0).round() as u8);
    }
    let mixes = r
        .get("audio_stream_mixes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?;
            if id.is_empty() || id.len() > 40 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return None;
            }
            let title = m.pointer("/stream_mix/title").or_else(|| m.pointer("/titles/common_state")).and_then(Value::as_str).unwrap_or("VK Микс");
            Some(Mix { id: id.into(), title: title.into(), description: m.get("description").and_then(Value::as_str).unwrap_or("").into() })
        })
        .collect();
    let mut groups: Vec<PlaylistGroup> = Vec::new();
    let mut header = String::new();
    let blocks = r.pointer("/catalog/sections/0/blocks").and_then(Value::as_array).cloned().unwrap_or_default();
    for b in blocks {
        let layout = b.pointer("/layout/name").and_then(Value::as_str).unwrap_or("");
        if layout.starts_with("header") {
            header = b.pointer("/layout/title").and_then(Value::as_str).unwrap_or("").to_string();
            continue;
        }
        let dt = b.get("data_type").and_then(Value::as_str).unwrap_or("");
        if dt != "music_playlists" && dt != "music_recommended_playlists" {
            continue;
        }
        let list: Vec<ApiPlaylist> = b
            .get("playlists_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().and_then(|id| by_id.get(id)).cloned())
            .collect();
        if !list.is_empty() {
            groups.push(PlaylistGroup { title: if header.is_empty() { "Плейлисты".into() } else { header.clone() }, playlists: list });
        }
    }
    (Home { mixes, groups }, keys)
}

fn api_hosts(base: &str) -> (&'static str, &'static str) {
    if base.ends_with("vk.com") { ("https://login.vk.com", "https://api.vk.com") } else { ("https://login.vk.ru", "https://api.vk.ru") }
}

impl Vk {
    /// POST в очереди запросов (GAP), без проверки «увели на вход» — для login.vk.ru и api.vk.ru.
    async fn post_json(&self, s: &Session, url: &str, form: &[(&str, String)]) -> Res<Value> {
        let req = self.client(s)?.post(url).header("Origin", s.base).header("Referer", s.referrer()).form(form);
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < GAP {
                tokio::time::sleep(GAP - t.elapsed()).await;
            }
        }
        let started = Instant::now();
        let res = req.send().await;
        *last = Some(started);
        drop(last);
        let res = res.map_err(|e| format!("Нет связи с ВКонтакте: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("ВКонтакте ответил HTTP {}", res.status().as_u16()));
        }
        let text = res.text().await.map_err(|e| format!("ВКонтакте: {e}"))?;
        if text.len() > MAX_BODY {
            return Err("Слишком большой ответ ВКонтакте".into());
        }
        serde_json::from_str(&text).map_err(|_| ERR_LIMIT.to_string())
    }

    /// web-токен сайта: тот же, что получает браузер (login.vk.ru/?act=web_token по кукам сессии). Только в памяти.
    async fn web_token(&self, s: &Session) -> Res<(String, i64)> {
        let fp = s.fingerprint();
        if let Some((f, t, uid, until)) = self.token.lock().unwrap().clone() {
            if f == fp && Instant::now() < until {
                return Ok((t, uid));
            }
        }
        let (login, _) = api_hosts(s.base);
        let v = self
            .post_json(s, &format!("{login}/?act=web_token"), &[("version", "1".into()), ("app_id", API_APP.into())])
            .await?;
        let d = v.get("data").filter(|_| v.get("type").and_then(Value::as_str) == Some("okay")).ok_or(ERR_SESSION)?;
        let tok = d.get("access_token").and_then(Value::as_str).filter(|t| !t.is_empty() && t.len() < 1024).ok_or(ERR_SESSION)?;
        let uid = d.get("user_id").and_then(Value::as_i64).unwrap_or(0);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let left = d.get("expires").and_then(Value::as_u64).map(|e| e.saturating_sub(now + 60)).unwrap_or(600).min(3600);
        *self.token.lock().unwrap() = Some((fp, tok.to_string(), uid, Instant::now() + Duration::from_secs(left)));
        Ok((tok.to_string(), uid))
    }

    /// Метод API. Токен устарел (ошибка 5) — один раз берём новый.
    async fn api(&self, s: &Session, method: &str, params: &[(&str, String)]) -> Res<Value> {
        let (_, api) = api_hosts(s.base);
        for attempt in 0..2 {
            let (tok, _) = self.web_token(s).await?;
            let mut form: Vec<(&str, String)> = vec![("access_token", tok)];
            form.extend(params.iter().cloned());
            let v = self.post_json(s, &format!("{api}/method/{method}?v={API_V}&client_id={API_APP}"), &form).await?;
            if let Some(e) = v.get("error") {
                let code = e.get("error_code").and_then(Value::as_i64).unwrap_or(0);
                if code == 5 && attempt == 0 {
                    *self.token.lock().unwrap() = None;
                    continue;
                }
                return Err(match code {
                    5 => ERR_SESSION.to_string(),
                    6 | 9 | 29 => "ВКонтакте ограничил число запросов. Попробуйте через несколько минут.".to_string(),
                    _ => format!("ВКонтакте: {}", e.get("error_msg").and_then(Value::as_str).unwrap_or("ошибка API")),
                });
            }
            return v.get("response").cloned().ok_or_else(|| ERR_LIMIT.to_string());
        }
        Err(ERR_SESSION.into())
    }

    pub fn remember_keys(&self, keys: impl IntoIterator<Item = (String, String)>) {
        let mut g = self.keys.lock().unwrap();
        if g.len() > 20_000 {
            g.clear();
        }
        g.extend(keys);
    }

    fn tracks_from(&self, items: Option<&Value>) -> Vec<(Track, Option<String>)> {
        let mut out = Vec::new();
        let mut keys = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for a in items.and_then(Value::as_array).into_iter().flatten() {
            if let Some((t, url, key)) = api_track(a) {
                if !seen.insert(t.id.clone()) {
                    continue;
                }
                if let Some(k) = key {
                    keys.push((t.id.clone(), k));
                }
                out.push((t, url));
            }
        }
        self.remember_keys(keys);
        out
    }

    /// Главная музыки: VK Микс и подборки плейлистов.
    pub async fn home(&self, s: &Session) -> Res<Home> {
        let r = self.api(s, "catalog.getAudio", &[("need_blocks", "1".into())]).await?;
        let (home, keys) = parse_home(&r);
        self.remember_keys(keys);
        Ok(home)
    }

    /// Ваши плейлисты (созданные и добавленные).
    pub async fn my_playlists(&self, s: &Session) -> Res<Vec<ApiPlaylist>> {
        let (_, uid) = self.web_token(s).await?;
        let r = self.api(s, "audio.getPlaylists", &[("owner_id", uid.to_string()), ("count", "100".into())]).await?;
        let mut keys = Vec::new();
        let list = r
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(api_playlist)
            .map(|(mut pl, k)| {
                pl.mine = pl.owner_id == uid;
                pl.editable &= pl.mine;
                if let Some(k) = k {
                    keys.push((format!("pl:{}_{}", pl.owner_id, pl.id), k));
                }
                pl
            })
            .collect();
        self.remember_keys(keys);
        Ok(list)
    }

    /// Треки плейлиста, страница из 200 начиная с offset.
    pub async fn playlist(&self, s: &Session, owner: i64, id: i64, offset: u64) -> Res<(Vec<(Track, Option<String>)>, Option<u64>)> {
        let key = self.keys.lock().unwrap().get(&format!("pl:{owner}_{id}")).cloned().unwrap_or_default();
        let r = self
            .api(s, "audio.get", &[("owner_id", owner.to_string()), ("playlist_id", id.to_string()), ("access_key", key), ("offset", offset.to_string()), ("count", "200".into())])
            .await?;
        let tracks = self.tracks_from(r.get("items"));
        let total = r.get("count").and_then(Value::as_u64).unwrap_or(0);
        let next = offset + 200;
        Ok((tracks, (next < total).then_some(next)))
    }

    /// Следующие треки микса (каждый вызов — новые 2–3 трека; бесконечный, как на сайте).
    pub async fn mix(&self, s: &Session, mix_id: &str) -> Res<Vec<(Track, Option<String>)>> {
        let r = self.api(s, "audio.getStreamMixAudios", &[("mix_id", mix_id.into()), ("count", "10".into())]).await?;
        Ok(self.tracks_from(Some(&r)))
    }

    // ── управление своими плейлистами (просьба владельца, 2026-11-02) ──

    /// Ваш id (из web-токена); плейлист чужого владельца менять не даём.
    async fn own(&self, s: &Session, owner: i64) -> Res<i64> {
        let (_, uid) = self.web_token(s).await?;
        if uid == 0 || owner != uid {
            return Err("Это не ваш плейлист".into());
        }
        Ok(uid)
    }

    /// Ответ «готово»: 1 / true / объект.
    fn done(r: &Value, what: &str) -> Res<()> {
        match r {
            Value::Number(n) if n.as_i64() == Some(0) => Err(format!("ВКонтакте не {what}")),
            Value::Bool(false) | Value::Null => Err(format!("ВКонтакте не {what}")),
            _ => Ok(()),
        }
    }

    /// Новый плейлист (пустой) — в ваших плейлистах.
    pub async fn playlist_create(&self, s: &Session, title: &str) -> Res<ApiPlaylist> {
        let (_, uid) = self.web_token(s).await?;
        let r = self.api(s, "audio.createPlaylist", &[("owner_id", uid.to_string()), ("title", title.into())]).await?;
        let (mut pl, _) = api_playlist(&r).ok_or("ВКонтакте не создал плейлист")?;
        pl.mine = pl.owner_id == uid;
        pl.editable = pl.mine;
        Ok(pl)
    }

    pub async fn playlist_rename(&self, s: &Session, owner: i64, id: i64, title: &str) -> Res<()> {
        self.own(s, owner).await?;
        let r = self
            .api(s, "audio.editPlaylist", &[("owner_id", owner.to_string()), ("playlist_id", id.to_string()), ("title", title.into())])
            .await?;
        Self::done(&r, "переименовал плейлист")
    }

    /// Удалить свой плейлист (добавленный чужой — убрать из своих).
    pub async fn playlist_delete(&self, s: &Session, owner: i64, id: i64) -> Res<()> {
        self.own(s, owner).await?;
        let r = self.api(s, "audio.deletePlaylist", &[("owner_id", owner.to_string()), ("playlist_id", id.to_string())]).await?;
        Self::done(&r, "удалил плейлист")
    }

    /// Трек в свой плейлист. Трек из API — с его access_key (без него ВК может не дать чужую запись).
    pub async fn playlist_add(&self, s: &Session, owner: i64, id: i64, track: &str, saved_key: Option<&str>) -> Res<()> {
        self.own(s, owner).await?;
        let audio = match self.access_key(track).or_else(|| saved_key.filter(|k| valid_hash(k)).map(str::to_string)) {
            Some(k) => format!("{track}_{k}"),
            None => track.to_string(),
        };
        let r = self
            .api(s, "audio.addToPlaylist", &[("owner_id", owner.to_string()), ("playlist_id", id.to_string()), ("audio_ids", audio)])
            .await?;
        Self::done(&r, "добавил трек в плейлист")
    }

    pub async fn playlist_remove(&self, s: &Session, owner: i64, id: i64, track: &str) -> Res<()> {
        self.own(s, owner).await?;
        let r = self
            .api(s, "audio.removeFromPlaylist", &[("owner_id", owner.to_string()), ("playlist_id", id.to_string()), ("audio_ids", track.into())])
            .await?;
        Self::done(&r, "убрал трек из плейлиста")
    }

    /// Чужой плейлист (подборка, плейлист друга) — к себе.
    pub async fn playlist_follow(&self, s: &Session, owner: i64, id: i64) -> Res<()> {
        let key = self.keys.lock().unwrap().get(&format!("pl:{owner}_{id}")).cloned().unwrap_or_default();
        let r = self
            .api(s, "audio.followPlaylist", &[("owner_id", owner.to_string()), ("playlist_id", id.to_string()), ("access_key", key)])
            .await?;
        Self::done(&r, "добавил плейлист")
    }

    /// access_key трека из последних ответов API (для трека, который добавляют в библиотеку).
    pub fn access_key(&self, id: &str) -> Option<String> {
        self.keys.lock().unwrap().get(id).cloned()
    }

    /// Текст трека от самого ВК (`audio.getLyrics`, как плеер сайта): строки, у синхронизированного — без меток
    /// времени; кто предоставил. Нет текста (ошибка 104) — None. Первым шагом поиска текста «как в VK MP3 Mod».
    pub async fn lyrics(&self, s: &Session, id: &str) -> Res<Option<(String, String)>> {
        let key = self.keys.lock().unwrap().get(id).cloned();
        let audio = match key {
            Some(k) => format!("{id}_{k}"),
            None => id.to_string(),
        };
        let r = match self.api(s, "audio.getLyrics", &[("audio_id", audio)]).await {
            Ok(r) => r,
            Err(e) if e.contains("Not found") => return Ok(None),
            Err(e) => return Err(e),
        };
        let l = r.get("lyrics");
        let lines: Vec<String> = match l.and_then(|l| l.get("timestamps")).and_then(Value::as_array) {
            Some(ts) => ts.iter().map(|x| x.get("line").and_then(Value::as_str).unwrap_or("").to_string()).collect(),
            None => l
                .and_then(|l| l.get("text"))
                .and_then(Value::as_array)
                .map(|t| t.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
        };
        let text = lines.join("\n").trim().to_string();
        let credits = r.get("credits").and_then(Value::as_str).unwrap_or("").chars().take(200).collect();
        Ok((!text.is_empty()).then_some((text, credits)))
    }

    /// Свежая ссылка на трек из API (старая истекла). saved_key — access_key из библиотеки, если в памяти нет.
    pub async fn api_stream(&self, s: &Session, id: &str, saved_key: Option<&str>) -> Res<String> {
        let key = self.access_key(id).or_else(|| saved_key.filter(|k| valid_hash(k)).map(str::to_string));
        let audios = match key {
            Some(k) => format!("{id}_{k}"),
            None => id.to_string(),
        };
        let r = self.api(s, "audio.getById", &[("audios", audios)]).await?;
        self.tracks_from(Some(&r))
            .into_iter()
            .find_map(|(t, url)| (t.id == id).then_some(url).flatten())
            .ok_or_else(|| "ВКонтакте не дал ссылку на этот трек — возможно, он недоступен в вашей стране или удалён.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cookies_filtered() {
        let c = |n: &str, v: &str, d: &str| Cookie { name: n.into(), value: v.into(), domain: d.into(), path: "/".into() };
        let got = vk_cookies([
            c("remixsid", "x", ".vk.ru"),
            c("bad;name", "1", ".vk.ru"),
            c("b", "2\n", ".vk.ru"),
            c("other", "1", ".example.com"),
            c("remixstid", "y", "login.vk.ru"),
        ]);
        assert_eq!(got.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["remixsid", "remixstid"]);

        // хранилище отдаёт куки домена с точкой и поддоменам, без точки — только своему хосту
        let s = Session { base: "https://vk.ru", cookies: got };
        let jar = jar_for(&s);
        use reqwest::cookie::CookieStore;
        let h = |u: &str| jar.cookies(&u.parse().unwrap()).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        assert_eq!(h("https://vk.ru/audio"), "remixsid=x");
        assert!(h("https://login.vk.ru/").contains("remixstid=y"));
        // после перезапуска ВК обновляет сессию через login.vk.ru (role=fast) — туда нужны куки .vk.ru
        assert!(h("https://login.vk.ru/").contains("remixsid=x"));
    }

    #[test]
    fn session_like_nk_player() {
        assert_eq!(session_base([("remixsid", "x", ".vk.ru")]), Some("https://vk.ru"));
        assert_eq!(session_base([("remixnsid", "x", ".vk.com")]), Some("https://vk.com"));
        assert_eq!(
            session_base([("remixnsid", "x", ".vk.com"), ("remixsid", "y", "login.vk.ru")]),
            Some("https://vk.ru")
        );
        assert_eq!(session_base([("remixsid", "", ".vk.ru"), ("remixlang", "0", ".vk.ru")]), None);
        assert_eq!(session_base([("remixsid", "x", "vk.ru.evil.com")]), None);
    }

    #[test]
    fn domains() {
        assert!(is_vk_domain(".vk.ru"));
        assert!(is_vk_domain("login.vk.com"));
        assert!(is_vk_domain("id.vk.id"));
        assert!(!is_vk_domain("notvk.ru"));
        assert!(!is_vk_domain("vk.ru.evil.com"));
    }

    #[test]
    fn reload_ids() {
        assert!(valid_reload_id("-2001_456239017_ab12_cd34"));
        assert!(!valid_reload_id("1_2_ab"));
        assert!(!valid_reload_id("1_2_a b_c"));
        assert!(!valid_reload_id("1_2_a&ids=3_c"));
    }

    fn track(url: &str, flags: Value) -> Value {
        json!([456, -2001, url, "Song &amp; Co", "Artist &#39;s", 215, 0, 0, "", 0, 66, "", flags,
               "a/b/hash1//c/hash2/", "https://sun9-1.userapi.com/c.jpg,https://x/y.jpg"])
    }

    #[test]
    fn track_parsed() {
        let t = parse_track(&track("", json!(""))).unwrap();
        assert_eq!(t.id, "-2001_456");
        assert_eq!(t.reload.as_deref(), Some("-2001_456_hash1_hash2"));
        assert_eq!(t.title, "Song & Co");
        assert_eq!(t.artist, "Artist 's");
        assert_eq!(t.duration, 215.0);
        assert_eq!(t.cover.as_deref(), Some("https://x/y.jpg")); // крупная — последняя
        assert_eq!(t.del, None); // в тестовом [13] хэш удаления пустой
        let mut with_del = track("", json!(""));
        with_del[13] = json!("add1/edit1/act1/del1/rep1/url1");
        assert_eq!(parse_track(&with_del).unwrap().del.as_deref(), Some("del1"));
        assert_eq!(parse_track(&with_del).unwrap().reload.as_deref(), Some("-2001_456_act1_url1"));
        assert!(parse_track(&json!({"a": 1})).is_none());
    }

    #[test]
    fn streams_only_open_links() {
        let now = 1_000_000;
        // зашифрованная ссылка — не играем
        let enc = track("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=abc", json!(""));
        assert_eq!(pick_stream(&enc, now), None);
        // audio_streams: mp3 важнее hls, протухшая пропускается
        let flags = json!({"audio_streams": [
            {"type": "hls", "url": "https://cs1.okcdn.ru/a.m3u8"},
            {"type": "mp3", "url": "https://cs1.okcdn.ru/a.mp3?expires=999000"},
            {"type": "aac", "url": "https://cs1.okcdn.ru/a.aac?expires=2000000"}
        ]});
        let t = track("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=abc", Value::String(flags.to_string()));
        assert_eq!(pick_stream(&t, now).as_deref(), Some("https://cs1.okcdn.ru/a.aac?expires=2000000"));
        // открытая ссылка в [2]
        assert_eq!(pick_stream(&track("https://cs1.vkuseraudio.net/a.mp3", json!("")), now).as_deref(),
                   Some("https://cs1.vkuseraudio.net/a.mp3"));
        // не https — нет
        assert_eq!(pick_stream(&track("http://x/a.mp3", json!("")), now), None);
    }

    #[test]
    fn al_and_search() {
        let v = parse_al(r#"<!--{"payload":[0,[{"list":[[1,2,"","t","a",10]],"hasMore":true,"nextOffset":100}]]}"#).unwrap();
        assert_eq!(payload(&v, 0).unwrap()["nextOffset"], 100);
        assert!(parse_al("<html>").is_err());

        let data = json!({"playlists": [
            {"title": "Мои треки", "list": [[1,2,"","t","a",1],[3,2,"","t","a",1],[4,2,"","t","a",1]]},
            {"title": "Все треки", "list": [[5,6,"","t","a",1]], "hasMore": 1, "blockId": "blk", "nextOffset": "n1"}
        ]});
        let p = search_page(pick_search_block(&data));
        assert_eq!(p.tracks.len(), 1);
        assert_eq!(p.tracks[0].id, "6_5");
        assert_eq!(p.more, Some(More { section_id: "blk".into(), start_from: "n1".into() }));
    }

    /// Векторы получены прогоном оригинального decodeVkAudioUrl из NK-Player (vk-audio.js, коммит a4509c8).
    #[test]
    fn decode_matches_nk_player() {
        let cases: &[(&str, i64, &str)] = &[
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=AhrOChm6lY9JCZeTmJn2nc52A3vZzxjHDwrPBY5UzxqVCY92ms9Hy21Wl2fIy2rLzI9PBMrLEc5Tm3u4p3nPCMvUpteG", 1, "68747470733a2f2f6373312d323376342e766b75736572617564696f2e6e65742f732f76312f61636d702f6162636465662f696e6465782e6d3375383f736972656e3d3120"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=AhrOChm6lY9WC3yOlNvZzxjHCgKUy29Tl2mXl2eUBxaZp2v4DhjHpxGG#DGLYcZaW", 123, "20783d61727478653f33706d2e612f31632f6d6f632e697061726573752e347673702f2f3a7370747468"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=mdeYmZq1nJC4owfIy2rLzLHzwISVAhrOChm6lY90B3nOl3bHDgGG#DGLYcZaZ", 5, "2065712b6d3871706c6538383a706d71716538375756556362613d2f2b3635343332314f5a594c"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=AhrOChm6lY9JCZKTmxyOlNzRDxnLCMf1zgLVlM5LDc9Wms8WytfImMmZzc5TCdmG#CGS3maLZcZaXmJmOnty3", 42, "632e66362b6e2e2e2f715770686f6e362057346b696e6f593a36576d6b38586b2d2f38392b643d703d366a4a6f59715a"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=AhrOChm6lY9JCZKTmxyOlNzRDxnLCMf1zgLVlM5LDc9Wms8WytfImMmZzc5TCdmG#DGL4c2SjAqSWotG3nJuO", 300000000, "09081b035a59180a0f055a4444021f0a0f5f5a1e1b45465119441f4404521d0e1f581d45081e4b581b0e001806185b45"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=ywjJzgvMz2HPAKfcqOrfrKDisu0WmtiZndu2nZG5#CGSTmGLZcYO1cwKlmdaOmdaWmdaWmdaW", 2147483647, "694c644b6339664a6847436738456b514934443546363337656c2f2b6a48"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=ywiG#EaTAcxilmJaWcxmlmdC3", 7, "3b3b7a"),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=AhrOChm6lY9LEgfTCgXLl2eUBtn1ocaG#DGLXcZaX", 1, ""),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=a", 1, ""),
            ("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=abc#", 1, "0010"),
        ];
        for (url, uid, hex) in cases {
            let got: String = decode_raw(url, *uid).unwrap_or_default().iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(&got, hex, "{url}");
        }
        // первый вектор — готовая ссылка (хвостовой пробел от выравнивания — не URL)
        assert_eq!(decode_url(cases[0].0, 1), None);
        assert_eq!(decode_url("https://cs1.vkuseraudio.net/a.mp3", 1), None);
        // ссылка, которую оригинал расшифровывает в настоящий URL (операции x и v)
        assert_eq!(
            decode_url("https://vk.ru/mp3/audio_api_unavailable.mp3?extra=uX5ybKutdG8faKqicqPecaPewH1egeqFdGvfbaiphG0zdHGEab1fxX1ArLiycereurGBhX8d#DGL4c2S", 1).as_deref(),
            Some("https://cs9-1v4.vkuseraudio.net/s/v1/ac/abc/index.m3u8")
        );
    }

    #[test]
    fn delete_answer() {
        assert!(al_ok(&json!({"payload": [0, []]})).is_ok());
        assert!(al_ok(&json!({"payload": ["0", []]})).is_ok());
        assert_eq!(al_ok(&json!({"payload": ["8", ["\"Доступ запрещён\""]]})).unwrap_err(), "ВКонтакте не удалил трек: Доступ запрещён");
        assert_eq!(al_ok(&json!({"payload": [3, ["<b>x</b>"]]})).unwrap_err(), "ВКонтакте не удалил трек");
        assert!(al_ok(&json!({})).is_err());
    }

    #[test]
    fn profile_parsed() {
        let html = r#"<script>window.vk = {"id":123,"first_name":"Иван","last_name":"P\"x"}</script>"#;
        let p = parse_profile(html).unwrap();
        assert_eq!(p.id, 123);
        assert_eq!(p.name, "Иван P\"x");
        assert!(parse_profile("<html>login</html>").is_none());
    }

    #[test]
    fn api_home_and_tracks() {
        let r = serde_json::json!({
            "audio_stream_mixes": [{"id": "common", "description": "Рекомендации", "stream_mix": {"id": "common", "title": "Слушать VK Микс"}}, {"id": "bad id!"}],
            "playlists": [
                {"owner_id": 1, "id": -21, "title": "Для вас", "count": 100, "access_key": "abc123", "photo": {"photo_300": "https://x/1.jpg"}},
                {"owner_id": -5, "id": 7, "title": "Редакция", "count": 30, "photo": {"photo_600": "https://x/2.jpg"}},
                {"owner_id": 9, "id": 3, "title": "Друга", "count": 12}
            ],
            "recommended_playlists": [{"owner_id": 9, "id": 3, "cover": "https://x/3.png", "percentage": 0.98}],
            "profiles": [{"id": 9, "first_name": "Иван", "last_name": "Петров"}],
            "catalog": {"sections": [{"blocks": [
                {"data_type": "audio_stream_mixes", "layout": {"name": "audio_stream_mix_interactive"}},
                {"data_type": "none", "layout": {"name": "header", "title": "Мои треки"}},
                {"data_type": "music_audios", "layout": {"name": "triple_stacked_slider"}},
                {"data_type": "none", "layout": {"name": "header", "title": "Собрано алгоритмами"}},
                {"data_type": "music_playlists", "layout": {"name": "recomms_slider"}, "playlists_ids": ["1_-21", "404_1"]},
                {"data_type": "none", "layout": {"name": "header_extended", "title": "Слушайте друг друга"}},
                {"data_type": "music_recommended_playlists", "layout": {"name": "large_slider"}, "playlists_ids": ["9_3"]},
                {"data_type": "none", "layout": {"name": "header", "title": "Собрано редакцией"}},
                {"data_type": "music_playlists", "layout": {"name": "large_slider"}, "playlists_ids": ["-5_7"]}
            ]}]}
        });
        let (home, keys) = parse_home(&r);
        assert_eq!(home.mixes.len(), 1);
        assert_eq!(home.mixes[0].title, "Слушать VK Микс");
        let titles: Vec<(&str, usize)> = home.groups.iter().map(|g| (g.title.as_str(), g.playlists.len())).collect();
        assert_eq!(titles, [("Собрано алгоритмами", 1), ("Слушайте друг друга", 1), ("Собрано редакцией", 1)]);
        assert_eq!(home.groups[1].playlists[0].cover.as_deref(), Some("https://x/3.png"));
        assert_eq!(home.groups[1].playlists[0].owner_name.as_deref(), Some("Иван Петров"));
        assert_eq!(home.groups[1].playlists[0].match_pct, Some(98));
        assert_eq!(home.groups[0].playlists[0].owner_name, None);
        assert_eq!(keys, [("pl:1_-21".to_string(), "abc123".to_string())]);

        let a = serde_json::json!({"owner_id": -2001, "id": 55, "artist": " Daft Punk ", "title": "Harder", "duration": 226,
            "url": "https://cs1.vkuseraudio.net/a.mp3?extra=1", "access_key": "k1", "album": {"thumb": {"photo_300": "https://x/c.jpg"}}});
        let (t, url, key) = api_track(&a).unwrap();
        assert_eq!((t.id.as_str(), t.artist.as_str(), t.api), ("-2001_55", "Daft Punk", true));
        assert_eq!(t.cover.as_deref(), Some("https://x/c.jpg"));
        assert!(url.is_some() && key.as_deref() == Some("k1"));
        // без ссылки — слушать нельзя
        let (t, url, _) = api_track(&serde_json::json!({"owner_id": 1, "id": 2, "artist": "a", "title": "b"})).unwrap();
        assert!(!t.api && url.is_none());
    }


    /// Живая проверка API на сессии приложения: VKM_COOKIES=~/.local/share/vkmusic-whisper/cookies
    /// cargo test live_api -- --ignored --nocapture. Печатает только количества и названия, не токен.
    #[test]
    #[ignore]
    fn live_api() {
        let path = std::env::var("VKM_COOKIES").expect("VKM_COOKIES");
        let mut cookies = Vec::new();
        for l in std::fs::read_to_string(path).unwrap().lines() {
            let l = l.strip_prefix("#HttpOnly_").unwrap_or(l);
            if l.starts_with('#') || l.trim().is_empty() {
                continue;
            }
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() >= 7 {
                cookies.push(Cookie { domain: p[0].into(), path: p[2].into(), name: p[5].into(), value: p[6].into() });
            }
        }
        let s = Session { base: "https://vk.ru", cookies: vk_cookies(cookies) };
        let vk = Vk::new();
        let p = tauri::async_runtime::block_on(vk.profile(&s));
        println!("профиль: {:?}", p.as_ref().map(|p| p.id > 0));
        let mt = tauri::async_runtime::block_on(vk.my_tracks(&s, 0));
        println!("мои треки: {:?}", mt.as_ref().map(|p| p.tracks.len()));
        let home = tauri::async_runtime::block_on(vk.home(&s)).unwrap();
        println!("миксы: {:?}", home.mixes.iter().map(|m| &m.title).collect::<Vec<_>>());
        for g in &home.groups {
            println!("{}: {:?}", g.title, g.playlists.iter().map(|p| format!("{} ({})", p.title, p.count)).collect::<Vec<_>>());
        }
        let mine = tauri::async_runtime::block_on(vk.my_playlists(&s)).unwrap();
        println!("мои: {:?}", mine.iter().map(|p| format!("{} ({})", p.title, p.count)).collect::<Vec<_>>());
        let pl = &home.groups[0].playlists[0];
        let (tr, next) = tauri::async_runtime::block_on(vk.playlist(&s, pl.owner_id, pl.id, 0)).unwrap();
        println!("{}: {} треков, со ссылкой {}, дальше {:?}", pl.title, tr.len(), tr.iter().filter(|x| x.1.is_some()).count(), next);
        let m = tauri::async_runtime::block_on(vk.mix(&s, "common")).unwrap();
        println!("микс: {:?}", m.iter().map(|x| format!("{} — {}", x.0.artist, x.0.title)).collect::<Vec<_>>());
        let id = m.iter().find(|x| x.0.api).map(|x| x.0.id.clone()).unwrap();
        let url = tauri::async_runtime::block_on(vk.api_stream(&s, &id, None)).unwrap();
        println!("свежая ссылка getById: {}", url.starts_with("https://"));
    }

    /// Сколько идёт поиск: VKM_COOKIES=… VKM_QUERY=… cargo test live_search -- --ignored --nocapture.
    /// Печатает время профиля и поиска и число треков; набор подряд — средний запрос отменяется (ERR_SUPERSEDED).
    #[test]
    #[ignore]
    fn live_search() {
        let path = std::env::var("VKM_COOKIES").expect("VKM_COOKIES");
        let q = std::env::var("VKM_QUERY").unwrap_or_else(|_| "linkin park numb".into());
        let mut cookies = Vec::new();
        for l in std::fs::read_to_string(path).unwrap().lines() {
            let l = l.strip_prefix("#HttpOnly_").unwrap_or(l);
            if l.starts_with('#') || l.trim().is_empty() {
                continue;
            }
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() >= 7 {
                cookies.push(Cookie { domain: p[0].into(), path: p[2].into(), name: p[5].into(), value: p[6].into() });
            }
        }
        let s = Session { base: "https://vk.ru", cookies: vk_cookies(cookies) };
        let vk = Vk::new();
        tauri::async_runtime::block_on(async {
            let t = Instant::now();
            let p = vk.profile(&s).await;
            println!("профиль: {:?} за {:?}", p.as_ref().map(|p| p.id > 0), t.elapsed());
            for i in 0..2 {
                let t = Instant::now();
                let r = vk.search(&s, &q, None, false).await;
                println!("поиск {i}: {:?} треков за {:?}", r.as_ref().map(|p| p.tracks.len()), t.elapsed());
            }
            // печатают дальше: первый уже ушёл, второй ждёт очереди и отменяется третьим (в ВК уходят два)
            let t = Instant::now();
            let later = |ms: u64, q: String| {
                let vk = &vk;
                let s = &s;
                async move {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    vk.search(s, &q, None, true).await
                }
            };
            let (a, b, c) = tokio::join!(later(0, q.clone()), later(100, format!("{q} a")), later(200, format!("{q} b")));
            println!("набор: {:?} / {:?} / {:?} за {:?}", a.map(|p| p.tracks.len()), b.map(|p| p.tracks.len()), c.map(|p| p.tracks.len()), t.elapsed());
        });
    }

    /// Ретранслятор (relay.rs) на настоящем VK Миксе: ссылка CDN через 127.0.0.1 отдаёт звук, перемотка (Range) работает.
    /// VKM_COOKIES=… cargo test live_relay -- --ignored --nocapture. VKM_RELAY_OUT=файл — записать локальный адрес
    /// (чтобы проиграть его mpv, пока тест ждёт VKM_RELAY_WAIT секунд).
    #[test]
    #[ignore]
    fn live_relay() {
        let path = std::env::var("VKM_COOKIES").expect("VKM_COOKIES");
        let mut cookies = Vec::new();
        for l in std::fs::read_to_string(path).unwrap().lines() {
            let l = l.strip_prefix("#HttpOnly_").unwrap_or(l);
            if l.starts_with('#') || l.trim().is_empty() {
                continue;
            }
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() >= 7 {
                cookies.push(Cookie { domain: p[0].into(), path: p[2].into(), name: p[5].into(), value: p[6].into() });
            }
        }
        let s = Session { base: "https://vk.ru", cookies: vk_cookies(cookies) };
        let vk = Vk::new();
        let m = tauri::async_runtime::block_on(vk.mix(&s, "common")).unwrap();
        let url = m.iter().find_map(|x| x.1.clone()).expect("ссылка");
        let local = crate::relay::url_for(&url, &s.referrer());
        assert!(crate::relay::is_relay(&local));
        let c = reqwest::Client::new();
        let r = tauri::async_runtime::block_on(c.get(&local).header("Range", "bytes=1000-1999").send()).unwrap();
        println!("через ретранслятор: {} {:?}", r.status(), r.headers().get("content-range"));
        assert_eq!(r.status().as_u16(), 206);
        assert_eq!(tauri::async_runtime::block_on(r.bytes()).unwrap().len(), 1000);
        if let Ok(out) = std::env::var("VKM_RELAY_OUT") {
            std::fs::write(out, &local).unwrap();
            let wait = std::env::var("VKM_RELAY_WAIT").ok().and_then(|x| x.parse().ok()).unwrap_or(0);
            std::thread::sleep(std::time::Duration::from_secs(wait));
        }
    }

    /// Методы управления плейлистами — без изменений в аккаунте: всё на чужом плейлисте (owner_id=1),
    /// ВК отказывает. Код 3 («Unknown method») — имя метода неверное.
    /// VKM_COOKIES=… cargo test live_playlist_methods -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_playlist_methods() {
        let path = std::env::var("VKM_COOKIES").expect("VKM_COOKIES");
        let mut cookies = Vec::new();
        for l in std::fs::read_to_string(path).unwrap().lines() {
            let l = l.strip_prefix("#HttpOnly_").unwrap_or(l);
            let p: Vec<&str> = l.split('\t').collect();
            if !l.starts_with('#') && p.len() >= 7 {
                cookies.push(Cookie { domain: p[0].into(), path: p[2].into(), name: p[5].into(), value: p[6].into() });
            }
        }
        let s = Session { base: "https://vk.ru", cookies: vk_cookies(cookies) };
        let vk = Vk::new();
        let o = |k: &'static str, v: &str| (k, v.to_string());
        for (m, params) in [
            ("audio.notAMethod", vec![]),
            ("audio.createPlaylist", vec![o("owner_id", "1"), o("title", "x")]),
            ("audio.editPlaylist", vec![o("owner_id", "1"), o("playlist_id", "1"), o("title", "x")]),
            ("audio.deletePlaylist", vec![o("owner_id", "1"), o("playlist_id", "1")]),
            ("audio.addToPlaylist", vec![o("owner_id", "1"), o("playlist_id", "1"), o("audio_ids", "1_1")]),
            ("audio.removeFromPlaylist", vec![o("owner_id", "1"), o("playlist_id", "1"), o("audio_ids", "1_1")]),
            ("audio.followPlaylist", vec![o("owner_id", "1"), o("playlist_id", "999999999")]),
        ] {
            let r = tauri::async_runtime::block_on(vk.api(&s, m, &params));
            println!("{m}: {:?}", r.map(|v| v.to_string().chars().take(80).collect::<String>()));
        }
    }


    /// Текст ВК для одного трека: VKM_COOKIES=… VKM_VK_ID=owner_id cargo test live_vk_lyrics -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_vk_lyrics() {
        let mut cookies = Vec::new();
        for l in std::fs::read_to_string(std::env::var("VKM_COOKIES").unwrap()).unwrap().lines() {
            let l = l.strip_prefix("#HttpOnly_").unwrap_or(l);
            if l.starts_with('#') || l.trim().is_empty() {
                continue;
            }
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() >= 7 {
                cookies.push(Cookie { domain: p[0].into(), path: p[2].into(), name: p[5].into(), value: p[6].into() });
            }
        }
        let s = Session { base: "https://vk.ru", cookies: vk_cookies(cookies) };
        let vk = Vk::new();
        let id = std::env::var("VKM_VK_ID").unwrap();
        match tauri::async_runtime::block_on(vk.lyrics(&s, &id)).unwrap() {
            Some((text, credits)) => println!("текст ВК: {} строк, credits={credits:?}, первая: {:?}", text.lines().count(), text.lines().next()),
            None => println!("у трека нет текста во ВК"),
        }
        // трек без текста — None, не ошибка
        println!("без текста: {:?}", tauri::async_runtime::block_on(vk.lyrics(&s, "1_1")).map(|x| x.is_some()));
    }

}
