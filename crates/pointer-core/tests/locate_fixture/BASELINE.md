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

Both columns cover only the cases that finished under concurrency — the 10 and 1
timeouts are excluded. Comparing a partial set against a full set is how an
earlier revision of this file ended up claiming a 2.6× latency regression that
did not exist; from v3 onward the tables use all 81 cases after serial re-runs.
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
| mean per case (all 81, after serial re-runs) | 29.2 s | **31.8 s** |
| mean per round | 16.8 s | **16.6 s** |
| total rounds | 141 | **155** |

**Median offset halved.** Half the cases that still miss land on an adjacent
cell, so the centre is either right or a full cell off — nothing in between.

**Latency rose 8.9 %, and none of it comes from slower calls.** Mean time per
round is flat (16.8 s → 16.6 s). What changed is the round count: 11 cases that
used to pass in one round now take two.

That is the intended effect, not a regression. Previously the verifier saw "the
target is in the box" and stopped; now it has to measure the box's edges to fill
`box_px`, notices the box is wrong, and reports `unclear` — so the correction
that was always needed finally happens. v3 had 7 cases with `ok=true` and the box
on the wrong cell; v4 has 4, and the median offset halved.

Round distribution:

| rounds | v3 | v4 |
|---|---|---|
| 1 | 26 | **15** |
| 2 | 50 | **58** |
| 3 | 5 | **8** |

Twelve cases timed out under 10-way concurrency (v3: 8); re-running them
serially finished all twelve in ≤ 41.5 s with 9 hits.

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

### All six pass with thinking on

Re-running those six with `POINTER_LOCATE_THINKING=1`:

| case | hit | rounds | secs | offset |
|---|---|---|---|---|
| r3c2 | ✅ | 2 | 38.5 | (0, 0) |
| r3c7 | ✅ | 2 | 99.9 | (0, −1) |
| r4c3 | ✅ | 2 | 72.2 | (+1, −2) |
| r5c0 | ✅ | 2 | 33.9 | (0, 0) |
| r5c8 | ✅ | 2 | 51.3 | (0, 0) |
| r7c5 | ✅ | 2 | 70.2 | (0, −1) |

6/6, every one in two rounds, offsets 0–2 px. Mean 61 s.

So the remaining misses are not a difficulty ceiling — they are the same
measurement failure as the small-hint cases, just surfacing on empty cells where
the anchor text is the only cue. With thinking off the verifier reads 「框里有
空白格」 and stops; with it on, it counts columns and rows and gets there.

One case (r7c5) timed out at >180 s when six ran together but finished in 70 s
alone — that was queueing, not difficulty, same as the timeout caveat for the
main run.

### Deferred: blank-region targets

`truth_desktop.json` gained five 「点击空白处」 cases. All five fail, and the
cause is structural rather than a prompt bug:

- **「正上方」 has no unique solution.** Told 「App Store 图标正上方那片空地」 the
  verifier frames the strip hugging the icon (y≈870) while the ground truth is
  170 px higher (y=700). Its x was exact (375.7 vs 400).
- **A blank region has no single correct centre.** One case returned a 741×736
  box centred on the screen — correct per the description 「左右无图标」, wrong per
  the ground truth's 200×200 window on the left third.
- **The anchor can fall outside the crop.** `side = 框长边 × 3`, clamped to
  ≥240. A 2 px-wide box gives side=240, so the 访达 icon at y=910 is outside a
  crop covering y≈660–900. The verifier correctly refused with
  `anchor_out_of_view` — but the second-round `locate_in` reuses that same crop,
  so it cannot see the anchor either.

Scoring blank regions by 「centre inside a fixed bbox」 measures whether the model
guessed which patch the annotator picked, not whether it found a safe place to
click. Either the harness needs a 「centre lands in any icon-free area」 check, or
these targets are out of scope for a design that assumes a frameable element.

Not fixed. The five cases stay in the dataset as a marker.

### Deferred: are `target_px` / `box_px` usable for code-side judgement?

Not yet measured. Need the per-case values written into the `--nocapture` output
so they can be paired with ground truth; `RUST_LOG` at 10-way concurrency
interleaves them. Open question is whether the **ratio** is stable — the two
numbers can both be off while their quotient is fine, and it is the quotient a
code-side check would use.

Spot samples so far disagree: four readings of a true ratio of 1.78 gave
1.33 / 1.48 / 1.25 / 2.50, but r7c5 gave 1.33 against a true 1.32. Too few
samples to call it either way.

## v5 — the verifier must check its own boundary against the box

