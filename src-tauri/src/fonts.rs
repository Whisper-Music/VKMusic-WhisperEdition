//! Системные шрифты для выбора шрифта интерфейса — список семейств из fontconfig (`fc-list`), на Windows — из .NET.

use std::collections::BTreeMap;
#[cfg(desktop)]
use std::process::{Command, Stdio};

/// Семейства шрифтов, установленных в системе, по алфавиту, без повторов (Windows — через .NET:
/// `InstalledFontCollection`, вывод — в UTF-8, иначе русские имена придут в кодировке консоли).
#[cfg(windows)]
pub fn list() -> Result<Vec<String>, String> {
    let script = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName System.Drawing; \
                  (New-Object System.Drawing.Text.InstalledFontCollection).Families | ForEach-Object { $_.Name }";
    let mut cmd = Command::new("powershell");
    crate::os::hide_console(&mut cmd);
    let out = cmd
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Список шрифтов недоступен: {e}"))?;
    if !out.status.success() {
        return Err("Список шрифтов недоступен".into());
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Android: семейства из /system/etc/fonts.xml (`<family name>` и `<alias name>`) — их и понимает CSS в WebView
/// (sans-serif, serif-monospace, casual…). fontconfig на телефоне нет.
#[cfg(mobile)]
pub fn list() -> Result<Vec<String>, String> {
    let xml = std::fs::read_to_string("/system/etc/fonts.xml").map_err(|e| format!("Список шрифтов недоступен: {e}"))?;
    let re = regex::Regex::new(r#"<(?:family|alias)\s[^>]*?\bname="([^"]+)""#).map_err(|e| e.to_string())?;
    let names: Vec<&str> = re.captures_iter(&xml).filter_map(|c| c.get(1)).map(|m| m.as_str()).collect();
    Ok(parse(&names.join("\n")))
}

/// Семейства шрифтов, установленных в системе, по алфавиту, без повторов.
#[cfg(all(desktop, not(windows)))]
pub fn list() -> Result<Vec<String>, String> {
    let out = Command::new("fc-list")
        .args(["--format", "%{family[0]}\n", ":"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("fc-list недоступен ({e}) — установите fontconfig"))?;
    if !out.status.success() {
        return Err("fc-list завершился с ошибкой".into());
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

pub fn parse(text: &str) -> Vec<String> {
    // ключ — без регистра, чтобы «DejaVu Sans» и «Dejavu sans» не повторялись
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        // на случай вывода `fc-list : family` — берём первое имя до запятой
        let name = line.split(',').next().unwrap_or("").trim();
        if valid(name) {
            seen.entry(name.to_lowercase()).or_insert_with(|| name.to_string());
        }
    }
    seen.into_values().collect()
}

/// Имя семейства безопасно подставлять в CSS: без кавычек, скобок и управляющих символов.
pub fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && !name.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\' | ';' | '{' | '}' | '<' | '>'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_dedupes() {
        let v = parse("Inter Display,Inter Display Thin\nJetBrains Mono\njetbrains mono\nDejaVu Sans\n.LastResort\nEvil\"Font\n\n");
        assert_eq!(v, vec!["DejaVu Sans", "Inter Display", "JetBrains Mono"]);
        assert!(valid("Noto Serif CJK JP"));
        assert!(!valid("a;b{}"));
    }
}
