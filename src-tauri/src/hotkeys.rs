//! Глобальные горячие клавиши (настройки → «Общее» → «Горячие клавиши»), как у Whisper Yandex.
//!
//! Работают, даже когда окно свёрнуто или поверх открыта игра: в Windows это системные горячие клавиши
//! (RegisterHotKey) — нажатое сочетание «съедается», другие программы его не видят. В Linux — через X11
//! (в чистом Wayland глобальные сочетания приложениям недоступны: регистрация вернёт ошибку, интерфейс её покажет).
//!
//! Сочетания задаёт интерфейс (src/hotkeys.js) командой `hotkeys_set`; нажатия уходят в главное окно событием
//! `hotkey` (имя действия). «Показать / спрятать окно» выполняется прямо здесь.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const ACTIONS: [&str; 12] = [
    "toggle", "next", "prev", "vol_up", "vol_down", "mute", "seek_fwd", "seek_back", "like", "repeat", "shuffle", "show",
];

fn map() -> &'static Mutex<HashMap<u32, String>> {
    static M: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

pub fn plugin() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = map().lock().unwrap().get(&shortcut.id()).cloned();
            if let Some(a) = action {
                run(app, &a);
            }
        })
        .build()
}

fn run(app: &AppHandle, action: &str) {
    if action == "show" {
        if crate::host::active() {
            return;
        }
        if let Some(w) = app.get_window("main") {
            let shown = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            if shown && w.is_focused().unwrap_or(false) {
                let _ = w.minimize();
            } else {
                crate::show_main(app);
            }
        }
        return;
    }
    let _ = app.emit("hotkey", action);
}

#[derive(Deserialize)]
pub struct Bind {
    action: String,
    accel: String,
}

#[derive(Serialize)]
pub struct BindResult {
    action: String,
    error: Option<String>,
}

/// Заменить все сочетания. Возвращает, какие не удалось зарегистрировать и почему.
#[tauri::command]
pub fn hotkeys_set(app: AppHandle, binds: Vec<Bind>) -> Vec<BindResult> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut m = map().lock().unwrap();
    m.clear();
    let mut out = Vec::new();
    for b in binds.into_iter().take(ACTIONS.len()) {
        if !ACTIONS.contains(&b.action.as_str()) || b.accel.trim().is_empty() || b.accel.len() > 64 {
            continue;
        }
        let error = match Shortcut::from_str(b.accel.trim()) {
            Err(_) => Some("такое сочетание не поддерживается".to_string()),
            Ok(sc) if m.contains_key(&sc.id()) => Some("уже назначено другому действию".to_string()),
            Ok(sc) => match gs.register(sc) {
                Ok(()) => {
                    m.insert(sc.id(), b.action.clone());
                    None
                }
                Err(_) => Some("занято другой программой или системой".to_string()),
            },
        };
        out.push(BindResult { action: b.action, error });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Сочетания по умолчанию (src/hotkeys.js) и коды клавиш из браузера (KeyboardEvent.code) понятны плагину.
    #[test]
    fn accels_parse() {
        for a in [
            "Ctrl+Alt+Space", "Ctrl+Alt+PageDown", "Ctrl+Alt+PageUp", "Ctrl+Alt+Equal", "Ctrl+Alt+Minus", "Ctrl+Alt+Digit0",
            "Ctrl+Alt+Period", "Ctrl+Alt+Comma", "Ctrl+Alt+Insert", "Ctrl+Alt+KeyR", "Ctrl+Alt+KeyH", "Ctrl+Alt+Home",
            "MediaPlayPause", "MediaTrackNext", "MediaTrackPrevious", "AudioVolumeUp", "F13", "Super+Shift+F5",
        ] {
            assert!(Shortcut::from_str(a).is_ok(), "не разобралось: {a}");
        }
        assert_eq!(Shortcut::from_str("Shift+Alt+KeyM").unwrap().id(), Shortcut::from_str("Alt+Shift+KeyM").unwrap().id());
    }
}
