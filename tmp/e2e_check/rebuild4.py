"""Rebuild cell truth boxes from the row/column named in each description.

The old boxes are off by up to 24px, which exceeds the 20px half-cell, so
snapping their centres to the lattice picks the wrong cell for some cases.
The description always names the cell ("G行第五列"), which is unambiguous.
"""
import json
import re
import numpy as np

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
d = json.load(open(f'{DATA}/e2e_truth.json'))
cells = [c for c in d['cases']
         if '数独盘面' in c['description']
         and ('空单元格' in c['description'] or '的格子' in c['description'])]

CN = {'一': 1, '二': 2, '三': 3, '四': 4, '五': 5, '六': 6, '七': 7, '八': 8, '九': 9}


def col_of(desc):
    m = re.search(r'第([一二三四五六七八九])列', desc)
    return CN[m.group(1)] if m else None


def row_of(desc):
    m = re.search(r'数独盘面([A-I])行', desc)
    return ord(m.group(1)) - ord('A') + 1 if m else None


# lattice from the projection fit
stepx, phasex = 39.980, 36.60
stepy, phasey = 41.000, 17.20
# column 1..9 -> k = 2..10 ; row 1..9 -> k = 6..14
KX0, KY0 = 2, 6

rebuilt, failed = 0, []
for c in cells:
    col, row = col_of(c['description']), row_of(c['description'])
    if not col or not row:
        failed.append(c['scene'])
        continue
    ccx = phasex + (KX0 + col - 1) * stepx
    ccy = phasey + (KY0 + row - 1) * stepy
    c['old_bbox'] = c.get('old_bbox') or c['bbox']
    c['cell'] = [col, row]
    c['bbox'] = [round(ccx - stepx / 2, 1), round(ccy - stepy / 2, 1),
                 round(ccx + stepx / 2, 1), round(ccy + stepy / 2, 1)]
    rebuilt += 1

json.dump(d, open(f'{DATA}/e2e_truth.json.rebuilt4', 'w'), ensure_ascii=False, indent=1)
print(f'rebuilt {rebuilt}  failed {len(failed)} {failed}')
print(f'board x: {phasex + KX0*stepx - stepx/2:.1f} .. {phasex + (KX0+8)*stepx + stepx/2:.1f}')
print(f'board y: {phasey + KY0*stepy - stepy/2:.1f} .. {phasey + (KY0+8)*stepy + stepy/2:.1f}')

for c in cells:
    if c['scene'] in ('d6373a20', 'cd2ba013', '07f72a66', '0dfd3f6f'):
        b = c['bbox']
        print(f"  {c['scene']} cell={c['cell']} bbox={[round(v) for v in b]} "
              f"centre=({(b[0]+b[2])/2:.0f},{(b[1]+b[3])/2:.0f})")
print('  [d6373a20 hint was (280,506)]')
