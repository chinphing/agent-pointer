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

Only reach this section when the content did **not** shift at all — no row moved,
no offset changed, `moved 0 screen`. If anything moved, the scroll succeeded and
you are done with this section; report `pass` and the fraction from step 1, even
when the movement was small because the content hit an edge mid-scroll.

When nothing moved, look for evidence that the content is already at the edge
**in the direction the scroll was going**. Any one of these is enough:

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

The single test for success is **whether the two screenshots differ because the
content scrolled**. Everything else follows from that.

| Situation | `action_result` | `failure_cause` |
| --- | --- | --- |
| Content offset changed — by any amount, including a short move that ran into an edge | `pass` | — |
| No movement, loading visible | `pass` with `loading_detected: true` | — |
| **No movement at all, at the edge** | **`fail`** | **`at_edge`** |
| No movement at all, not at the edge | `fail` | `wrong_operation` |
| No movement in the target panel, but an unrelated panel moved | `fail` | `wrong_operation` |

`at_edge` describes a scroll that **produced no displacement because the
viewport cannot move further** — not a scroll that reached the end while moving.
A scroll that moved 10% of a screen and then stopped at the top is a `pass`:
content moved, the goal was met as far as the content allows, and `moved ~0.1
screen` already tells the planner the rest.

Reporting `wrong_operation` for a no-movement-at-edge sends the planner off to
change anchors and notch counts for no reason; it will read `at_edge` and stop
instead. Because `at_edge` is still a `fail`, repeated scrolls at an exhausted
edge keep counting toward the give-up threshold — that is intended.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. Estimated shift as a viewport fraction, then what changed.
2. Pass or fail + `failure_cause`; when unchanged, name the edge evidence you used.
3. Optional: `loading_detected` when unchanged but a spinner is visible.

**Forbidden:** pointer-position analysis; numbered CoT templates; JSON.
