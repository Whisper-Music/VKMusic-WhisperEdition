//! Автообновление — только Windows (на Linux приложение обновляют пакеты: .deb, AUR).
//!
//! tauri-plugin-updater: `latest.json` последнего выпуска GitHub (его собирает release.yml), установщик NSIS
//! проверяется подписью minisign (открытый ключ — `plugins.updater.pubkey` в tauri.windows.conf.json, закрытый —
//! секрет `TAURI_SIGNING_PRIVATE_KEY` репозитория). Проверка — раз при запуске (интерфейс), установка — только по
//! нажатию «Обновить»: установщик закрывает приложение, ставит новую версию и запускает её.

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

/// Есть ли новая версия (не на Windows — всегда нет).
#[tauri::command(rename_all = "snake_case")]
pub async fn update_check(app: AppHandle) -> Result<Option<Update>, String> {
    #[cfg(windows)]
    return imp::check(&app).await;
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(None)
    }
}

/// Скачать и установить новую версию (только Windows).
#[tauri::command(rename_all = "snake_case")]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    return imp::install(&app).await;
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Обновления — через пакеты системы".into())
    }
}

/// Плагин подключается только на Windows (на Linux его настроек в конфиге нет): `.with_updater()` в цепочке Builder.
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
