//! Сохранённые треки сервисов — вторая половина библиотеки (первая — файлы в папке музыки).
//!
//! ~/.config/vkmusic/library.json. Хранится только описание трека (сервис, id, название, исполнитель,
//! длительность, обложка, страница); звук — в кэше треков (`cache`), пока трек в библиотеке.
//! Два вида: добавленные вручную («+» в поиске, миксе, плейлисте) и синхронизированные из аккаунта (`sync`: «Мои треки» ВК,
//! лайки SoundCloud) — их список целиком заменяется при каждой синхронизации и очищается при выходе.

use serde::{Deserialize, Serialize};
use std::io::Write;
use crate::os::Private;
use std::path::PathBuf;
use std::sync::Mutex;

type Res<T> = Result<T, String>;

const MAX_ITEMS: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    /// "vk" | "sc"
    pub service: String,
    /// vk: owner_id; sc: числовой id
    pub id: String,
    /// vk: id для reload_audio
    #[serde(default)]
    pub reload: Option<String>,
    /// sc: страница трека
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub cover: Option<String>,
    /// когда добавлен (unix, с)
    #[serde(default)]
    pub added: u64,
    /// пришёл синхронизацией из аккаунта сервиса
    #[serde(default)]
    pub sync: bool,
    /// vk: хэш удаления — трек можно удалить из «Моих треков» ВК
    #[serde(default)]
    pub del: Option<String>,
    /// vk: трек из микса или плейлиста (API) — без reload, ссылка через audio.getById
    #[serde(default)]
    pub api: bool,
    /// vk, api: access_key трека для audio.getById (берётся из ответа API в Rust, не из интерфейса)
    #[serde(default)]
    pub access_key: Option<String>,
}

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect::<String>().trim().to_string()
}

fn https(s: &Option<String>, prefix: &str) -> Option<String> {
    s.as_ref()
        .filter(|u| u.starts_with(prefix) && u.len() < 2048 && !u.chars().any(char::is_whitespace))
        .cloned()
}

impl Saved {
    /// Данные приходят из интерфейса — проверяем и нормализуем.
    pub fn validate(mut self) -> Res<Saved> {
        match self.service.as_str() {
            "vk" => {
                let mut p = self.id.splitn(2, '_');
                let ok = p.next().is_some_and(|o| o.trim_start_matches('-').parse::<u64>().is_ok())
                    && p.next().is_some_and(|i| i.parse::<u64>().is_ok());
                if !ok {
                    return Err("Неверный id трека ВК".into());
                }
                self.reload = self.reload.filter(|r| crate::vk::valid_reload_id(r));
                self.del = self.del.filter(|h| crate::vk::valid_hash(h));
                self.access_key = self.access_key.filter(|k| crate::vk::valid_hash(k));
                if self.reload.is_some() {
                    self.api = false;
                    self.access_key = None;
                } else if !self.api {
                    return Err("Этот трек ВК недоступен для прослушивания".into());
                }
                self.url = None;
            }
            "sc" => {
                if self.id.is_empty() || self.id.len() > 20 || !self.id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("Неверный id трека SoundCloud".into());
                }
                self.reload = None;
                self.del = None;
                self.api = false;
                self.access_key = None;
                self.url = https(&self.url, "https://soundcloud.com/");
            }
            _ => return Err("Неизвестный сервис".into()),
        }
        self.artist = clip(&self.artist, 300);
        self.title = clip(&self.title, 300);
        self.duration = if self.duration.is_finite() { self.duration.clamp(0.0, 86_400.0) } else { 0.0 };
        self.cover = https(&self.cover, "https://");
        Ok(self)
    }

    fn key(&self) -> (&str, &str) {
        (&self.service, &self.id)
    }
}

pub struct SavedStore {
    items: Mutex<Vec<Saved>>,
    path: PathBuf,
}

impl SavedStore {
    pub fn new() -> SavedStore {
        let path = crate::config::config_dir().join("library.json");
        let items = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<Saved>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.validate().ok())
            .collect();
        SavedStore { items: Mutex::new(items), path }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> SavedStore {
        SavedStore { items: Mutex::new(Vec::new()), path }
    }

    pub fn list(&self) -> Vec<Saved> {
        self.items.lock().unwrap().clone()
    }

    pub fn add(&self, s: Saved) -> Res<Saved> {
        let mut s = s.validate()?;
        s.sync = false;
        let mut items = self.items.lock().unwrap();
        if let Some(old) = items.iter_mut().find(|x| x.key() == s.key()) {
            s.added = old.added;
            *old = s.clone();
        } else {
            if items.len() >= MAX_ITEMS {
                return Err("В библиотеке слишком много треков".into());
            }
            s.added = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            items.push(s.clone());
        }
        self.save(&items)?;
        Ok(s)
    }

