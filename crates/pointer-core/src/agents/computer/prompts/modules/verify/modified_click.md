### Scenario: modified click

Cmd/Ctrl/Shift+click. Same two steps as a plain click, with one difference: the
expected change is a **multi-selection** or a special action (open in a new tab,
extend the selection), not a single state change.

**Evidence:** **[Screen before action]** vs **[Screen after action]**. Both carry
the pointer overlay, so where the click landed is visible.

### Step 1 — where did the click land?

Find the element `goal` names and look at the landing point.

- **On that element** → go to step 2.
- **On a different element or empty space** → `fail`, `precision_miss`.

### Step 2 — did the expected change happen?

- **The selection grew, or the special action ran** — another row joined the
  selected set, a new tab opened, the range extended → `pass`.
- **The expected effect is missing** — only that row got selected and the
  previous selection collapsed (the modifier did not register), or the action
  opened the same way a plain click would → `fail`, `wrong_operation`.
- **No change anywhere** → `fail`, `wrong_operation`.

**`loading_detected`:** UI still settling.

**`step_summary`:** on `pass`, name the landing point and the change —
`row 5 joined the selection, 3 rows now selected`. On `fail`, name the one fact
that decided it.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
