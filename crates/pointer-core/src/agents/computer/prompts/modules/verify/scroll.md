### Scenario: scroll

Judge whether the viewport content moved and **how far**. The runtime measures
nothing — your read of the two screenshots is the only signal the planner gets
for tuning the next notch count.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### How far did it move

Estimate the content shift as a fraction of the viewport height, and put it
**first** in `step_summary`:

```
moved ~0.35 screen
```

Pick the number however it is easiest to judge — how many rows went past, or
what fraction of the height the content slid. One decimal place is plenty; the
planner only needs the magnitude to scale the next notch count. Use `moved 0
screen` when the content did not shift at all.

Do not try to count pixels — the estimate is for scaling, not for measurement.

### Outcome

The single test for success is **whether the two screenshots differ because the
content scrolled**. Everything else follows from that.

| Situation | `action_result` | `failure_cause` |
| --- | --- | --- |
| Content offset changed — by any amount, including a short move that ran into an edge | `pass` | — |
| No movement, loading visible | `pass` with `loading_detected: true` | — |
| **No movement at all, at the edge** | **`fail`** | **`at_edge`** |
| No movement at all, not at the edge | `fail` | `wrong_operation` |
| No movement in the target panel, but an unrelated panel moved | `fail` | `wrong_operation` |

### Telling `at_edge` from `wrong_operation`

This check runs **only when nothing moved at all** — `moved 0 screen`. Look for
evidence the content is already at the edge in the direction the scroll was
going. Any one row is enough:

| Evidence | at the edge? |
| --- | --- |
| A footer, pagination bar, "load more", "no more results", "已显示全部", or an end-of-list marker is visible and stays put | yes |
| The same rows are visible before and after, and nothing below them changed | yes |
| A scrollbar is visible and its thumb sits at the far end | yes |
| Content clearly continues past the viewport — a cut-off row at the bottom, more cards below | no |
| A spinner, skeleton, or progress bar is visible | no — report `loading_detected` instead |
| No movement and no way to tell | no |

Do **not** rely on the scrollbar alone: macOS hides it when idle, so its absence
proves nothing.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. Estimated shift as a viewport fraction, then what changed.
2. Pass or fail + `failure_cause`; when nothing moved, name the edge evidence you used.
3. Optional: `loading_detected` when nothing moved but a spinner is visible.

**Forbidden:** pointer-position analysis; numbered CoT templates; JSON.
