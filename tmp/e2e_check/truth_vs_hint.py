"""Render the full frame with the rebuilt truth box and the hint box overlaid."""
import json
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json'))
by = {c['scene']: c for c in d['cases']}

# scene -> hint box that locate actually received (from the trace)
CASES = {
    'd6373a20': [258, 485, 302, 528],
}

SCALE = 5
GREEN = (0, 200, 0)
RED = (230, 0, 0)
BLUE = (0, 90, 230)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 16)
except Exception:
    font = ImageFont.load_default()

panels = []
for scene, hint in CASES.items():
    c = by[scene]
    nb = c['bbox']
    ob = c.get('old_bbox')
    img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')

    # crop the whole board so the column labels are visible
    pad = 120
    ox = int(min(nb[0], hint[0]) - pad)
    oy = int(min(nb[1], hint[1]) - pad)
    ex = int(max(nb[2], hint[2]) + pad)
    ey = int(max(nb[3], hint[3]) + pad)
    crop = img.crop((ox, oy, ex, ey))
    crop = crop.resize((crop.width * SCALE, crop.height * SCALE), Image.LANCZOS)
    dd = ImageDraw.Draw(crop)

    def rect(b, colour, w, label=None):
        dd.rectangle([(b[0]-ox)*SCALE, (b[1]-oy)*SCALE, (b[2]-ox)*SCALE, (b[3]-oy)*SCALE],
                     outline=colour, width=w)
        if label:
            dd.text(((b[0]-ox)*SCALE + 3, (b[1]-oy)*SCALE - 18), label, fill=colour, font=font)

    if ob:
        rect(ob, BLUE, 2, 'old truth')
    rect(nb, GREEN, 3, 'new truth')
    rect(hint, RED, 2, 'hint / locate')

    panels.append((crop, f'{scene}  blue=old truth  green=rebuilt  red=hint'))

W = max(p[0].width for p in panels)
H = sum(p[0].height + 26 for p in panels)
sheet = Image.new('RGB', (W, H), (250, 250, 250))
sd = ImageDraw.Draw(sheet)
y = 0
for crop, label in panels:
    sheet.paste(crop, (0, y))
    sd.text((6, y + crop.height + 4), label, fill=(10, 10, 10), font=font)
    y += crop.height + 26
sheet.save(f'{OUT}/truth_vs_hint.jpg', quality=94)
print(f'wrote {OUT}/truth_vs_hint.jpg ({sheet.width}x{sheet.height})')
for scene, hint in CASES.items():
    nb = by[scene]['bbox']
    print(f'{scene}: new truth={[round(v) for v in nb]} centre=({(nb[0]+nb[2])/2:.0f},{(nb[1]+nb[3])/2:.0f})')
    print(f'         hint={hint} centre=({(hint[0]+hint[2])/2:.0f},{(hint[1]+hint[3])/2:.0f})')
