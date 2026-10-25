# locate baseline v1 — sudoku, 81 cells

Recorded 2026-10-25. Re-run and compare before changing the locate prompt,
`LocateConfig`, or the hint pipeline.

## Dataset

`truth/truth_sudoku.json`, 81 cases over `fixtures/sudoku.jpg` (1920×1080).

- **Contents are stated honestly**: 43 cases are empty cells (`空格子`), 38 hold
  a digit (`有数字N的格子`). An earlier revision called all 81 empty and produced
  meaningless failures — the grounder correctly refused cells that were not empty.
- **Two coordinate forms, split at random**: 40 `C3`-style (`coord_style:
  letter`), 41 `第三行第三列`-style (`coord_style: ordinal`). The board prints
  `A`–`I` down the side and `1`–`9` across the top, so both are readable; the
  split exists to compare them.
- **Anchors carry distance**: `位于数字9的左边`, `…隔一格`, `…隔二格`.
- **Hint boxes are deliberately off**: anchor inside the cell, side 0.5–3× the
  cell side.
- **`goal`** is `选中这一格准备填数` for empty cells, `选中这一格` for filled ones,
  mirroring what the model actually writes.

## Command

```bash
POINTER_LOCATE_TRUTH=truth_sudoku.json POINTER_LOCATE_CONCURRENCY=10 \
  cargo test -p pointer-core --test locate_fixture frozen_desktop_grounding \
  -- --ignored --nocapture
```

## Result

| | |
|---|---|
| hit | **56 / 81 = 69.1 %** |
| wall clock | **297.8 s** (10 concurrent) |
| completed | 66 |
| timed out (>60 s per case) | 15 |
| hit among completed | **56 / 66 = 84.8 %** |

The 70 % bar is barely missed, and the miss is entirely the timeouts: among the
cases that returned, 84.8 % were correct.

## Breakdown (66 completed)

**By coordinate form** — the opposite of what the previous, flawed dataset
suggested:

| `coord_style` | hit |
|---|---|
| `ordinal` (`第三行第三列`) | **31/33 = 93.9 %** |
| `letter` (`C3`) | **25/33 = 75.8 %** |

Fisher's exact test on 25/33 vs 31/33 gives p ≈ 0.045. Suggestive, not settled —
and directly contradicts the earlier run, which is why the earlier numbers are
discarded rather than reconciled. Re-run before acting on this.

**By cell content**:

| | hit |
|---|---|
| filled (`有数字7的格子`) | **29/29 = 100 %** |
| empty (`空格子`) | **27/37 = 73.0 %** |

A filled cell has a visual feature to key on. Empty cells are identical to each
other, so the anchor text is all the model has.

**By hint size**:

| hint / cell | hit |
|---|---|
| < 1× | **13/13 = 100 %** |
| 1–2× | 23/26 = 88.5 % |
| ≥ 2× | **20/27 = 74.1 %** |

Consistent with the previous run: a box that swallows neighbours hurts.

**By rounds** — retries stay counterproductive:

| rounds | hit |
|---|---|
| 1 | **35/36 = 97.2 %** |
| 2 | 18/25 = 72.0 % |
| 3 | 3/5 = 60.0 % |

**Failure shape**: `ok=false` (the grounder refused) happened **0 times**. All 10
misses were `ok=true` with the box on a different cell. The grounder's own
acceptance check still cannot tell neighbouring empty cells apart.

## Timeouts

15 cases hit the 60 s per-case cap under 10-way concurrency:

```
r1c0 r1c8 r2c2 r2c4 r3c3 r3c4 r3c5 r3c8 r4c8 r6c0 r6c5 r6c7 r6c8 r7c1 r7c2
```

**Not yet diagnosed.** An earlier serial run of 15 timeout cases from a different
dataset completed every one in ≤ 24.3 s, which suggests queueing rather than
genuine difficulty — but that has to be repeated on this dataset before it can be
claimed.

## What this baseline measures

81 near-identical targets, descriptions built only from coordinates and
neighbouring digits, hints from 0.5× to 3× the cell. It is a floor for
"look-alikes everywhere", not a general accuracy figure.

The signals that held across both datasets, and are therefore worth trusting:

- hints above 2× the target hurt;
- retries reduce accuracy rather than improving it;
- filled cells are found perfectly, empty cells are not.

The `letter` vs `ordinal` split is the one open question, and the earlier data
said the opposite. Treat it as unresolved.

## Superseded

An earlier revision of this file reported 46.9 % (then 54.3 % with letter row
labels) on a dataset whose descriptions claimed 37 filled cells were empty. Those
numbers measured the dataset's errors, not the grounder's. Kept here only so the
change is traceable.
