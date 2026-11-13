#!/usr/bin/env bash
# Сборка VK Music для Android (APK). Нужны JDK 17, Android SDK и NDK, цели Rust:
#   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
# Переменные: JAVA_HOME, ANDROID_HOME, NDK_HOME (по умолчанию — ~/.local/jdk-17, ~/Android/Sdk, последний NDK в нём).
#
#   packaging/android.sh              отладочный APK для arm64 (телефоны последних лет)
#   packaging/android.sh --all        отладочный APK для всех архитектур
#   packaging/android.sh --release    выпуск: все архитектуры, подписан ключом
#                                     VKM_KEYSTORE (файл .jks), VKM_KEYSTORE_PASS, VKM_KEY_ALIAS (по умолчанию vkmusic)
#   … --install                       поставить на подключённый телефон (adb, включена «Отладка по USB»)
set -euo pipefail
cd "$(dirname "$0")/.."

release=0 all=0 install=0
for a in "$@"; do
  case "$a" in
    --release) release=1 all=1 ;;
    --all) all=1 ;;
    --install) install=1 ;;
    *) echo "Неизвестный параметр: $a" >&2; exit 2 ;;
  esac
done

export JAVA_HOME="${JAVA_HOME:-$HOME/.local/jdk-17}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
if [ -z "${NDK_HOME:-}" ]; then
  NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
  export NDK_HOME
fi
export PATH="$JAVA_HOME/bin:$PATH"
for need in "$JAVA_HOME/bin/java" "$ANDROID_HOME/platform-tools" "$NDK_HOME"; do
  [ -e "$need" ] || { echo "Не найдено: $need — поставьте JDK 17, Android SDK и NDK (см. начало файла)" >&2; exit 1; }
done

args=(android build --apk --ci)
[ "$release" = 1 ] || args+=(--debug)
[ "$all" = 1 ] || args+=(--target aarch64)
# своего package.json нет — Tauri CLI через npx, как в AGENTS.md
npx --yes @tauri-apps/cli@2 "${args[@]}"

out=src-tauri/gen/android/app/build/outputs/apk/universal
if [ "$release" = 1 ]; then
  : "${VKM_KEYSTORE:?укажите VKM_KEYSTORE — файл ключа .jks (создать: keytool -genkeypair -keystore vkmusic.jks -alias vkmusic -keyalg RSA -keysize 4096 -validity 10000)}"
  : "${VKM_KEYSTORE_PASS:?укажите VKM_KEYSTORE_PASS — пароль ключа}"
  bt="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
  unsigned="$out/release/app-universal-release-unsigned.apk"
  apk="$out/release/vkmusic-$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1).apk"
  "$bt/zipalign" -f -p 4 "$unsigned" "$apk.aligned"
  "$bt/apksigner" sign --ks "$VKM_KEYSTORE" --ks-pass env:VKM_KEYSTORE_PASS --ks-key-alias "${VKM_KEY_ALIAS:-vkmusic}" \
    --out "$apk" "$apk.aligned"
  rm -f "$apk.aligned"
else
  apk="$out/debug/app-universal-debug.apk"
fi
echo "APK: $apk"
if [ "$install" = 1 ]; then
  "$ANDROID_HOME/platform-tools/adb" install -r "$apk"
fi
