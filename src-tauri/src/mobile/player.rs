//! Плеер на Android: ExoPlayer (Media3) в фоновой службе `PlaybackService` — играет при выключенном экране,
//! с уведомлением и кнопками на экране блокировки, наушниках и часах. Тот же API, что у mpv (player.rs), чтобы
//! команды (cmd/player_cmd.rs, vk_cmd, …) не знали, на чём играют.
//!
//! * Громкость ползунка — множитель ExoPlayer: усиление = (v/100)², как шкала интерфейса на компьютере
//!   (`player::GAIN_POW`); системную громкость меняют кнопки телефона.
//! * Эквалайзер и «Постоянный уровень громкости» — `DynamicsProcessing` Android (9+): 10 полос с теми же
//!   частотами, предусиление; выравнивание — сжатие и ограничитель на лету (замера EBU R128, как с ffmpeg на
//!   компьютере, здесь нет). На Android 8 и старше их нет — звук идёт как есть.
//! * Устройство вывода выбирает система (наушники, Bluetooth).
//! * Кнопки «назад/вперёд» уведомления и гарнитуры приходят событием `ctl` → `tray://ctl` (как меню трея).

use crate::player::{AudioDevice, Eq, PlayerEvent, PlayerState};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter};

/// Сколько ждёт «начать с секунды / на паузе» (`cue`), как на компьютере.
const CUE_TTL: Duration = Duration::from_secs(30);

pub struct Player {
    cue: Mutex<Option<(f64, bool, Instant)>>,
    norm: Mutex<bool>,
    eq: Mutex<Eq>,
    /// громкость ползунка — общая с каналом событий (в состоянии для интерфейса)
    vol: Arc<Mutex<f64>>,
    fade: Mutex<f64>,
}

/// Событие от ExoPlayer (VkmPlugin.kt → канал).
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Native {
    State { pos: f64, duration: f64, paused: bool, idle: bool, file: u64 },
    Ended,
    Error { message: String },
    /// кнопка уведомления, гарнитуры или экрана блокировки: prev | toggle | next
    Ctl { action: String },
    /// визуализатор: уровни полос, дБ
    Viz { b: Vec<i16> },
}

impl Player {
    pub fn start<F>(app: &AppHandle, volume: f64, eq: Eq, on_event: F) -> Result<Player, String>
    where
        F: Fn(PlayerEvent) + Send + Sync + 'static,
    {
        let p = Player {
            cue: Mutex::new(None),
            norm: Mutex::new(false),
            eq: Mutex::new(eq.clean()),
            vol: Arc::new(Mutex::new(volume.clamp(0.0, 100.0))),
            fade: Mutex::new(1.0),
        };
        let vol = p.vol.clone();
        let app = app.clone();
        let channel = Channel::<serde_json::Value>::new(move |body: InvokeResponseBody| {
            let text = match body {
                InvokeResponseBody::Json(s) => s,
                InvokeResponseBody::Raw(b) => String::from_utf8_lossy(&b).into_owned(),
            };
            let Ok(ev) = serde_json::from_str::<Native>(&text) else { return Ok(()) };
            match ev {
                Native::State { pos, duration, paused, idle, file } => {
                    let volume = *vol.lock().unwrap();
                    on_event(PlayerEvent::State(PlayerState { pos, duration, paused, idle, volume, file }));
                }
                Native::Ended => on_event(PlayerEvent::Ended),
                Native::Error { message } => on_event(PlayerEvent::Error { message }),
                Native::Viz { b } => on_event(PlayerEvent::Viz { b }),
                Native::Ctl { action } => {
                    if ["prev", "toggle", "next"].contains(&action.as_str()) {
                        let _ = app.emit_to("main", "tray://ctl", action);
                    }
                }
            }
            Ok(())
        });
        super::send("setEvents", json!({ "channel": channel }))?;
        p.send_volume()?;
        p.send_effects()?;
        Ok(p)
    }

    pub fn cue(&self, at: f64, hold: bool) {
        *self.cue.lock().unwrap() = Some((at.max(0.0), hold, Instant::now()));
    }

    fn load_with(&self, src: &str, title: &str, user_agent: &str, referrer: &str) -> Result<(), String> {
        let cue = self.cue.lock().unwrap().take().filter(|c| c.2.elapsed() < CUE_TTL);
        let (at, hold) = cue.map(|c| (c.0, c.1)).unwrap_or((0.0, false));
        super::send(
            "load",
            json!({ "src": src, "title": title, "userAgent": user_agent, "referer": referrer, "start": at, "pause": hold }),
        )
    }

