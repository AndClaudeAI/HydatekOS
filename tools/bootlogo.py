#!/usr/bin/env python3
"""Make the boot logo from the Hydatek Systems wordmark.

    python3 tools/bootlogo.py [source image] -> kernel/assets/boot-logo.png

The source is dark lettering on a light background
(assets/branding/hydatek-systems.jpg). The output is a greyscale PNG cropped to
the lettering, where the grey level is how much of each pixel the lettering
covers (255 = solid), so HydatekOS can draw it in any colour; at boot it is
white on black. Needs Pillow (build machine only; the kernel decodes the PNG
itself).
"""
import os
import sys

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "assets/branding/hydatek-systems.jpg")
out = os.path.join(ROOT, "kernel/assets/boot-logo.png")

grey = Image.open(src).convert("L")
w, h = grey.size
px = grey.load()
# the lettering: clearly dark pixels, away from the grey page edges
xs, ys = [], []
for y in range(h):
    for x in range(w):
        if px[x, y] < 100:
            xs.append(x)
            ys.append(y)
# ignore stray dark pixels at the image's left and right borders
inner = [(x, y) for x, y in zip(xs, ys) if w * 0.05 < x < w * 0.95]
x0 = min(x for x, _ in inner)
x1 = max(x for x, _ in inner)
y0 = min(y for _, y in inner)
y1 = max(y for _, y in inner)
pad = 6
box = (max(x0 - pad, 0), max(y0 - pad, 0), min(x1 + pad + 1, w), min(y1 + pad + 1, h))
crop = grey.crop(box)
# coverage: paper (light) -> 0, ink (dark) -> 255, with a soft edge for anti-aliasing
def cover(v):
    return max(0, min(255, (230 - v) * 255 // 190))
mask = crop.point(cover)
mask.save(out, optimize=True)
print("%s: %dx%d (from %s, box %s)" % (out, mask.size[0], mask.size[1], os.path.basename(src), box))
