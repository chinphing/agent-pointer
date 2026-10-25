### Scenario: pointer click

Two questions in order. Stop at the first one that decides it.

**Evidence:** **[Screen before action]** vs **[Screen after action]**. Both carry
the pointer overlay, so where the click landed is visible.

### Step 1 — where did the click land?

Find the element `goal` names — a cell, a button, a row, a menu item — and look
at the landing point.

- **On that element** → go to step 2.
- **On a different element or empty space** → `fail`, `precision_miss`. The
  click missed.

### Step 2 — did the expected change happen?

The change does not have to be on the element itself. A save button looks the
same before and after; the toast, the dialog, the new page, the row that
disappeared — that is the change. Look at the whole screen.

**When `goal` names a position** — a row and column, an ordinal, a named panel —
the change must appear **at that position**. The right kind of change in the
wrong cell, the wrong row, or the wrong panel is a `fail`.

- **Yes, the change `goal` expects, in the right place** → `pass`.
- **The right change in the wrong place** — the value landed one cell over, the
  wrong row got selected, the wrong tab opened → `fail`, `wrong_operation`.
  Say where it landed instead.
- **Some change, but not the expected one** — highlighted instead of entered,
  focused instead of opened, the wrong menu expanded → `fail`, `wrong_operation`.
- **No change anywhere** → `fail`, `wrong_operation`. The click reached the
  element and nothing happened.

**`loading_detected`:** spinner, progress bar, or partial load in after.

**`step_summary`:** on `pass`, name the landing point and the change —
`row 7 col 3 now shows 1`, `clicked Save, toast 已保存 appeared`.
On `fail`, name the one fact that decided it.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.
