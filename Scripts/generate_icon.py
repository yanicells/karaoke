#!/usr/bin/env python3
"""Render assets/logo.svg's simple geometry deterministically, using only stdlib."""
import math
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

ROOT = Path(__file__).resolve().parent.parent
SIZE = 1024
BACKGROUND = (240, 245, 252)
MARK = (73, 107, 148)


def rounded_box(x, y, cx, cy, half_width, half_height, radius):
    """Signed distance to a rounded rectangle; negative values are inside."""
    dx = abs(x - cx) - half_width + radius
    dy = abs(y - cy) - half_height + radius
    return math.hypot(max(dx, 0), max(dy, 0)) + min(max(dx, dy), 0) - radius


def coverage(distance):
    return min(max(0.5 - distance, 0), 1)


def png_chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def render_png():
    scanlines = bytearray()
    for row in range(SIZE):
        scanlines.append(0)  # PNG filter: none.
        y = row + 0.5
        for column in range(SIZE):
            x = column + 0.5
            alpha = coverage(rounded_box(x, y, 512, 512, 448, 448, 200))
            if alpha == 0:
                scanlines.extend((0, 0, 0, 0))
                continue
            # Capsule, U-shaped cradle, short stem, and horizontal foot.
            cradle = (
                abs(math.hypot(x - 512, y - 500) - 164) - 20
                if y >= 500
                else min(
                    rounded_box(x, y, 348, 470, 20, 50, 20),
                    rounded_box(x, y, 676, 470, 20, 50, 20),
                )
            )
            ink = coverage(min(
                rounded_box(x, y, 512, 424, 104, 176, 104),
                cradle,
                rounded_box(x, y, 512, 716, 20, 72, 20),
                rounded_box(x, y, 512, 768, 120, 20, 20),
            ))
            scanlines.extend(round(bg + (fg - bg) * ink) for bg, fg in zip(BACKGROUND, MARK))
            scanlines.append(round(alpha * 255))
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", header)
        + png_chunk(b"IDAT", zlib.compress(scanlines, 9))
        + png_chunk(b"IEND", b"")
    )


def main():
    png = ROOT / "assets/KaraokeMic.png"
    png.write_bytes(render_png())
    with tempfile.TemporaryDirectory(prefix="karaokemic-icon-") as staging:
        iconset = Path(staging) / "KaraokeMic.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                suffix = "@2x" if scale == 2 else ""
                destination = iconset / f"icon_{size}x{size}{suffix}.png"
                subprocess.run(
                    ["/usr/bin/sips", "-z", str(size * scale), str(size * scale),
                     str(png), "--out", str(destination)],
                    check=True, stdout=subprocess.DEVNULL,
                )
        subprocess.run(
            ["/usr/bin/iconutil", "-c", "icns", str(iconset), "-o", str(ROOT / "assets/KaraokeMic.icns")],
            check=True,
        )
    print("Generated assets/KaraokeMic.png and assets/KaraokeMic.icns")


if __name__ == "__main__":
    main()
