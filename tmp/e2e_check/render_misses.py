"""Render the three remaining misses: truth box, hint, final point."""
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
HINTS = {
    'word_macos_38': [1268, 612, 1403, 627],
    'windows_common_windows_5': [912, 837, 956, 890],
    'windows_common_windows_41': [228, 540, 241, 553],
}

rows = {}
for f in glob.glob(os.path.join(ANN, '*.json')):
    for r in json.load(open(f)):
        if r['id'] in CASES:
            rows[r['id']] = r

for cid, pt in CASES.items():
    r = rows[cid]
    b = r['bbox']
    print(f'=== {cid}')
    print(f'  instruction: {r["instruction"]!r}')
    print(f'  img: {r["img_filename"]}  size={r["img_size"]}  ui_type={r["ui_type"]}')
    print(f'  gt bbox={b}  {b[2]-b[0]:.0f}x{b[3]-b[1]:.0f}px  centre=({(b[0]+b[2])/2:.0f},{(b[1]+b[3])/2:.0f})')
    print(f'  point=({pt[0]:.0f},{pt[1]:.0f})  dist={((pt[0]-(b[0]+b[2])/2)**2+(pt[1]-(b[1]+b[3])/2)**2)**0.5:.0f}px')
    print(f'  point inside gt? {b[0]<=pt[0]<=b[2] and b[1]<=pt[1]<=b[3]}')

    path = os.path.join(IMG, r['img_filename'])
    img = Image.open(path).convert('RGB')
    W, H = img.size
    d = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 22)
    except Exception:
        font = ImageFont.load_default()

    h = HINTS[cid]
    d.rectangle(b, outline=(0, 220, 0), width=5)                    # truth
    d.rectangle(h, outline=(0, 120, 255), width=3)                  # hint
    px, py = pt
    d.line([px-22, py, px+22, py], fill=(255, 0, 0), width=4)
    d.line([px, py-22, px, py+22], fill=(255, 0, 0), width=4)
    d.text((b[0], max(0, b[1]-28)), 'TRUTH', fill=(0, 220, 0), font=font)
    d.text((h[0], min(H-26, h[3]+4)), 'hint', fill=(0, 120, 255), font=font)

    # downscale for delivery, keep whole screen
    if W > 1600:
        img = img.resize((1600, round(H * 1600 / W)), Image.LANCZOS)
    out = os.path.join(OUT, f'miss_{cid}.jpg')
    img.save(out, quality=92)
    print(f'  -> {out} ({img.width}x{img.height})\n')