`verify_prompt` gained a fourth task step:

```
④ 写完这个边界，立刻拿它和蓝框比一次：蓝框的左右边缘是不是落在这个边界里？
   框比边界偏左或偏右就是框错了，判 unclear——不要因为「框子里确实是空格子」就判 yes。
```

The motivating failure was r4c3, whose `why` read 「蓝框偏右压住第5列边」 — it saw
the mismatch, wrote it down, and still answered `match=yes`.

| | v4 | **v5** |
|---|---|---|
| hit (all 81, after serial re-runs) | 75/81 = 92.6 % | **76/81 = 93.8 %** |
| median centre offset | 1.7 px | 2.3 px |
| mean centre offset | 5.9 px | **4.4 px** |
| filled cells | 100 % | 100 % |
| empty cells | 86.0 % | **88.4 %** |
| hint < 1× | 100 % | 100 % |
| hint 1–2× | 85.7 % | **88.9 %** |
| hint ≥ 2× | 94.9 % | 95.1 % |
| `ok=false` (refused) | 2 | 3 |
| `ok=true` but box on the wrong cell | 4 | **2** |
| mean per case | 31.8 s | 35.4 s |

The number that matters most is the last one but one: **cases that claimed success
with the box on the wrong cell dropped from 4 to 2.** Those are the silent failures
— a caller would click the wrong place and never know.

Nine cases timed out under 10-way concurrency; re-running them serially finished
all nine in ≤ 29.3 s with 7 hits.

## The five remaining misses, with the verifier's own words

**Two are `ok=true` with the box slightly off:**

```
r2c6  hint=2.37x  d=(+1,-34)   one row up
      框内是一个高亮的空白数独格。目标 C7 空格子位于第 3 行（含 8、4 的那一行）第 7 列，
      该列横跨 x≈415 到 580，正是蓝框所在列；其左边是第 6 列空格、右边是数字…

r3c7  hint=1.21x  d=(-14,-22)  half a cell left and up
      蓝框内是数独盘面的空白格子（无数字）。盘面列边界：第4列x≈310-468、
      第5列x≈468-642、第6列x≈642-810；行边界：第4行y≈497-670。目标"第四行第八列…
```

r2c6 got the column right and the row wrong by exactly one cell. r3c7's boundary
table is internally consistent but shifted — its 「第4列」 starts at x≈310, which
in the crop's own pixels is not where column 4 is.

**Three are `ok=false` — the verifier refused, correctly, and three rounds were
not enough to converge:**

```
r7c7  hint=2.94x
      蓝框里是含数字6的格子（高亮白底），非空格。目标空格在6左上方：
      第2列横跨x≈185-360、第4行y≈250-420，即7下方、8右方；框偏右下约两格。

r5c4  hint=1.89x
      框内是空白格（第4列，x≈455-625）。目标锚点：数字2在第3列(x≈300-455)，
      隔一格右即第5列；上方数字3在第5列(x≈625-790)。故目标应在第5列横跨x≈62…

r7c5  hint=1.33x
      框里是白色高亮空格。目标H6应是普通灰底空格，被「3」和「5」夹住：
      第4列横跨x≈410到585、行y≈415到585，该列正是框所在列。
      但框把整格涂白、四条边线全部不可见，无法核…
```

**r5c4 is the most telling.** Its reasoning is correct end to end: it identifies
column 3 from the 「2」, steps one cell right to column 5, cross-checks against
the 「3」 above — and still fails, because the locator could not produce a box
matching that conclusion within three rounds. **The verifier knew where the target
was and could not make the locator go there.**

r7c5 is a different shape: the highlight fill makes the cell's own border
invisible, so the verifier cannot confirm 「四条边线」 and refuses forever.

## What changed in the failure mix

v4's four silent failures were mostly 「column counted wrong, box one cell left」.
v5's two silent failures are one row off and one half-cell off — **the left-neighbour
class is gone from the silent failures, and now shows up as explicit refusals instead.**

That is the intended trade: refusing is visible to the caller, a wrong box is not.

## What this baseline measures

81 near-identical targets, descriptions built only from coordinates and
neighbouring digits, hints from 0.5× to 3× the cell. It is a floor for
"look-alikes everywhere", not a general accuracy figure.

Signals that held across every dataset so far:

- hints above 2× the target hurt;
- filled cells are found perfectly, empty cells are not;
- after the v2 fixes, retrying helps instead of hurting.

## v6 — three independent checks, plus a centre mark

