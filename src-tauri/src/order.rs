//! Порядок треков в библиотеке — хранится локально: ~/.config/vkmusic/order.json.
//!
//! Список ключей, новые сверху: путь к файлу (`/home/…`) или `vk:<id>` / `sc:<id>`. Трек, впервые
//! появившийся в библиотеке (новый файл, синхронизированный или добавленный «+» трек сервиса), встаёт
//! первым; остальные сохраняют места. Новые из одной пачки (например, синхронизации) — в порядке пачки.
//! Пока файла нет, порядок собирается один раз из прежних дат (`init`).

use std::collections::HashSet;
use std::io::Write;
use crate::os::Private;
use std::path::PathBuf;
use std::sync::Mutex;

type Res<T> = Result<T, String>;

const MAX_KEYS: usize = 50_000;

pub struct OrderStore {
    /// None — порядок ещё не собран (нет файла)
    keys: Mutex<Option<Vec<String>>>,
    path: PathBuf,
}

/// Новые ключи пачки — в начало (в порядке пачки), без повторов.
pub fn prepend_new(keys: &mut Vec<String>, batch: impl IntoIterator<Item = String>) -> bool {
    let known: HashSet<String> = keys.iter().cloned().collect();
    let mut seen = HashSet::new();
    let fresh: Vec<String> = batch.into_iter().filter(|k| !known.contains(k) && seen.insert(k.clone())).collect();
    if fresh.is_empty() {
        return false;
    }
    let mut out = fresh;
    out.append(keys);
    out.truncate(MAX_KEYS);
    *keys = out;
    true
}

impl OrderStore {
    pub fn new() -> OrderStore {
        let path = crate::config::config_dir().join("order.json");
        let keys = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok());
        OrderStore { keys: Mutex::new(keys), path }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> OrderStore {
        OrderStore { keys: Mutex::new(None), path }
    }

    pub fn initialized(&self) -> bool {
        self.keys.lock().unwrap().is_some()
    }

    /// Первый запуск: порядок из прежних дат (ключи уже отсортированы, новые сначала).
    pub fn init(&self, keys: Vec<String>) -> Res<()> {
        let mut v = Vec::new();
        prepend_new(&mut v, keys);
        let mut g = self.keys.lock().unwrap();
        *g = Some(v);
        self.save(g.as_ref().unwrap())
    }

    /// Пачка известных сейчас ключей: незнакомые встают первыми. До `init` ничего не делает.
    pub fn seen(&self, batch: impl IntoIterator<Item = String>) -> Res<()> {
        let mut g = self.keys.lock().unwrap();
        let Some(keys) = g.as_mut() else { return Ok(()) };
        if prepend_new(keys, batch) {
            self.save(keys)?;
        }
        Ok(())
    }

    /// Убрать ключи, для которых `keep` вернул false (трек пропал из библиотеки).
    pub fn retain(&self, keep: impl Fn(&str) -> bool) -> Res<()> {
        let mut g = self.keys.lock().unwrap();
        let Some(keys) = g.as_mut() else { return Ok(()) };
        let before = keys.len();
        keys.retain(|k| keep(k));
        if keys.len() != before {
            self.save(keys)?;
        }
        Ok(())
    }

    /// Перетаскивание в библиотеке: поставить `key` перед `before` (None — в конец). До `init` — ошибка.
    pub fn move_before(&self, key: &str, before: Option<&str>) -> Res<()> {
        let mut g = self.keys.lock().unwrap();
        let Some(keys) = g.as_mut() else { return Err("Порядок библиотеки ещё не собран".into()) };
        if before == Some(key) {
            return Ok(());
        }
        keys.retain(|k| k != key);
        let at = before.and_then(|b| keys.iter().position(|k| k == b)).unwrap_or(keys.len());
        keys.insert(at, key.to_string());
        keys.truncate(MAX_KEYS);
        self.save(keys)
    }

    pub fn list(&self) -> Vec<String> {
        self.keys.lock().unwrap().clone().unwrap_or_default()
    }

    fn save(&self, keys: &[String]) -> Res<()> {
        let dir = self.path.parent().ok_or("Нет папки настроек")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
        let tmp = self.path.with_extension("json.tmp");
        let data = serde_json::to_vec(keys).map_err(|e| e.to_string())?;
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .private()
                .open(&tmp)
                .map_err(|e| format!("Не удалось сохранить порядок треков: {e}"))?;
            f.write_all(&data).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn new_tracks_go_first() {
        let dir = std::env::temp_dir().join(format!("vkmusic-order-test-{}", std::process::id()));
        let o = OrderStore::at(dir.join("order.json"));
        // до init — ничего
        o.seen(s(&["/m/x.opus"])).unwrap();
        assert!(!o.initialized());
        o.init(s(&["vk:1_1", "/m/a.opus", "vk:1_1"])).unwrap();
        assert_eq!(o.list(), s(&["vk:1_1", "/m/a.opus"]));
        // новый локальный файл — первый
        o.seen(s(&["/m/a.opus", "/m/b.opus"])).unwrap();
        assert_eq!(o.list(), s(&["/m/b.opus", "vk:1_1", "/m/a.opus"]));
        // потом синхронизация принесла новый трек ВК — он сдвигает локальный; пачка — в своём порядке
        o.seen(s(&["vk:1_9", "vk:1_8", "vk:1_1"])).unwrap();
        assert_eq!(o.list(), s(&["vk:1_9", "vk:1_8", "/m/b.opus", "vk:1_1", "/m/a.opus"]));
        // пропавший трек уходит, остальные на местах
        o.retain(|k| k != "/m/b.opus").unwrap();
        assert_eq!(o.list(), s(&["vk:1_9", "vk:1_8", "vk:1_1", "/m/a.opus"]));
        // перетаскивание: перед другим ключом, в конец, неизвестный ключ — тоже встаёт
        o.move_before("/m/a.opus", Some("vk:1_9")).unwrap();
        assert_eq!(o.list(), s(&["/m/a.opus", "vk:1_9", "vk:1_8", "vk:1_1"]));
        o.move_before("vk:1_9", None).unwrap();
        assert_eq!(o.list(), s(&["/m/a.opus", "vk:1_8", "vk:1_1", "vk:1_9"]));
        o.move_before("sc:5", Some("vk:1_1")).unwrap();
        assert_eq!(o.list(), s(&["/m/a.opus", "vk:1_8", "sc:5", "vk:1_1", "vk:1_9"]));
        o.retain(|k| k != "sc:5").unwrap();
        // сохранено на диск
        let back: Vec<String> = serde_json::from_slice(&std::fs::read(dir.join("order.json")).unwrap()).unwrap();
        assert_eq!(back, o.list());
        let _ = std::fs::remove_dir_all(dir);
    }
}
