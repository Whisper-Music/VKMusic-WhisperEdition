//! Режим `--host [ПОРТ]`: интерфейс открывается в браузере на http://127.0.0.1:ПОРТ вместо окна приложения.
//!
//! Приложение то же самое: Tauri, mpv, все команды и события. Главное окно создаётся скрытым и пустым
//! (about:blank) — оно держит хранилище кук WebKit (вход во ВКонтакте и SoundCloud) и через него идут команды.
//! Браузер получает те же файлы интерфейса (`asset_resolver`), а `window.__TAURI__` ему подменяет `src/host.js`:
//! * invoke — `POST /invoke/<команда>` → обработчик команд Tauri (`Webview::on_message`, как из окна);
//! * события — поток SSE `/events` (список — `EVENTS`; новое событие для интерфейса — добавить туда);
//! * протоколы `vkm://` и `wa://` — `/p/vkm/…` и `/p/wa/…`.
//!
//! Страница входа в сервис открывается отдельным окном (`main.rs`, `svc_login`). Работает одна вкладка:
//! новая забирает поток событий у старой (`host://taken`).
//!
//! Защита (сервер умеет удалять файлы музыки и играть что угодно из библиотеки, поэтому так строго):
//! * слушаем только 127.0.0.1; заголовок `Host` — только `127.0.0.1:ПОРТ` или `localhost:ПОРТ` (DNS rebinding);
//! * каждый запрос — с кукой-ключом (HttpOnly, SameSite=Strict); ставит её адрес `/?key=…`, который приложение
//!   открывает в браузере и печатает в консоль. Ключ — `~/.config/vkmusic/host-key` (0600), постоянный: открытая
//!   вкладка переживает перезапуск приложения;
//! * POST — только с `Origin` этого же адреса (или без него), страницу нельзя встроить во фрейм.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
use tauri::{AppHandle, Listener, Manager};

/// Порт по умолчанию. Постоянный: от адреса зависит localStorage браузера (история очереди, настройки модулей).
pub const DEFAULT_PORT: u16 = 7380;

/// События Rust, которые уходят в браузер.
const EVENTS: [&str; 6] = ["player://event", "dl://event", "svc://event", "cache://event", "discord-status", "room-link"];

/// Заголовки запроса — не больше; тело — тема до 64 МБ сырым куском (`theme_install`) с запасом.
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = crate::wmt::MAX_FILE + 1024 * 1024;

const KEY_FILE: &str = "host-key";

struct Host {
    port: u16,
    key: String,
}

static HOST: OnceLock<Host> = OnceLock::new();
/// Открытые потоки событий (вкладки). Работает одна — последняя.
static SUBS: Mutex<Vec<mpsc::Sender<String>>> = Mutex::new(Vec::new());

/// Приложение запущено с `--host`.
pub fn active() -> bool {
    HOST.get().is_some()
}

/// Начало адресов протокола vkm:// для интерфейса: в браузере такого протокола нет — тот же путь через сервер.
pub fn vkm_base() -> &'static str {
    if active() {
        "/p/vkm/"
    } else if cfg!(windows) {
        // WebView2 открывает свои схемы как http://<схема>.localhost
        "http://vkm.localhost/"
    } else {
        "vkm://localhost/"
    }
}

/// `--host`, `--host 8080`, `--host=8080` → порт; без `--host` — None (обычное окно).
pub fn port_from_args(args: &[String]) -> Result<Option<u16>, String> {
    let bad = |v: &str| format!("Неверный порт «{v}»: vkmusic --host [ПОРТ], ПОРТ — от 1 до 65535");
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--host=") {
            return v.parse::<u16>().ok().filter(|p| *p > 0).map(Some).ok_or_else(|| bad(v));
        }
        if a == "--host" {
            return match it.peek() {
                Some(v) if !v.starts_with('-') && !v.contains(':') => {
                    v.parse::<u16>().ok().filter(|p| *p > 0).map(Some).ok_or_else(|| bad(v))
                }
                _ => Ok(Some(DEFAULT_PORT)),
            };
        }
    }
    Ok(None)
}

