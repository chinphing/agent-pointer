### Scenario: scroll

Judge whether the viewport content moved and **how far**. The runtime measures
nothing — your read of the two screenshots is the only signal the planner gets
for tuning the next notch count.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### How far did it move

Report the shift along **whichever axis moved**, as a fraction of the target
area's extent on that axis. The target area is the panel or list the scroll was
meant to move, as named in the operation text.

Vertical movement:

```
moved ~0.4 target-heights
```

Horizontal movement:

```
moved ~0.4 target-widths
```

Both axes moved:

```
moved ~0.4 target-heights, ~0.2 target-widths
```

When the operation names no target panel (the scroll ran at the current pointer),
use the viewport extent instead and write `moved ~0.4 screen` for vertical,
`moved ~0.4 screen-widths` for horizontal.

Judge the number however it is easiest — how many rows or columns went past, or
what fraction of that area the content slid. One decimal place is plenty; the
planner only needs the magnitude to scale the next notch count. Use `moved 0`
when the content did not shift at all.

**When the before anchor is gone from the after frame, do not measure it.** That
means the jump was bigger than the panel and the rows in between were never
visible — a distance you cannot count. Do not invent one:

- in `step_summary`, write `jumped past a full panel — rows between the two
  frames were never visible`. Never `moved ~1.5 target-heights`: there is no
  anchor left to measure against, and the number reads as a measurement to the
  planner.
- set `scroll_past_panel: true` and **leave `scroll_moved` out**. The flag is the
  whole answer — the host does not need a number, and a number here would be
  read as a measurement.

The host answers the flag with a fixed rule — scroll back to read the gap, then
halve the notch count. A guessed distance would scale the next scroll by the
same guess, which is how a 3-panel jump gets a 1.5-panel correction.

The summary follows core.md's shape, with the shift appended:

```
<panel>: expected new rows below the last visible one — before last row "P", after last row "T", moved ~0.6 target-heights
```

**The expectation is 「new content appears」, never a distance.** How far one
notch carries is the app's business, not something you can predict — writing
`expected ~0.6 target-heights` turns an unpredictable number into the yardstick
and fails a scroll that did its job. Expect the view to move on to rows that
were not visible; report how far it actually went as the measurement.

**Also submit the number as fields**, not only in prose: `scroll_moved` (a
number, e.g. `0.05` — omitted when `scroll_past_panel` is true) and
`scroll_unit` (`target-heights` / `target-widths` / `screen` / `screen-widths` —
the same words you wrote in `step_summary`). The host multiplies those two by the
notch count that produced the move and prints the next notch count for the
planner; prose is not parsed. Omit both fields for every non-scroll operation.

Do not try to count pixels — the estimate is for scaling, not for measurement.

### Outcome

The single test for success is **whether the two screenshots differ because the
content scrolled**. Everything else follows from that.

**One anchor does both jobs: the row at the edge the scroll was heading
toward.** Last **fully visible** row for down, first for up, rightmost column
for right, leftmost for left. Read that row in both frames and it answers
everything:

| Ask | How the anchor answers it |
| --- | --- |
| how far did the content move | count the rows between the before anchor and where it sits in the after frame |
| did new content appear | the after anchor was not visible in before |
| did the jump exceed a panel | the before anchor is gone from the after frame — too far to count |

「Fully visible」 is the point: the leading edge is exactly where the viewport
clips, so the half-drawn row is easy to misread and easy to confuse with its
neighbour. Take the last row you can read in full — the count is the same,
because every row in the panel moves by the same amount.

Same anchor row in both frames means nothing moved. Not-new anchor means the
content did not advance — either it stayed put or it went the **wrong
direction**, which is `fail` / `wrong_operation` however much the rest of the
panel moved.

| Situation | `action_result` | `failure_cause` |
| --- | --- | --- |
| Content offset changed — by any amount, including a short move that ran into an edge | `pass` | — |
| No movement, loading visible | `pass` with `loading_detected: true` | — |
| **No movement at all, at the edge** | **`fail`** | **`at_edge`** |
| No movement at all, not at the edge | `fail` | `wrong_operation` |
| No movement in the target panel, but an unrelated panel moved | `fail` | `wrong_operation` |

### Telling `at_edge` from `wrong_operation`

This check runs **only when nothing moved at all** — `moved 0`. Look for
evidence the content is already at the edge **in the direction the scroll was
going**: scrolling down can only stop at the bottom, up at the top, right at the
right-hand end, left at the left-hand end. Any one row is enough.

| Evidence | at the edge? |
| --- | --- |
| A footer, pagination bar, "load more", "no more results", "已显示全部", or an end-of-list marker is visible and stays put | yes — bottom |
| The first item of the list is visible at the top and the rows below it are unchanged | yes — top |
| A page header, site nav, or title block sits at the viewport top with nothing above it, and did not move | yes — top |
| The first column is visible at the left edge with nothing cut off to its left | yes — left |
| The last column is visible at the right edge with nothing cut off to its right | yes — right |
| The same rows (or columns) are visible before and after, and nothing beyond them on that axis changed | yes — either direction |
| A scrollbar is visible and its thumb sits at the far end the scroll was heading toward | yes — either axis |
| Content clearly continues past the viewport in the scroll direction — a cut-off row at the bottom when scrolling down, a cut-off column at the right when scrolling right, and the mirror for the opposite directions | no |
| A spinner, skeleton, or progress bar is visible | no — report `loading_detected` instead |
| No movement and no way to tell | no |

Do **not** rely on the scrollbar alone: macOS hides it when idle, so its absence
proves nothing.

A scroll that moved 10% of the target area and then stopped at the top is a
`pass`, not `at_edge`: content moved, and `moved ~0.1 target-heights` already
tells the planner the rest. `at_edge` is reserved for a scroll that produced
**no** displacement.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. The leading-edge row in before, then in after — how far it moved, as a viewport fraction.
2. Pass or fail + `failure_cause`.
3. On `fail`, the edge evidence you used.

Optional: `loading_detected` when nothing moved but a spinner is visible.

`step_summary` always appends the shift — the planner scales the next notch count
from that number, so it goes in even when nothing moved (`moved 0`).

**Forbidden:** numbered CoT templates; JSON.
