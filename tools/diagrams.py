#!/usr/bin/env python3
"""Draws the README's two diagrams from the mod's own formulas (api/src/lib.rs):

    media/blast-zones.svg     what a blast of 25,087 TNT (the one in the video) does, to scale
    media/blast-scaling.svg   how far each effect reaches, by the TNT in the mass

    python diagrams.py
"""
import math
import os

MEDIA = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "media")

# The formulas, as lib.rs has them.
BASE_SIZE, CRATER, SHOCK_SPEED, SHOCK_PUSH = 4.8, 4.5, 42.0, 1.3
LIFT_YIELD = 64
BREAKS_AT = {"glass": 0.5, "wood": 0.8, "turf": 1.5, "masonry": 2.0}


def size(y):
    return BASE_SIZE * y ** (1 / 3)


def reach(y):
    return size(y) * 3.75


def crater(y):
    return CRATER * y ** (1 / 3)


def front_speed(y):
    return SHOCK_SPEED * max(size(y) / BASE_SIZE, 1) ** 0.5


def fuse_secs(y):
    return min(80 + max(reach(y) - 18, 0) / 10 * 20, 32767) / 20


def breaks_out_to(y, push):
    """Where the shockwave's push falls to `push` blocks a tick (`shock_effect`); none below
    LIFT_YIELD, where it tears up nothing past the crater."""
    most = SHOCK_PUSH * (1 + 0.35 * math.log(max(y, 1)))
    if y < LIFT_YIELD or most <= push:
        return None
    return reach(y) * (1 - math.sqrt(push / most))


# The look: a light card of its own (it reads on GitHub's light and dark pages alike), ink for
# text, and one blue ramp, light to dark, for the zones from the edge of the shockwave in to the
# crater (steps 250, 350, 450, 550, 650: an ordinal ramp, every step 0.09 OKLCH L from the next).
SURFACE, BORDER, GRID = "#fcfcfb", "#e6e5e0", "#ecebe7"
INK, INK_2, INK_3 = "#0b0b0b", "#52514e", "#8a8986"
TIER = {"reach": "#86b6ef", "glass": "#5598e7", "wood": "#2a78d6", "masonry": "#1c5cab",
        "crater": "#104281"}
FONT = "font-family=\"Segoe UI, Helvetica, Arial, sans-serif\""


def text(x, y, s, size_px=15, fill=INK, weight="normal", anchor="start"):
    s = s.replace("&", "&amp;").replace("<", "&lt;")
    return (f'<text x="{x:.1f}" y="{y:.1f}" {FONT} font-size="{size_px}" fill="{fill}" '
            f'font-weight="{weight}" text-anchor="{anchor}">{s}</text>')


def card(width, height, body, title):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
            f'viewBox="0 0 {width} {height}" role="img" aria-label="{title}">\n'
            f'<title>{title}</title>\n'
            f'<rect x="0.5" y="0.5" width="{width - 1}" height="{height - 1}" rx="14" fill="{SURFACE}" '
            f'stroke="{BORDER}"/>\n' + "\n".join(body) + "\n</svg>\n")


