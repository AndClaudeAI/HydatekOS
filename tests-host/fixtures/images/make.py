#!/usr/bin/env python3
"""Regenerates the image test files (needs Pillow). Each is compared with
Pillow's own decoding in tests-host/src/image_tests.rs."""
import random
from PIL import Image, ImageDraw

random.seed(11)

def scene(w, h):
    im = Image.new("RGB", (w, h))
    px = im.load()
    for y in range(h):
        for x in range(w):
            px[x, y] = ((x * 255) // max(w - 1, 1), (y * 255) // max(h - 1, 1), ((x + y) * 7) % 256)
    d = ImageDraw.Draw(im)
    d.ellipse((w // 5, h // 5, w * 3 // 4, h * 4 // 5), fill=(250, 240, 40), outline=(20, 20, 160))
    d.rectangle((w // 2, h // 8, w - 3, h // 2), fill=(200, 30, 60))
    d.line((0, h - 1, w - 1, 0), fill=(255, 255, 255), width=2)
    for _ in range(w * h // 40):
        x, y = random.randrange(w), random.randrange(h)
        px[x, y] = (random.randrange(256), random.randrange(256), random.randrange(256))
    return im

import struct, zlib

def write_png(path, im, interlace):
    """Pillow can't write interlaced PNGs: a small writer that uses all five
    row filters in turn."""
    ch = len(im.getbands())
    w, h = im.size
    px = im.load()
    def chunk(t, b):
        return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b))
    passes = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)] if interlace else [(0, 0, 1, 1)]
    raw = bytearray()
    n = 0
    for x0, y0, dx, dy in passes:
        xs = list(range(x0, w, dx))
        prev = bytes(len(xs) * ch)
        for y in range(y0, h, dy):
            if not xs:
                break
            line = bytes(v for x in xs for v in (px[x, y] if ch > 1 else (px[x, y],)))
            f = n % 5
            n += 1
            out = bytearray()
            for i, v in enumerate(line):
                a = line[i - ch] if i >= ch else 0
                b = prev[i]
                c = prev[i - ch] if i >= ch else 0
                if f == 0: p = 0
                elif f == 1: p = a
                elif f == 2: p = b
                elif f == 3: p = (a + b) // 2
                else:
                    q = a + b - c
                    pa, pb, pc = abs(q - a), abs(q - b), abs(q - c)
                    p = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                out.append((v - p) % 256)
            raw += bytes([f]) + out
            prev = line
    ctype = {1: 0, 2: 4, 3: 2, 4: 6}[ch]
    ihdr = struct.pack(">IIBBBBB", w, h, 8, ctype, 0, 0, 1 if interlace else 0)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b""))

base = scene(67, 45)
rgba = base.convert("RGBA")
a = rgba.load()
for y in range(45):
    for x in range(67):
        r, g, b, _ = a[x, y]
        a[x, y] = (r, g, b, (x * 4 + y * 2) % 256)

# PNG: every colour type, bit depths, interlacing, palette transparency, 16-bit
base.save("rgb.png")
write_png("rgb-interlaced.png", base, True)
write_png("rgb-filters.png", base, False)
rgba.save("rgba.png")
write_png("rgba-interlaced.png", rgba, True)
base.convert("L").save("grey.png")
base.convert("LA").save("grey-alpha.png")
base.convert("1").save("mono.png")
base.convert("P", palette=Image.ADAPTIVE, colors=16).save("pal4.png", bits=4)
p = base.convert("P", palette=Image.ADAPTIVE, colors=200)
p.info["transparency"] = 3
p.save("pal8-trns.png", transparency=3)
Image.frombytes("I;16", (67, 45), bytes(random.randrange(256) for _ in range(67 * 45 * 2))).save("grey16.png")
base.convert("RGB").save("rgb-trns.png", transparency=(0, 0, 0))

# JPEG: subsamplings, greyscale, progressive, restart markers, CMYK, EXIF orientation
big = scene(203, 131)
big.save("q90-444.jpg", quality=90, subsampling=0)
big.save("q75-420.jpg", quality=75, subsampling=2)
big.save("q80-422.jpg", quality=80, subsampling=1)
big.save("progressive.jpg", quality=85, progressive=True)
big.save("progressive-444.jpg", quality=92, progressive=True, subsampling=0)
big.convert("L").save("grey.jpg", quality=85)
big.convert("L").save("grey-progressive.jpg", quality=85, progressive=True)
base.save("small-odd.jpg", quality=70)
big.convert("CMYK").save("cmyk.jpg", quality=90)
exif = Image.Exif()
exif[0x0112] = 6
base.save("rotated.jpg", quality=90, exif=exif.tobytes())
# restart intervals: Pillow can't set them, so make them with cjpeg-like options via libjpeg if present
try:
    big.save("restart.jpg", quality=85, restart_marker_rows=1)
except TypeError:
    pass

# GIF: palette, transparency, interlacing
base.convert("P", palette=Image.ADAPTIVE, colors=64).save("pal.gif")
g = base.convert("P", palette=Image.ADAPTIVE, colors=32)
g.save("trns-interlaced.gif", transparency=5, interlace=True)

# BMP
base.save("rgb24.bmp")
base.convert("P", palette=Image.ADAPTIVE, colors=256).save("pal8.bmp")
rgba.save("rgba32.bmp")
