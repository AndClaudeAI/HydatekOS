#!/usr/bin/env python3
"""HydatekOS font atlas generator.

HydatekOS has no TrueType engine in the kernel (yet). Instead, glyphs are
pre-rasterised at build time into 8-bit coverage bitmaps and packed into a
single blob that the kernel embeds with `include_bytes!`.

Pack layout (little endian):
    b"HFPK" u16 face_count
    per face:
        u8 face_id, u8 reserved, u16 px, i16 ascent, i16 descent,
        u16 glyph_count, u32 data_len
        per glyph: u32 codepoint, i16 left, i16 top, u16 w, u16 h, u16 adv64
        coverage bytes (glyphs back to back, row major)

Usage: python3 tools/fontgen.py   (writes kernel/assets/fonts.bin)
"""
import math
import os
import struct
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONTS = os.path.join(ROOT, "assets", "fonts")
OUT = os.path.join(ROOT, "kernel", "assets", "fonts.bin")

# Special characters for the on-screen keyboard (currencies incl. the Naira).
SPECIAL = "₦€£¥¢₹₵§¶®™±÷¿¡«»µ¬¤¦"
TEXT = [chr(c) for c in range(32, 127)] + list("·–—•…°©’‘“”×‹›←→✓Σ≤≥≠" + SPECIAL)
DIGITS = list("0123456789: ")

# Glyphs a face lacks (Figtree has no ₦, ₹ or ₵) come from DejaVu Sans,
# scaled so its capitals match the face's cap height.
FALLBACK = {0: "dejavu-sans.ttf", 1: "dejavu-sans.ttf", 2: "dejavu-sans-bold.ttf"}

# face_id -> (file, charset, logical sizes). Must match kernel/src/font.rs
FACES = {
    0: ("figtree-400.ttf", TEXT, [10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 28]),
    1: ("figtree-500.ttf", TEXT, [10, 11, 12, 13, 14, 15, 16, 18, 20, 24]),
    2: ("figtree-600.ttf", TEXT, [10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 28, 36]),
    3: ("bodoni-moda-500.ttf", DIGITS, [48, 64, 96]),
    4: ("dejavu-sans-mono.ttf", TEXT, [12, 13, 14]),
    # Figtree ships no italics: 5 and 6 are slanted (oblique) renderings of
    # the regular and semibold weights, for documents.
    5: ("figtree-400.ttf", TEXT, [11, 13, 15, 18, 22, 28]),
    6: ("figtree-600.ttf", TEXT, [11, 13, 15, 18, 22, 28, 36]),
}
OBLIQUE = {5, 6}
SLANT = 0.2
FALLBACK[5] = FALLBACK[0]
FALLBACK[6] = FALLBACK[2]
SCALES = [1, 2]


def cap_height(font):
    l, t, r, b = font.getbbox("H", anchor="ls")
    return b - t


def render_face(face_id, path, chars, px):
    from fontTools.ttLib import TTFont
    font = ImageFont.truetype(path, px)
    ascent, descent = font.getmetrics()
    cmap = TTFont(path).getBestCmap()
    fb = None
    if face_id in FALLBACK:
        fpath = os.path.join(FONTS, FALLBACK[face_id])
        probe = ImageFont.truetype(fpath, px)
        fb = ImageFont.truetype(fpath, max(1, round(px * cap_height(font) / max(1, cap_height(probe)))))
    glyphs, data = [], bytearray()
    for ch in chars:
        if ord(ch) in cmap:
            font_for = font
        elif fb is not None:
            font_for = fb
        else:
            continue
        adv = font_for.getlength(ch)
        l, t, r, b = font_for.getbbox(ch, anchor="ls")
        w, h = max(0, r - l), max(0, b - t)
        if w and h:
            img = Image.new("L", (w, h), 0)
            ImageDraw.Draw(img).text((-l, -t), ch, font=font_for, fill=255, anchor="ls")
            if face_id in OBLIQUE:
                # shear: rows above the baseline move right, below move left
                o = int(math.ceil(max(b, 0) * SLANT))
                nw = w + int(math.ceil(h * SLANT)) + 1
                img = img.transform((nw, h), Image.AFFINE, (1, SLANT, -o + t * SLANT, 0, 1, 0), resample=Image.BILINEAR)
                l -= o
                w = nw
            buf = img.tobytes()
        else:
            w = h = 0
            buf = b""
        glyphs.append(struct.pack("<IhhHHH", ord(ch), l, -t, w, h, int(round(adv * 64))))
        data += buf
    head = struct.pack("<BBHhhHI", face_id, 0, px, ascent, descent, len(glyphs), len(data))
    return head + b"".join(glyphs) + bytes(data)


def main():
    faces = []
    for fid, (fname, chars, sizes) in FACES.items():
        path = os.path.join(FONTS, fname)
        for s in sorted({s * k for s in sizes for k in SCALES}):
            faces.append(render_face(fid, path, chars, s))
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "wb") as f:
        f.write(b"HFPK" + struct.pack("<H", len(faces)))
        for blob in faces:
            f.write(blob)
    print(f"wrote {OUT} ({os.path.getsize(OUT)} bytes, {len(faces)} faces)")


if __name__ == "__main__":
    main()
