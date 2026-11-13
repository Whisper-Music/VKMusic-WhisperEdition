//! Стиль «Omarchy» (просьба владельца, 2026-11-09): VK Music подстраивается под текущую тему Omarchy.
//!
//! Стиль с id `omarchy` — обычная тема .wmt, только собирает её не файл, а этот модуль: из палитры текущей темы
//! (`colors.toml`, у старых тем Omarchy — `alacritty.toml`, у своей темы без них — `quickshell.json`), её обоев
//! и превью. Разметка — та же, что у встроенной Osaka Jade (плитки, как в Hyprland, со скруглёнными углами), цвета — подставленные.
//! Дальше тема идёт обычным путём (`ThemeStore`, `theme_file`, движок тем ядра), поэтому правила безопасности тем
//! действуют и здесь.
//!
//! Где взять текущую тему (только чтение, ничего не пишем) — `locate`, по порядку:
//! * Omarchy 3.3+ — `~/.local/state/omarchy/current/theme/` (копия темы с `colors.toml`), `theme.name`,
//!   ссылка `background` — текущие обои; старые Omarchy — `~/.config/omarchy/current/theme` (ссылка на папку темы)
//!   и `~/.config/omarchy/current/background`;
//! * без Omarchy — переключатель тем владельца `~/.config/themes/theme` (не Omarchy: темы Omarchy лежат клоном
//!   в `~/.config/themes/omarchy/themes`): имя текущей — `~/.local/state/themes/current`, папка —
//!   `~/.config/themes/<имя>` или `~/.config/themes/omarchy/themes/<имя>` (с `colors.toml`; своя тема без него —
//!   `~/.config/themes/<имя>` с `quickshell.json`), путь к обоям — первая строка `~/.cache/current_wallpaper`.
//!
//! Смену темы замечает `watch`: раз в секунду сравнивает отпечаток (`signature`: пути, размеры и время изменения
//! файлов — без чтения больших) и, когда новый отпечаток держится две проверки подряд (тема меняется в несколько
//! шагов: палитра, затем обои), шлёт интерфейсу `theme://omarchy`. Следит, только пока выбран стиль `omarchy`.

use crate::wmt::{self, Theme};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub const ID: &str = "omarchy";
pub const EVENT: &str = "theme://omarchy";
/// превью темы для карточки стиля — не больше (у Omarchy это снимок экрана, обычно ~1–3 МБ)
const MAX_PREVIEW: u64 = 8 << 20;
/// файлы палитры, по порядку: какой нашёлся первым — тот и читается
const PALETTE_FILES: [&str; 3] = ["colors.toml", "alacritty.toml", "quickshell.json"];

/// Текущая тема на диске.
struct Paths {
    /// папка темы (настоящая, после canonicalize)
    dir: PathBuf,
    /// имя темы как есть («tokyo-night»)
    name: String,
    /// текущие обои (настоящий файл)
    background: Option<PathBuf>,
}

fn first_line(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let line = s.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Имя темы из файла состояния: только простое имя папки, без «/» и «..».
fn plain_name(s: &str) -> Option<&str> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('.')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    ok.then_some(s)
}

fn has_palette(dir: &Path) -> bool {
    PALETTE_FILES.iter().any(|f| dir.join(f).is_file())
}

