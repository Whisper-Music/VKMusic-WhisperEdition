//! Визуализатор VK Микса на компьютере (просьба владельца, 2026-11-15; рисует viz.js, как у Whisper Yandex).
//!
//! Звук играет mpv, поэтому спектр берётся у него: в конец цепочки `af` (после выравнивания и эквалайзера)
//! добавляется фильтр `@vkviz` — ветка анализа, которая в звук не возвращается: моно → 32 кГц → порции по 1024
//! отсчёта (~31 замер в секунду) → 24 полосы `bandpass` (40 Гц … 12 кГц, по логарифму) → `join` в 24 канала →
//! `astats` (RMS каждого канала, дБ) → `ametadata` печатает замеры в именованный канал (FIFO в $XDG_RUNTIME_DIR).
//! Поток здесь читает их, ставит в очередь по времени (`pts_time`) и отдаёт интерфейсу тот замер, время которого
//! дошло до позиции воспроизведения (фильтр работает раньше, чем звук слышен, — на размер буфера mpv).
//!
//! Фильтр в цепочке, пока визуализатор включён в настройках (`enabled`; включили/выключили — `af` собирается
//! заново, короткая заминка), а события идут, только пока его видно (`live`: страница VK Микса, окно на экране).
//! Только unix: FIFO; на Windows визуализатор рисует спокойную анимацию без данных.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Полос — столько же считает Android (VkmPlugin, Engine.viz); viz.js раскладывает их по частотам.
pub const BANDS: usize = 24;
const LO: f64 = 40.0;
const HI: f64 = 12000.0;

/// Центры полос, Гц (геометрическая прогрессия от LO до HI).
pub fn centers() -> Vec<f64> {
    let r = (HI / LO).powf(1.0 / (BANDS as f64 - 1.0));
    (0..BANDS).map(|i| LO * r.powi(i as i32)).collect()
}

struct Clock {
    pos: f64,
    at: Instant,
    paused: bool,
}

pub struct Viz {
    path: PathBuf,
    enabled: AtomicBool,
    live: AtomicBool,
    clock: Mutex<Clock>,
    /// замеры, ещё не дошедшие до позиции: (время, уровни дБ)
    frames: Mutex<VecDeque<(f64, Vec<i16>)>>,
}

