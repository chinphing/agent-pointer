"""Render truth box vs the locate box for three e2e cases."""
import json, os
from PIL import Image, ImageDraw, ImageFont

ROOT = '/Users/starliu/pointer-all/agent-pointer'
DATA = '/Users/starliu/pointer-all/pointer-eval-data/computer_e2e'
OUT = f'{ROOT}/tmp/e2e_check'
os.makedirs(OUT, exist_ok=True)

truth = {c['scene']: c for c in json.load(open(f'{DATA}/e2e_truth.json'))['cases']}

# (scene, locate box = the hint box the main LLM produced, which locate accepted)
CASES = [
    ('c4826d07', [336, 558, 379, 612]),
    ('d6373a20', [246, 488, 285, 527]),
    ('def03c2c', [168, 448, 210, 486]),
]

GREEN = (0, 190, 0)
RED = (225, 0, 0)
BLUE = (0, 90, 220)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 15)
except Exception:
    font = ImageFont.load_default()

tiles = []
for scene, box in CASES:
    c = truth[scene]
    b = c['bbox']
    img = Image.open(f'{DATA}/{scene}_frame.jpg').convert('RGB')

    # crop generously around both boxes so the offset is visible in context
    x1 = min(b[0], box[0]) - 45
    y1 = min(b[1], box[1]) - 45
    x2 = max(b[2], box[2]) + 45
    y2 = max(b[3], box[3]) + 45
    crop = img.crop((int(x1), int(y1), int(x2), int(y2)))

    scale = 4
    crop = crop.resize((crop.width * scale, crop.height * scale), Image.LANCZOS)
    d = ImageDraw.Draw(crop)

    def sx(v):
        return (v - x1) * scale

    def sy(v):
        return (v - y1) * scale

    d.rectangle([sx(b[0]), sy(b[1]), sx(b[2]), sy(b[3])], outline=GREEN, width=3)
    d.rectangle([sx(box[0]), sy(box[1]), sx(box[2]), sy(box[3])], outline=RED, width=3)

    bcx, bcy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    lcx, lcy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    r = 5
    d.ellipse([sx(bcx) - r, sy(bcy) - r, sx(bcx) + r, sy(bcy) + r], fill=GREEN)
    d.ellipse([sx(lcx) - r, sy(lcy) - r, sx(lcx) + r, sy(lcy) + r], fill=RED)
    d.line([sx(bcx), sy(bcy), sx(lcx), sy(lcy)], fill=BLUE, width=2)

    dx, dy = lcx - bcx, lcy - bcy
    label = f'{scene}  truth {int(b[2]-b[0])}x{int(b[3]-b[1])}  d=({dx:+.0f},{dy:+.0f})'
    tiles.append((crop, label, (dx * dx + dy * dy) ** 0.5))

W = max(t[0].width for t in tiles)
H = sum(t[0].height + 26 for t in tiles)
sheet = Image.new('RGB', (W, H), (248, 248, 248))
sd = ImageDraw.Draw(sheet)
y = 0
for crop, label, dist in tiles:
    sheet.paste(crop, (0, y))
    sd.text((6, y + crop.height + 4), label, fill=(15, 15, 15), font=font)
    y += crop.height + 26

sheet.save(f'{OUT}/three_cases.jpg', quality=93)
for crop, label, dist in tiles:
    print(label, f'|{dist:.0f}px|')
print(f'\nwrote {OUT}/three_cases.jpg ({sheet.width}x{sheet.height})')
