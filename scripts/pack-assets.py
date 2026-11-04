#!/usr/bin/env python3
"""Regenerate embedded font masks and icon RGBA (development only; requires Pillow)."""
from pathlib import Path
import struct
from PIL import Image, ImageFont

root = Path(__file__).resolve().parent.parent
size = 128
# Nunito (OFL, assets/FONT-LICENSE.txt): ExtraBold for headings, buttons and
# labels; SemiBold for body text.
chars = list(map(chr, range(32, 127))) + ["—", "…", "−", "·", "\ufffd"]


def pack(ttf, dwf):
    font = ImageFont.truetype(str(root / "assets/fonts" / ttf), size)
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
    (root / "assets" / dwf).write_bytes(out)


pack("Nunito-ExtraBold.ttf", "font.dwf")
pack("Nunito-SemiBold.ttf", "font-body.dwf")
icon = Image.open(root / "assets/icon.png").convert("RGBA").resize((256, 256))
(root / "assets/icon.rgba").write_bytes(icon.tobytes())
print(f"Packed {len(chars)} glyphs in two weights and the 256px icon")
