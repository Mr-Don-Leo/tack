#!/usr/bin/env python3
"""Generate the Tack source app icon (1024px) and the monochrome tray icon.

Run from the repo root:  python3 scripts/make-icon.py
Then regenerate the platform icon set with:  npx tauri icon assets/icon.png
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
TRAY_DIR = ROOT / "src-tauri" / "icons"

SS = 4  # supersampling factor
SIZE = 1024


def rounded(draw, box, radius, fill):
    draw.rounded_rectangle([c * SS for c in box], radius=radius * SS, fill=fill)


def poly(draw, points, fill):
    draw.polygon([(x * SS, y * SS) for x, y in points], fill=fill)


def pin(draw, fill):
    """A thumbtack: cap, tapered skirt, collar, needle."""
    rounded(draw, (330, 236, 694, 330), 46, fill)
    poly(draw, [(392, 330), (632, 330), (596, 508), (428, 508)], fill)
    rounded(draw, (398, 492, 626, 556), 30, fill)
    poly(draw, [(512, 806), (478, 556), (546, 556)], fill)


def app_icon() -> Image.Image:
    img = Image.new("RGBA", (SIZE * SS, SIZE * SS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Vertical accent gradient behind a squircle-ish rounded rect.
    grad = Image.new("RGBA", (1, SIZE * SS))
    top, bottom = (10, 132, 255), (0, 82, 214)
    for y in range(SIZE * SS):
        t = y / (SIZE * SS - 1)
        grad.putpixel(
            (0, y),
            tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,),
        )
    grad = grad.resize((SIZE * SS, SIZE * SS))

    mask = Image.new("L", (SIZE * SS, SIZE * SS), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [64 * SS, 64 * SS, (SIZE - 64) * SS, (SIZE - 64) * SS], radius=224 * SS, fill=255
    )
    img.paste(grad, (0, 0), mask)

    pin(draw, (255, 255, 255, 255))
    return img.resize((SIZE, SIZE), Image.LANCZOS)


def tray_icon(size: int) -> Image.Image:
    """Monochrome template icon — the OS tints it to match the menu bar."""
    img = Image.new("RGBA", (SIZE * SS, SIZE * SS), (0, 0, 0, 0))
    pin(ImageDraw.Draw(img), (255, 255, 255, 255))
    # Trim to the glyph and pad so the pin fills the tray slot evenly.
    img = img.crop(img.getbbox()).resize((size - 4, size - 6), Image.LANCZOS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(img, (2, 3))
    return out


def main():
    ASSETS.mkdir(exist_ok=True)
    TRAY_DIR.mkdir(parents=True, exist_ok=True)
    app_icon().save(ASSETS / "icon.png")
    tray_icon(32).save(TRAY_DIR / "tray.png")
    tray_icon(64).save(TRAY_DIR / "tray@2x.png")
    print(f"wrote {ASSETS / 'icon.png'} and tray icons in {TRAY_DIR}")


if __name__ == "__main__":
    main()
