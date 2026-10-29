"""Project every sudoku target centre onto each axis and find the 9 cell bands.

The old boxes are noisy (~9.6px) but unbiased overall, and each cell holds
several targets, so a histogram along x and along y shows 9 clear peaks.
Peak positions give the cell centres directly — no grid-line detection and no
per-case vision needed.
"""
import json
import numpy as np

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
d = json.load(open(f'{DATA}/e2e_truth.json'))
cells = [c for c in d['cases']
         if '数独盘面' in c['description']
         and ('空单元格' in c['description'] or '的格子' in c['description'])]

cxs = np.array([((c.get('old_bbox') or c['bbox'])[0] + (c.get('old_bbox') or c['bbox'])[2]) / 2
                for c in cells])
cys = np.array([((c.get('old_bbox') or c['bbox'])[1] + (c.get('old_bbox') or c['bbox'])[3]) / 2
                for c in cells])
print(f'{len(cells)} cell targets')


def bands(vals, step_lo=39.0, step_hi=43.0):
    """Find the step/phase that puts the most values into 9 tight bands."""
    best = None
    for step in np.arange(step_lo, step_hi, 0.02):
        for phase in np.arange(0, step, 0.2):
            k = np.round((vals - phase) / step)
            err = np.abs(vals - (phase + k * step))
            inl = err < 8.0
            if inl.sum() < len(vals) * 0.6:
                continue
            spread = err[inl].std()
            score = (int(inl.sum()), -spread)
            if best is None or score > best[0]:
                best = (score, step, phase)
    return best


for name, vals in (('x', cxs), ('y', cys)):
    score, step, phase = bands(vals)
    k = np.round((vals - phase) / step)
    snapped = phase + k * step
    err = np.abs(vals - snapped)
    print(f'\n{name}: step={step:.2f} phase={phase:.2f} inliers={score[0]}/{len(vals)} '
          f'spread={score[1]:.2f}px')
    ks = sorted(set(k.astype(int)))
    print(f'  distinct k values: {ks[0]}..{ks[-1]}  ({len(ks)} bands)')
    for kk in ks:
        m = k == kk
        if m.sum():
            print(f'    k={kk:3d} centre={phase + kk*step:7.1f}  n={m.sum():2d}  '
                  f'mean={vals[m].mean():7.1f}  sd={vals[m].std():5.1f}')
