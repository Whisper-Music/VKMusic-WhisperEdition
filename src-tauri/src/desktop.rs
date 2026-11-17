//! Обои рабочего стола как фон стиля (как у Whisper Yandex, просьба владельца 2026-11-17; порт его desktop.rs):
//! Windows — SystemParametersInfo, Linux — по очереди оболочки (своя из XDG_CURRENT_DESKTOP — первой), затем
//! оконные менеджеры без оболочки. Своё у VK Music: Omarchy и переключатель тем владельца (`current/background`,
//! `~/.cache/current_wallpaper` — как в omarchy.rs) и swww. Только читаем, картинка — до 50 МБ.


#[cfg(windows)]
mod imp {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(
            ui_action: u32,
            ui_param: u32,
            pv_param: *mut core::ffi::c_void,
            f_win_ini: u32,
        ) -> i32;
    }

    const SPI_GETDESKWALLPAPER: u32 = 0x0073;
    const MAX_BYTES: u64 = 50 * 1024 * 1024;

    fn system_path() -> Option<PathBuf> {
        let mut buf = [0u16; 520];
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETDESKWALLPAPER,
                buf.len() as u32,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                0,
            )
        };
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if len == 0 {
            return None;
        }
        Some(PathBuf::from(OsString::from_wide(&buf[..len])))
    }

    fn looks_like_image(p: &PathBuf) -> bool {
        let name_ok = p
            .file_name()
            .map(|n| n.to_string_lossy().eq_ignore_ascii_case("TranscodedWallpaper"))
            .unwrap_or(false);
        let ext_ok = p
            .extension()
            .map(|e| {
                matches!(
                    e.to_string_lossy().to_ascii_lowercase().as_str(),
                    "jpg" | "jpeg" | "jfif" | "png" | "bmp" | "webp" | "gif" | "avif"
                )
            })
            .unwrap_or(false);
        name_ok || ext_ok
    }

    pub fn read_wallpaper() -> Result<Vec<u8>, String> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(p) = system_path() {
            candidates.push(p);
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            candidates.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Themes\TranscodedWallpaper"));
        }
        for p in candidates {
            if !p.is_file() || !looks_like_image(&p) {
                continue;
            }
            let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
            if meta.len() > MAX_BYTES {
                return Err("Файл обоев больше 50 МБ".into());
            }
            return super::read_capped(&p, MAX_BYTES);
        }
        Err("Не удалось найти картинку обоев: возможно, на рабочем столе однотонный цвет. Выбери картинку вручную.".into())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    //! В Linux единого API для обоев нет — у каждой оболочки свой способ их хранить.
    //! Проверяем по очереди: сначала оболочку из XDG_CURRENT_DESKTOP, потом все остальные.
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const MAX_BYTES: u64 = 50 * 1024 * 1024;
    const IMAGE_EXT: [&str; 8] = ["jpg", "jpeg", "jfif", "png", "bmp", "webp", "gif", "avif"];

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    fn config_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home().map(|h| h.join(".config")))
    }

    /// `file:///...`, `~/...`, `$HOME/...` или обычный путь → путь к файлу.
    fn to_path(raw: &str) -> Option<PathBuf> {
        let s = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
        if s.is_empty() {
            return None;
        }
        if s.starts_with("file://") {
            return tauri::Url::parse(s).ok()?.to_file_path().ok();
        }
        if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("$HOME/")) {
            return home().map(|h| h.join(rest));
        }
        let p = PathBuf::from(s);
        p.is_absolute().then_some(p)
    }

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn gsettings(schema: &str, key: &str) -> Option<PathBuf> {
        to_path(&run("gsettings", &["get", schema, key])?)
    }

    /// Значения `key=` из ini-подобного файла (порядок сохраняется).
    fn ini_values(file: &Path, key: &str) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
        text.lines()
            .filter_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
            })
            .collect()
    }

    fn gnome() -> Vec<PathBuf> {
        let dark = run("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"])
            .is_some_and(|v| v.contains("dark"));
        let keys: &[&str] = if dark { &["picture-uri-dark", "picture-uri"] } else { &["picture-uri", "picture-uri-dark"] };
        keys.iter().filter_map(|k| gsettings("org.gnome.desktop.background", k)).collect()
    }

    fn cinnamon() -> Vec<PathBuf> {
        gsettings("org.cinnamon.desktop.background", "picture-uri").into_iter().collect()
    }

    fn mate() -> Vec<PathBuf> {
        gsettings("org.mate.background", "picture-filename").into_iter().collect()
    }

    fn kde() -> Vec<PathBuf> {
        let Some(cfg) = config_dir() else { return Vec::new() };
        ini_values(&cfg.join("plasma-org.kde.plasma.desktop-appletsrc"), "Image")
            .iter()
            .filter_map(|v| to_path(v))
            .collect()
    }

    fn xfce() -> Vec<PathBuf> {
        let Some(list) = run("xfconf-query", &["-c", "xfce4-desktop", "-l"]) else { return Vec::new() };
        list.lines()
            .filter(|p| p.ends_with("/last-image") || p.ends_with("/image-path"))
            .filter_map(|p| run("xfconf-query", &["-c", "xfce4-desktop", "-p", p]))
            .filter_map(|v| to_path(&v))
            .collect()
    }

    fn lxde_lxqt() -> Vec<PathBuf> {
        let Some(cfg) = config_dir() else { return Vec::new() };
        let mut out: Vec<PathBuf> = ini_values(&cfg.join("pcmanfm-qt/lxqt/settings.conf"), "Wallpaper")
            .iter()
            .filter_map(|v| to_path(v))
            .collect();
        if let Ok(rd) = std::fs::read_dir(cfg.join("pcmanfm")) {
            for profile in rd.flatten() {
                for f in ["desktop-items-0.conf", "pcmanfm.conf"] {
                    out.extend(ini_values(&profile.path().join(f), "wallpaper").iter().filter_map(|v| to_path(v)));
                }
            }
        }
        out
    }

    /// Оконные менеджеры без своей оболочки: Omarchy и переключатель тем владельца, swww, feh, nitrogen,
    /// hyprpaper, swaybg (из конфига sway).
    fn wm_tools() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(h) = home() {
            // Omarchy: ссылка current/background; переключатель тем — путь первой строкой ~/.cache/current_wallpaper
            for p in [".local/state/omarchy/current/background", ".config/omarchy/current/background"] {
                out.push(h.join(p));
            }
            if let Ok(t) = std::fs::read_to_string(h.join(".cache/current_wallpaper")) {
                out.extend(t.lines().next().and_then(to_path));
            }
        }
        // swww: «<монитор>: …, currently displaying: image: /путь»
        if let Some(q) = run("swww", &["query"]) {
            out.extend(q.lines().filter_map(|l| l.split_once("image: ").and_then(|x| to_path(x.1))));
        }
        if let Some(h) = home() {
            if let Ok(t) = std::fs::read_to_string(h.join(".fehbg")) {
                // feh --bg-fill '/путь/к/картинке.jpg'
                out.extend(t.split('\'').skip(1).step_by(2).filter_map(to_path));
            }
        }
        if let Some(cfg) = config_dir() {
            out.extend(ini_values(&cfg.join("nitrogen/bg-saved.cfg"), "file").iter().filter_map(|v| to_path(v)));
            for v in ini_values(&cfg.join("hypr/hyprpaper.conf"), "wallpaper") {
                // wallpaper = монитор,путь
                out.extend(to_path(v.split_once(',').map(|x| x.1).unwrap_or(&v)));
            }
            out.extend(ini_values(&cfg.join("hypr/hyprpaper.conf"), "preload").iter().filter_map(|v| to_path(v)));
            if let Ok(t) = std::fs::read_to_string(cfg.join("sway/config")) {
                // output * bg /путь/к/картинке.jpg fill
                for l in t.lines().map(str::trim).filter(|l| l.starts_with("output ")) {
                    let w: Vec<&str> = l.split_whitespace().collect();
                    if let Some(i) = w.iter().position(|x| *x == "bg") {
                        if let Some(p) = w.get(i + 1).and_then(|x| to_path(x)) {
                            out.push(p);
                        }
                    }
                }
            }
        }
        out
    }

    fn is_image(p: &Path) -> bool {
        p.extension()
            .map(|e| IMAGE_EXT.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    }

    /// Картинка внутри «пакета обоев» (KDE: папка с contents/images/*) или первая из слайд-шоу GNOME (*.xml).
    fn resolve(p: PathBuf) -> Option<PathBuf> {
        if p.is_dir() {
            let dir = if p.join("contents/images").is_dir() { p.join("contents/images") } else { p };
            return std::fs::read_dir(dir)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|f| f.is_file() && is_image(f))
                .max_by_key(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0));
        }
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) {
            let t = std::fs::read_to_string(&p).ok()?;
            let start = t.find("<file>")? + "<file>".len();
            let end = start + t[start..].find("</file>")?;
            return to_path(&t[start..end]).filter(|f| f.is_file());
        }
        // ссылка (Omarchy current/background) — на саму картинку
        let p = std::fs::canonicalize(&p).unwrap_or(p);
        (p.is_file() && is_image(&p)).then_some(p)
    }

    pub fn read_wallpaper() -> Result<Vec<u8>, String> {
        let desk = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
        let mut sources: Vec<(&str, fn() -> Vec<PathBuf>)> = vec![
            ("kde", kde),
            ("cinnamon", cinnamon),
            ("mate", mate),
            ("xfce", xfce),
            ("lxqt", lxde_lxqt),
            ("lxde", lxde_lxqt),
            ("gnome", gnome),
            ("unity", gnome),
            ("budgie", gnome),
            ("pantheon", gnome),
            ("", wm_tools),
        ];
        // своя оболочка — первой; оболочки из списка нет (Hyprland, sway, i3…) — первыми оконные менеджеры:
        // иначе gsettings отдал бы картинку GNOME по умолчанию, которой на этом столе нет
        let named = sources.iter().any(|(n, _)| !n.is_empty() && desk.contains(n));
        sources.sort_by_key(|(name, _)| if named { !(!name.is_empty() && desk.contains(name)) } else { !name.is_empty() });
        let mut seen = std::collections::HashSet::new();
        for (_, src) in sources {
            if !seen.insert(src as usize) {
                continue;
            }
            for p in src().into_iter().filter_map(resolve) {
                let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
                if meta.len() > MAX_BYTES {
                    return Err("Файл обоев больше 50 МБ".into());
                }
                return super::read_capped(&p, MAX_BYTES);
            }
        }
        Err("Не удалось найти картинку обоев: эта оболочка рабочего стола не сообщает, какие обои стоят, или на столе однотонный цвет. Выбери картинку вручную.".into())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn read_wallpaper() -> Result<Vec<u8>, String> {
        Err("В этой системе взять обои рабочего стола пока нельзя — выбери картинку вручную".into())
    }
}

/// Чтение не больше лимита: файл мог вырасти между проверкой размера и чтением (или это не файл вовсе).
#[allow(dead_code)]
fn read_capped(p: &std::path::Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    if !f.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Обои — не обычный файл".into());
    }
    let mut buf = Vec::new();
    f.take(max + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err(format!("Файл обоев больше {} МБ", max / (1024 * 1024)));
    }
    Ok(buf)
}

#[tauri::command]
pub async fn desktop_wallpaper() -> Result<tauri::ipc::Response, String> {
    imp::read_wallpaper().map(tauri::ipc::Response::new)
}

#[cfg(test)]
mod tests {
    /// Обои этой машины: cargo test live_wallpaper -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_wallpaper() {
        let b = super::imp::read_wallpaper().unwrap();
        println!("{} байт, начало: {:02x?}", b.len(), &b[..8.min(b.len())]);
    }
}
