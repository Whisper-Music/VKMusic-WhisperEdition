//! Ссылки whisper://room/КОД — приглашения в комнату «Слушать вместе» (core/together.js).
//!
//! Друзьям уходит https-ссылка архива (https://origin.whisper-archive.ru/join/КОД): страница сама открывает
//! whisper://room/КОД, а система передаёт её приложению — аргументом при запуске (ярлык packaging/vkmusic.desktop,
//! MimeType x-scheme-handler/whisper) или, если оно уже открыто, через single-instance (feature deep-link отдаёт
//! ссылку второго запуска плагину deep-link — on_open_url ниже).
//!
//! Из ссылки берётся только код по строгому шаблону. Войти в комнату пользователь подтверждает в окне —
//! ссылка ничего не делает молча. Схему в системе приложение само не регистрирует (это делает install.sh).

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;

/// Код из ссылки, с которой запустили приложение, — пока интерфейс его не забрал (room_link_take).
#[derive(Default)]
pub struct Pending(Mutex<Option<String>>);

/// whisper://room/K7QH2M → "K7QH2M": 4–12 латинских букв и цифр, регистр не важен.
pub fn room_code(url: &Url) -> Option<String> {
    if !url.scheme().eq_ignore_ascii_case("whisper") {
        return None;
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let mut parts = url.path().split('/').filter(|s| !s.is_empty());
    let raw = if host == "room" || host == "join" {
        parts.next()?
    } else {
        match parts.next()? {
            p if p.eq_ignore_ascii_case("room") || p.eq_ignore_ascii_case("join") => parts.next()?,
            _ => return None,
        }
    };
    if parts.next().is_some() {
        return None;
    }
    let code = raw.to_ascii_uppercase();
    ((4..=12).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())).then_some(code)
}

/// running — ссылка пришла в уже открытое приложение: сразу событием в окно и окно — наверх.
fn handle(app: &AppHandle, urls: &[Url], running: bool) {
    let Some(code) = urls.iter().find_map(room_code) else { return };
    if let Some(p) = app.try_state::<Pending>() {
        *p.0.lock().unwrap() = Some(code.clone());
    }
    if running {
        let _ = app.emit("room-link", code);
        crate::show_main(app);
    }
}

/// В setup(): ссылка, с которой запущено приложение, и подписка на новые.
pub fn init(app: &AppHandle) {
    app.manage(Pending::default());
    let h = app.clone();
    app.deep_link().on_open_url(move |event| handle(&h, &event.urls(), true));
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        handle(app, &urls, false);
    }
    // запуск по ссылке без регистрации схемы плагином (Linux: ссылка — просто аргумент)
    let args: Vec<String> = std::env::args().skip(1).collect();
    let urls: Vec<Url> = args.iter().filter_map(|a| Url::parse(a).ok()).collect();
    handle(app, &urls, false);
}

/// Интерфейс загрузился — забрать код ссылки, с которой запустили (один раз).
#[tauri::command]
pub fn room_link_take(p: tauri::State<'_, Pending>) -> Option<String> {
    p.0.lock().unwrap().take()
}

#[cfg(test)]
mod tests {
    use super::room_code;
    use tauri::Url;

    #[test]
    fn codes() {
        let c = |s: &str| room_code(&Url::parse(s).unwrap());
        assert_eq!(c("whisper://room/k7qh2m").as_deref(), Some("K7QH2M"));
        assert_eq!(c("whisper://join/ABCD").as_deref(), Some("ABCD"));
        assert_eq!(c("whisper:/room/ABCD").as_deref(), Some("ABCD"));
        assert_eq!(c("whisper://room/AB"), None);
        assert_eq!(c("whisper://room/ABCD/x"), None);
        assert_eq!(c("whisper://room/AB-CD"), None);
        assert_eq!(c("https://room/ABCD"), None);
    }
}
