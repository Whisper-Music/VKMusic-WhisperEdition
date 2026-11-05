//! Плеер: mpv в фоне, управление через JSON IPC (unix-сокет; на Windows — именованный канал).
//!
//! mpv играет любые локальные форматы (Opus, M4A/AAC, FLAC, MP3, OGG, WAV…) без потерь.
//! Если стоит mpv-mpris, медиаклавиши тоже работают.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Default)]
pub struct PlayerState {
    pub pos: f64,
    pub duration: f64,
    pub paused: bool,
    pub idle: bool,
    pub volume: f64,
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
}

const CUE_TTL: Duration = Duration::from_secs(30);

/// Громкость интерфейса (0…100, линейная амплитуда — как `audio.volume` в Whisper Yandex) → громкость mpv.
/// У mpv шкала кубическая (усиление = (v/100)³): ползунок на 14% давал −51 дБ — почти тишину. Поэтому
/// mpv получает кубический корень: 14% → усиление 0,14 (−17 дБ), 50% → 0,5 (−6 дБ), как в браузере.
pub fn to_mpv(v: f64) -> f64 {
    100.0 * (v.max(0.0) / 100.0).cbrt()
}

/// Обратно: громкость mpv → громкость интерфейса.
pub fn from_mpv(m: f64) -> f64 {
    100.0 * (m.max(0.0) / 100.0).powi(3)
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
    pub fn start<F>(volume: f64, on_event: F) -> Result<Player, String>
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
        let player = Player { writer: Mutex::new(writer), child: Mutex::new(child), sock, cue: Mutex::new(None) };

        for (id, prop) in [(1, "time-pos"), (2, "duration"), (3, "pause"), (4, "idle-active"), (5, "volume")] {
            player.send(json!(["observe_property", id, prop]))?;
        }
        // сообщения mpv об ошибках — чтобы к «loading failed» дописать причину (HTTP 403, TLS, формат)
        player.send(json!(["request_log_messages", "warn"]))?;

        std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || read_events(reader, on_event))
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
        self.send(json!(["loadfile", src, "replace"]))
    }

    /// Скорость воспроизведения (высота звука сохраняется — scaletempo mpv).
    pub fn set_speed(&self, rate: f64) -> Result<(), String> {
        self.send(json!(["set_property", "speed", rate.clamp(0.25, 4.0)]))
    }

    /// Играть поток по https (ВКонтакте): с User-Agent и Referer браузера, в котором выполнен вход.
    /// На диск поток не пишется — кэш mpv только в памяти (--cache-on-disk=no).
    pub fn load_stream(&self, url: &str, title: &str, user_agent: &str, referrer: &str) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err("Неверная ссылка на поток".into());
        }
        if !user_agent.is_empty() {
            self.send(json!(["set_property", "user-agent", user_agent]))?;
        }
        self.send(json!(["set_property", "referrer", referrer]))?;
        self.load(url, title)
    }

    /// Выравнивание громкости: тихие и громкие треки звучат одинаково (фильтр dynaudnorm).
    pub fn set_normalize(&self, on: bool) -> Result<(), String> {
        let af = if on { "lavfi=[dynaudnorm=f=250:g=15:p=0.9:m=10]" } else { "" };
        self.send(json!(["set_property", "af", af]))
    }

    pub fn toggle(&self) -> Result<(), String> {
        self.send(json!(["cycle", "pause"]))
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

fn read_events<F: Fn(PlayerEvent)>(lines: impl Iterator<Item = String>, on_event: F) {
    let mut st = PlayerState { idle: true, ..Default::default() };
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    // последняя ошибка из журнала mpv для текущего файла
    let mut why: Option<String> = None;
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v.get("event").and_then(Value::as_str) {
            Some("property-change") => {
                let data = v.get("data");
                let f = data.and_then(Value::as_f64);
                let b = data.and_then(Value::as_bool);
                let mut urgent = true;
                match v.get("name").and_then(Value::as_str) {
                    Some("time-pos") => {
                        st.pos = f.unwrap_or(0.0);
                        urgent = false;
                    }
                    Some("duration") => st.duration = f.unwrap_or(0.0),
                    Some("pause") => st.paused = b.unwrap_or(false),
                    Some("idle-active") => st.idle = b.unwrap_or(false),
                    Some("volume") => st.volume = f.map(from_mpv).unwrap_or(st.volume),
                    _ => continue,
                }
                if urgent || last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    on_event(PlayerEvent::State(st.clone()));
                }
            }
            Some("start-file") => why = None,
            Some("log-message") => {
                let level = v.get("level").and_then(Value::as_str).unwrap_or("");
                let text = v.get("text").and_then(Value::as_str).unwrap_or("");
                if matches!(level, "error" | "fatal" | "warn") {
                    if let Some(t) = log_reason(text) {
                        why = Some(t);
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
                    if let Some(w) = why.take() {
                        msg = format!("{msg} ({w})");
                    }
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
    fn volume_is_linear_amplitude() {
        // усиление mpv = (v/100)³ — должно совпасть с долей ползунка
        for v in [0.0, 14.0, 50.0, 100.0] {
            let gain = (to_mpv(v) / 100.0).powi(3);
            assert!((gain - v / 100.0).abs() < 1e-9, "{v}");
            assert!((from_mpv(to_mpv(v)) - v).abs() < 1e-9);
        }
    }
}