def zones(y=25087):
    w, h = 900, 640
    cx, cy, rmax = 280, 330, 225
    scale = rmax / reach(y)
    r = {
        "reach": reach(y),
        "glass": breaks_out_to(y, BREAKS_AT["glass"]),
        "wood": breaks_out_to(y, BREAKS_AT["wood"]),
        "turf": breaks_out_to(y, BREAKS_AT["turf"]),
        "masonry": breaks_out_to(y, BREAKS_AT["masonry"]),
        "crater": crater(y),
    }
    px = {k: v * scale for k, v in r.items()}
    body = [
        text(32, 48, f"What {y:,} TNT does, to scale", 24, weight="bold"),
        text(32, 74, "Seen from above: how far out the shockwave of the blast in the video knocks things down.", 15, INK_2),
    ]
    # The zones, the widest first, each ringed by a 2 px gap of the card's colour.
    for key in ("reach", "glass", "wood", "masonry", "crater"):
        body.append(f'<circle cx="{cx}" cy="{cy}" r="{px[key]:.1f}" fill="{TIER[key]}" stroke="{SURFACE}" stroke-width="2"/>')
    body.append(f'<circle cx="{cx}" cy="{cy}" r="{px["turf"]:.1f}" fill="none" stroke="{SURFACE}" '
                f'stroke-width="1.6" stroke-dasharray="5 4"/>')
    body.append(f'<circle cx="{cx}" cy="{cy}" r="5" fill="{SURFACE}" stroke="{INK}" stroke-width="2"/>')
    # Labels on the right, each with a leader into its own band.
    rows = [
        ("reach", "Shockwave hurts and throws", "reach", "glass", -205, False),
        ("glass", "Glass, leaves and plants go", "glass", "wood", -150, False),
        ("wood", "Trees and wooden houses fall", "wood", "turf", -95, False),
        ("turf", "Turf and topsoil torn off", "turf", "masonry", -40, True),
        ("masonry", "Brick and stone buildings fall", "masonry", "crater", 15, False),
        ("crater", "Crater dug out", "crater", None, 70, False),
    ]
    lx = 600
    for key, label, outer, inner, dy, dashed in rows:
        ro = px[outer]
        ri = px[inner] if inner else 0
        rm = (ro + ri) / 2 if not dashed else ro
        yy = cy + dy
        if abs(dy) >= rm:
            rm = abs(dy) + 2
        ax = cx + math.sqrt(max(rm * rm - dy * dy, 0))
        body.append(f'<line x1="{ax:.1f}" y1="{yy:.1f}" x2="{lx - 14}" y2="{yy:.1f}" stroke="{INK_3}" stroke-width="1"/>')
        body.append(f'<circle cx="{ax:.1f}" cy="{yy:.1f}" r="3" fill="{INK}" stroke="{SURFACE}" stroke-width="1.5"/>')
        if dashed:
            body.append(f'<line x1="{lx}" y1="{yy - 4}" x2="{lx + 16}" y2="{yy - 4}" stroke="{INK_2}" stroke-width="2" stroke-dasharray="4 3"/>')
        else:
            body.append(f'<rect x="{lx}" y="{yy - 12}" width="16" height="16" rx="4" fill="{TIER[key]}"/>')
        body.append(text(lx + 26, yy + 1, label, 15, INK, "bold"))
        body.append(text(lx + 26, yy + 20, f"out to {r[key]:,.0f} blocks", 14, INK_2))
    # A scale bar and the blast's other numbers.
    bar = 100 * scale
    by = cy + 150
    body.append(f'<line x1="{lx}" y1="{by}" x2="{lx + bar:.1f}" y2="{by}" stroke="{INK}" stroke-width="2"/>')
    for x in (lx, lx + bar):
        body.append(f'<line x1="{x:.1f}" y1="{by - 6}" x2="{x:.1f}" y2="{by + 6}" stroke="{INK}" stroke-width="2"/>')
    body.append(text(lx + bar + 10, by + 5, "100 blocks (to scale)", 14, INK_2))
    facts = (f"Fuse {fuse_secs(y):.0f} s  ·  fireball and cloud {size(y):,.0f} blocks  ·  "
             f"shockwave front {front_speed(y):,.0f} blocks/s  ·  centre: {r['crater']:,.0f}-block crater")
    body.append(f'<line x1="32" y1="{h - 58}" x2="{w - 32}" y2="{h - 58}" stroke="{BORDER}"/>')
    body.append(text(32, h - 30, facts, 14, INK_2))
    return card(w, h, body, f"What {y:,} TNT does, to scale")


