"""Locate sudoku grid lines by looking for columns/rows that are dark along their
whole span. A grid line runs the full board height; a digit does not."""
import json
import numpy as np
from PIL import Image, ImageDraw

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
truth = {c['scene']: c for c in json.load(open(f'{DATA}/e2e_truth.json'))['cases']}

CASES = ['c4826d07', 'd6373a20', 'def03c2c']


def grid_lines(a, box, axis, pad=90):
    """axis=0 -> vertical lines (scan columns), axis=1 -> horizontal lines."""
    x0 = int(box[0]) - pad
    x1 = int(box[2]) + pad
    y0 = int(box[1]) - pad
    y1 = int(box[3]) + pad
    win = a[max(0, y0):y1, max(0, x0):x1]
    prof = win if axis == 1 else win.T          # rows of prof are the candidates
    # fraction of dark pixels along the candidate's whole length
    dark = (prof < np.percentile(prof, 25)).mean(axis=1)
    # a real grid line is dark for most of its length
    cand = np.where(dark > 0.55)[0]
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
    return [(origin + sum(g) / len(g), dark[g].max()) for g in groups]


for scene in CASES:
    b = truth[scene]['bbox']
    img = Image.open(f'{DATA}/{scene}_frame.jpg').convert('L')
    a = np.asarray(img, dtype=np.float32)

    vs = grid_lines(a, b, axis=0)   # x positions of vertical lines
    hs = grid_lines(a, b, axis=1)   # y positions of horizontal lines

    print(f'=== {scene}  truth=[{b[0]:.0f},{b[1]:.0f},{b[2]:.0f},{b[3]:.0f}] '
          f'({b[2]-b[0]:.0f}x{b[3]-b[1]:.0f})  centre=({(b[0]+b[2])/2:.1f},{(b[1]+b[3])/2:.1f})')
    print(f'  vertical lines   x: {[round(v[0],1) for v in vs]}')
    print(f'  horizontal lines y: {[round(v[0],1) for v in hs]}')

    # nearest line to each truth edge, and the cell the truth centre falls in
    def cell_of(lines, v):
        if len(lines) < 2:
            return None
        s = sorted(l[0] for l in lines)
        for i in range(len(s) - 1):
            if s[i] <= v <= s[i+1]:
                return (s[i], s[i+1])
        return (s[0], s[-1])

    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    xc, yc = cell_of(vs, cx), cell_of(hs, cy)
    if xc and yc:
        print(f'  cell containing truth centre: x[{xc[0]:.1f},{xc[1]:.1f}] y[{yc[0]:.1f},{yc[1]:.1f}]')
        ccx, ccy = (xc[0] + xc[1]) / 2, (yc[0] + yc[1]) / 2
        print(f'  that cell centre = ({ccx:.1f},{ccy:.1f})')
        print(f'  truth centre is off the cell centre by '
              f'({cx - ccx:+.1f}, {cy - ccy:+.1f})  -> {abs(cx-ccx):.1f}px in x '
              f'({100*abs(cx-ccx)/(xc[1]-xc[0]):.0f}% of the cell)')
    print()