fn locate() -> Option<Paths> {
    // пути у Omarchy и у переключателя владельца зашиты от $HOME (XDG_* они не читают) — так же и здесь
    let home = crate::config::home();
    for cur in [home.join(".local/state/omarchy/current"), home.join(".config/omarchy/current")] {
        let Ok(dir) = std::fs::canonicalize(cur.join("theme")) else { continue };
        if !dir.is_dir() {
            continue;
        }
        let name = first_line(&cur.join("theme.name"))
            .or_else(|| dir.file_name()?.to_str().map(str::to_string))
            .unwrap_or_else(|| "Omarchy".into());
        let background = std::fs::canonicalize(cur.join("background")).ok().filter(|b| b.is_file());
        return Some(Paths { dir, name, background });
    }
    let root = home.join(".config/themes");
    let name = first_line(&home.join(".local/state/themes/current"))?;
    let name = plain_name(&name)?.to_string();
    let with_colors = [root.join(&name), root.join("omarchy/themes").join(&name)]
        .into_iter()
        .find(|d| d.join("colors.toml").is_file());
    let dir = with_colors.or_else(|| Some(root.join(&name)).filter(|d| has_palette(d)))?;
    let dir = std::fs::canonicalize(dir).ok()?;
    let background = first_line(&home.join(".cache/current_wallpaper"))
        .and_then(|p| std::fs::canonicalize(p).ok())
        .filter(|b| b.is_file());
    Some(Paths { dir, name, background })
}

/// Отпечаток текущей темы: меняется вместе с палитрой, обоями и режимом. None — темы сейчас нет (или она
/// посреди смены: Omarchy удаляет старую папку и переносит новую).
fn signature(p: &Paths) -> Option<String> {
    let meta = |path: &Path| std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()));
    if !has_palette(&p.dir) {
        return None;
    }
    let files: Vec<_> = PALETTE_FILES.iter().map(|f| meta(&p.dir.join(f))).collect();
    let bg = p.background.as_ref().map(|b| (b, meta(b)));
    Some(format!("{}|{:?}|{:?}|{files:?}|{bg:?}|{}", p.name, p.dir, meta(&p.dir), p.dir.join("light.mode").exists()))
}

/// Тема есть: её имя для показа («Tokyo Night»).
pub fn current_name() -> Option<String> {
    let p = locate()?;
    signature(&p)?;
    Some(pretty_name(&p.name))
}

/// Как omarchy-theme-current: «tokyo-night» → «Tokyo Night».
fn pretty_name(raw: &str) -> String {
    let pretty = raw
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    pretty.chars().filter(|c| !c.is_control()).take(60).collect()
}

// ───────────────────────── палитра ─────────────────────────

/// `#rrggbb` (или `0xrrggbb` из alacritty.toml) → `#rrggbb` в нижнем регистре; остальное — None.
fn hex(v: &str) -> Option<String> {
    let v = v.trim();
    let h = v.strip_prefix('#').or_else(|| v.strip_prefix("0x"))?;
    (h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{}", h.to_ascii_lowercase()))
}

/// Значение строки `ключ = значение` без кавычек и комментария.
fn toml_value(v: &str) -> String {
    let v = v.trim();
    match v.chars().next() {
        Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_string(),
        _ => v.split('#').next().unwrap_or("").trim().to_string(),
    }
}

/// colors.toml Omarchy: плоский список `ключ = "значение"` (у Omarchy и разбор такой же — построчный).
fn parse_colors(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim().trim_matches(['"', '\'']);
        if !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            m.insert(k.to_string(), toml_value(v));
        }
    }
    m
}

/// alacritty.toml старых тем Omarchy → ключи colors.toml (как `omarchy-theme-colors-from-alacritty`).
fn parse_alacritty(text: &str) -> HashMap<String, String> {
    const ANSI: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    let mut m = HashMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(s) = line.strip_prefix('[') {
            section = s.trim_end_matches(']').trim().to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), toml_value(v));
        let key = match section.as_str() {
            "colors.primary" if k == "background" || k == "foreground" => k.to_string(),
            "colors.selection" if k == "background" => "selection_background".into(),
            "colors.normal" => match ANSI.iter().position(|a| *a == k) {
                Some(i) => format!("color{i}"),
                None => continue,
            },
            "colors.bright" => match ANSI.iter().position(|a| *a == k) {
                Some(i) => format!("color{}", i + 8),
                None => continue,
            },
            _ => continue,
        };
        m.insert(key, v);
    }
    m
}

