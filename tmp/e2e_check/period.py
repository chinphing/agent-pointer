"""Find the sudoku grid by its periodicity, then check the truth box against it.

Grid lines repeat every cell width, so an autocorrelation of the darkness
profile gives the period, and phase alignment gives the exact line positions.
This is deterministic — no model judgement involved.
"""
import json
import numpy as np
from PIL import Image

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
truth = {c['scene']: c for c in json.load(open(f'{DATA}/e2e_truth.json'))['cases']}

CASES = ['c4826d07', 'd6373a20', 'def03c2c']


def period_and_phase(profile, lo=35, hi=50):
    """Return (period, phase, score) for the strongest periodic dip pattern."""
    p = profile - profile.mean()
    n = len(p)
    best = None
    for period in np.arange(lo, hi, 0.05):
        # fold the profile at this period and see how well the folds agree
        idx = np.arange(n)
        phase = (idx % period)
        # bin by phase, average — a real grid makes sharp dips at fixed phases
        bins = np.floor(phase).astype(int)
        acc = np.zeros(int(np.ceil(period)) + 1)
        cnt = np.zeros(int(np.ceil(period)) + 1)
        np.add.at(acc, bins, p)
        np.add.at(cnt, bins, 1)
        prof = acc / np.maximum(cnt, 1)
        score = prof.min()  # deepest dip after folding
        if best is None or score < best[2]:
            best = (period, prof, score)
    period, prof, score = best
    # the deepest folded bin is a grid line's phase
    phase = int(np.argmin(prof))
    return period, phase, prof


for scene in CASES:
    b = truth[scene]['bbox']
    img = Image.open(f'{DATA}/{scene}_frame.jpg').convert('L')
    a = np.asarray(img, dtype=np.float32)

    # board window: generous margin around the truth box
    x0, x1 = max(0, int(b[0]) - 150), int(b[2]) + 150
    y0, y1 = max(0, int(b[1]) - 150), int(b[3]) + 150
    win = a[y0:y1, x0:x1]

    colprof = win.mean(axis=0)
    rowprof = win.mean(axis=1)

    px, phx, _ = period_and_phase(colprof)
    py, phy, _ = period_and_phase(rowprof)

    def lines(period, phase, origin, count=12):
        # generate line positions near the truth box
        out = []
        k = -count
        while k < count:
            v = origin + phase + k * period
            if 0 <= v - origin < 1e9:
                out.append(v)
            k += 1
        return sorted(out)

    vx = [x0 + phx + k * px for k in range(-3, 20) if 0 <= x0 + phx + k * px <= a.shape[1]]
    vy = [y0 + phy + k * py for k in range(-3, 20) if 0 <= y0 + phy + k * py <= a.shape[0]]

    print(f'=== {scene}  truth={[round(v) for v in b]}  ({round(b[2]-b[0])}x{round(b[3]-b[1])})')
    print(f'  period: x={px:.2f}px  y={py:.2f}px')

    def nearest(lines, v):
        d = [abs(l - v) for l in lines]
        i = int(np.argmin(d))
        return lines[i], d[i]

    worst = 0.0
    for name, v in (('left', b[0]), ('right', b[2])):
        l, d = nearest(vx, v)
        worst = max(worst, d)
        print(f'  truth {name:5s}={v:6.1f}   line={l:6.1f}   off {d:+.1f}px')
    for name, v in (('top', b[1]), ('bottom', b[3])):
        l, d = nearest(vy, v)
        worst = max(worst, d)
        print(f'  truth {name:5s}={v:6.1f}   line={l:6.1f}   off {d:+.1f}px')
    print(f'  -> worst edge deviation from a grid line: {worst:.1f}px'
          f'  ({100*worst/px:.0f}% of a cell)')
    print()
