"""Compute the target cell bbox for every sudoku verify sample.

Lattice from the e2e truth fit:
  column c (1..9) -> centre_x = 36.60 + (2 + c - 1) * 39.980
  row    r (1..9) -> centre_y = 17.20 + (6 + r - 1) * 41.000
Cell box = centre +/- half step  -> 39.98 x 41.00

Writes verify_cells.jsonl next to the source. Nothing else is changed.
"""
import json
import re

ROOT = '/Users/starliu/pointer-all/pointer-eval-data'
SRC = f'{ROOT}/pointer-2026-10-28-locate-verify/verify.jsonl'
OUT = f'{ROOT}/pointer-2026-10-28-locate-verify/verify_cells.jsonl'

STEP_X, PHASE_X, KX0 = 39.980, 36.60, 2
STEP_Y, PHASE_Y, KY0 = 41.000, 17.20, 6

CN = {'一': 1, '二': 2, '三': 3, '四': 4, '五': 5, '六': 6, '七': 7, '八': 8, '九': 9}


def parse_cell(desc):
    """Return (row 1..9, col 1..9) or None."""
    m = re.search(r'数独盘面第([一二三四五六七八九])行第([一二三四五六七八九])列', desc)
    if m:
        return CN[m.group(1)], CN[m.group(2)]
    m = re.search(r'数独盘面([A-I])行[^)]*\)第([一二三四五六七八九])列', desc)
    if m:
        return ord(m.group(1)) - 64, CN[m.group(2)]
    m = re.search(r'数独盘面([A-I])([1-9])', desc)
    if m:
        return ord(m.group(1)) - 64, int(m.group(2))
    m = re.search(r'数独盘面([A-I])行', desc)
    mc = re.search(r'第([一二三四五六七八九])列', desc)
    if m and mc:
        return ord(m.group(1)) - 64, CN[mc.group(1)]
    return None


def cell_box(row, col):
    cx = PHASE_X + (KX0 + col - 1) * STEP_X
    cy = PHASE_Y + (KY0 + row - 1) * STEP_Y
    return [round(cx - STEP_X / 2, 1), round(cy - STEP_Y / 2, 1),
            round(cx + STEP_X / 2, 1), round(cy + STEP_Y / 2, 1)]


rows = [json.loads(l) for l in open(SRC) if l.strip()]
out, n_cell, n_skip = [], 0, 0
for r in rows:
    rc = parse_cell(r.get('target_desc', ''))
    rec = dict(r)
    if rc:
        row, col = rc
        rec['cell_row'] = row
        rec['cell_col'] = col
        rec['cell_bbox'] = cell_box(row, col)
        n_cell += 1
    else:
        rec['cell_row'] = None
        rec['cell_col'] = None
        rec['cell_bbox'] = None
        n_skip += 1
    out.append(rec)

with open(OUT, 'w') as f:
    for rec in out:
        f.write(json.dumps(rec, ensure_ascii=False) + '\n')

print(f'total {len(rows)}  cell-resolved {n_cell}  not-sudoku {n_skip}')
print(f'wrote {OUT}')

# sanity: cell box size must be uniform
sizes = {(round(r['cell_bbox'][2] - r['cell_bbox'][0], 1),
          round(r['cell_bbox'][3] - r['cell_bbox'][1], 1))
         for r in out if r['cell_bbox']}
print(f'cell box sizes: {sizes}')

# board extent
xs = [r['cell_bbox'][0] for r in out if r['cell_bbox']] + [r['cell_bbox'][2] for r in out if r['cell_bbox']]
ys = [r['cell_bbox'][1] for r in out if r['cell_bbox']] + [r['cell_bbox'][3] for r in out if r['cell_bbox']]
print(f'board extent x {min(xs):.1f}..{max(xs):.1f}  y {min(ys):.1f}..{max(ys):.1f}')

# how far is each round's reference box from its cell?
import statistics
for rnd in (1, 2, 3):
    ds = []
    for r in out:
        if r['cell_bbox'] and r['round'] == rnd:
            b, c = r['box'], r['cell_bbox']
            dx = (b[0] + b[2]) / 2 - (c[0] + c[2]) / 2
            dy = (b[1] + b[3]) / 2 - (c[1] + c[3]) / 2
            ds.append((dx * dx + dy * dy) ** 0.5)
    if ds:
        print(f'round {rnd}: n={len(ds):4d}  offset median={statistics.median(ds):6.1f}px  '
              f'mean={statistics.mean(ds):6.1f}px  <20px={sum(1 for d in ds if d<20)*100//len(ds)}%')
