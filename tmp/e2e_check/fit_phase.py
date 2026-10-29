"""Fit the sudoku lattice by exhaustive phase search over detected lines.

The earlier fits failed for two different reasons:
  - detected lines alone: digits and UI chrome add noise, phase drifts a cell
  - old bbox centres: the old boxes are themselves off by up to 20px

So: detect lines (noisy but unbiased), then search phase and step for the
lattice that explains the most detected lines. A wrong phase explains far
fewer lines than the right one.
"""
import json
import numpy as np
from PIL import Image

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'


def grid_lines(a, box, axis, pad, thr):
    x0, x1 = int(box[0]) - pad, int(box[2]) + pad
    y0, y1 = int(box[1]) - pad, int(box[3]) + pad
    win = a[max(0, y0):y1, max(0, x0):x1]
    prof = win if axis == 1 else win.T
    dark = (prof < np.percentile(prof, 25)).mean(axis=1)
    cand = np.where(dark > thr)[0]
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
    return [origin + sum(g) / len(g) for g in groups]


def dedupe(lines, gap=6):
    out = []
    for v in sorted(lines):
        if not out or v - out[-1] > gap:
            out.append(v)
    return out


def best_lattice(values, step_lo=38.0, step_hi=44.0):
    """Search (step, phase) maximising how many detected lines snap to it."""
    vals = np.array(sorted(values))
    best = None
    for step in np.arange(step_lo, step_hi, 0.05):
        # phase candidates: each value mod step, refined by a fine sweep
        for phase in np.arange(0, step, 0.25):
            k = np.round((vals - phase) / step)
            snapped = phase + k * step
            err = np.abs(vals - snapped)
            inliers = int((err < 3.0).sum())
            if inliers < 5:
                continue
            cost = err[err < 3.0].mean()
            score = (inliers, -cost)
            if best is None or score > best[0]:
                best = (score, step, phase)
    if not best:
        return None
    _, step, phase = best
    # normalise phase so it is the position of some line near the data
    k0 = round((vals.min() - phase) / step)
    origin = phase + k0 * step
    return origin, step, best[0][0]


data = json.load(open(f'{DATA}/e2e_truth.json'))
cells = [c for c in data['cases']
         if '数独盘面' in c['description']
         and ('空单元格' in c['description'] or '的格子' in c['description'])]

all_v, all_h = [], []
for c in cells:
    b = c['bbox']
    a = np.asarray(Image.open(f'{DATA}/{c["fixture"]}').convert('L'), dtype=np.float32)
    for pad, thr in ((90, 0.55), (160, 0.40)):
        vs = dedupe(grid_lines(a, b, 0, pad, thr))
        hs = dedupe(grid_lines(a, b, 1, pad, thr))
        if len(vs) >= 3 and len(hs) >= 3:
            all_v.extend(vs)
            all_h.extend(hs)
            break

gv = best_lattice(all_v)
gh = best_lattice(all_h)
print(f'vertical  : origin={gv[0]:.2f} step={gv[1]:.3f} inliers={gv[2]}')
print(f'horizontal: origin={gh[0]:.2f} step={gh[1]:.3f} inliers={gh[2]}')

# the known answer: d6373a20 target cell centre must be ~280 in x
for c in cells:
    if c['scene'] == 'd6373a20':
        b = c['bbox']
        cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        ox, stepx, _ = gv
        oy, stepy, _ = gh
        kx = round((cx - ox) / stepx - 0.5)
        ky = round((cy - oy) / stepy - 0.5)
        ccx = ox + (kx + 0.5) * stepx
        ccy = oy + (ky + 0.5) * stepy
        print(f'\ncheck d6373a20: old centre=({cx:.0f},{cy:.0f}) -> cell centre=({ccx:.0f},{ccy:.0f})')
        print(f'  expected ~(280,506) from the verify crop')
