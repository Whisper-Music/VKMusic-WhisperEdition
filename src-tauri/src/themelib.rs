//! Библиотека тем: репозиторий https://github.com/Whisper-Music/whisper-themes (ветка main).
//!
//! Скачиваются только три вида файлов и только по фиксированному адресу raw.githubusercontent.com:
//! * `index.json`                — список тем (до 1 МБ);
//! * `themes/<категория>/<id>.wmt` — сама тема (до 64 МБ, как любой файл темы);
//! * `previews/<категория>/<id>.<jpg|png|webp>` — картинка карточки (до 2 МБ; её грузит сам интерфейс).
//! Скачанная тема проходит ту же проверку, что и файл, загруженный вручную.
//!
//! Формат index.json 2 (с 2026-10-29): темы разложены по папкам-категориям (`whisper`, `community`,
//! `artists`…), список категорий — в `categories`, у тем артистов есть `artist`. Формат 1 (без папок:
//! `themes/<id>.wmt`) тоже понимаем.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const BASE: &str = "https://raw.githubusercontent.com/Whisper-Music/whisper-themes/main/";
const MB: usize = 1024 * 1024;
pub const INDEX_FORMAT: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// папка темы в репозитории (формат 2)
    #[serde(default)]
    pub category: Option<String>,
    /// исполнитель — у тем «по артистам»
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub file: String,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Index {
    format: u32,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    themes: Vec<Entry>,
}

/// Каталог библиотеки: категории в порядке репозитория и темы.
#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    pub categories: Vec<Category>,
    pub themes: Vec<Entry>,
}

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect::<String>().trim().to_string()
}

/// Путь внутри репозитория, который разрешено скачивать, и предел размера.
pub fn limit_for(path: &str) -> Option<usize> {
    if path == "index.json" {
        return Some(MB);
    }
    // <папка>/<id>.<ext> (формат 1) или <папка>/<категория>/<id>.<ext> (формат 2)
    let parts: Vec<&str> = path.split('/').collect();
    let (dir, name) = match parts.as_slice() {
        [dir, name] => (*dir, *name),
        [dir, cat, name] if crate::wmt::valid_theme_id(cat) => (*dir, *name),
        _ => return None,
    };
    let (stem, ext) = name.rsplit_once('.')?;
    if !crate::wmt::valid_theme_id(stem) {
        return None;
    }
    match (dir, ext) {
        ("themes", "wmt") => Some(crate::wmt::MAX_FILE),
        ("previews", "jpg" | "jpeg" | "png" | "webp") => Some(2 * MB),
        _ => None,
    }
}

pub fn url_for(path: &str) -> Option<String> {
    limit_for(path).map(|_| format!("{BASE}{path}"))
}

/// Разбор index.json: только записи с правильным id, файлом `themes/[<категория>/]<id>.wmt` и допустимым
/// превью из той же папки. Тексты из каталога чистятся от управляющих символов и обрезаются.
pub fn parse_index(bytes: &[u8]) -> Result<Catalog, String> {
    let idx: Index = serde_json::from_slice(bytes).map_err(|e| format!("index.json: {e}"))?;
    if idx.format > INDEX_FORMAT {
        return Err("Библиотека тем в новом формате — обновите VK Music".into());
    }
    let valid = crate::wmt::valid_theme_id;
    let mut categories: Vec<Category> = Vec::new();
    for c in idx.categories {
        if valid(&c.id) && !categories.iter().any(|x| x.id == c.id) {
            categories.push(Category {
                name: Some(clip(&c.name, 100)).filter(|n| !n.is_empty()).unwrap_or_else(|| c.id.clone()),
                description: c.description.map(|d| clip(&d, 300)).filter(|d| !d.is_empty()),
                id: c.id,
            });
        }
    }
    let themes: Vec<Entry> = idx
        .themes
        .into_iter()
        .filter_map(|mut e| {
            e.category = e.category.filter(|c| valid(c));
            let dir = match &e.category {
                Some(c) => format!("{c}/{}", e.id),
                None => e.id.clone(),
            };
            if !valid(&e.id) || e.file != format!("themes/{dir}.wmt") {
                return None;
            }
            if let Some(p) = &e.preview {
                if !(p.starts_with(&format!("previews/{dir}.")) && limit_for(p).is_some()) {
                    e.preview = None;
                }
            }
            e.artist = e.artist.map(|a| clip(&a, 100)).filter(|a| !a.is_empty());
            Some(e)
        })
        .collect();
    // папки, которых нет в списке категорий, — в конец, с именем по id
    for e in &themes {
        if let Some(c) = &e.category {
            if !categories.iter().any(|x| &x.id == c) {
                categories.push(Category { id: c.clone(), name: c.clone(), description: None });
            }
        }
    }
    Ok(Catalog { categories, themes })
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("vkmusic/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(90))
        .build()
        .expect("http client")
}

/// Скачать разрешённый файл репозитория с проверкой размера.
pub async fn fetch(client: &reqwest::Client, path: &str) -> Result<Vec<u8>, String> {
    let limit = limit_for(path).ok_or_else(|| format!("недопустимый путь «{path}»"))?;
    let url = format!("{BASE}{path}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub ответил {} на {path}", resp.status()));
    }
    if resp.content_length().is_some_and(|n| n as usize > limit) {
        return Err(format!("{path}: файл больше допустимого"));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("Загрузка прервалась: {e}"))?;
    if bytes.len() > limit {
        return Err(format!("{path}: файл больше допустимого"));
    }
    Ok(bytes.to_vec())
}

