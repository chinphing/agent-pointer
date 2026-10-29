"""Render the five failing cases: truth box (green) vs hint box (red)."""
import json
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json'))
by = {c['scene']: c for c in d['cases']}

# hint_box exactly as locate received it (matched by final point in the trace)
HINTS = {
    '0069815c': [236, 322, 276, 358],
    '0e896470': [268, 237, 310, 280],
    'f672584d': [232, 368, 272, 402],
    'ed32a73f': [391, 361, 437, 401],
    'c788790b': [125, 540, 165, 578],
}
ORDER = ['0069815c', '0e896470', 'f672584d', 'ed32a73f', 'c788790b']

S = 5
GREEN = (0, 190, 0)
RED = (230, 0, 0)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 15)
except Exception:
    font = ImageFont.load_default()

panels = []
for scene in ORDER:
    c = by[scene]
    b = c['bbox']
    hint = HINTS[scene]
    img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')

    pad = 70
    ox = int(min(b[0], hint[0]) - pad)
    oy = int(min(b[1], hint[1]) - pad)
    ex = int(max(b[2], hint[2]) + pad)
    ey = int(max(b[3], hint[3]) + pad)
    crop = img.crop((ox, oy, ex, ey))
    crop = crop.resize((crop.width * S, crop.height * S), Image.LANCZOS)
    dd = ImageDraw.Draw(crop)

    dd.rectangle([(hint[0]-ox)*S, (hint[1]-oy)*S, (hint[2]-ox)*S, (hint[3]-oy)*S],
                 outline=RED, width=2)
    dd.rectangle([(b[0]-ox)*S, (b[1]-oy)*S, (b[2]-ox)*S, (b[3]-oy)*S],
                 outline=GREEN, width=3)

    hc = ((hint[0]+hint[2])/2, (hint[1]+hint[3])/2)
    tc = ((b[0]+b[2])/2, (b[1]+b[3])/2)
    for (px, py), colour in ((hc, RED), (tc, GREEN)):
        X, Y = (px-ox)*S, (py-oy)*S
        dd.line([X-8, Y, X+8, Y], fill=colour, width=2)
        dd.line([X, Y-8, X, Y+8], fill=colour, width=2)

    dx, dy = hc[0]-tc[0], hc[1]-tc[1]
    panels.append((crop, f'{scene} cell={c.get("cell")}  red=hint  green=truth  off=({dx:+.0f},{dy:+.0f})'))

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

sheet.save(f'{OUT}/five_fails.jpg', quality=94)
print(f'wrote {OUT}/five_fails.jpg ({sheet.width}x{sheet.height})')
for _, lab in panels:
    print(' ', lab)
