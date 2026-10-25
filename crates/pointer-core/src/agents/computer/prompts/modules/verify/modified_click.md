### Scenario: modified click

Cmd/Ctrl/Shift+click. Same two steps as a plain click, with one difference: the
expected change is a **multi-selection** or a special action (open in a new tab,
extend the selection), not a single state change.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — where did the click land?

The before frame carries the pointer at the click point, and the target `goal`
names is intact in that frame.

- **Pointer inside the target** → go to step 2.
- **Pointer outside it** — the neighbouring row, the adjacent item → `fail`,
  `precision_miss`. The click missed; the planner re-aims from this.
- **Somewhere unrelated** → `fail`, `wrong_operation`. The click reached the
  wrong element.
- **No pointer visible in the before frame** → judge from where the change
  appeared, below.

### Step 2 — did the expected change happen?

Read the selection in both frames. Only a **difference between the two** counts —
the rows already selected in the before frame were not selected by this click.

- **The selection grew, or the special action ran** — another row joined the
  selected set, a new tab opened, the range extended → `pass`.
- **The expected effect is missing** — only that row got selected and the
  previous selection collapsed (the modifier did not register), or the action
  opened the same way a plain click would → `fail`, `wrong_operation`.

**`loading_detected`:** UI still settling.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
