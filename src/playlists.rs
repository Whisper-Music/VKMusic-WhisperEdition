//! Ваши плейлисты ВК на диске (просьба владельца, 2026-11-02): ~/.config/vkmusic/playlists.json — список
//! плейлистов и треки открывавшихся своих плейлистов. Интерфейс показывает их сразу, без сети, а ответы ВК
//! только обновляют сохранённое. access_key плейлистов и треков лежат здесь же (как у треков в library.json),
//! при запуске возвращаются в `Vk::keys` и интерфейсу не отдаются. Выход из ВК удаляет файл.

use crate::vk::{ApiPlaylist, Track};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

type Res<T> = Result<T, String>;

/// столько треков одного плейлиста держим на диске
const MAX_TRACKS: usize = 10_000;
const MAX_PLAYLISTS: usize = 1_000;

/// Сохранённые треки плейлиста: что уже загружено и с какого места продолжать («Показать ещё»).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Stored {
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub next: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Data {
    /// None — список ещё ни разу не загружали
    #[serde(default)]
    list: Option<Vec<ApiPlaylist>>,
    /// «owner_id_id» → треки
    #[serde(default)]
    tracks: HashMap<String, Stored>,
    /// «pl:owner_id_id» (плейлист) и «owner_id_id» (трек) → access_key
    #[serde(default)]
    keys: HashMap<String, String>,
}

pub struct PlaylistStore {
    path: PathBuf,
    data: Mutex<Data>,
}

fn pl_key(owner: i64, id: i64) -> String {
    format!("{owner}_{id}")
}

impl PlaylistStore {
    pub fn new() -> PlaylistStore {
        Self::at(crate::config::config_dir().join("playlists.json"))
    }

    pub fn at(path: PathBuf) -> PlaylistStore {
        let data = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        PlaylistStore { path, data: Mutex::new(data) }
    }

    /// Сохранённый список; None — ещё не загружали (или вышли из ВК).
    pub fn list(&self) -> Option<Vec<ApiPlaylist>> {
        self.data.lock().unwrap().list.clone()
    }

