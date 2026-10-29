"""Fit one global 9x9 sudoku grid, then rebuild every cell truth box from it.

Collect the grid lines detected across all cell-target cases, fit a single
`origin + k*step` lattice per axis, and snap each cell target to its lattice
cell. A global fit is far more robust than per-case detection: a case whose
profile latched onto a digit gets pulled back to the lattice instead of
producing a 122x6 "cell".
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
    return sorted(origin + sum(g) / len(g) for g in groups)


def dedupe(lines, gap=6):
    out = []
    for v in lines:
        if not out or v - out[-1] > gap:
            out.append(v)
    return out


def fit_lattice(values, lo=34.0, hi=48.0):
    """Fit v = origin + k*step. Returns (origin, step) or None."""
    vals = np.array(sorted(values))
    if len(vals) < 6:
        return None
    diffs = np.diff(vals)
    diffs = diffs[(diffs > lo) & (diffs < hi)]
    if len(diffs) < 3:
        return None
    step = float(np.median(diffs))
    # refine step: total span / number of steps across it
    span = vals[-1] - vals[0]
    k = round(span / step)
    if k >= 1:
        step = span / k
    # phase: every value should be origin + integer*step; take the circular mean
    phase = np.angle(np.mean(np.exp(2j * np.pi * (vals - vals[0]) / step)))
    origin = vals[0] + (phase / (2 * np.pi)) * step
    return origin, step


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

gv = fit_lattice(all_v)
gh = fit_lattice(all_h)
print(f'vertical lattice:   origin={gv[0]:.2f} step={gv[1]:.3f}')
print(f'horizontal lattice: origin={gh[0]:.2f} step={gh[1]:.3f}')

# snap to the board's own 9 lines: the lattice spans many boards? no - one board.
# find the 10 lattice lines that bracket the data
def board_lines(fit, values):
    origin, step = fit
    k0 = round((min(values) - origin) / step)
    k1 = round((max(values) - origin) / step)
    return [origin + k * step for k in range(k0, k1 + 1)]

bv = board_lines(gv, all_v)
bh = board_lines(gh, all_h)
print(f'board vertical lines   ({len(bv)}): {[round(v) for v in bv]}')
print(f'board horizontal lines ({len(bh)}): {[round(v) for v in bh]}')

rebuilt, kept, failed = 0, 0, []
for c in cells:
    b = c['bbox']
    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    # nearest lattice line pair around the centre
    xs = [v for v in bv if v <= cx + 1]
    ys = [v for v in bh if v <= cy + 1]
    if not xs or not ys:
        failed.append(f'{c["scene"]}(outside lattice)')
        kept += 1
        continue
    x0 = xs[-1]
    x1 = x0 + gv[1]
    y0 = ys[-1]
    y1 = y0 + gh[1]
    # sanity: centre must sit inside the snapped cell
    if not (x0 - 2 <= cx <= x1 + 2 and y0 - 2 <= cy <= y1 + 2):
        failed.append(f'{c["scene"]}(centre off)')
        kept += 1
        continue
    c['old_bbox'] = b
    c['bbox'] = [round(x0, 1), round(y0, 1), round(x1, 1), round(y1, 1)]
    rebuilt += 1

json.dump(data, open(f'{DATA}/e2e_truth.json.rebuilt', 'w'), ensure_ascii=False, indent=1)
print(f'\nrebuilt {rebuilt}  kept {kept}  failed {len(failed)}')
for f in failed:
    print('  failed:', f)