/// quickshell.json своей темы (палитра в духе wallust: background, foreground, color0…15, muted): в шаблоне
/// переключателя `color1` — это accent.
fn parse_json(text: &str) -> HashMap<String, String> {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let mut m: HashMap<String, String> = v
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    if !m.contains_key("accent") {
        if let Some(a) = m.get("color1").cloned() {
            m.insert("accent".into(), a);
        }
    }
    // «muted» там — второстепенный текст (dark_foreground), а не цвет комментариев
    if let Some(mu) = m.remove("muted") {
        m.entry("dark_foreground".into()).or_insert(mu);
    }
    m
}

type Rgb = (f64, f64, f64);

fn rgb(h: &str) -> Rgb {
    let (r, g, b) = crate::themes::hex6(h).unwrap_or((0, 0, 0));
    (r as f64, g as f64, b as f64)
}

fn to_hex((r, g, b): Rgb) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

/// Смесь двух цветов: amount — доля второго (как `mix_color` у Omarchy).
fn mix(a: &str, b: &str, amount: f64) -> String {
    let (a, b) = (rgb(a), rgb(b));
    to_hex((a.0 + (b.0 - a.0) * amount, a.1 + (b.1 - a.1) * amount, a.2 + (b.2 - a.2) * amount))
}

fn rgba(h: &str, a: f64) -> String {
    let (r, g, b) = rgb(h);
    format!("rgba({r}, {g}, {b}, {a})")
}

