//! Плеер: mpv в фоне, управление через JSON IPC (unix-сокет; на Windows — именованный канал).
//!
//! mpv играет любые локальные форматы (Opus, M4A/AAC, FLAC, MP3, OGG, WAV…) без потерь.
//! Если стоит mpv-mpris, медиаклавиши тоже работают.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Default)]
pub struct PlayerState {
    pub pos: f64,
    pub duration: f64,
    pub paused: bool,
    pub idle: bool,
    pub volume: f64,
    /// номер загруженного файла (растёт на каждом file-loaded): интерфейс отличает состояние нового трека от
    /// запоздавших событий прошлого — иначе «загрузился» и позиция брались от старого трека
    pub file: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerEvent {
    State(PlayerState),
    /// трек доиграл до конца
    Ended,
    /// не удалось открыть/проиграть
    Error { message: String },
}

/// Устройство вывода звука из `audio-device-list` mpv.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AudioDevice {
    /// имя для `audio-device` (например, `pipewire/alsa_output…`; `auto` — системное по умолчанию)
    pub name: String,
    pub description: String,
}

pub struct Player {
    writer: Mutex<ipc::Writer>,
    child: Mutex<Child>,
    /// путь сокета — удалить при выходе (на Windows канал исчезает сам)
    #[cfg_attr(windows, allow(dead_code))]
    sock: PathBuf,
    /// с какой секунды и на паузе ли начать следующий загружаемый трек («Слушать вместе»: гость встаёт на позицию
    /// ведущего и, пока тот грузит трек, готовит его без звука); действует CUE_TTL — иначе случайно достался бы
    /// следующему включению, если это не загрузилось
    cue: Mutex<Option<(f64, bool, Instant)>>,
    /// «Постоянный уровень громкости»: включён ли и что сейчас играет (фильтр зависит от файла)
    norm: Mutex<bool>,
    current: Mutex<String>,
    loud: Arc<crate::loudness::Loudness>,
    /// устройства вывода и текущее (`audio-device`) — mpv присылает их сам (observe_property), в том числе
    /// когда подключили наушники
    devices: Arc<Mutex<(Vec<AudioDevice>, String)>>,
}

const CUE_TTL: Duration = Duration::from_secs(30);

/// Шкала громкости интерфейса: усиление = (v/100)^GAIN_POW. У mpv своя шкала кубическая (усиление = (m/100)³):
/// с ней ползунок на 14% давал −51 дБ — почти тишину; линейная (как `audio.volume`) оказалась слишком громкой.
/// Квадратичная — посередине (просьба владельца, 2026-11-05: «обычная громкость — около 20%»):
/// 14% → −34 дБ, 20% → −28 дБ, 50% → −12 дБ, 100% → 0 дБ.
const GAIN_POW: f64 = 2.0;

/// Громкость интерфейса (0…100) → громкость mpv.
pub fn to_mpv(v: f64) -> f64 {
    100.0 * (v.max(0.0) / 100.0).powf(GAIN_POW / 3.0)
}

/// Обратно: громкость mpv → громкость интерфейса.
pub fn from_mpv(m: f64) -> f64 {
    100.0 * (m.max(0.0) / 100.0).powf(3.0 / GAIN_POW)
}

#[cfg(windows)]
fn sock_path() -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\vkmusic-mpv-{}", std::process::id()))
}

#[cfg(unix)]
fn sock_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("vkmusic-mpv-{}.sock", std::process::id()))
}

