//! Режим экономии ресурсов, пока окно спрятано в трей (как у Whisper Yandex — порт его power.rs, просьба владельца
//! 2026-11-17). Своё у VK Music: звук играет mpv, а не страница, поэтому заморозка страницы музыку не останавливает,
//! но страница тогда не услышит конец трека — замораживаем, как и там, только когда музыка не играет.
//!
//! Что происходит, когда окно уходит в трей:
//! 2. движок окна (WebView2) помечается невидимым (не рисует ни кадра) и получает низкий уровень памяти —
//!    Edge-процессы отдают системе всё, что можно;
//! 3. у самого приложения сбрасывается рабочий набор памяти;
//! 4. если музыка не играет (и не грузится следующий трек) — через 8 с страница полностью замораживается (TrySuspend): скрипты и таймеры стоят,
//!    процессор не тратится вообще. Пока музыка играет, заморозки нет (иначе звук остановится);
//!    поставил на паузу из трея — заморозка наступает сама.
//! При показе окна всё возвращается как было.
//! Окно просто свёрнуто на панель задач — движок перестаёт рисовать (видеообои и анимации на паузе), но не замораживается.
//!
//! Всё, что выше про WebView2, — только Windows. В Linux движок окна другой (WebKitGTK), и у него нет
//! ни TrySuspend, ни уровней памяти. Там работает то, что не зависит от движка:
//! * спрятанное окно WebKitGTK сам считает невидимым: `document.hidden`, анимации и таймеры притормаживаются;
//! * память, которую приложение уже освободило, возвращается системе (malloc_trim).
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::time::Duration;
use tauri::{AppHandle, Manager};

static PLAYING: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static SUSPENDED: AtomicBool = AtomicBool::new(false);
static MINIMIZED: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0); // отменяет отложенную заморозку, если окно успели показать или разбудить

pub fn enter(app: &AppHandle) {
    if HIDDEN.swap(true, SeqCst) {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        imp::low(&w);
    }
    imp::trim_self();
    schedule_suspend(app);
}

pub fn leave(app: &AppHandle) {
    HIDDEN.store(false, SeqCst);
    GEN.fetch_add(1, SeqCst);
    SUSPENDED.store(false, SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        imp::normal(&w);
    }
}

/// Окно свернули на панель задач (или развернули обратно): движок окна перестаёт рисовать,
/// страница получает `document.hidden` — останавливаются видеообои, визуализатор и анимации.
pub fn on_resized(window: &tauri::Window) {
    let m = window.is_minimized().unwrap_or(false);
    if MINIMIZED.swap(m, SeqCst) == m || HIDDEN.load(SeqCst) {
        return;
    }
    if let Some(w) = window.app_handle().get_webview_window("main") {
        imp::visible(&w, !m);
    }
}

/// Команда из меню трея (пауза, следующий трек): замороженную страницу сначала будим, иначе она не услышит.
pub fn wake(app: &AppHandle) {
    if !HIDDEN.load(SeqCst) {
        return;
    }
    GEN.fetch_add(1, SeqCst);
    SUSPENDED.store(false, SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        imp::resume(&w);
    }
}

/// Страница сообщает, играет ли музыка. Поставили на паузу, пока окно в трее, — можно замораживать.
pub fn on_playing(app: &AppHandle, playing: bool) {
    PLAYING.store(playing, SeqCst);
    if !playing && HIDDEN.load(SeqCst) {
        schedule_suspend(app);
    }
}

fn schedule_suspend(app: &AppHandle) {
    let gen = GEN.fetch_add(1, SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        // запас на случай, если страница на миг сообщила «не играет» (стык треков, смена плеера визуализатора):
        // раньше 1,5 с не хватало, и страницу замораживало посреди загрузки следующего трека
        std::thread::sleep(Duration::from_secs(8));
        if GEN.load(SeqCst) != gen || !HIDDEN.load(SeqCst) || PLAYING.load(SeqCst) {
            return;
        }
        if let Some(w) = app.get_webview_window("main") {
            imp::suspend(&w);
        }
    });
}

#[cfg(windows)]
mod imp {
    use super::{SeqCst, SUSPENDED};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2_19, ICoreWebView2_3,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use webview2_com::TrySuspendCompletedHandler;
    use windows::core::Interface;

    fn with_core<F>(w: &tauri::WebviewWindow, f: F)
    where
        F: FnOnce(&ICoreWebView2Controller, &ICoreWebView2) + Send + 'static,
    {
        let _ = w.with_webview(move |wv| unsafe {
            let ctrl = wv.controller();
            if let Ok(core) = ctrl.CoreWebView2() {
                f(&ctrl, &core);
            }
        });
    }

    pub fn low(w: &tauri::WebviewWindow) {
        with_core(w, |ctrl, core| unsafe {
            let _ = ctrl.SetIsVisible(false);
            if let Ok(c) = core.cast::<ICoreWebView2_19>() {
                let _ = c.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
        });
    }

    pub fn normal(w: &tauri::WebviewWindow) {
        with_core(w, |ctrl, core| unsafe {
            if let Ok(c) = core.cast::<ICoreWebView2_3>() {
                let _ = c.Resume();
            }
            if let Ok(c) = core.cast::<ICoreWebView2_19>() {
                let _ = c.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
            }
            let _ = ctrl.SetIsVisible(true);
        });
    }

    pub fn visible(w: &tauri::WebviewWindow, on: bool) {
        with_core(w, move |ctrl, _| unsafe {
            let _ = ctrl.SetIsVisible(on);
        });
    }

    pub fn resume(w: &tauri::WebviewWindow) {
        with_core(w, |_, core| unsafe {
            if let Ok(c) = core.cast::<ICoreWebView2_3>() {
                let _ = c.Resume();
            }
        });
    }

    pub fn suspend(w: &tauri::WebviewWindow) {
        with_core(w, |ctrl, core| unsafe {
            let _ = ctrl.SetIsVisible(false); // заморозка работает только для невидимого движка
            if let Ok(c) = core.cast::<ICoreWebView2_3>() {
                let h = TrySuspendCompletedHandler::create(Box::new(|res, ok| {
                    if res.is_ok() && ok {
                        SUSPENDED.store(true, SeqCst);
                    }
                    Ok(())
                }));
                let _ = c.TrySuspend(&h);
            }
        });
    }

    /// Сбросить рабочий набор памяти самого приложения (страницы вернутся с диска, если понадобятся).
    pub fn trim_self() {
        unsafe {
            let _ = windows::Win32::System::ProcessStatus::EmptyWorkingSet(
                windows::Win32::System::Threading::GetCurrentProcess(),
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn low(_: &tauri::WebviewWindow) {}
    pub fn normal(_: &tauri::WebviewWindow) {}
    pub fn resume(_: &tauri::WebviewWindow) {}
    pub fn visible(_: &tauri::WebviewWindow, _: bool) {}
    pub fn suspend(_: &tauri::WebviewWindow) {}

    /// Linux (glibc): отдать системе свободные страницы кучи. В musl и других системах — ничего не делаем.
    pub fn trim_self() {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            unsafe {
                malloc_trim(0);
            }
        }
    }
}