fn luminance(h: &str) -> f64 {
    let ch = |v: f64| {
        let v = v / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = rgb(h);
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Палитра для темы VK Music (все значения — проверенные `#rrggbb`).
#[derive(Debug, Clone, PartialEq)]
struct Palette {
    light: bool,
    bg: String,
    fg: String,
    heading: String,
    muted: String,
    accent: String,
    accent2: String,
    on_accent: String,
    line: String,
    surface: String,
    selection: String,
    danger: String,
}

/// Значения Omarchy → палитра. Порядок замен — как в `omarchy-theme-color` (новые имена, короткие, ANSI colorN).
fn palette(raw: &HashMap<String, String>, light_file: bool) -> Option<Palette> {
    let get = |keys: &[&str]| keys.iter().find_map(|k| raw.get(*k).and_then(|v| hex(v)));
    let bg = get(&["background", "bg", "color0"])?;
    let fg = get(&["foreground", "fg", "color7"])?;
    let mode = raw.get("mode").or_else(|| raw.get("theme_type")).map(|s| s.to_ascii_lowercase());
    let light = match mode.as_deref() {
        Some("light") => true,
        Some("dark") => false,
        _ => light_file || {
            let (r, g, b) = rgb(&bg);
            r + g + b > 382.0
        },
    };
    let accent = get(&["accent", "blue", "color4"]).unwrap_or_else(|| fg.clone());
    let heading = get(&["bright_foreground", "bright_fg", "color15"]).filter(|h| contrast(h, &bg) >= 4.5).unwrap_or_else(|| fg.clone());
    // второстепенный текст: dark_foreground у тем Omarchy бывает почти фоном (это цвет комментариев) — тогда смесь
    let muted = get(&["dark_foreground", "dark_fg"]).filter(|m| contrast(m, &bg) >= 3.0).unwrap_or_else(|| mix(&fg, &bg, 0.35));
    let line = get(&["lighter_background", "lighter_bg"]).filter(|l| *l != bg).unwrap_or_else(|| mix(&bg, &fg, 0.12));
    let surface = get(&["dark_background", "dark_bg"]).unwrap_or_else(|| mix(&bg, "#000000", if light { 0.06 } else { 0.25 }));
    let selection = get(&["selection", "selection_background"]).filter(|s| *s != bg).unwrap_or_else(|| mix(&bg, &accent, 0.25));
    let danger = get(&["red", "color1"]).unwrap_or_else(|| "#e05555".into());
    // текст на залитых акцентом кнопках: фон темы (так у Omarchy), если читается, иначе чёрный или белый
    let on_accent = if contrast(&accent, &bg) >= 3.0 {
        bg.clone()
    } else if contrast(&accent, "#000000") >= contrast(&accent, "#ffffff") {
        "#000000".into()
    } else {
        "#ffffff".into()
    };
    let accent2 = mix(&accent, &bg, 0.3);
    Some(Palette { light, bg, fg, heading, muted, accent, accent2, on_accent, line, surface, selection, danger })
}

// ───────────────────────── тема ─────────────────────────

/// Разметка — как у встроенной Osaka Jade (themes/osaka-jade/theme.wmt): плитки с рамками lighter_background
/// и скруглёнными углами (просьба владельца, 2026-11-09: без строгих прямых), без свечения. Значения `{{…}}` подставляет `css`.
const CSS: &str = r#"
:root {
  --heading: {{heading}};
  --selection: {{selection}};
  --line: {{line}};
  --surface: {{surface}};
  --hover: {{hover}};
  --danger: {{danger}};
}
.panel { border-radius: 14px; border: 2px solid var(--line); box-shadow: none; background: {{panel}}; }
.player.panel { border-color: {{accent}}; }
.titlebar { background: {{titlebar}}; border-bottom: 1px solid var(--line); }
.tbdot { border-radius: 3px; background: var(--accent); box-shadow: none; }
.tbbrand { color: var(--accent); letter-spacing: .14em; text-transform: lowercase; }

.head h2 { color: var(--heading); text-shadow: none; letter-spacing: .03em; }
.side .nav { border-radius: 10px; background: transparent; }
.side .nav:hover:not(.active) { background: var(--hover); }
.side .nav.active { background: var(--selection); color: var(--heading); text-shadow: none; box-shadow: inset 3px 0 0 var(--accent); }

.search, .field { border-radius: 10px; background: {{field}}; border: 1px solid var(--line); color: var(--text); }
.search:focus, .field:focus { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }

.list li { border-radius: 10px; }
.list li:hover { background: var(--hover); }
.list li.playing { background: {{playing}}; }
.list li.playing .t { color: var(--accent); text-shadow: none; }
.list img, .cover, .simcard img, .lastAlbum img, .fcover { border-radius: 8px; }

.card, .lastAlbum, .simcard, .stylecard, .chip, .nav, .tmenu, .toast, .login-box, .qpanel, .volpop { border-radius: 12px; }
.card, .lastAlbum, .tmenu, .toast, .volpop { background: {{card}}; border-color: var(--line); }
.nav, .chip { background: {{card}}; border: 1px solid var(--line); color: var(--text); }
.nav:hover:not(:disabled):not(.active) { border-color: var(--accent); }
.nav.active, .chip.active, .player .playbtn, .wavebtn, .fbtns .playbtn { background: var(--accent); color: {{on_accent}}; box-shadow: none; text-shadow: none; }
.player .playbtn:hover, .nav.active:hover { background: {{accent_hi}}; color: {{on_accent}}; filter: none; }
.player button { border-radius: 10px; }
.player .playbtn { border-radius: 50%; }
.wavebtn { border-radius: 16px; }
/* кнопки режимов — как кнопка очереди (.qtoggle ядра): выключенная — цвета текста, включённая — акцент со свечением;
   с --muted основы выключенная почти не отличалась от включённой */
.modebtn { color: inherit; }
.modebtn.on { color: var(--accent); }
.likebtn.on { color: var(--accent); filter: none; }

input[type=range]::-webkit-slider-runnable-track { background: linear-gradient(90deg, var(--accent2), var(--accent) var(--p, 0%), var(--line) var(--p, 0%)); border-radius: 999px; }
input[type=range]::-webkit-slider-thumb { border-radius: 4px; box-shadow: none; background: var(--heading); }
/* кружок на полосе трека VK Music (свой элемент, не родной кружок ползунка) */
.seekdot { border-radius: 4px; box-shadow: none; background: var(--heading); }
::selection { background: var(--selection); color: var(--heading); }
"#;

/// Светлая тема: тени и свечение текста основы «Неон» рассчитаны на тёмный фон — на светлом буквы расплываются.
const LIGHT_CSS: &str = r#"
body, body * { text-shadow: none !important; }
"#;

fn css(p: &Palette) -> String {
    let vars = [
        ("heading", p.heading.clone()),
        ("selection", p.selection.clone()),
        ("line", p.line.clone()),
        ("surface", p.surface.clone()),
        ("hover", rgba(&p.accent, if p.light { 0.12 } else { 0.16 })),
        ("danger", p.danger.clone()),
        // непрозрачность панелей — настройка «Фон и панели» (переменная основы)
        ("panel", rgba(&p.surface, 1.0).replace(", 1)", ", var(--panel-a))")),
        ("accent", p.accent.clone()),
        ("titlebar", rgba(&p.surface, 0.9)),
        ("field", rgba(&p.surface, 0.8)),
        ("playing", rgba(&p.selection, 0.55)),
        ("card", rgba(&p.surface, if p.light { 0.9 } else { 0.8 })),
        ("on_accent", p.on_accent.clone()),
        ("accent_hi", mix(&p.accent, &p.heading, 0.2)),
    ];
    let mut out = CSS.to_string();
    if p.light {
        out.push_str(LIGHT_CSS);
    }
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), &v);
    }
    out
}