impl Player {
    pub fn start<F>(volume: f64, loud: Arc<crate::loudness::Loudness>, on_event: F) -> Result<Player, String>
    where
        F: Fn(PlayerEvent) + Send + 'static,
    {
        let sock = sock_path();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&sock);
        let mut cmd = crate::os::tool("mpv");
        cmd.args([
            "--idle=yes",
            "--no-video",
            "--no-terminal",
            "--really-quiet",
            "--force-window=no",
            "--audio-display=no",
            "--ytdl=no",
            "--keep-open=no",
            "--cache=yes",
            "--cache-on-disk=no",
            // не продолжать трек с места, где его оставили (watch_later из mpv.conf пользователя)
            "--resume-playback=no",
            "--save-position-on-quit=no",
            "--prefetch-playlist=no",
        ])
        .arg(format!("--input-ipc-server={}", sock.display()))
        .arg(format!("--volume={:.3}", to_mpv(volume.clamp(0.0, 100.0))))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        // mpv закроется вместе с приложением, даже если оно упадёт
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "mpv не найден — установите его: sudo pacman -S mpv".to_string()
            } else {
                format!("Не удалось запустить mpv: {e}")
            }
        })?;
        #[cfg(windows)]
        ipc::kill_with_app(&child);

        let started = Instant::now();
        let (writer, reader) = loop {
            match ipc::connect(&sock) {
                Ok(s) => break s,
                Err(_) if started.elapsed() < Duration::from_secs(6) => {
                    if let Ok(Some(st)) = child.try_wait() {
                        return Err(format!("mpv завершился при запуске ({st})"));
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(e) => {
                    let _ = child.kill();
                    return Err(format!("Нет связи с mpv: {e}"));
                }
            }
        };
        let player = Player {
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            sock,
            cue: Mutex::new(None),
            norm: Mutex::new(false),
            current: Mutex::new(String::new()),
            loud,
            devices: Arc::new(Mutex::new((Vec::new(), "auto".into()))),
        };

        let props = [
            (1, "time-pos"),
            (2, "duration"),
            (3, "pause"),
            (4, "idle-active"),
            (5, "volume"),
            (6, "audio-device-list"),
            (7, "audio-device"),
        ];
        for (id, prop) in props {
            player.send(json!(["observe_property", id, prop]))?;
        }
        // сообщения mpv об ошибках — чтобы к «loading failed» дописать причину (HTTP 403, TLS, формат)
        player.send(json!(["request_log_messages", "warn"]))?;

        let devices = player.devices.clone();
        std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || read_events(reader, devices, on_event))
            .map_err(|e| e.to_string())?;
        Ok(player)
    }

    fn send(&self, command: Value) -> Result<(), String> {
        let line = json!({ "command": command }).to_string();
        let mut w = self.writer.lock().unwrap();
        w.write_all(line.as_bytes())
            .and_then(|_| w.write_all(b"\n"))
            .map_err(|e| format!("mpv не отвечает: {e}"))
    }

    /// Следующий трек начать с секунды `at`, `hold` — загрузить на паузе.
    pub fn cue(&self, at: f64, hold: bool) {
        *self.cue.lock().unwrap() = Some((at.max(0.0), hold, Instant::now()));
    }

    /// Играть файл; title — для MPRIS / заголовка. Позиция старта и пауза — до loadfile: start — опция mpv
    /// для следующих файлов (каждый раз задаётся заново), пауза до загрузки — чтобы «без звука» не прозвучало начало.
    pub fn load(&self, src: &str, title: &str) -> Result<(), String> {
        let cue = self.cue.lock().unwrap().take().filter(|c| c.2.elapsed() < CUE_TTL);
        let (at, hold) = cue.map(|c| (c.0, c.1)).unwrap_or((0.0, false));
        self.send(json!(["set_property", "force-media-title", title]))?;
        self.send(json!(["set_property", "start", if at > 0.0 { format!("{at:.3}") } else { "none".into() }]))?;
        self.send(json!(["set_property", "pause", hold]))?;
        *self.current.lock().unwrap() = src.to_string();
        if *self.norm.lock().unwrap() {
            self.send(json!(["set_property", "af", self.norm_filter(src)]))?;
        }
        self.send(json!(["loadfile", src, "replace"]))
    }

    /// Фильтр выравнивания для источника: файл на диске — по замеру (`loudness.rs`), поток — на лету.
    fn norm_filter(&self, src: &str) -> String {
        if src.is_empty() || src.contains("://") {
            return crate::loudness::LIVE_FILTER.into();
        }
        self.loud.filter(std::path::Path::new(src))
    }

    /// Скорость воспроизведения (высота звука сохраняется — scaletempo mpv).
    pub fn set_speed(&self, rate: f64) -> Result<(), String> {
        self.send(json!(["set_property", "speed", rate.clamp(0.25, 4.0)]))
    }

    /// Играть поток по https (ВКонтакте): с User-Agent и Referer браузера, в котором выполнен вход.
    /// На диск поток не пишется — кэш mpv только в памяти (--cache-on-disk=no).
    pub fn load_stream(&self, url: &str, title: &str, user_agent: &str, referrer: &str) -> Result<(), String> {
        if !url.starts_with("https://") && !crate::relay::is_relay(url) {
            return Err("Неверная ссылка на поток".into());
        }
        if !user_agent.is_empty() {
            self.send(json!(["set_property", "user-agent", user_agent]))?;
        }
        self.send(json!(["set_property", "referrer", referrer]))?;
        self.load(url, title)
    }

    /// «Постоянный уровень громкости»: разные треки — одной громкости (−14 LUFS, `loudness.rs`).
    pub fn set_normalize(&self, on: bool) -> Result<(), String> {
        *self.norm.lock().unwrap() = on;
        let cur = self.current.lock().unwrap().clone();
        let af = if on { self.norm_filter(&cur) } else { String::new() };
        self.send(json!(["set_property", "af", af]))
    }

    /// Устройства вывода и текущее.
    pub fn devices(&self) -> (Vec<AudioDevice>, String) {
        self.devices.lock().unwrap().clone()
    }

    /// Играть на устройстве `name` — только из списка mpv (`auto` — системное по умолчанию).
    pub fn set_device(&self, name: &str) -> Result<(), String> {
        if name != "auto" && !self.devices.lock().unwrap().0.iter().any(|d| d.name == name) {
            return Err("Такого устройства вывода нет".into());
        }
        self.send(json!(["set_property", "audio-device", name]))
    }

    /// Устройство из настроек — когда mpv пришлёт список (сразу после запуска его ещё нет); не нашлось за
    /// несколько секунд (наушники отключены) — остаётся системное.
    pub fn restore_device(self: &Arc<Self>, name: String) {
        if name.is_empty() || name == "auto" {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                if !me.devices.lock().unwrap().0.is_empty() {
                    let _ = me.set_device(&name);
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }

    pub fn set_pause(&self, paused: bool) -> Result<(), String> {
        self.send(json!(["set_property", "pause", paused]))
    }

    pub fn seek(&self, pos: f64) -> Result<(), String> {
        self.send(json!(["seek", pos.max(0.0), "absolute"]))
    }

    pub fn set_volume(&self, v: f64) -> Result<(), String> {
        self.send(json!(["set_property", "volume", to_mpv(v.clamp(0.0, 130.0))]))
    }

    pub fn stop(&self) -> Result<(), String> {
        self.send(json!(["stop"]))
    }

    pub fn quit(&self) {
        let _ = self.send(json!(["quit"]));
        let mut c = self.child.lock().unwrap();
        std::thread::sleep(Duration::from_millis(80));
        let _ = c.kill();
        let _ = c.wait();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.sock);
    }
}

