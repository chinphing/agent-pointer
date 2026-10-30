"""Side-by-side: what the annotator picked vs what the run landed on."""
import json, glob, os
from PIL import Image, ImageDraw, ImageFont

ANN = '/Users/starliu/pointer-all/pointer-eval-data/screenspot_grid/data/annotations'
IMG = '/Users/starliu/pointer-all/pointer-eval-data/screenspot_grid/data/images'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check'

CASES = {
    'word_macos_38': (1720.32, 751.68),
    'windows_common_windows_5': (1881.6, 1636.2),
    'windows_common_windows_41': (234.5, 546.5),
}

rows = {}
for f in glob.glob(os.path.join(ANN, '*.json')):
    for r in json.load(open(f)):
        if r['id'] in CASES:
            rows[r['id']] = r

try:
    F = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 20)
except Exception:
    F = ImageFont.load_default()

for cid, pt in CASES.items():
    r = rows[cid]
    b = r['bbox']
    img = Image.open(os.path.join(IMG, r['img_filename'])).convert('RGB')
    W, H = img.size
    tcx, tcy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    tl = max(b[2] - b[0], b[3] - b[1])
    win = max(160, int(tl * 5))          # context window around the target

    def shot(cx, cy, mark_truth, mark_point, label):
        x0 = int(cx - win / 2); y0 = int(cy - win / 2)
        x0 = max(0, min(x0, W - win)); y0 = max(0, min(y0, H - win))
        crop = img.crop((x0, y0, x0 + win, y0 + win))
        S = 420 / win
        crop = crop.resize((420, 420), Image.LANCZOS)
        d = ImageDraw.Draw(crop)
        if mark_truth:
            d.rectangle([(b[0]-x0)*S, (b[1]-y0)*S, (b[2]-x0)*S, (b[3]-y0)*S],
                        outline=(0, 220, 0), width=3)
        if mark_point:
            px, py = (pt[0]-x0)*S, (pt[1]-y0)*S
            if 0 <= px < 420 and 0 <= py < 420:
                d.line([px-14, py, px+14, py], fill=(255, 0, 0), width=3)
                d.line([px, py-14, px, py+14], fill=(255, 0, 0), width=3)
        d.text((8, 8), label, fill=(255, 255, 0), font=F)
        return crop

    left = shot(tcx, tcy, True, False, 'TRUTH (annotator)')
    right = shot(pt[0], pt[1], False, True, 'POINT (run)')

    canvas = Image.new('RGB', (852, 452), (30, 30, 30))
    canvas.paste(left, (4, 28))
    canvas.paste(right, (428, 28))
    d = ImageDraw.Draw(canvas)
    d.text((8, 4), f'{cid}  off={((pt[0]-tcx)**2+(pt[1]-tcy)**2)**0.5:.0f}px', fill=(255, 255, 255), font=F)
    out = os.path.join(OUT, f'cmp_{cid}.jpg')
    canvas.save(out, quality=94)
    print(f'{out}  win={win}px  target={tl:.0f}px')