impl Viz {
    /// Канал и потоки: чтение замеров и отдача их по позиции. `on_frame` — уровни 24 полос, дБ (−100…0).
    pub fn start(on_frame: impl Fn(Vec<i16>) + Send + 'static) -> Option<Arc<Viz>> {
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = base.join(format!("vkmusic-viz-{}.fifo", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let c = std::ffi::CString::new(path.to_string_lossy().as_bytes()).ok()?;
        // SAFETY: путь — строка без нулей; права 0600
        if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
            eprintln!("vkmusic: визуализатор: не удалось создать канал {}", path.display());
            return None;
        }
        let viz = Arc::new(Viz {
            path,
            enabled: AtomicBool::new(false),
            live: AtomicBool::new(false),
            clock: Mutex::new(Clock { pos: 0.0, at: Instant::now(), paused: true }),
            frames: Mutex::new(VecDeque::new()),
        });
        // чтение: канал открыт на чтение и запись — писатель (ffmpeg в mpv) открывает его при каждой сборке `af`
        // и закрывает при следующей, а у нас конца файла не бывает
        let me = viz.clone();
        std::thread::Builder::new()
            .name("viz-read".into())
            .spawn(move || {
                let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&me.path) else { return };
                let mut cur: Option<(f64, Vec<i16>)> = None;
                for line in BufReader::new(f).lines() {
                    let Ok(line) = line else { break };
                    if let Some(t) = line.strip_prefix("frame:") {
                        if let Some(done) = cur.take() {
                            me.push(done);
                        }
                        let pts = t.split("pts_time:").nth(1).and_then(|s| s.trim().parse::<f64>().ok());
                        cur = pts.map(|p| (p, vec![-100; BANDS]));
                    } else if let (Some((_, levels)), Some(rest)) = (cur.as_mut(), line.strip_prefix("lavfi.astats.")) {
                        // «N.RMS_level=-23.4» (N — номер канала-полосы с 1); тишина — «-inf»
                        let Some((n, v)) = rest.split_once(".RMS_level=") else { continue };
                        let (Ok(n), v) = (n.parse::<usize>(), v.trim()) else { continue };
                        let db = v.parse::<f64>().ok().filter(|d| d.is_finite()).unwrap_or(-100.0);
                        if (1..=BANDS).contains(&n) {
                            levels[n - 1] = db.clamp(-100.0, 0.0).round() as i16;
                        }
                    }
                }
            })
            .ok()?;
        // отдача: ~30 раз в секунду — последний замер, время которого уже наступило
        let me = viz.clone();
        std::thread::Builder::new()
            .name("viz-emit".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(33));
                if !me.live.load(Relaxed) {
                    continue;
                }
                let now = {
                    let c = me.clock.lock().unwrap();
                    if c.paused {
                        continue;
                    }
                    c.pos + c.at.elapsed().as_secs_f64().min(1.0)
                };
                let mut q = me.frames.lock().unwrap();
                let mut last = None;
                while q.front().is_some_and(|f| f.0 <= now) {
                    last = q.pop_front();
                }
                drop(q);
                if let Some((_, b)) = last {
                    on_frame(b);
                }
            })
            .ok()?;
        Some(viz)
    }

    fn push(&self, f: (f64, Vec<i16>)) {
        let mut q = self.frames.lock().unwrap();
        // перемотка назад или новый трек: время пошло заново — прежние замеры не годятся
        if q.back().is_some_and(|b| f.0 < b.0 - 0.25) {
            q.clear();
        }
        q.push_back(f);
        while q.len() > 300 {
            q.pop_front();
        }
    }

    /// Позиция и пауза от mpv (time-pos, pause).
    pub fn clock(&self, pos: Option<f64>, paused: Option<bool>) {
        let mut c = self.clock.lock().unwrap();
        if let Some(p) = pos {
            c.pos = p;
            c.at = Instant::now();
        }
        if let Some(p) = paused {
            if c.paused && !p {
                c.at = Instant::now();
            }
            c.paused = p;
        }
    }

    /// Новый файл: очередь — с нуля.
    pub fn reset(&self) {
        self.frames.lock().unwrap().clear();
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Relaxed)
    }

    /// Включён ли в настройках; true — фильтр изменился (цепочку `af` собрать заново).
    pub fn set_enabled(&self, on: bool) -> bool {
        self.enabled.swap(on, Relaxed) != on
    }

    pub fn set_live(&self, on: bool) {
        self.live.store(on, Relaxed);
    }

    /// Фильтр для `af` (или None, если выключен).
    pub fn filter(&self) -> Option<String> {
        if !self.enabled() {
            return None;
        }
        Some(graph(&self.path.to_string_lossy()))
    }

    pub fn quit(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Граф ветки анализа: звук проходит насквозь (`[o]anull`), копия — в полосы и замеры.
pub fn graph(out: &str) -> String {
    let r = (HI / LO).powf(1.0 / (BANDS as f64 - 1.0));
    let width = r.log2();
    let labels: String = (0..BANDS).map(|i| format!("[b{i}]")).collect();
    let mut parts = vec!["asplit[o][an]".to_string(), format!("[an]aformat=channel_layouts=mono,aresample=32000,asetnsamples=n=1024:p=0,asplit={BANDS}{labels}")];
    for (i, f) in centers().iter().enumerate() {
        parts.push(format!("[b{i}]bandpass=f={:.0}:width_type=o:w={width:.3}[c{i}]", f));
    }
    let inputs: String = (0..BANDS).map(|i| format!("[c{i}]")).collect();
    parts.push(format!(
        "{inputs}join=inputs={BANDS}:channel_layout=22.2,astats=metadata=1:reset=1:measure_perchannel=RMS_level:measure_overall=none,ametadata=mode=print:file={out}:direct=1,anullsink"
    ));
    parts.push("[o]anull".into());
    format!("@vkviz:lavfi=[{}]", parts.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_shape() {
        let g = graph("/run/user/1000/vkmusic-viz-1.fifo");
        assert!(g.starts_with("@vkviz:lavfi=[asplit[o][an];"));
        assert_eq!(g.matches("bandpass=").count(), BANDS);
        assert!(g.contains("join=inputs=24:channel_layout=22.2"));
        assert!(g.ends_with("[o]anull]"));
        let c = centers();
        assert_eq!(c.len(), BANDS);
        assert!((c[0] - 40.0).abs() < 0.01 && (c[BANDS - 1] - 12000.0).abs() < 1.0);
    }
}
