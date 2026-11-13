#!/usr/bin/env bash
# Сборка и установка VK Music для текущего пользователя.
set -euo pipefail
cd "$(dirname "$0")/.."
for dep in mpv yt-dlp ffmpeg; do
  command -v "$dep" >/dev/null || echo "⚠ нет $dep — поставьте: sudo pacman -S mpv yt-dlp ffmpeg python-mutagen"
done
# Своя сборка — под x86-64-v3 (AVX2 и т. п.), если процессор его поддерживает; иначе — обычный x86_64.
# Выпуски (release.yml) — всегда обычный x86_64: они должны работать у всех.
if [ "$(uname -m)" = x86_64 ] && /lib64/ld-linux-x86-64.so.2 --help 2>/dev/null | grep -q 'x86-64-v3 (supported'; then
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-cpu=x86-64-v3"
  echo "Сборка под x86-64-v3"
fi
cargo build --release
install -Dm755 target/release/vkmusic "$HOME/.local/bin/vkmusic"
install -Dm644 src-tauri/icons/icon.png "$HOME/.local/share/icons/hicolor/512x512/apps/vkmusic.png"
install -Dm644 packaging/vkmusic.desktop "$HOME/.local/share/applications/vkmusic.desktop"
mkdir -p "$HOME/Music/vk"
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
# ссылки whisper://room/КОД («Слушать вместе») — нам, если их ещё никто не открывает (например, другое приложение Whisper)
if command -v xdg-mime >/dev/null && [ -z "$(xdg-mime query default x-scheme-handler/whisper 2>/dev/null)" ]; then
  xdg-mime default vkmusic.desktop x-scheme-handler/whisper || true
fi
echo "Готово: vkmusic (или «VK Music» в меню приложений)"