/// Поднять сервер (в setup, до запуска mpv). Возвращает адрес с ключом — его открыть в браузере.
pub fn start(app: &AppHandle, port: u16) -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("Не удалось занять порт {port}: {e}. Укажите другой: vkmusic --host {}", port.wrapping_add(1).max(1)))?;
    let key = load_key()?;
    let url = format!("http://127.0.0.1:{port}/?key={key}");
    let _ = HOST.set(Host { port, key });
    for name in EVENTS {
        app.listen_any(name, move |e| broadcast(name, e.payload()));
    }
    let app = app.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(s) = conn else { continue };
            let app = app.clone();
            std::thread::spawn(move || {
                let _ = handle(&app, s);
            });
        }
    });
    Ok(url)
}

/// Открыть интерфейс в браузере по умолчанию.
pub fn open_browser() {
    let Some(h) = HOST.get() else { return };
    let url = format!("http://127.0.0.1:{}/?key={}", h.port, h.key);
    if let Err(e) = crate::os::open(&url) {
        eprintln!("vkmusic: не удалось открыть браузер ({e}); откройте адрес сами");
    }
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Окна нет — выход по Ctrl+C / SIGTERM, штатно (RunEvent::Exit: сохранить настройки, закрыть mpv).
pub fn exit_on_signal(app: &AppHandle) {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        if STOP.load(Ordering::SeqCst) {
            app.exit(0);
            break;
        }
    });
}

// ───────────────────────── ключ ─────────────────────────

/// Ключ доступа: из файла или новый (32 шестнадцатеричных знака).
fn load_key() -> Result<String, String> {
    use crate::os::Private;
    let dir = crate::config::config_dir();
    let path = dir.join(KEY_FILE);
    if let Ok(k) = std::fs::read_to_string(&path) {
        let k = k.trim();
        if valid_key(k) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            return Ok(k.to_string());
        }
    }
    let mut r = [0u8; 16];
    getrandom::fill(&mut r).map_err(|e| e.to_string())?;
    let key: String = r.iter().map(|b| format!("{b:02x}")).collect();
    let mut d = std::fs::DirBuilder::new();
    d.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut d, 0o700);
    d.create(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true).private();
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW);
    let mut f = o
        .open(&path)
        .map_err(|e| format!("Не удалось сохранить ключ доступа: {e}"))?;
    f.write_all(key.as_bytes()).map_err(|e| format!("Не удалось сохранить ключ доступа: {e}"))?;
    Ok(key)
}

fn valid_key(k: &str) -> bool {
    k.len() == 32 && k.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Сравнение без утечки по времени.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ───────────────────────── события ─────────────────────────

fn broadcast(name: &str, payload: &str) {
    // JSON без переводов строк: строка SSE — одна строка data
    let payload: String = payload.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    let msg = format!("{{\"event\":{},\"payload\":{}}}", serde_json::Value::from(name), payload);
    SUBS.lock().unwrap().retain(|s| s.send(msg.clone()).is_ok());
}

// ───────────────────────── HTTP ─────────────────────────

struct Req {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("cookie"))
            .flat_map(|(_, v)| v.split(';'))
            .filter_map(|c| c.trim().split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v)
    }
}

fn read_req(r: &mut BufReader<TcpStream>) -> Option<Req> {
    let mut head = Vec::new();
    loop {
        let n = r.by_ref().take((MAX_HEAD - head.len()) as u64 + 1).read_until(b'\n', &mut head).ok()?;
        if n == 0 || head.len() > MAX_HEAD {
            return None;
        }
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            break;
        }
    }
    let head = String::from_utf8(head).ok()?;
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let mut req = Req { method, path: path.to_string(), query: query.to_string(), headers, body: Vec::new() };
    let len: usize = req.header("content-length").map(|v| v.parse().ok()).unwrap_or(Some(0))?;
    if len > MAX_BODY || req.header("transfer-encoding").is_some() {
        return None;
    }
    req.body = vec![0; len];
    r.read_exact(&mut req.body).ok()?;
    Some(req)
}

fn reason(code: u16) -> &'static str {
    tauri::http::StatusCode::from_u16(code).ok().and_then(|s| s.canonical_reason()).unwrap_or("")
}

/// Ответ целиком; соединение закрывается.
fn send(s: &mut TcpStream, code: u16, headers: &[(&str, &str)], body: &[u8]) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {code} {}\r\n", reason(code));
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: frame-ancestors 'none'\r\nReferrer-Policy: no-referrer\r\n\r\n",
        body.len()
    ));
    s.write_all(out.as_bytes())?;
    s.write_all(body)?;
    s.flush()
}