/// Связь с mpv: на Linux — unix-сокет (отдельные дескрипторы на чтение и запись).
#[cfg(unix)]
mod ipc {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    use std::path::Path;

    pub type Writer = UnixStream;

    pub fn connect(sock: &Path) -> std::io::Result<(Writer, impl Iterator<Item = String> + Send + 'static)> {
        let w = UnixStream::connect(sock)?;
        let r = w.try_clone()?;
        Ok((w, BufReader::new(r).lines().map_while(Result::ok)))
    }
}

/// Связь с mpv на Windows: именованный канал. Синхронный дескриптор канала не даёт писать, пока поток событий
/// ждёт чтения, поэтому канал асинхронный (tokio): запись — через очередь, строки событий — в канал std.
#[cfg(windows)]
mod ipc {
    use std::path::Path;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::sync::mpsc;

    pub struct Writer(mpsc::UnboundedSender<Vec<u8>>);

    impl std::io::Write for Writer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.send(buf.to_vec()).map_err(|_| std::io::ErrorKind::BrokenPipe)?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub fn connect(sock: &Path) -> std::io::Result<(Writer, impl Iterator<Item = String> + Send + 'static)> {
        let rt = tauri::async_runtime::handle();
        let pipe = {
            let _ctx = rt.inner().enter();
            ClientOptions::new().open(sock)?
        };
        let (rd, mut wr) = tokio::io::split(pipe);
        let (wtx, mut wrx) = mpsc::unbounded_channel::<Vec<u8>>();
        rt.spawn(async move {
            while let Some(buf) = wrx.recv().await {
                if wr.write_all(&buf).await.is_err() {
                    break;
                }
            }
        });
        let (ltx, lrx) = std::sync::mpsc::channel::<String>();
        rt.spawn(async move {
            let mut lines = BufReader::new(rd).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if ltx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok((Writer(wtx), lrx.into_iter()))
    }

    /// mpv — в задание (Job Object) с KILL_ON_JOB_CLOSE: задание закрывается вместе с приложением,
    /// даже если оно упало, — и mpv завершается (аналог PR_SET_PDEATHSIG на Linux).
    pub fn kill_with_app(child: &std::process::Child) {
        use std::os::windows::io::AsRawHandle;
        use std::sync::OnceLock;
        use windows_sys::Win32::System::JobObjects::*;
        static JOB: OnceLock<usize> = OnceLock::new();
        let job = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            job as usize
        });
        if job != 0 {
            unsafe {
                AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
            }
        }
    }
}

