//! Вход в сервисы на Android: обычная страница сервиса в окне поверх приложения (`LoginDialog` — свой WebView
//! с UA `vk::USER_AGENT`, без доступа к командам приложения), как страница входа во втором webview на компьютере.
//! Куки — общий `CookieManager` Android (в нём же куки WebView приложения и окна входа).
//!
//! `CookieManager` отдаёт куки для адреса строкой «имя=значение; …», без домена. Домен восстанавливается так:
//! кука, которая видна на самом сайте (`https://vk.ru`), — кука сайта и поддоменов (`.vk.ru`); видна только
//! на поддомене (`login.vk.ru`) — кука этого хоста. Для хранилища reqwest (`vk::jar_for`) этого достаточно:
//! запросы уходят на те же хосты.

use crate::vk::Cookie;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

type Res<T> = Result<T, String>;

/// Сайты и поддомены, на которые ходят запросы ВКонтакте и SoundCloud (vk.rs, sc.rs).
const SITES: &[(&str, &[&str])] = &[
    ("vk.ru", &["login.vk.ru", "m.vk.ru", "api.vk.ru", "id.vk.ru"]),
    ("vk.com", &["login.vk.com", "m.vk.com", "api.vk.com", "id.vk.com"]),
    ("soundcloud.com", &["api-v2.soundcloud.com", "secure.soundcloud.com", "api-auth.soundcloud.com"]),
];

fn urls() -> Vec<String> {
    let mut out = Vec::new();
    for (site, subs) in SITES {
        out.push(format!("https://{site}"));
        out.extend(subs.iter().map(|s| format!("https://{s}")));
    }
    out
}

/// «имя=значение; имя2=значение2» → пары.
fn parse(line: &str) -> Vec<(String, String)> {
    line.split(';')
        .filter_map(|p| {
            let (n, v) = p.trim().split_once('=')?;
            let n = n.trim();
            (!n.is_empty()).then(|| (n.to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Куки по ответу плагина ({ адрес: строка кук }) — с доменами по правилу выше.
fn cookies_from(map: &HashMap<String, String>) -> Vec<Cookie> {
    let mut out = Vec::new();
    for (site, subs) in SITES {
        let apex: Vec<(String, String)> = map.get(&format!("https://{site}")).map(|l| parse(l)).unwrap_or_default();
        for (name, value) in &apex {
            out.push(Cookie { name: name.clone(), value: value.clone(), domain: format!(".{site}"), path: "/".into() });
        }
        for sub in subs.iter() {
            for (name, value) in map.get(&format!("https://{sub}")).map(|l| parse(l)).unwrap_or_default() {
                if apex.iter().any(|(n, v)| *n == name && *v == value) {
                    continue;
                }
                out.push(Cookie { name, value, domain: (*sub).to_string(), path: "/".into() });
            }
        }
    }
    out
}

/// Все куки сервисов. Не из главного потока Android.
pub fn all_cookies(_app: &tauri::AppHandle) -> Res<Vec<Cookie>> {
    let map: HashMap<String, String> = super::ask("cookies", json!({ "urls": urls() }))?;
    Ok(cookies_from(&map))
}

/// Удалить куки сервиса (выход): каждой куке на каждом её адресе — истёкшая с тем же именем.
pub fn delete_cookies(_app: &tauri::AppHandle, own: fn(&str) -> bool) -> Res<()> {
    let targets: Vec<String> = urls().into_iter().filter(|u| own(u.trim_start_matches("https://"))).collect();
    let _: serde_json::Value = super::ask("cookiesDelete", json!({ "urls": targets }))?;
    Ok(())
}

#[derive(Deserialize, Default)]
struct PageState {
    open: bool,
    #[serde(default)]
    url: String,
}

fn page_state() -> PageState {
    super::ask::<PageState>("loginState", json!({})).unwrap_or_default()
}

/// Открыть страницу входа. `title` — название сервиса в полосе окна, `lang` — язык интерфейса (подписи полосы).
pub fn open(url: &str, title: &str, lang: &str) -> Res<()> {
    let en = lang == "en";
    let title = match (title, en) {
        ("ВКонтакте", true) => "VK — sign in".to_string(),
        (t, true) => format!("{t} — sign in"),
        (t, false) => format!("{t} — вход"),
    };
    let cancel = if en { "Cancel" } else { "Отмена" };
    super::send(
        "loginOpen",
        json!({ "url": url, "title": title, "cancel": cancel, "userAgent": crate::vk::USER_AGENT }),
    )
}

pub fn close() {
    let _ = super::send("loginClose", json!({}));
}

/// Страница входа ещё открыта (пользователь не закрыл её). Не из главного потока Android.
pub fn is_open() -> bool {
    page_state().open
}

/// Адрес, открытый на странице входа сейчас.
pub fn url() -> Option<tauri::Url> {
    let s = page_state();
    if !s.open {
        return None;
    }
    tauri::Url::parse(&s.url).ok()
}

pub fn navigate(url: &tauri::Url) -> bool {
    super::send("loginNavigate", json!({ "url": url.as_str() })).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_from_where_seen() {
        let mut m = HashMap::new();
        m.insert("https://vk.ru".to_string(), "remixsid=abc; remixlang=0".to_string());
        m.insert("https://login.vk.ru".to_string(), "remixsid=abc; remixlang=0; p=xyz".to_string());
        let c = cookies_from(&m);
        assert!(c.iter().any(|c| c.name == "remixsid" && c.domain == ".vk.ru"));
        assert!(c.iter().any(|c| c.name == "p" && c.domain == "login.vk.ru"));
        assert_eq!(c.iter().filter(|c| c.name == "remixsid").count(), 1);
    }
}
