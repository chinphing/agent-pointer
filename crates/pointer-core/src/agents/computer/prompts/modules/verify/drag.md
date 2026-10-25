### Scenario: drag

Judge whether drag-and-drop produced the expected outcome.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — did the drop achieve `goal`?

Read the dragged item and the drop area in both frames. Only a **difference
between the two** counts as a drop — an item that was already in the target
position did not move because of this drag.

**Where the item landed is part of `goal`.** The item in the neighbouring folder,
or the adjacent time slot, did not achieve `goal`.

- **Yes, the item moved to where `goal` says** → `pass`. Done.
- **No** → go to step 2 and find out why.

### Step 2 — why not?

Read the pointer in the after frame — it sits where the drag ended.

- **The end point is just outside the drop area, or the item landed beside it** —
  the neighbouring folder, the adjacent slot → `fail`, `precision_miss`.
- **The end point is unrelated, or a different item moved** → `fail`,
  `wrong_operation`.
- **Nothing changed anywhere** → `wrong_operation`.

**`loading_detected`:** file transfer progress or spinner visible.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. The item and the drop area in before, then in after — what differs.
2. Pass or fail + `failure_cause`.
3. Optional: loading.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