**Result: 76/81 (93.8 %) · median 2.6 px · mean 5.0 px · 0 wrong cells · 86.7 s/case**
(concurrency 8, per-case timeout 400 s, 81/81 completed with no timeouts)

Three runs on the same configuration gave 95.1 %, 96.3 % and 93.8 %; the run
reported above is the only one where all 81 cases finished, so it is the one to
compare against.

### The problem with one verdict

v5 asked a single question: 「is the target in the box」. A box generous enough to
contain the target answers *yes* — and then the engine returns the box's **centre**,
which can be a whole cell away. Two distinct failures hid behind that:

| failure | what the verifier said | what was actually wrong |
| --- | --- | --- |
| box one cell off, target still inside it | `match = yes` | the **centre** was not on the target |
| box tight around a neighbouring empty cell | `match = yes` | the **cell** was the wrong one |

Neither is visible from inside the box. Both need a check that looks *outside* it.

### The three checks

| field | question | fails as |
| --- | --- | --- |
| **`center_on_target`** | is the centre mark on the target? | coordinate wrong |
| **`size_ok`** | does the box frame the target (≤1.5×, fully containing it)? | coordinate drifts |
| **`position_matches`** | is this the cell the description names? | coordinate off by a cell |

A hit needs all three. They are orthogonal: a box can be perfectly sized and
centred on the wrong cell, or correctly placed on the right cell and twice its
size — and each combination needs a different repair.

### The centre mark

The engine now draws a two-ring mark (red core, white ring) at the box's
geometric centre — **which is exactly the coordinate it returns**. That turns
「is the coordinate right」 from a four-edge alignment judgement into 「is this
point on the target」, which is both easier and the question that matters.

Two colours rather than one because a single-colour dot disappears on a
background of the same colour. The pair is deliberately generic — it has no
relationship to any particular application's palette.

### `notes` — four fixed sections, passed through as the retry hint

```
图像：清晰 / 模糊 / 目标太小 / 被遮挡
中心：标记落在目标的哪个位置（正中 / 偏左半格 / 落在格子外）
大小：框大约是目标的几倍（约 1.0 倍 / 约 1.8 倍，框进左右两个邻格）
位置：描述说的位置 vs 图里读到的位置（描述第四行第八列，图里 C 行第三行，差一行）
```

「不涉及」 means **the check did not happen** — the description names no position,
or the target has no outline to measure. It is not a synonym for 「passed」; a
check that passed may carry a short description instead.

The three verdicts say *whether* each check passed. `notes` says *what was seen*,
and it reaches the locator verbatim — the model that has to fix the box reads the
verifier's own words rather than a translation of them.

### The verifier's crop had to grow

`VERIFY_MULT` went 2.0 → 4.0. The locator's side bottoms out at `CROP_MIN`
(240 px), so 2.0 gave the verifier only 480 px — while the sudoku board spans
370 px plus ~30 px of row labels. A window that follows a slightly-off box crops
the labels away, the verifier correctly reports 「no row labels in view」, and
`position_matches` can never be answered.

At 4.0 the window is 960 px and the labels are in frame with room for the box to
drift a cell. `VERIFY_OUT_PX` (1920) was added at the same time and kept separate
from `out_px`: the two crops are asked different questions, and sharing one
number forced a trade that satisfied neither.

### What this fixed

- **All five v5 stable failures now pass** (`r2c6`, `r3c7`, `r5c4`, `r7c5`, `r7c7`).
- **Wrong cells went to zero** in the complete run (v5: 2, single 480 crop: 5).
- All five failures are now `ok = false` after three rounds — a visible refusal,
  not a wrong coordinate.

### What is still open

- Five cases exhaust three rounds without converging (`r0c2`, `r2c2`, `r3c3`,
  `r5c5`, `r5c8`). At least one of them (`r2c2`) shows the model writing only the
  first line of `notes`, so the hint carries no diagnosis.
- Round distribution: 13 cases need one round, 57 need two, 11 need three. The
  two-round majority is the cost of the position check — the first round often
  lands a cell off and only the second reads the labels.
- `POINTER_LOCATE_TIMEOUT_SECS` must be ≥ 400 at concurrency 8. At 180 s nine
  cases were reported as timeouts that actually finish in 18–91 s; they were
  queueing, not stuck.

## Superseded

- **v1** (69.1 %) ran on the same dataset before the four locate fixes above.
- **Pre-v1** (46.9 %, then 54.3 % with letter row labels) ran on a dataset whose
  descriptions claimed 37 filled cells were empty, and whose anchor directions
  were inverted. Those numbers measured the dataset's errors, not the grounder's.
