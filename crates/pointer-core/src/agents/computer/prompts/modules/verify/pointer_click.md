### Scenario: pointer click

Two questions in order. Stop at the first one that decides it.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — is the expected effect there?

State the effect this click should produce, then look for it in the two frames
— on the element itself, or wherever that effect would show: a toast, a dialog,
a window that is gone, a row that is now highlighted.

- **Present, where `action` says** → `pass`. Done.
- **Absent, or the only difference is a side effect** → go to step 2 and find
  out why.

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

1. The expected effect, in one clause.
2. Whether the after frame shows that effect — or what it shows instead.
3. Pass or fail + `failure_cause` when fail.

`step_summary` leads with that expected effect, then what the frames show for it.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.