def scaling(video=25087):
    w, h = 900, 600
    x0, x1, y0, y1 = 96, 700, 500, 150  # plot box: left, right, bottom, top
    lx_min, lx_max = 0.0, 6.0  # 1 to 1,000,000 TNT
    ly_min, ly_max = 0.0, math.log10(3000)

    def X(t):
        return x0 + (math.log10(t) - lx_min) / (lx_max - lx_min) * (x1 - x0)

    def Y(d):
        return y0 - (math.log10(d) - ly_min) / (ly_max - ly_min) * (y0 - y1)

    body = [
        text(32, 48, "How far it reaches, by the TNT in the mass", 24, weight="bold"),
        text(32, 74, "Every distance grows with the cube root of the TNT, with no limit (both axes are logarithmic).", 15, INK_2),
    ]
    # Legend, in the order of the lines from top to bottom.
    series = [
        ("reach", "Shockwave hurts and throws", reach, 1),
        ("wood", "Trees and wooden houses fall", lambda t: breaks_out_to(t, BREAKS_AT["wood"]), LIFT_YIELD),
        ("masonry", "Brick and stone buildings fall", lambda t: breaks_out_to(t, BREAKS_AT["masonry"]), LIFT_YIELD),
        ("crater", "Crater", crater, 2),
    ]
    lx = 32
    for key, label, _, _ in series:
        body.append(f'<rect x="{lx}" y="96" width="14" height="14" rx="3" fill="{TIER[key]}"/>')
        body.append(text(lx + 20, 108, label, 14, INK))
        lx += 40 + len(label) * 6.9
    # Grid and axes.
    for e in range(0, 7):
        x = X(10 ** e)
        body.append(f'<line x1="{x:.1f}" y1="{y1}" x2="{x:.1f}" y2="{y0}" stroke="{GRID}"/>')
        label = ["1", "10", "100", "1,000", "10,000", "100,000", "1,000,000"][e]
        body.append(text(x, y0 + 22, label, 13, INK_2, anchor="middle"))
    for d in (1, 10, 100, 1000):
        y = Y(d)
        body.append(f'<line x1="{x0}" y1="{y:.1f}" x2="{x1}" y2="{y:.1f}" stroke="{GRID}"/>')
        body.append(text(x0 - 10, y + 5, f"{d:,}", 13, INK_2, anchor="end"))
    body.append(f'<line x1="{x0}" y1="{y0}" x2="{x1}" y2="{y0}" stroke="{INK_3}"/>')
    body.append(text((x0 + x1) / 2, y0 + 48, "TNT in the mass", 14, INK_2, anchor="middle"))
    body.append(f'<text x="34" y="{(y0 + y1) / 2:.1f}" {FONT} font-size="14" fill="{INK_2}" '
                f'text-anchor="middle" transform="rotate(-90 34 {(y0 + y1) / 2:.1f})">blocks from the centre</text>')
    # The video's blast.
    vx = X(video)
    body.append(f'<line x1="{vx:.1f}" y1="{y1 - 6}" x2="{vx:.1f}" y2="{y0}" stroke="{INK_3}" stroke-dasharray="4 4"/>')
    body.append(text(vx, y1 - 12, f"the video: {video:,} TNT", 13, INK, "bold", "middle"))
    # The lines, 2 px... 3 px for the palest, and their ends labelled.
    end_labels = []
    for key, label, f, start in series:
        pts = []
        t = start
        while t <= 1_000_000:
            d = f(t)
            # Inside the crater everything is gone: a zone shows only past it.
            if d and d >= 1 and (key in ("reach", "crater") or d > crater(t)):
                pts.append((X(t), Y(d)))
            t *= 1.08
        d_end = f(1_000_000)
        pts.append((X(1_000_000), Y(d_end)))
        path = " ".join(f"{'M' if i == 0 else 'L'}{x:.1f},{y:.1f}" for i, (x, y) in enumerate(pts))
        width = 3 if key == "reach" else 2.5
        body.append(f'<path d="{path}" fill="none" stroke="{TIER[key]}" stroke-width="{width}" '
                    f'stroke-linecap="round" stroke-linejoin="round"/>')
        dv = f(video)
        body.append(f'<circle cx="{vx:.1f}" cy="{Y(dv):.1f}" r="5" fill="{TIER[key]}" stroke="{SURFACE}" stroke-width="2"/>')
        body.append(text(vx - 10, Y(dv) + 5, f"{dv:,.0f}", 13, INK, "bold", "end"))
        end_labels.append((Y(d_end), f"{d_end:,.0f} · {label.split(' ')[0].lower() if key != 'reach' else 'shockwave'}"))
    # End labels, nudged apart.
    end_labels.sort()
    last = -1e9
    for y, s in end_labels:
        y = max(y + 5, last + 17)
        body.append(text(x1 + 10, y, s, 13, INK_2))
        last = y
    body.append(f'<line x1="32" y1="{h - 46}" x2="{w - 32}" y2="{h - 46}" stroke="{BORDER}"/>')
    body.append(text(32, h - 20, "Trees and buildings are knocked down by blasts of 64 TNT or more; one TNT keeps vanilla's blast.", 14, INK_2))
    return card(w, h, body, "How far it reaches, by the TNT in the mass")


def main():
    os.makedirs(MEDIA, exist_ok=True)
    for name, svg in (("blast-zones.svg", zones()), ("blast-scaling.svg", scaling())):
        with open(os.path.join(MEDIA, name), "w", encoding="utf-8", newline="\n") as out:
            out.write(svg)
        print("wrote", name)


if __name__ == "__main__":
    main()
