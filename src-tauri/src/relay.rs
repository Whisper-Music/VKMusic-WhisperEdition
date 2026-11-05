//! Поток трека ВК через само приложение: mpv (и ffmpeg кэша) берут его с `http://127.0.0.1:<порт>/r/<токен>`,
//! а Rust скачивает с CDN тем же HTTP-клиентом и той же сетью, что и запросы к API.
//!
//! Зачем: ссылки ВК на официальные треки (VK Микс, плейлисты, треки из API — CDN `*.okcdn.ru`) подписаны под IP
//! того, кто запросил API (`srcIp`). На Windows запросы приложения идут через системный прокси / VPN, а mpv —
//! напрямую, с другого адреса: CDN отказывал, mpv писал «loading failed (Failed to open …)» (2026-11-05). Через
//! ретранслятор сеть, прокси и TLS у потока те же, что у API.
//!
//! Безопасность: слушает только 127.0.0.1; токен — 128 случайных бит; отдаёт только ссылки, которые зарегистрировал
//! сам Rust (`url_for`), последние `KEEP`. Это не открытый прокси: адрес из запроса никуда не уходит.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const KEEP: usize = 32;
const MAX_HEAD: usize = 8 * 1024;

struct Target {
    token: String,
    url: String,
    referrer: String,
}

struct Relay {
    port: u16,
    http: reqwest::Client,
    targets: Mutex<VecDeque<Target>>,
}

static RELAY: OnceLock<Option<Relay>> = OnceLock::new();

fn relay() -> Option<&'static Relay> {
    RELAY
        .get_or_init(|| {
            let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
            let port = listener.local_addr().ok()?.port();
            let http = reqwest::Client::builder()
                .user_agent(crate::vk::USER_AGENT)
                .connect_timeout(Duration::from_secs(20))
                .build()
                .ok()?;
            std::thread::Builder::new()
                .name("relay".into())
                .spawn(move || {
                    for conn in listener.incoming() {
                        let Ok(s) = conn else { continue };
                        let _ = std::thread::Builder::new().name("relay-conn".into()).spawn(move || {
                            let _ = handle(s);
                        });
                    }
                })
                .ok()?;
            Some(Relay { port, http, targets: Mutex::new(VecDeque::new()) })
        })
        .as_ref()
}

/// Локальный адрес для потока `url` (https) с Referer `referrer`. Не запустился — сама ссылка.
pub fn url_for(url: &str, referrer: &str) -> String {
    let Some(r) = relay() else { return url.to_string() };
    if !url.starts_with("https://") {
        return url.to_string();
    }
    let mut t = r.targets.lock().unwrap();
    if let Some(x) = t.iter().find(|x| x.url == url) {
        return format!("http://127.0.0.1:{}/r/{}", r.port, x.token);
    }
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        return url.to_string();
    }
    let token: String = b.iter().map(|x| format!("{x:02x}")).collect();
    t.push_back(Target { token: token.clone(), url: url.into(), referrer: referrer.into() });
    while t.len() > KEEP {
        t.pop_front();
    }
    format!("http://127.0.0.1:{}/r/{token}", r.port)
}

/// Адрес ретранслятора этого запуска (для проверок: такой поток не https, но свой).
pub fn is_relay(url: &str) -> bool {
    relay().is_some_and(|r| url.starts_with(&format!("http://127.0.0.1:{}/r/", r.port)))
}

fn reply(s: &mut TcpStream, status: &str) -> std::io::Result<()> {
    s.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())
}

fn handle(mut s: TcpStream) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = s.read(&mut buf)?;
        if n == 0 || head.len() + n > MAX_HEAD {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    if method != "GET" && method != "HEAD" {
        return reply(&mut s, "405 Method Not Allowed");
    }
    let range = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| v.len() < 100 && v.starts_with("bytes="));
    let Some(r) = relay() else { return reply(&mut s, "503 Service Unavailable") };
    let token = path.strip_prefix("/r/").unwrap_or("");
    let target = r.targets.lock().unwrap().iter().find(|x| x.token == token).map(|x| (x.url.clone(), x.referrer.clone()));
    let Some((url, referrer)) = target else { return reply(&mut s, "404 Not Found") };
    s.set_read_timeout(None)?;

    let head_only = method == "HEAD";
    tauri::async_runtime::block_on(async move {
        let mut req = r.http.get(&url).header("Referer", referrer);
        if let Some(rg) = range {
            req = req.header("Range", rg);
        }
        let mut resp = match req.send().await {
            Ok(x) => x,
            Err(_) => return reply(&mut s, "502 Bad Gateway"),
        };
        let st = resp.status();
        let mut out = format!("HTTP/1.1 {} {}\r\nConnection: close\r\n", st.as_u16(), st.canonical_reason().unwrap_or(""));
        for h in ["content-type", "content-length", "content-range", "accept-ranges"] {
            if let Some(v) = resp.headers().get(h).and_then(|v| v.to_str().ok()) {
                out.push_str(&format!("{h}: {v}\r\n"));
            }
        }
        out.push_str("\r\n");
        s.write_all(out.as_bytes())?;
        if head_only {
            return Ok(());
        }
        // mpv закрыл соединение (перемотка, другой трек) — запись падает, поток заканчивается
        while let Ok(Some(chunk)) = resp.chunk().await {
            s.write_all(&chunk)?;
        }
        Ok(())
    })
}
