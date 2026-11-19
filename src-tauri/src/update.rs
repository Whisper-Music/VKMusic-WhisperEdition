//! Автообновление — Windows и Android (на Linux приложение обновляют пакеты: .deb, AUR).
//!
//! **Windows.**
//! tauri-plugin-updater: `latest.json` последнего выпуска GitHub (его собирает release.yml), установщик NSIS
//! проверяется подписью minisign (открытый ключ — `plugins.updater.pubkey` в tauri.windows.conf.json, закрытый —
//! секрет `TAURI_SIGNING_PRIVATE_KEY` репозитория). Проверка — раз при запуске (интерфейс), установка — только по
//! нажатию «Обновить»: установщик закрывает приложение, ставит новую версию и запускает её.
//!
//! **Android** (просьба владельца, 2026-11-15). Плагин Tauri на телефоне не работает, поэтому своё: `latest-android.json`
//! последнего выпуска (`{ version, url, size }`, его кладёт задание `android` в release.yml) — проверка раз при запуске;
//! по «Обновить» APK качается в кэш приложения (адрес — только выпуски этого репозитория, размер — как в описании) и
//! открывается установщиком Android (`VkmPlugin.installApk`). Подпись проверяет система: поверх установленного встанет
//! только APK, подписанный тем же ключом, — подменённый файл не установится.

use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize)]
pub struct Update {
    pub version: String,
}

#[cfg(windows)]
mod imp {
    use super::Update;
    use tauri::AppHandle;
    use tauri_plugin_updater::UpdaterExt;

    pub async fn check(app: &AppHandle) -> Result<Option<Update>, String> {
        let u = app.updater().map_err(|e| e.to_string())?;
        let found = u.check().await.map_err(|e| format!("Не удалось проверить обновления: {e}"))?;
        Ok(found.map(|f| Update { version: f.version }))
    }

    pub async fn install(app: &AppHandle) -> Result<(), String> {
        let u = app.updater().map_err(|e| e.to_string())?;
        let Some(found) = u.check().await.map_err(|e| format!("Не удалось проверить обновления: {e}"))? else {
            return Ok(());
        };
        found
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| format!("Не удалось установить обновление: {e}"))?;
        // на Windows установщик сам закрывает приложение; на случай, если нет, — перезапуск
        app.restart();
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::Update;
    use serde::Deserialize;
    use tauri::AppHandle;

    const MANIFEST: &str = "https://github.com/Whisper-Music/whisper-vkmusic/releases/latest/download/latest-android.json";
    /// APK — только из выпусков этого репозитория. Прежнее имя (до 2026-11-19 — VKMusic-WhisperEdition) GitHub
    /// перенаправляет на новое; v0.2.0 принимает только его, поэтому release.yml пока пишет ссылку со старым именем.
    const DOWNLOADS: [&str; 2] = [
        "https://github.com/Whisper-Music/whisper-vkmusic/releases/download/",
        "https://github.com/Whisper-Music/VKMusic-WhisperEdition/releases/download/",
    ];
    const MAX_APK: u64 = 300 << 20;

    #[derive(Deserialize)]
    struct Manifest {
        version: String,
        url: String,
        size: u64,
    }

    fn client() -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .user_agent(concat!("VK-Music/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())
    }

    use super::newer;

    async fn manifest(app: &AppHandle) -> Result<Option<Manifest>, String> {
        let r = client()?.get(MANIFEST).send().await.map_err(|e| format!("Не удалось проверить обновления: {e}"))?;
        // выпуска с APK ещё нет — обновлений нет
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body = r
            .error_for_status()
            .map_err(|e| format!("Не удалось проверить обновления: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("Не удалось проверить обновления: {e}"))?;
        let m: Manifest = serde_json::from_slice(&body).map_err(|e| format!("Не удалось проверить обновления: {e}"))?;
        let ok = newer(&m.version, &app.package_info().version.to_string())
            && DOWNLOADS.iter().any(|d| m.url.starts_with(d))
            && !m.url.contains("..")
            && m.url.ends_with(".apk")
            && m.size > 0
            && m.size <= MAX_APK;
        Ok(ok.then_some(m))
    }

    pub async fn check(app: &AppHandle) -> Result<Option<Update>, String> {
        Ok(manifest(app).await?.map(|m| Update { version: m.version }))
    }

    pub async fn install(app: &AppHandle) -> Result<(), String> {
        let Some(m) = manifest(app).await? else { return Ok(()) };
        // кэш приложения Android (MainActivity ставит XDG_CACHE_HOME = cacheDir) — его отдаёт установщику FileProvider
        let dir = std::path::PathBuf::from(std::env::var_os("XDG_CACHE_HOME").ok_or("Нет папки кэша")?).join("update");
        // прошлые скачанные APK больше не нужны
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("vkmusic-{}.apk", m.version.trim().trim_start_matches('v')));
        let mut r = client()?
            .get(&m.url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Не удалось скачать обновление: {e}"))?;
        let save = |e: std::io::Error| format!("Не удалось сохранить обновление: {e}");
        let mut f = std::fs::File::create(&path).map_err(save)?;
        let mut got = 0u64;
        while let Some(chunk) = r.chunk().await.map_err(|e| format!("Не удалось скачать обновление: {e}"))? {
            got += chunk.len() as u64;
            if got > m.size {
                break;
            }
            std::io::Write::write_all(&mut f, &chunk).map_err(save)?;
        }
        drop(f);
        if got != m.size {
            let _ = std::fs::remove_file(&path);
            return Err("Обновление скачалось не полностью — попробуйте ещё раз".into());
        }
        crate::mobile::send("installApk", serde_json::json!({ "path": path.to_string_lossy() }))
    }
}

/// «0.2.0» → (0, 2, 0); суффиксы (-beta) не поддерживаются — такая версия не предлагается.
#[cfg(any(test, target_os = "android"))]
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|x| x.parse::<u64>().ok());
    let r = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(r)
}

/// Версия выпуска новее установленной.
#[cfg(any(test, target_os = "android"))]
fn newer(found: &str, current: &str) -> bool {
    matches!((parse_version(found), parse_version(current)), (Some(a), Some(b)) if a > b)
}

/// Есть ли новая версия (Linux — всегда нет).
#[tauri::command(rename_all = "snake_case")]
pub async fn update_check(app: AppHandle) -> Result<Option<Update>, String> {
    #[cfg(any(windows, target_os = "android"))]
    return imp::check(&app).await;
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = app;
        Ok(None)
    }
}

/// Скачать и установить новую версию (Windows; Android — открыть установщик системы).
#[tauri::command(rename_all = "snake_case")]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    #[cfg(any(windows, target_os = "android"))]
    return imp::install(&app).await;
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = app;
        Err("Обновления — через пакеты системы".into())
    }
}

/// Плагин подключается только на Windows (на Linux его настроек в конфиге нет): `.with_updater()` в цепочке Builder
/// компьютера (на Android плагина нет — там своё, `imp` выше).
#[cfg_attr(mobile, allow(dead_code))]
pub trait WithUpdater {
    fn with_updater(self) -> Self;
}

impl<R: tauri::Runtime> WithUpdater for tauri::Builder<R> {
    fn with_updater(self) -> Self {
        #[cfg(windows)]
        return self.plugin(tauri_plugin_updater::Builder::new().build());
        #[cfg(not(windows))]
        self
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn versions() {
        assert!(newer("0.2.1", "0.2.0"));
        assert!(newer("v1.0.0", "0.9.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
        assert!(!newer("0.3.0-beta", "0.2.0"));
        assert!(!newer("0.3", "0.2.0"));
    }
}
