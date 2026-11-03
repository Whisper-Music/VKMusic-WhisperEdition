# Фон темы «Грифель»: школьная доска со стёртым мелом. Процедурный, без чужих ассетов.
# Запуск: python3 scripts/slate-background.py themes/slate/background.webp, затем scripts/pack-theme.sh themes/slate
import sys, numpy as np
from PIL import Image, ImageDraw, ImageFilter
W, H = 2560, 1600
rng = np.random.default_rng(29)

def blur(a, r):
    im = Image.fromarray(np.clip(a * 255, 0, 255).astype(np.uint8))
    return np.asarray(im.filter(ImageFilter.GaussianBlur(r)), dtype=np.float32) / 255

# основа: тёмный графит с крупными пятнами
base = np.full((H, W), 0.165, np.float32)
low = rng.random((H // 160 + 2, W // 160 + 2)).astype(np.float32)
low = np.asarray(Image.fromarray((low * 255).astype(np.uint8)).resize((W, H), Image.BICUBIC), np.float32) / 255
low = blur(low, 90)
base += (low - 0.5) * 0.05

# стёртый мел: широкие мутные дуги «тряпкой»
dust = Image.new("L", (W, H), 0)
d = ImageDraw.Draw(dust)
for _ in range(9):
    cx, cy = rng.uniform(-0.2, 1.2) * W, rng.uniform(-0.3, 1.3) * H
    r = rng.uniform(0.25, 0.7) * W
    a0 = rng.uniform(0, 360); a1 = a0 + rng.uniform(40, 110)
    d.arc([cx - r, cy - r * 0.6, cx + r, cy + r * 0.6], a0, a1, fill=int(rng.uniform(40, 90)), width=int(rng.uniform(60, 160)))
for _ in range(7):
    y = rng.uniform(0, H); x0 = rng.uniform(-0.2, 0.5) * W; x1 = x0 + rng.uniform(0.3, 0.8) * W
    d.line([x0, y, x1, y + rng.uniform(-80, 80)], fill=int(rng.uniform(25, 60)), width=int(rng.uniform(50, 130)))
dust = np.asarray(dust.filter(ImageFilter.GaussianBlur(55)), np.float32) / 255
base += dust * 0.16

# бледные нотные станы и ноты, как недостёртые
lines = Image.new("L", (W, H), 0)
d = ImageDraw.Draw(lines)
for (x0, y0, w) in [(0.30, 0.20, 0.42), (0.36, 0.70, 0.40), (0.05, 0.47, 0.22)]:
    x0, y0, w = x0 * W, y0 * H, w * W
    for k in range(5):
        y = y0 + k * 16 + rng.uniform(-1.5, 1.5)
        d.line([x0, y, x0 + w, y + rng.uniform(-4, 4)], fill=int(rng.uniform(70, 120)), width=2)
    for _ in range(int(rng.integers(3, 7))):
        nx = x0 + rng.uniform(0.05, 0.95) * w; ny = y0 + rng.integers(-1, 9) * 8
        d.ellipse([nx - 9, ny - 7, nx + 9, ny + 7], fill=int(rng.uniform(90, 140)))
        d.line([nx + 8, ny, nx + 8, ny - 52], fill=int(rng.uniform(90, 140)), width=3)
lines = np.asarray(lines.filter(ImageFilter.GaussianBlur(1.6)), np.float32) / 255
# мел крошится: рвём линии шумом
lines *= 0.45 + 0.55 * rng.random((H, W)).astype(np.float32)
base += lines * 0.22

# мелкое зерно доски и виньетка
base += (rng.random((H, W)).astype(np.float32) - 0.5) * 0.022
yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
v = ((xx / W - 0.5) ** 2 + (yy / H - 0.5) ** 2) ** 0.5
base *= 1 - 0.35 * np.clip(v - 0.25, 0, 1)

g = np.clip(base, 0, 1)
rgb = np.stack([g * 0.985, g * 1.0, g * 0.975], -1)  # чуть зеленоватый графит
Image.fromarray((rgb * 255).astype(np.uint8)).save(sys.argv[1], "WEBP", quality=88)
