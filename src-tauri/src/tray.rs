//! Значок в трее и мини-плеер (как у Whisper Yandex: lib.rs → TrayIconBuilder, mini.rs).
//!
//! * Меню значка: «Открыть», «Мини-плеер», «Предыдущий», «Пауза / Играть», «Следующий», «Выход». Подписи и
//!   состояние кнопки присылает интерфейс (`tray_state`, на языке интерфейса).
//! * Linux (libayatana-appindicator / StatusNotifierItem): щелчки по значку приложению не приходят — оболочка сама
//!   показывает меню, поэтому всё продублировано пунктами. Библиотеку tray-icon загружает сама (dlopen) и без неё
//!   падает — поэтому сначала `available` проверяет, что она есть; нет — значка нет, остальное работает.
//!   В GNOME значки трея — только с расширением AppIndicator.
//! * Windows: левый щелчок — окно, правый — меню.
//! * «Закрывать в трей» (настройка): крестик прячет окно, музыка играет дальше; вернуть — «Открыть» в меню или
//!   повторный запуск приложения (single-instance показывает окно).
//! * Вид значка (`Config::tray_icon`, «Настройки → Общее → Трей»): всегда полоски логотипа без подложки, как значки
//!   других программ; `app` — цветами значка приложения, `light` / `dark` — белые или тёмные (под тёмную или светлую
//!   панель), `accent` — цветом темы (`Config::tray_accent`, присылает интерфейс при смене
//!   стиля). Полоски рисуются здесь (`bars`), без файлов картинок.
//! * Мини-плеер — окно `mini` (mini.html): обложка, название, кнопки. Создаётся при открытии, уничтожается через
//!   30 с после того, как его спрятали. Состояние — от главного окна (`mini_state`), кнопки — обратно событием
//!   `tray://ctl` главному окну (`mini_ctl`). Фиксированный размер: тайлинговые композиторы (Hyprland) делают
//!   такое окно плавающим.

use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const MINI: &str = "mini";
const MINI_W: f64 = 360.0;
const MINI_H: f64 = 132.0;
/// действия кнопок, которые выполняет главное окно (app.js → `tray://ctl`)
const PLAYER_ACTIONS: [&str; 6] = ["prev", "toggle", "next", "shuffle", "repeat", "like"];

/// виды значка (`Config::tray_icon`)
pub const ICONS: [&str; 4] = ["app", "light", "dark", "accent"];

/// значок создан (закрывать в трей можно только тогда)
static ACTIVE: AtomicBool = AtomicBool::new(false);
static HIDE_GEN: AtomicU64 = AtomicU64::new(0);

