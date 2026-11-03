//! Что VK Music даёт Rust-части ядра Whisper (src/core/rust — подмодуль whisper-core): Whisper Archive и
//! «Слушать вместе» (whisper/), Discord (discord.rs), виджет OBS (obs.rs). Ядро обращается сюда как
//! `crate::whisper_host::…`; образец — src/core/rust/whisper_host.example.rs.

/// Ключ wa_… — в системной связке ключей (secret.rs). В JavaScript не отдаётся никогда.
pub(crate) fn token_get() -> Result<Option<String>, String> {
    crate::secret::get()
}
pub(crate) fn token_set(token: &str) -> Result<(), String> {
    crate::secret::set(token)
}
pub(crate) fn token_clear() -> Result<(), String> {
    crate::secret::clear()
}

/// Место для большого файла в памяти: файл трека из архива бывает до 400 МБ — не больше трёх таких сразу.
pub struct HeavySlot(#[allow(dead_code)] tokio::sync::SemaphorePermit<'static>);
pub(crate) async fn heavy_slot() -> Result<HeavySlot, String> {
    static S: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);
    S.acquire().await.map(HeavySlot).map_err(|_| "Очередь загрузок закрыта".to_string())
}

/// Origin страниц приложения: протокол wa:// разрешает CORS только ему. В Linux у Tauri это tauri://localhost.
pub(crate) fn app_origin(origin: Option<&str>) -> Option<&'static str> {
    ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"].into_iter().find(|x| Some(*x) == origin)
}

/// Так приложение подписано в списке ключей на странице /developers архива (не длиннее 40 символов).
pub(crate) const DEVICE_NAME: &str = "VK Music (Whisper VK)";

// ---------- Discord и виджет OBS ----------

/// Приложение Discord, чьё название видно в строке «Слушает …». Это общее приложение «Whisper» семейства
/// (то же, что у Whisper Yandex): ID публичный, не секрет. Своё — https://discord.com/developers/applications.
pub(crate) const DISCORD_APP_ID: &str = "1561411272946352150";
/// Кнопка в профиле Discord: подпись (не длиннее 32 символов) и страница приложения.
pub(crate) const DISCORD_BUTTON: &str = "Whisper";
pub(crate) const APP_URL: &str = "https://github.com/Whisper-Music";

/// Откуда виджет OBS может брать картинки (CSP img-src его страницы): обложки ВКонтакте и SoundCloud.
pub(crate) const OBS_IMG_SRC: &str = "https://*.userapi.com https://*.vkuserphoto.ru https://*.vk.me https://*.sndcdn.com";
const COVER_HOSTS: [&str; 4] = [".userapi.com", ".vkuserphoto.ru", ".vk.me", ".sndcdn.com"];

/// Адрес обложки, который можно отдать наружу (виджет OBS, Discord) или скачать для архива:
/// https, хост — поддомен ВКонтакте или SoundCloud из COVER_HOSTS, без порта и логина.
pub(crate) fn cover_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    url.len() <= 2048
        && !host.is_empty()
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && COVER_HOSTS.iter().any(|d| host.len() > d.len() && host.ends_with(d))
}

#[cfg(test)]
mod tests {
    use super::cover_ok;

    #[test]
    fn covers() {
        assert!(cover_ok("https://sun9-1.userapi.com/impg/x.jpg?size=600x600"));
        assert!(cover_ok("https://i1.sndcdn.com/artworks-abc-t500x500.jpg"));
        assert!(!cover_ok("https://userapi.com/x"));
        assert!(!cover_ok("https://evil.com/sun9-1.userapi.com/x"));
        assert!(!cover_ok("https://sun9-1.userapi.com.evil.com/x"));
        assert!(!cover_ok("https://sun9-1.userapi.com:8443/x"));
        assert!(!cover_ok("https://user@sun9-1.userapi.com/x"));
        assert!(!cover_ok("http://sun9-1.userapi.com/x"));
        assert!(!cover_ok("vkm://localhost/cover?path=/x"));
    }
}
