### Scenario: pointer hover

Judge whether hover revealed the expected UI (tooltip, popup preview, menu,
highlight).

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — where is the pointer, and what did the hover reveal?

The after frame carries the pointer at its current position. Compare it against
the element `goal` names.

- **Pointer on that element** → go to step 2.
- **Pointer on the neighbouring element** — the adjacent icon, the item above or
  below → `fail`, `precision_miss`. The hover landed one element over.
- **Pointer somewhere unrelated** → `fail`, `wrong_operation`.
- **No pointer visible** → judge from where the hover UI appeared, below.

### Step 2 — is that the UI `goal` asked for?

Read the area around the pointer in both frames. Only a **difference between the
two** counts as a reveal — a tooltip already open in the before frame did not
appear because of this hover.

| Difference | `action_result` | `failure_cause` |
| --- | --- | --- |
| The expected tooltip / popup / highlight, on the element `goal` names | `pass` | — |
| A different UI appeared than the one `goal` describes | `fail` | `wrong_operation` |
| Nothing changed anywhere | `fail` | `wrong_operation` |

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. What the pointer area showed in before, then in after — what differs.
2. Pass or fail + `failure_cause`.
3. Optional: loading if applicable.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