pub struct Tray {
    icon: TrayIcon,
    items: Vec<(&'static str, MenuItem<tauri::Wry>)>,
}

#[derive(Default)]
pub struct Mini {
    state: Mutex<Value>,
    shown_at: Mutex<Option<Instant>>,
    hidden_at: Mutex<Option<Instant>>,
}

/// Есть ли значок в трее (для «Закрывать в трей»).
pub fn active() -> bool {
    ACTIVE.load(SeqCst)
}

/// Можно ли показать значок: на Linux нужна библиотека AppIndicator (tray-icon грузит её сама и без неё падает).
#[cfg(target_os = "linux")]
pub fn available() -> bool {
    use std::ffi::CString;
    ["libayatana-appindicator3.so.1", "libappindicator3.so.1"].iter().any(|name| {
        let c = CString::new(*name).expect("имя библиотеки");
        // SAFETY: только проверка, что библиотека грузится; сразу выгружаем
        unsafe {
            let h = libc::dlopen(c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
            if h.is_null() {
                false
            } else {
                libc::dlclose(h);
                true
            }
        }
    })
}

#[cfg(not(target_os = "linux"))]
pub fn available() -> bool {
    true
}

/// Создать значок (при запуске, если включён в настройках и библиотека есть); `look`, `accent` — вид значка
/// из настроек (параметрами: вызывающий может держать замок настроек).
pub fn init(app: &AppHandle, look: &str, accent: &str) -> tauri::Result<()> {
    if app.try_state::<Tray>().is_some() || !available() {
        return Ok(());
    }
    let item = |id: &'static str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>).map(|m| (id, m));
    let items = vec![
        item("show", "Открыть VK Music")?,
        item("mini", "Мини-плеер")?,
        item("prev", "Предыдущий")?,
        item("toggle", "Играть")?,
        item("next", "Следующий")?,
        item("quit", "Выход")?,
    ];
    let sep = PredefinedMenuItem::separator(app)?;
    let by = |id: &str| &items.iter().find(|x| x.0 == id).expect("пункт меню").1;
    let menu = Menu::with_items(app, &[by("show"), by("mini"), &sep, by("prev"), by("toggle"), by("next"), &sep, by("quit")])?;
    let mut b = TrayIconBuilder::with_id("vkmusic")
        .tooltip("VK Music")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(not(windows)))
        .on_menu_event(|app, e| on_action(app, e.id.as_ref()))
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                crate::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = image(app, look, accent) {
        b = b.icon(icon);
    }
    let icon = b.build(app)?;
    app.manage(Tray { icon, items });
    ACTIVE.store(true, SeqCst);
    Ok(())
}

/// Убрать значок (выключили в настройках). Мини-плеер тоже закрывается.
pub fn remove(app: &AppHandle) {
    if let Some(t) = app.try_state::<Tray>() {
        let _ = t.icon.set_visible(false);
    }
    ACTIVE.store(false, SeqCst);
    if let Some(w) = app.get_webview_window(MINI) {
        let _ = w.destroy();
    }
}

/// Вернуть значок, если он уже создавался (повторное включение), иначе — создать.
pub fn show(app: &AppHandle, look: &str, accent: &str) -> tauri::Result<()> {
    if let Some(t) = app.try_state::<Tray>() {
        t.icon.set_visible(true)?;
        ACTIVE.store(true, SeqCst);
        return Ok(());
    }
    init(app, look, accent)
}

/// Сменить вид значка (настройки или новый цвет темы у вида `accent`).
pub fn set_look(app: &AppHandle, look: &str, accent: &str) {
    let Some(t) = app.try_state::<Tray>() else { return };
    if let Some(img) = image(app, look, accent) {
        let _ = t.icon.set_icon(Some(img));
    }
}

/// Цвет `#rrggbb`.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

/// цвета полосок значка приложения: средняя — яркая, боковые — приглушённые (icons/icon.png)
const APP_MID: [u8; 3] = [0x2d, 0xd5, 0xb7];
const APP_SIDE: [u8; 3] = [0x50, 0x94, 0x7a];

fn image(_app: &AppHandle, look: &str, accent: &str) -> Option<tauri::image::Image<'static>> {
    // одноцветные — боковые полоски того же цвета, полупрозрачные (как приглушённые на значке приложения)
    let mono = |c: [u8; 3]| [(c, SIDE_ALPHA), (c, 1.0), (c, SIDE_ALPHA)];
    let colors = match look {
        "light" => mono([0xf2, 0xf2, 0xf2]),
        "dark" => mono([0x26, 0x26, 0x2b]),
        "accent" => mono(parse_hex(accent).unwrap_or(APP_MID)),
        _ => [(APP_SIDE, 1.0), (APP_MID, 1.0), (APP_SIDE, 1.0)],
    };
    Some(tauri::image::Image::new_owned(bars(colors), ICON_PX, ICON_PX))
}

const SIDE_ALPHA: f32 = 0.62;

const ICON_PX: u32 = 64;