    /// Файл на телефоне (папка музыки, кэш, архив).
    pub fn load(&self, src: &str, title: &str) -> Result<(), String> {
        if src.contains("://") {
            return Err("Неверный путь к файлу".into());
        }
        self.load_with(src, title, "", "")
    }

    /// Поток сервиса по https: с User-Agent и Referer сайта, на котором выполнен вход.
    pub fn load_stream(&self, url: &str, title: &str, user_agent: &str, referrer: &str) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err("Неверная ссылка на поток".into());
        }
        self.load_with(url, title, user_agent, referrer)
    }

    /// Визуализатор: замеры (Engine.viz в Kotlin) — только пока он включён и его видно.
    pub fn set_viz(&self, enabled: bool, live: bool) -> Result<(), String> {
        super::send("viz", json!({ "on": enabled && live }))
    }

    pub fn set_speed(&self, rate: f64) -> Result<(), String> {
        super::send("speed", json!({ "rate": rate.clamp(0.25, 4.0) }))
    }

    pub fn set_normalize(&self, on: bool) -> Result<(), String> {
        *self.norm.lock().unwrap() = on;
        self.send_effects()
    }

    pub fn set_eq(&self, eq: Eq) -> Result<(), String> {
        *self.eq.lock().unwrap() = eq.clean();
        self.send_effects()
    }

    fn send_effects(&self) -> Result<(), String> {
        let eq = self.eq.lock().unwrap().clone();
        let norm = *self.norm.lock().unwrap();
        super::send(
            "effects",
            json!({
                "eq": eq.on,
                "pre": eq.pre_db(),
                "gains": eq.gains,
                "bands": crate::player::EQ_BANDS,
                "normalize": norm,
            }),
        )
    }

    /// Устройство вывода на телефоне выбирает система.
    pub fn devices(&self) -> (Vec<AudioDevice>, String) {
        (Vec::new(), "auto".into())
    }

    pub fn set_device(&self, name: &str) -> Result<(), String> {
        if name == "auto" {
            return Ok(());
        }
        Err("На телефоне устройство вывода выбирает система".into())
    }

    pub fn restore_device(self: &Arc<Self>, _name: String) {}

    pub fn set_pause(&self, paused: bool) -> Result<(), String> {
        super::send("pause", json!({ "paused": paused }))
    }

    pub fn seek(&self, pos: f64) -> Result<(), String> {
        super::send("seek", json!({ "pos": pos.max(0.0) }))
    }

    pub fn set_volume(&self, v: f64) -> Result<(), String> {
        let v = v.clamp(0.0, 130.0);
        *self.vol.lock().unwrap() = v;
        self.send_volume()
    }

    pub fn set_fade(&self, k: f64) -> Result<(), String> {
        let k = if k.is_finite() { k.clamp(0.0, 1.0) } else { 1.0 };
        let old = std::mem::replace(&mut *self.fade.lock().unwrap(), k);
        if (old - k).abs() < 1e-6 {
            return Ok(());
        }
        self.send_volume()
    }

    /// Множитель ExoPlayer: та же квадратичная шкала, что на компьютере; выше 100% — без усиления (у ExoPlayer
    /// громкость не больше 1).
    fn send_volume(&self) -> Result<(), String> {
        let v = (*self.vol.lock().unwrap() * *self.fade.lock().unwrap() / 100.0).clamp(0.0, 1.0);
        super::send("volume", json!({ "gain": v * v }))
    }

    pub fn stop(&self) -> Result<(), String> {
        super::send("stop", json!({}))
    }

    pub fn quit(&self) {
        let _ = self.stop();
    }
}

/// Что играет — в уведомление и на экран блокировки (зовёт `mini_state` из tray.js).
pub fn meta(title: &str, artist: &str, cover: &str, playing: bool) {
    // обложка — только с CDN сервисов по https (файлы из папки музыки — vkm://, системе их не отдать)
    let cover = if cover.starts_with("https://") && crate::whisper_host::cover_ok(cover) { cover } else { "" };
    let _ = super::send("meta", json!({ "title": title, "artist": artist, "cover": cover, "playing": playing }));
}
