//! Хранилище тем и подготовка темы для интерфейса.
//!
//! * встроенная тема `osaka-jade` вшита в бинарник;
//! * свои темы лежат в ~/.config/vkmusic/themes/<id>.wmt;
//! * файлы темы раздаются интерфейсу по адресу vkm://localhost/theme/<id>/<имя>;
//! * CSS темы здесь собирается и проходит первую очистку (`sanitize_css`), а привязку к стилю, роли
//!   ядра Whisper и окончательный фильтр делает движок тем ядра в интерфейсе (`src/core/theme-css.js`).

use crate::wmt::{self, Theme};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// id приложения в секциях `@@ css whisper-vk` / `@@ settings whisper-vk` (как в ядре Whisper: `Whisper.setApp`)
pub const APP_ID: &str = "whisper-vk";
/// прежнее имя секций — темы, написанные до перехода на ядро Whisper, читаются, если своей секции нет
pub const LEGACY_APP_ID: &str = "vkmusic";
/// общая секция тем Whisper: правила по ролям интерфейса (`[data-w="player.play"]`), одинаковые во всех приложениях
pub const CORE_ID: &str = "whisper-core";
/// секция виджета OBS — не для окна приложения
const OBS_ID: &str = "whisper-obs";
pub const DEFAULT_THEME: &str = "osaka-jade";

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

#[derive(Debug, Clone, Serialize)]
pub struct ThemeRender {
    pub id: String,
    pub name: String,
    pub base: Option<String>,
    /// CSS темы (`@font-face` чужих секций, `@@ css whisper-core`, `@@ css whisper-vk`), уже без
    /// @import/внешних адресов, asset: → vkm://. Интерфейс привязывает его к стилю движком тем ядра.
    pub css: String,
    /// :root { --accent: … } из tokens — подключается после CSS темы и главнее его
    pub vars: String,
    /// адреса файлов темы (vkm://…) — единственные, которые движок тем ядра оставит в url()
    pub assets: Vec<String>,
    pub settings: Value,
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

