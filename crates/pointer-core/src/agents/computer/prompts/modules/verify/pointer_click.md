### Scenario: pointer click

Two questions in order. Stop at the first one that decides it.

**Evidence:** **[Screen before action]** vs **[Screen after action]**. Both carry
the pointer overlay, so where the click landed is visible.

### Step 1 — did the element `goal` names change?

Find that element in both screenshots — a cell, a button, a row, a menu item.

- **It changed** → go to step 2.
- **It did not change, but something else did** → `fail`, `wrong_operation`.
  The click reached a different element.
- **Nothing changed anywhere, and the click landed on that element** → `fail`,
  `wrong_operation`. The element ignored the click.
- **Nothing changed anywhere, and the click landed elsewhere** → `fail`,
  `precision_miss`. The click missed.

### Step 2 — is that change the one `goal` asked for?

- **Yes** — value entered, page opened, menu expanded, row selected → `pass`.
- **Opposite** — the state `goal` wanted was already there and the click removed
  it → `fail`, `wrong_operation`.
- **Partial or different** — highlighted but not entered, focused but not opened
  → `fail`, `wrong_operation`.

**`loading_detected`:** spinner, progress bar, or partial load in after.

**`step_summary`:** on `pass`, name where the change landed and what it was —
`row 7 col 3 now shows 1`. On `fail`, name the one fact that decided it.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.

**Pointer rule:** cursor on target alone ≠ pass; visible UI outcome required.
Exception: `mouse_move` — cursor at target = pass.