/// Три полоски логотипа (как на значке приложения: средняя выше) без подложки, RGBA ICON_PX×ICON_PX; `colors` —
/// цвет и непрозрачность каждой полоски слева направо. Края сглажены: покрытие пикселя считается по 4×4 точкам.
fn bars(colors: [([u8; 3], f32); 3]) -> Vec<u8> {
    // (левый край, верх); ширина 14, низ — 58, концы скруглены полностью; полоски не касаются друг друга
    const BARS: [(f32, f32); 3] = [(7.0, 27.0), (25.0, 6.0), (43.0, 17.0)];
    const W: f32 = 14.0;
    const BOTTOM: f32 = 58.0;
    let n = ICON_PX as usize;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            // пиксель целиком у одной полоски (между ними 4px), её цвет — по центру пикселя
            let Some(k) = (0..3).min_by(|&a, &b| {
                let d = |k: usize| (x as f32 + 0.5 - (BARS[k].0 + W / 2.0)).abs();
                d(a).total_cmp(&d(b))
            }) else { continue };
            let (left, top) = BARS[k];
            let (rgb, alpha) = colors[k];
            let mut a = 0.0f32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (px, py) = (x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0);
                    if inside_pill(px, py, left, top, W, BOTTOM) {
                        a += alpha / 16.0;
                    }
                }
            }
            let i = (y * n + x) * 4;
            out[i..i + 3].copy_from_slice(&rgb);
            out[i + 3] = (a.min(1.0) * 255.0).round() as u8;
        }
    }
    out
}

/// Точка внутри вертикальной полоски с полукруглыми концами.
fn inside_pill(px: f32, py: f32, left: f32, top: f32, w: f32, bottom: f32) -> bool {
    let r = w / 2.0;
    let cx = left + r;
    let cy = py.clamp(top + r, bottom - r);
    (px - cx).powi(2) + (py - cy).powi(2) <= r * r
}

fn on_action(app: &AppHandle, id: &str) {
    match id {
        "show" => {
            hide_mini(app);
            crate::show_main(app);
        }
        "mini" => toggle_mini(app),
        "hide" => hide_mini(app),
        "quit" => app.exit(0),
        a if PLAYER_ACTIONS.contains(&a) => {
            let _ = app.emit_to("main", "tray://ctl", a);
        }
        _ => {}
    }
}

/// Подписи пунктов меню (на языке интерфейса) и состояние «Пауза / Играть»; подсказка значка — что играет.
#[tauri::command(rename_all = "snake_case")]
pub fn tray_state(app: AppHandle, labels: std::collections::HashMap<String, String>, playing: bool, title: String) {
    let Some(t) = app.try_state::<Tray>() else { return };
    for (id, item) in &t.items {
        let key = if *id == "toggle" { if playing { "pause" } else { "play" } } else { id };
        if let Some(text) = labels.get(key).filter(|s| !s.is_empty() && s.len() <= 80) {
            let _ = item.set_text(text);
        }
    }
    let tip: String = if title.is_empty() { "VK Music".into() } else { format!("VK Music — {title}") };
    let _ = t.icon.set_tooltip(Some(tip.chars().take(120).collect::<String>()));
}

/// Главное окно сообщает мини-плееру, что играет.
#[tauri::command(rename_all = "snake_case")]
pub fn mini_state(app: AppHandle, mini: tauri::State<'_, Mini>, state: Value) {
    *mini.state.lock().unwrap() = state.clone();
    if app.get_webview_window(MINI).is_some() {
        let _ = app.emit_to(MINI, "mini://state", state);
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn mini_state_get(mini: tauri::State<'_, Mini>) -> Value {
    mini.state.lock().unwrap().clone()
}

/// Кнопки мини-плеера.
#[tauri::command(rename_all = "snake_case")]
pub fn mini_ctl(app: AppHandle, action: String) {
    match action.as_str() {
        "open" => on_action(&app, "show"),
        "hide" | "quit" => on_action(&app, &action),
        a if PLAYER_ACTIONS.contains(&a) => on_action(&app, a),
        _ => {}
    }
}

/// Мини-плеер из главного окна (кнопка в «⋯» плеера / настройках) — то же, что пункт меню значка.
#[tauri::command(rename_all = "snake_case")]
pub fn mini_toggle(app: AppHandle) {
    toggle_mini(&app);
}

fn hide_mini(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MINI) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        *app.state::<Mini>().hidden_at.lock().unwrap() = Some(Instant::now());
        let gen = HIDE_GEN.fetch_add(1, SeqCst) + 1;
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            if HIDE_GEN.load(SeqCst) != gen {
                return; // за это время окно снова открыли
            }
            if let Some(w) = app.get_webview_window(MINI) {
                if !w.is_visible().unwrap_or(true) {
                    let _ = w.destroy();
                }
            }
        });
    }
}

