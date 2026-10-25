### Scenario: modified click

Cmd/Ctrl/Shift+click. Same two steps as a plain click, with one difference: the
expected change is a **multi-selection** or a special action (open in a new tab,
extend the selection), not a single state change.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — where did the change happen?

Compare the two frames and find the region that differs.

- **At the element `goal` names** → go to step 2.
- **At a different element, while the one `goal` names is unchanged** → `fail`,
  `wrong_operation`. The click reached something else.
- **Nothing differs anywhere** → `fail`, `wrong_operation`.

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
