"""Detect the sudoku grid lines in a screenshot and compare them to the truth box.

No labels, no LLM: dark-pixel column/row profiles give the grid geometry
directly, so this is an independent check on the dataset annotation.
"""
import json
import numpy as np
from PIL import Image

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
truth = {c['scene']: c for c in json.load(open(f'{DATA}/e2e_truth.json'))['cases']}

CASES = ['c4826d07', 'd6373a20', 'def03c2c']

for scene in CASES:
    b = truth[scene]['bbox']
    img = Image.open(f'{DATA}/{scene}_frame.jpg').convert('L')
    a = np.asarray(img, dtype=np.float32)

    # Search a window around the truth box for the board.
    x0, y0 = int(b[0]) - 120, int(b[1]) - 120
    x1, y1 = int(b[2]) + 120, int(b[3]) + 120
    win = a[max(0, y0):y1, max(0, x0):x1]

    # Grid lines are darker than the cell background. Column profile = mean
    # darkness per x; a grid line is a local minimum.
    col = win.mean(axis=0)
    row = win.mean(axis=1)

    def peaks(profile, thresh_ratio=0.97):
        m = profile.mean()
        cut = m * thresh_ratio
        idx = [i for i, v in enumerate(profile) if v < cut]
        # collapse runs
        out, run = [], []
        for i in idx:
            if run and i == run[-1] + 1:
                run.append(i)
            else:
                if run:
                    out.append(sum(run) / len(run))
                run = [i]
        if run:
            out.append(sum(run) / len(run))
        return out

    cols = [c + max(0, x0) for c in peaks(col)]
    rows = [r + max(0, y0) for r in peaks(row)]

    print(f'=== {scene}  truth={[round(v) for v in b]} ({round(b[2]-b[0])}x{round(b[3]-b[1])})')
    print(f'  detected vertical lines   ({len(cols)}): {[round(c) for c in cols]}')
    print(f'  detected horizontal lines ({len(rows)}): {[round(r) for r in rows]}')

    # Which detected cell does the truth box sit on?
    def nearest(lines, v):
        if not lines:
            return None, None
        d = [abs(l - v) for l in lines]
        i = d.index(min(d))
        return lines[i], d[i]

    for name, v in (('left', b[0]), ('right', b[2])):
        l, d = nearest(cols, v)
        print(f'  truth {name:5s}={v:6.1f}  nearest grid line={l if l is None else round(l)}  off by {d if d is None else round(d,1)}px')
    for name, v in (('top', b[1]), ('bottom', b[3])):
        l, d = nearest(rows, v)
        print(f'  truth {name:5s}={v:6.1f}  nearest grid line={l if l is None else round(l)}  off by {d if d is None else round(d,1)}px')
    print()
