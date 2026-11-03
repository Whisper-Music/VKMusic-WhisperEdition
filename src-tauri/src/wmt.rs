//! Whisper Music Theme (`.wmt`), версия 1 — чтение тем.
//!
//! Формат: https://github.com/Whisper-Music/YandexMusic-WhisperEdition/blob/main/docs/WMT-FORMAT.md
//! Реализация своя (без зависимостей, кроме flate2 для deflate):
//! * текстовая тема — `#WMT 1` + секции `@@ <тип> [<имя>] [<кодировка>]`;
//! * тема с фоном — zip (stored/deflate, без zip64) с `theme.wmt`, `background.*` и любыми файлами.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::io::Read;

pub const SUPPORTED_MAJOR: u32 = 1;
pub const MAX_FILE: usize = 64 << 20;
pub const MAX_UNPACKED: usize = 80 << 20;
pub const MAX_VIDEO: usize = 48 << 20;

pub const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "avif", "bmp"];
pub const VIDEO_EXT: &[&str] = &["mp4", "webm", "m4v", "mov"];

#[derive(Debug, Clone)]
pub struct WmtError {
    pub line: Option<usize>,
    pub msg: String,
}

impl fmt::Display for WmtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "строка {l}: {}", self.msg),
            None => write!(f, "{}", self.msg),
        }
    }
}

impl std::error::Error for WmtError {}

fn err<T>(line: Option<usize>, msg: impl Into<String>) -> Result<T, WmtError> {
    Err(WmtError { line, msg: msg.into() })
}

#[derive(Debug, Clone)]
pub struct Section {
    pub kind: String,
    pub name: Option<String>,
    pub enc: Option<String>,
    pub body: String,
    /// номер строки заголовка секции (с 1)
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Theme {
    /// тема пришла zip-архивом
    pub archive: bool,
    pub manifest: Value,
    pub tokens: Value,
    pub sections: Vec<Section>,
    settings: HashMap<String, Value>,
    /// файлы архива и секции `@@ asset` (файл архива главнее)
    pub files: HashMap<String, Vec<u8>>,
}

impl Theme {
    pub fn id(&self) -> &str {
        self.manifest.get("id").and_then(Value::as_str).unwrap_or("")
    }
    pub fn name(&self) -> &str {
        self.manifest.get("name").and_then(Value::as_str).unwrap_or("")
    }
    pub fn manifest_str(&self, key: &str) -> Option<&str> {
        self.manifest.get(key).and_then(Value::as_str)
    }

    /// CSS для приложения `app` (все секции `@@ css <app>` подряд) или None.
    pub fn css(&self, app: &str) -> Option<String> {
        let parts: Vec<&str> = self
            .sections
            .iter()
            .filter(|s| s.kind == "css" && s.name.as_deref() == Some(app))
            .map(|s| s.body.as_str())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    /// JSON-настройки приложения `app` или пустой объект.
    pub fn settings(&self, app: &str) -> Value {
        self.settings
            .get(app)
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()))
    }

    /// Файл по имени; принимает и `asset:имя`.
    pub fn asset(&self, name: &str) -> Option<&[u8]> {
        let name = name.strip_prefix("asset:").unwrap_or(name);
        self.files.get(name).map(|v| v.as_slice())
    }

    /// Фон темы: (имя файла, это видео). Файл `background.*` главнее старого `tokens.background.image`.
    pub fn background(&self) -> Option<(String, bool)> {
        for ext in IMAGE_EXT.iter().chain(VIDEO_EXT.iter()) {
            let name = format!("background.{ext}");
            if let Some(data) = self.files.get(&name) {
                let video = VIDEO_EXT.contains(ext);
                if video && data.len() > MAX_VIDEO {
                    continue;
                }
                return Some((name, video));
            }
        }
        let legacy = self
            .tokens
            .pointer("/background/image")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix("asset:"))?;
        if self.files.contains_key(legacy) {
            let video = VIDEO_EXT.contains(&ext_of(legacy).as_str());
            return Some((legacy.to_string(), video));
        }
        None
    }
}

pub fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn mime_of(name: &str) -> &'static str {
    match ext_of(name).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "css" => "text/css",
        "txt" | "wmt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn valid_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

