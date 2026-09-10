#!/usr/bin/env python3
"""Derive the tray icons from the app logo.

The platform icon set (icns/ico/png) comes from `npx tauri icon`, which only
needs a square source. The tray is the part that needs hand-holding, because a
menu bar and a system tray want different things:

* Linux and Windows draw the icon as-is over a panel whose colour we do not
  control, so a full-colour icon is the only thing guaranteed to stay visible.
* macOS expects a *template* image — a monochrome silhouette it tints itself to
  match the menu bar. Shrinking the logo to a black blob would lose every
  internal edge, so the template is drawn as a flat pin instead.

Run from the repo root:

    python3 scripts/make-icon.py
    npx tauri icon assets/logo.png
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
LOGO = ROOT / "assets" / "logo.png"
TRAY_DIR = ROOT / "src-tauri" / "icons"

SS = 4  # supersampling factor for the drawn template
CANVAS = 1024


def tray_colour(size: int) -> Image.Image:
    """The logo itself, trimmed to its artwork and fitted to the tray slot."""
    logo = Image.open(LOGO).convert("RGBA")
    bbox = logo.getbbox()
    if bbox:
        logo = logo.crop(bbox)

    # Fit inside a square with a little breathing room, preserving aspect.
    inner = size - 2
    logo.thumbnail((inner, inner), Image.LANCZOS)

    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(logo, ((size - logo.width) // 2, (size - logo.height) // 2))
    return out


def tray_template(size: int) -> Image.Image:
    """A flat pin, drawn large and downsampled so the edges stay clean."""
    img = Image.new("RGBA", (CANVAS * SS, CANVAS * SS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    white = (255, 255, 255, 255)

    def rounded(box, radius):
        draw.rounded_rectangle([c * SS for c in box], radius=radius * SS, fill=white)

    def poly(points):
        draw.polygon([(x * SS, y * SS) for x, y in points], fill=white)

    rounded((330, 236, 694, 330), 46)          # cap
    poly([(392, 330), (632, 330), (596, 508), (428, 508)])  # skirt
    rounded((398, 492, 626, 556), 30)          # collar
    poly([(512, 806), (478, 556), (546, 556)])  # needle

    bbox = img.getbbox()
    if bbox:
        img = img.crop(bbox)
    img = img.resize((size - 4, size - 6), Image.LANCZOS)

    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(img, (2, 3))
    return out


def main() -> None:
    if not LOGO.exists():
        raise SystemExit(f"missing {LOGO}")
    TRAY_DIR.mkdir(parents=True, exist_ok=True)

    tray_colour(32).save(TRAY_DIR / "tray.png")
    tray_colour(64).save(TRAY_DIR / "tray@2x.png")
    tray_template(32).save(TRAY_DIR / "tray-template.png")
    tray_template(64).save(TRAY_DIR / "tray-template@2x.png")
    print(f"wrote tray icons in {TRAY_DIR}")

    # A small copy for the sidebar mark; the full-size logo would put most of a
    # megabyte into the bundle to render a 22px badge.
    small = LOGO.parent / "logo-small.png"
    tray_colour(96).save(small)
    print(f"wrote {small}")


if __name__ == "__main__":
    main()
