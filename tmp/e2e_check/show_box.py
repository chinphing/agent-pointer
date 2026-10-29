"""Render a candidate bbox on the frame so it can be eyeballed."""
import json
import sys
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json'))
by = {c['scene']: c for c in d['cases']}

scene = sys.argv[1] if len(sys.argv) > 1 else 'd6373a20'
box = json.loads(sys.argv[2]) if len(sys.argv) > 2 else [250, 445, 370, 555]

c = by[scene]
img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')

pad = 90
ox, oy = int(min(c['bbox'][0], box[0]) - pad), int(min(c['bbox'][1], box[1]) - pad)
ex, ey = int(max(c['bbox'][2], box[2]) + pad), int(max(c['bbox'][3], box[3]) + pad)
crop = img.crop((ox, oy, ex, ey))
S = 5
crop = crop.resize((crop.width * S, crop.height * S), Image.LANCZOS)
dd = ImageDraw.Draw(crop)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 16)
except Exception:
    font = ImageFont.load_default()

def rect(b, colour, w, label):
    dd.rectangle([(b[0]-ox)*S, (b[1]-oy)*S, (b[2]-ox)*S, (b[3]-oy)*S], outline=colour, width=w)
    dd.text(((b[0]-ox)*S + 3, (b[1]-oy)*S - 19), label, fill=colour, font=font)

rect(box, (230, 0, 0), 3, f'vision bbox {box}')
rect(c['bbox'], (0, 190, 0), 2, f'current truth')

crop.save(f'{OUT}/{scene}_check.jpg', quality=94)
print(f'wrote {OUT}/{scene}_check.jpg ({crop.width}x{crop.height})')
print(f'  vision bbox = {box}  size {box[2]-box[0]}x{box[3]-box[1]}')
print(f'  current truth = {[round(v) for v in c["bbox"]]}  size {c["bbox"][2]-c["bbox"][0]:.0f}x{c["bbox"][3]-c["bbox"][1]:.0f}')
