"""Render the vscode tab strip: truth box, hint box, final point."""
from PIL import Image, ImageDraw, ImageFont

IMG = '/Users/starliu/pointer-all/pointer-eval-data/screenspot_grid/data/images/vscode_mac/screenshot_2024-12-03_16-55-36.png'
OUT = '/Users/starliu/pointer-all/agent-pointer/tmp/e2e_check/vscode_tabs.jpg'

TRUTH = [723, 104, 738, 124]
HINT = [1265, 107, 1315, 124]
POINT = (1175.28, 113.14)

img = Image.open(IMG).convert('RGB')
print('image size', img.size)

# tab strip region: wide enough to see both boxes
ox, oy, ex, ey = 640, 70, 1360, 160
crop = img.crop((ox, oy, ex, ey))
S = 4
crop = crop.resize((crop.width * S, crop.height * S), Image.LANCZOS)
d = ImageDraw.Draw(crop)

try:
    font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 15)
except Exception:
    font = ImageFont.load_default()


def rect(b, colour, w, label):
    d.rectangle([(b[0]-ox)*S, (b[1]-oy)*S, (b[2]-ox)*S, (b[3]-oy)*S], outline=colour, width=w)
    d.text(((b[0]-ox)*S + 2, (b[1]-oy)*S - 18), label, fill=colour, font=font)


rect(TRUTH, (0, 190, 0), 2, 'truth (utils.py x)')
rect(HINT, (0, 90, 230), 2, 'hint')
px, py = (POINT[0]-ox)*S, (POINT[1]-oy)*S
d.line([px-10, py, px+10, py], fill=(230, 0, 0), width=3)
d.line([px, py-10, px, py+10], fill=(230, 0, 0), width=3)
d.text((px+12, py-8), 'final point', fill=(230, 0, 0), font=font)

crop.save(OUT, quality=94)
print(f'wrote {OUT} ({crop.width}x{crop.height})')
print(f'truth centre=({(TRUTH[0]+TRUTH[2])/2:.0f},{(TRUTH[1]+TRUTH[3])/2:.0f})')
print(f'point={POINT}  off={abs(POINT[0]-(TRUTH[0]+TRUTH[2])/2):.0f}px')
