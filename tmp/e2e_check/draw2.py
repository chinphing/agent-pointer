"""Render truth box alone (green) and locate box alone (red), side by side.

Two separate panels per case so neither box can occlude the other, and integer
crop origin shared by both the crop and the drawing transform.
"""
import json
import os
from PIL import Image, ImageDraw, ImageFont

DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'
os.makedirs(OUT, exist_ok=True)

truth = {c['scene']: c for c in json.load(open(f'{DATA}/e2e_truth.json'))['cases']}

CASES = [
    ('c4826d07', [336, 558, 379, 612]),
    ('d6373a20', [246, 488, 285, 527]),
    ('def03c2c', [168, 448, 210, 486]),
]

SCALE = 8
GREEN = (0, 190, 0)
RED = (225, 0, 0)
GRID = (120, 120, 255)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 16)
except Exception:
    font = ImageFont.load_default()

panels = []
for scene, box in CASES:
    b = truth[scene]['bbox']
    img = Image.open(f'{DATA}/{scene}_frame.jpg').convert('RGB')

    pad = 40
    # one integer origin for both the crop and the transform
    ox = int(min(b[0], box[0]) - pad)
    oy = int(min(b[1], box[1]) - pad)
    ex = int(max(b[2], box[2]) + pad)
    ey = int(max(b[3], box[3]) + pad)

    for rect, colour, tag in ((b, GREEN, 'TRUTH'), (box, RED, 'LOCATE')):
        crop = img.crop((ox, oy, ex, ey))
        crop = crop.resize((crop.width * SCALE, crop.height * SCALE), Image.LANCZOS)
        d = ImageDraw.Draw(crop)
        d.rectangle(
            [(rect[0] - ox) * SCALE, (rect[1] - oy) * SCALE,
             (rect[2] - ox) * SCALE, (rect[3] - oy) * SCALE],
            outline=colour, width=2,
        )
        # centre cross
        cx, cy = ((rect[0] + rect[2]) / 2 - ox) * SCALE, ((rect[1] + rect[3]) / 2 - oy) * SCALE
        d.line([cx - 9, cy, cx + 9, cy], fill=colour, width=2)
        d.line([cx, cy - 9, cx, cy + 9], fill=colour, width=2)
        panels.append((crop, f'{scene} {tag} {int(rect[2]-rect[0])}x{int(rect[3]-rect[1])}'))

W = sum(p[0].width for p in panels[:2])
H = max(p[0].height for p in panels) * 3 + 26 * 3
sheet = Image.new('RGB', (W, H), (248, 248, 248))
sd = ImageDraw.Draw(sheet)

y = 0
for i in range(0, len(panels), 2):
    left, right = panels[i], panels[i + 1]
    sheet.paste(left[0], (0, y))
    sheet.paste(right[0], (left[0].width, y))
    sd.text((6, y + left[0].height + 3), left[1], fill=(15, 15, 15), font=font)
    sd.text((left[0].width + 6, y + right[0].height + 3), right[1], fill=(15, 15, 15), font=font)
    y += max(left[0].height, right[0].height) + 26

sheet.save(f'{OUT}/separate.jpg', quality=94)
print(f'wrote {OUT}/separate.jpg ({sheet.width}x{sheet.height})')
