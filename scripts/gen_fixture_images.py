#!/usr/bin/env python3
"""Draw the fixture images in fixtures/images/ (standard library only, deterministic).

The PNGs are committed, so this only needs re-running to change them:

    python3 scripts/gen_fixture_images.py
"""

import math
import random
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "images"
W, H = 960, 640


# ---- tiny raster toolkit -------------------------------------------------------------


def hexrgb(h: str):
    h = h.lstrip("#")
    return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))


def mix(a, b, t):
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


def smooth(edge0, edge1, x):
    t = max(0.0, min(1.0, (x - edge0) / (edge1 - edge0)))
    return t * t * (3 - 2 * t)


class Canvas:
    def __init__(self, w=W, h=H):
        self.w, self.h = w, h
        self.px = [[(0.0, 0.0, 0.0)] * w for _ in range(h)]

    def vertical_gradient(self, stops):
        """stops: [(t, color)] with t in 0..1 from top to bottom."""
        for y in range(self.h):
            t = y / (self.h - 1)
            for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
                if t0 <= t <= t1:
                    c = mix(c0, c1, (t - t0) / (t1 - t0) if t1 > t0 else 0)
                    break
            self.px[y] = [c] * self.w

    def blend(self, x, y, color, alpha):
        if 0 <= x < self.w and 0 <= y < self.h and alpha > 0:
            self.px[y][x] = mix(self.px[y][x], color, min(alpha, 1.0))

    def shape(self, bbox, sdf, color, alpha=1.0, soft=1.0, shade=None):
        """Fills where the signed distance `sdf(x, y)` < 0, antialiased over `soft` px.
        `shade(x, y, d)` may return a per-pixel color."""
        x0, y0, x1, y1 = (int(max(0, bbox[0])), int(max(0, bbox[1])), int(min(self.w, bbox[2])), int(min(self.h, bbox[3])))
        for y in range(y0, y1):
            row = self.px[y]
            for x in range(x0, x1):
                d = sdf(x + 0.5, y + 0.5)
                if d < soft:
                    a = alpha * (1 - smooth(-soft, soft, d))
                    c = shade(x, y, d) if shade else color
                    row[x] = mix(row[x], c, a)

    def circle(self, cx, cy, r, color, alpha=1.0, shade=None, soft=1.0):
        m = r + soft + 2
        self.shape((cx - m, cy - m, cx + m, cy + m),
                   lambda x, y: math.hypot(x - cx, y - cy) - r, color, alpha, soft, shade)

    def ellipse(self, cx, cy, rx, ry, angle, color, alpha=1.0, shade=None):
        ca, sa = math.cos(angle), math.sin(angle)
        m = max(rx, ry) + 2

        def sdf(x, y):
            dx, dy = x - cx, y - cy
            u, v = dx * ca + dy * sa, -dx * sa + dy * ca
            k = math.hypot(u / rx, v / ry)
            return (k - 1) * min(rx, ry)

        self.shape((cx - m, cy - m, cx + m, cy + m), sdf, color, alpha, 1.0, shade)

    def rrect(self, x0, y0, x1, y1, r, color, alpha=1.0, shade=None):
        cx, cy, hx, hy = (x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2 - r, (y1 - y0) / 2 - r

        def sdf(x, y):
            qx, qy = abs(x - cx) - hx, abs(y - cy) - hy
            return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r

        self.shape((x0 - 2, y0 - 2, x1 + 2, y1 + 2), sdf, color, alpha, 1.0, shade)

    def polyline(self, pts, width, color, alpha=1.0):
        """A stroke through `pts`. Coverage is merged across segments before blending,
        so translucent strokes have no darker joints."""
        r = width / 2
        cover: dict[tuple[int, int], float] = {}
        for (ax, ay), (bx, by) in zip(pts, pts[1:]):
            vx, vy = bx - ax, by - ay
            vv = vx * vx + vy * vy or 1
            for y in range(int(max(0, min(ay, by) - r - 2)), int(min(self.h, max(ay, by) + r + 2))):
                for x in range(int(max(0, min(ax, bx) - r - 2)), int(min(self.w, max(ax, bx) + r + 2))):
                    px, py = x + 0.5 - ax, y + 0.5 - ay
                    t = max(0.0, min(1.0, (px * vx + py * vy) / vv))
                    d = math.hypot(px - vx * t, py - vy * t) - r
                    if d < 1:
                        a = 1 - smooth(-1, 1, d)
                        if a > cover.get((x, y), 0):
                            cover[(x, y)] = a
        for (x, y), a in cover.items():
            self.px[y][x] = mix(self.px[y][x], color, a * alpha)

    def fill_below(self, f, color, alpha=1.0, shade=None):
        """Fills every pixel below the curve y = f(x), antialiased at the edge."""
        for x in range(self.w):
            top = f(x + 0.5)
            for y in range(max(0, int(top) - 1), self.h):
                a = alpha * smooth(top - 0.5, top + 0.5, y + 0.5)
                c = shade(x, y) if shade else color
                self.px[y][x] = mix(self.px[y][x], c, a)

    def save(self, name):
        raw = bytearray()
        for row in self.px:
            line = bytearray()
            for r, g, b in row:
                line += bytes((int(max(0, min(255, r))), int(max(0, min(255, g))), int(max(0, min(255, b)))))
            # PNG "Sub" filter: smooth gradients compress far better.
            filtered = bytearray(line[:3])
            for i in range(3, len(line)):
                filtered.append((line[i] - line[i - 3]) & 0xFF)
            raw += b"\x01" + filtered

        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

        png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", self.w, self.h, 8, 2, 0, 0, 0))
        png += chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / name).write_bytes(png)
        print(f"  {name}  {len(png) // 1024} KB")