/// Короткий хэш для имён файлов: обои под новым именем — WebView не покажет прежние из своего кэша.
fn short_hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

/// Картинка темы по ссылке/пути: (имя в теме, байты), если это известный формат и не больше предела.
fn asset(path: &Path, stem: &str, tag: &str, video_ok: bool, max: u64) -> Option<(String, Vec<u8>)> {
    let real = std::fs::canonicalize(path).ok()?;
    let ext = wmt::ext_of(real.file_name()?.to_str()?);
    let video = wmt::VIDEO_EXT.contains(&ext.as_str());
    if !(wmt::IMAGE_EXT.contains(&ext.as_str()) || video && video_ok) {
        return None;
    }
    let max = if video { max.min(wmt::MAX_VIDEO as u64) } else { max };
    if std::fs::metadata(&real).ok()?.len() > max {
        return None;
    }
    Some((format!("{stem}-{tag}.{ext}"), std::fs::read(&real).ok()?))
}

/// Текст темы .wmt из палитры (без файлов: обои и превью кладёт `build` в `Theme.files`).
fn wmt_text(name: &str, p: &Palette, background: Option<&str>, preview: Option<&str>) -> String {
    let mut manifest = serde_json::json!({
        "id": ID,
        "name": "Omarchy",
        "author": "Omarchy",
        "description": format!("Сейчас: {name}. Цвета и обои меняются вместе с темой системы"),
        "version": "1.0",
        "base": "neon",
        "apps": ["whisper-music"],
    });
    if let Some(pv) = preview {
        manifest["preview"] = format!("asset:{pv}").into();
    }
    // как у встроенных тем из Omarchy (и шрифт Omarchy по умолчанию); свой шрифт пользователя всё равно главнее
    let family = "\"JetBrainsMono Nerd Font\", \"JetBrains Mono\", \"Fira Code\", monospace";
    let mut tokens = serde_json::json!({
        "color": { "accent": p.accent, "accent2": p.accent2, "text": p.fg, "muted": p.muted, "background": p.bg },
        "effects": { "glow": 0, "panelOpacity": if p.light { 0.88 } else { 0.8 }, "dim": if p.light { 0.08 } else { 0.28 }, "backgroundBlur": 0 },
        "font": { "body": family, "display": family },
    });
    if let Some(bg) = background {
        // фон под своим именем (не background.*) — у Theme::background это «tokens.background.image»
        tokens["background"] = serde_json::json!({ "image": format!("asset:{bg}") });
    }
    let pretty = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default();
    format!(
        "#WMT 1\n# Собрано VK Music из текущей темы системы «{}» (стиль «Omarchy»).\n\n@@ manifest\n{}\n\n@@ tokens\n{}\n\n@@ settings whisper-music\n{{ \"features\": {{ \"glow\": false }} }}\n\n@@ css whisper-music\n{}\n",
        name.replace('\n', " "),
        pretty(&manifest),
        pretty(&tokens),
        css(p).trim()
    )
}

