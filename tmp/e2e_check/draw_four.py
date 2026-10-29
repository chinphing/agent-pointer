"""Render the description-derived truth boxes for the four failing cases."""
import json
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json.rebuilt4'))
by = {c['scene']: c for c in d['cases']}

SCENES = ['d6373a20', '07f72a66', '0dfd3f6f', 'cd2ba013']
S = 5
GREEN = (0, 190, 0)
RED = (230, 0, 0)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 15)
except Exception:
    font = ImageFont.load_default()

panels = []
for scene in SCENES:
    c = by[scene]
    b, ob = c['bbox'], c['old_bbox']
    img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')
    pad = 75
    ox, oy = int(b[0] - pad), int(b[1] - pad)
    ex, ey = int(b[2] + pad), int(b[3] + pad)
    crop = img.crop((ox, oy, ex, ey))
    crop = crop.resize((crop.width * S, crop.height * S), Image.LANCZOS)
    dd = ImageDraw.Draw(crop)
    dd.rectangle([(ob[0]-ox)*S, (ob[1]-oy)*S, (ob[2]-ox)*S, (ob[3]-oy)*S], outline=RED, width=2)
    dd.rectangle([(b[0]-ox)*S, (b[1]-oy)*S, (b[2]-ox)*S, (b[3]-oy)*S], outline=GREEN, width=3)
    panels.append((crop, f"{scene} cell={c['cell']}  red=old  green=new"))

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

sheet.save(f'{OUT}/four_cases_new.jpg', quality=94)
print(f'wrote {OUT}/four_cases_new.jpg ({sheet.width}x{sheet.height})')
