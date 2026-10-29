"""Draw truth bbox (green) vs dataset hint_box (red) for truth_sudoku.json."""
import json, os, re
from PIL import Image, ImageDraw, ImageFont

ROOT = '/Users/starliu/pointer-all/agent-pointer'
TRUTH = f'{ROOT}/crates/pointer-core/tests/locate_fixture/truth/truth_sudoku.json'
IMG = f'{ROOT}/crates/pointer-core/tests/locate_fixture/fixtures/sudoku.jpg'
OUT = f'{ROOT}/tmp/sudoku_check'
os.makedirs(OUT, exist_ok=True)

d = json.load(open(TRUTH))
img = Image.open(IMG).convert('RGB')
g = d['grid']
cw = (g['right'] - g['left']) / g['cols']
ch = (g['bottom'] - g['top']) / g['rows']


def cell_box(scene):
    m = re.match(r'r(\d+)c(\d+)', scene)
    r, c = int(m.group(1)), int(m.group(2))
    return [g['left'] + c * cw, g['top'] + r * ch,
            g['left'] + (c + 1) * cw, g['top'] + (r + 1) * ch]


GREEN = (0, 190, 0)
RED = (225, 0, 0)
BLUE = (0, 90, 220)

ov = img.copy()
dr = ImageDraw.Draw(ov)
for x in d['cases']:
    dr.rectangle(cell_box(x['scene']), outline=GREEN, width=2)
    dr.rectangle(x['hint_box'], outline=RED, width=2)
dr.rectangle([g['left'], g['top'], g['right'], g['bottom']], outline=BLUE, width=3)
ov.save(f'{OUT}/overview.jpg', quality=90)

rows = []
for x in d['cases']:
    b = cell_box(x['scene'])
    h = x['hint_box']
    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    inside = h[0] <= cx <= h[2] and h[1] <= cy <= h[3]
    w = (h[2] - h[0]) / cw
    hh = (h[3] - h[1]) / ch
    if not inside:
        why = 'NO-CENTER'
    elif w < 0.8 or hh < 0.8:
        why = 'TIGHT'
    elif w > 1.8 or hh > 1.8:
        why = 'HUGE'
    else:
        why = 'ok'
    rows.append((x['scene'], why, w, hh, b, h))

bad = [r for r in rows if r[1] != 'ok']
print(f'cases={len(rows)}  flagged={len(bad)}')
for sc, why, w, hh, b, h in bad:
    print(f'  {sc:6s} {why:9s} w={w:.2f} h={hh:.2f} '
          f'bbox={[round(v) for v in b]} hint={[round(v) for v in h]}')

if bad:
    PAD = 34
    CELL = 260
    cols = 4
    nrows = (len(bad) + cols - 1) // cols
    sheet = Image.new('RGB', (cols * CELL, nrows * (CELL + 18)), (250, 250, 250))
    try:
        font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 13)
    except Exception:
        font = ImageFont.load_default()
    sd = ImageDraw.Draw(sheet)
    for i, (sc, why, w, hh, b, h) in enumerate(bad):
        x1 = min(b[0], h[0]) - PAD
        y1 = min(b[1], h[1]) - PAD
        x2 = max(b[2], h[2]) + PAD
        y2 = max(b[3], h[3]) + PAD
        crop = img.crop((int(x1), int(y1), int(x2), int(y2)))
        f = min(CELL / crop.width, CELL / crop.height)
        crop = crop.resize((max(1, int(crop.width * f)), max(1, int(crop.height * f))),
                           Image.LANCZOS)
        cd = ImageDraw.Draw(crop)
        for bx, col in ((b, GREEN), (h, RED)):
            cd.rectangle([(bx[0] - x1) * f, (bx[1] - y1) * f,
                          (bx[2] - x1) * f, (bx[3] - y1) * f],
                         outline=col, width=2)
        px, py = (i % cols) * CELL, (i // cols) * (CELL + 18)
        sheet.paste(crop, (px, py))
        sd.text((px + 3, py + CELL + 2), f'{sc} {why} {w:.2f}x{hh:.2f}',
                fill=(20, 20, 20), font=font)
    sheet.save(f'{OUT}/flagged.jpg', quality=92)

print(f'\nwrote {OUT}/overview.jpg and {OUT}/flagged.jpg')
