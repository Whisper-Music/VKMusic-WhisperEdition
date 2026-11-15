//! Встроенные темы: каждый `themes/<id>.wmt` вшивается в бинарник (`themes::BUILTIN`, файл `builtin_themes.rs`
//! в OUT_DIR). В репозитории — Osaka Jade и Vantablack; свои темы владельца лежат там же, но не в git
//! (`.gitignore`): в его сборках они встроены, в выпусках и у остальных их нет.

use std::path::Path;

/// Первыми в списке стилей — темы из репозитория, остальные — по алфавиту.
const FIRST: &[&str] = &["osaka-jade", "vantablack"];

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../themes");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str()?.strip_suffix(".wmt").map(str::to_string))
                .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
                .collect()
        })
        .unwrap_or_default();
    ids.sort_by_key(|id| (FIRST.iter().position(|f| f == id).unwrap_or(FIRST.len()), id.clone()));
    let mut out = String::from("&[\n");
    for id in &ids {
        let path = dir.join(format!("{id}.wmt")).canonicalize().expect("путь темы");
        println!("cargo:rerun-if-changed={}", path.display());
        out += &format!("    ({id:?}, include_bytes!({:?})),\n", path.display().to_string());
    }
    out += "]\n";
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin_themes.rs");
    std::fs::write(dest, out).expect("builtin_themes.rs");
    tauri_build::build()
}
