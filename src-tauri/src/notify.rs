//! Уведомления «Слушать вместе» (как у Whisper Yandex, просьба владельца 2026-11-17): друг зовёт в комнату, участник
//! «подмигнул». Модули ядра зовут `kit.notify` → `notify.js` (включено ли, что показывать) → `notify_show`.
//!
//! * Linux — уведомление системы (org.freedesktop.Notifications по D-Bus): на Wayland приложение не может само
//!   поставить окно в угол экрана, а сервер уведомлений (GNOME, KDE, mako, swaync, quickshell…) умеет. Нажатие —
//!   действие `default` (сигнал ActionInvoked) → открыть VK Music. Угол и вид выбирает сервер.
//! * Windows — своё окно-карточка, как у Whisper Yandex (notifywin.html): поверх всех, в выбранном углу рабочей
//!   области, без значка на панели задач и без перехвата фокуса; стопка до трёх карточек; окно создаётся под первое
//!   уведомление и закрывается с последним.
//! Звук играет главное окно (notify.js), если включён.

use serde::Deserialize;
use tauri::AppHandle;

#[derive(Deserialize)]
pub struct Note {
    kind: Option<String>,
    title: String,
    body: String,
    corner: Option<String>,
    accent: Option<String>,
    accent2: Option<String>,
}

/// Строка для уведомления: без управляющих символов, не длиннее `max` символов.
fn clean(s: &str, max: usize) -> String {
    let t: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > max {
        t.chars().take(max - 1).collect::<String>() + "…"
    } else {
        t
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn corner(s: Option<&str>) -> &'static str {
    match s.unwrap_or("") {
        "tl" => "tl",
        "tr" => "tr",
        "bl" => "bl",
        _ => "br",
    }
}

/// Цвет акцента карточки: только #rgb / #rrggbb / #rrggbbaa.
#[cfg_attr(not(windows), allow(dead_code))]
fn color(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string())
        .filter(|c| matches!(c.len(), 4 | 7 | 9) && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit()))
}

/// Показать уведомление (только из главного окна).
#[tauri::command]
pub async fn notify_show(app: AppHandle, webview: tauri::Webview, note: Note) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Нет доступа".into());
    }
    let title = clean(&note.title, 80);
    let body = clean(&note.body, 240);
    if title.is_empty() {
        return Err("пустой заголовок".into());
    }
    let kind = clean(note.kind.as_deref().unwrap_or(""), 16);
    #[cfg(target_os = "linux")]
    {
        let _ = (&kind, &note.corner, &note.accent, &note.accent2);
        tauri::async_runtime::spawn_blocking(move || fdo::show(&app, &title, &body))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "linux"))]
    {
        win::show(
            &app,
            serde_json::json!({
                "kind": kind, "title": title, "body": body, "corner": corner(note.corner.as_deref()),
                "accent": color(note.accent), "accent2": color(note.accent2),
            }),
        )
    }
}

#[cfg(target_os = "linux")]
mod fdo {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use tauri::AppHandle;
    use zbus::blocking::Connection;
    use zbus::zvariant::Value;

    const DEST: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";

    static CONN: OnceLock<Option<Connection>> = OnceLock::new();
    /// номера наших уведомлений (последние 50): ActionInvoked приходит про все уведомления сеанса
    static IDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

    pub fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
        let conn = CONN
            .get_or_init(|| {
                let c = Connection::session().ok()?;
                listen(app.clone(), c.clone());
                Some(c)
            })
            .as_ref()
            .ok_or("Нет сеанса D-Bus — уведомления системы недоступны")?;
        let mut hints: HashMap<&str, Value> = HashMap::new();
        hints.insert("desktop-entry", Value::from("vkmusic"));
        hints.insert("category", Value::from("im.received"));
        let actions = vec!["default", "Открыть"];
        let reply = conn
            .call_method(Some(DEST), PATH, Some(DEST), "Notify", &("VK Music", 0u32, "vkmusic", title, body, actions, hints, 7000i32))
            .map_err(|e| format!("Сервер уведомлений не ответил: {e}"))?;
        let id: u32 = reply.body().deserialize().map_err(|e| e.to_string())?;
        let mut ids = IDS.lock().unwrap();
        ids.push(id);
        if ids.len() > 50 {
            ids.remove(0);
        }
        Ok(())
    }

    /// Нажатие на наше уведомление — открыть VK Music (окно или вкладку браузера с --host).
    fn listen(app: AppHandle, conn: Connection) {
        std::thread::spawn(move || {
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(DEST)
                .and_then(|b| b.member("ActionInvoked"))
                .map(|b| b.build());
            let Ok(rule) = rule else { return };
            let Ok(it) = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None) else { return };
            for m in it.flatten() {
                let Ok((id, _action)) = m.body().deserialize::<(u32, String)>() else { continue };
                if IDS.lock().unwrap().contains(&id) {
                    let a = app.clone();
                    let _ = app.run_on_main_thread(move || crate::tray::open(&a));
                }
            }
        });
    }
}

/// Окно уведомлений (Windows) забирает накопившееся.
#[tauri::command]
pub fn notify_take(app: AppHandle, webview: tauri::Webview) -> Vec<serde_json::Value> {
    #[cfg(not(target_os = "linux"))]
    if webview.label() == win::LABEL {
        return win::take(&app);
    }
    let _ = (app, webview);
    Vec::new()
}

