# locate baseline — sudoku, 81 cells

Recorded 2026-10-25. Re-run and compare before changing the locate prompt,
`LocateConfig`, or the hint pipeline.

## Dataset

`truth/truth_sudoku.json`, 81 cases over `fixtures/sudoku.jpg` (1920×1080).

- **Contents are stated honestly**: 43 empty cells (`空格子`), 38 holding a digit
  (`有数字N的格子`).
- **Two coordinate forms, split at random**: 40 `C3`-style (`coord_style:
  letter`), 41 `第三行第三列`-style (`coord_style: ordinal`). The board prints
  `A`–`I` down the side and `1`–`9` across the top, so both are readable.
- **Anchors carry distance and the right side**: `位于数字9的左边`, `…隔一格`.
- **Hint boxes are deliberately off**: anchor inside the cell, side 0.5–3× the
  cell side.
- **`goal`** is `选中这一格准备填数` for empty cells, `选中这一格` for filled ones.

## Command

```bash
POINTER_LOCATE_TRUTH=truth_sudoku.json POINTER_LOCATE_CONCURRENCY=10 \
  cargo test -p pointer-core --test locate_fixture frozen_desktop_grounding \
  -- --ignored --nocapture
```

## Result

| | v1 (before the locate fixes) | **v2 (current)** |
|---|---|---|
| hit | 56/81 = 69.1 % | **73/81 = 90.1 %** |
| completed | 66 | **80** |
| timed out | 15 | **1** |
| hit among completed | 84.8 % | **91.2 %** |
| wall clock | 297.8 s | 292.2 s |
| mean per completed case | 27.0 s | 32.2 s |
| median centre offset | — | **2.9 px** (cell is 41 px) |
| mean centre offset | — | 9.9 px |

The 70 % bar now passes. Offsets are measured as `|dx| + |dy|` from the true
cell centre.

## What changed between v1 and v2

Four defects in the locate retry path, each fixed separately:

1. **「框大得离谱」had no threshold.** The verifier spent whole rounds arguing
   with itself. Now: 1.5× the target's width and height, or 1.2× for tightly
   packed labelled cells (table cells, sudoku, chessboard, calendar).
2. **`box_too_loose` was missing from the `report` tool schema.** The model
   reasoned its way to the right verdict and then wrote 「unclear_kind 选项里没有
   box_too_loose」, falling back to `yes`. Fixed the enum.
3. **`refine_hint` had no loose-box branch**, so `no` + `elsewhere_dir=inside`
   produced 「不是目标」+「目标在框内部」. Added a symmetric branch that passes
   the measured ratio through.
4. **`BoxTooLoose` fell through to a full-screen re-locate** because the branch
   tested only `elsewhere_dir.is_some()`. The target never moved, so re-scanning
   1920×1080 re-derived the same answer from a harder image and blew the timeout.
   Now it stays in the crop.

Effect on the retry path — **retries no longer hurt**:

| rounds | v1 | v2 |
|---|---|---|
| 1 | 97.2 % | 95.7 % |
| 2 | 72.0 % | **89.1 %** |
| 3 | 60.0 % | 100 % (2 cases) |

## Breakdown (80 completed)

**By coordinate form** — the v1 gap is gone:

| `coord_style` | v1 | v2 |
|---|---|---|
| `letter` (`C3`) | 75.8 % | 90.0 % |
| `ordinal` (`第三行第三列`) | 93.9 % | 92.5 % |

The v1 split was an artefact: that dataset described 37 filled cells as empty,
and the two halves happened to draw different shares of those. Both forms are
equally readable when the description is truthful.

**By cell content** — the remaining weakness:

| | v2 |
|---|---|
| filled (`有数字7的格子`) | **37/37 = 100 %** |
| empty (`空格子`) | **36/43 = 83.7 %** |

A filled cell has a visual feature to key on. Empty cells are identical to each
other, so the anchor text is all the model has.

**By hint size** — improved but still the dominant factor:

| hint / cell | v1 | v2 |
|---|---|---|
| < 1× | 100 % | 100 % |
| 1–2× | 88.5 % | 93.5 % |
| ≥ 2× | 74.1 % | **85.7 %** |

## Remaining misses (7 of 80)

All seven are **empty cells**, and six carry a hint ≥ 1.55×:

```
r1c3  hint=1.55x  d=(+41,-41)
r2c6  hint=2.23x  d=(-41,-34)
r4c0  hint=2.66x  d=(+4,+22)
r4c1  hint=1.79x  d=(-13,-44)
r4c3  hint=2.83x  d=(-68,-2)
r4c6  hint=2.35x  d=(-1,-33)
r5c4  hint=2.61x  d=(-40,+1)
```

Three offsets are exactly ±41 px — **one full cell**. Those are
neighbour-confusion, not sloppiness: the box is centred on the adjacent cell.

`ok=false` (the grounder refused) happened **0 times**; all seven are `ok=true`
with the box on the wrong cell. The acceptance check still cannot tell
neighbouring empty cells apart when the anchor text does not disambiguate them.

