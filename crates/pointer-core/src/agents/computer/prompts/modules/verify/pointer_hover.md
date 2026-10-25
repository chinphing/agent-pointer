### Scenario: pointer hover

Judge whether hover revealed the expected UI (tooltip, popup preview, menu,
highlight).

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — did the hover achieve `goal`?

Read the area around the pointer in both frames. Only a **difference between the
two** counts as a reveal — a tooltip already open in the before frame did not
appear because of this hover.

**Which element revealed the UI is part of `goal`.** The expected tooltip on the
neighbouring icon did not achieve `goal`.

- **Yes, the UI `goal` describes, on the element `goal` says** → `pass`. Done.
- **No** → go to step 2 and find out why.

### Step 2 — why not?

Read the pointer in the after frame — it sits where the hover was aimed.

- **The pointer is on the neighbouring element, or that element reacted** —
  the adjacent icon, the item above or below → `fail`, `precision_miss`.
- **The pointer is on an unrelated element, or a different UI appeared** →
  `fail`, `wrong_operation`.
- **Nothing changed anywhere** → `wrong_operation`.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. What the pointer area showed in before, then in after — what differs.
2. Pass or fail + `failure_cause`.
3. Optional: loading if applicable.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