/// Собранная тема: отпечаток, из которого она собрана, сама тема и её текст (для `theme_file`).
pub struct Built {
    sig: String,
    pub theme: Arc<Theme>,
    pub raw: Arc<Vec<u8>>,
}

static CACHE: Mutex<Option<Arc<Built>>> = Mutex::new(None);

fn build(p: &Paths, sig: String) -> Result<Built, String> {
    let read = |f: &str| std::fs::read_to_string(p.dir.join(f)).ok();
    let raw = if let Some(t) = read("colors.toml") {
        parse_colors(&t)
    } else if let Some(t) = read("alacritty.toml") {
        parse_alacritty(&t)
    } else {
        parse_json(&read("quickshell.json").unwrap_or_default())
    };
    let pal = palette(&raw, p.dir.join("light.mode").exists()).ok_or("В теме нет цветов фона и текста")?;
    let tag = short_hash(&sig);
    let bg = p.background.as_deref().and_then(|b| asset(b, "background", &tag, true, wmt::MAX_FILE as u64));
    let preview = asset(&p.dir.join("preview.png"), "preview", &tag, false, MAX_PREVIEW);
    let name = pretty_name(&p.name);
    let text = wmt_text(&name, &pal, bg.as_ref().map(|b| b.0.as_str()), preview.as_ref().map(|b| b.0.as_str()));
    let mut theme = wmt::parse_text(&text).map_err(|e| format!("тема Omarchy: {e}"))?;
    theme.files.extend(bg.into_iter().chain(preview));
    Ok(Built { sig, theme: Arc::new(theme), raw: Arc::new(text.into_bytes()) })
}

/// Тема из текущей темы Omarchy (собирается заново, только если та поменялась).
pub fn current() -> Result<Arc<Built>, String> {
    let p = locate().ok_or("Тема Omarchy не найдена")?;
    let sig = signature(&p).ok_or("Тема Omarchy сейчас недоступна")?;
    let mut cache = CACHE.lock().unwrap();
    if let Some(b) = cache.as_ref().filter(|b| b.sig == sig) {
        return Ok(b.clone());
    }
    let b = Arc::new(build(&p, sig)?);
    *cache = Some(b.clone());
    Ok(b)
}

/// Следить за темой Omarchy, пока выбран стиль `omarchy` (см. описание модуля).
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        let (mut shown, mut seen): (Option<String>, Option<String>) = (None, None);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let on = app.state::<crate::AppState>().cfg.lock().unwrap().theme == ID;
            let sig = if on { locate().and_then(|p| signature(&p)) } else { None };
            if !on {
                (shown, seen) = (None, None);
                continue;
            }
            if sig.is_none() {
                continue; // посреди смены темы — ждём, пока появится новая
            }
            if shown.is_none() {
                shown = sig; // стиль только что включили: интерфейс уже показывает эту тему
                continue;
            }
            if sig != shown && sig == seen {
                shown = sig.clone();
                let _ = app.emit(EVENT, ());
            }
            seen = sig;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKYO: &str = r##"mode = "dark"

accent = "#7aa2f7"
selection = "#292e42"
muted = "#414868"

background = "#1a1b26"
dark_background = "#13141c"
lighter_background = "#24283b"