# ---- illustrations -------------------------------------------------------------------


def ridge_sunrise():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#2b2a4c")), (0.45, hexrgb("#e6795a")), (0.7, hexrgb("#f6c983")), (1, hexrgb("#f6c983"))])
    c.circle(610, 380, 70, hexrgb("#fff1c9"), 0.95)
    c.circle(610, 380, 140, hexrgb("#ffd89a"), 0.18, soft=60)
    layers = [("#8c5a6b", 380, 40, 0.008, 1.2), ("#5e4462", 430, 55, 0.006, 2.1), ("#3b2f4a", 500, 50, 0.011, 0.4), ("#231d30", 560, 35, 0.017, 3.0)]
    for color, base, amp, freq, phase in layers:
        c.fill_below(lambda x, b=base, a=amp, f=freq, p=phase: b - a * math.sin(x * f + p) - a * 0.4 * math.sin(x * f * 2.7 + p * 2), hexrgb(color))
    # trail
    c.polyline([(120, 640), (260, 590), (380, 600), (520, 570), (700, 585)], 4, hexrgb("#f0b27a"), 0.55)
    c.save("ridge-sunrise.png")


def tomato():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#f4efe3")), (1, hexrgb("#e3dccb"))])
    c.ellipse(480, 520, 210, 30, 0, hexrgb("#c9bfa8"), 0.6)
    red, dark, hi = hexrgb("#d9412b"), hexrgb("#8f2316"), hexrgb("#ff8c6b")

    def shade(x, y, d):
        t = math.hypot(x - 430, y - 270) / 260
        return mix(mix(hi, red, min(1, t * 1.6)), dark, max(0, t - 0.55) * 1.6)

    c.ellipse(480, 340, 190, 165, 0, red, shade=shade)
    c.ellipse(420, 270, 40, 22, -0.5, hexrgb("#ffd2bf"), 0.55)
    for i in range(5):
        a = -math.pi / 2 + i * 2 * math.pi / 5
        c.ellipse(480 + 45 * math.cos(a), 185 + 18 * math.sin(a), 52, 13, a, hexrgb("#3f7d3a"))
    c.rrect(472, 140, 488, 190, 6, hexrgb("#4f6b2c"))
    c.save("tomato.png")