## Timeout

One case exceeded 60 s under 10-way concurrency. Concurrency queues requests, so
part of that is scheduling rather than the model looping — re-run serially before
treating it as hard.

## v3 — anchors limited to adjacent or one gap

`「隔二格」` is ambiguous. r1c3 was anchored on the 9 in B1, three columns away,
written as 「位于数字9的右边隔二格」; the model read "two cells apart" as
distance 2 and went to column 6:

```
Target: "数独盘面B4的空格子，位于数字9的右边隔二格"...
Digit 9 is at column 4. Two columns right = column 6.
```

Only adjacent (「右边」) and one-gap (「右边隔一格」) anchors are generated now.
Distribution: 49 adjacent, 31 one-gap, 1 coordinate-only.

| | v2 | **v3** |
|---|---|---|
| hit (all 81, after serially re-running the timeouts) | 73/81 = 90.1 % | **73/81 = 90.1 %** |
| hit among completed | 91.2 % | **91.8 %** |
| empty cells | 83.7 % | **92.1 %** |
| filled cells | 100 % | 91.4 % |
| hint ≥ 2× | 85.7 % | 86.1 % |
| median centre offset | 2.9 px | 3.1 px |

**The two datasets are tied overall.** Empty cells improved by 8.4 points, which
was the target; filled cells lost 8.6 points. Small samples (38–43 per bucket),
so treat both movements as noise until a repeat run says otherwise.

Eight cases timed out under 10-way concurrency; re-running them serially finished
all eight in ≤ 18.4 s with 6 hits. **Timeouts in this harness are queueing, not
difficulty** — always re-run them serially before comparing runs.

The two genuine misses after the serial re-run:

```
r3c7  第四行第八列的空格子，位于数字2的右边隔一格   box landed one cell left
r5c8  第六行第九列的空格子，位于数字6的右边隔一格、数字9的下方   ok=false
```

`r3c7` shows `隔一格` still reads as "the next cell over" rather than "one cell
in between" — the same off-by-one as `隔二格`, just smaller. `r5c8` the model
refused outright: 「锚点自相矛盾，图中无符合的空格」.

## v4 — the verifier reports target and box sizes

`report` now requires two measured pairs alongside the verdict:

```
target_px  目标在裁剪图里的宽高
box_px     蓝框在裁剪图里的宽高
```

They are **recorded, not enforced** — `engine` still decides from `match` /
`unclear_kind`. The point is to make the model look at the box's edges before
answering, and to have the numbers on record for a later code-side check.

| | v3 | **v4** |
|---|---|---|
| hit (all 81, after serially re-running timeouts) | 73/81 = 90.1 % | **75/81 = 92.6 %** |
| **median centre offset** | 3.1 px | **1.7 px** |
| mean centre offset | 13.1 px | **5.9 px** |
| filled cells | 91.4 % | **100 %** |
| empty cells | 92.1 % | 86.0 % |
| hint < 1× | 100 % | 100 % |
| hint 1–2× | 96.0 % | 85.7 % |
| hint ≥ 2× | 86.1 % | **94.9 %** |
| mean per case | 12.4 s | **31.8 s** |
| cases needing >1 round | — | 66/81 |

**Median offset halved.** Half the cases that still miss land on an adjacent
cell, so the centre is either right or a full cell off — nothing in between.

**Cost: latency roughly tripled.** Requiring two extra measured fields makes
every verify call emit more tokens. Twelve cases timed out under 10-way
concurrency versus 8 in v3; re-running them serially finished all twelve in
≤ 41.5 s with 9 hits.

## Remaining misses (6 of 81, all empty cells)

```
r3c2  hint=1.85x  d=(-41,+0)    one cell left
r5c0  hint=2.01x  d=(-1,-41)    one cell up
r7c5  hint=1.33x  d=(-41,+0)    one cell left
r3c7  hint=1.21x  ok=false, 3 rounds
r4c3  hint=1.61x  ok=false, 3 rounds
r5c8  hint=2.44x  d=(-42,-0)    one cell left
```

Four are exactly one cell off; two exhausted their rounds refusing
(`ok=false`). **Filled cells are now perfect — every remaining failure is an
empty cell**, where the only distinguishing feature is the anchor text.

## What this baseline measures

81 near-identical targets, descriptions built only from coordinates and
neighbouring digits, hints from 0.5× to 3× the cell. It is a floor for
"look-alikes everywhere", not a general accuracy figure.

Signals that held across every dataset so far:

- hints above 2× the target hurt;
- filled cells are found perfectly, empty cells are not;
- after the v2 fixes, retrying helps instead of hurting.

## Superseded

- **v1** (69.1 %) ran on the same dataset before the four locate fixes above.
- **Pre-v1** (46.9 %, then 54.3 % with letter row labels) ran on a dataset whose
  descriptions claimed 37 filled cells were empty, and whose anchor directions
  were inverted. Those numbers measured the dataset's errors, not the grounder's.
