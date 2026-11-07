//! Тексты песен с Genius (просьба владельца, 2026-10-30) — для правой панели «Сейчас играет».
//!
//! Без ключа API: поиск — `genius.com/api/search/song` (тот же, что у сайта), текст — со страницы песни
//! (блоки `data-lyrics-container`). Названия треков из ВК грязные (`[vk.com/reuploads]`, `(Slowed & Reverb)`,
//! `feat.`, «Исполнитель - Название» в названии, корейское имя в скобках), поэтому:
//! 1. исполнитель и название чистятся (`clean_title`, `clean_artist`);
//! 2. до трёх поисков: «исполнитель название», «название», вариант из «А - Б» в названии;
//! 3. каждый результат оценивается похожестью названия и исполнителя (`score`): у Genius к русским и корейским
//!    названиям в скобках добавлен перевод — «КИНО (KINO)», «Группа крови (Blood Type)», поэтому сравниваются
//!    и полные строки, и без скобок, и содержимое скобок; страницы «Genius Romanizations/Translations» пропускаются;
//! 4. результат (найден / инструментал / нет) кэшируется в ~/.cache/vkmusic/lyrics, «нет» — на 3 дня.
//!
//! Переводы (2026-10-30): у найденной песни — список `translation_songs` из `/api/songs/<id>` (`meta`: отдельным
//! шагом после текста — текст показывается на запрос раньше); текст перевода грузится по выбору (`translation`).
//! Всё — в кэше на диске: тексты, переводы, пояснения (`notes/<id>.txt`). Пояснения: фрагменты текста, у которых на Genius есть пояснение, в `text`
//! обёрнуты служебными символами `U+E000 id U+E001 фрагмент U+E002`; само пояснение — по нажатию (`annotation`).
//! Запросы — по включению трека (интерфейс ждёт пару секунд, чтобы не искать пролистанные), не чаще раза в
//! секунду (`GAP`), без повторов.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

const GAP: Duration = Duration::from_millis(1000);
const API: &str = "https://genius.com/api/search/song";
const MAX_PAGE: usize = 4 * 1024 * 1024;
/// «не нашли» перепроверяем не раньше чем через столько
const MISS_TTL: u64 = 3 * 86400;
/// версия записи кэша: старее — перезапрашиваем (v2 — переводы и пометки пояснений, v3 — язык оригинала)
const CACHE_V: u32 = 4;
/// записи старше — не годятся; текст ВК до версии 4 сохранялся без времени строк — его спрашиваем заново
const MIN_V: u32 = 3;
const VK_V: u32 = 4;
/// пометки фрагментов с пояснениями в тексте
pub const ANN_START: char = '\u{E000}';
pub const ANN_MID: char = '\u{E001}';
pub const ANN_END: char = '\u{E002}';
const MIN_TITLE: f64 = 0.72;
const MIN_ARTIST: f64 = 0.5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lyrics {
    /// "found" | "instrumental" | "none"
    pub status: String,
    #[serde(default)]
    pub text: String,
    /// страница на Genius
    #[serde(default)]
    pub url: String,
    /// как песня называется на Genius
    #[serde(default)]
    pub song_title: String,
    #[serde(default)]
    pub song_artist: String,
    /// когда получено (unix, с)
    #[serde(default)]
    pub at: u64,
    /// id песни на Genius
    #[serde(default)]
    pub song_id: u64,
    /// переводы песни на Genius (оригинал — сама запись)
    #[serde(default)]
    pub translations: Vec<Translation>,
    /// язык оригинала по Genius: "ru", "ko"…; пусто — неизвестен
    #[serde(default)]
    pub language: String,
    /// переводы и язык уже запрошены (`meta`)
    #[serde(default)]
    pub meta: bool,
    /// откуда текст: "genius" | "vk"
    #[serde(default)]
    pub source: String,
    /// чей текст (ВК пишет правообладателя), если есть
    #[serde(default)]
    pub credits: String,
    /// синхронный текст (только ВК): начало каждой строки `text`, мс
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub times: Vec<u64>,
    #[serde(default)]
    pub v: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Translation {
    /// код языка Genius: "en", "ru", "romanization"…
    pub language: String,
    pub title: String,
    pub url: String,
}

impl Lyrics {
    /// Пустая запись (для текста ВК: заполняется снаружи).
    pub fn empty() -> Lyrics {
        Lyrics::none()
    }

    fn none() -> Lyrics {
        Lyrics {
            status: "none".into(),
            text: String::new(),
            url: String::new(),
            song_title: String::new(),
            song_artist: String::new(),
            at: now(),
            song_id: 0,
            translations: Vec::new(),
            language: String::new(),
            meta: false,
            source: "genius".into(),
            credits: String::new(),
            times: Vec::new(),
            v: CACHE_V,
        }
    }
}

/// Адрес страницы Genius, который можно скачать: https://genius.com/<путь> без запроса и фрагмента.
pub fn genius_page(url: &str) -> bool {
    url.strip_prefix("https://genius.com/").is_some_and(|p| {
        !p.is_empty() && p.len() < 300 && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_/%.".contains(&b)) && !p.contains("..")
    })
}

/// Язык оригинала из ответа `/api/songs/<id>` (код вроде "ru", "ko"; иначе пусто).
pub fn song_language(v: &Value) -> String {
    v.pointer("/response/song/language")
        .and_then(Value::as_str)
        .filter(|l| l.len() <= 20 && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .unwrap_or("")
        .to_string()
}

/// Переводы из ответа `/api/songs/<id>`.
pub fn translations(v: &Value) -> Vec<Translation> {
    v.pointer("/response/song/translation_songs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| t.get("lyrics_state").and_then(Value::as_str).unwrap_or("complete") == "complete")
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?.to_string();
            let language = t.get("language")?.as_str()?.to_string();
            (genius_page(&url) && language.len() <= 20 && language.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
                .then(|| Translation { language, title: t.get("title").and_then(Value::as_str).unwrap_or("").chars().take(200).collect(), url })
        })
        .collect()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("regex"))
}

// ───────────────────────── чистка названий ─────────────────────────

/// Пометки в скобках, которые не часть названия песни.
fn noise_paren(inner: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)\b(feat|ft|prod|remix|mix|slowed|sped|speed|reverb|nightcore|bass|boost|pitched|cover|version|ver|edit|remaster|remastered|live|acoustic|instrumental|extended|radio|official|audio|video|lyrics?|explicit|clean|clip|id|from|ost|8d|solo|tiktok|hardstyle|phonk)\b|\.(com|me|ru|net|org)\b|^\d+$")
        .is_match(inner)
}