/// Потеря фокуса — мини-плеер прячется, как всплывающее меню (кроме первых мгновений после показа).
pub fn mini_blur(app: &AppHandle) {
    let just = app.state::<Mini>().shown_at.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(400));
    if !just {
        hide_mini(app);
    }
}

fn toggle_mini(app: &AppHandle) {
    let st = app.state::<Mini>();
    // щелчок по пункту меню сначала уводит фокус (окно прячется), потом приходит сам щелчок — это «закрыть»
    if st.hidden_at.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
        return;
    }
    if let Some(w) = app.get_webview_window(MINI) {
        if w.is_visible().unwrap_or(false) {
            return hide_mini(app);
        }
    }
    let w = match app.get_webview_window(MINI) {
        Some(w) => w,
        None => match WebviewWindowBuilder::new(app, MINI, WebviewUrl::App("mini.html".into()))
            .title("Мини-плеер VK Music")
            .inner_size(MINI_W, MINI_H)
            .min_inner_size(MINI_W, MINI_H)
            .max_inner_size(MINI_W, MINI_H)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
        {
            Ok(w) => {
                let a = app.clone();
                w.on_window_event(move |e| {
                    if let tauri::WindowEvent::Focused(false) = e {
                        mini_blur(&a);
                    }
                });
                w
            }
            Err(e) => return eprintln!("vkmusic: мини-плеер: {e}"),
        },
    };
    HIDE_GEN.fetch_add(1, SeqCst); // отменить отложенное уничтожение
    // место: правый верхний угол рабочей области (значки трея на Linux обычно в верхней панели справа;
    // на Windows — внизу, тогда правый нижний)
    let mon = app.primary_monitor().ok().flatten();
    if let Some(m) = mon {
        let scale = m.scale_factor();
        let (pw, ph) = ((MINI_W * scale).round() as i32, (MINI_H * scale).round() as i32);
        let wa = m.work_area();
        let gap = (12.0 * scale) as i32;
        let x = wa.position.x + wa.size.width as i32 - pw - gap;
        let y = if cfg!(windows) { wa.position.y + wa.size.height as i32 - ph - gap } else { wa.position.y + gap };
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
    *st.shown_at.lock().unwrap() = Some(Instant::now());
    let _ = w.show();
    let _ = w.set_focus();
    let state = st.state.lock().unwrap().clone();
    let _ = app.emit_to(MINI, "mini://state", state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex("#2dd5b7"), Some([0x2d, 0xd5, 0xb7]));
        assert_eq!(parse_hex("#FFF"), None);
        assert_eq!(parse_hex("2dd5b7"), None);
        assert_eq!(parse_hex("#2dd5bz"), None);
        assert_eq!(parse_hex("#ффф"), None);
    }

    #[test]
    fn bars_drawn() {
        let w = [255, 255, 255];
        let px = bars([(w, SIDE_ALPHA), (w, 1.0), (w, SIDE_ALPHA)]);
        assert_eq!(px.len(), (ICON_PX * ICON_PX * 4) as usize);
        let alpha = |x: usize, y: usize| px[(y * ICON_PX as usize + x) * 4 + 3];
        assert_eq!(alpha(32, 30), 255, "середина средней полоски");
        assert_eq!(alpha(14, 45), 158, "боковая полоска — приглушённая");
        assert_eq!(alpha(32, 2), 0, "над полосками пусто");
        assert_eq!(alpha(22, 45), 0, "между полосками пусто");
        // цветной: у каждой полоски свой цвет, все непрозрачные
        let px = bars([(APP_SIDE, 1.0), (APP_MID, 1.0), (APP_SIDE, 1.0)]);
        let at = |x: usize, y: usize| &px[(y * ICON_PX as usize + x) * 4..][..4];
        assert_eq!(at(14, 45), [0x50, 0x94, 0x7a, 255]);
        assert_eq!(at(32, 30), [0x2d, 0xd5, 0xb7, 255]);
    }
}
