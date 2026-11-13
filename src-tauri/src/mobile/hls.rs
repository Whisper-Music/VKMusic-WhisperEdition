//! Кэш треков сервисов на Android — без ffmpeg (на компьютере поток выкачивает `ffmpeg -c copy`, cache.rs).
//!
//! * Обычный файл (mp3/m4a/ogg по https) пишется как есть.
//! * HLS (ВКонтакте): плейлист → сегменты по порядку; зашифрованные (`#EXT-X-KEY:METHOD=AES-128`) —
//!   расшифровываются (AES-128-CBC, IV — из плейлиста или номер сегмента). Сегменты MPEG-TS разбираются здесь же:
//!   звук из PES-пакетов дорожки — в `.mp3` (MPEG audio) или `.aac` (ADTS), без перекодирования, как `-c copy`.
//!   Сегменты fMP4 (`#EXT-X-MAP`) — склеиваются в `.m4a`.
//!
//! Запросы — с тем же User-Agent и Referer, что у плеера; по одному, как на компьютере (очередь `TrackCache`).

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use std::io::Write;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Больше трек не бывает; защита от бесконечного потока.
const MAX_BYTES: u64 = 400 * 1024 * 1024;
const MAX_SEGMENTS: usize = 5000;

fn client() -> Res<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(crate::vk::USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(c: &reqwest::Client, url: &str, referrer: &str) -> Res<reqwest::Response> {
    if !url.starts_with("https://") {
        return Err("Неверная ссылка на поток".into());
    }
    let r = c.get(url).header("Referer", referrer).send().await.map_err(|e| format!("Поток не открылся: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Поток не открылся: HTTP {}", r.status().as_u16()));
    }
    Ok(r)
}

async fn bytes(c: &reqwest::Client, url: &str, referrer: &str) -> Res<Vec<u8>> {
    let r = get(c, url, referrer).await?;
    if r.content_length().is_some_and(|n| n > MAX_BYTES) {
        return Err("Слишком большой файл".into());
    }
    r.bytes().await.map(|b| b.to_vec()).map_err(|e| format!("Поток оборвался: {e}"))
}

/// Выкачать поток в `dir/<key>.<расширение>`; возвращает путь готового файла. Блокирующий вызов.
pub fn fetch(dir: &Path, key: &str, url: &str, referrer: &str) -> Res<PathBuf> {
    tauri::async_runtime::block_on(fetch_async(dir, key, url, referrer))
}