/// Название трека → название песни: без пометок в скобках, «feat.», «Lyrics» и т. п.
pub fn clean_title(t: &str) -> String {
    static BR: OnceLock<Regex> = OnceLock::new();
    static PAR: OnceLock<Regex> = OnceLock::new();
    static TAIL: OnceLock<Regex> = OnceLock::new();
    // [vk.com/reuploads], [low pitched] — квадратные скобки целиком
    let s = re(&BR, r"\[[^\]]*\]|【[^】]*】").replace_all(t, " ");
    // (feat. X), (Slowed & Reverb), (ID: 770520) — круглые с пометками
    let s = re(&PAR, r"\(([^()]*)\)").replace_all(&s, |c: &regex::Captures| {
        if noise_paren(&c[1]) { " ".to_string() } else { c[0].to_string() }
    });
    // хвосты без скобок: «feat. X», «prod. by», « Lyrics», «- Slowed»
    let s = re(&TAIL, r"(?i)\s+(feat\.?|ft\.|prod\.?|при уч\.?)\s.*$|\s+(official\s+)?(lyrics?|audio|video)\s*$|\s+-\s+(slowed|sped up|speed up|remix).*$")
        .replace_all(&s, "");
    tidy(&s)
}

/// Исполнитель → основной исполнитель: без «feat.», имени в скобках («IVE (아이브)»), меток сайтов.
pub fn clean_artist(a: &str) -> String {
    static BR: OnceLock<Regex> = OnceLock::new();
    static FEAT: OnceLock<Regex> = OnceLock::new();
    let s = re(&BR, r"\[[^\]]*\]|\([^()]*\)").replace_all(a, " ");
    let s = re(&FEAT, r"(?i)\s*(\bfeat\.?|\bft\.|\bx\b|&|,|;|/|\bпри уч\.?|\bи\b|\bvs\.?|\bwith\b).*$").replace(&s, "");
    tidy(&s)
}

fn tidy(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim_matches(|c: char| c.is_whitespace() || "-–—|·:,.".contains(c)).to_string()
}

// ───────────────────────── похожесть ─────────────────────────