def lentil_soup():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#3a2e26")), (1, hexrgb("#2a211b"))])
    for i in range(0, W, 48):  # wooden table planks
        c.rrect(i, -10, i + 44, H + 10, 4, hexrgb("#4a3a2f"), 0.35)
    c.circle(480, 320, 250, hexrgb("#efe6d8"))
    c.circle(480, 320, 205, hexrgb("#d9cdb9"))
    soup = hexrgb("#e08a2e")
    c.circle(480, 320, 190, soup, shade=lambda x, y, d: mix(hexrgb("#f2a64a"), hexrgb("#c46b1c"), min(1, math.hypot(x - 450, y - 290) / 210)))
    rng = random.Random(4)
    for _ in range(70):  # herbs and lentils
        a, r = rng.uniform(0, 2 * math.pi), rng.uniform(0, 170)
        x, y = 480 + r * math.cos(a), 320 + r * math.sin(a)
        if rng.random() < 0.45:
            c.ellipse(x, y, 7, 3, rng.uniform(0, 3), hexrgb("#4f7a33"))
        else:
            c.circle(x, y, 4, hexrgb("#b5561c"), 0.9)
    c.ellipse(560, 250, 28, 10, -0.4, hexrgb("#f6e7a8"), 0.9)  # lemon
    c.ellipse(790, 470, 70, 22, -0.8, hexrgb("#c7c3bb"))  # spoon
    c.save("lentil-soup.png")


def latency_chart():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#fffcf7")), (1, hexrgb("#f6efe4"))])
    left, right, top, bottom = 110, 880, 90, 540
    for i in range(5):
        y = top + i * (bottom - top) / 4
        c.polyline([(left, y), (right, y)], 1.5, hexrgb("#e6ddd0"))
    c.polyline([(left, top - 10), (left, bottom), (right + 10, bottom)], 3, hexrgb("#8a8278"))
    rng = random.Random(7)
    before = [(left + i * 22, top + 70 + rng.uniform(-30, 40)) for i in range(18)]
    drop_x = before[-1][0]
    after = [(drop_x + 22 + i * 22, bottom - 110 + rng.uniform(-12, 12)) for i in range(17)]
    pts = before + [(drop_x + 11, (before[-1][1] + after[0][1]) / 2)] + after
    c.shape((left, top, right, bottom), lambda x, y: drop_x - x, hexrgb("#dcebdf"), 0.6)  # after: improved
    c.polyline([(drop_x, top - 10), (drop_x, bottom)], 2, hexrgb("#d4542b"), 0.6)
    c.polyline(pts, 5, hexrgb("#d4542b"))
    for x, y in pts[-1:]:
        c.circle(x, y, 9, hexrgb("#d4542b"))
        c.circle(x, y, 4, hexrgb("#fffcf7"))
    c.save("latency-chart.png")


def book_stack():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#e9e2d5")), (1, hexrgb("#d8cfbf"))])
    c.rrect(0, 520, W, H, 0, hexrgb("#8a6a4f"))
    colors = ["#3d6b4a", "#c2532d", "#2f4f6f", "#e2b04a", "#6b4a8a", "#a23b5a", "#2f6f73"]
    rng = random.Random(3)
    y = 520
    for i, col in enumerate(colors):
        h = rng.randint(44, 62)
        w = rng.randint(300, 440)
        x = 480 - w / 2 + rng.randint(-40, 40)
        c.rrect(x, y - h, x + w, y, 6, hexrgb(col))
        c.rrect(x + 20, y - h + 12, x + w - 20, y - h + 16, 2, hexrgb("#f4efe7"), 0.55)
        c.rrect(x + 20, y - 16, x + w - 20, y - 12, 2, hexrgb("#f4efe7"), 0.35)
        y -= h
    c.save("book-stack.png")