fn text(s: &mut TcpStream, code: u16, msg: &str) -> std::io::Result<()> {
    send(s, code, &[("Content-Type", "text/plain; charset=utf-8"), ("Cache-Control", "no-store")], msg.as_bytes())
}

/// Ответ обработчика протокола (vkm://, wa://) как есть.
fn send_response(s: &mut TcpStream, resp: tauri::http::Response<Vec<u8>>) -> std::io::Result<()> {
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter(|(k, _)| !["content-length", "connection"].contains(&k.as_str()))
        .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
        .collect();
    let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    send(s, resp.status().as_u16(), &refs, resp.body())
}

fn handle(app: &AppHandle, s: TcpStream) -> std::io::Result<()> {
    let h = HOST.get().expect("сервер без ключа");
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    s.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut s = s;
    let Some(req) = read_req(&mut r) else { return text(&mut s, 400, "bad request") };

    // только наш адрес: против DNS rebinding
    let origin_ok = |o: &str| o == format!("http://127.0.0.1:{}", h.port) || o == format!("http://localhost:{}", h.port);
    if !req.header("host").is_some_and(|v| origin_ok(&format!("http://{v}"))) {
        return text(&mut s, 403, "Неверный адрес");
    }
    let cookie = format!("vkm_key_{}", h.port);
    // вход по ключу: кука и сразу на «/», чтобы ключ не оставался в адресной строке
    if req.path == "/" {
        if let Some(k) = req.query.split('&').find_map(|p| p.strip_prefix("key=")) {
            if !same(k, &h.key) {
                return text(&mut s, 403, "Неверный ключ. Откройте адрес, который VK Music напечатал при запуске.");
            }
            let set = format!("{cookie}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000", h.key);
            return send(&mut s, 303, &[("Location", "/"), ("Set-Cookie", &set), ("Cache-Control", "no-store")], b"");
        }
    }
    if !req.cookie(&cookie).is_some_and(|k| same(k, &h.key)) {
        return text(&mut s, 403, "Нет доступа. Откройте адрес, который VK Music напечатал при запуске (vkmusic --host).");
    }

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/" | "/index.html") => index(app, &mut s),
        ("GET", "/events") => events(s),
        ("POST", p) if p.starts_with("/invoke/") => {
            if req.header("origin").is_some_and(|o| !origin_ok(o)) {
                return text(&mut s, 403, "Чужой Origin");
            }
            invoke(app, &mut s, &req)
        }
        ("GET", p) if p.starts_with("/p/vkm/") => {
            let uri = format!("vkm://localhost/{}{}{}", &p[7..], if req.query.is_empty() { "" } else { "?" }, req.query);
            let resp = crate::serve(app, &uri);
            send_response(&mut s, resp)
        }
        ("GET", p) if p.starts_with("/p/wa/") => {
            let mut b = tauri::http::Request::builder().uri(format!("wa://localhost/{}", &p[6..]));
            for name in ["range", "origin"] {
                if let Some(v) = req.header(name) {
                    b = b.header(name, v);
                }
            }
            let Ok(r) = b.body(Vec::new()) else { return text(&mut s, 400, "bad request") };
            let resp = tauri::async_runtime::block_on(crate::whisper::stream::serve_stream(r));
            send_response(&mut s, resp)
        }
        ("GET", p) => match app.asset_resolver().get(p.to_string()) {
            Some(a) => send(&mut s, 200, &[("Content-Type", &a.mime_type), ("Cache-Control", "no-cache")], &a.bytes),
            None => text(&mut s, 404, "not found"),
        },
        _ => text(&mut s, 405, "method not allowed"),
    }
}

/// index.html с заглушкой Tauri (host.js) первым скриптом — до ядра Whisper.
fn index(app: &AppHandle, s: &mut TcpStream) -> std::io::Result<()> {
    let Some(a) = app.asset_resolver().get("index.html".into()) else { return text(s, 404, "not found") };
    let html = String::from_utf8_lossy(&a.bytes);
    let html = html.replacen("<head>", "<head>\n  <script src=\"host.js\"></script>", 1);
    send(s, 200, &[("Content-Type", "text/html; charset=utf-8"), ("Cache-Control", "no-store")], html.as_bytes())
}