    pub fn is_builtin(id: &str) -> bool {
        BUILTIN.iter().any(|(b, _)| *b == id)
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
        let t = self.get(id)?;
        let id = t.id().to_string();
        Ok(ThemeRender {
            name: t.name().to_string(),
            base: t.manifest_str("base").map(str::to_string),
            css: sanitize_css(&theme_css(&t), &id, &t),
            vars: token_vars(&t.tokens),
            assets: {
                let mut v: Vec<String> = t.files.keys().map(|n| asset_url(&id, n)).collect();
                v.sort();
                v
            },
            settings: app_settings(&t),
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

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("regex"))
}

/// Секция приложения в теме: `whisper-vk`, а у тем, написанных до перехода на ядро, — `vkmusic`.
pub fn app_section(t: &Theme) -> &'static str {
    let has = |app: &str| t.sections.iter().any(|s| matches!(s.kind.as_str(), "css" | "settings") && s.name.as_deref() == Some(app));
    if !has(APP_ID) && has(LEGACY_APP_ID) {
        LEGACY_APP_ID
    } else {
        APP_ID
    }
}

pub fn app_settings(t: &Theme) -> Value {
    t.settings(app_section(t))
}

/// CSS темы для VK Music: шрифты `@font-face` из секций других приложений (темы из библиотеки
/// Whisper Music подключают свои шрифты в `@@ css whisper-music`), затем общая секция ролей
/// `@@ css whisper-core` и своя `@@ css whisper-vk` (она главнее — идёт последней).
/// Чужую разметку не берём — у приложений разный интерфейс, — а шрифты самодостаточны.
pub fn theme_css(t: &Theme) -> String {
    static FACE: OnceLock<Regex> = OnceLock::new();
    let face = re(&FACE, r"(?s)@font-face\s*\{[^{}]*\}");
    let app = app_section(t);
    let own = |s: &crate::wmt::Section| matches!(s.name.as_deref(), Some(n) if n == app || n == CORE_ID || n == OBS_ID);
    let mut out = String::new();
    for sec in t.sections.iter().filter(|s| s.kind == "css" && !own(s)) {
        for m in face.find_iter(&sec.body) {
            out.push_str(m.as_str());
            out.push('\n');
        }
    }
    for css in [t.css(CORE_ID), t.css(app)].into_iter().flatten() {
        out.push_str(&css);
        out.push('\n');
    }
    out
}

/// Привести CSS темы к безопасному виду: без @import, без внешних адресов; `asset:имя` → vkm://.
pub fn sanitize_css(css: &str, id: &str, theme: &Theme) -> String {
    static IMPORT: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    let css = re(&IMPORT, r"(?i)@import[^;]*;?").replace_all(css, "");
    let css = re(&URL, r#"(?i)url\(\s*(?:"([^"]*)"|'([^']*)'|([^)'"]*))\s*\)"#).replace_all(&css, |c: &regex::Captures| {
        let target = c
            .get(1)
            .or_else(|| c.get(2))
            .or_else(|| c.get(3))
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        if let Some(name) = target.strip_prefix("asset:") {
            if theme.files.contains_key(name) {
                return format!("url(\"{}\")", asset_url(id, name));
            }
        } else if target.starts_with("data:") && !target.contains('"') {
            return format!("url(\"{target}\")");
        }
        "url(\"\")".to_string()
    });
    // закрывающий тег не нужен CSS и не должен попасть в <style>
    css.replace("</", "<\\/")
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

fn num(v: &Value, ptr: &str, lo: f64, hi: f64) -> Option<f64> {
    v.pointer(ptr).and_then(Value::as_f64).map(|n| n.clamp(lo, hi))
}

/// tokens → `:root { … }` с CSS-переменными приложения.
pub fn token_vars(tokens: &Value) -> String {
    let mut vars: Vec<(String, String)> = Vec::new();
    let s = |ptr: &str| tokens.pointer(ptr).and_then(Value::as_str);

    for (key, var) in [("accent", "accent"), ("accent2", "accent2")] {
        if let Some(c) = s(&format!("/color/{key}")) {
            if let Some((r, g, b)) = hex6(c) {
                vars.push((var.into(), c.to_string()));
                vars.push((format!("{var}-rgb"), format!("{r}, {g}, {b}")));
            }
        }
    }
    for (key, var) in [("text", "text"), ("muted", "muted"), ("background", "bg")] {
        if let Some(c) = s(&format!("/color/{key}")).filter(|c| css_value_ok(c)) {
            vars.push((var.into(), c.to_string()));
            if let Some((r, g, b)) = hex6(c) {
                vars.push((format!("{var}-rgb"), format!("{r}, {g}, {b}")));
            }
        }
    }
    if let Some(n) = num(tokens, "/effects/glow", 0.0, 48.0) {
        vars.push(("glow".into(), format!("{n}px")));
    }
    if let Some(n) = num(tokens, "/effects/panelOpacity", 0.2, 1.0) {
        vars.push(("panel-opacity".into(), format!("{n}")));
    }
    if let Some(n) = num(tokens, "/effects/dim", 0.0, 0.85) {
        vars.push(("dim".into(), format!("{n}")));
    }
    if let Some(n) = num(tokens, "/effects/backgroundBlur", 0.0, 64.0) {
        vars.push(("bg-blur".into(), format!("{n}px")));
    }
    for (key, var) in [("body", "font-body"), ("display", "font-display")] {
        if let Some(f) = s(&format!("/font/{key}")).filter(|f| css_value_ok(f)) {
            vars.push((var.into(), f.to_string()));
        }
    }
    let body: Vec<String> = vars.iter().map(|(k, v)| format!("  --{k}: {v};")).collect();
    format!(":root {{\n{}\n}}", body.join("\n"))
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
            assert!(t.css(APP_ID).is_some(), "{id}: нет CSS секции {APP_ID}");
            assert!(token_vars(&t.tokens).contains("--accent:"), "{id}: нет --accent");
        }
    }

    #[test]
    fn foreign_font_faces() {
        let t = wmt::parse_text(
            "#WMT 1\n@@ manifest\n{\"id\":\"a\",\"name\":\"a\"}\n@@ asset f.woff2 text\nx\n@@ css whisper-music\n@font-face { font-family: \"Hand\"; src: url(asset:f.woff2) format(\"woff2\"); }\n.panel { display: none; }\n@@ css vkmusic\n.x{color:red}",
        )
        .unwrap();
        let css = sanitize_css(&theme_css(&t), "a", &t);
        assert!(css.contains("font-family: \"Hand\""));
        assert!(css.contains("vkm://localhost/theme/a/f.woff2"));
        assert!(!css.contains("display: none"), "чужая разметка не должна попадать");
        assert!(css.contains(".x{color:red}"));
        assert_eq!(app_section(&t), LEGACY_APP_ID);
    }

    #[test]
    fn core_and_app_sections() {
        let t = wmt::parse_text(
            "#WMT 1\n@@ manifest\n{\"id\":\"a\",\"name\":\"a\"}\n@@ css whisper-vk\n.own{color:red}\n@@ css vkmusic\n.old{color:blue}\n@@ css whisper-core\n[data-w=\"player\"]{color:green}\n@@ css whisper-obs\n.obs{color:pink}\n@@ settings whisper-vk\n{\"x\":1}",
        )
        .unwrap();
        assert_eq!(app_section(&t), APP_ID);
        let css = theme_css(&t);
        let (core, own) = (css.find("data-w").unwrap(), css.find(".own").unwrap());
        assert!(core < own, "своя секция должна идти после общей");
        assert!(!css.contains(".old") && !css.contains(".obs"));
        assert_eq!(app_settings(&t)["x"], 1);
    }

    #[test]
    fn css_is_sanitized() {
        let t = wmt::parse_text("#WMT 1\n@@ manifest\n{\"id\":\"a\",\"name\":\"a\"}\n@@ asset f.png text\nx").unwrap();
        let out = sanitize_css(
            "@import url(http://evil/x.css);\n.a{background:url(asset:f.png)} .b{background:url('https://x/y.png')} .c{background:url(\"asset:none.png\")}",
            "a",
            &t,
        );
        assert!(!out.contains("@import"));
        assert!(out.contains("vkm://localhost/theme/a/f.png"));
        assert!(!out.contains("https://x"));
    }
}
