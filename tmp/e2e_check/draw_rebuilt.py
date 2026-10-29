"""Render the rebuilt truth boxes over the frames to eyeball alignment."""
import json
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json.rebuilt'))
rebuilt = [c for c in d['cases'] if 'old_bbox' in c]
by = {c['scene']: c for c in rebuilt}

PICK = ['c4826d07', 'd6373a20', 'def03c2c', '07f72a66']
SCALE = 6
GREEN = (0, 200, 0)
RED = (230, 0, 0)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 15)
except Exception:
    font = ImageFont.load_default()

panels = []
for scene in PICK:
    c = by.get(scene)
    if not c:
        continue
    b, ob = c['bbox'], c['old_bbox']
    img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')
    pad = 55
    ox, oy = int(b[0] - pad), int(b[1] - pad)
    ex, ey = int(b[2] + pad), int(b[3] + pad)
    crop = img.crop((ox, oy, ex, ey))
    crop = crop.resize((crop.width * SCALE, crop.height * SCALE), Image.LANCZOS)
    dd = ImageDraw.Draw(crop)
    dd.rectangle([(ob[0]-ox)*SCALE, (ob[1]-oy)*SCALE, (ob[2]-ox)*SCALE, (ob[3]-oy)*SCALE],
                 outline=RED, width=2)
    dd.rectangle([(b[0]-ox)*SCALE, (b[1]-oy)*SCALE, (b[2]-ox)*SCALE, (b[3]-oy)*SCALE],
                 outline=GREEN, width=2)
    dx = (b[0]+b[2])/2 - (ob[0]+ob[2])/2
    dy = (b[1]+b[3])/2 - (ob[1]+ob[3])/2
    panels.append((crop, f'{scene}  red=old  green=rebuilt  shift=({dx:+.0f},{dy:+.0f})'))

W = sum(p[0].width for p in panels[:2])
H = 0
for i in range(0, len(panels), 2):
    h = max(panels[i][0].height, panels[i+1][0].height if i+1 < len(panels) else 0)
    H += h + 26
sheet = Image.new('RGB', (W, H), (250, 250, 250))
sd = ImageDraw.Draw(sheet)
y = 0
for i in range(0, len(panels), 2):
    a = panels[i]
    sheet.paste(a[0], (0, y))
    sd.text((5, y + a[0].height + 4), a[1], fill=(10, 10, 10), font=font)
    h = a[0].height
    if i + 1 < len(panels):
        b2 = panels[i+1]
        sheet.paste(b2[0], (a[0].width, y))
        sd.text((a[0].width + 5, y + b2[0].height + 4), b2[1], fill=(10, 10, 10), font=font)
        h = max(h, b2[0].height)
    y += h + 26

sheet.save(f'{OUT}/rebuilt_check.jpg', quality=94)
print(f'wrote {OUT}/rebuilt_check.jpg ({sheet.width}x{sheet.height})')
for _, lab in panels:
    print(' ', lab)
