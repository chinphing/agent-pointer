"""Draw the detected cell borders and the truth box on the same panel.

Blue = cell borders found from the image itself (grid lines).
Green = dataset truth box.  Red cross = truth centre.  Blue cross = cell centre.
"""
import json
import numpy as np
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
truth = {c['scene']: c for c in json.load(open(f'{DATA}/e2e_truth.json'))['cases']}

CASES = ['c4826d07', 'd6373a20', 'def03c2c']
SCALE = 7
GREEN = (0, 200, 0)
BLUE = (0, 90, 230)
RED = (230, 0, 0)


def grid_lines(a, box, axis, pad=90):
    x0, x1 = int(box[0]) - pad, int(box[2]) + pad
    y0, y1 = int(box[1]) - pad, int(box[3]) + pad
    win = a[max(0, y0):y1, max(0, x0):x1]
    prof = win if axis == 1 else win.T
    dark = (prof < np.percentile(prof, 25)).mean(axis=1)
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
    return sorted(origin + sum(g) / len(g) for g in groups)


def dedupe(lines, gap=6):
    out = []
    for v in lines:
        if not out or v - out[-1] > gap:
            out.append(v)
    return out


try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 17)
except Exception:
    font = ImageFont.load_default()

panels = []
for scene in CASES:
    b = truth[scene]['bbox']
    img = Image.open(f'{DATA}/{scene}_frame.jpg').convert('RGB')
    a = np.asarray(img.convert('L'), dtype=np.float32)

    vs = dedupe(grid_lines(a, b, 0))
    hs = dedupe(grid_lines(a, b, 1))

    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2

    def cell(lines, v):
        for i in range(len(lines) - 1):
            if lines[i] <= v <= lines[i + 1]:
                return lines[i], lines[i + 1]
        return None

    xc, yc = cell(vs, cx), cell(hs, cy)
    if not xc or not yc:
        continue
    ccx, ccy = (xc[0] + xc[1]) / 2, (yc[0] + yc[1]) / 2

    pad = 60
    ox, oy = int(xc[0] - pad), int(yc[0] - pad)
    ex, ey = int(xc[1] + pad), int(yc[1] + pad)
    crop = img.crop((ox, oy, ex, ey))
    crop = crop.resize((crop.width * SCALE, crop.height * SCALE), Image.LANCZOS)
    d = ImageDraw.Draw(crop)

    # every detected line in view, faint blue
    for v in vs:
        if ox <= v <= ex:
            d.line([(v - ox) * SCALE, 0, (v - ox) * SCALE, crop.height], fill=(150, 180, 240), width=1)
    for v in hs:
        if oy <= v <= ey:
            d.line([0, (v - oy) * SCALE, crop.width, (v - oy) * SCALE], fill=(150, 180, 240), width=1)

    # the cell that contains the truth centre
    d.rectangle([(xc[0] - ox) * SCALE, (yc[0] - oy) * SCALE,
                 (xc[1] - ox) * SCALE, (yc[1] - oy) * SCALE], outline=BLUE, width=3)
    # truth box
    d.rectangle([(b[0] - ox) * SCALE, (b[1] - oy) * SCALE,
                 (b[2] - ox) * SCALE, (b[3] - oy) * SCALE], outline=GREEN, width=3)

    for (px, py), colour, arm in (((cx, cy), RED, 11), ((ccx, ccy), BLUE, 11)):
        X, Y = (px - ox) * SCALE, (py - oy) * SCALE
        d.line([X - arm, Y, X + arm, Y], fill=colour, width=3)
        d.line([X, Y - arm, X, Y + arm], fill=colour, width=3)
    d.line([(cx - ox) * SCALE, (cy - oy) * SCALE, (ccx - ox) * SCALE, (ccy - oy) * SCALE],
           fill=RED, width=2)

    panels.append((crop, f'{scene}  truth centre off cell centre by ({cx-ccx:+.1f},{cy-ccy:+.1f})px'
                         f' = {100*abs(cx-ccx)/(xc[1]-xc[0]):.0f}% of cell width'))

W = max(p[0].width for p in panels)
H = sum(p[0].height + 28 for p in panels)
sheet = Image.new('RGB', (W, H), (250, 250, 250))
sd = ImageDraw.Draw(sheet)
y = 0
for crop, label in panels:
    sheet.paste(crop, (0, y))
    sd.text((6, y + crop.height + 4), label, fill=(10, 10, 10), font=font)
    y += crop.height + 28

sheet.save(f'{OUT}/cell_vs_truth.jpg', quality=94)
for _, label in panels:
    print(label)
print(f'\nwrote {OUT}/cell_vs_truth.jpg ({sheet.width}x{sheet.height})')