/// Каталог держим в памяти столько: открыть магазин, установить тему, вернуться — без новых запросов к GitHub.
const INDEX_TTL: Duration = Duration::from_secs(10 * 60);

/// Каталог библиотеки (из памяти, если свежий). fresh — запросить заново («Обновить список»).
pub async fn index(client: &reqwest::Client, fresh: bool) -> Result<Catalog, String> {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Catalog)>> = std::sync::Mutex::new(None);
    if !fresh {
        if let Some((at, c)) = CACHE.lock().unwrap().as_ref() {
            if at.elapsed() < INDEX_TTL {
                return Ok(c.clone());
            }
        }
    }
    let c = parse_index(&fetch(client, "index.json").await?)?;
    *CACHE.lock().unwrap() = Some((std::time::Instant::now(), c.clone()));
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(limit_for("index.json"), Some(MB));
        assert!(limit_for("themes/chalk.wmt").is_some());
        assert!(limit_for("previews/chalk.jpg").is_some());
        assert!(limit_for("themes/whisper/chalk.wmt").is_some());
        assert!(limit_for("previews/artists/madk1d.jpg").is_some());
        for bad in [
            "", "README.md", "themes/../index.json", "themes/Chalk.wmt", "themes/../chalk.wmt",
            "themes/a/b/c.wmt", "themes/A/b.wmt", "previews/chalk.svg", "tools/wmt.js", "/index.json",
            "themes/chalk.wmt?x", "themes//chalk.wmt", "index.json/x.wmt",
        ] {
            assert!(limit_for(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn index_filtering() {
        let v = parse_index(
            r#"{"format":1,"themes":[
              {"id":"chalk","name":"Мел","version":"1.2","file":"themes/chalk.wmt","preview":"previews/chalk.jpg","size":10,"tags":["a"]},
              {"id":"evil","name":"x","file":"themes/other.wmt"},
              {"id":"Bad","name":"x","file":"themes/Bad.wmt"},
              {"id":"np","name":"x","file":"themes/np.wmt","preview":"previews/other.jpg"}
            ]}"#
            .as_bytes(),
        )
        .unwrap();
        let ids = |c: &Catalog| c.themes.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&v), ["chalk", "np"]);
        assert!(v.themes[1].preview.is_none());
        assert!(parse_index(br#"{"format":3,"themes":[]}"#).is_err());
    }

    #[test]
    fn index_format2_folders() {
        let v = parse_index(
            r#"{"format":2,
              "categories":[{"id":"whisper","name":"Whisper Music","description":"Темы автора"},{"id":"Bad","name":"x"},
                            {"id":"artists","name":"По артистам\u0007"}],
              "themes":[
              {"id":"chalk","category":"whisper","name":"Мел","file":"themes/whisper/chalk.wmt","preview":"previews/whisper/chalk.jpg"},
              {"id":"madk1d","category":"artists","artist":" madk1d\u0007 ","name":"sexyswag","file":"themes/artists/madk1d.wmt","preview":"previews/whisper/madk1d.jpg"},
              {"id":"moved","category":"community","name":"x","file":"themes/whisper/moved.wmt"},
              {"id":"flat","name":"x","file":"themes/flat.wmt"},
              {"id":"odd","category":"new-cat","name":"x","file":"themes/new-cat/odd.wmt"}
            ]}"#
            .as_bytes(),
        )
        .unwrap();
        let ids: Vec<&str> = v.themes.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["chalk", "madk1d", "flat", "odd"]);
        assert_eq!(v.themes[0].preview.as_deref(), Some("previews/whisper/chalk.jpg"));
        // превью из чужой папки не берём
        assert!(v.themes[1].preview.is_none());
        assert_eq!(v.themes[1].artist.as_deref(), Some("madk1d"));
        let cats: Vec<(&str, &str)> = v.categories.iter().map(|c| (c.id.as_str(), c.name.as_str())).collect();
        assert_eq!(cats, [("whisper", "Whisper Music"), ("artists", "По артистам"), ("new-cat", "new-cat")]);
    }
}
