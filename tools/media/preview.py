"""Preview the rendered media the way slot composites them.

    python tools/media/preview.py OUT.png [name ...]

Blends each assets/<name>_neutral.png with a shell colour through the same Overlay slot uses,
pastes a stand-in label into its label box and lays them out on a dark shelf, so a render can be
judged in context without building the frontend.
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets"

# name: (shell colour, label box x, y, w, h) — the boxes render_media.py prints.
MEDIA = {
    "nes": ((0x8a, 0x8a, 0x8e), (25, 20, 253, 208)),
    "snes": ((0x9a, 0x99, 0xa0), (84, 56, 191, 146)),
    "genesis": ((0x2a, 0x2a, 0x2c), (27, 107, 213, 213)),
    "sms": ((0x26, 0x26, 0x28), (118, 17, 219, 146)),
    "gamegear": ((0x22, 0x22, 0x24), (56, 71, 207, 160)),
    "hucard": ((0xe6, 0xe4, 0xde), (16, 16, 181, 259)),
    "ngp": ((0x30, 0x30, 0x34), (42, 108, 210, 174)),
    "wonderswan": ((0x2c, 0x2c, 0x30), (28, 34, 193, 204)),
    "lynx": ((0x3a, 0x3a, 0x3e), (36, 82, 268, 134)),
    "disc": ((0xd8, 0xd8, 0xdc), None),
    "pcb": ((0x2e, 0x6b, 0x3a), (24, 22, 168, 106)),
    "mvs": ((0x22, 0x22, 0x24), (27, 58, 306, 166)),
    "fds": ((0xe8, 0xc4, 0x2c), (25, 25, 222, 136)),
    "n64": ((0x5a, 0x5a, 0x60), (50, 74, 261, 137)),
    "vb": ((0x2a, 0x2a, 0x2c), (33, 100, 233, 200)),
    "pokemini": ((0x50, 0x50, 0x58), (33, 89, 233, 156)),
    "gw": ((0xc4, 0xac, 0x80), (98, 33, 164, 131)),
    "32x": ((0x24, 0x24, 0x26), (30, 135, 190, 180)),
    "sg1000": ((0x28, 0x28, 0x2a), (33, 99, 214, 198)),
    "a2600": ((0x22, 0x22, 0x22), (30, 30, 240, 188)),
    "a5200": ((0x26, 0x26, 0x28), (29, 23, 243, 160)),
    "a7800": ((0x28, 0x28, 0x2a), (29, 22, 230, 202)),
    "coleco": ((0x2a, 0x2a, 0x2c), (29, 29, 237, 209)),
    "intv": ((0x2a, 0x2a, 0x2a), (17, 23, 143, 148)),
    "vectrex": ((0x30, 0x30, 0x32), (30, 38, 240, 180)),
    "odyssey": ((0x26, 0x26, 0x28), (36, 100, 268, 157)),
    "channelf": ((0xe0, 0xb8, 0x30), (30, 30, 240, 162)),
    "supervision": ((0x34, 0x34, 0x38), (32, 32, 256, 160)),
    "megaduck": ((0x88, 0x88, 0x8c), (31, 72, 238, 186)),
    "msx": ((0x2c, 0x2c, 0x2e), (90, 24, 246, 138)),
    "floppy35": ((0x30, 0x30, 0x34), (33, 153, 233, 147)),
    "floppy525": ((0x26, 0x26, 0x28), (25, 15, 280, 55)),
    "tape": ((0x3a, 0x3a, 0x3e), (29, 22, 302, 79)),
    "cpc3": ((0xd0, 0xd0, 0xcc), (29, 29, 230, 180)),
    "umd": ((0xe6, 0xe6, 0xe2), (18, 15, 284, 284)),
    "fantasy": ((0xe0, 0x50, 0x60), (41, 46, 238, 192)),
}


def overlay(base: np.ndarray, colour: np.ndarray) -> np.ndarray:
    base = base.astype(np.int32)
    c = colour.astype(np.int32)
    lift = c + ((255 - c) * (base - 128) + 63) // 127
    sink = c - (c * (128 - base) + 64) // 128
    return np.clip(np.where(base >= 128, lift, sink), 0, 255)


def label(w: int, h: int, hue: int) -> np.ndarray:
    img = Image.new("RGB", (w, h))
    draw = ImageDraw.Draw(img)
    for y in range(h):
        t = y / max(h - 1, 1)
        draw.line([(0, y), (w, y)], fill=(int(60 + 150 * t), int(90 + hue % 120), int(200 - 120 * t)))
    draw.rectangle([w * 0.1, h * 0.38, w * 0.9, h * 0.62], fill=(250, 240, 220))
    return np.asarray(img)


def compose(name: str, colour: tuple[int, int, int], box: tuple[int, int, int, int] | None) -> Image.Image:
    src = np.asarray(Image.open(ASSETS / f"{name}_neutral.png").convert("RGBA")).copy()
    h, w = src.shape[:2]
    out = src.copy()
    out[..., :3] = overlay(src[..., :3], np.array(colour))
    if box is None:
        # The disc's print: everything between the clear hub and the rim.
        yy, xx = np.mgrid[0:h, 0:w]
        r = np.hypot(xx - w / 2 + 0.5, yy - h / 2 + 0.5) / (w / 2)
        ring = (r > 22.5 / 60) & (r < 58.5 / 60)
        art = label(w, h, 40)
        out[ring, :3] = art[ring]
    else:
        x, y, lw, lh = box
        art = label(lw, lh, x * 7)
        if name == "umd":
            # The UMD's print: between its white hub and the edge of its window.
            yy, xx = np.mgrid[0:lh, 0:lw]
            r = np.hypot(xx - lw / 2 + 0.5, yy - lh / 2 + 0.5) / (lw / 2)
            ring = (r > 12.6 / 28.4) & (r < 1)
            out[y : y + lh, x : x + lw, :3][ring] = art[ring]
        else:
            out[y : y + lh, x : x + lw, :3] = art
    return Image.fromarray(out.astype(np.uint8), "RGBA")


def main() -> None:
    target = Path(sys.argv[1])
    # Any names after the file show only those media.
    names = sys.argv[2:] or list(MEDIA)
    tiles = [compose(n, *MEDIA[n]) for n in names]
    cols = min(5, len(tiles))
    cell = 380
    rows = -(-len(tiles) // cols)
    sheet = Image.new("RGB", (cols * cell, rows * cell), (0x2b, 0x1d, 0x16))
    for i, tile in enumerate(tiles):
        cx, cy = (i % cols) * cell, (i // cols) * cell
        sheet.paste(tile, (cx + (cell - tile.width) // 2, cy + (cell - tile.height) // 2), tile)
    sheet.save(target)


main()
