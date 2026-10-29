"""Render the whole frame with a coordinate ruler, for asking a vision model
for the sudoku board's outer bounds."""
import json
import sys
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
d = json.load(open(f'{DATA}/e2e_truth.json'))
by = {c['scene']: c for c in d['cases']}

scene = sys.argv[1] if len(sys.argv) > 1 else 'd6373a20'
c = by[scene]
img = Image.open(f'{DATA}/{c["fixture"]}').convert('RGB')
W, H = img.size

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 13)
except Exception:
    font = ImageFont.load_default()

# downscale a bit so the whole screen fits one vision call, then draw rulers
S = 0.75
im = img.resize((int(W * S), int(H * S)), Image.LANCZOS)
dd = ImageDraw.Draw(im)

for x in range(0, W, 100):
    X = int(x * S)
    dd.line([X, 0, X, im.height], fill=(255, 120, 0), width=1)
    dd.text((X + 2, 2), str(x), fill=(255, 120, 0), font=font)
for y in range(0, H, 100):
    Y = int(y * S)
    dd.line([0, Y, im.width, Y], fill=(255, 120, 0), width=1)
    dd.text((2, Y + 1), str(y), fill=(255, 120, 0), font=font)

p = f'{OUT}/{scene}_full_ruler.jpg'
im.save(p, quality=90)
print(f'{scene}  source {W}x{H}  rendered {im.size} -> {p}')
print(f'  target: {c["description"][:70]}')
