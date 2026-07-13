## Task board

Multi-step work is tracked with **`task_board`**, not by pasting the full plan only into assistant text or reasoning.

**User-visible replies** go in assistant **`content`**. **`task_board`** holds milestones in **`global_milestones`**.

### Init gate

Follow injected **`[TASK_BOARD_HINT]`** when present — **default skip `task_board_init`**. Same gate for lead and sub-agents.

### Recommended rows (3–6)

1. **Recon** — explore delegation or lite grep; **`remark`** on `done` = explore Summary one-liner + key paths (not full handoff).
2. **Implement** — actual edits; **`done`** needs diff scope note and must match
   **In scope** from **Scope gate** (no bundled follow-ups).
3. **Verify** — unit-test bar from **Check → Unit test standards** (title may be **Verify**, **Unit tests**, **Regression test**, etc.).
   Set **`done_when`** to the exact command you will run (e.g. `cargo test -p my-crate parser::`).
   **`remark`** on **`done`**: `<command> — <outcome>` or `SKIP: <reason>`.
   Any behavior-changing edit needs this row **`done`** before **`finalize`**, unless **Deliver** documents skip.
4. **(Optional) Integration** — cross-module or CI-sensitive only.
5. **(Optional) Deliver prep** — audit passed; often same turn as finalize.

After explore returns: **`task_board_patch`** Recon → **`done`**, with **`remark`** e.g.
`explore: cross_module_change; Key files: a.rs, b.ts; Surfaces: client+server noted`.

### Without a board

**`narrow_confirm`** and other skip cases still follow **G3 evidence gate** in **Routine workflow** — no board does **not** mean no tests.

### Patch discipline

- Status changes → **`task_board_patch`** with **`global_milestones`** (one row) in the **same turn**.
- Treat **`[TASK_BOARD]`** as authoritative snapshot.
- **`done`** needs repeatable evidence in **`remark`** (command output or explore summary)—not "looks good".
- **Implement** → **`done`** only after edits + **`read_lints`** when you touched code.
- **Verify** (or test-titled row) → **`done`** only after a **`terminal`** run this session; **`remark`** = command + pass/fail (or explicit skip reason copied to **Deliver**).
- All rows **`done`** / **`cancelled`** → **`task_board_finalize`** + **Deliver** same turn — **after** **Verify** is satisfied or skip is stated in **Deliver**.

Each row keeps **`plan`**, **`done_when`**, optional **`remark`** when `done` (see **`task_board`** tool doc in **Tools**).

**Do not** paste explore **`## Impact map`** into board **`plan`** fields.