pub fn valid_theme_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 48
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Открыть тему из байтов файла: текст или архив определяется по первым байтам.
pub fn open(bytes: &[u8]) -> Result<Theme, WmtError> {
    if bytes.len() > MAX_FILE {
        return err(None, "файл темы больше 64 МБ");
    }
    if bytes.starts_with(b"PK\x03\x04") {
        open_archive(bytes)
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| WmtError { line: None, msg: "тема не в кодировке UTF-8".into() })?;
        parse_text(text)
    }
}

fn open_archive(bytes: &[u8]) -> Result<Theme, WmtError> {
    let entries = zip_read(bytes)?;
    // theme.wmt может лежать в папке — берём самый неглубокий
    let theme_path = entries
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| n == &"theme.wmt" || n.ends_with("/theme.wmt"))
        .min_by_key(|n| n.matches('/').count())
        .map(str::to_string)
        .ok_or(WmtError { line: None, msg: "в архиве нет theme.wmt".into() })?;
    let prefix = theme_path[..theme_path.len() - "theme.wmt".len()].to_string();

    let mut text: Option<Vec<u8>> = None;
    let mut files = HashMap::new();
    for (name, data) in entries {
        let Some(rel) = name.strip_prefix(prefix.as_str()) else { continue };
        if rel == "theme.wmt" {
            text = Some(data);
        } else if !rel.contains('/') && valid_asset_name(rel) {
            files.insert(rel.to_string(), data);
        }
    }
    let text = text.unwrap_or_default();
    let text = std::str::from_utf8(&text)
        .map_err(|_| WmtError { line: None, msg: "theme.wmt не в кодировке UTF-8".into() })?;
    let mut theme = parse_text(text)?;
    theme.archive = true;
    theme.files.extend(files);
    Ok(theme)
}

/// Разбор текста темы.
pub fn parse_text(text: &str) -> Result<Theme, WmtError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();

    let first = lines.iter().position(|l| !l.trim().is_empty());
    let Some(first) = first else {
        return err(None, "пустой файл");
    };
    let header = lines[first].trim();
    let ver = header
        .strip_prefix("#WMT")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or(WmtError { line: Some(first + 1), msg: "первая строка должна быть «#WMT 1»".into() })?;
    let major: u32 = ver
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .ok_or(WmtError { line: Some(first + 1), msg: format!("непонятная версия формата «{ver}»") })?;
    if major > SUPPORTED_MAJOR {
        return err(
            Some(first + 1),
            format!("тема в формате #WMT {major}, а приложение знает только #WMT {SUPPORTED_MAJOR} — обновите приложение"),
        );
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut cur: Option<(Section, Vec<&str>)> = None;
    for (idx, line) in lines.iter().enumerate().skip(first + 1) {
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some((mut s, body)) = cur.take() {
                s.body = join_body(&body);
                sections.push(s);
            }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() || parts.len() > 3 {
                return err(Some(idx + 1), "заголовок секции: «@@ <тип> [<имя>] [<кодировка>]»");
            }
            let kind = parts[0];
            let ok_kind = kind.as_bytes()[0].is_ascii_lowercase()
                && kind
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
            if !ok_kind {
                return err(Some(idx + 1), format!("неверный тип секции «{kind}»"));
            }
            cur = Some((
                Section {
                    kind: kind.to_string(),
                    name: parts.get(1).map(|s| s.to_string()),
                    enc: parts.get(2).map(|s| s.to_string()),
                    body: String::new(),
                    line: idx + 1,
                },
                Vec::new(),
            ));
        } else if let Some((_, body)) = cur.as_mut() {
            body.push(line);
        }
        // строки до первой секции — комментарии
    }
    if let Some((mut s, body)) = cur.take() {
        s.body = join_body(&body);
        sections.push(s);
    }

    // manifest — ровно один
    let manifests: Vec<&Section> = sections.iter().filter(|s| s.kind == "manifest").collect();
    if manifests.len() != 1 {
        return err(None, if manifests.is_empty() { "нет секции @@ manifest" } else { "секция @@ manifest повторяется" });
    }
    let manifest = parse_json_object(manifests[0])?;
    let id = manifest.get("id").and_then(Value::as_str).unwrap_or("");
    if !valid_theme_id(id) {
        return err(
            Some(manifests[0].line),
            "manifest.id: латиница a–z, цифры и «-», до 48 символов",
        );
    }
    let name_ok = manifest
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|n| !n.trim().is_empty());
    if !name_ok {
        return err(Some(manifests[0].line), "manifest.name обязателен");
    }

    let token_secs: Vec<&Section> = sections.iter().filter(|s| s.kind == "tokens").collect();
    if token_secs.len() > 1 {
        return err(Some(token_secs[1].line), "секция @@ tokens повторяется");
    }
    let tokens = match token_secs.first() {
        Some(s) => parse_json_object(s)?,
        None => Value::Object(Map::new()),
    };

    let mut settings = HashMap::new();
    let mut files = HashMap::new();
    for s in &sections {
        match s.kind.as_str() {
            "settings" => {
                let Some(app) = &s.name else {
                    return err(Some(s.line), "у @@ settings нужно имя приложения");
                };
                settings.insert(app.clone(), parse_json_object(s)?);
            }
            "asset" => {
                let Some(name) = s.name.as_deref().filter(|n| valid_asset_name(n)) else {
                    return err(Some(s.line), "у @@ asset нужно имя файла [A-Za-z0-9._-]");
                };
                let data = match s.enc.as_deref().unwrap_or("base64") {
                    "base64" => {
                        let clean: String = s.body.chars().filter(|c| !c.is_whitespace()).collect();
                        B64.decode(clean.as_bytes()).map_err(|_| WmtError {
                            line: Some(s.line),
                            msg: format!("ресурс {name}: неверный base64"),
                        })?
                    }
                    "text" => s.body.as_bytes().to_vec(),
                    other => return err(Some(s.line), format!("неизвестная кодировка ресурса «{other}»")),
                };
                files.insert(name.to_string(), data);
            }
            // manifest/tokens уже разобраны, css читается по запросу, неизвестное пропускаем
            _ => {}
        }
    }

    Ok(Theme { archive: false, manifest, tokens, sections, settings, files })
}

