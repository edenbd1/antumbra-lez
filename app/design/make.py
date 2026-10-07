"""The Antumbra Vesting logo, redrawn as vectors from the author's bitmap.

gen.py holds the geometry (23 thin lines and 24 thick bands in a circle, the
thick bands cut by 60-degree edges, point-symmetric about the centre);
best.json holds its parameters, fitted to the original 1504 x 1224 bitmap by
maximising the overlap of the two rasterised shapes (IoU 0.978). This script
writes the marks and the 256 px icon tiles:

    python3 app/design/make.py <out-dir>
"""
import sys, json, math, re, os
here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here); import gen
P = json.load(open(os.path.join(here, 'best.json')))
full = gen.svg(P)
D = re.search(r'd="([^"]+)"', full).group(1)
CX, CY = P['cx'], P['cy']
# The mark's own box: the sphere plus nothing. Square, centred on the sphere.
H = 400.0
VB = (CX - H, CY - H, 2 * H, 2 * H)

def mark_path(fill, extra=""):
    return f'<path fill="{fill}" {extra} d="{D}"/>'

def logo(fill, size=512, title="Antumbra Vesting"):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{VB[0]:.2f} {VB[1]:.2f} {VB[2]:.0f} {VB[3]:.0f}" width="{size}" height="{size}">\n'
            f'<title>{title}</title>\n{mark_path(fill)}\n</svg>\n')

OPTIONS = {
    # name: tile gradient stops (deep -> light), extrusion colour, mark colours
    "orange": dict(stops=["#c63c12", "#d34e1e", "#de6a37", "#e8915c"], ext="#8f2a0c", mark=["#fffaf0", "#f8efd3"]),
    "teal":   dict(stops=["#0b6f63", "#12897a", "#20a491", "#4cc2ad"], ext="#06463e", mark=["#ffffff", "#e3f6f1"]),
    "violet": dict(stops=["#4b2fd0", "#5d40e6", "#7a5ef2", "#a189fb"], ext="#2c1a8c", mark=["#ffffff", "#ece6ff"]),
}

def icon(opt):
    o = OPTIONS[opt]
    s = 168 / (2 * H)              # the mark spans 168 of 256 px, like the forum's bubble
    tx, ty = 128 - CX * s, 128 - CY * s
    st = "".join(f'<stop offset="{i/(len(o["stops"])-1):.2f}" stop-color="{c}"/>' for i, c in enumerate(o["stops"]))
    ext = "".join(f'<g transform="translate({-0.5*i:.1f} {1.0*i:.1f})"><path fill="{o["ext"]}" d="{D}" transform="translate({tx:.3f} {ty:.3f}) scale({s:.5f})"/></g>' for i in range(0))
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">
<defs>
<radialGradient id="tile" cx=".28" cy=".66" r=".86" fx=".26" fy=".68">{st}</radialGradient>
<linearGradient id="mk" gradientUnits="userSpaceOnUse" x1="0" y1="40" x2="0" y2="215"><stop offset="0" stop-color="{o["mark"][0]}"/><stop offset="1" stop-color="{o["mark"][1]}"/></linearGradient>
</defs>
<path d="M56 0 H200 A56 56 0 0 1 256 56 V200 A56 56 0 0 1 200 256 H56 A56 56 0 0 1 0 200 V56 A56 56 0 0 1 56 0 Z" fill="url(#tile)"/>
<path d="M56 0 H200 A56 56 0 0 1 256 56 V200 A56 56 0 0 1 200 256 H56 A56 56 0 0 1 0 200 V56 A56 56 0 0 1 56 0 Z" fill="none" stroke="#fff" stroke-opacity=".16" stroke-width="2" transform="translate(1 1) scale(.9922)"/>
{ext}
<path fill="url(#mk)" d="{D}" transform="translate({tx:.3f} {ty:.3f}) scale({s:.5f})"/>
</svg>
'''
if __name__ == "__main__":
    out = sys.argv[1]
    import os; os.makedirs(out, exist_ok=True)
    for k in OPTIONS:
        open(f"{out}/icon-{k}.svg", "w").write(icon(k))
    open(f"{out}/mark-cream.svg", "w").write(logo("#F8F3D2"))
    open(f"{out}/mark-dark.svg", "w").write(logo("#101114"))
    open(f"{out}/mark-orange.svg", "w").write(logo("#D44D1B"))
    open(f"{out}/mark-teal.svg", "w").write(logo("#2BB39E"))
    open(f"{out}/mark-violet.svg", "w").write(logo("#8B6CFF"))
    open(f"{out}/original-recreated.svg", "w").write(gen.svg(P, bg="#D44D1B"))
