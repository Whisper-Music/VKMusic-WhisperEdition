//! Команды интерфейса по разделам (до 2026-11-06 — все в main.rs, теперь общее — в lib.rs).

pub(crate) mod player_cmd;
pub(crate) mod vk_cmd;
// на телефоне раскладки GTK и второго webview нет (mobile/login.rs)
#[cfg_attr(mobile, allow(dead_code))]
pub(crate) mod login;
pub(crate) mod sc_cmd;
pub(crate) mod saved_cmd;
pub(crate) mod local_cmd;
pub(crate) mod lyrics_cmd;
pub(crate) mod storage_cmd;
pub(crate) mod themes_cmd;
