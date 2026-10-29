"""Detect the sudoku board rectangle in each frame by scanning the whole image.

A board is a run of equally spaced long dark lines. Scan full-image column and
row profiles for lines that are dark along most of their span, then look for
runs whose spacing is consistent (~20-45px depending on window size).
"""
import json
import os
import numpy as np
from PIL import Image

E2E = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
d = json.load(open(f'{E2E}/e2e_truth.json'))
cases = [c for c in d['cases'] if '数独盘面' in c['description']]


def long_lines(a, axis, dark_frac=0.5, min_len=120):
    """Positions of rows/cols that are dark along a long contiguous span."""
    prof = a if axis == 1 else a.T
    thresh = np.percentile(a, 22)
    dark = prof < thresh
    out = []
    for i in range(dark.shape[0]):
        row = dark[i]
        # longest run of dark pixels
        best, run = 0, 0
        for v in row:
            run = run + 1 if v else 0
            if run > best:
                best = run
        if best >= min_len:
            out.append(i)
    # collapse adjacent
    groups, g = [], []
    for i in out:
        if g and i == g[-1] + 1:
            g.append(i)
        else:
            if g:
                groups.append(sum(g) / len(g))
            g = [i]
    if g:
        groups.append(sum(g) / len(g))
    return groups


res = []
for c in cases:
    a = np.asarray(Image.open(f'{E2E}/{c["fixture"]}').convert('L'), dtype=np.float32)
    vs = long_lines(a, 0)
    hs = long_lines(a, 1)
    res.append((c['scene'], vs, hs, c['bbox']))

print(f'{len(cases)} sudoku cases\n')
sizes = []
for scene, vs, hs, bbox in res[:8]:
    span_x = (max(vs) - min(vs)) if len(vs) > 1 else 0
    span_y = (max(hs) - min(hs)) if len(hs) > 1 else 0
    print(f'{scene}: vlines={len(vs)} x-span={span_x:.0f}  hlines={len(hs)} y-span={span_y:.0f}')
    print(f'   v: {[round(v) for v in vs[:12]]}')
    print(f'   h: {[round(v) for v in hs[:12]]}')

# how many distinct board sizes?
import collections
cnt = collections.Counter()
for scene, vs, hs, bbox in res:
    if len(vs) > 2 and len(hs) > 2:
        sx = round((max(vs) - min(vs)) / 10) * 10
        sy = round((max(hs) - min(hs)) / 10) * 10
        cnt[(sx, sy)] += 1
print('\nboard span distribution (rounded to 10px):')
for k, v in cnt.most_common(10):
    print(f'  {k}: {v}')
