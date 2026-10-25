# locate baseline — sudoku, 81 cells

Recorded 2026-10-25. Re-run and compare before changing the locate prompt,
`LocateConfig`, or the hint pipeline.

## Command

```bash
POINTER_LOCATE_TRUTH=truth_sudoku.json POINTER_LOCATE_CONCURRENCY=10 \
  cargo test -p pointer-core --test locate_fixture frozen_desktop_grounding \
  -- --ignored --nocapture
```

## Result

| | |
|---|---|
| hit | **38 / 81 = 46.9 %** |
| wall clock | **270.5 s** (10 concurrent) |
| completed | 71 |
| timed out (>60 s per case) | 10 |
| mean per completed case | 27.0 s |
| rounds | 1: 39 cases, 2: 30 cases, 3: 2 cases |

The 70 % bar in the test asserts a full run; this baseline sits well below it.

## Where it fails

**Every completed case returned `ok=true`.** Of the 71 that finished, 33 (46 %)
put the box on a *different* cell while the grounder reported success. The
grounder's own acceptance check does not distinguish neighbouring empty cells on
a sudoku board — it sees "an empty cell matching the description" and stops.

Misses by hint size (hint box side / cell side):

| hint ratio | hit |
|---|---|
| 0.5–1.0× | 7/11 = 63.6 % |
| 1.0–1.5× | 8/14 = 57.1 % |
| 1.5–2.0× | 12/17 = 70.6 % |
| 2.0–3.0× | 11/29 = 37.9 % |

A hint three times the target is worse than one half its size. When the box
swallows neighbours, the model has several valid-looking answers inside it and
picks one.

Misses by description shape:

| shape | hit |
|---|---|
| D plain (`左上角的格子`) | 3/3 = 100 % |
| B edge (`最左边的空格子，位于…`) | 5/7 = 71.4 % |
| A between (`位于数字9和数字6之间`) | 11/20 = 55.0 % |
| C direction (`位于数字5的右边，上方是数字7`) | 19/41 = 46.3 % |

Relative-anchor descriptions are the weakest. `C direction` is the most common
shape and the worst performer — "right of the 5" is ambiguous when the 5 sits
two cells away.

Misses by rounds — **retries make it worse, not better**:

| rounds | hit |
|---|---|
| 1 | 30/39 = 76.9 % |
| 2 | 8/30 = 26.7 % |
| 3 | 0/2 = 0 % |

The retry path gives the model its previous (wrong) box plus a complaint. On a
board of identical cells it relocates to another equally plausible cell rather
than correcting.

Miss offsets cluster at **22–36 px** — half a cell to a full cell. These are not
near-misses from imprecise edges; the box is centred on the wrong cell.

## Timeouts

Ten cases hit the 60 s per-case cap:

```
r2c2 r2c4 r3c4 r3c5 r3c8 r4c8 r6c0 r6c7 r7c1 r7c2
```

Nine of the ten carry a hint of 1.18–2.92×; one (`r7c2`, 0.74×) is below the
cell. Concurrency of 10 keeps requests queued behind each other, so part of the
timeout is scheduling rather than the model looping. Re-run serially before
concluding these cases are genuinely hard.

## What this baseline does and does not measure

It measures the grounder under **deliberately hostile** conditions: 81
near-identical targets, descriptions built only from relative position, and hint
boxes from 0.5× to 3× the cell.

Production `target_desc` carries more distinguishing detail, and the caller's
hint is usually closer. So 46.9 % is not a general accuracy figure — it is the
floor for "the board is full of look-alikes and the description relies on
neighbouring numbers".

The useful signals here are the *relative* ones: hint size hurts above 2×,
relative-anchor descriptions underperform, and retries are counterproductive.
Those hold regardless of absolute level.
