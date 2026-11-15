//! Трея и мини-плеера на телефоне нет: их место занимает уведомление плеера (mobile/player.rs). Здесь — те же
//! команды и помощники, что у tray.rs, чтобы обработчик команд и настройки были одни на все платформы.
//! `mini_state` (что играет — от tray.js) уходит в уведомление и на экран блокировки.

use serde_json::Value;
use tauri::AppHandle;

pub const ICONS: [&str; 4] = ["app", "light", "dark", "accent"];

pub fn available() -> bool {
    false
}

#[allow(dead_code)]
pub fn active() -> bool {
    false
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

pub fn set_look(_: &AppHandle, _: &str, _: &str) {}

pub fn show(_: &AppHandle, _: &str, _: &str) -> Result<(), String> {
    Err("На телефоне значка в трее нет — плеер в уведомлении".into())
}

pub fn remove(_: &AppHandle) {}

#[tauri::command]
pub fn tray_state() {}

/// Что играет — в уведомление плеера.
#[tauri::command]
pub fn mini_state(state: Value) {
    let s = |k: &str| state.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let has = state.get("has").and_then(Value::as_bool).unwrap_or(false);
    let playing = state.get("playing").and_then(Value::as_bool).unwrap_or(false);
    if has {
        super::player::meta(&s("title"), &s("artist"), &s("cover"), playing);
    }
    // цвет системных панелей — отдельной командой app_bars (mobile.js: фон страницы, а не --bg темы)
}

#[tauri::command]
pub fn mini_state_get() -> Value {
    Value::Null
}

#[tauri::command]
pub fn mini_ctl() {}

#[tauri::command]
pub fn mini_toggle() -> Result<(), String> {
    Err("На телефоне мини-плеера нет — плеер в уведомлении".into())
}