async fn fetch_async(dir: &Path, key: &str, url: &str, referrer: &str) -> Res<PathBuf> {
    let c = client()?;
    let mut r = get(&c, url, referrer).await?;
    let ctype = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let first = r.chunk().await.map_err(|e| format!("Поток оборвался: {e}"))?.unwrap_or_default();
    let tmp = dir.join(format!("{key}.part"));
    if first.starts_with(b"#EXTM3U") || ctype.contains("mpegurl") {
        let mut text = first.to_vec();
        while let Some(ch) = r.chunk().await.map_err(|e| format!("Поток оборвался: {e}"))? {
            text.extend_from_slice(&ch);
            if text.len() > 4 * 1024 * 1024 {
                return Err("Слишком большой плейлист".into());
            }
        }
        let ext = hls(&c, url, &String::from_utf8_lossy(&text), referrer, &tmp).await;
        let ext = match ext {
            Ok(x) => x,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        };
        let dst = dir.join(format!("{key}.{ext}"));
        std::fs::rename(&tmp, &dst).map_err(|e| e.to_string())?;
        return Ok(dst);
    }
    // обычный файл — как есть
    let ext = ext_for(&ctype, url, &first);
    let mut f = std::fs::File::create(&tmp).map_err(|e| format!("Не удалось записать в кэш: {e}"))?;
    let mut total = first.len() as u64;
    f.write_all(&first).map_err(|e| e.to_string())?;
    loop {
        match r.chunk().await {
            Ok(Some(ch)) => {
                total += ch.len() as u64;
                if total > MAX_BYTES {
                    drop(f);
                    let _ = std::fs::remove_file(&tmp);
                    return Err("Слишком большой файл".into());
                }
                f.write_all(&ch).map_err(|e| e.to_string())?;
            }
            Ok(None) => break,
            Err(e) => {
                drop(f);
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("Поток оборвался: {e}"));
            }
        }
    }
    drop(f);
    let dst = dir.join(format!("{key}.{ext}"));
    std::fs::rename(&tmp, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

/// Расширение обычного файла: по Content-Type, по адресу, по первым байтам.
fn ext_for(ctype: &str, url: &str, head: &[u8]) -> &'static str {
    if head.starts_with(b"OggS") {
        return if head.windows(8).any(|w| w == b"OpusHead") { "opus" } else { "ogg" };
    }
    if head.starts_with(b"fLaC") {
        return "flac";
    }
    if head.len() > 8 && &head[4..8] == b"ftyp" {
        return "m4a";
    }
    if head.starts_with(b"ID3") || (head.len() > 1 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        // ADTS (AAC) тоже начинается с 0xFFF…: у него слой (биты 1–2 второго байта) — 00
        if head.len() > 1 && head[0] == 0xFF && head[1] & 0x06 == 0 {
            return "aac";
        }
        return "mp3";
    }
    if ctype.contains("mp4") || ctype.contains("m4a") {
        return "m4a";
    }
    if ctype.contains("ogg") {
        return "ogg";
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    for x in crate::library::AUDIO_EXT {
        if path.to_ascii_lowercase().ends_with(&format!(".{x}")) {
            return x;
        }
    }
    "mp3"
}

/// Строки плейлиста: атрибут `NAME=значение` (в кавычках или без).
fn attr(line: &str, name: &str) -> Option<String> {
    let i = line.find(&format!("{name}="))?;
    let rest = &line[i + name.len() + 1..];
    if let Some(r) = rest.strip_prefix('"') {
        return r.split('"').next().map(str::to_string);
    }
    rest.split(',').next().map(str::to_string)
}

fn join(base: &str, rel: &str) -> Res<String> {
    let b = reqwest::Url::parse(base).map_err(|e| e.to_string())?;
    let u = b.join(rel.trim()).map_err(|e| e.to_string())?;
    if u.scheme() != "https" {
        return Err("Неверная ссылка в плейлисте".into());
    }
    Ok(u.to_string())
}

struct Seg {
    url: String,
    /// (адрес ключа, IV) — если сегмент зашифрован
    key: Option<(String, Option<[u8; 16]>)>,
    seq: u64,
}

fn parse_iv(s: &str) -> Option<[u8; 16]> {
    let h = s.trim().strip_prefix("0x").or_else(|| s.trim().strip_prefix("0X"))?;
    let h = format!("{h:0>32}");
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Скачать HLS в файл `out`; возвращает расширение.
async fn hls(c: &reqwest::Client, url: &str, text: &str, referrer: &str, out: &Path) -> Res<&'static str> {
    let mut base = url.to_string();
    let mut text = text.to_string();
    // мастер-плейлист — берём вариант с самым высоким битрейтом
    if text.contains("#EXT-X-STREAM-INF") {
        let mut best: Option<(u64, String)> = None;
        let mut lines = text.lines();
        while let Some(l) = lines.next() {
            if l.starts_with("#EXT-X-STREAM-INF") {
                let bw = attr(l, "BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
                if let Some(u) = lines.by_ref().find(|x| !x.trim().is_empty() && !x.starts_with('#')) {
                    if best.as_ref().is_none_or(|b| bw > b.0) {
                        best = Some((bw, u.to_string()));
                    }
                }
            }
        }
        let (_, rel) = best.ok_or("Пустой плейлист")?;
        base = join(url, &rel)?;
        text = String::from_utf8_lossy(&bytes(c, &base, referrer).await?).into_owned();
    }
    let mut segs = Vec::new();
    let mut key: Option<(String, Option<[u8; 16]>)> = None;
    let mut seq: u64 = 0;
    let mut init: Option<String> = None;
    for l in text.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if l.starts_with("#EXT-X-KEY:") {
            key = match attr(l, "METHOD").as_deref() {
                Some("NONE") | None => None,
                Some("AES-128") => {
                    let uri = attr(l, "URI").ok_or("Нет ключа в плейлисте")?;
                    Some((join(&base, &uri)?, attr(l, "IV").and_then(|v| parse_iv(&v))))
                }
                Some(m) => return Err(format!("Поток зашифрован ({m}) — кэшировать нельзя")),
            };
        } else if l.starts_with("#EXT-X-MAP:") {
            init = attr(l, "URI").map(|u| join(&base, &u)).transpose()?;
        } else if !l.is_empty() && !l.starts_with('#') {
            segs.push(Seg { url: join(&base, l)?, key: key.clone(), seq });
            seq += 1;
            if segs.len() > MAX_SEGMENTS {
                return Err("Слишком длинный плейлист".into());
            }
        }
    }
    if segs.is_empty() {
        return Err("Пустой плейлист".into());
    }
    let mut keys: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    let mut f = std::fs::File::create(out).map_err(|e| format!("Не удалось записать в кэш: {e}"))?;
    let mut ts = Ts::default();
    let mut total: u64 = 0;
    let mut kind: Option<&'static str> = None;
    if let Some(u) = &init {
        let b = bytes(c, u, referrer).await?;
        f.write_all(&b).map_err(|e| e.to_string())?;
        kind = Some("m4a");
    }
    for s in &segs {
        let mut data = bytes(c, &s.url, referrer).await?;
        if let Some((kurl, iv)) = &s.key {
            if !keys.contains_key(kurl) {
                let k = bytes(c, kurl, referrer).await?;
                if k.len() != 16 {
                    return Err("Неверный ключ потока".into());
                }
                keys.insert(kurl.clone(), k);
            }
            let k = &keys[kurl];
            let iv = iv.unwrap_or_else(|| {
                let mut v = [0u8; 16];
                v[8..].copy_from_slice(&s.seq.to_be_bytes());
                v
            });
            let dec = Aes128CbcDec::new_from_slices(k, &iv).map_err(|e| e.to_string())?;
            data = dec.decrypt_padded_vec_mut::<Pkcs7>(&data).map_err(|_| "Не удалось расшифровать поток".to_string())?;
        }
        total += data.len() as u64;
        if total > MAX_BYTES {
            return Err("Слишком большой файл".into());
        }
        if kind.is_none() {
            kind = Some(if data.first() == Some(&0x47) { "ts" } else { "raw" });
        }
        let audio = if kind == Some("ts") { ts.push(&data) } else { data };
        f.write_all(&audio).map_err(|e| e.to_string())?;
    }
    f.flush().map_err(|e| e.to_string())?;
    Ok(match kind {
        Some("m4a") => "m4a",
        Some("ts") => {
            if ts.stream_type == Some(0x0F) {
                "aac"
            } else {
                "mp3"
            }
        }
        _ => {
            drop(f);
            let head = std::fs::read(out).map(|b| b.into_iter().take(64).collect::<Vec<_>>()).unwrap_or_default();
            ext_for("", "", &head)
        }
    })
}

/// Разбор MPEG-TS: из пакетов дорожки звука — содержимое PES (кадры MP3 / ADTS AAC) подряд.
#[derive(Default)]
struct Ts {
    pmt_pid: Option<u16>,
    audio_pid: Option<u16>,
    /// stream_type из PMT: 0x03/0x04 — MPEG audio, 0x0F — AAC (ADTS)
    stream_type: Option<u8>,
    /// хвост неполного пакета с прошлого сегмента
    rest: Vec<u8>,
}

impl Ts {
    fn push(&mut self, data: &[u8]) -> Vec<u8> {
        let mut buf = std::mem::take(&mut self.rest);
        buf.extend_from_slice(data);
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        while i + 188 <= buf.len() {
            if buf[i] != 0x47 {
                i += 1; // потеряли синхронизацию — ищем следующий пакет
                continue;
            }
            self.packet(&buf[i..i + 188], &mut out);
            i += 188;
        }
        self.rest = buf[i..].to_vec();
        out
    }

    fn packet(&mut self, p: &[u8], out: &mut Vec<u8>) {
        let pid = (((p[1] & 0x1F) as u16) << 8) | p[2] as u16;
        let start = p[1] & 0x40 != 0;
        let afc = (p[3] >> 4) & 3;
        if afc & 1 == 0 {
            return; // без полезной нагрузки
        }
        let mut off = 4;
        if afc & 2 != 0 {
            off += 1 + p[4] as usize;
        }
        if off >= 188 {
            return;
        }
        let pay = &p[off..];
        if pid == 0 && start {
            // PAT: первая программа → PID таблицы PMT
            let t = pay.get(1 + pay[0] as usize..).unwrap_or(&[]);
            if t.len() >= 12 {
                let len = ((((t[1] & 0x0F) as usize) << 8) | t[2] as usize).min(t.len().saturating_sub(3));
                let mut j = 8;
                while j + 4 <= 3 + len.saturating_sub(4) {
                    let prog = ((t[j] as u16) << 8) | t[j + 1] as u16;
                    if prog != 0 {
                        self.pmt_pid = Some((((t[j + 2] & 0x1F) as u16) << 8) | t[j + 3] as u16);
                        break;
                    }
                    j += 4;
                }
            }
        } else if Some(pid) == self.pmt_pid && start && self.audio_pid.is_none() {
            let t = pay.get(1 + pay[0] as usize..).unwrap_or(&[]);
            if t.len() >= 12 {
                let len = ((((t[1] & 0x0F) as usize) << 8) | t[2] as usize).min(t.len().saturating_sub(3));
                let info = (((t[10] & 0x0F) as usize) << 8) | t[11] as usize;
                let mut j = 12 + info;
                while j + 5 <= 3 + len.saturating_sub(4) {
                    let st = t[j];
                    let epid = (((t[j + 1] & 0x1F) as u16) << 8) | t[j + 2] as u16;
                    let es = (((t[j + 3] & 0x0F) as usize) << 8) | t[j + 4] as usize;
                    if matches!(st, 0x03 | 0x04 | 0x0F) {
                        self.audio_pid = Some(epid);
                        self.stream_type = Some(st);
                        break;
                    }
                    j += 5 + es;
                }
            }
        } else if pid != 0 && Some(pid) != self.pmt_pid {
            // без PMT — первая дорожка со звуком (stream_id 0xC0–0xDF)
            if self.audio_pid.is_none() && start && pay.len() > 3 && pay[..3] == [0, 0, 1] && (0xC0..=0xDF).contains(&pay[3]) {
                self.audio_pid = Some(pid);
            }
            if Some(pid) != self.audio_pid {
                return;
            }
            if start {
                if pay.len() < 9 || pay[..3] != [0, 0, 1] {
                    return;
                }
                let skip = 9 + pay[8] as usize;
                if skip < pay.len() {
                    out.extend_from_slice(&pay[skip..]);
                }
            } else {
                out.extend_from_slice(pay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iv_and_attrs() {
        assert_eq!(parse_iv("0x00000000000000000000000000000001").unwrap()[15], 1);
        assert_eq!(attr(r#"#EXT-X-KEY:METHOD=AES-128,URI="key.pub",IV=0x1"#, "URI").as_deref(), Some("key.pub"));
        assert_eq!(attr("#EXT-X-KEY:METHOD=NONE", "METHOD").as_deref(), Some("NONE"));
    }
}
