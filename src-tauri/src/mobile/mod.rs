//! Android: то, что на компьютере делают mpv, WebKitGTK, трей и система, здесь делает Kotlin-плагин приложения
//! `VkmPlugin` (gen/android/app/src/main/java/io/github/whispermusic/vkmusic/): плеер — ExoPlayer (Media3) в фоновой
//! службе с уведомлением и экраном блокировки, вход в сервисы — окно с WebView, куки — `CookieManager` Android,
//! ссылки — приложение по умолчанию.
//!
//! Связь с плагином — `PluginHandle::run_mobile_plugin_async`. Плагин выполняет команды в главном потоке Android,
//! поэтому отсюда в него не ждут из главного потока: команды плеера уходят очередью (`send`, по порядку, без
//! ожидания ответа), а то, что возвращает данные (`ask` — куки, состояние входа), зовут только из фоновых потоков
//! и с пределом ожидания.

pub mod hls;
pub mod hotkeys;
pub mod login;
pub mod player;
pub mod tray;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::Wry;

type Res<T> = Result<T, String>;

/// Пакет Kotlin-классов приложения (= identifier из tauri.android.conf.json).
const PACKAGE: &str = "io.github.whispermusic.vkmusic";

struct Bridge {
    handle: PluginHandle<Wry>,
    /// команды без ответа — по одной, в порядке вызова (загрузить → пауза → перемотать)
    queue: tokio::sync::mpsc::UnboundedSender<(String, Value)>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// Плагин приложения: регистрирует Kotlin-часть (`VkmPlugin`) и очередь команд к ней.
pub fn init() -> TauriPlugin<Wry> {
    Builder::new("vkm")
        .setup(|_app, api| {
            let handle = api.register_android_plugin(PACKAGE, "VkmPlugin")?;
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, Value)>();
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                while let Some((cmd, payload)) = rx.recv().await {
                    if let Err(e) = h.run_mobile_plugin_async::<Value>(&cmd, payload).await {
                        eprintln!("vkmusic: {cmd}: {e}");
                    }
                }
            });
            let _ = BRIDGE.set(Bridge { handle, queue: tx });
            Ok(())
        })
        .build()
}

/// Команда плагину без ответа: уходит в очередь и выполняется после предыдущих.
pub fn send(cmd: &str, payload: impl Serialize) -> Res<()> {
    let b = BRIDGE.get().ok_or("Плеер Android ещё не готов")?;
    let v = serde_json::to_value(payload).map_err(|e| e.to_string())?;
    b.queue.send((cmd.to_string(), v)).map_err(|_| "Плеер Android остановлен".to_string())
}

/// Команда плагину с ответом. Только не из главного потока Android (плагин отвечает из него же).
pub fn ask<T: DeserializeOwned + Send + 'static>(cmd: &str, payload: impl Serialize) -> Res<T> {
    let b = BRIDGE.get().ok_or("Приложение ещё не готово")?;
    let v = serde_json::to_value(payload).map_err(|e| e.to_string())?;
    let (tx, rx) = std::sync::mpsc::channel();
    let h = b.handle.clone();
    let name = cmd.to_string();
    tauri::async_runtime::spawn(async move {
        let r = h.run_mobile_plugin_async::<T>(&name, v).await.map_err(|e| e.to_string());
        let _ = tx.send(r);
    });
    rx.recv_timeout(Duration::from_secs(10)).map_err(|_| format!("Android не ответил ({cmd})"))?
}

/// Открыть ссылку приложением по умолчанию (браузер) — вместо xdg-open.
pub fn open_url(url: &str) -> Res<()> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("На телефоне открываются только ссылки".into());
    }
    send("openUrl", serde_json::json!({ "url": url }))
}
