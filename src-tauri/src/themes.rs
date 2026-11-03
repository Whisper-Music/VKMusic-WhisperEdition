//! Хранилище тем и подготовка темы для интерфейса.
//!
//! * встроенная тема `osaka-jade` вшита в бинарник;
//! * свои темы лежат в ~/.config/vkmusic/themes/<id>.wmt;
//! * файлы темы раздаются интерфейсу по адресу vkm://localhost/theme/<id>/<имя>;
//! * основы Whisper (neon, royal, steam, lolitta) — стили без файла: их CSS — в src/whisper.css;
//! * CSS темы собирает и очищает движок тем ядра в интерфейсе, как у Whisper Yandex (`WMT.open` + `themeCss`
//!   из src/core): сюда за ним интерфейс приходит только за самим файлом (`raw`).

use crate::wmt::{self, Theme};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Секция разметки Whisper: у VK Music та же разметка, что у Whisper Yandex, поэтому тема применяется целиком.
pub const WHISPER_ID: &str = "whisper-music";
// своё для VK Music поверх неё — секция `@@ css whisper-vk` (необязательно; её подключает интерфейс)
/// секция виджета OBS — не для окна приложения
const OBS_ID: &str = "whisper-obs";
pub const DEFAULT_THEME: &str = "osaka-jade";
/// Основы оформления Whisper — стили без файла (CSS — в src/whisper.css). id заняты, как у встроенных тем.
pub const BASES: [(&str, &str); 4] = [("neon", "Неон"), ("royal", "Королевский"), ("steam", "Стимпанк"), ("lolitta", "Lolitta")];

const BUILTIN: &[(&str, &[u8])] = &[
    ("osaka-jade", include_bytes!("../../themes/osaka-jade.wmt")),
    ("vantablack", include_bytes!("../../themes/vantablack.wmt")),
    ("ashen-inventory", include_bytes!("../../themes/ashen-inventory.wmt")),
    ("slate", include_bytes!("../../themes/slate.wmt")),
];

#[derive(Debug, Clone, Serialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub builtin: bool,
    /// адрес картинки для карточки: preview из манифеста или фон
    pub preview: Option<String>,
    pub accent: Option<String>,
    pub accent2: Option<String>,
    pub background_color: Option<String>,
}

/// Что интерфейсу нужно, чтобы включить стиль. CSS и tokens темы он берёт из самого файла (`theme_file`).
#[derive(Debug, Clone, Serialize)]
pub struct ThemeRender {
    pub id: String,
    pub name: String,
    /// основа Whisper: neon | royal | steam | lolitta (у темы — manifest.base, по умолчанию neon)
    pub base: String,
    /// стиль — тема из файла (а не основа без файла)
    pub file: bool,
    /// в теме есть оформление виджета OBS (`@@ css whisper-obs`)
    pub obs: bool,
    pub background: Option<Background>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Background {
    pub url: String,
    pub video: bool,
}

pub struct ThemeStore {
    dir: PathBuf,
    cache: Mutex<HashMap<String, Arc<Theme>>>,
}

pub fn asset_url(id: &str, name: &str) -> String {
    format!("vkm://localhost/theme/{id}/{name}")
}

impl ThemeStore {
    pub fn new() -> ThemeStore {
        let dir = crate::config::config_dir().join("themes");
        let _ = std::fs::create_dir_all(&dir);
        ThemeStore { dir, cache: Mutex::new(HashMap::new()) }
    }

    /// id занят: встроенная тема или основа Whisper — своя тема с ним не ставится.
    pub fn is_builtin(id: &str) -> bool {
        BUILTIN.iter().any(|(b, _)| *b == id) || is_base(id)
    }

    fn user_files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("wmt"))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Загрузить тему по id (из кэша, встроенных или папки тем).
    pub fn get(&self, id: &str) -> Result<Arc<Theme>, String> {
        if let Some(t) = self.cache.lock().unwrap().get(id) {
            return Ok(t.clone());
        }
        let theme = if let Some((_, bytes)) = BUILTIN.iter().find(|(b, _)| *b == id) {
            wmt::open(bytes).map_err(|e| format!("встроенная тема {id}: {e}"))?
        } else {
            if !wmt::valid_theme_id(id) {
                return Err("неверный id темы".into());
            }
            let path = self.dir.join(format!("{id}.wmt"));
            let bytes = std::fs::read(&path).map_err(|_| format!("Тема «{id}» не найдена"))?;
            wmt::open(&bytes).map_err(|e| format!("{}: {e}", path.display()))?
        };
        let theme = Arc::new(theme);
        self.cache.lock().unwrap().insert(id.to_string(), theme.clone());
        Ok(theme)
    }

    /// Файл темы как есть (встроенная — из бинарника). Нужен интерфейсу только для виджета OBS:
    /// секции whisper-obs с ресурсами разбирает движок ядра (WMT.open + WhisperThemeCss.obsCss).
    pub fn raw(&self, id: &str) -> Result<Vec<u8>, String> {
        if let Some((_, bytes)) = BUILTIN.iter().find(|(b, _)| *b == id) {
            return Ok(bytes.to_vec());
        }
        if !wmt::valid_theme_id(id) {
            return Err("неверный id темы".into());
        }
        std::fs::read(self.dir.join(format!("{id}.wmt"))).map_err(|_| format!("Тема «{id}» не найдена"))
    }