/// Строка журнала mpv как причина ошибки: без ссылок (в них подписи и адрес), не длиннее 160 символов.
/// Шум, не связанный с загрузкой (теги, обложки, хуки скриптов), пропускается.
fn log_reason(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || ["hook", "auto_profiles", "Cover art", "metadata", "tag"].iter().any(|n| t.contains(n)) {
        return None;
    }
    let clean: Vec<&str> = t.split_whitespace().map(|w| if w.contains("://") { "…" } else { w }).collect();
    Some(clean.join(" ").chars().take(160).collect())
}

/// Причина для сообщения: первая и последняя строки журнала (если разные).
fn reason_of(why: &[String]) -> Option<String> {
    match why {
        [] => None,
        [one] => Some(one.clone()),
        [first, .., last] => Some(format!("{first}; {last}")),
    }
}

/// Устройство для меню «Устройство вывода»: понятное имя и группа (звуковая карта).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ShownDevice {
    /// имя для `audio-device`
    pub name: String,
    /// подпись без названия карты («Headphones», «HDMI / DisplayPort 1 Output»)
    pub label: String,
    /// название карты, общее для нескольких устройств (заголовок группы); None — подпись и так полная
    pub group: Option<String>,
}

/// Список mpv для меню. mpv перечисляет каждое устройство в каждой звуковой системе (pipewire, pulse, alsa, jack…)
/// и ещё плагины ALSA — на обычном ноутбуке это 30–40 строк, большинство из них — то же самое или не устройства.
/// Основной список — «Системное» и настоящие устройства той системы, что у mpv первая (её mpv и использует при
/// `auto`; на Linux обычно pipewire, на Windows — wasapi) и выбранное, если оно из другой. Остальное — `more`
/// («Показать все»). В основном списке общее начало названий одной карты выносится в заголовок группы.
pub fn shown_devices(list: &[AudioDevice], current: &str) -> (Vec<ShownDevice>, Vec<ShownDevice>) {
    let backend = |n: &str| n.split_once('/').map(|x| x.0.to_string());
    let primary = list.iter().find_map(|d| backend(&d.name));
    let in_main = |d: &AudioDevice| d.name == current || (primary.is_some() && backend(&d.name) == primary);
    let mut main: Vec<&AudioDevice> = list.iter().filter(|d| d.name != "auto" && in_main(d)).collect();
    let more = list
        .iter()
        .filter(|d| d.name != "auto" && !in_main(d))
        .map(|d| {
            let b = backend(&d.name).unwrap_or_else(|| d.name.clone());
            let label = if d.description.to_lowercase().contains(&b) { d.description.clone() } else { format!("{} · {b}", d.description) };
            ShownDevice { name: d.name.clone(), label, group: None }
        })
        .collect();
    // группы — по первому слову названия (карта); у групп из двух и больше — общее начало до границы слова
    let first = |d: &AudioDevice| d.description.split_whitespace().next().unwrap_or("").to_string();
    main.sort_by_key(|d| first(d)); // стабильно: внутри карты — порядок mpv, затем по подписи
    let mut shown = vec![ShownDevice { name: "auto".into(), label: String::new(), group: None }];
    let mut i = 0;
    while i < main.len() {
        let key = first(main[i]);
        let j = main[i..].iter().position(|d| first(d) != key).map_or(main.len(), |p| i + p);
        let descs: Vec<&str> = main[i..j].iter().map(|d| d.description.as_str()).collect();
        let cut = if descs.len() > 1 { common_prefix(&descs) } else { 0 };
        let mut group: Vec<ShownDevice> = main[i..j]
            .iter()
            .map(|d| {
                let rest = d.description[cut..].trim();
                let label = rest.strip_suffix(" (Stereo)").unwrap_or(rest).trim().to_string();
                let group = (cut > 0).then(|| d.description[..cut].trim().trim_end_matches(['-', '/', ',']).trim().to_string());
                ShownDevice { name: d.name.clone(), label, group }
            })
            .collect();
        group.sort_by(|a, b| a.label.cmp(&b.label));
        shown.extend(group);
        i = j;
    }
    (shown, more)
}