foreground = "#a9b1d6"
dark_foreground = "#565f89"
bright_foreground = "#c0caf5"
red = "#f7768e" # комментарий
"##;

    #[test]
    fn colors_toml() {
        let p = palette(&parse_colors(TOKYO), false).unwrap();
        assert!(!p.light);
        assert_eq!((p.bg.as_str(), p.fg.as_str(), p.accent.as_str()), ("#1a1b26", "#a9b1d6", "#7aa2f7"));
        assert_eq!((p.heading.as_str(), p.line.as_str(), p.danger.as_str()), ("#c0caf5", "#24283b", "#f7768e"));
        // dark_foreground у Tokyo Night на фоне не читается (~2.8:1) — второстепенный текст смешан из текста и фона
        assert_ne!(p.muted, "#565f89");
        assert!(contrast(&p.muted, &p.bg) >= 3.0);
        assert_eq!(p.on_accent, "#1a1b26");
    }

    #[test]
    fn light_and_legacy() {
        let latte = parse_colors("accent = \"#1e66f5\"\nbackground = \"#eff1f5\"\nforeground = \"#4c4f69\"\n");
        assert!(palette(&latte, false).unwrap().light, "светлая — по яркости фона");
        let dark_named = parse_colors("mode = 'dark'\nbackground = '#eeeeee'\nforeground = '#111111'\n");
        assert!(!palette(&dark_named, false).unwrap().light, "mode главнее яркости");
        // старая тема: только alacritty.toml
        let a = parse_alacritty(
            "[colors.primary]\nbackground = '#121212'\nforeground = \"0xBEBEBE\"\n[colors.normal]\nblack = '#333333'\nred = '#D35F5F'\nblue = '#8a8a8d'\n[colors.bright]\nwhite = '#ffffff'\n",
        );
        let p = palette(&a, true).unwrap();
        assert!(p.light, "light.mode рядом с темой");
        assert_eq!((p.bg.as_str(), p.fg.as_str(), p.accent.as_str(), p.heading.as_str()), ("#121212", "#bebebe", "#8a8a8d", "#ffffff"));
        assert_eq!(p.danger, "#d35f5f");
        assert!(palette(&parse_colors("accent = \"#ffffff\"\n"), false).is_none(), "без фона и текста темы нет");
    }

    #[test]
    fn quickshell_json() {
        let m = parse_json(r##"{ "theme": "bobrik", "foreground": "#F5F2F7", "background": "#1A191B", "color1": "#706E7E",
            "color4": "#8D8293", "color8": "#A2A0A5", "text": "#FFFFFF", "muted": "#888888" }"##);
        let p = palette(&m, false).unwrap();
        assert_eq!((p.bg.as_str(), p.fg.as_str(), p.accent.as_str(), p.muted.as_str()), ("#1a191b", "#f5f2f7", "#706e7e", "#888888"));
        assert_eq!(plain_name("bobrik"), Some("bobrik"));
        assert!(plain_name("../x").is_none() && plain_name(".hidden").is_none() && plain_name("a/b").is_none());
    }

    #[test]
    fn values_are_checked() {
        let m = parse_colors("background = \"#000000; } body { x\"\nforeground = \"url(x)\"\n");
        assert!(palette(&m, false).is_none());
        assert_eq!(hex("#ABCDEF").as_deref(), Some("#abcdef"));
        assert_eq!(hex("#abc"), None);
    }

    #[test]
    fn theme_text_parses() {
        let p = palette(&parse_colors(TOKYO), false).unwrap();
        let text = wmt_text("Tokyo Night", &p, Some("background-1.png"), None);
        let mut t = wmt::parse_text(&text).unwrap();
        assert_eq!(t.id(), ID);
        assert!(!t.css(crate::themes::WHISPER_ID).unwrap().contains("{{"), "все значения подставлены");
        assert_eq!(t.tokens.pointer("/color/accent").and_then(|v| v.as_str()), Some("#7aa2f7"));
        assert!(t.background().is_none(), "фона нет, пока нет файла");
        t.files.insert("background-1.png".into(), vec![1]);
        assert_eq!(t.background(), Some(("background-1.png".into(), false)));
    }

    /// Текущая тема системы на этой машине: `cargo test live_omarchy -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_omarchy() {
        let p = locate().expect("тема не найдена");
        println!("папка: {}\nимя: {}\nобои: {:?}", p.dir.display(), p.name, p.background);
        let b = current().unwrap();
        println!("файлы: {:?}\n{}", b.theme.files.keys().collect::<Vec<_>>(), String::from_utf8_lossy(&b.raw));
    }
}
