//! Добавление треков на диск через yt-dlp (по умолчанию в ~/Music/vk) — в лучшем доступном качестве.
//!
//! Качество: берётся лучшая аудиодорожка (`-f bestaudio/best`) и сохраняется **без перекодирования**
//! (`--audio-format best`): Opus с YouTube остаётся Opus, AAC — AAC, FLAC — FLAC. Перекодирование
//! в mp3 убрано — оно только теряет качество.
//!
//! Источники:
//! * ссылка — всё, что понимает yt-dlp (YouTube, YouTube Music, SoundCloud, Bandcamp…);
//!   ссылка на плейлист или альбом скачивается целиком;
//! * запрос без ссылки — поиск на YouTube, берётся первый результат.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Deserialize)]
pub struct DlRequest {
    pub input: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DlEvent {
    /// название очередного трека (для плейлиста — каждого по очереди)
    Title { id: u64, title: String },
    Progress { id: u64, percent: f64, stage: String },
    /// трек сохранён (для плейлиста приходит на каждый файл)
    Saved { id: u64, path: String, count: u32 },
    Done { id: u64, count: u32 },
    Failed { id: u64, error: String, count: u32 },
}

/// Что установлено: от этого зависит, какие шаги можно включить.
#[derive(Debug, Clone, Serialize)]
pub struct Caps {
    pub ytdlp: Option<String>,
    pub ffmpeg: bool,
    /// python-mutagen нужен yt-dlp, чтобы встроить обложку в Opus/M4A/FLAC
    pub mutagen: bool,
}

const MARK: &str = "VKMPROG";
const FILE_MARK: &str = "VKMFILE";
const TITLE_MARK: &str = "VKMTITLE";

fn run_ok(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn caps() -> &'static Caps {
    static CAPS: OnceLock<Caps> = OnceLock::new();
    CAPS.get_or_init(|| Caps {
        ytdlp: run_ok("yt-dlp", &["--version"]),
        ffmpeg: run_ok("ffmpeg", &["-hide_banner", "-version"]).is_some(),
        mutagen: run_ok("python3", &["-c", "import mutagen"]).is_some(),
    })
}

pub fn args_for(req: &DlRequest, dir: &Path, caps: &Caps) -> Result<Vec<String>, String> {
    let input = req.input.trim();
    if input.is_empty() {
        return Err("Пустая ссылка или запрос".into());
    }
    let mut a: Vec<String> = [
        // лучшая аудиодорожка; если у сайта нет отдельного аудио — лучший общий формат
        "-f",
        "bestaudio/best",
        "--extract-audio",
        // без перекодирования: исходный кодек как есть
        "--audio-format",
        "best",
        "--audio-quality",
        "0",
        "--embed-metadata",
        // исполнитель: у YouTube Music есть artist, у обычных видео — автор канала
        "--parse-metadata",
        "%(artist,creator,uploader|)s:%(meta_artist)s",
        // ссылка на видео внутри плейлиста — только это видео; ссылка на сам плейлист — весь
        "--no-playlist",
        "--newline",
        "--no-colors",
        "--progress",
        "--no-mtime",
        "--trim-filenames",
        "180",
        "-o",
        "%(artist,creator,uploader|Unknown)s - %(track,title)s.%(ext)s",
        "--progress-template",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(format!("download:{MARK} %(progress._percent_str)s"));
    a.push("--print".into());
    a.push(format!("before_dl:{TITLE_MARK} %(artist,creator,uploader|)s — %(track,title)s"));
    a.push("--print".into());
    a.push(format!("after_move:{FILE_MARK} %(filepath)s"));
    if caps.mutagen {
        // обложка в файл; миниатюры YouTube — webp, плееры понимают jpg
        a.push("--embed-thumbnail".into());
        a.push("--convert-thumbnails".into());
        a.push("jpg".into());
    }
    a.push("-P".into());
    a.push(dir.display().to_string());
    a.push("--".into());
    if input.starts_with("http://") || input.starts_with("https://") {
        a.push(input.to_string());
    } else {
        a.push(format!("ytsearch1:{input}"));
    }
    Ok(a)
}

/// Запустить загрузку в фоне. События приходят в `on`.
pub fn spawn<F>(id: u64, req: DlRequest, dir: PathBuf, on: F) -> Result<(), String>
where
    F: Fn(DlEvent) + Send + Sync + 'static,
{
    let caps = caps();
    if caps.ytdlp.is_none() {
        return Err("yt-dlp не найден — установите: sudo pacman -S yt-dlp ffmpeg".into());
    }
    if !caps.ffmpeg {
        return Err("ffmpeg не найден — он нужен yt-dlp, чтобы вынуть аудио: sudo pacman -S ffmpeg".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
    let args = args_for(&req, &dir, caps)?;
    let mut child = Command::new("yt-dlp")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Не удалось запустить yt-dlp: {e}"))?;

    let on = Arc::new(on);
    let count = Arc::new(Mutex::new(0u32));
    let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let mut readers = Vec::new();
    let stdout = child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>);
    let stderr = child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>);
    for stream in [stdout, stderr].into_iter().flatten() {
        let on = on.clone();
        let count = count.clone();
        let errors = errors.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let line = line.trim().to_string();
                if let Some(rest) = line.strip_prefix(MARK) {
                    let pct = rest.trim().trim_end_matches('%').trim().parse::<f64>().unwrap_or(0.0);
                    on(DlEvent::Progress { id, percent: pct, stage: "download".into() });
                } else if let Some(title) = line.strip_prefix(TITLE_MARK) {
                    let title = title.trim().trim_start_matches('—').trim().to_string();
                    on(DlEvent::Title { id, title });
                } else if let Some(path) = line.strip_prefix(FILE_MARK) {
                    let n = {
                        let mut c = count.lock().unwrap();
                        *c += 1;
                        *c
                    };
                    on(DlEvent::Saved { id, path: path.trim().to_string(), count: n });
                } else if line.contains("[ExtractAudio]")
                    || line.contains("[Metadata]")
                    || line.contains("[EmbedThumbnail]")
                    || line.contains("[ThumbnailsConvertor]")
                {
                    on(DlEvent::Progress { id, percent: 100.0, stage: "convert".into() });
                } else if line.starts_with("ERROR") {
                    errors.lock().unwrap().push(line);
                }
            }
        }));
    }

    std::thread::spawn(move || {
        let status = child.wait();
        for r in readers {
            let _ = r.join();
        }
        let n = *count.lock().unwrap();
        let ok = status.as_ref().map(|s| s.success()).unwrap_or(false);
        if ok && n > 0 {
            on(DlEvent::Done { id, count: n });
        } else {
            let errs = errors.lock().unwrap();
            let msg = errs
                .last()
                .cloned()
                .map(|e| e.trim_start_matches("ERROR:").trim().to_string())
                .unwrap_or_else(|| {
                    if ok {
                        "yt-dlp ничего не скачал (нет аудио по этой ссылке?)".into()
                    } else {
                        "yt-dlp завершился с ошибкой".into()
                    }
                });
            on(DlEvent::Failed { id, error: msg, count: n });
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(mutagen: bool) -> Caps {
        Caps { ytdlp: Some("2026.10.01".into()), ffmpeg: true, mutagen }
    }

    #[test]
    fn best_quality_no_reencode() {
        let r = DlRequest { input: "https://music.youtube.com/watch?v=abc".into() };
        let a = args_for(&r, Path::new("/tmp"), &caps(true)).unwrap();
        let pos = |x: &str| a.iter().position(|s| s == x).unwrap();
        assert_eq!(a[pos("-f") + 1], "bestaudio/best");
        assert_eq!(a[pos("--audio-format") + 1], "best");
        assert!(!a.iter().any(|s| s == "mp3"), "не должно быть перекодирования в mp3");
        assert!(a.contains(&"--embed-thumbnail".to_string()));
        assert_eq!(a.last().unwrap(), "https://music.youtube.com/watch?v=abc");
    }

    #[test]
    fn query_and_no_mutagen() {
        let r = DlRequest { input: "Daft Punk One More Time".into() };
        let a = args_for(&r, Path::new("/tmp"), &caps(false)).unwrap();
        assert_eq!(a.last().unwrap(), "ytsearch1:Daft Punk One More Time");
        assert!(!a.contains(&"--embed-thumbnail".to_string()));
        assert!(args_for(&DlRequest { input: "  ".into() }, Path::new("/tmp"), &caps(true)).is_err());
    }
}
