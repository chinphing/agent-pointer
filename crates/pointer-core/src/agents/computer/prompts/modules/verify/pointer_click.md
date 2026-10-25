### Scenario: pointer click

Two questions in order. Stop at the first one that decides it.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — did the click achieve `goal`?

Compare the two frames. The change does not have to be on the element itself —
a save button looks the same before and after; the toast, the dialog, the new
page, the row that disappeared is the change. But it must be a **difference
between the two frames**, not a state you notice in the after frame alone.

**Where the change landed is part of `goal`.** When `goal` names a position — a
row and column, an ordinal, a named panel — the change must appear there. The
right kind of change in the wrong cell, the wrong row, or the wrong panel did
not achieve `goal`.

- **Yes, the change `goal` describes, where `goal` says** → `pass`. Done.
- **No** → go to step 2 and find out why.

### Step 2 — why not?

Read the pointer in the before frame — it sits at the click point, and the target
is intact there. That tells you which kind of miss this was.

- **The change landed on a neighbouring element, or the pointer is on one** —
  the adjacent cell, the row above or below, the button next door →
  `fail`, `precision_miss`. The aim was off by one; the planner re-aims.
- **The change landed somewhere unrelated, or the pointer is on an unrelated
  element** → `fail`, `wrong_operation`. The click reached the wrong thing.
- **Nothing changed anywhere** → pointer on the target means the element ignored
  the click (`wrong_operation`); pointer off it means the click missed
  (`precision_miss`).

**`loading_detected`:** spinner, progress bar, or partial load in after.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.
