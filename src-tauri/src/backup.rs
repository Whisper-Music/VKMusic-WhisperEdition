//! Резервные копии (как backup.rs / backup.js у Whisper Yandex): один файл `.vkmbak` со всем, что пользователь
//! настроил и собрал, — настройки, библиотека сервисов, порядок, плейлисты, статистика, свои темы и настройки
//! страницы (localStorage: история очереди, цвета, раскладка, модули ядра). Не входят: ключ `--host`, кэши,
//! куки входа (вход — заново) и ключ архива (он в связке ключей).
//!
//! * «Сохранить копию» — файл в папку загрузок; восстановить — файлом (перетащить в окно или выбрать).
//! * Автокопии — раз в день при запуске, в `~/.config/vkmusic/backups/` (последние `AUTO_KEEP`): на случай, если
//!   настройки повредятся (lsguard.js замечает это до того, как модули запишут поверх значения по умолчанию).
//! * Восстановление идёт через перезапуск: файлы — в `restore-pending.json`, их раскладывает `apply_pending` при
//!   следующем запуске, до загрузки настроек (иначе работающее приложение записало бы свои данные поверх).
//!   Перед этим текущее состояние сохраняется автокопией «before-restore».
//!
//! Файл — JSON: { format, v, created, version, storage: {ключ: строка}, files: {имя: текст JSON},
//! themes: {имя файла: base64} }. Всё из файла проверяется: имена — только из списка, JSON — разбирается,
//! темы — `wmt::open`, размеры ограничены.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FORMAT: &str = "vkmusic-backup";
const V: u32 = 1;
pub const EXT: &str = "vkmbak";
/// файлы настроек в копии (все — JSON в ~/.config/vkmusic)
const FILES: [&str; 5] = ["config.json", "library.json", "order.json", "playlists.json", "stats.json"];
const MAX_TOTAL: usize = crate::wmt::MAX_FILE;
const MAX_STORAGE_KEY: usize = 200;
const AUTO_KEEP: usize = 5;
const PENDING: &str = "restore-pending.json";

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Backup {
    format: String,
    v: u32,
    #[serde(default)]
    created: u64,
    #[serde(default)]
    version: String,
    #[serde(default)]
    pub storage: BTreeMap<String, String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    themes: BTreeMap<String, String>,
}

impl Backup {
    /// Когда сделана, какой версией, сколько файлов настроек и тем.
    pub fn summary(&self) -> (u64, String, usize, usize) {
        (self.created, self.version.clone(), self.files.len(), self.themes.keys().filter(|n| n.ends_with(".wmt")).count())
    }
}

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub created: u64,
    pub size: u64,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Дата (UTC) по unix-времени: «2026-11-06» — для имён файлов.
