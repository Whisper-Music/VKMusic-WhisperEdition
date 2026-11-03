#!/usr/bin/env bash
# Сборка и установка VK Music для текущего пользователя.
set -euo pipefail
cd "$(dirname "$0")/.."
for dep in mpv yt-dlp ffmpeg; do
  command -v "$dep" >/dev/null || echo "⚠ нет $dep — поставьте: sudo pacman -S mpv yt-dlp ffmpeg python-mutagen"
done
cargo build --release
install -Dm755 target/release/vkmusic "$HOME/.local/bin/vkmusic"
install -Dm644 icons/icon.png "$HOME/.local/share/icons/hicolor/512x512/apps/vkmusic.png"
install -Dm644 packaging/vkmusic.desktop "$HOME/.local/share/applications/vkmusic.desktop"
mkdir -p "$HOME/Music/vk"
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
echo "Готово: vkmusic (или «VK Music» в меню приложений)"
