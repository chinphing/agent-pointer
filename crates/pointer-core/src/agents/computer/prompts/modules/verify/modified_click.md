### Scenario: modified click

Cmd/Ctrl/Shift+click. Same two steps as a plain click, with one difference: the
expected change is a **multi-selection** or a special action (open in a new tab,
extend the selection), not a single state change.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — did the click achieve `goal`?

Compare the two frames. The expected change is a **multi-selection** or a special
action (open in a new tab, extend the selection). It must be a **difference
between the two frames** — the rows already selected in the before frame were not
selected by this click.

**Where the change landed is part of `goal`.** The rows that joined the selection
must be the ones `goal` names. A neighbouring row selected instead did not
achieve `goal`.

- **Yes, the change `goal` describes, on the rows `goal` says** → `pass`. Done.
- **No** → go to step 2 and find out why.

### Step 2 — why not?

Read the pointer in the before frame — it sits at the click point, and the rows
are intact there.

- **The pointer is on a neighbouring row, or the neighbouring row joined the
  selection** → `fail`, `precision_miss`. The aim was off by one.
- **The pointer is on an unrelated element, or the effect is unrelated** →
  `fail`, `wrong_operation`.
- **Only that row got selected and the previous selection collapsed** — the
  modifier did not register → `fail`, `wrong_operation`.
- **Nothing changed anywhere** → `wrong_operation`.

**`loading_detected`:** UI still settling.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