pub fn ymd(t: u64) -> String {
    // алгоритм days_from_civil наоборот (Howard Hinnant)
    let z = (t / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn dir() -> PathBuf {
    crate::config::config_dir()
}

fn backups_dir() -> PathBuf {
    dir().join("backups")
}

/// Имя файла темы в копии: как в папке тем — `<id>.wmt` или превью `<id>.preview.<ext>`.
fn theme_name_ok(n: &str) -> bool {
    let (stem, ext) = match n.rsplit_once('.') {
        Some(x) => x,
        None => return false,
    };
    let id = match ext {
        "wmt" => stem,
        "jpg" | "jpeg" | "png" | "webp" => match stem.strip_suffix(".preview") {
            Some(id) => id,
            None => return false,
        },
        _ => return false,
    };
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Собрать копию: настройки страницы — от интерфейса, остальное — с диска.
pub fn collect(storage: BTreeMap<String, String>) -> Backup {
    let base = dir();
    let mut files = BTreeMap::new();
    for f in FILES {
        if let Ok(text) = std::fs::read_to_string(base.join(f)) {
            files.insert(f.to_string(), text);
        }
    }
    let mut themes = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(base.join("themes")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if theme_name_ok(&name) {
                if let Ok(b) = std::fs::read(e.path()) {
                    themes.insert(name, base64::engine::general_purpose::STANDARD.encode(b));
                }
            }
        }
    }
    let storage = storage.into_iter().filter(|(k, _)| storage_key_ok(k)).collect();
    Backup { format: FORMAT.into(), v: V, created: now(), version: env!("CARGO_PKG_VERSION").into(), storage, files, themes }
}

/// Ключ localStorage, который входит в копию: снимок стиля и индекс архива пересобираются сами.
pub fn storage_key_ok(k: &str) -> bool {
    !k.is_empty() && k.len() <= MAX_STORAGE_KEY && !["vkm_boot", "wa_index_v3"].contains(&k)
}

fn to_bytes(b: &Backup) -> Result<Vec<u8>, String> {
    let out = serde_json::to_vec(b).map_err(|e| e.to_string())?;
    if out.len() > MAX_TOTAL {
        return Err("Копия больше 64 МБ — слишком много своих тем".into());
    }
    Ok(out)
}

/// Разобрать и проверить файл копии.
pub fn parse(bytes: &[u8]) -> Result<Backup, String> {
    if bytes.len() > MAX_TOTAL {
        return Err("Файл копии больше 64 МБ".into());
    }
    let b: Backup = serde_json::from_slice(bytes).map_err(|_| "Это не резервная копия VK Music".to_string())?;
    if b.format != FORMAT {
        return Err("Это не резервная копия VK Music".into());
    }
    if b.v > V {
        return Err("Копия сделана более новой версией VK Music — обновите приложение".into());
    }
    for (name, text) in &b.files {
        if !FILES.contains(&name.as_str()) {
            return Err(format!("В копии лишний файл: {name}"));
        }
        serde_json::from_str::<serde_json::Value>(text).map_err(|_| format!("В копии повреждён {name}"))?;
    }
    if let Some(c) = b.files.get("config.json") {
        serde_json::from_str::<crate::config::Config>(c).map_err(|_| "В копии повреждён config.json".to_string())?;
    }
    for (name, data) in &b.themes {
        if !theme_name_ok(name) {
            return Err(format!("В копии неверное имя темы: {name}"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| format!("В копии повреждена тема {name}"))?;
        if name.ends_with(".wmt") {
            crate::wmt::open(&bytes).map_err(|e| format!("Тема {name} в копии не открылась: {e}"))?;
        }
    }
    if b.storage.keys().any(|k| !storage_key_ok(k)) {
        return Err("В копии неверные настройки страницы".into());
    }
    Ok(b)
}

/// Записать копию в папку `dest` под именем `<stem>-<дата>.vkmbak` (не затирая существующие).
pub fn write_to(dest: &Path, stem: &str, b: &Backup) -> Result<PathBuf, String> {
    let bytes = to_bytes(b)?;
    std::fs::create_dir_all(dest).map_err(|e| format!("Не удалось создать {}: {e}", dest.display()))?;
    let out = crate::cache::free_path(dest, &format!("{stem}-{}", ymd(b.created)), EXT);
    std::fs::write(&out, bytes).map_err(|e| format!("Не удалось сохранить копию: {e}"))?;
    Ok(out)
}

/// Автокопия: не чаще раза в день (есть сегодняшняя — ничего не делать), хранятся последние AUTO_KEEP.
pub fn auto(storage: BTreeMap<String, String>) -> Result<Option<PathBuf>, String> {
    let today = ymd(now());
    let d = backups_dir();
    if list().iter().any(|e| e.name.starts_with("auto-") && e.name.contains(&today)) {
        return Ok(None);
    }
    let out = write_to(&d, "auto", &collect(storage))?;
    prune(&d);
    Ok(Some(out))
}

fn prune(d: &Path) {
    let mut autos: Vec<_> = list().into_iter().filter(|e| e.name.starts_with("auto-")).collect();
    autos.sort_by_key(|e| std::cmp::Reverse(e.created));
    for e in autos.into_iter().skip(AUTO_KEEP) {
        let _ = std::fs::remove_file(d.join(e.name));
    }
}

/// Автокопии и копии перед восстановлением — новые первыми.
pub fn list() -> Vec<Entry> {
    let mut out: Vec<Entry> = std::fs::read_dir(backups_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(&format!(".{EXT}")) {
                return None;
            }
            let m = e.metadata().ok()?;
            let created = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some(Entry { name, created, size: m.len() })
        })
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.created));
    out
}

/// Автокопия по имени из `list` (имя — только файл в папке копий).
pub fn read_auto(name: &str) -> Result<Vec<u8>, String> {
    if name.contains(['/', '\\']) || name.starts_with('.') || !name.ends_with(&format!(".{EXT}")) {
        return Err("Неверное имя копии".into());
    }
    std::fs::read(backups_dir().join(name)).map_err(|e| format!("Не удалось прочитать копию: {e}"))
}

/// Подготовить восстановление: текущее — в копию «before-restore», файлы копии — в restore-pending.json.
/// Настройки страницы интерфейс записывает сам (их возвращает вызывающему `parse`).
pub fn stage(b: &Backup, current_storage: BTreeMap<String, String>) -> Result<(), String> {
    write_to(&backups_dir(), "before-restore", &collect(current_storage))?;
    let pending = Backup { storage: BTreeMap::new(), files: b.files.clone(), themes: b.themes.clone(), ..Backup::default() };
    let pending = Backup { format: FORMAT.into(), v: V, created: now(), ..pending };
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    std::fs::write(dir().join(PENDING), to_bytes(&pending)?).map_err(|e| format!("Не удалось подготовить восстановление: {e}"))
}

/// При запуске, до загрузки настроек: разложить восстановленные файлы (если восстановление подготовлено).
pub fn apply_pending() {
    apply_pending_in(&dir());
}

fn apply_pending_in(base: &Path) {
    let path = base.join(PENDING);
    let Ok(bytes) = std::fs::read(&path) else { return };
    let _ = std::fs::remove_file(&path);
    let b = match parse(&bytes) {
        Ok(b) => b,
        Err(e) => return eprintln!("vkmusic: восстановление из копии: {e}"),
    };
    for (name, text) in &b.files {
        let tmp = base.join(format!("{name}.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, base.join(name));
        }
    }
    let themes = base.join("themes");
    let _ = std::fs::create_dir_all(&themes);
    for (name, data) in &b.themes {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
            let _ = std::fs::write(themes.join(name), bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(1_793_957_335), "2026-11-06");
        assert_eq!(ymd(951_782_400), "2000-02-29");
    }

    #[test]
    fn theme_names() {
        assert!(theme_name_ok("chalk.wmt"));
        assert!(theme_name_ok("retro-tv.preview.jpg"));
        assert!(!theme_name_ok("../config.wmt"));
        assert!(!theme_name_ok("x.preview.wmt.exe"));
        assert!(!theme_name_ok("Chalk.wmt"));
        assert!(!theme_name_ok("chalk.jpg"), "превью — только <id>.preview.<ext>");
    }

    #[test]
    fn parse_checks_everything() {
        let mut b = Backup { format: FORMAT.into(), v: V, ..Backup::default() };
        b.files.insert("config.json".into(), r#"{"volume": 20}"#.into());
        b.storage.insert("vkm_layout".into(), "{}".into());
        let ok = serde_json::to_vec(&b).unwrap();
        assert_eq!(parse(&ok).unwrap().storage.len(), 1);

        let mut bad = Backup { format: FORMAT.into(), v: V, ..Backup::default() };
        bad.files.insert("../../.bashrc".into(), "{}".into());
        assert!(parse(&serde_json::to_vec(&bad).unwrap()).unwrap_err().contains("лишний файл"));

        let mut bad = Backup { format: FORMAT.into(), v: V, ..Backup::default() };
        bad.files.insert("stats.json".into(), "{не json".into());
        assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err());

        let mut bad = Backup { format: FORMAT.into(), v: V, ..Backup::default() };
        bad.themes.insert("x.wmt".into(), base64::engine::general_purpose::STANDARD.encode(b"not a theme"));
        assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err());

        assert!(parse(br#"{"format":"other","v":1}"#).is_err());
        assert!(parse(br#"{"format":"vkmusic-backup","v":99}"#).unwrap_err().contains("новой версией"));
        assert!(parse(b"garbage").is_err());
    }

    #[test]
    fn pending_is_applied_once() {
        let base = std::env::temp_dir().join(format!("vkm-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("order.json"), "[\"old\"]").unwrap();
        let mut b = Backup { format: FORMAT.into(), v: V, ..Backup::default() };
        b.files.insert("order.json".into(), "[\"new\"]".into());
        std::fs::write(base.join(PENDING), serde_json::to_vec(&b).unwrap()).unwrap();
        apply_pending_in(&base);
        assert_eq!(std::fs::read_to_string(base.join("order.json")).unwrap(), "[\"new\"]");
        assert!(!base.join(PENDING).exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
