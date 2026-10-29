"""Overlay a coordinate ruler on the frame so a vision model can report bbox pixels."""
import json
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json'))
by = {c['scene']: c for c in d['cases']}

SCENES = ['d6373a20']

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 14)
except Exception:
    font = ImageFont.load_default()

for scene in SCENES:
    c = by[scene]
    img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')
    W, H = img.size

    # work on the board region only, at 2x, with a 50px ruler
    pad = 140
    b = c['bbox']
    ox = int(b[0] - pad)
    oy = int(b[1] - pad)
    ex = int(b[2] + pad)
    ey = int(b[3] + pad)
    crop = img.crop((ox, oy, ex, ey))
    S = 3
    crop = crop.resize((crop.width * S, crop.height * S), Image.LANCZOS)
    dd = ImageDraw.Draw(crop)

    # ruler lines every 50 source px, labelled with source coordinates
    for x in range(ox - ox % 50, ex, 50):
        X = (x - ox) * S
        if 0 <= X < crop.width:
            dd.line([X, 0, X, crop.height], fill=(255, 140, 0), width=1)
            dd.text((X + 2, 2), str(x), fill=(255, 140, 0), font=font)
    for y in range(oy - oy % 50, ey, 50):
        Y = (y - oy) * S
        if 0 <= Y < crop.height:
            dd.line([0, Y, crop.width, Y], fill=(255, 140, 0), width=1)
            dd.text((2, Y + 1), str(y), fill=(255, 140, 0), font=font)

    p = f'{OUT}/{scene}_ruler.jpg'
    crop.save(p, quality=92)
    print(f'{scene}: {c["description"][:60]}')
    print(f'  crop origin=({ox},{oy}) size={crop.size}  -> {p}')
