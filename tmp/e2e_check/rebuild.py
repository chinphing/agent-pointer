"""Recompute sudoku-cell truth boxes from detected grid lines.

Cell targets get their truth box = the four grid lines around the cell the
current centre falls in. Non-cell targets (keypad keys, buttons, icons) keep
their existing box — they are element boundaries, not grid cells.

Writes e2e_truth.json.rebuilt, never touching the original.
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


def cell_around(lines, v):
    for i in range(len(lines) - 1):
        if lines[i] <= v <= lines[i + 1]:
            return lines[i], lines[i + 1]
    return None


def detect(a, b):
    for pad, thr in ((90, 0.55), (160, 0.40), (220, 0.30)):
        vs = dedupe(grid_lines(a, b, 0, pad, thr))
        hs = dedupe(grid_lines(a, b, 1, pad, thr))
        if len(vs) >= 3 and len(hs) >= 3:
            return vs, hs
    return None, None


data = json.load(open(f'{DATA}/e2e_truth.json'))
rebuilt, kept, failed = 0, 0, []

for c in data['cases']:
    b = c['bbox']
    desc = c['description']
    is_cell = '数独盘面' in desc and ('空单元格' in desc or '的格子' in desc)
    if not is_cell:
        kept += 1
        continue
    a = np.asarray(Image.open(f'{DATA}/{c["fixture"]}').convert('L'), dtype=np.float32)
    vs, hs = detect(a, b)
    if not vs:
        failed.append(c['scene'])
        kept += 1
        continue
    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    xc, yc = cell_around(vs, cx), cell_around(hs, cy)
    if not xc or not yc:
        failed.append(c['scene'])
        kept += 1
        continue
    c['old_bbox'] = b
    w, h = xc[1] - xc[0], yc[1] - yc[0]
    # A sudoku cell is square and about the same size everywhere on the board.
    # A detected "cell" that is 122x6 means the profile latched onto a digit's
    # stroke instead of a grid line — keep the original box rather than replace
    # a mediocre truth with a wrong one.
    if not (36.0 <= w <= 47.0 and 36.0 <= h <= 47.0):
        failed.append(f'{c["scene"]}({w:.0f}x{h:.0f})')
        kept += 1
        continue
    c['bbox'] = [round(xc[0], 1), round(yc[0], 1), round(xc[1], 1), round(yc[1], 1)]
    rebuilt += 1

json.dump(data, open(f'{DATA}/e2e_truth.json.rebuilt', 'w'), ensure_ascii=False, indent=1)
print(f'rebuilt {rebuilt}  kept {kept}  failed {len(failed)}')
if failed:
    print('failed scenes:', failed)
