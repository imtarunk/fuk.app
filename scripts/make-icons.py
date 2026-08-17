#!/usr/bin/env python3
"""Turn the source artwork into a macOS app icon and a menu bar template icon.

macOS does not round app icons for you, so the app icon is composited onto an
Apple-style superellipse. The menu bar is a separate problem: it wants a
template image, meaning pure black plus alpha, which the system then recolours
for the light or dark bar and for the highlighted state.

Usage: python3 scripts/make-icons.py
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "assets" / "icon-source.png"
APP_ICON = ROOT / "assets" / "icon-app.png"
TRAY_ICON = ROOT / "src-tauri" / "icons" / "tray.png"

# macOS scales a status item image to 18pt tall and keeps the aspect ratio, so
# one asset is enough as long as it has the pixels for a 3x display.
TRAY_POINT_HEIGHT = 18
TRAY_SCALE = 3

CANVAS = 1024
# Apple's grid: the icon body is 824pt inside a 1024pt canvas, leaving room for
# the shadow the system draws around it.
BODY = 824
# Apple's corner is a continuous curve, not a circular arc: straight edges that
# ease into the turn. Model it as a superellipse applied only inside a corner
# box of RADIUS, so the sides stay perfectly flat.
CORNER_RADIUS = 0.2237
CORNER_N = 4.0
# Share of the icon body the artwork should span.
ARTWORK_SPAN = 0.78
# Supersampling factor for the mask, to get clean antialiased corners.
SS = 4


def background_colour(image: Image.Image) -> tuple[int, int, int]:
    return image.convert("RGB").getpixel((0, 0))


def artwork_box(image: Image.Image, bg: tuple[int, int, int], tolerance: int = 24):
    """Bounding box of everything that is not the flat background."""
    rgb = image.convert("RGB")
    diff = Image.new("L", rgb.size)
    diff.putdata(
        [
            255 if max(abs(p[0] - bg[0]), abs(p[1] - bg[1]), abs(p[2] - bg[2])) > tolerance else 0
            for p in rgb.getdata()
        ]
    )
    return diff.getbbox(), diff


def squircle_mask(size: int) -> Image.Image:
    """Antialiased rounded square, rendered large and downsampled.

    A point is inside when its distance *into* a corner box satisfies the
    superellipse. Anywhere along a flat edge one of those distances is zero, so
    the edge stays straight.
    """
    big = size * SS
    radius = CORNER_RADIUS * big
    pixels = bytearray(big * big)
    for y in range(big):
        py = y + 0.5
        dy = max(0.0, radius - py, py - (big - radius))
        if dy >= radius:
            continue
        # Solve for how far into the corner x may reach on this row.
        reach = radius * (1.0 - (dy / radius) ** CORNER_N) ** (1.0 / CORNER_N)
        start = int(radius - reach)
        end = big - start
        row = y * big
        for x in range(max(0, start), min(big, end)):
            pixels[row + x] = 255
    mask = Image.new("L", (big, big))
    mask.frombytes(bytes(pixels))
    return mask.resize((size, size), Image.LANCZOS)


def build_app_icon(source: Image.Image, bg: tuple[int, int, int]) -> Image.Image:
    box, _ = artwork_box(source, bg)
    art = source.convert("RGBA").crop(box)

    # Scale the artwork so its longest side covers ARTWORK_SPAN of the body,
    # then centre it on a background-coloured tile.
    scale = (BODY * ARTWORK_SPAN) / max(art.width, art.height)
    art = art.resize((round(art.width * scale), round(art.height * scale)), Image.LANCZOS)

    body = Image.new("RGBA", (BODY, BODY), (*bg, 255))
    body.paste(art, ((BODY - art.width) // 2, (BODY - art.height) // 2), art)
    body.putalpha(squircle_mask(BODY))

    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    offset = (CANVAS - BODY) // 2
    canvas.paste(body, (offset, offset), body)
    return canvas


def build_tray_icon(source: Image.Image, bg: tuple[int, int, int], height: int) -> Image.Image:
    """Black-plus-alpha silhouette, which macOS treats as a template image."""
    box, diff = artwork_box(source, bg)
    alpha = diff.crop(box)

    width = round(alpha.width * height / alpha.height)
    alpha = alpha.resize((width, height), Image.LANCZOS)

    icon = Image.new("RGBA", alpha.size, (0, 0, 0, 0))
    icon.putalpha(alpha)
    return icon


def main() -> None:
    source = Image.open(SOURCE)
    bg = background_colour(source)
    print(f"source {source.width}x{source.height}, background rgb{bg}")

    APP_ICON.parent.mkdir(parents=True, exist_ok=True)
    build_app_icon(source, bg).save(APP_ICON)
    print(f"wrote {APP_ICON.relative_to(ROOT)} ({CANVAS}x{CANVAS})")

    tray = build_tray_icon(source, bg, TRAY_POINT_HEIGHT * TRAY_SCALE)
    tray.save(TRAY_ICON)
    print(f"wrote {TRAY_ICON.relative_to(ROOT)} ({tray.width}x{tray.height})")


if __name__ == "__main__":
    main()
