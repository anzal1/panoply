#!/usr/bin/env python3
"""Draws the placeholder Panoply icon (panoply-512.png, panoply.ico) with the standard library only.

A warm-black rounded square with an oxidised-copper ring and a copper dot. It is a stand-in until
the real icon exists: drop `apps/desktop/packaging/icons/panoply-512.png` (and `panoply.ico`) in
place and the release scripts prefer those. Run: python3 packaging/assets/gen_icon.py
"""
import math
import struct
import zlib
from pathlib import Path

SIZE = 512
BG = (0x16, 0x12, 0x0f)
COPPER = (0xc0, 0x7a, 0x4e)


def coverage(x, y):
    """Return (alpha, color) for a point in the 0..1 unit square."""
    # Rounded square, corner radius 22% of the side.
    cx, cy = abs(x - 0.5), abs(y - 0.5)
    r, half = 0.22, 0.5
    qx, qy = max(cx - (half - r), 0), max(cy - (half - r), 0)
    if math.hypot(qx, qy) > r:
        return 0.0, BG
    d = math.hypot(x - 0.5, y - 0.5)
    if 0.255 <= d <= 0.335 or d <= 0.085:
        return 1.0, COPPER
    return 1.0, BG


def render():
    ss = 3
    rows = []
    for py in range(SIZE):
        row = bytearray([0])
        for px in range(SIZE):
            a = r = g = b = 0.0
            for sy in range(ss):
                for sx in range(ss):
                    al, col = coverage((px + (sx + 0.5) / ss) / SIZE, (py + (sy + 0.5) / ss) / SIZE)
                    a += al
                    r += col[0] * al
                    g += col[1] * al
                    b += col[2] * al
            n = ss * ss
            if a:
                row += bytes((round(r / a), round(g / a), round(b / a), round(255 * a / n)))
            else:
                row += bytes((0, 0, 0, 0))
        rows.append(bytes(row))
    return b"".join(rows)


def png(raw):
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def main():
    here = Path(__file__).parent
    data = png(render())
    (here / "panoply-512.png").write_bytes(data)
    # An .ico entry holds at most 256 pixels, so render again at 256.
    global SIZE
    SIZE = 256
    small = png(render())
    entry = struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(small), 22)
    (here / "panoply.ico").write_bytes(struct.pack("<HHH", 0, 1, 1) + entry + small)


if __name__ == "__main__":
    main()
