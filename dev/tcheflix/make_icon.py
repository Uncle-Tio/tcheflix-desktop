#!/usr/bin/env python3
"""
Builds `src/tcheflix/assets/tcheflix_icon.ico` from `tcheflix_icon.png`, embedding
several sizes (16..256) so Windows shows a crisp icon in the taskbar, Explorer,
shortcuts and the Inno wizard instead of downscaling a single 256px image.

After regenerating, force the icon to be re-embedded (the build script only
tracks the .rc template, not the .ico it points at, so cargo would otherwise
reuse the cached resource):
    cargo clean -p jfn-rust   # then `just build`
"""
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "src" / "tcheflix" / "assets" / "tcheflix_icon.png"
OUT = ROOT / "src" / "tcheflix" / "assets" / "tcheflix_icon.ico"

TARGET_SIZES = (16, 24, 32, 48, 64, 128, 256)

def main() -> None:
    img = Image.open(SRC).convert("RGBA")
    w, h = img.size
    print(f"source: {SRC.name} {w}x{h}")

    if w != h:
        side = max(w, h)
        canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        canvas.paste(img, ((side - w) // 2, (side - h) // 2))
        img = canvas
        print(f"padded to square: {side}x{side}")

    side = img.size[0]
    sizes = [s for s in TARGET_SIZES if s <= side]
    if not sizes:
        sizes = [side]

    img.save(OUT, format="ICO", sizes=[(s, s) for s in sizes])
    print(f"wrote {OUT.name}: {sizes}")

    data = OUT.read_bytes()
    count = int.from_bytes(data[4:6], "little")
    got = []
    for i in range(count):
        o = 6 + i * 16
        bw = data[o] or 256
        bh = data[o + 1] or 256
        got.append(f"{bw}x{bh}")
    print(f"verify: {count} frames -> {', '.join(got)}  ({len(data)} bytes)")


if __name__ == "__main__":
    main()
