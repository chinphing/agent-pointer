### Scenario: scroll

Judge whether the viewport content moved, **how far**, and — when it did not
move — **whether that is because the content is already at the edge**.

The runtime measures nothing. Your read of the two screenshots is the only
signal the planner gets for tuning the next notch count, so the distance
estimate matters as much as the pass/fail.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### 1. How far did it move

Estimate the content shift as a fraction of the viewport height, and put it
**first** in `step_summary`:

```
moved ~0.35 screen
```

Pick the number however it is easiest to judge — how many rows went past, or
what fraction of the height the content slid. One decimal place is plenty; the
planner only needs the magnitude to scale the next notch count. Use `moved 0
screen` when the content did not shift at all, since that value decides the
edge check below.

Do not try to count pixels — the estimate is for scaling, not for measurement.

### 2. If nothing moved, why

Look for evidence that the content has reached the edge **in the direction the
scroll was going**. Any one of these is enough:

| Evidence | Verdict |
| --- | --- |
| A footer, pagination bar, "load more", "no more results", "已显示全部", or an end-of-list marker is visible and stays put | **at the edge** |
| The same rows are visible before and after, and nothing below them changed | **at the edge** |
| A scrollbar is visible and its thumb sits at the far end | **at the edge** |
| Content clearly continues past the viewport — a cut-off row at the bottom, more cards below | **not at the edge** |
| A spinner, skeleton, or progress bar is visible | **loading**, not an edge |
| No movement and no way to tell | **not at the edge** |

Do **not** rely on the scrollbar alone: macOS hides it when idle, so its absence
proves nothing.

### Outcome table

| Situation | `action_result` | `failure_cause` |
| --- | --- | --- |
| Content offset changed | `pass` | — |
| Unchanged, loading visible | `pass` with `loading_detected: true` | — |
| **Unchanged, at the edge** | **`pass`** | — |
| Unchanged, not at the edge | `fail` | `wrong_operation` |
| Wrong area scrolled — content moved in an unrelated panel | `fail` | `wrong_operation` |

A scroll that reaches the edge is a **success**, not a failure: the planner asked
to move the viewport and the viewport is as far as it goes. Reporting `fail`
here sends the planner off to change anchors and notch counts for no reason.

Because `at_edge` counts as `pass`, nothing else will stop the planner from
scrolling again. Make the conclusion explicit — `moved 0 screen; footer
已显示全部 is visible, list is exhausted` — so the next round reads it and
moves on instead of repeating the same scroll.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. Estimated shift as a viewport fraction, then what changed.
2. Pass or fail + `failure_cause`; when unchanged, name the edge evidence you used.
3. Optional: `loading_detected` when unchanged but a spinner is visible.

**Forbidden:** pointer-position analysis; numbered CoT templates; JSON.
