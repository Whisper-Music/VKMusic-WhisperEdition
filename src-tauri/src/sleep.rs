//! Режим сна (как у Whisper Yandex): через заданное время музыка плавно затихает и встаёт на паузу, а компьютер,
//! если выбрано, выключается.
//!
//! Таймер и затухание — здесь, а не в странице: окно может быть свёрнуто в трей, а страница — занята или
//! перезагружена, а выключить компьютер нужно в любом случае. Время сверяется с системными часами каждую секунду —
//! если компьютер успел уснуть и проснуться, таймер не «растягивается». Затухание — множитель громкости mpv
//! (`Player::set_fade`): ползунок и настройки не меняются, после паузы громкость возвращается.
//!
//! Событие странице — `sleep://event`: `{ type: "end", shutdown, error }` — время вышло (музыка на паузе;
//! `shutdown` — компьютер выключится через минуту, `error` — выключить не удалось).
//! Выключение — системной командой с минутой на отмену (`shutdown -h +1` / `shutdown /s /t 60`); `sleep_cancel`
//! отменяет и его. Подтверждение выключения — окошко у кнопки в интерфейсе (`confirmClick`).

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

pub const EVENT: &str = "sleep://event";
/// последние 3 минуты музыка тихо затихает
const FADE_SECS: u64 = 180;

static GEN: AtomicU64 = AtomicU64::new(0);
/// мс от эпохи; 0 — режим выключен
static DEADLINE: AtomicU64 = AtomicU64::new(0);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
/// команда выключения уже отдана системе — «Не выключать» её отменяет
static SHUTDOWN_SENT: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Serialize)]
pub struct SleepState {
    /// когда время выйдет, мс от эпохи
    deadline: u64,
    shutdown: bool,
}

/// Множитель громкости за `left` секунд до конца: последние `len` секунд — от 1 до 0.
fn fade_at(left: u64, len: u64) -> f64 {
    if len == 0 || left >= len {
        1.0
    } else {
        left as f64 / len as f64
    }
}

fn set_fade(app: &AppHandle, k: f64) {
    if let Ok(p) = app.state::<crate::AppState>().player() {
        let _ = p.set_fade(k);
    }
}

pub fn start(app: AppHandle, minutes: u32, shutdown: bool) -> Result<SleepState, String> {
    if !(1..=24 * 60).contains(&minutes) {
        return Err("Время должно быть от 1 минуты до 24 часов".into());
    }
    cancel(&app);
    let total = minutes as u64 * 60;
    let deadline = now_ms() + total * 1000;
    let run_id = GEN.fetch_add(1, SeqCst) + 1;
    DEADLINE.store(deadline, SeqCst);
    SHUTDOWN.store(shutdown, SeqCst);
    let fade_len = FADE_SECS.min(total);
    std::thread::Builder::new()
        .name("sleep".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if GEN.load(SeqCst) != run_id {
                return; // отменили или включили заново
            }
            let now = now_ms();
            if now < deadline {
                let left = (deadline - now).div_ceil(1000);
                if left <= fade_len {
                    set_fade(&app, fade_at(left, fade_len));
                }
                continue;
            }
            DEADLINE.store(0, SeqCst);
            if let Ok(p) = app.state::<crate::AppState>().player() {
                let _ = p.set_pause(true);
                let _ = p.set_fade(1.0);
            }
            let mut error = None;
            if SHUTDOWN.load(SeqCst) {
                // настройки и размер окна — на диск сейчас: через минуту приложение закроет система
                let _ = app.state::<crate::AppState>().save();
                match power_off() {
                    Ok(()) => SHUTDOWN_SENT.store(true, SeqCst),
                    Err(e) => error = Some(format!("Не удалось выключить компьютер: {e}")),
                }
            }
            let sent = SHUTDOWN_SENT.load(SeqCst);
            let _ = app.emit(EVENT, serde_json::json!({ "type": "end", "shutdown": sent, "error": error }));
            return;
        })
        .map_err(|e| e.to_string())?;
    Ok(SleepState { deadline, shutdown })
}

/// Выключить режим сна; если компьютер уже собирается выключиться — отменить и это.
pub fn cancel(app: &AppHandle) {
    GEN.fetch_add(1, SeqCst);
    DEADLINE.store(0, SeqCst);
    set_fade(app, 1.0);
    if SHUTDOWN_SENT.swap(false, SeqCst) {
        abort_power_off();
    }
}

pub fn status() -> Option<SleepState> {
    let d = DEADLINE.load(SeqCst);
    (d != 0).then(|| SleepState { deadline: d, shutdown: SHUTDOWN.load(SeqCst) })
}

/// Системная команда; Ok — только если она отработала без ошибки.
fn run(prog: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = std::process::Command::new(prog);
    cmd.args(args).stdin(std::process::Stdio::null());
    crate::os::hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| format!("{prog}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    Err(if err.is_empty() { format!("{prog}: {}", out.status) } else { format!("{prog}: {err}") })
}

/// Выключение через минуту (её хватает, чтобы нажать «Не выключать»).
fn power_off() -> Result<(), String> {
    if cfg!(windows) {
        run("shutdown", &["/s", "/t", "60", "/c", "Режим сна VK Music: компьютер выключится через минуту."])
    } else {
        // systemd: logind разрешает это пользователю активного локального сеанса
        run("shutdown", &["-h", "+1"])
    }
}

fn abort_power_off() {
    let _ = if cfg!(windows) { run("shutdown", &["/a"]) } else { run("shutdown", &["-c"]) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn fade_curve() {
        use super::fade_at;
        assert_eq!(fade_at(500, 180), 1.0);
        assert_eq!(fade_at(180, 180), 1.0);
        assert_eq!(fade_at(90, 180), 0.5);
        assert_eq!(fade_at(0, 180), 0.0);
        // режим короче затухания: затихает всё время
        assert_eq!(fade_at(30, 60), 0.5);
        assert_eq!(fade_at(5, 0), 1.0);
    }
}
