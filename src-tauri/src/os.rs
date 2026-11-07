//! Отличия Linux и Windows в одном месте: права на файлы настроек, запуск внешних программ
//! (mpv, ffmpeg, ffprobe, yt-dlp), открытие папок и ссылок.
//!
//! На Windows программы лежат в папке `bin` рядом с `vkmusic.exe` (их кладёт установщик, см. release.yml),
//! запускаются без консольного окна; на Linux — из PATH (пакеты дистрибутива).

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

/// Файл только для владельца (0600 на Linux; на Windows права даёт папка профиля пользователя).
pub trait Private {
    fn private(&mut self) -> &mut Self;
}

impl Private for std::fs::OpenOptions {
    #[cfg(unix)]
    fn private(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.mode(0o600)
    }
    #[cfg(not(unix))]
    fn private(&mut self) -> &mut Self {
        self
    }
}

/// Папка с программами, которые поставляются вместе с приложением (только Windows).
pub fn bin_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let dir = std::env::current_exe().ok()?.parent()?.join("bin");
    dir.is_dir().then_some(dir)
}

/// Путь к программе: своя из `bin_dir`, иначе — имя для поиска в PATH.
pub fn tool_path(name: &str) -> PathBuf {
    if let Some(p) = bin_dir().map(|d| d.join(format!("{name}.exe"))).filter(|p| p.is_file()) {
        return p;
    }
    PathBuf::from(name)
}

/// Команда для внешней программы (на Windows — без консольного окна).
pub fn tool(name: &str) -> Command {
    let mut cmd = Command::new(tool_path(name));
    hide_console(&mut cmd);
    cmd
}

#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_: &mut Command) {}

/// Открыть папку или ссылку программой по умолчанию.
pub fn open(target: impl AsRef<OsStr>) -> Result<(), String> {
    #[cfg(windows)]
    let (prog, mut cmd) = ("explorer", Command::new("explorer"));
    #[cfg(not(windows))]
    let (prog, mut cmd) = ("xdg-open", Command::new("xdg-open"));
    cmd.arg(target).spawn().map(|_| ()).map_err(|e| format!("{prog}: {e}"))
}

/// Подождать, пока процесс `pid` завершится (не дольше `max`). Нужно перезапуску (`--host` в уже открытое окно,
/// main.rs): новый экземпляр должен стартовать, когда прежний уже вышел, — иначе single-instance отдал бы запуск ему.
#[cfg(unix)]
pub fn wait_exit(pid: u32, max: std::time::Duration) {
    let t = std::time::Instant::now();
    let proc_dir = PathBuf::from(format!("/proc/{pid}"));
    while proc_dir.exists() && t.elapsed() < max {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(windows)]
pub fn wait_exit(pid: u32, max: std::time::Duration) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: дескриптор только на ожидание; закрывается здесь же
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return; // процесса уже нет
        }
        WaitForSingleObject(h, max.as_millis().min(u32::MAX as u128) as u32);
        CloseHandle(h);
    }
}
