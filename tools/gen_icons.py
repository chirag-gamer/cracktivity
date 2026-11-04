"""Generates the Tauri icon set (32/128/256 PNG + ICO) without external deps.

Regenerate: python tools/gen_icons.py
"""
import os
import struct
import zlib

ROOT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
BG = (79, 70, 229, 255)      # indigo
FG = (255, 255, 255, 255)    # white mark


def png_bytes(size: int) -> bytes:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    px = bytearray()
    c = size / 2.0
    r_outer = size * 0.47
    r_ring = size * 0.30
    r_inner = size * 0.24
    for y in range(size):
        px.append(0)  # PNG filter byte per scanline
        for x in range(size):
            d = ((x - c) ** 2 + (y - c) ** 2) ** 0.5
            if d <= r_outer:
                if r_inner <= d <= r_ring and x <= c:  # open ring, right side
                    px += bytes(FG)
                else:
                    px += bytes(BG)
            else:
                px += bytes((0, 0, 0, 0))
    raw = bytes(px)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico_from_png(png: bytes) -> bytes:
    # 256x256 entry; PNG-compressed icons are valid since Vista.
    icondir = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 22)
    return icondir + entry + png


def main() -> None:
    os.makedirs(ROOT, exist_ok=True)
    png256 = png_bytes(256)
    for size, name in ((32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png")):
        with open(os.path.join(ROOT, name), "wb") as f:
            f.write(png_bytes(size))
    with open(os.path.join(ROOT, "icon.ico"), "wb") as f:
        f.write(ico_from_png(png256))
    print("icons written to", os.path.abspath(ROOT))


if __name__ == "__main__":
    main()