fn join_body(lines: &[&str]) -> String {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    lines[..end].join("\n")
}

fn parse_json_object(s: &Section) -> Result<Value, WmtError> {
    let clean = strip_jsonc(&s.body);
    let v: Value = serde_json::from_str(&clean).map_err(|e| WmtError {
        line: Some(s.line + e.line()),
        msg: format!("@@ {}: {}", s.kind, e),
    })?;
    if !v.is_object() {
        return err(Some(s.line), format!("@@ {}: ожидается объект {{ … }}", s.kind));
    }
    Ok(v)
}

/// JSON с комментариями `//`, `/* */` и лишними запятыми → обычный JSON.
/// Переводы строк сохраняются, чтобы номера строк в ошибках совпадали.
pub fn strip_jsonc(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                // лишняя запятая перед } или ]
                let mut j = i + 1;
                loop {
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    // комментарии между запятой и скобкой
                    if chars.get(j) == Some(&'/') && chars.get(j + 1) == Some(&'/') {
                        while j < chars.len() && chars[j] != '\n' {
                            j += 1;
                        }
                        continue;
                    }
                    if chars.get(j) == Some(&'/') && chars.get(j + 1) == Some(&'*') {
                        j += 2;
                        while j < chars.len() && !(chars[j] == '*' && chars.get(j + 1) == Some(&'/')) {
                            j += 1;
                        }
                        j += 2;
                        continue;
                    }
                    break;
                }
                if !matches!(chars.get(j), Some('}') | Some(']')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

// ───────────────────────── zip (только чтение) ─────────────────────────

fn u16le(b: &[u8], at: usize) -> Option<usize> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)
}
fn u32le(b: &[u8], at: usize) -> Option<usize> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

fn bad_zip() -> WmtError {
    WmtError { line: None, msg: "повреждённый архив темы".into() }
}

