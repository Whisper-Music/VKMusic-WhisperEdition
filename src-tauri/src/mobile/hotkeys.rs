//! Глобальных горячих клавиш на телефоне нет (на компьютере — hotkeys.rs): команда та же, сочетания не ставятся.

use serde::Serialize;

#[derive(Serialize)]
pub struct BindResult {
    action: String,
    error: Option<String>,
}

#[tauri::command]
pub fn hotkeys_set() -> Vec<BindResult> {
    Vec::new()
}
