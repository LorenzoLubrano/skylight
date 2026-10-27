#!/usr/bin/env python3
"""Genera le icone di Skylight: python tools/make_icon.py assets  (richiede Pillow).

Parte da assets/skylight-1024.png (l'icona grande, già ripulita e quadrata).
Dai 32 px in giù le sfumature diventano una macchia, quindi le misure piccole
usano una versione ridisegnata a tinta unita: pillola nera, cono di luce,
quadrato blu.
"""
import sys
from PIL import Image, ImageDraw

out = sys.argv[1]
master = Image.open(f"{out}/skylight-1024.png").convert("RGBA")

# Versione semplificata, disegnata a 8x e poi ridotta
S = 1024
BLUE_TOP, BLUE_BOTTOM = (34, 92, 214), (16, 52, 160)
PILL = (10, 12, 20, 255)
LIGHT = (255, 214, 120)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def simple():
    img = Image.new("RGBA", (S, S))
    d = ImageDraw.Draw(img)
    for y in range(S):
        d.line([(0, y), (S, y)], fill=lerp(BLUE_TOP, BLUE_BOTTOM, y / S) + (255,))

    # Cono di luce: trapezio pieno che si allarga verso il basso
    px0, px1, py = S * 0.17, S * 0.83, S * 0.36
    d.polygon([(px0, py), (px1, py), (S * 0.90, S * 0.78), (S * 0.10, S * 0.78)], fill=LIGHT + (255,))

    # Pillola nera, larga e spessa per restare visibile a 16 px
    ph = S * 0.22
    d.rounded_rectangle([px0, py - ph, px1, py], radius=int(ph / 2), fill=PILL)

    # Ritaglio a quadrato arrotondato
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.22), fill=255)
    img.putalpha(mask)
    return img


small = simple()


def icon(size):
    src = small if size <= 32 else master
    return src.resize((size, size), Image.LANCZOS)


icon(256).save(f"{out}/icon-256.png", optimize=True)
for s in (16, 20, 24, 32, 40, 48, 64):
    icon(s).save(f"{out}/tray-{s}.png", optimize=True)

# .ico con ogni misura scelta a mano (Pillow altrimenti ridurrebbe solo la grande)
sizes = (16, 20, 24, 32, 40, 48, 64, 128, 256)
frames = [icon(s) for s in sizes]
frames[-1].save(f"{out}/skylight.ico", sizes=[(s, s) for s in sizes], append_images=frames[:-1])