/// Все файлы архива (имя → данные). Каталоги, `__MACOSX/` и `.DS_Store` пропускаются.
pub fn zip_read(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>, WmtError> {
    // конец центрального каталога: сигнатура PK\x05\x06, ищем с конца (комментарий до 64 КБ)
    let min = b.len().saturating_sub(22 + 0xFFFF);
    let mut eocd = None;
    let mut p = b.len().saturating_sub(22);
    loop {
        if b.get(p..p + 4) == Some(b"PK\x05\x06") {
            eocd = Some(p);
            break;
        }
        if p == min || p == 0 {
            break;
        }
        p -= 1;
    }
    let eocd = eocd.ok_or_else(bad_zip)?;
    let count = u16le(b, eocd + 10).ok_or_else(bad_zip)?;
    let cd_off = u32le(b, eocd + 16).ok_or_else(bad_zip)?;
    if count == 0xFFFF || cd_off == 0xFFFF_FFFF {
        return err(None, "архивы zip64 не поддерживаются");
    }

    let mut out = Vec::new();
    let mut total = 0usize;
    let mut at = cd_off;
    for _ in 0..count {
        if b.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err(bad_zip());
        }
        let flags = u16le(b, at + 8).ok_or_else(bad_zip)?;
        let method = u16le(b, at + 10).ok_or_else(bad_zip)?;
        let csize = u32le(b, at + 20).ok_or_else(bad_zip)?;
        let usize_ = u32le(b, at + 24).ok_or_else(bad_zip)?;
        let nlen = u16le(b, at + 28).ok_or_else(bad_zip)?;
        let xlen = u16le(b, at + 30).ok_or_else(bad_zip)?;
        let clen = u16le(b, at + 32).ok_or_else(bad_zip)?;
        let local = u32le(b, at + 42).ok_or_else(bad_zip)?;
        let raw_name = b.get(at + 46..at + 46 + nlen).ok_or_else(bad_zip)?;
        let name = String::from_utf8_lossy(raw_name).replace('\\', "/");
        at += 46 + nlen + xlen + clen;

        if name.ends_with('/')
            || name.starts_with("__MACOSX/")
            || name.contains("/__MACOSX/")
            || name.rsplit('/').next() == Some(".DS_Store")
        {
            continue;
        }
        if csize == 0xFFFF_FFFF || usize_ == 0xFFFF_FFFF || local == 0xFFFF_FFFF {
            return err(None, "архивы zip64 не поддерживаются");
        }
        if flags & 1 != 0 {
            return err(None, format!("{name}: архив с паролем не поддерживается"));
        }
        total += usize_;
        if total > MAX_UNPACKED {
            return err(None, "распакованные файлы темы больше 80 МБ");
        }
        if b.get(local..local + 4) != Some(b"PK\x03\x04") {
            return Err(bad_zip());
        }
        let lnlen = u16le(b, local + 26).ok_or_else(bad_zip)?;
        let lxlen = u16le(b, local + 28).ok_or_else(bad_zip)?;
        let start = local + 30 + lnlen + lxlen;
        let data = b.get(start..start + csize).ok_or_else(bad_zip)?;
        let bytes = match method {
            0 => data.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(data)
                    .take(usize_ as u64 + 1)
                    .read_to_end(&mut v)
                    .map_err(|_| bad_zip())?;
                v
            }
            m => return err(None, format!("{name}: способ сжатия {m} не поддерживается (нужен stored или deflate)")),
        };
        if bytes.len() != usize_ {
            return Err(bad_zip());
        }
        out.push((name, bytes));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal() {
        let t = parse_text(
            "#WMT 1\n@@ manifest\n{ \"id\": \"tiny\", \"name\": \"Маленькая\", \"base\": \"neon\" }\n@@ tokens\n{ \"color\": { \"accent\": \"#3fb8af\", // c\n \"accent2\": \"#ff6f59\", } }\n",
        )
        .unwrap();
        assert_eq!(t.id(), "tiny");
        assert_eq!(t.tokens.pointer("/color/accent2").unwrap(), "#ff6f59");
    }

    #[test]
    fn newer_version() {
        assert!(parse_text("#WMT 2\n@@ manifest\n{\"id\":\"a\",\"name\":\"a\"}").is_err());
        assert!(parse_text("#WMT 1.3\n@@ manifest\n{\"id\":\"a\",\"name\":\"a\"}\n@@ future x\nwhatever").is_ok());
    }

    #[test]
    fn css_section() {
        let t = parse_text("#WMT 1\n@@ manifest\n{\"id\":\"a\",\"name\":\"a\"}\n@@ css vkmusic\n.panel{color:red}\n\n\n@@ css other\nx{}").unwrap();
        assert_eq!(t.css("vkmusic").unwrap(), ".panel{color:red}");
    }
}
