//! Плеер: включение файлов, пауза, перемотка, громкость, устройство вывода, трей, выравнивание громкости.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── плеер ─────────────────────────

/// Команды «играть» возвращают true, если трек начал кэшироваться (тогда придёт `cache://event`);
/// false — он уже в кэше или не из библиотеки, следующий в очереди можно кэшировать сразу.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_play(st: State<'_, AppState>, src: String, title: Option<String>) -> Res<bool> {
    let p = st.player()?;
    st.cache.begin();
    // играем только файлы из папки музыки
    if !library::inside(&st.music_dir(), Path::new(&src)) {
        return Err("Файл вне папки музыки".into());
    }
    let title = title.unwrap_or_default();
    let Some(job) = local_job(&src) else { return p.load(&src, &title).map(|_| false) };
    if let Some(c) = st.cache.get(&job.key) {
        {
                st.cache.touch(&c);
                return p.load(&c.to_string_lossy(), &title).map(|_| false);
            }
    }
    p.load(&src, &title)?;
    st.cache.enqueue(job);
    Ok(true)
}

pub(crate) fn local_job(src: &str) -> Option<cache::Job> {
    Some(cache::Job {
        key: cache::local_key(Path::new(src))?,
        lib: src.to_string(),
        source: cache::Source::File(src.into()),
        artist: String::new(),
        title: String::new(),
    })
}

/// Следующее включение начать с секунды `at`; `hold` — загрузить на паузе («Слушать вместе», ядро Whisper).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_cue(st: State<'_, AppState>, at: f64, hold: bool) -> Res<()> {
    st.player()?.cue(if at.is_finite() { at } else { 0.0 }, hold);
    Ok(())
}

/// Скорость воспроизведения (0.25–4).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_rate(st: State<'_, AppState>, rate: f64) -> Res<()> {
    st.player()?.set_speed(if rate.is_finite() { rate } else { 1.0 })
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_pause(st: State<'_, AppState>, paused: bool) -> Res<()> {
    st.player()?.set_pause(paused)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_seek(st: State<'_, AppState>, pos: f64) -> Res<()> {
    st.player()?.seek(pos)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_volume(st: State<'_, AppState>, volume: f64) -> Res<()> {
    st.cfg.lock().unwrap().volume = volume.clamp(0.0, 100.0);
    st.player()?.set_volume(volume)
}

#[derive(Serialize)]
pub(crate) struct Devices {
    /// «Системное» и настоящие устройства (`player::shown_devices`)
    list: Vec<player::ShownDevice>,
    /// остальное из списка mpv («Показать все»)
    more: Vec<player::ShownDevice>,
    current: String,
}

/// Устройства вывода звука и выбранное.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_devices(st: State<'_, AppState>) -> Res<Devices> {
    let (all, current) = st.player()?.devices();
    let (list, more) = player::shown_devices(&all, &current);
    Ok(Devices { list, more, current })
}

/// Играть на устройстве (`auto` — системное по умолчанию); запоминается.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_device(st: State<'_, AppState>, name: String) -> Res<()> {
    st.player()?.set_device(&name)?;
    let mut c = st.cfg.lock().unwrap();
    c.audio_device = (name != "auto").then_some(name);
    c.save()
}

/// Значок в трее, его вид и «Закрывать в трей» (настройки «Общее»). `accent` — цвет темы для вида "accent"
/// (интерфейс присылает его при смене стиля).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn tray_prefs(
    app: AppHandle,
    st: State<'_, AppState>,
    tray: Option<bool>,
    close_to_tray: Option<bool>,
    icon: Option<String>,
    accent: Option<String>,
) -> Res<()> {
    let mut c = st.cfg.lock().unwrap();
    let mut look = false;
    if let Some(icon) = icon {
        if !tray::ICONS.contains(&icon.as_str()) {
            return Err("Неизвестный вид значка".into());
        }
        look |= c.tray_icon != icon;
        c.tray_icon = icon;
    }
    if let Some(accent) = accent {
        if tray::parse_hex(&accent).is_none() {
            return Err("Неверный цвет значка".into());
        }
        look |= c.tray_accent != accent && c.tray_icon == "accent";
        c.tray_accent = accent;
    }
    if look {
        tray::set_look(&app, &c.tray_icon, &c.tray_accent);
    }
    if let Some(on) = tray {
        c.tray = on;
        if on && !host::active() {
            tray::show(&app, &c.tray_icon, &c.tray_accent).map_err(|e| format!("Значок в трее не создался: {e}"))?;
        } else if !on {
            tray::remove(&app);
        }
    }
    if let Some(on) = close_to_tray {
        c.close_to_tray = on;
    }
    c.save()
}

/// «Постоянный уровень громкости»: разные треки — одной громкости (loudness.rs).
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_normalize(st: State<'_, AppState>, on: bool) -> Res<()> {
    st.player()?.set_normalize(on)?;
    st.cfg.lock().unwrap().normalize = on;
    st.save()
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn player_stop(st: State<'_, AppState>) -> Res<()> {
    st.player()?.stop()
}
