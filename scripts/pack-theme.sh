#!/usr/bin/env bash
# Собрать тему из папки: scripts/pack-theme.sh themes/osaka-jade  →  themes/osaka-jade.wmt
# В папке: theme.wmt (обязательно), background.<jpg|png|webp|…|mp4|webm>, любые файлы темы.
set -euo pipefail
dir="${1:?папка темы}"
dir="${dir%/}"
out="$(cd "$(dirname "$dir")" && pwd)/$(basename "$dir").wmt"
[ -f "$dir/theme.wmt" ] || { echo "нет $dir/theme.wmt" >&2; exit 1; }
rm -f "$out"
(cd "$dir" && zip -X -q -r "$out" . -x '.*' -x '__MACOSX/*')
echo "$out"