/// Длина общего начала строк до границы слова (после пробела); 0 — если у какой-то строки после него ничего не остаётся.
fn common_prefix(s: &[&str]) -> usize {
    let mut n = s[0].len();
    for x in &s[1..] {
        n = s[0].bytes().zip(x.bytes()).take(n).take_while(|(a, b)| a == b).count();
    }
    while !s[0].is_char_boundary(n) {
        n -= 1; // общие байты кончились посреди буквы (кириллица)
    }
    let head = &s[0][..n];
    let cut = if s.iter().all(|x| x.len() > n && x.as_bytes()[n] == b' ') { n } else { head.rfind(' ').map_or(0, |p| p + 1) };
    if cut < 4 || s.iter().any(|x| x[cut..].trim().is_empty()) { 0 } else { cut }
}

/// `audio-device-list` mpv → устройства.
fn parse_devices(v: Option<&Value>) -> Vec<AudioDevice> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    let name = d.get("name")?.as_str()?.to_string();
                    let description = d.get("description").and_then(Value::as_str).unwrap_or(&name).to_string();
                    Some(AudioDevice { name, description })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_events<F: Fn(PlayerEvent)>(
    lines: impl Iterator<Item = String>,
    devices: Arc<Mutex<(Vec<AudioDevice>, String)>>,
    on_event: F,
) {
    let mut st = PlayerState { idle: true, ..Default::default() };
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    // ошибки из журнала mpv для текущего файла (первые — обычно причина: «HTTP error 403», последние — «Failed to open»)
    let mut why: Vec<String> = Vec::new();
    // последние значения от mpv: между start-file и file-loaded наружу уходят нули (позиция прошлого трека
    // уже не в счёт), на file-loaded — то, что mpv успел сообщить о новом файле
    let (mut raw_pos, mut raw_dur, mut loading) = (0.0, 0.0, false);
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v.get("event").and_then(Value::as_str) {
            Some("property-change") => {
                let data = v.get("data");
                let f = data.and_then(Value::as_f64);
                let b = data.and_then(Value::as_bool);
                let mut urgent = true;
                match v.get("name").and_then(Value::as_str) {
                    Some("audio-device-list") => {
                        devices.lock().unwrap().0 = parse_devices(data);
                        continue;
                    }
                    Some("audio-device") => {
                        devices.lock().unwrap().1 = data.and_then(Value::as_str).unwrap_or("auto").to_string();
                        continue;
                    }
                    Some("time-pos") => {
                        raw_pos = f.unwrap_or(0.0);
                        st.pos = if loading { 0.0 } else { raw_pos };
                        urgent = false;
                    }
                    Some("duration") => {
                        raw_dur = f.unwrap_or(0.0);
                        st.duration = if loading { 0.0 } else { raw_dur };
                    }
                    Some("pause") => st.paused = b.unwrap_or(false),
                    Some("idle-active") => st.idle = b.unwrap_or(false),
                    // громкость задаёт сам интерфейс: каждый шаг ползунка не должен гонять полное состояние
                    // плеера (кнопки, время, отчёты ядру) — уходит вместе с ближайшей позицией
                    Some("volume") => {
                        st.volume = f.map(from_mpv).unwrap_or(st.volume);
                        urgent = false;
                    }
                    _ => continue,
                }
                if urgent || last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    on_event(PlayerEvent::State(st.clone()));
                }
            }
            Some("start-file") => {
                why.clear();
                loading = true;
                st.pos = 0.0;
                st.duration = 0.0;
                on_event(PlayerEvent::State(st.clone()));
            }
            Some("file-loaded") => {
                loading = false;
                st.file += 1;
                st.pos = raw_pos;
                st.duration = raw_dur;
                on_event(PlayerEvent::State(st.clone()));
            }
            Some("log-message") => {
                let level = v.get("level").and_then(Value::as_str).unwrap_or("");
                let text = v.get("text").and_then(Value::as_str).unwrap_or("");
                if matches!(level, "error" | "fatal" | "warn") {
                    if let Some(t) = log_reason(text) {
                        if !why.contains(&t) && why.len() < 8 {
                            why.push(t);
                        }
                    }
                }
            }
            Some("end-file") => match v.get("reason").and_then(Value::as_str) {
                Some("eof") => on_event(PlayerEvent::Ended),
                Some("error") => {
                    let mut msg = v
                        .get("file_error")
                        .and_then(Value::as_str)
                        .unwrap_or("не удалось воспроизвести")
                        .to_string();
                    if let Some(w) = reason_of(&why) {
                        msg = format!("{msg} ({w})");
                    }
                    why.clear();
                    on_event(PlayerEvent::Error { message: msg });
                }
                _ => {}
            },
            _ => {}
        }
    }
    on_event(PlayerEvent::Error { message: "mpv остановился — перезапустите приложение".into() });
}

