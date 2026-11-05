//! Настройки приложения: ~/.config/vkmusic/config.json.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use crate::os::Private;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// выбранная тема (manifest.id)
    pub theme: String,
    /// громкость 0…100
    pub volume: f64,
    /// куда yt-dlp складывает треки; по умолчанию ~/Music/vk
    pub music_dir: Option<String>,
    /// ручные правки поверх темы: dim, blur, panel (затемнение, размытие, прозрачность панелей)
    pub overrides: HashMap<String, f64>,
    pub shuffle: bool,
    /// "off" | "all" | "one"
    pub repeat: String,
    /// «постоянный уровень громкости» (выравнивание в mpv)
    pub normalize: bool,
    /// шрифт интерфейса (семейство из системы); None — как в теме
    pub font: Option<String>,
    /// сервисы, в которых ищет общая строка поиска: "vk", "sc"
    pub search: Vec<String>,
    /// скрытые категории библиотеки тем (папки whisper-themes: "artists" и т. п.)
    pub hidden_theme_categories: Vec<String>,
    /// язык интерфейса: "ru" | "en"
    pub lang: String,
    /// в тексте прятать строки частей песни: [Куплет], [Припев]…
    pub lyrics_hide_parts: bool,
    /// в тексте убирать пустые строки между частями
    pub lyrics_compact: bool,
    /// куда кнопка «Скачать» кладёт треки; по умолчанию ~/Downloads
    pub download_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: crate::themes::DEFAULT_THEME.to_string(),
            volume: 70.0,
            music_dir: None,
            overrides: HashMap::new(),
            shuffle: false,
            repeat: "off".into(),
            normalize: false,
            font: None,
            search: vec!["vk".into(), "sc".into()],
            hidden_theme_categories: Vec::new(),
            lang: "ru".into(),
            lyrics_hide_parts: false,
            lyrics_compact: false,
            download_dir: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| home().join(".config"))
        .join("vkmusic")
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// В config.json остался токен ВКонтакте от старой версии (вход больше не используется).
pub fn has_legacy_secrets() -> bool {
    std::fs::read_to_string(config_path())
        .map(|s| s.contains("\"token\"") || s.contains("\"user_id\""))
        .unwrap_or(false)
}

impl Config {
    pub fn load() -> Config {
        match std::fs::read(config_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
        let tmp = dir.join("config.json.tmp");
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .private()
                .open(&tmp)
                .map_err(|e| format!("Не удалось записать настройки: {e}"))?;
            f.write_all(&data).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, config_path()).map_err(|e| e.to_string())
    }

    /// Папка локальной музыки: настройка или ~/Music/vk.
    pub fn music_dir(&self) -> PathBuf {
        expand(&self.music_dir).unwrap_or_else(|| home().join("Music").join("vk"))
    }

    /// Папка загрузок: настройка или ~/Downloads.
    pub fn download_dir(&self) -> PathBuf {
        expand(&self.download_dir).unwrap_or_else(|| home().join("Downloads"))
    }
}

/// Путь из настроек: «~/…» — от домашней папки; пустой — None.
fn expand(p: &Option<String>) -> Option<PathBuf> {
    let p = p.as_deref()?.trim();
    if p.is_empty() {
        None
    } else if p == "~" {
        Some(home())
    } else if let Some(rest) = p.strip_prefix("~/") {
        Some(home().join(rest))
    } else {
        Some(PathBuf::from(p))
    }
}

/// Путь папки, введённый в настройках: абсолютный или от «~», без управляющих символов.
pub fn valid_dir(p: &str) -> bool {
    let p = p.trim();
    p.len() <= 1024 && !p.chars().any(char::is_control) && (p.starts_with('/') || p == "~" || p.starts_with("~/"))
}
