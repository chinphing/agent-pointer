### Scenario: drag

Judge whether drag-and-drop produced the expected outcome.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — where did the drag end?

The before frame carries the pointer where the drag started; the after frame
carries it where it ended. Compare the end point against the drop area `goal`
names.

- **End point inside the drop area** → go to step 2.
- **End point just outside it** — the neighbouring folder, the adjacent time
  slot → `fail`, `precision_miss`. The drop missed; the planner re-aims.
- **End point somewhere unrelated** → `fail`, `wrong_operation`.
- **No pointer visible in the after frame** → judge from where the change
  appeared, below.

### Step 2 — did the drop produce the expected outcome?

Read the dragged item and the drop area in both frames. Only a **difference
between the two** counts as a drop — an item that was already in the target
position did not move because of this drag.

| Difference | `action_result` | `failure_cause` |
| --- | --- | --- |
| The item moved, reordered, or a transfer started at the drop zone | `pass` | — |
| A different item moved | `fail` | `wrong_operation` |
| Nothing changed anywhere | `fail` | `wrong_operation` |

**`loading_detected`:** file transfer progress or spinner visible.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. The item and the drop area in before, then in after — what differs.
2. Pass or fail + `failure_cause`.
3. Optional: loading.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
