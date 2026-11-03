//! Секреты приложения — сейчас один: API-ключ Whisper Archive (wa_…).
//!
//! * Обычно — Secret Service (GNOME Keyring, KWallet, KeePassXC…) через крейт keyring.
//! * Если «связки ключей» в системе нет (минимальные оконные менеджеры), ключ кладётся в файл
//!   `~/.config/vkmusic/wa_token.secret` с правами 0600. Это открытый текст на диске, поэтому тихо это не происходит:
//!   `secret_backend` сообщает интерфейсу, где лежит ключ, и он предупреждает. Появилась связка ключей —
//!   ключ переезжает туда при первом чтении, файл удаляется.
//!
//! В JavaScript ключ не отдаётся никогда: интерфейс передаёт его один раз (`wa_token_set` ядра).

use keyring::Entry;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

const SERVICE: &str = "ru.bobrik.vkmusic";
const ACCOUNT: &str = "wa_token";
const FILE: &str = "wa_token.secret";
const MAX_SECRET: u64 = 16 * 1024;

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
}

fn file() -> PathBuf {
    crate::config::config_dir().join(FILE)
}

/// Связка ключей недоступна (нет Secret Service), а не «записи нет».
fn no_keyring(e: &keyring::Error) -> bool {
    matches!(e, keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_))
}

/// Файл читается, только если это обычный файл (не ссылка), он наш и не больше 16 КБ. Проверка — по открытому
/// дескриптору (O_NOFOLLOW): подменить файл между проверкой и чтением нельзя.
fn file_read() -> Option<String> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(file())
        .ok()?;
    let m = f.metadata().ok()?;
    // SAFETY: geteuid без побочных эффектов и не завершается ошибкой
    let me = unsafe { libc::geteuid() };
    if !m.is_file() || m.uid() != me || m.len() > MAX_SECRET {
        return None;
    }
    if m.mode() & 0o077 != 0 {
        let _ = f.set_permissions(fs::Permissions::from_mode(0o600));
    }
    let mut t = String::new();
    Read::by_ref(&mut f).take(MAX_SECRET).read_to_string(&mut t).ok()?;
    let t = t.trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// Запись через новый временный файл (O_EXCL, 0600) и переименование: старый файл на запись не открывается,
/// подложенная на его место ссылка не уведёт ключ в чужой файл, прерванная запись не оставит половину ключа.
fn file_write(token: &str) -> Result<(), String> {
    let p = file();
    let dir = p.parent().ok_or("Не найдена папка настроек")?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| e.to_string())?;
    let mut r = [0u8; 8];
    getrandom::fill(&mut r).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{FILE}.{:016x}", u64::from_le_bytes(r)));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .map_err(|e| format!("Не удалось сохранить ключ: {e}"))?;
    let res = f.write_all(token.as_bytes()).and_then(|_| f.sync_all()).and_then(|_| fs::rename(&tmp, &p));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(|e| format!("Не удалось сохранить ключ: {e}"))
}

pub fn get() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(t) => {
            let _ = fs::remove_file(file()); // ключ уже в связке ключей — запасной файл не нужен
            Ok(Some(t))
        }
        Err(keyring::Error::NoEntry) => {
            // связка ключей появилась после того, как ключ лёг в файл, — переносим
            let Some(t) = file_read() else { return Ok(None) };
            if entry()?.set_password(&t).is_ok() {
                let _ = fs::remove_file(file());
            }
            Ok(Some(t))
        }
        Err(e) if no_keyring(&e) => Ok(file_read()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set(token: &str) -> Result<(), String> {
    match entry()?.set_password(token) {
        Ok(()) => {
            let _ = fs::remove_file(file());
            Ok(())
        }
        Err(e) if no_keyring(&e) => file_write(token),
        Err(e) => Err(e.to_string()),
    }
}

pub fn clear() -> Result<(), String> {
    let _ = fs::remove_file(file());
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) if no_keyring(&e) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Где лежит ключ Whisper Archive: "keyring", "file" (открытым текстом — интерфейс предупреждает) или "none".
#[tauri::command]
pub fn secret_backend() -> &'static str {
    if file_read().is_some() {
        return "file";
    }
    match entry().map(|e| e.get_password()) {
        Ok(Ok(_)) => "keyring",
        _ => "none",
    }
}