#[cfg(test)]
mod tests {
    use super::{from_mpv, to_mpv};

    #[test]
    fn log_reason_hides_links() {
        assert_eq!(
            super::log_reason("HTTP error 403 Forbidden\n").as_deref(),
            Some("HTTP error 403 Forbidden")
        );
        assert_eq!(
            super::log_reason("Failed to open https://vd1.okcdn.ru/?sig=abc.").as_deref(),
            Some("Failed to open …")
        );
        assert_eq!(super::log_reason("Failed sending hook command auto_profiles/on_load."), None);
    }

    #[test]
    fn devices_shown() {
        let dev = |name: &str, description: &str| super::AudioDevice { name: name.into(), description: description.into() };
        let cav = "pipewire/alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__";
        let list = vec![
            dev("auto", "Autoselect device"),
            dev("pipewire", "Default (pipewire)"),
            dev(&format!("{cav}HDMI2__sink"), "Raptor Lake-P/U/H cAVS HDMI / DisplayPort 2 Output (Stereo)"),
            dev(&format!("{cav}HDMI1__sink"), "Raptor Lake-P/U/H cAVS HDMI / DisplayPort 1 Output (Stereo)"),
            dev(&format!("{cav}Headphones__sink"), "Raptor Lake-P/U/H cAVS Headphones"),
            dev("pipewire/bluez_output.AA_BB", "WH-1000XM4"),
            dev("pulse/alsa_output.x.HiFi__Headphones__sink", "Raptor Lake-P/U/H cAVS Headphones"),
            dev("alsa", "Default (alsa)"),
            dev("alsa/speex", "Plugin using Speex DSP (resample, agc, denoise, echo, dereverb)"),
            dev("alsa/hdmi:CARD=NVidia,DEV=0", "HDA NVidia, HDMI 0/HDMI Audio Output"),
            dev("jack", "Default (jack)"),
        ];
        let (main, more) = super::shown_devices(&list, "alsa/hdmi:CARD=NVidia,DEV=0");
        let labels: Vec<(&str, Option<&str>)> = main.iter().map(|d| (d.label.as_str(), d.group.as_deref())).collect();
        let card = Some("Raptor Lake-P/U/H cAVS");
        assert_eq!(
            labels,
            [
                ("", None),
                ("HDA NVidia, HDMI 0/HDMI Audio Output", None), // выбранное — в основном списке, хоть и из alsa
                ("HDMI / DisplayPort 1 Output", card),
                ("HDMI / DisplayPort 2 Output", card),
                ("Headphones", card),
                ("WH-1000XM4", None),
            ]
        );
        assert_eq!(main[0].name, "auto");
        let more: Vec<&str> = more.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(
            more,
            [
                "Default (pipewire)",
                "Raptor Lake-P/U/H cAVS Headphones · pulse",
                "Default (alsa)",
                "Plugin using Speex DSP (resample, agc, denoise, echo, dereverb) · alsa",
                "Default (jack)",
            ]
        );
        // одна система без устройств (только «auto» и общие) — основной список из одного «Системного»
        // кириллица: общее начало кончается посреди буквы («Аналоговый» / «Аналоговая» — общие байты до «ы»/«а»)
        let ru = [dev("pipewire/a", "Встроенное аудио Аналоговый стерео"), dev("pipewire/b", "Встроенное аудио Аналоговая гарнитура")];
        let (main, _) = super::shown_devices(&ru, "auto");
        assert_eq!(main[1].label, "Аналоговая гарнитура");
        assert_eq!(main[1].group.as_deref(), Some("Встроенное аудио"));
        let (main, more) = super::shown_devices(&[dev("auto", "A"), dev("sdl", "Default (sdl)")], "auto");
        assert_eq!(main.len(), 1);
        assert_eq!(more.len(), 1);
    }

    #[test]
    fn devices_from_mpv() {
        let v = serde_json::json!([
            { "name": "auto", "description": "Autoselect device" },
            { "name": "pipewire/alsa_output.usb", "description": "USB наушники" },
            { "description": "без имени" }
        ]);
        let d = super::parse_devices(Some(&v));
        assert_eq!(d.len(), 2);
        assert_eq!(d[1].description, "USB наушники");
        assert!(super::parse_devices(None).is_empty());
    }

    #[test]
    fn volume_scale() {
        // усиление mpv = (m/100)³ — должно быть (v/100)²
        for v in [0.0, 14.0, 20.0, 50.0, 100.0] {
            let gain = (to_mpv(v) / 100.0).powi(3);
            assert!((gain - (v / 100.0).powi(2)).abs() < 1e-9, "{v}");
            assert!((from_mpv(to_mpv(v)) - v).abs() < 1e-9);
        }
    }
}