    pub fn list(&self) -> Vec<ThemeInfo> {
        let mut out = Vec::new();
        for (id, _) in BUILTIN {
            if let Ok(t) = self.get(id) {
                out.push(info(&t, true));
            }
        }
        for path in self.user_files() {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if Self::is_builtin(stem) {
                continue;
            }
            if let Ok(t) = self.get(stem) {
                out.push(info(&t, false));
            }
        }
        out
    }

    /// Установить тему из байтов файла. Тот же manifest.id — обновление темы.
    pub fn install(&self, bytes: &[u8]) -> Result<ThemeInfo, String> {
        let theme = wmt::open(bytes).map_err(|e| format!("Тема не загружена: {e}"))?;
        // как у Whisper: тема без своей разметки, без общих правил и без цветов ничего не меняет
        if theme.css(WHISPER_ID).is_none() && theme.css("whisper-core").is_none() && theme.css("whisper-vk").is_none() && theme.tokens.get("color").is_none() {
            return Err("В теме нет ни секции «@@ css whisper-music» или «@@ css whisper-core», ни цветов в tokens — применять нечего".into());
        }
        let id = theme.id().to_string();
        if Self::is_builtin(&id) {
            return Err(format!(
                "id «{id}» занят встроенной темой — поменяйте manifest.id, чтобы загрузить свою версию"
            ));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.dir.join(format!("{id}.wmt"));
        std::fs::write(&path, bytes).map_err(|e| format!("Не удалось сохранить тему: {e}"))?;
        let theme = Arc::new(theme);
        self.cache.lock().unwrap().insert(id, theme.clone());
        Ok(info(&theme, false))
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        if Self::is_builtin(id) {
            return Err("Встроенную тему удалить нельзя".into());
        }
        if !wmt::valid_theme_id(id) {
            return Err("неверный id темы".into());
        }
        self.cache.lock().unwrap().remove(id);
        std::fs::remove_file(self.dir.join(format!("{id}.wmt"))).map_err(|e| e.to_string())
    }

    pub fn render(&self, id: &str) -> Result<ThemeRender, String> {
        if let Some((b, name)) = BASES.iter().find(|(b, _)| *b == id) {
            return Ok(ThemeRender { id: b.to_string(), name: name.to_string(), base: b.to_string(), file: false, obs: false, background: None });
        }
        let t = self.get(id)?;
        let id = t.id().to_string();
        let base = t.manifest_str("base").filter(|b| is_base(b)).unwrap_or("neon").to_string();
        Ok(ThemeRender {
            name: t.name().to_string(),
            base,
            file: true,
            obs: t.sections.iter().any(|x| x.kind == "css" && x.name.as_deref() == Some(OBS_ID)),
            background: t.background().map(|(name, video)| Background { url: asset_url(&id, &name), video }),
            id,
        })
    }
}

fn info(t: &Theme, builtin: bool) -> ThemeInfo {
    let id = t.id().to_string();
    let preview = t
        .manifest_str("preview")
        .and_then(|p| p.strip_prefix("asset:"))
        .filter(|p| t.files.contains_key(*p))
        .map(|p| asset_url(&id, p))
        .or_else(|| t.background().filter(|(_, v)| !v).map(|(n, _)| asset_url(&id, &n)));
    let color = |k: &str| t.tokens.pointer(&format!("/color/{k}")).and_then(Value::as_str).map(str::to_string);
    ThemeInfo {
        name: t.name().to_string(),
        author: t.manifest_str("author").map(str::to_string),
        description: t.manifest_str("description").map(str::to_string),
        version: t.manifest_str("version").map(str::to_string),
        builtin,
        preview,
        accent: color("accent").filter(|c| hex6(c).is_some()),
        accent2: color("accent2").filter(|c| hex6(c).is_some()),
        background_color: color("background").filter(|c| css_value_ok(c)),
        id,
    }
}

pub fn is_base(id: &str) -> bool {
    BASES.iter().any(|(b, _)| *b == id)
}

pub fn hex6(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

pub fn css_value_ok(s: &str) -> bool {
    let low = s.to_ascii_lowercase();
    !s.trim().is_empty()
        && s.len() <= 400
        && !s.contains([';', '{', '}', '<', '>'])
        && !low.contains("url(")
        && !low.contains("expression(")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_parses() {
        for (id, bytes) in BUILTIN {
            let t = wmt::open(bytes).unwrap_or_else(|e| panic!("{id}.wmt: {e}"));
            assert_eq!(t.id(), *id);
            assert!(t.background().is_some(), "{id}: нет фона");
            assert!(t.css(WHISPER_ID).is_some(), "{id}: нет CSS секции {WHISPER_ID}");
            assert!(t.tokens.pointer("/color/accent").and_then(Value::as_str).and_then(hex6).is_some(), "{id}: нет accent");
        }
    }

    #[test]
    fn bases_render_without_file() {
        let st = ThemeStore { dir: std::env::temp_dir().join("vkm-no-themes"), cache: Mutex::new(HashMap::new()) };
        let r = st.render("royal").unwrap();
        assert!(!r.file && r.base == "royal" && r.background.is_none());
        assert!(ThemeStore::is_builtin("lolitta") && ThemeStore::is_builtin("osaka-jade"));
        let j = st.render("osaka-jade").unwrap();
        assert!(j.file && j.base == "neon" && j.background.is_some());
    }
}
