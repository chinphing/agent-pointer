"""Rebuild cell truth boxes by fitting a lattice to the OLD bbox centres.

Why centres instead of detected grid lines: the old boxes are off by ~9.6px
(std), far less than the 20.5px half-cell, so every old centre still falls
inside its correct cell. Detected grid lines, by contrast, pick up digits and
UI chrome and drag the fitted phase by a whole cell.

Fit: centre_x = origin + k*step over the 51 cell targets, per axis.
Then the truth box for a target = its cell = centre +/- step/2.
"""
import json
import numpy as np

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'


def fit_lattice(values, lo=34.0, hi=48.0, cluster_gap=12.0):
    """Fit v = origin + k*step to a set of near-lattice points.

    Many targets share a column, so raw sorted diffs contain zeros and
    multi-cell gaps. Cluster first, then fit on the distinct lattice points.
    """
    vals = np.array(sorted(values))
    # cluster: one representative per distinct lattice point
    uniq, run = [], [vals[0]]
    for v in vals[1:]:
        if v - run[-1] <= cluster_gap:
            run.append(v)
        else:
            uniq.append(float(np.mean(run)))
            run = [v]
    uniq.append(float(np.mean(run)))
    uniq = np.array(uniq)
    if len(uniq) < 3:
        return None
    diffs = np.diff(uniq)
    diffs = diffs[(diffs > lo) & (diffs < hi)]
    if len(diffs) < 2:
        return None
    step = float(np.median(diffs))
    # refine step over the full span
    span = uniq[-1] - uniq[0]
    k = round(span / step)
    if k >= 1:
        step = span / k
    # circular-mean phase
    ref = uniq[0]
    ang = np.angle(np.mean(np.exp(2j * np.pi * (uniq - ref) / step)))
    origin = ref + (ang / (2 * np.pi)) * step
    return origin, step


data = json.load(open(f'{DATA}/e2e_truth.json'))
cells = [c for c in data['cases']
         if '数独盘面' in c['description']
         and ('空单元格' in c['description'] or '的格子' in c['description'])]

# use the ORIGINAL boxes (pre-rebuild) as the source of centres
cxs = [((c.get('old_bbox') or c['bbox'])[0] + (c.get('old_bbox') or c['bbox'])[2]) / 2 for c in cells]
cys = [((c.get('old_bbox') or c['bbox'])[1] + (c.get('old_bbox') or c['bbox'])[3]) / 2 for c in cells]

gx = fit_lattice(cxs)
gy = fit_lattice(cys)
print(f'cell-centre lattice x: origin={gx[0]:.2f} step={gx[1]:.3f}')
print(f'cell-centre lattice y: origin={gy[0]:.2f} step={gy[1]:.3f}')

# residual check: every centre should sit close to a lattice point
def residuals(vals, fit):
    origin, step = fit
    return [((v - origin) / step - round((v - origin) / step)) * step for v in vals]

rx = residuals(cxs, gx)
ry = residuals(cys, gy)
print(f'residual x: max|r|={max(abs(r) for r in rx):.1f}px  sd={np.std(rx):.1f}')
print(f'residual y: max|r|={max(abs(r) for r in ry):.1f}px  sd={np.std(ry):.1f}')

rebuilt = 0
for c, cx, cy in zip(cells, cxs, cys):
    ox, stepx = gx
    oy, stepy = gy
    kx = round((cx - ox) / stepx)
    ky = round((cy - oy) / stepy)
    ccx = ox + kx * stepx
    ccy = oy + ky * stepy
    c['old_bbox'] = c.get('old_bbox') or c['bbox']
    c['bbox'] = [round(ccx - stepx / 2, 1), round(ccy - stepy / 2, 1),
                 round(ccx + stepx / 2, 1), round(ccy + stepy / 2, 1)]
    rebuilt += 1

json.dump(data, open(f'{DATA}/e2e_truth.json.rebuilt2', 'w'), ensure_ascii=False, indent=1)
print(f'\nrebuilt {rebuilt} cells -> e2e_truth.json.rebuilt2')

# show the case we know the answer for
for c in cells:
    if c['scene'] == 'd6373a20':
        print(f"  d6373a20 new box={[round(v) for v in c['bbox']]} "
              f"centre=({(c['bbox'][0]+c['bbox'][2])/2:.0f},{(c['bbox'][1]+c['bbox'][3])/2:.0f})")
        print(f"           hint centre was (280,506); should land in the same cell")
