import math, json, sys
# Geometry of the author's logo, measured from its 1504 x 1224 bitmap. These are the starting values; best.json holds the fitted ones.
P = dict(cx=751.25, cy=611.4, R=393.0, pitch=34.36, thin=4.6, band=12.9,
         apexx=751.5, apexy=218.0, s=0.5774,  # dx/dy of the 60-degree edges
         llx=692.0, lly=321.0,              # a point on the left edge of the diagonal band
         bw=113.0,                          # horizontal width of the diagonal band
         rx=811.0, ry=321.0)                # a point on the right-hand edge (R)
def build(P):
    cx, cy, R, p = P['cx'], P['cy'], P['R'], P['pitch']
    s = P['s']
    LL = lambda y: P['llx'] - s * (y - P['lly'])
    LR = lambda y: LL(y) + P['bw']
    RR = lambda y: P['rx'] + s * (y - P['ry'])
    # y where LR meets RR: the top of the hollow triangle
    yh = (P['llx'] + P['bw'] - P['rx'] + s * (P['lly'] + P['ry'])) / (2 * s) if False else None
    def chord(y):
        d = R * R - (y - cy) ** 2
        return (cx - math.sqrt(d), cx + math.sqrt(d)) if d > 0 else None
    shapes = []  # each: (y1, y2, leftkind, rightkind, xl(y), xr(y))
    t = P['thin']; b = P['band']
    n = int(P.get('n', 11))
    for k in range(-n, 1):
        yk = cy + p * k
        # thin line, full chord
        if t > 0: shapes.append((yk - t / 2, yk + t / 2, 'c', 'c', None, None))
        if k == 0:
            y1, y2 = yk - t / 2 - b, yk - t / 2
            shapes.append((y1, y2, 'l', 'l', LL, LR))
            continue
        y1, y2 = yk - t / 2 - b, yk - t / 2
        if k == -n:
            y1 = P['apexy']
        hollow = RR(y1) > LR(y1)   # the diagonal band and the right edge have separated
        if hollow:
            shapes.append((y1, y2, 'l', 'l', LL, LR))
            shapes.append((y1, y2, 'l', 'c', RR, None))
        else:
            shapes.append((y1, y2, 'l', 'l', LL, RR))
    return shapes
def path(shape, rot=False, P=P):
    cx, cy, R = P['cx'], P['cy'], P['R']
    y1, y2, lk, rk, xl, xr = shape
    def ch(y): d = math.sqrt(max(R * R - (y - cy) ** 2, 0)); return cx - d, cx + d
    pts = []
    xl1 = ch(y1)[0] if lk == 'c' else xl(y1); xl2 = ch(y2)[0] if lk == 'c' else xl(y2)
    xr1 = ch(y1)[1] if rk == 'c' else xr(y1); xr2 = ch(y2)[1] if rk == 'c' else xr(y2)
    # clip straight ends to the circle too
    if lk == 'l': xl1 = max(xl1, ch(y1)[0]); xl2 = max(xl2, ch(y2)[0])
    if rk == 'l': xr1 = min(xr1, ch(y1)[1]); xr2 = min(xr2, ch(y2)[1])
    f = (lambda x, y: (2 * cx - x, 2 * cy - y)) if rot else (lambda x, y: (x, y))
    def P_(x, y): x, y = f(x, y); return f"{x:.2f} {y:.2f}"
    d = f"M{P_(xl1, y1)} L{P_(xr1, y1)} "
    d += (f"A{R:.2f} {R:.2f} 0 0 1 {P_(xr2, y2)} " if rk == 'c' else f"L{P_(xr2, y2)} ")
    d += f"L{P_(xl2, y2)} "
    d += (f"A{R:.2f} {R:.2f} 0 0 1 {P_(xl1, y1)} " if lk == 'c' else f"L{P_(xl1, y1)} ")
    return d + "Z"
def svg(P, fill="#F8F3D2", bg=None, w=1504, h=1224, view=None):
    sh = build(P)
    ds = [path(x, False, P) for x in sh] + [path(x, True, P) for x in sh if not (x[4] is None and abs((x[0]+x[1])/2 - P['cy']) < 1)]
    vb = view or f"0 0 {w} {h}"
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" width="{w}" height="{h}">']
    if bg: out.append(f'<rect x="-10000" y="-10000" width="20000" height="20000" fill="{bg}"/>')
    out.append(f'<path fill="{fill}" d="' + " ".join(ds) + '"/>')
    out.append('</svg>')
    return "\n".join(out)
if __name__ == "__main__":
    print(svg(P, bg="#D44D1B"))
