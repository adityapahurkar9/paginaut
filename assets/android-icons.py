"""Android launcher icons from assets/icon-1024.png (run assets/render-icon.mjs first).

Writes into src-tauri/gen/android/app/src/main/res:
  mipmap-<dpi>/ic_launcher.png, ic_launcher_round.png   legacy icons (Android 7: the mark on a white tile / circle)
  mipmap-<dpi>/ic_launcher_foreground.png               adaptive layer: the mark inside the 66dp safe zone
  mipmap-<dpi>/ic_launcher_monochrome.png               themed icons (Android 13+): the navy parts, the rest cut out
  mipmap-anydpi-v26/ic_launcher(.xml, _round.xml)       adaptive icon: white background + layers
Usage: E:/Anaconda/python.exe assets/android-icons.py
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
RES = ROOT / "src-tauri/gen/android/app/src/main/res"
DPIS = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}

src = Image.open(ROOT / "assets/icon-1024.png").convert("RGBA")
mark = src.crop(src.getbbox())  # the mark alone, no margins


def fit(img: Image.Image, box: int) -> Image.Image:
    """`img` scaled to fit a box of `box` pixels, centred on a transparent square."""
    w, h = img.size
    s = box / max(w, h)
    scaled = img.resize((max(1, round(w * s)), max(1, round(h * s))), Image.LANCZOS)
    out = Image.new("RGBA", (box, box), (0, 0, 0, 0))
    out.paste(scaled, ((box - scaled.width) // 2, (box - scaled.height) // 2), scaled)
    return out


def monochrome(img: Image.Image) -> Image.Image:
    """Opaque where the mark is dark (navy, blue), clear where it is white or light: Android tints it."""
    px = img.load()
    out = Image.new("RGBA", img.size, (0, 0, 0, 0))
    po = out.load()
    for y in range(img.height):
        for x in range(img.width):
            r, g, b, a = px[x, y]
            # The palest channel: white is light, navy and the blue fold are dark. The text
            # lines (mid grey) go with the page they're on.
            light = min(r, g, b)
            keep = max(0.0, min(1.0, (125 - light) / 40))
            po[x, y] = (255, 255, 255, round(a * keep))
    return out


def tile(size: int, round_: bool) -> Image.Image:
    """Legacy icon: white rounded square (or circle) with a soft edge, the mark on it."""
    s = size * 4  # drawn large, then scaled down for smooth edges
    base = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(base)
    m = round(s * 0.04)
    if round_:
        d.ellipse((m, m, s - m, s - m), fill=(255, 255, 255, 255))
    else:
        d.rounded_rectangle((m, m, s - m, s - m), radius=round(s * 0.2), fill=(255, 255, 255, 255))
    inner = fit(mark, round(s * (0.58 if round_ else 0.64)))
    base.alpha_composite(inner, ((s - inner.width) // 2, (s - inner.height) // 2))
    return base.resize((size, size), Image.LANCZOS)


mono_src = monochrome(mark)
for name, k in DPIS.items():
    d = RES / f"mipmap-{name}"
    d.mkdir(parents=True, exist_ok=True)
    layer = round(108 * k)
    safe = round(62 * k)  # inside the 66dp circle every launcher shape keeps
    fg = Image.new("RGBA", (layer, layer), (0, 0, 0, 0))
    m = fit(mark, safe)
    fg.alpha_composite(m, ((layer - safe) // 2, (layer - safe) // 2))
    fg.save(d / "ic_launcher_foreground.png", optimize=True)
    mono = Image.new("RGBA", (layer, layer), (0, 0, 0, 0))
    mm = fit(mono_src, safe)
    mono.alpha_composite(mm, ((layer - safe) // 2, (layer - safe) // 2))
    mono.save(d / "ic_launcher_monochrome.png", optimize=True)
    legacy = round(48 * k)
    tile(legacy, False).save(d / "ic_launcher.png", optimize=True)
    tile(legacy, True).save(d / "ic_launcher_round.png", optimize=True)

anydpi = RES / "mipmap-anydpi-v26"
anydpi.mkdir(exist_ok=True)
xml = """<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
    <monochrome android:drawable="@mipmap/ic_launcher_monochrome" />
</adaptive-icon>
"""
for f in ("ic_launcher.xml", "ic_launcher_round.xml"):
    (anydpi / f).write_text(xml, encoding="utf-8")
values = RES / "values"
(values / "ic_launcher_background.xml").write_text(
    '<?xml version="1.0" encoding="utf-8"?>\n<resources>\n    <color name="ic_launcher_background">#FFFFFF</color>\n</resources>\n',
    encoding="utf-8",
)
print("android icons written to", RES)
