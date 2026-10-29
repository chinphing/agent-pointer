"""Compute what verify SHOULD have answered, from the cell truth, and compare
with what the model actually answered at the time.

Rules (derived from the cell geometry, not from any pixel special case):
  center.ok : reference-box centre relative to the cell, as a fraction of the
              cell size.  within 15% of centre -> yes; otherwise no.
  size.ok   : reference box size / cell size, both axes must be in 0.8..1.3
  position.ok: which cell the reference centre falls in, vs the cell the
              description names.
"""
import json
import statistics

SRC = '/Users/starliu/pointer-all/pointer-eval-data/pointer-2026-10-28-locate-verify/verify_cells.jsonl'
rows = [json.loads(l) for l in open(SRC) if l.strip() and json.loads(l)['cell_bbox']]

CW = 39.98
CH = 41.00
# lattice for mapping a point back to a cell index
PHASE_X, KX0, STEP_X = 36.60, 2, 39.98
PHASE_Y, KY0, STEP_Y = 17.20, 6, 41.00


def col_of(x):
    return round((x - PHASE_X) / STEP_X) - KX0 + 1


def row_of(y):
    return round((y - PHASE_Y) / STEP_Y) - KY0 + 1


def truth(r):
    b, c = r['box'], r['cell_bbox']
    bcx, bcy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    ccx, ccy = (c[0] + c[2]) / 2, (c[1] + c[3]) / 2

    fx = abs(bcx - ccx) / CW
    fy = abs(bcy - ccy) / CH
    # prompt says centre must sit in 30%..70% of the element, i.e. within 20%
    center = 'Yes' if (fx <= 0.20 and fy <= 0.20) else 'No'

    wr = (b[2] - b[0]) / CW
    hr = (b[3] - b[1]) / CH
    size = 'Yes' if (0.8 <= wr <= 1.3 and 0.8 <= hr <= 1.3) else 'No'

    same = (col_of(bcx) == r['cell_col']) and (row_of(bcy) == r['cell_row'])
    position = 'yes' if same else 'no'

    return center, size, position, fx, fy, wr, hr


def norm(v):
    return str(v).strip().lower()


agree = {'center': 0, 'size': 0, 'position': 0, 'all3': 0}
n = 0
rows_out = []
for r in rows:
    tc, ts, tp, fx, fy, wr, hr = truth(r)
    mc = norm(r['labels'].get('center'))
    ms = norm(r['labels'].get('size'))
    mp = norm(r['labels'].get('position'))
    n += 1
    a_c = (tc.lower() == mc)
    a_s = (ts.lower() == ms)
    a_p = (tp == mp)
    agree['center'] += a_c
    agree['size'] += a_s
    agree['position'] += a_p
    agree['all3'] += (a_c and a_s and a_p)
    rows_out.append((r, tc, ts, tp, mc, ms, mp, a_c, a_s, a_p))

print(f'samples with cell truth: {n}\n')
print('agreement between computed truth and the model label at the time:')
for k in ('center', 'size', 'position', 'all3'):
    print(f'  {k:9s} {agree[k]:5d}/{n} = {100*agree[k]/n:5.1f}%')

print('\nconfusion, center:')
import collections
cc = collections.Counter((t[1].lower(), t[4]) for t in rows_out)
for k, v in cc.most_common():
    print(f'  truth={k[0]:4s} model={k[1]:8s} {v}')

print('\nconfusion, size:')
cc = collections.Counter((t[2].lower(), t[5]) for t in rows_out)
for k, v in cc.most_common():
    print(f'  truth={k[0]:4s} model={k[1]:8s} {v}')

print('\nconfusion, position:')
cc = collections.Counter((t[3], t[6]) for t in rows_out)
for k, v in cc.most_common():
    print(f'  truth={k[0]:4s} model={k[1]:8s} {v}')

# the dangerous cell: model said all-yes but truth says otherwise
danger = [t for t in rows_out if t[4] == 'yes' and t[5] == 'yes' and t[6] == 'yes'
          and not (t[1].lower() == 'yes' and t[2].lower() == 'yes' and t[3] == 'yes')]
print(f'\nmodel said all-yes but truth disagrees: {len(danger)}')
offs = [max(t[0]['box'][2]-t[0]['box'][0], 0) for t in danger]
if danger:
    ds = []
    for t in danger:
        r = t[0]
        b, c = r['box'], r['cell_bbox']
        dx = (b[0]+b[2])/2 - (c[0]+c[2])/2
        dy = (b[1]+b[3])/2 - (c[1]+c[3])/2
        ds.append((dx*dx+dy*dy)**0.5)
    print(f'  their offset from the true cell: median={statistics.median(ds):.1f}px '
          f'max={max(ds):.1f}px  >20px={sum(1 for d in ds if d>20)}')
