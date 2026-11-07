//! Окно помнит себя (как у Whisper Yandex, `winstate.rs`): размер, положение и развёрнутость — в config.json
//! (`window`). Главное окно создаётся скрытым (`visible: false` в tauri.conf.json), `restore` ставит прежний
//! размер и только потом показывает — без «прыжка» со стандартного размера. Положение возвращается, только
//! если окно попадёт на один из подключённых мониторов (монитор отключили — окно не уедет за край).
//! На Wayland положение окну задаёт композитор: там помнится только размер.

use serde::{Deserialize, Serialize};
use tauri::{LogicalSize, Manager, PhysicalPosition, Window, WindowEvent};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WinState {
    /// размер обычного (не развёрнутого) окна, логические пиксели
    pub w: f64,
    pub h: f64,
    /// левый верхний угол, физические пиксели (None — неизвестно, Wayland)
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub max: bool,
}

/// Не меньше минимального размера окна (tauri.conf.json) и не больше разумного.
const MIN: (f64, f64) = (860.0, 560.0);
const MAX: f64 = 16384.0;

impl WinState {
    fn sane(&self) -> bool {
        self.w.is_finite() && self.h.is_finite() && (MIN.0..=MAX).contains(&self.w) && (MIN.1..=MAX).contains(&self.h)
    }
}

/// Точка (угол окна плюс немного внутрь) — на каком-то мониторе: (x, y, ширина, высота) мониторов.
fn on_screen(x: i32, y: i32, monitors: &[(i32, i32, u32, u32)]) -> bool {
    let (px, py) = (x.saturating_add(40), y.saturating_add(16));
    monitors
        .iter()
        .any(|&(mx, my, mw, mh)| px >= mx && py >= my && px < mx.saturating_add(mw as i32) && py < my.saturating_add(mh as i32))
}

/// Прежние размер и положение — окну, затем показать его.
pub fn restore(win: &Window, saved: Option<WinState>) {
    if let Some(s) = saved.filter(WinState::sane) {
        let _ = win.set_size(LogicalSize::new(s.w, s.h));
        if let (Some(x), Some(y)) = (s.x, s.y) {
            let monitors: Vec<_> = win
                .available_monitors()
                .unwrap_or_default()
                .iter()
                .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
                .collect();
            if on_screen(x, y, &monitors) {
                let _ = win.set_position(PhysicalPosition::new(x, y));
            }
        }
        if s.max {
            let _ = win.maximize();
        }
    }
    let _ = win.show();
}

/// Текущее состояние окна поверх прежнего: у развёрнутого / свёрнутого размер и положение не трогаем
/// (иначе после «развернуть» обычный размер стал бы размером экрана).
fn snapshot(win: &Window, prev: Option<WinState>) -> Option<WinState> {
    let max = win.is_maximized().unwrap_or(false);
    let mut s = prev.unwrap_or(WinState { w: 1280.0, h: 820.0, x: None, y: None, max });
    s.max = max;
    if max || win.is_minimized().unwrap_or(false) || win.is_fullscreen().unwrap_or(false) {
        return Some(s);
    }
    let scale = win.scale_factor().unwrap_or(1.0);
    if let Ok(size) = win.inner_size() {
        let l = size.to_logical::<f64>(scale);
        if l.width >= MIN.0 && l.height >= MIN.1 {
            s.w = l.width.round();
            s.h = l.height.round();
        }
    }
    if let Ok(p) = win.outer_position() {
        // Wayland отдаёт (0, 0) — это не положение
        if p.x != 0 || p.y != 0 {
            s.x = Some(p.x);
            s.y = Some(p.y);
        }
    }
    Some(s)
}

/// Следить за окном: изменения — в настройки (на диск они попадут при выходе, вместе с остальными).
pub fn track(win: &Window, store: impl Fn(&dyn Fn(Option<WinState>) -> Option<WinState>) + Send + Sync + 'static) {
    let w = win.clone();
    win.on_window_event(move |e| {
        if matches!(e, WindowEvent::Resized(_) | WindowEvent::Moved(_) | WindowEvent::CloseRequested { .. }) {
            store(&|prev| snapshot(&w, prev));
        }
    });
}

/// Окно main — для `restore` / `track` (пока открыт вход, в нём два webview, поэтому — Window, не WebviewWindow).
pub fn main_window(app: &tauri::AppHandle) -> Option<Window> {
    app.get_window("main")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_only_on_connected_monitor() {
        let mons = [(0, 0, 2560, 1440), (2560, 0, 1920, 1080)];
        assert!(on_screen(100, 100, &mons));
        assert!(on_screen(3000, 50, &mons));
        // второй монитор отключили — окно было на нём
        assert!(!on_screen(3000, 50, &mons[..1]));
        // ниже второго (он ниже ростом) — мимо
        assert!(!on_screen(3000, 1200, &mons));
        assert!(!on_screen(-500, 10, &mons));
    }

    #[test]
    fn sane_sizes() {
        let s = WinState { w: 1280.0, h: 820.0, x: None, y: None, max: false };
        assert!(s.sane());
        assert!(!WinState { w: 100.0, ..s }.sane());
        assert!(!WinState { h: f64::NAN, ..s }.sane());
    }
}
