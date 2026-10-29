"""Render computed cell boxes against the reference boxes for a few verify samples."""
import json
import os
from PIL import Image, ImageDraw, ImageFont

ROOT = '/Users/starliu/pointer-all/pointer-eval-data'
SRC = f'{ROOT}/pointer-2026-10-28-locate-verify/verify_cells.jsonl'
E2E = f'{ROOT}/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'

rows = [json.loads(l) for l in open(SRC) if l.strip()]
have_frame = {f[:-10] for f in os.listdir(E2E) if f.endswith('_frame.jpg')}
cand = [r for r in rows if r['cell_bbox'] and r['run_id'] in have_frame and r['round'] == 1]

# pick a spread: large offset, small offset, different rows/cols
def off(r):
    b, c = r['box'], r['cell_bbox']
    dx = (b[0] + b[2]) / 2 - (c[0] + c[2]) / 2
    dy = (b[1] + b[3]) / 2 - (c[1] + c[3]) / 2
    return (dx * dx + dy * dy) ** 0.5

cand.sort(key=off)
picks = []
seen = set()
for want in ('small', 'mid', 'large'):
    pool = cand[:12] if want == 'small' else (cand[-12:] if want == 'large' else cand[len(cand)//2-6:len(cand)//2+6])
    for r in pool:
        key = (r['cell_row'], r['cell_col'])
        if key in seen:
            continue
        seen.add(key)
        picks.append(r)
        break

S = 5
GREEN = (0, 190, 0)
RED = (230, 0, 0)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 15)
except Exception:
    font = ImageFont.load_default()

panels = []
for r in picks:
    c = r['cell_bbox']
    b = r['box']
    img = Image.open(f'{E2E}/{r["run_id"]}_frame.jpg').convert('RGB')
    pad = 70
    ox = int(min(c[0], b[0]) - pad)
    oy = int(min(c[1], b[1]) - pad)
    ex = int(max(c[2], b[2]) + pad)
    ey = int(max(c[3], b[3]) + pad)
    crop = img.crop((ox, oy, ex, ey))
    crop = crop.resize((crop.width * S, crop.height * S), Image.LANCZOS)
    dd = ImageDraw.Draw(crop)
    dd.rectangle([(b[0]-ox)*S, (b[1]-oy)*S, (b[2]-ox)*S, (b[3]-oy)*S], outline=RED, width=2)
    dd.rectangle([(c[0]-ox)*S, (c[1]-oy)*S, (c[2]-ox)*S, (c[3]-oy)*S], outline=GREEN, width=3)
    panels.append((crop, f"{r['run_id']} {chr(64+r['cell_row'])}{r['cell_col']}  "
                         f"red=ref box  green=cell  off={off(r):.0f}px"))

W = sum(p[0].width for p in panels[:2])
H = 0
for i in range(0, len(panels), 2):
    h = max(panels[i][0].height, panels[i+1][0].height if i+1 < len(panels) else 0)
    H += h + 24
sheet = Image.new('RGB', (W, H), (250, 250, 250))
sd = ImageDraw.Draw(sheet)
y = 0
for i in range(0, len(panels), 2):
    a = panels[i]
    sheet.paste(a[0], (0, y))
    sd.text((5, y + a[0].height + 3), a[1], fill=(10, 10, 10), font=font)
    h = a[0].height
    if i + 1 < len(panels):
        b2 = panels[i+1]
        sheet.paste(b2[0], (a[0].width, y))
        sd.text((a[0].width + 5, y + b2[0].height + 3), b2[1], fill=(10, 10, 10), font=font)
        h = max(h, b2[0].height)
    y += h + 24

sheet.save(f'{OUT}/verify_cells_check.jpg', quality=94)
print(f'wrote {OUT}/verify_cells_check.jpg ({sheet.width}x{sheet.height})')
for _, lab in panels:
    print(' ', lab)