def basil():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#f1f4e8")), (1, hexrgb("#dfe7cf"))])
    c.rrect(350, 430, 610, 620, 18, hexrgb("#b8643e"))
    c.rrect(330, 410, 630, 450, 12, hexrgb("#c97650"))
    rng = random.Random(11)
    for i in range(26):
        a = -math.pi / 2 + rng.uniform(-1.3, 1.3)
        dist = rng.uniform(40, 210)
        x, y = 480 + dist * math.cos(a) * 1.2, 400 + dist * math.sin(a)
        c.polyline([(480, 420), (x, y)], 4, hexrgb("#4d7a2e"), 0.8)
        g = hexrgb(rng.choice(["#3f8a3a", "#4f9d42", "#5aa84a", "#367a32"]))
        c.ellipse(x, y, 46, 26, a, g, shade=lambda px, py, d, g=g: mix(g, hexrgb("#a4d27a"), max(0, -d / 40) * 0.35))
    c.save("basil.png")


def sourdough_jar():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#f7efe2")), (1, hexrgb("#eadbc4"))])
    c.rrect(300, 120, 660, 580, 60, hexrgb("#dfe9ee"), 0.55)
    c.rrect(320, 260, 640, 560, 46, hexrgb("#f3e3c3"))
    rng = random.Random(5)
    for _ in range(80):
        x, y, r = rng.uniform(345, 615), rng.uniform(280, 540), rng.uniform(3, 12)
        c.circle(x, y, r, hexrgb("#d9c39a"))
        c.circle(x - r / 3, y - r / 3, r / 3, hexrgb("#fff8ea"), 0.7)
    c.rrect(310, 90, 650, 130, 12, hexrgb("#a8794f"))
    c.polyline([(330, 252), (630, 252)], 6, hexrgb("#c2532d"), 0.9)  # rubber band at the start level
    c.save("sourdough-starter.png")


def brush_strokes():
    c = Canvas()
    c.vertical_gradient([(0, hexrgb("#fbf7ef")), (1, hexrgb("#f3ecdf"))])
    rng = random.Random(9)
    for i in range(9):
        y0 = 90 + i * 55
        pts = [(80 + t * 8, y0 + 26 * math.sin(t * 0.09 + i) + rng.uniform(-2, 2)) for t in range(101)]
        width = 4 + 10 * abs(math.sin(i * 0.8))
        c.polyline(pts, width, hexrgb("#1d1a16"), 0.85 - i * 0.05)
    c.circle(780, 160, 46, hexrgb("#d4542b"), 0.9)
    c.save("brush-strokes.png")


def pattern(name, seed, palette):
    """Abstract geometric tiles for generated posts."""
    c = Canvas()
    rng = random.Random(seed)
    c.vertical_gradient([(0, hexrgb(palette[0])), (1, hexrgb(palette[1]))])
    for _ in range(14):
        col = hexrgb(rng.choice(palette[2:]))
        kind = rng.random()
        x, y = rng.uniform(0, W), rng.uniform(0, H)
        if kind < 0.45:
            c.circle(x, y, rng.uniform(30, 140), col, rng.uniform(0.5, 0.9))
        elif kind < 0.8:
            w, h = rng.uniform(60, 260), rng.uniform(40, 200)
            c.rrect(x, y, x + w, y + h, rng.uniform(6, 30), col, rng.uniform(0.5, 0.9))
        else:
            pts = [(x + i * 30, y + 30 * math.sin(i * 0.9)) for i in range(12)]
            c.polyline(pts, rng.uniform(6, 16), col, 0.8)
    c.save(name)


def main():
    print(f"writing {OUT}")
    ridge_sunrise()
    tomato()
    lentil_soup()
    latency_chart()
    book_stack()
    basil()
    sourdough_jar()
    brush_strokes()
    pattern("pattern-ember.png", 1, ["#f4efe7", "#ead9c6", "#d4542b", "#e8a25c", "#1d1a16", "#3d6b4a"])
    pattern("pattern-moss.png", 2, ["#e7efe3", "#cfdcc6", "#3d6b4a", "#7fb38d", "#2f6f73", "#e2b04a"])
    pattern("pattern-dusk.png", 3, ["#2b2a4c", "#4a3a5e", "#e6795a", "#f6c983", "#8c5a6b", "#fff1c9"])
    pattern("pattern-ink.png", 4, ["#fbf7ef", "#efe6d6", "#1d1a16", "#5b5f97", "#a23b5a", "#c2532d"])


if __name__ == "__main__":
    main()