    /// Заменить синхронизированные треки сервиса новым списком (в порядке сервиса: новые сначала).
    /// Добавленные вручную не трогаем; у уже знакомых треков сохраняется время добавления.
    pub fn replace_synced(&self, service: &str, fresh: Vec<Saved>) -> Res<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut items = self.items.lock().unwrap();
        let old: std::collections::HashMap<String, u64> = items
            .iter()
            .filter(|x| x.service == service && x.sync)
            .map(|x| (x.id.clone(), x.added))
            .collect();
        items.retain(|x| !(x.service == service && x.sync));
        let manual: std::collections::HashSet<String> =
            items.iter().filter(|x| x.service == service).map(|x| x.id.clone()).collect();
        let mut seen = std::collections::HashSet::new();
        for (i, t) in fresh.into_iter().enumerate() {
            if t.service != service || manual.contains(&t.id) || !seen.insert(t.id.clone()) {
                continue;
            }
            let Ok(mut t) = t.validate() else { continue };
            if items.len() >= MAX_ITEMS {
                break;
            }
            // порядок сервиса сохраняем через время добавления: первый — самый новый
            t.added = old.get(&t.id).copied().unwrap_or(now.saturating_sub(i as u64));
            t.sync = true;
            items.push(t);
        }
        self.save(&items)
    }

    /// Выход из сервиса: убрать его синхронизированные треки.
    pub fn clear_synced(&self, service: &str) -> Res<()> {
        let mut items = self.items.lock().unwrap();
        items.retain(|x| !(x.service == service && x.sync));
        self.save(&items)
    }

    pub fn remove(&self, service: &str, id: &str) -> Res<()> {
        let mut items = self.items.lock().unwrap();
        items.retain(|x| x.key() != (service, id));
        self.save(&items)
    }

    fn save(&self, items: &[Saved]) -> Res<()> {
        let dir = self.path.parent().ok_or("Нет папки настроек")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
        let tmp = self.path.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(items).map_err(|e| e.to_string())?;
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .private()
                .open(&tmp)
                .map_err(|e| format!("Не удалось сохранить библиотеку: {e}"))?;
            f.write_all(&data).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(service: &str, id: &str) -> Saved {
        Saved {
            service: service.into(),
            id: id.into(),
            reload: Some("-1_2_ab_cd".into()),
            url: Some("https://soundcloud.com/a/b".into()),
            artist: " A\u{7}rtist ".into(),
            title: "T".into(),
            duration: f64::NAN,
            cover: Some("http://insecure/x.jpg".into()),
            added: 0,
            sync: false,
            del: Some("bad hash!".into()),
            api: false,
            access_key: None,
        }
    }

    #[test]
    fn sync_replaces_only_synced() {
        let dir = std::env::temp_dir().join(format!("vkmusic-saved-test-{}", std::process::id()));
        let store = SavedStore::at(dir.join("library.json"));
        store.add(s("sc", "1")).unwrap(); // вручную
        store.replace_synced("sc", vec![s("sc", "1"), s("sc", "2"), s("sc", "3"), s("sc", "2")]).unwrap();
        let ids = |st: &SavedStore| {
            let mut v: Vec<(String, bool)> = st.list().into_iter().map(|x| (x.id, x.sync)).collect();
            v.sort();
            v
        };
        // ручной «1» не задвоился, «2» — один раз
        assert_eq!(ids(&store), [("1".into(), false), ("2".into(), true), ("3".into(), true)]);
        // порядок сервиса: «2» новее «3»
        let l = store.list();
        let added = |id: &str| l.iter().find(|x| x.id == id).unwrap().added;
        assert!(added("2") > added("3"));
        // новая синхронизация заменяет список; выход — убирает синхронизированные
        store.replace_synced("sc", vec![s("sc", "4")]).unwrap();
        assert_eq!(ids(&store), [("1".into(), false), ("4".into(), true)]);
        store.clear_synced("sc").unwrap();
        assert_eq!(ids(&store), [("1".into(), false)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validated() {
        let v = s("vk", "-1_2").validate().unwrap();
        assert_eq!(v.artist, "Artist");
        assert_eq!(v.duration, 0.0);
        assert_eq!(v.cover, None);
        assert_eq!(v.url, None);
        assert_eq!(v.del, None); // недопустимый хэш отброшен
        let c = s("sc", "123").validate().unwrap();
        assert_eq!(c.reload, None);
        assert_eq!(c.url.as_deref(), Some("https://soundcloud.com/a/b"));
        assert!(s("vk", "abc").validate().is_err());
        assert!(s("sc", "1_2").validate().is_err());
        assert!(s("yt", "1").validate().is_err());
        let mut no_reload = s("vk", "1_2");
        no_reload.reload = None;
        assert!(no_reload.validate().is_err());
        // трек из микса/плейлиста (API): без reload, с access_key
        let mut api = s("vk", "1_2");
        api.reload = None;
        api.api = true;
        api.access_key = Some("abc123".into());
        let a = api.clone().validate().unwrap();
        assert!(a.api && a.reload.is_none() && a.access_key.as_deref() == Some("abc123"));
        api.access_key = Some("bad key".into());
        assert_eq!(api.validate().unwrap().access_key, None);
        // есть reload — обычный трек, флаг api снимается
        let mut both = s("vk", "1_2");
        both.api = true;
        both.access_key = Some("abc".into());
        let b = both.validate().unwrap();
        assert!(!b.api && b.access_key.is_none());
    }
}
