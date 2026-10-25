### Scenario: drag

Judge whether drag-and-drop produced the expected outcome.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — what moved, and where did it land?

Read the dragged item and the drop area in both frames. Only a **difference
between the two** counts as a drop — an item that was already in the target
position did not move because of this drag.

### Step 2 — is that the outcome `goal` asked for?

| Difference | `action_result` | `failure_cause` |
| --- | --- | --- |
| The item moved, reordered, or a transfer started at the drop zone | `pass` | — |
| A different item moved, or the right item landed outside the drop zone | `fail` | `wrong_operation` |
| Nothing changed anywhere | `fail` | `precision_miss` |

**`loading_detected`:** file transfer progress or spinner visible.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. The item and the drop area in before, then in after — what differs.
2. Pass or fail + `failure_cause`.
3. Optional: loading.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
