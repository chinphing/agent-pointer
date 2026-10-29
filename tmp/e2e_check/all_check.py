"""Check every e2e truth box against the sudoku grid lines detected in its frame."""
import json
import numpy as np
from PIL import Image

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
cases = json.load(open(f'{DATA}/e2e_truth.json'))['cases']


def grid_lines(a, box, axis, pad=90):
    x0, x1 = int(box[0]) - pad, int(box[2]) + pad
    y0, y1 = int(box[1]) - pad, int(box[3]) + pad
    win = a[max(0, y0):y1, max(0, x0):x1]
    prof = win if axis == 1 else win.T
    dark = (prof < np.percentile(prof, 25)).mean(axis=1)
    cand = np.where(dark > 0.55)[0]
    groups, run = [], []
    for i in cand:
        if run and i == run[-1] + 1:
            run.append(i)
        else:
            if run:
                groups.append(run)
            run = [i]
    if run:
        groups.append(run)
    origin = max(0, y0) if axis == 1 else max(0, x0)
    return sorted(origin + sum(g) / len(g) for g in groups)


def dedupe(lines, gap=6):
    out = []
    for v in lines:
        if not out or v - out[-1] > gap:
            out.append(v)
    return out


rows = []
for c in cases:
    b = c['bbox']
    try:
        a = np.asarray(Image.open(f'{DATA}/{c["fixture"]}').convert('L'), dtype=np.float32)
    except Exception:
        continue
    vs = dedupe(grid_lines(a, b, 0))
    hs = dedupe(grid_lines(a, b, 1))
    if len(vs) < 3 or len(hs) < 3:
        rows.append((c['scene'], None, None, len(vs), len(hs)))
        continue
    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2

    def cell(lines, v):
        for i in range(len(lines) - 1):
            if lines[i] <= v <= lines[i + 1]:
                return lines[i], lines[i + 1]
        return None

    xc, yc = cell(vs, cx), cell(hs, cy)
    if not xc or not yc:
        rows.append((c['scene'], None, None, len(vs), len(hs)))
        continue
    ccx, ccy = (xc[0] + xc[1]) / 2, (yc[0] + yc[1]) / 2
    cw, ch = xc[1] - xc[0], yc[1] - yc[0]
    rows.append((c['scene'], cx - ccx, cy - ccy, cw, ch))

ok = [r for r in rows if r[1] is not None]
print(f'total {len(rows)}  measurable {len(ok)}  not measurable {len(rows)-len(ok)}')
dx = np.array([r[1] for r in ok])
dy = np.array([r[2] for r in ok])
cw = np.array([r[3] for r in ok])
print(f'cell width  median={np.median(cw):.1f}px')
print(f'dx: mean={dx.mean():+.1f}  median={np.median(dx):+.1f}  sd={dx.std():.1f}  '
      f'min={dx.min():+.1f} max={dx.max():+.1f}')
print(f'dy: mean={dy.mean():+.1f}  median={np.median(dy):+.1f}  sd={dy.std():.1f}  '
      f'min={dy.min():+.1f} max={dy.max():+.1f}')
for t in (5, 10, 15):
    n = int((np.abs(dx) > t).sum())
    print(f'  |dx| > {t}px: {n}/{len(ok)} = {100*n/len(ok):.0f}%')
print('\nworst 12 by |dx|:')
for r in sorted(ok, key=lambda r: -abs(r[1]))[:12]:
    print(f'  {r[0]}  dx={r[1]:+.1f} dy={r[2]:+.1f}  cell={r[3]:.0f}x{r[4]:.0f}')