/// Для сравнения: нижний регистр, ё→е, без знаков и пробелов, латиница без диакритики.
pub fn norm(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ё' => 'е',
            'ø' => 'o',
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            _ => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Коэффициент Дайса по биграммам: 0…1.
fn dice(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let bg = |v: &[char]| v.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>();
    let (ba, mut bb) = (bg(&a), bg(&b));
    let total = (ba.len() + bb.len()) as f64;
    let mut hit = 0usize;
    for x in ba {
        if let Some(i) = bb.iter().position(|y| *y == x) {
            bb.swap_remove(i);
            hit += 1;
        }
    }
    2.0 * hit as f64 / total
}

/// Варианты строки с Genius: целиком, без скобок, содержимое скобок («КИНО (KINO)» → кино, kino).
fn variants(s: &str) -> Vec<String> {
    static PAR: OnceLock<Regex> = OnceLock::new();
    let r = re(&PAR, r"\(([^()]*)\)");
    let mut v = vec![norm(s), norm(&r.replace_all(s, " "))];
    for c in r.captures_iter(s) {
        v.push(norm(&c[1]));
    }
    v.retain(|x| !x.is_empty());
    v.dedup();
    v
}

/// Похожесть нашего названия/исполнителя на вариант с Genius.
fn sim(ours: &str, theirs: &str) -> f64 {
    let a = norm(ours);
    if a.is_empty() {
        return 0.0;
    }
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    variants(theirs)
        .iter()
        .map(|b| {
            if *b == a {
                1.0
            } else if digits(b) != digits(&a) {
                // «Psycho Dream» и «Psycho Dreams 2» — разные песни
                dice(&a, b).min(0.6)
            } else if a.chars().count() >= 4 && b.chars().count() >= 4 && (b.contains(&a) || a.contains(b.as_str())) {
                0.88
            } else {
                dice(&a, b)
            }
        })
        .fold(0.0, f64::max)
}

/// Пометки версий в названии на Genius: такой версии нет в нашем названии — ниже оригинала (инструментал — сильно).
const VERSION_WORDS: &[(&str, f64)] = &[
    ("instrumental", 0.3), ("инструментал", 0.3), ("karaoke", 0.3),
    ("translation", 0.2), ("romanized", 0.2), ("перевод", 0.2),
    ("slowed", 0.06), ("sped", 0.06), ("speed", 0.06), ("reverb", 0.06), ("nightcore", 0.06),
    ("remix", 0.06), ("mix", 0.04), ("edit", 0.06), ("live", 0.06), ("session", 0.06), ("acoustic", 0.06),
    ("version", 0.06), ("ver.", 0.06), ("demo", 0.06), ("snippet", 0.08), ("cover", 0.06), ("house", 0.06),
    ("theme", 0.04), ("remaster", 0.04), ("interlude", 0.04), ("intro", 0.04),
];

#[derive(Debug, Clone)]
pub struct Hit {
    pub title: String,
    pub artist: String,
    /// основные исполнители песни
    pub artists: Vec<String>,
    /// приглашённые (feat.)
    pub featured: Vec<String>,
    pub url: String,
    pub instrumental: bool,
    pub id: u64,
}

/// Оценка результата поиска: (похожесть названия, похожесть исполнителя). None — не подходит.
pub fn score(title: &str, artist: &str, raw_title: &str, h: &Hit) -> Option<(f64, f64)> {
    // переводы и романизации Genius — не те страницы
    if h.artist.starts_with("Genius ") || h.artist.eq_ignore_ascii_case("Genius") {
        return None;
    }
    let ts = sim(title, &h.title);
    // совпадение только с приглашённым исполнителем — это чужая песня «при участии» (NEOGEN SENSEI feat. Miki
    // Matsubara вместо самой Miki Matsubara): такое не засчитываем
    let as_ = if artist.is_empty() {
        0.6
    } else {
        let main = h.artists.iter().map(|a| sim(artist, a)).fold(0.0, f64::max);
        let feat = h.featured.iter().map(|a| sim(artist, a)).fold(0.0, f64::max) * 0.45;
        main.max(feat)
    };
    // версии (ремикс, инструментал, sped up…), которых нет в нашем названии, — ниже оригинала
    let low = raw_title.to_lowercase();
    let theirs = h.title.to_lowercase();
    let mut t = ts;
    for (w, pen) in VERSION_WORDS {
        if theirs.contains(w) && !low.contains(w) {
            t -= pen;
        }
    }
    // прочие лишние скобки (но не перевод названия: он есть у всех версий русских и корейских песен)
    t -= 0.01 * theirs.matches('(').count() as f64;
    (ts >= MIN_TITLE && as_ >= MIN_ARTIST).then_some((t, as_))
}

fn hits(v: &Value) -> Vec<Hit> {
    let s = |x: &Value, p: &str| x.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
    v.pointer("/response/sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|sec| sec.get("hits").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter(|h| h.get("type").and_then(Value::as_str) == Some("song"))
        .filter_map(|h| {
            let r = h.get("result")?;
            let url = s(r, "/url");
            if !url.starts_with("https://genius.com/") {
                return None;
            }
            let names = |key: &str| -> Vec<String> {
                r.get(key).and_then(Value::as_array).into_iter().flatten().map(|a| s(a, "/name")).filter(|a| !a.is_empty()).collect()
            };
            let mut artists = vec![s(r, "/primary_artist/name")];
            artists.extend(names("primary_artists"));
            artists.retain(|a| !a.is_empty());
            Some(Hit {
                title: s(r, "/title"),
                artist: s(r, "/primary_artist/name"),
                artists,
                featured: names("featured_artists"),
                url,
                instrumental: r.get("instrumental").and_then(Value::as_bool).unwrap_or(false),
                id: r.get("id").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect()
}

// ───────────────────────── текст со страницы ─────────────────────────

/// Конец элемента `<div …>`, который начинается в `start` (с учётом вложенных div): индекс после `</div>`.
fn div_end(h: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    loop {
        let open = h[i..].find("<div").map(|x| x + i);
        let close = h[i..].find("</div>").map(|x| x + i);
        match (open, close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                i = o + 4;
            }
            (_, Some(c)) => {
                depth = depth.checked_sub(1)?;
                i = c + 6;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => return None,
        }
    }
}

/// Убрать вложенные элементы с атрибутом `attr` (шапка «N Contributors … Lyrics»).
fn drop_divs(mut h: String, attr: &str) -> String {
    while let Some(a) = h.find(attr) {
        let Some(start) = h[..a].rfind("<div") else { break };
        let Some(end) = div_end(&h, start) else { break };
        h.replace_range(start..end, "");
    }
    h
}

fn decode_entities(s: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);")
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let ch = if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
            } else if let Some(d) = e.strip_prefix('#') {
                d.parse().ok().and_then(char::from_u32)
            } else {
                match e {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "hellip" => Some('…'),
                    _ => None,
                }
            };
            ch.map(String::from).unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

/// Текст песни из HTML страницы Genius. None — блоков с текстом нет.
pub fn extract(html: &str) -> Option<String> {
    static BR: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static REF: OnceLock<Regex> = OnceLock::new();
    const MARK: &str = "data-lyrics-container=\"true\"";
    let mut parts = Vec::new();
    let mut from = 0;
    while let Some(a) = html[from..].find(MARK).map(|x| x + from) {
        let start = html[..a].rfind("<div")?;
        let end = div_end(html, start)?;
        let inner = drop_divs(html[start..end].to_string(), "data-exclude-from-selection=\"true\"");
        // фрагмент с пояснением: <a href="/<id>/…">…</a> → U+E000 id U+E001 фрагмент U+E002
        let inner = re(&REF, r#"(?s)<a\b[^>]*\bhref="/(\d{1,12})/[^"]*"[^>]*>(.*?)</a>"#)
            .replace_all(&inner, |c: &regex::Captures| format!("{ANN_START}{}{ANN_MID}{}{ANN_END}", &c[1], &c[2]));
        let t = re(&BR, r"(?i)<br\s*/?>").replace_all(&inner, "\n");
        let t = re(&TAG, r"<[^>]*>").replace_all(&t, "");
        parts.push(decode_entities(&t));
        from = end;
    }
    let text = parts.join("\n");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = String::new();
    let mut blank = 0;
    for l in lines {
        if l.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 || l.starts_with('[') { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(l);
    }
    (!out.trim().is_empty()).then_some(out)
}

// ───────────────────────── клиент ─────────────────────────

/// Текст пояснения из ответа `/api/referents/<id>`: все пояснения фрагмента, через пустую строку.
pub fn annotation_text(v: &Value) -> Option<String> {
    let parts: Vec<String> = v
        .pointer("/response/referent/annotations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| a.pointer("/body/plain").and_then(Value::as_str))
        .map(|t| t.trim().chars().take(5000).collect::<String>())
        .filter(|t| !t.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

pub struct Genius {
    /// пояснения, уже показанные в этом запуске
    notes: std::sync::Mutex<std::collections::HashMap<u64, String>>,
    http: reqwest::Client,
    last: tokio::sync::Mutex<Option<Instant>>,
    dir: PathBuf,
}

impl Genius {
    pub fn new() -> Genius {
        let base = dirs::cache_dir().unwrap_or_else(|| crate::config::home().join(".cache"));
        Genius::at(base.join("vkmusic").join("lyrics"))
    }

    fn at(dir: PathBuf) -> Genius {
        Genius {
            notes: std::sync::Mutex::new(std::collections::HashMap::new()),
            http: reqwest::Client::builder()
                .user_agent(crate::vk::USER_AGENT)
                .timeout(Duration::from_secs(20))
                .build()
                .expect("http client"),
            last: tokio::sync::Mutex::new(None),
            dir,
        }
    }

    fn cache_path(&self, artist: &str, title: &str) -> PathBuf {
        let key = format!("{}\n{}", norm(artist), norm(title));
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in key.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        self.dir.join(format!("{h:016x}.json"))
    }

    async fn get(&self, url: &str, query: Option<&str>) -> Res<String> {
        let mut req = self.http.get(url).header("Referer", "https://genius.com/");
        if let Some(q) = query {
            // 20 — больше Genius не отдаёт; у K-pop первые 10 часто заняты страницами переводов
            req = req.query(&[("q", q), ("per_page", "20"), ("page", "1")]);
        }
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < GAP {
                tokio::time::sleep(GAP - t.elapsed()).await;
            }
        }
        let res = req.send().await;
        *last = Some(Instant::now());
        drop(last);
        let res = res.map_err(|e| format!("Нет связи с Genius: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("Genius ответил HTTP {}", res.status().as_u16()));
        }
        if res.content_length().is_some_and(|n| n as usize > MAX_PAGE) {
            return Err("Слишком большой ответ Genius".into());
        }
        let body = res.text().await.map_err(|e| format!("Genius: {e}"))?;
        if body.len() > MAX_PAGE {
            return Err("Слишком большой ответ Genius".into());
        }
        Ok(body)
    }

    /// Найти песню: лучший результат по похожести из нескольких поисков.
    pub async fn search(&self, artist: &str, title: &str) -> Res<Option<Hit>> {
        let (t, a) = (clean_title(title), clean_artist(artist));
        let mut tries: Vec<(String, String, String)> = Vec::new();
        // «Hikaru Station - Bishounen (… Remix)» в названии: исполнитель — слева
        if let Some((l, r)) = t.split_once(" - ") {
            tries.push((tidy(r), clean_artist(l), format!("{} {}", clean_artist(l), tidy(r))));
        }
        if !a.is_empty() {
            tries.push((t.clone(), a.clone(), format!("{a} {t}")));
        }
        tries.push((t.clone(), a.clone(), t.clone()));
        let mut seen = std::collections::HashSet::new();
        for (qt, qa, q) in tries {
            if q.trim().is_empty() || !seen.insert(q.clone()) {
                continue;
            }
            let body = self.get(API, Some(&q)).await?;
            let v: Value = serde_json::from_str(&body).map_err(|e| format!("Genius: {e}"))?;
            // при равной оценке — тот, что Genius поставил выше
            let mut best: Option<(f64, Hit)> = None;
            for h in hits(&v) {
                if let Some((ts, as_)) = score(&qt, &qa, title, &h) {
                    let sc = ts * 0.6 + as_ * 0.4;
                    if best.as_ref().is_none_or(|b| sc > b.0 + 1e-9) {
                        best = Some((sc, h));
                    }
                }
            }
            if let Some((_, h)) = best {
                return Ok(Some(h));
            }
        }
        Ok(None)
    }

    /// Положить текст в кэш (текст ВК приходит не отсюда, а кэшируется здесь же).
    pub fn store_for(&self, artist: &str, title: &str, l: &Lyrics) {
        self.store(&self.cache_path(artist, title), l);
    }

    /// Текст только из кэша на диске — без запросов (интерфейс показывает его сразу при включении трека).
    pub fn lyrics_cached(&self, artist: &str, title: &str) -> Option<Lyrics> {
        let mut l = self.cached(&self.cache_path(artist, title))?;
        if l.v < MIN_V || (l.source == "vk" && l.v < VK_V) || (l.status == "none" && now().saturating_sub(l.at) >= MISS_TTL) {
            return None;
        }
        // записи до появления `meta` уже содержат переводы и язык — не перезапрашиваем
        if !l.meta && (!l.translations.is_empty() || !l.language.is_empty()) {
            l.meta = true;
        }
        Some(l)
    }

    /// Прежний текст ВК (сохранён без времени строк): пока новый не получен — лучше он, чем поиск на Genius.
    pub fn old_vk(&self, artist: &str, title: &str) -> Option<Lyrics> {
        self.cached(&self.cache_path(artist, title)).filter(|l| l.source == "vk" && l.status == "found")
    }

    /// Текст песни (из кэша или с Genius).
    pub async fn lyrics(&self, artist: &str, title: &str) -> Res<Lyrics> {
        let path = self.cache_path(artist, title);
        if let Some(l) = self.lyrics_cached(artist, title) {
            return Ok(l);
        }
        let out = match self.search(artist, title).await? {
            None => Lyrics::none(),
            Some(h) => {
                let page = self.get(&h.url, None).await?;
                let mut l = Lyrics { url: h.url.clone(), song_title: h.title.clone(), song_artist: h.artist.clone(), song_id: h.id, ..Lyrics::none() };
                match extract(&page) {
                    Some(text) => {
                        l.status = "found".into();
                        l.text = text;
                        // переводы и язык — следующим шагом (`meta`): текст показывается сразу
                    }
                    None if h.instrumental => l.status = "instrumental".into(),
                    None => {}
                }
                l
            }
        };
        self.store(&path, &out);
        Ok(out)
    }

    fn cached(&self, path: &std::path::Path) -> Option<Lyrics> {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Lyrics>(&b).ok())
    }

    fn store(&self, path: &std::path::Path, l: &Lyrics) {
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(b) = serde_json::to_vec(l) {
            let _ = std::fs::write(path, b);
        }
    }

    /// Переводы и язык оригинала найденной песни (`/api/songs/<id>`), дописываются в кэш текста. Не получилось —
    /// без них (при следующем включении трека — ещё раз).
    pub async fn meta(&self, artist: &str, title: &str) -> Res<Lyrics> {
        let path = self.cache_path(artist, title);
        let mut l = self.cached(&path).ok_or("Текст ещё не найден")?;
        if l.meta || l.status != "found" || l.song_id == 0 {
            return Ok(l);
        }
        let body = self.get(&format!("https://genius.com/api/songs/{}", l.song_id), None).await?;
        let v: Value = serde_json::from_str(&body).map_err(|e| format!("Genius: {e}"))?;
        l.translations = translations(&v);
        l.language = song_language(&v);
        l.meta = true;
        self.store(&path, &l);
        Ok(l)
    }

    /// Текст перевода (страница Genius из `translations`), с кэшем на диске.
    /// Перевод только из кэша на диске — без запросов.
    pub fn translation_cached(&self, url: &str) -> Option<String> {
        if !genius_page(url) {
            return None;
        }
        std::fs::read_to_string(self.cache_path("translation", url)).ok().filter(|t| !t.is_empty())
    }

    pub async fn translation(&self, url: &str) -> Res<String> {
        if !genius_page(url) {
            return Err("Недопустимая ссылка".into());
        }
        let path = self.cache_path("translation", url);
        if let Some(t) = self.translation_cached(url) {
            return Ok(t);
        }
        let text = extract(&self.get(url, None).await?).ok_or("На странице перевода нет текста")?;
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(&path, &text);
        Ok(text)
    }

    /// Пояснение к фрагменту текста (id из пометки `U+E000`): обычный текст.
    pub async fn annotation(&self, id: u64) -> Res<String> {
        if let Some(t) = self.notes.lock().unwrap().get(&id) {
            return Ok(t.clone());
        }
        // кэш на диске: пояснения Genius меняются редко
        let file = self.dir.join("notes").join(format!("{id}.txt"));
        if let Some(t) = std::fs::read_to_string(&file).ok().filter(|t| !t.is_empty()) {
            self.notes.lock().unwrap().insert(id, t.clone());
            return Ok(t);
        }
        let body = self.get(&format!("https://genius.com/api/referents/{id}?text_format=plain"), None).await?;
        let v: Value = serde_json::from_str(&body).map_err(|e| format!("Genius: {e}"))?;
        let text = annotation_text(&v).ok_or("У этого фрагмента нет пояснения")?;
        if std::fs::create_dir_all(file.parent().unwrap_or(&self.dir)).is_ok() {
            let _ = std::fs::write(&file, &text);
        }
        let mut notes = self.notes.lock().unwrap();
        if notes.len() > 2000 {
            notes.clear();
        }
        notes.insert(id, text.clone());
        Ok(text)
    }

    /// Очистить кэш текстов (вместе с кэшем треков).
    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_cleaned() {
        assert_eq!(clean_title("In Halo [vk.com/reuploads]"), "In Halo");
        assert_eq!(clean_title("World Hold On (Slowed & Reverb)"), "World Hold On");
        assert_eq!(clean_title("Gangsta's Paradise (feat. L.V.)"), "Gangsta's Paradise");
        assert_eq!(clean_title("Thermodynamix (ID: 770520)"), "Thermodynamix");
        assert_eq!(clean_title("Hold On Tight Lyrics"), "Hold On Tight");
        assert_eq!(clean_title("Saddle Up (WINTER Solo)"), "Saddle Up");
        assert_eq!(clean_title("Severed Arm [low pitched]"), "Severed Arm");
        assert_eq!(clean_title("WDA (Whole Different Animal)"), "WDA (Whole Different Animal)");
        assert_eq!(clean_title("Перегарик (feat. Voskresenskii)"), "Перегарик");
        assert_eq!(clean_artist("IVE (아이브)"), "IVE");
        assert_eq!(clean_artist("aespa (feat.Ty Dolla)"), "aespa");
        assert_eq!(clean_artist("Mark Ronson feat. Bruno Mars"), "Mark Ronson");
        assert_eq!(clean_artist("LXST CXNTURY, Kingpin Skinny Pimp"), "LXST CXNTURY");
        assert_eq!(clean_artist("Жанна Фриске [mp3crazy.me]"), "Жанна Фриске");
        assert_eq!(clean_artist("Bob Sinclar (Roma)"), "Bob Sinclar");
    }

    fn hit(artist: &str, title: &str) -> Hit {
        Hit { title: title.into(), artist: artist.into(), artists: vec![artist.into()], featured: vec![], url: "https://genius.com/x".into(), instrumental: false, id: 1 }
    }

    #[test]
    fn scoring() {
        // перевод в скобках у Genius
        assert!(score("Группа крови", "Кино", "Группа крови", &hit("КИНО (KINO)", "Группа крови (Blood Type)")).is_some());
        // романизация — мимо
        assert!(score("Группа крови", "Кино", "Группа крови", &hit("Genius Romanizations", "Кино (Kino) – Группа крови (Blood type) (Romanized)")).is_none());
        // чужая песня с похожим названием
        assert!(score("Табуретка", "Кино", "Табуретка", &hit("killamyself", "Табуретка (Stool)")).is_none());
        assert!(score("Spotlight", "Marshmello", "Spotlight", &hit("Marshmello", "Spotlight")).is_some());
        // оригинал выше ремикса
        let o = score("Spotlight", "Marshmello", "Spotlight", &hit("Marshmello", "Spotlight")).unwrap();
        let r = score("Spotlight", "Marshmello", "Spotlight", &hit("Marshmello", "Spotlight (Remix)")).unwrap();
        assert!(o.0 > r.0);
        assert_eq!(norm("VØJ"), "voj");
        // наш исполнитель — только приглашённый: чужая песня
        let mut f = hit("NEOGEN SENSEI", "Stay With Me (Feat. Miki Matsubara)");
        f.featured = vec!["Miki Matsubara".into()];
        assert!(score("Stay With Me", "Miki Matsubara", "Stay With Me", &f).is_none());
    }

    /// Проверка на настоящем Genius по библиотеке: VKM_LYRICS_SAMPLE=40 cargo test live_library -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_library() {
        let n: usize = std::env::var("VKM_LYRICS_SAMPLE").ok().and_then(|x| x.parse().ok()).unwrap_or(30);
        let lib = crate::config::config_dir().join("library.json");
        let items: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(lib).unwrap()).unwrap();
        let step = (items.len() / n).max(1);
        let g = Genius::at(std::env::temp_dir().join(format!("vkmusic-lyrics-live-{}", std::process::id())));
        let (mut found, mut total) = (0, 0);
        let skip: usize = std::env::var("VKM_LYRICS_OFFSET").ok().and_then(|x| x.parse().ok()).unwrap_or(0);
        for it in items.iter().skip(skip).step_by(step).take(n) {
            let (a, t) = (it["artist"].as_str().unwrap_or(""), it["title"].as_str().unwrap_or(""));
            let r = tauri::async_runtime::block_on(g.lyrics(a, t));
            total += 1;
            match r {
                Ok(l) if l.status == "found" => {
                    found += 1;
                    println!("✓ {a} — {t}  →  {} — {}  ({} строк)", l.song_artist, l.song_title, l.text.lines().count());
                }
                Ok(l) => println!("✗ {a} — {t}  [{}] {}", l.status, l.url),
                Err(e) => println!("! {a} — {t}  {e}"),
            }
        }
        println!("найдено {found} из {total}");
        g.clear();
    }

    /// Одна песня целиком: VKM_ARTIST=aespa VKM_TITLE=Whiplash cargo test live_one -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_one() {
        let (a, t) = (std::env::var("VKM_ARTIST").unwrap_or_default(), std::env::var("VKM_TITLE").unwrap_or_default());
        let g = Genius::at(std::env::temp_dir().join(format!("vkmusic-lyrics-one-{}", std::process::id())));
        let _ = tauri::async_runtime::block_on(g.lyrics(&a, &t)).unwrap();
        let l = tauri::async_runtime::block_on(g.meta(&a, &t)).unwrap();
        let shown: String = l.text.chars().take(400).collect::<String>().replace(ANN_START, "⟦").replace(ANN_MID, "|").replace(ANN_END, "⟧");
        println!("{} {} — {} id={}\n{shown}", l.status, l.song_artist, l.song_title, l.song_id);
        println!("переводов: {} {:?}", l.translations.len(), l.translations.iter().map(|x| x.language.as_str()).collect::<Vec<_>>());
        let ids: Vec<u64> = l.text.split(ANN_START).skip(1).filter_map(|x| x.split(ANN_MID).next()?.parse().ok()).collect();
        println!("пояснений в тексте: {}", ids.len());
        if let Some(id) = ids.first() {
            println!("пояснение {id}: {:?}", tauri::async_runtime::block_on(g.annotation(*id)).map(|x| x.chars().take(200).collect::<String>()));
        }
        if let Some(tr) = l.translations.iter().find(|x| x.language == "ru" || x.language == "en") {
            let txt = tauri::async_runtime::block_on(g.translation(&tr.url)).unwrap();
            println!("перевод {}: {}", tr.language, txt.chars().take(150).collect::<String>().replace('\n', " / "));
        }
        g.clear();
    }

    #[test]
    fn extracts_lyrics() {
        let html = r#"<html><div data-lyrics-container="true" class="L"><div data-exclude-from-selection="true" class="H"><span>42 Contributors</span><div>Blood Type Lyrics</div></div>[Куплет 1]<br/>Тёплое место, но <a href="/30410576/Kino-blood-type" class="ReferentFragment"><span>улицы ждут</span></a><br>Отпечатков наших ног &amp; &#x27;пыль&#39;</div><div class="ad">реклама</div><div data-lyrics-container="true">[Припев]<br/>Группа крови — на рукаве</div></html>"#;
        assert_eq!(
            extract(html).unwrap(),
            "[Куплет 1]\nТёплое место, но \u{E000}30410576\u{E001}улицы ждут\u{E002}\nОтпечатков наших ног & 'пыль'\n\n[Припев]\nГруппа крови — на рукаве"
        );
        assert!(extract("<div>Instrumental</div>").is_none());
    }

    #[test]
    fn translations_and_notes() {
        let v = serde_json::json!({"response": {"song": {"translation_songs": [
            {"language": "en", "title": "aespa - Whiplash (English Translation)", "url": "https://genius.com/Genius-english-translations-aespa-whiplash-english-translation-lyrics", "lyrics_state": "complete"},
            {"language": "ru", "title": "x", "url": "https://evil.example/x", "lyrics_state": "complete"},
            {"language": "fr", "title": "x", "url": "https://genius.com/a-b-lyrics", "lyrics_state": "unreleased"}
        ]}}});
        let t = translations(&v);
        assert_eq!(song_language(&serde_json::json!({"response": {"song": {"language": "ko"}}})), "ko");
        assert_eq!(song_language(&serde_json::json!({"response": {"song": {"language": "<b>"}}})), "");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].language, "en");
        assert!(genius_page("https://genius.com/Kino-blood-type-lyrics"));
        assert!(!genius_page("https://genius.com/../x"));
        assert!(!genius_page("https://genius.com/a?b=1"));
        assert!(!genius_page("http://genius.com/a"));
        let n = serde_json::json!({"response": {"referent": {"annotations": [{"body": {"plain": " Про войну. "}}]}}});
        assert_eq!(annotation_text(&n).as_deref(), Some("Про войну."));
        assert!(annotation_text(&serde_json::json!({})).is_none());
    }
}
