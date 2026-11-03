//! Плеер: mpv в фоне, управление через JSON IPC (unix-сокет).
//!
//! mpv играет любые локальные форматы (Opus, M4A/AAC, FLAC, MP3, OGG, WAV…) без потерь.
//! Если стоит mpv-mpris, медиаклавиши тоже работают.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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
    writer: Mutex<UnixStream>,
    child: Mutex<Child>,
    sock: PathBuf,
    /// с какой секунды и на паузе ли начать следующий загружаемый трек («Слушать вместе»: гость встаёт на позицию
    /// ведущего и, пока тот грузит трек, готовит его без звука); действует CUE_TTL — иначе случайно достался бы
    /// следующему включению, если это не загрузилось
    cue: Mutex<Option<(f64, bool, Instant)>>,
}

const CUE_TTL: Duration = Duration::from_secs(30);

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
        let _ = std::fs::remove_file(&sock);
        let mut cmd = Command::new("mpv");
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
        .arg(format!("--volume={}", volume.clamp(0.0, 100.0)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        // mpv закроется вместе с приложением, даже если оно упадёт
        unsafe {
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

        let started = Instant::now();
        let stream = loop {
            match UnixStream::connect(&sock) {
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
        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let player = Player { writer: Mutex::new(stream), child: Mutex::new(child), sock, cue: Mutex::new(None) };

        for (id, prop) in [(1, "time-pos"), (2, "duration"), (3, "pause"), (4, "idle-active"), (5, "volume")] {
            player.send(json!(["observe_property", id, prop]))?;
        }

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
        self.send(json!(["set_property", "volume", v.clamp(0.0, 130.0)]))
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
        let _ = std::fs::remove_file(&self.sock);
    }
}

fn read_events<F: Fn(PlayerEvent)>(reader: UnixStream, on_event: F) {
    let mut st = PlayerState { idle: true, ..Default::default() };
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let rd = BufReader::new(reader);
    for line in rd.lines() {
        let Ok(line) = line else { break };
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
                    Some("volume") => st.volume = f.unwrap_or(st.volume),
                    _ => continue,
                }
                if urgent || last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    on_event(PlayerEvent::State(st.clone()));
                }
            }
            Some("end-file") => match v.get("reason").and_then(Value::as_str) {
                Some("eof") => on_event(PlayerEvent::Ended),
                Some("error") => {
                    let msg = v
                        .get("file_error")
                        .and_then(Value::as_str)
                        .unwrap_or("не удалось воспроизвести")
                        .to_string();
                    on_event(PlayerEvent::Error { message: msg });
                }
                _ => {}
            },
            _ => {}
        }
    }
    on_event(PlayerEvent::Error { message: "mpv остановился — перезапустите приложение".into() });
}
