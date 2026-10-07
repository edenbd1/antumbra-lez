"""Logo and icon variants for the proposal page: the author's mark on the
Forum's 256 px tile at several sizes and colourings, a simplified mark for
small sizes, and the bare mark.

    python3 app/design/variants.py <out-dir>
"""
import sys, os, json, re
here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here); import gen
P = json.load(open(os.path.join(here, 'best.json')))
ORANGE, CREAM, DARK, PANEL = "#D44D1B", "#F8F3D2", "#101114", "#17191e"
TILE = "M56 0 H200 A56 56 0 0 1 256 56 V200 A56 56 0 0 1 200 256 H56 A56 56 0 0 1 0 200 V56 A56 56 0 0 1 56 0 Z"

def mark_d(params):
    return re.search(r'd="([^"]+)"', gen.svg(params)).group(1)

def icon(fill, bg, frac, params=P, outline=None):
    d = mark_d(params)
    s = 256 * frac / 800.0   # the mark's box is 800 units square around the sphere
    tx, ty = 128 - params['cx'] * s, 128 - params['cy'] * s
    out = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">']
    if bg: out.append(f'<path d="{TILE}" fill="{bg}"/>')
    if outline: out.append(f'<path d="{TILE}" fill="none" stroke="{outline}" stroke-width="6" transform="translate(3 3) scale(.9766)"/>')
    out.append(f'<path fill="{fill}" d="{d}" transform="translate({tx:.3f} {ty:.3f}) scale({s:.5f})"/>')
    out.append('</svg>')
    return "\n".join(out)

BANDS = dict(P, thin=0.0, band=P['band'] * 1.6)
SMALL = dict(P, n=5, pitch=P['pitch'] * 11 / 5, thin=P['thin'] * 2.2, band=P['band'] * 2.2)
VARIANTS = [
    ("L1", "Cream on orange, the mark at 66% (as now)", icon(CREAM, ORANGE, 0.66)),
    ("L2", "Cream on orange, the mark at 80%", icon(CREAM, ORANGE, 0.80)),
    ("L3", "Cream on orange, the mark at 90%", icon(CREAM, ORANGE, 0.90)),
    ("L4", "Orange on the Forum's dark panel, 80%", icon(ORANGE, PANEL, 0.80)),
    ("L5", "Orange on cream, 80%", icon(ORANGE, CREAM, 0.80)),
    ("L6", "Outline tile: orange rim and mark on dark, 76%", icon(ORANGE, DARK, 0.76, outline=ORANGE)),
    ("L7", "No tile: the bare orange mark, 92%", icon(ORANGE, None, 0.92)),
    ("L8", "Small sizes: 11 stripes instead of 23, twice as thick, cream on orange, 84%", icon(CREAM, ORANGE, 0.84, SMALL)),
    ("L9", "Small sizes: the thin lines dropped, the bands 60% thicker, cream on orange, 84%", icon(CREAM, ORANGE, 0.84, BANDS)),
]
if __name__ == "__main__":
    out = sys.argv[1]; os.makedirs(out, exist_ok=True)
    for vid, label, svg in VARIANTS:
        open(f"{out}/{vid}.svg", "w").write(svg)
    json.dump([[v, l] for v, l, _ in VARIANTS], open(f"{out}/variants.json", "w"))
