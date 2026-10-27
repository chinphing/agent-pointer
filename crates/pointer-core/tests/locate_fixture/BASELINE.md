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
POINTER_LOCATE_TRUTH=truth_sudoku.json POINTER_LOCATE_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test locate_fixture frozen_desktop_grounding \
  -- --ignored --nocapture
```

### Always `--release`

**Every timing number in this file before v9 was measured on a debug build, and
debug inflates the local rendering cost by ~30×.**

Measured on the same 5 cases, same machine, same model:

| stage | debug | release | ratio |
| --- | --- | --- | --- |
| LLM call (median) | 5.31 s | 4.20 s | 1.26× |
| **render (median)** | **5.082 s** | **0.172 s** | **29.5×** |
| case total (r2c2, 3 rounds) | 36.8 s | 17.6 s | 2.1× |
| case total (r8c8, 2 rounds) | 34.7 s | 12.4 s | 2.8× |

The render step (`render_crop`) is `Lanczos3` resize to 1600²/1920², a
per-pixel `dim_outside` pass, and a JPEG encode at quality 85. In debug that is
5 s per round; in release 0.17 s. **On a debug build rendering looks like 25 % of
the wall clock — in release it is ~4 %, and the LLM call is ~96 %.**

So: benchmark with `--release`, and do not optimise the renderer based on a debug
measurement. Run-to-run stability is also much better in release (render spread
0.001–0.211 s vs 0.002–5.546 s in debug).

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

## v9 — the verifier judges edges, and the locator goes back to the crop

**Result: 78/81 (96.3 %) · median 1.4 px · mean 2.1 px · 0 wrong cells · 91.2 s/case (debug build)**
(concurrency 8, per-case timeout 400 s, 81/81 completed with no timeouts)

> **The 91.2 s is a debug-build number.** Measured on 5 cases, release is 2.1–3.7×
> faster per case because the renderer alone drops from 5.08 s to 0.17 s per round.
> The hit rate and offsets are unaffected by the build profile; only the timings are.

| | v5 | v6 | **v9** |
|---|---|---|---|
| hit | 76/81 = 93.8 % | 76/81 = 93.8 % | **78/81 = 96.3 %** |
| median centre offset | 2.3 px | 2.6 px | **1.4 px** |
| mean centre offset | 4.4 px | 5.0 px | **2.1 px** |
| `ok=true` but wrong cell | 2 | 0 | **0** |
| rounds 1 / 2 / 3 | — | 13 / 57 / 11 | **2 / 65 / 14** |
| mean per case | 35.4 s | 86.7 s | **91.2 s** |

The median dropped 39 % and the mean 57 % against v6, with no silent failures.

### What changed

**Prompts moved out of Rust into `locate/prompts/`** — eight files
(`locate.md`, `locate_near.md`, `locate_hint.md`, `locate_schema.json`,
`verify_body.md`, `verify_intro_wide.md`, `verify_intro_close.md`,
`report_schema.json`). `prompt.rs` went from 806 to ~620 lines and now only loads,
substitutes and parses.

**Every judge is a boundary comparison, never an estimate.** v6 asked the model
to estimate ratios (`≤ 1.5×`); it answered 1.32× for a box that was 1.32× and
1.0× for one that was 0.76×. v9 asks what can be seen directly:

| check | v6 asked | v9 asks |
|---|---|---|
| `center` | is the mark on the target? | how far is the mark from **the element's own four edges** — explicitly not the box's |
| `size` | is the box ≤ 1.5× the target? | are **each of the four box edges** on the same line as the matching element edge |
| `position` | does the named position match? | do **both** the position id and the anchor point at the box |

**`unclear` removed from the schema.** Across 160 verifier calls in the v6 run it
appeared once, and that once was the parse-failure fallback, not a model answer.
The model now has `yes` / `no` and must commit.

**`stay_local` judged on `center`, not on the verdict as a whole.** The engine
used to re-locate on the full screen whenever `center_on_target != Yes`. A
`center = no` means the mark is off the target — the target is right there, the
box is merely misplaced — so the crop is exactly where to look again. At 240 px
a 41 px cell lands at 328 px in the image; on the full screen it is 41 px, and
41 vs 56 cannot be told apart. `r2c2` spent three rounds on the full screen
producing 56 → 60 → 62 px boxes for a 41 px cell.

**The size remark now reports all four edges, then the fix.** It used to report
only the worst edge, so each round repaired one edge: `r2c2` fixed its right edge
in round 1, its bottom edge in round 2, and ran out of rounds before the left
edge. Reporting all four lets one round repair all of them — `r2c2` now converges
in two rounds to within 1 px on every edge.

### Where the time goes

Single case (`r2c2`), 3 rounds, 45.9 s total, from provider-level timestamps:

| stage | time |
|---|---|
| verify round 1 | 14.66 s |
| locate round 2 | 1.47 s |
| render (crop + JPEG encode) | 5.02 s |
| verify round 2 | 8.82 s |
| locate round 3 | 1.67 s |
| render (crop + JPEG encode) | 5.01 s |
| verify round 3 | 4.19 s |

`verify` is 60 % of the wall clock, rendering 22 %, `locate` 7 %. Rendering
produces **two** images per round (close 1600×1600 + wide 1920×1920) since v6;
v5 rendered one. The 2.4× pixel increase is a candidate for the v5 → v9 slowdown
(35.4 s → 91.2 s), but the two numbers were measured under different conditions
(v5's figure mixes 10-way concurrency with serial re-runs) and **this has not
been verified by a controlled experiment**.

### What is still open

- Three cases exhaust three rounds: `r2c4`, `r3c7`, `r8c4`. All three pass when
  run alone at concurrency 1 — the failures are concurrency artefacts, not
  prompt defects. The same pattern held for v8 (9 failures in the full run,
  6 of them passing alone).
- `out_of_view` was proposed as a fourth `center` value (the target is not in
  either image) and then dropped: no case in any fixture can trigger it, because
  every `hint_box` sits within 40 px of its target while the wide crop reaches
  480 px. It would be untestable dead code.
- The first round never calls `locate` when the caller supplies a `hint_box` —
  the hint becomes the round-1 box verbatim. A 1.37× hint therefore guarantees a
  `size = no` first round. Whether spending a locate call to clean the hint up
  front pays for itself is unmeasured.

## v13 — the verifier stops rewriting the description, and the locator gets nine cells

**Result: 239/243 = 98.4 % over three full runs · median 1.3 px · 0 wrong cells
in run 1 (81/81) · 2 silent failures total · mean 1.80 rounds · 12–15 s/case
(release, concurrency 8)**

| run | landed inside target | silent failures | median | mean | max | rounds | per case |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **81/81 = 100.0 %** | **0** | 1.5 px | 4.1 | 22.4 | 24/56/1 | 12 s |
| 2 | 78/81 = 96.3 % | 1 (r6c6) | 1.3 px | 3.9 | 44.0 | 17/58/6 | 15 s |
| 3 | 80/81 = 98.8 % | 1 (r5c4) | 1.2 px | 3.8 | 29.2 | 18/60/3 | 15 s |
| **all** | **239/243 = 98.4 %** | **2** | — | — | — | 1.80 | — |

Accuracy is decided by **where the box centre lands** — the truth `bbox` contains
it or it does not. `ok` is not consulted: a run that reports success with the
centre outside the truth box is a silent failure, and one that refuses while its
box landed on the target still counts.

### The eight changes, in order of measured effect

| # | change | evidence |
| --- | --- | --- |
| 1 | **The verifier may not rewrite the description.** `remark` must quote the description verbatim; when evidence and conclusion disagree, `ok` changes, not the evidence. | Broke the self-justification chain. r6c4 went 5/10 → 9/10 on its own. |
| 2 | **Nine-cell wording for "which cell is the target inside the box"** — 左上 / 正上 / 右上 / 正左 / 正中 / 正右 / 左下 / 正下 / 右下, replacing 「框的左邻格」. | 4-case suite 32/40 → 35/40. r8c6 median 21 px → 1 px. |
| 3 | **`position = no` means re-locate absolutely, never slide the reference box.** | r8c6 4/10 → 7/10. Sliding inherited the verifier's own reading error. |
| 4 | **Anchors confirm the position id; they never overrule it.** "描述写错了 / 按锚点判定为同一目标" is forbidden. | r8c6 4/10 → 6/10. |
| 5 | **Horizontal and vertical must be reported separately**, each with its own ratio and offending edge. | r4c3 4/5 → 5/5. |
| 6 | **The previous box's four edges are passed to the locator** (`prev_box`, converted to the crop's 0–1000). Previously only the centre point was passed. | r8c6 0/3 → 2/3. |
| 7 | **Counting range is the target region's interior** — the header row and index column are labels, not row 1. | Killed the 「C 行 = 第 4 行」 error on r3c7. |
| 8 | **Centre mark radius 0.010 → 0.004, white rim 0.35 → 0.25.** At 0.010 the mark was 2.4 px on screen and sat on the very grid line it was being judged against, hiding the evidence. | r7c2 went from 3/3 failures to passing. |

### What the failures actually were

Every silent failure in this round was **the box one cell off with correct size**
(0.92–1.12×), never a size error and never a blank region. The chain was always
the same:

1. The verifier read the reference box as cell *X* while it was cell *X±1*.
2. To keep `ok = yes` consistent with `remark`, it rewrote the description to
   match — or invented 「按锚点判定为同一目标」 to dissolve the contradiction.
3. The locator, told 「差一格往左」, slid the box one cell further.

Change 1 cuts step 2, change 3 cuts step 3, change 2 removes the ambiguous
wording that fed step 1.

### Two changes that made it worse and were reverted

- **Direction hints in `position.remark`** ("差一格，目标在下方") *plus* an
  instruction to verify them first: 4-case suite 30/40 → 27/40, and every
  failure became a one-cell slide. The locator has no independent way to check
  the verifier — they share the same label-reading ability — so "confirm before
  acting" was unenforceable, while the direction itself was acted on.
- **Asking the locator to confirm corrections** was removed for the same reason:
  an unenforceable instruction only adds prompt weight.

### Still open

- **r6c6 and r5c4** — first appearance as silent failures (44 px and 29 px).
  Not yet diagnosed.
- **Concurrency depresses quality.** r7c4 passes 10/10 serially but failed in a
  full concurrent run. The cause is still unidentified; WARN count is low and
  there are no timeouts, so it is not call failure.
- **`VERIFY_OUT_PX` 1920 → 960** untried: the wide image exists only to read row
  and column labels, and at 960 the cell is still 68 px.
- **Full 81 × 3 has not been re-run since the `found` field landed.** v13's
  98.4 % predates it.

## v14 — the locator can say "not in this image"

`locate` used to be unable to express absence: `bbox` was required, so a crop
that did not contain the target forced the model to invent one. That is how
`frozen_desktop_far_hint_stays_honest` produced confidently wrong boxes — the
crop held only wallpaper and the model had to answer anyway.

The tool now takes `found` (boolean, the only required field) with `bbox`
required only when `found=true`. `LocateAnswer::{Found, NotFound}` replaces the
bare box; an explicit `found: false` wins even when a stray `bbox` is present,
and it is returned without retry — retrying a "not found" only invites
invention. The engine turns a crop-local `NotFound` into `widen_needed`, which
forces the full-screen re-render.

Effect on the two tests that were never passing:

| test | V6 | v13 | **v14** |
|---|---|---|---|
| `absent_targets_fail_honestly` | 1 hallucination | 2 | **0–1** |
| `frozen_desktop_far_hint_stays_honest` | rescued 17, honest 5, **dangerous 3** | rescued 6, honest 15, dangerous 4 | **rescued 19, honest 1–2, dangerous 4–5** |

`absent` reached 0 hallucinations once and 1 on a rerun; `far_hint` rescued 19
both times. Single-run variance on 25 cases is ±1–2, so these are not yet
distinguishable from noise — but `far_hint` recovered from 6 to 19, above the
V6 figure, and `found: false` fires 20 times per run (it looked like zero until
the test was given a logger; it never had one).

`dangerous` did not improve. Three of the four are 「空白壁纸」 targets — regions
with no visual signature, where any blank patch satisfies the description.

### Still open

- **r6c6 and r5c4** — first appearance as silent failures (44 px and 29 px).
  Not yet diagnosed.
- **Concurrency depresses quality.** r7c4 passes 10/10 serially but failed in a
  full concurrent run. The cause is still unidentified; WARN count is low and
  there are no timeouts, so it is not call failure.
- **`VERIFY_OUT_PX` 1920 → 960** untried: the wide image exists only to read row
  and column labels, and at 960 the cell is still 68 px.
- **Full 81 × 3 has not been re-run since the `found` field landed.** v13's
  98.4 % predates it.
- **`frozen_desktop_far_hint_stays_honest` still fails** on `dangerous` boxes.
  The remaining ones are blank-wallpaper targets; `found: false` does not help
  because the model genuinely believes a blank patch is the element.

## Superseded

- **v1** (69.1 %) ran on the same dataset before the four locate fixes above.
- **Pre-v1** (46.9 %, then 54.3 % with letter row labels) ran on a dataset whose
  descriptions claimed 37 filled cells were empty, and whose anchor directions
  were inverted. Those numbers measured the dataset's errors, not the grounder's.

## ScreenSpot-Pro — the external benchmark

The sudoku fixture is one screen with 81 near-identical cells: it measures
position precision but says nothing about real interfaces. `tests/screenspot_pro.rs`
runs the same engine on ScreenSpot-Pro (1581 GUI-grounding cases across 26
professional applications, 4K screenshots), reusing the split definitions,
descriptions and scoring rule that `benchmarks/screenspot_grid/` established for
the Python baselines.

### Result

| split | cases | hit (point in gt box) | silent failures | offset median | rounds | per case |
|---|---|---|---|---|---|---|
| M1 | 20 | 16 (80.0 %) | 2 | 4.5 px | 1.55 | 13.3 s |
| **M2** | **300** | **256 (85.3 %)** | **32 (10.7 %)** | **4.3 px** | **1.37** | **11.7 s** |

Against the Python baselines on the same M2 split and the same v2 descriptions:

| baseline | hit | per case | model calls |
|---|---|---|---|
| **Rust engine** | **85.3 %** (256/300) | 11.7 s | **1.37 rounds** |
| `b3_crop_ladder` | 86 % (258/300) | 10.3 s | 2.62 |
| `b2_agent_marker` | 84 % (253/300) | 12.1 s | 2.66 |

The engine matches the best Python baseline while making roughly half the model
calls — each round is one locate plus one verify, so 1.37 rounds ≈ 2.7 calls
against their 2.6 *plus* their own locate pass.

### The gap is silent failure, not precision

When it hits, it hits accurately: median offset 4.3 px. The loss is elsewhere —
32 of 44 failures claimed `ok=true` and put the point outside the box. On sudoku
the same ratio is 2 of 243 (0.8 %); here it is 10.7 %, an order of magnitude
worse. ScreenSpot-Pro targets are 15–30 px on a 4K canvas, and the verifier is
easier to fool with a look-alike neighbour. That is the next thing to work on.

### Two traps this benchmark sets

- **Thinking must be off.** Leaving the provider default (`on` for
  qwen3.8-flash) makes every call emit ~3000 reasoning tokens first: measured
  25–53 s per call instead of 3–5 s, a 12× slowdown with no accuracy gain.
  `thinking_setting()` in the test forces it off, matching the sudoku runs.
  With it on, three cases took 122.8 s each; with it off, 10.0 s.
- **`splits/M2.json` is stale.** It holds 100 ids that do not intersect
  `descriptions/v2/M2.json` at all, so scoring from the split file alone yields
  zero runnable cases. `descriptions/v2/M2.json` has 300 keys — all in the
  annotations, and that is what `META.json` records as M2 and what the Python
  harness effectively scores (`ids ∩ descriptions`). The description file
  defines the split; see `load_split_ids`.

### Command

```bash
# M2 (300 cases, ~6 min at concurrency 8)
POINTER_SP_SPLIT=M2 POINTER_LOCATE_CONCURRENCY=8 RUST_LOG=info \
cargo test --release -p pointer-core --test screenspot_pro screenspot_pro \
  -- --ignored --nocapture
```

`POINTER_SP_IDS` overrides the split, `POINTER_SP_LIMIT` truncates,
`POINTER_SP_THINKING=1` turns thinking back on.
