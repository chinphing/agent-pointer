"""Rebuild all cell truth boxes from the projected 9x9 cell-centre lattice."""
import json
import numpy as np

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
d = json.load(open(f'{DATA}/e2e_truth.json'))
cells = [c for c in d['cases']
         if '数独盘面' in c['description']
         and ('空单元格' in c['description'] or '的格子' in c['description'])]


def fit(vals, step_lo=39.0, step_hi=43.0):
    best = None
    for step in np.arange(step_lo, step_hi, 0.02):
        for phase in np.arange(0, step, 0.2):
            k = np.round((vals - phase) / step)
            err = np.abs(vals - (phase + k * step))
            inl = err < 8.0
            if inl.sum() < len(vals) * 0.6:
                continue
            score = (int(inl.sum()), -err[inl].std())
            if best is None or score > best[0]:
                best = (score, step, phase)
    return best[1], best[2]


centres = [((c.get('old_bbox') or c['bbox'])[0] + (c.get('old_bbox') or c['bbox'])[2]) / 2
           for c in cells]
centresy = [((c.get('old_bbox') or c['bbox'])[1] + (c.get('old_bbox') or c['bbox'])[3]) / 2
            for c in cells]

stepx, phasex = fit(np.array(centres))
stepy, phasey = fit(np.array(centresy))
print(f'lattice: x step={stepx:.3f} phase={phasex:.2f}   y step={stepy:.3f} phase={phasey:.2f}')

# board bounds from the 9 bands actually occupied
kx = np.round((np.array(centres) - phasex) / stepx).astype(int)
ky = np.round((np.array(centresy) - phasey) / stepy).astype(int)
print(f'board x: {phasex + kx.min()*stepx - stepx/2:.1f} .. {phasex + kx.max()*stepx + stepx/2:.1f}')
print(f'board y: {phasey + ky.min()*stepy - stepy/2:.1f} .. {phasey + ky.max()*stepy + stepy/2:.1f}')

rebuilt = 0
for c, cx, cy in zip(cells, centres, centresy):
    kx_ = round((cx - phasex) / stepx)
    ky_ = round((cy - phasey) / stepy)
    ccx = phasex + kx_ * stepx
    ccy = phasey + ky_ * stepy
    c['old_bbox'] = c.get('old_bbox') or c['bbox']
    c['cell'] = [kx_, ky_]
    c['bbox'] = [round(ccx - stepx / 2, 1), round(ccy - stepy / 2, 1),
                 round(ccx + stepx / 2, 1), round(ccy + stepy / 2, 1)]
    rebuilt += 1

json.dump(d, open(f'{DATA}/e2e_truth.json.rebuilt3', 'w'), ensure_ascii=False, indent=1)
print(f'\nrebuilt {rebuilt} -> e2e_truth.json.rebuilt3')

for c in cells:
    if c['scene'] == 'd6373a20':
        b = c['bbox']
        print(f"  d6373a20 cell={c['cell']} bbox={[round(v) for v in b]} "
              f"centre=({(b[0]+b[2])/2:.0f},{(b[1]+b[3])/2:.0f})   [hint was (280,506)]")
    if c['scene'] == 'cd2ba013':
        b = c['bbox']
        print(f"  cd2ba013 cell={c['cell']} bbox={[round(v) for v in b]} "
              f"centre=({(b[0]+b[2])/2:.0f},{(b[1]+b[3])/2:.0f})")
