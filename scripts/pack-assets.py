#!/usr/bin/env python3
"""Regenerate embedded font masks and icon RGBA (development only; requires Pillow)."""
from pathlib import Path
import struct
from PIL import Image, ImageFont

root = Path(__file__).resolve().parent.parent
size = 128
font = ImageFont.truetype(str(root / "assets/font.ttf"), size)
chars = list(map(chr, range(32, 127))) + ["—", "→", "…", "✓", "−", "·", "\ufffd"]
out = bytearray(b"DWFT" + struct.pack("<HH", size, len(chars)))
for char in chars:
    mask, offset = font.getmask2(char, mode="L", anchor="ls")
    w, h = mask.size
    data = bytes(mask)
    runs = bytearray()
    pos = 0
    while pos < len(data):
        end = pos + 1
        while end < len(data) and data[end] == data[pos] and end - pos < 65535:
            end += 1
        runs += struct.pack("<HB", end - pos, data[pos])
        pos = end
    out += struct.pack("<IfhhHHI", ord(char), font.getlength(char), *offset, w, h, len(runs)) + runs
(root / "assets/font.dwf").write_bytes(out)
icon = Image.open(root / "assets/icon.png").convert("RGBA").resize((256, 256))
(root / "assets/icon.rgba").write_bytes(icon.tobytes())
print(f"Packed {len(chars)} glyphs and 256px icon")
