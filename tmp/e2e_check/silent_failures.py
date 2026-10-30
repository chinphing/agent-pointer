"""Find traces where verify said ok=true but the point missed, and print why."""
import json, glob, os, re

ANN = '/Users/starliu/pointer-all/pointer-eval-data/screenspot_grid/data/annotations'
CAP = os.path.expanduser('~/Library/Application Support/PointerApp/computer-captures/locate')

# instruction -> gt bbox, for the ids we care about
gt = {}
for f in glob.glob(os.path.join(ANN, '*.json')):
    for r in json.load(open(f)):
        gt[r['instruction'].strip()] = (r['id'], r['bbox'])


def instruction_of(desc):
    """The raw arm wraps the instruction in a task declaration."""
    m = re.search(r'The instruction is:\n(.+)$', desc, re.S)
    return (m.group(1) if m else desc).strip()


rows = []
for f in glob.glob(os.path.join(CAP, '*.json')):
    try:
        t = json.load(open(f))
    except Exception:
        continue
    o = t.get('outcome') or {}
    if not o.get('ok') or not o.get('point'):
        continue
    desc = (t.get('request') or {}).get('target_desc', '')
    key = instruction_of(desc)
    if key not in gt:
        continue
    rid, bbox = gt[key]
    px, py = o['point']
    if bbox[0] <= px <= bbox[2] and bbox[1] <= py <= bbox[3]:
        continue                      # a real hit
    rows.append((os.path.getmtime(f), f, rid, bbox, (px, py), t))

rows.sort(reverse=True)
print(f'{len(rows)} silent failures found\n')

for _, f, rid, bbox, pt, t in rows[:6]:
    cx, cy = (bbox[0] + bbox[2]) / 2, (bbox[1] + bbox[3]) / 2
    dist = ((pt[0] - cx) ** 2 + (pt[1] - cy) ** 2) ** 0.5
    print(f'=== {rid}  off={dist:.0f}px  target={bbox[2]-bbox[0]:.0f}x{bbox[3]-bbox[1]:.0f}px')
    print(f'    desc: {t["request"]["target_desc"][:120]!r}')
    print(f'    hint: {t["request"].get("hint_box")}')
    print(f'    point: {pt}  gt centre: ({cx:.0f},{cy:.0f})')
    for r in t.get('rounds', []):
        v = r.get('verify')
        if not v:
            continue
        print(f'    r{r["round"]}: center={v.get("center")} size={v.get("size")} '
              f'position={v.get("position")}')
        notes = (v.get('notes') or '').replace('\n', ' | ')
        print(f'       {notes[:400]}')
    print()