/// Поток событий (SSE). Новая вкладка забирает его у прежних.
fn events(mut s: TcpStream) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    {
        let mut subs = SUBS.lock().unwrap();
        for old in subs.drain(..) {
            let _ = old.send("{\"event\":\"host://taken\",\"payload\":null}".into());
        }
        subs.push(tx);
    }
    s.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\
          X-Content-Type-Options: nosniff\r\n\r\nretry: 2000\n\n",
    )?;
    s.flush()?;
    loop {
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(msg) => {
                let last = msg.contains("\"host://taken\"");
                s.write_all(format!("data: {msg}\n\n").as_bytes())?;
                s.flush()?;
                if last {
                    return Ok(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                s.write_all(b": ping\n\n")?;
                s.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

/// Команда — в обработчик Tauri от имени главного окна (как invoke из него).
fn invoke(app: &AppHandle, s: &mut TcpStream, req: &Req) -> std::io::Result<()> {
    let cmd = &req.path["/invoke/".len()..];
    if cmd.is_empty() || cmd.len() > 100 || !cmd.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return text(s, 400, "bad command");
    }
    let ok = [("Content-Type", "application/json"), ("Cache-Control", "no-store")];
    // окна нет: свернуть, развернуть, закрыть, перетащить — ничего не делают
    if cmd.starts_with("win_") {
        return send(s, 200, &ok, b"null");
    }
    let raw = req.header("content-type").is_some_and(|c| c.starts_with("application/octet-stream"));
    let body = if raw {
        InvokeBody::Raw(req.body.clone())
    } else if req.body.is_empty() {
        InvokeBody::Json(serde_json::json!({}))
    } else {
        match serde_json::from_slice(&req.body) {
            Ok(v) => InvokeBody::Json(v),
            Err(_) => return text(s, 400, "bad json"),
        }
    };
    let Some(wv) = app.get_webview("main") else { return text(s, 503, "Нет главного окна") };
    let (tx, rx) = mpsc::channel();
    let request = tauri::webview::InvokeRequest {
        cmd: cmd.to_string(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost/".parse().expect("адрес"),
        body,
        headers: Default::default(),
        invoke_key: app.invoke_key().to_string(),
    };
    wv.on_message(
        request,
        Box::new(move |_, _, resp, _, _| {
            let _ = tx.send(resp);
        }),
    );
    match rx.recv() {
        Ok(InvokeResponse::Ok(InvokeResponseBody::Json(v))) => send(s, 200, &ok, v.as_bytes()),
        Ok(InvokeResponse::Ok(InvokeResponseBody::Raw(v))) => {
            send(s, 200, &[("Content-Type", "application/octet-stream"), ("Cache-Control", "no-store")], &v)
        }
        Ok(InvokeResponse::Err(e)) => send(s, 500, &ok, e.0.to_string().as_bytes()),
        Err(_) => send(s, 500, &ok, serde_json::Value::from(format!("Команда {cmd} не ответила")).to_string().as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn host_flag() {
        assert_eq!(port_from_args(&args(&[])), Ok(None));
        assert_eq!(port_from_args(&args(&["whisper://room/ABCD"])), Ok(None));
        assert_eq!(port_from_args(&args(&["--host"])), Ok(Some(DEFAULT_PORT)));
        assert_eq!(port_from_args(&args(&["--host", "8080"])), Ok(Some(8080)));
        assert_eq!(port_from_args(&args(&["--host=9000"])), Ok(Some(9000)));
        assert_eq!(port_from_args(&args(&["--host", "whisper://room/ABCD"])), Ok(Some(DEFAULT_PORT)));
        assert!(port_from_args(&args(&["--host", "0"])).is_err());
        assert!(port_from_args(&args(&["--host", "abc"])).is_err());
        assert!(port_from_args(&args(&["--host=70000"])).is_err());
    }

    #[test]
    fn keys() {
        assert!(valid_key("0123456789abcdef0123456789abcdef"));
        assert!(!valid_key("0123"));
        assert!(!valid_key("0123456789abcdef0123456789abcdeg"));
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
    }
}