    /// Все сохранённые access_key — в `Vk::keys` при запуске.
    pub fn keys(&self) -> Vec<(String, String)> {
        self.data.lock().unwrap().keys.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Свежий список из ВК. Треки исчезнувших плейлистов и их ключи убираются.
    pub fn set_list(&self, list: &[ApiPlaylist], keys: impl IntoIterator<Item = (String, String)>) {
        let mut d = self.data.lock().unwrap();
        let list: Vec<ApiPlaylist> = list.iter().take(MAX_PLAYLISTS).cloned().collect();
        let alive: std::collections::HashSet<String> = list.iter().map(|p| pl_key(p.owner_id, p.id)).collect();
        d.tracks.retain(|k, _| alive.contains(k));
        d.keys.retain(|k, _| !k.starts_with("pl:"));
        d.keys.extend(keys.into_iter().filter(|(k, _)| k.strip_prefix("pl:").is_some_and(|x| alive.contains(x))));
        d.list = Some(list);
        Self::prune_track_keys(&mut d);
        self.save(&d);
    }

    /// Треки плейлиста с диска (только плейлисты из сохранённого списка).
    pub fn tracks(&self, owner: i64, id: i64) -> Option<Stored> {
        self.data.lock().unwrap().tracks.get(&pl_key(owner, id)).cloned()
    }

    /// Страница треков из ВК: с начала — заменяет сохранённое, дальше — дописывается. Чужие плейлисты
    /// (не из вашего списка) не сохраняются.
    pub fn set_tracks(&self, owner: i64, id: i64, offset: u64, page: &[Track], next: Option<u64>, keys: impl IntoIterator<Item = (String, String)>) {
        let mut d = self.data.lock().unwrap();
        let k = pl_key(owner, id);
        if !d.list.as_ref().is_some_and(|l| l.iter().any(|p| pl_key(p.owner_id, p.id) == k)) {
            return;
        }
        let s = d.tracks.entry(k).or_default();
        if offset == 0 {
            s.tracks.clear();
        }
        let have: std::collections::HashSet<String> = s.tracks.iter().map(|t| t.id.clone()).collect();
        s.tracks.extend(page.iter().filter(|t| !have.contains(&t.id)).cloned());
        s.tracks.truncate(MAX_TRACKS);
        s.next = if s.tracks.len() >= MAX_TRACKS { None } else { next };
        d.keys.extend(keys);
        Self::prune_track_keys(&mut d);
        self.save(&d);
    }

    /// Новый плейлист — первым в списке.
    pub fn added(&self, pl: &ApiPlaylist) {
        let mut d = self.data.lock().unwrap();
        if let Some(l) = d.list.as_mut() {
            l.retain(|p| (p.owner_id, p.id) != (pl.owner_id, pl.id));
            l.insert(0, pl.clone());
            self.save(&d);
        }
    }

    pub fn renamed(&self, owner: i64, id: i64, title: &str) {
        let mut d = self.data.lock().unwrap();
        if let Some(p) = d.list.as_mut().and_then(|l| l.iter_mut().find(|p| (p.owner_id, p.id) == (owner, id))) {
            p.title = title.to_string();
            self.save(&d);
        }
    }

    pub fn removed(&self, owner: i64, id: i64) {
        let mut d = self.data.lock().unwrap();
        if let Some(l) = d.list.as_mut() {
            l.retain(|p| (p.owner_id, p.id) != (owner, id));
        }
        d.tracks.remove(&pl_key(owner, id));
        d.keys.remove(&format!("pl:{}", pl_key(owner, id)));
        Self::prune_track_keys(&mut d);
        self.save(&d);
    }

    /// Трек добавили в плейлист: счётчик +1; сохранённые треки — устарели (перечитаются при открытии).
    pub fn track_added(&self, owner: i64, id: i64) {
        let mut d = self.data.lock().unwrap();
        if let Some(p) = d.list.as_mut().and_then(|l| l.iter_mut().find(|p| (p.owner_id, p.id) == (owner, id))) {
            p.count += 1;
        }
        d.tracks.remove(&pl_key(owner, id));
        Self::prune_track_keys(&mut d);
        self.save(&d);
    }

    pub fn track_removed(&self, owner: i64, id: i64, track: &str) {
        let mut d = self.data.lock().unwrap();
        if let Some(p) = d.list.as_mut().and_then(|l| l.iter_mut().find(|p| (p.owner_id, p.id) == (owner, id))) {
            p.count = p.count.saturating_sub(1);
        }
        if let Some(s) = d.tracks.get_mut(&pl_key(owner, id)) {
            s.tracks.retain(|t| t.id != track);
        }
        Self::prune_track_keys(&mut d);
        self.save(&d);
    }

    /// Выход из ВК: всё забыть, файл удалить.
    pub fn clear(&self) {
        *self.data.lock().unwrap() = Data::default();
        let _ = std::fs::remove_file(&self.path);
    }

    /// Ключи треков — только для треков, которые ещё лежат в каком-нибудь плейлисте.
    fn prune_track_keys(d: &mut Data) {
        let ids: std::collections::HashSet<&str> = d.tracks.values().flat_map(|s| s.tracks.iter().map(|t| t.id.as_str())).collect();
        d.keys.retain(|k, _| k.starts_with("pl:") || ids.contains(k.as_str()));
    }

    fn save(&self, d: &Data) {
        if let Err(e) = self.write(d) {
            eprintln!("vkmusic: плейлисты не сохранены: {e}");
        }
    }

    fn write(&self, d: &Data) -> Res<()> {
        let dir = self.path.parent().ok_or("Нет папки настроек")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
        let tmp = self.path.with_extension("json.tmp");
        let data = serde_json::to_vec(d).map_err(|e| e.to_string())?;
        {
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
            f.write_all(&data).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(owner: i64, id: i64, title: &str) -> ApiPlaylist {
        ApiPlaylist { owner_id: owner, id, title: title.into(), subtitle: String::new(), count: 2, cover: None, mine: true, editable: true }
    }

    fn tr(id: &str) -> Track {
        Track { id: id.into(), reload: None, artist: "A".into(), title: "T".into(), duration: 1.0, cover: None, del: None, api: true }
    }

    #[test]
    fn saved_and_restored() {
        let dir = std::env::temp_dir().join(format!("vkmusic-pl-{}", std::process::id()));
        let path = dir.join("playlists.json");
        let s = PlaylistStore::at(path.clone());
        assert_eq!(s.list(), None);
        // чужой плейлист до списка не сохраняется
        s.set_tracks(5, 1, 0, &[tr("5_1")], None, []);
        assert_eq!(s.tracks(5, 1), None);
        s.set_list(&[pl(9, 1, "Любимое"), pl(9, 2, "Бег")], [("pl:9_1".to_string(), "k1".to_string()), ("pl:7_7".to_string(), "x".to_string())]);
        s.set_tracks(9, 1, 0, &[tr("1_1"), tr("1_2")], Some(200), [("1_1".to_string(), "a".to_string())]);
        s.set_tracks(9, 1, 200, &[tr("1_2"), tr("1_3")], None, []);

        let r = PlaylistStore::at(path.clone());
        assert_eq!(r.list().unwrap().len(), 2);
        let t = r.tracks(9, 1).unwrap();
        assert_eq!(t.tracks.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1_1", "1_2", "1_3"]);
        assert_eq!(t.next, None);
        let mut keys = r.keys();
        keys.sort();
        // ключ плейлиста не из списка не сохраняется
        assert_eq!(keys, [("1_1".to_string(), "a".to_string()), ("pl:9_1".to_string(), "k1".to_string())]);

        r.renamed(9, 2, "Пробежка");
        r.track_removed(9, 1, "1_1");
        assert_eq!(r.tracks(9, 1).unwrap().tracks.len(), 2);
        assert!(r.keys().iter().all(|(k, _)| k != "1_1"));
        r.removed(9, 1);
        assert_eq!(r.tracks(9, 1), None);
        assert_eq!(r.list().unwrap().iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Пробежка"]);
        // список с начала заменяет треки
        r.set_tracks(9, 2, 0, &[tr("2_1")], None, []);
        r.set_tracks(9, 2, 0, &[tr("2_2")], None, []);
        assert_eq!(PlaylistStore::at(path.clone()).tracks(9, 2).unwrap().tracks[0].id, "2_2");
        r.clear();
        assert!(!path.exists());
        assert_eq!(PlaylistStore::at(path).list(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