/// Окно сообщает высоту стопки; 0 — карточек не осталось.
#[tauri::command]
pub fn notify_layout(app: AppHandle, webview: tauri::Webview, h: f64, taken: u64) {
    #[cfg(not(target_os = "linux"))]
    if webview.label() == win::LABEL {
        win::layout(&app, h, taken);
    }
    let _ = (app, webview, h, taken);
}

/// Нажатие на карточку — открыть VK Music.
#[tauri::command]
pub fn notify_ctl(app: AppHandle, webview: tauri::Webview, action: String) {
    if webview.label() == "notify" && action == "open" {
        crate::tray::open(&app);
    }
}

#[cfg(not(target_os = "linux"))]
mod win {
    use serde_json::Value;
    use std::sync::Mutex;
    use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

    pub const LABEL: &str = "notify";
    /// ширина окна (логические px): карточка 360 + по 10 по краям на тень
    const W: f64 = 380.0;
    const GAP: f64 = 6.0;

    struct Shared {
        pending: Vec<Value>,
        /// сколько уведомлений отдано этому экземпляру окна — не закрыть его, пока новое ещё в пути
        pushed: u64,
        corner: String,
        height: f64,
    }
    static ST: Mutex<Shared> = Mutex::new(Shared { pending: Vec::new(), pushed: 0, corner: String::new(), height: 100.0 });

    pub fn show(app: &AppHandle, item: Value) -> Result<(), String> {
        let app2 = app.clone();
        app.run_on_main_thread(move || {
            {
                let mut st = ST.lock().unwrap();
                st.corner = item.get("corner").and_then(Value::as_str).unwrap_or("br").to_string();
                st.pending.push(item);
            }
            match app2.get_webview_window(LABEL) {
                Some(w) => {
                    ST.lock().unwrap().pushed += 1;
                    place(&app2, &w);
                    let _ = app2.emit_to(LABEL, "notify-poke", ());
                }
                None => open(&app2),
            }
        })
        .map_err(|e| format!("окно уведомления не открылось: {e}"))
    }

    pub fn take(_app: &AppHandle) -> Vec<Value> {
        std::mem::take(&mut ST.lock().unwrap().pending)
    }

    pub fn layout(app: &AppHandle, h: f64, taken: u64) {
        if h <= 0.0 {
            let busy = {
                let st = ST.lock().unwrap();
                st.pushed != taken || !st.pending.is_empty()
            };
            if busy {
                let _ = app.emit_to(LABEL, "notify-poke", ());
            } else if let Some(w) = app.get_webview_window(LABEL) {
                let _ = w.destroy();
            }
            return;
        }
        ST.lock().unwrap().height = h.clamp(40.0, 900.0);
        if let Some(w) = app.get_webview_window(LABEL) {
            place(app, &w);
        }
    }

    fn rect(app: &AppHandle) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
        let (c, h) = {
            let st = ST.lock().unwrap();
            (st.corner.clone(), st.height)
        };
        let m = app
            .get_webview_window("main")
            .and_then(|m| m.current_monitor().ok().flatten())
            .or_else(|| app.primary_monitor().ok().flatten())?;
        let sc = m.scale_factor();
        let wa = m.work_area();
        let (pw, ph, gap) = ((W * sc).round() as i32, (h * sc).round() as i32, (GAP * sc).round() as i32);
        let (l, t) = (wa.position.x, wa.position.y);
        let (r, b) = (l + wa.size.width as i32, t + wa.size.height as i32);
        let ph = ph.min(b - t);
        let x = if c.ends_with('l') { l + gap } else { r - pw - gap };
        let y = if c.starts_with('t') { t + gap } else { b - ph - gap };
        Some((PhysicalPosition::new(x, y.max(t)), PhysicalSize::new(pw.max(1) as u32, ph.max(1) as u32)))
    }

    fn place(app: &AppHandle, w: &tauri::WebviewWindow) {
        if let Some((p, s)) = rect(app) {
            let _ = w.set_position(p);
            let _ = w.set_size(s);
        }
    }

    fn open(app: &AppHandle) {
        {
            let mut st = ST.lock().unwrap();
            st.pushed = st.pending.len() as u64;
            st.height = 100.0;
        }
        let Ok(w) = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("notifywin.html".into()))
            .title("Уведомление")
            .inner_size(W, 100.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .build()
        else {
            return;
        };
        place(app, &w);
        show_quiet(&w);
    }

    /// Показать, не забирая фокус у текущей программы.
    #[cfg(windows)]
    fn show_quiet(w: &tauri::WebviewWindow) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        match w.hwnd() {
            Ok(h) => unsafe {
                ShowWindow(h.0 as _, SW_SHOWNOACTIVATE);
            },
            Err(_) => {
                let _ = w.show();
            }
        }
    }
    #[cfg(not(windows))]
    fn show_quiet(w: &tauri::WebviewWindow) {
        let _ = w.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans() {
        assert_eq!(clean("  Вадим\nзовёт\t в комнату ", 80), "Вадим зовёт в комнату");
        assert_eq!(clean("абвгдежзик", 5), "абвг…");
        assert_eq!(corner(Some("tl")), "tl");
        assert_eq!(corner(None), "br");
        assert_eq!(color(Some("#a1b2c3".into())).as_deref(), Some("#a1b2c3"));
        assert_eq!(color(Some("red".into())), None);
    }
}
