#!/usr/bin/env python3
# Иконка Android из логотипа VK Music (src/favicon.svg): три полоски на тёмной подложке.
# Адаптивная (подложка, полоски, одноцветный слой для тематических значков Android 13+) и PNG для Android 7.
# Запуск: python3 packaging/android-icon.py src-tauri/gen/android/app/src/main/res ВРЕМЕННАЯ_ПАПКА (нужен rsvg-convert)
import math, os, subprocess, sys
RES = sys.argv[1]
TMP = sys.argv[2]
BG, MID, SIDE = "#111c18", "#2dd5b7", "#50947a"
# полоски в координатах favicon.svg (512): x, y, w, h, цвет
BARS = [(133, 195, 61, 184, SIDE), (225, 81, 62, 298, MID), (317, 153, 62, 226, SIDE)]
CX, CY = 256, 230  # центр логотипа
# масштаб — чтобы самый дальний угол полосок был не дальше 31 dp от центра (безопасный круг адаптивной иконки — 33 dp)
far = max(math.hypot(max(abs(x - CX), abs(x + w - CX)), max(abs(y - CY), abs(y + h - CY))) for x, y, w, h, _ in BARS)
S = 31 / far

def pill(x, y, w, h):
    """Полоска со скруглёнными концами (r = w/2) — pathData в координатах 108 dp."""
    X, Y, W, H = 54 + (x - CX) * S, 54 + (y - CY) * S, w * S, h * S
    r = W / 2
    f = lambda v: f"{v:.3f}".rstrip("0").rstrip(".")
    return (f"M{f(X)},{f(Y + r)} A{f(r)},{f(r)} 0 0 1 {f(X + W)},{f(Y + r)} L{f(X + W)},{f(Y + H - r)} "
            f"A{f(r)},{f(r)} 0 0 1 {f(X)},{f(Y + H - r)} Z")

def vector(paths):
    body = "\n".join(f'    <path android:fillColor="{c}" android:pathData="{d}" />' for d, c in paths)
    return ('<?xml version="1.0" encoding="utf-8"?>\n<!-- Логотип VK Music (src/favicon.svg) для иконки Android: '
            'packaging/android-icon.py -->\n'
            '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            '    android:width="108dp" android:height="108dp"\n'
            '    android:viewportWidth="108" android:viewportHeight="108">\n' + body + "\n</vector>\n")

def write(rel, text):
    p = os.path.join(RES, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "w").write(text)

write("drawable-v24/ic_launcher_foreground.xml", vector([(pill(x, y, w, h), c) for x, y, w, h, c in BARS]))
write("drawable/ic_launcher_monochrome.xml", vector([(pill(x, y, w, h), "#FFFFFFFF") for x, y, w, h, _ in BARS]))
write("drawable/ic_launcher_background.xml",
      '<?xml version="1.0" encoding="utf-8"?>\n<!-- подложка иконки — цвет подложки значка VK Music -->\n'
      '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
      '    android:width="108dp" android:height="108dp"\n    android:viewportWidth="108" android:viewportHeight="108">\n'
      f'    <path android:fillColor="{BG}" android:pathData="M0,0h108v108h-108z" />\n</vector>\n')
adaptive = ('<?xml version="1.0" encoding="utf-8"?>\n'
            '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
            '    <background android:drawable="@drawable/ic_launcher_background" />\n'
            '    <foreground android:drawable="@drawable/ic_launcher_foreground" />\n'
            '    <monochrome android:drawable="@drawable/ic_launcher_monochrome" />\n'
            '</adaptive-icon>\n')
write("mipmap-anydpi-v26/ic_launcher.xml", adaptive)
write("mipmap-anydpi-v26/ic_launcher_round.xml", adaptive)

# PNG для Android 7 (без адаптивных иконок) и слой переднего плана — через rsvg-convert из SVG той же геометрии
def svg(shape):
    bars = "".join(f'<path fill="{c}" d="{pill(x, y, w, h)}"/>' for x, y, w, h, c in BARS)
    bg = {"square": f'<rect x="6" y="6" width="96" height="96" rx="22" fill="{BG}"/>',
          "round": f'<circle cx="54" cy="54" r="48" fill="{BG}"/>', "fg": ""}[shape]
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 108 108">{bg}{bars}</svg>'

DENS = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
for shape in ("square", "round", "fg"):
    src = os.path.join(TMP, shape + ".svg")
    open(src, "w").write(svg(shape))
    for d, k in DENS.items():
        size = int((108 if shape == "fg" else 48) * k)
        name = {"square": "ic_launcher.png", "round": "ic_launcher_round.png", "fg": "ic_launcher_foreground.png"}[shape]
        out = os.path.join(RES, f"mipmap-{d}", name)
        # иконки 48 dp рисуются с полей 108-холста, как адаптивная (подложка 96 из 108)
        subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", out, src], check=True)
print("масштаб", round(S, 4), "— полоска", round(298 * S, 1), "dp")
